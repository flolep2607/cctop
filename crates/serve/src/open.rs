//! `/api/term/<provider>/<id>`: one of this machine's sessions, opened as a
//! terminal from a tab on another machine.
//!
//! The far end is `cctop mux reach` in a sibling's tab (see
//! [`cctop_core::remote_open`]), which signs the request as a machine on the
//! account. This end runs `cctop mux open` on a pty of its own and pumps it
//! through the WebSocket: that command attaches to the session's rmux, resumes
//! it into one, or explains why it cannot and is a shell in the project.
//!
//! The terminal is not an rmux share. A share is end-to-end encrypted for a
//! browser running rmux's own client, which a pane on another machine is not;
//! this is a pty's bytes in binary frames, and a resize as a text frame.
//!
//! A tab that asks for it can come back to the same terminal when its socket
//! drops, and is sent what it missed: see [`crate::resume`].
//!
//! Who may: a full link with actions on — the same line `relay_terminal`
//! draws for a tab's terminal, since typing into an agent is acting on it. A
//! sibling's signature is a full link (see the peer check in `handle`).

use crate::http::{self, Request};
use crate::{Access, Shared, current, resume};
use cctop_core::ws::{self, Op, frame};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::fd::AsRawFd;

pub(crate) fn route(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    rest: &str,
    access: Access,
) {
    if access != Access::Full || !shared.actions {
        return http::respond_error(
            stream,
            Some(request),
            403,
            "this link cannot open terminals",
        );
    }
    if !request.is_websocket() {
        return http::respond_error(
            stream,
            Some(request),
            400,
            "a terminal socket is a WebSocket",
        );
    }
    let Some((provider, id)) = rest.split_once('/') else {
        return http::respond_error(stream, Some(request), 404, NO_SESSION);
    };
    // Only a session this machine has, exactly as named: the open is for
    // *that* session, and a shell for an id nobody has would make this a way
    // to a shell for any id at all.
    let snapshot = current(shared);
    let known = cctop_core::remote_open::id_ok(id)
        && snapshot
            .sessions
            .iter()
            .any(|s| s.remote.is_none() && s.provider.as_str() == provider && s.session_id == id);
    if !known {
        return http::respond_error(stream, Some(request), 404, NO_SESSION);
    }
    open_or_resume(stream, request, || open_argv(provider, id));
}

/// `/api/new-term/<agent>/<permissions>/<model>/<dir>`: a *new* agent on this
/// machine, started in rmux and opened as a terminal from a tab on another —
/// the launcher's `machine:path` for a sibling with no ssh route.
///
/// The same line as [`route`] for who may, since starting an agent is acting;
/// every part of the request is checked by
/// [`NewAgent::from_route`](cctop_core::remote_open::NewAgent::from_route),
/// and what this machine cannot do — an agent not installed here, a directory
/// that is not one — is refused before a terminal is opened, so the reason
/// arrives as the tab's message rather than as a pty that ended.
///
/// `with_account` for `/api/new-term-as/…/<account>`
/// ([`NEW_AS_ROUTE`](cctop_core::remote_open::NEW_AS_ROUTE)): the same, under
/// one of this machine's accounts, and an account it does not have is one of
/// the things refused.
pub(crate) fn route_new(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    rest: &str,
    access: Access,
    with_account: bool,
) {
    if access != Access::Full || !shared.actions {
        return http::respond_error(stream, Some(request), 403, "this link cannot start agents");
    }
    if !request.is_websocket() {
        return http::respond_error(
            stream,
            Some(request),
            400,
            "a terminal socket is a WebSocket",
        );
    }
    let parsed = match with_account {
        true => cctop_core::remote_open::NewAgent::from_route_as(rest),
        false => cctop_core::remote_open::NewAgent::from_route(rest),
    };
    let spec = match parsed {
        Ok(spec) => spec,
        Err(why) => return http::respond_error(stream, Some(request), 400, &why),
    };
    if let Some(why) = problem_here(&spec) {
        return http::respond_error(stream, Some(request), 404, &why);
    }
    open_or_resume(stream, request, || new_argv(&spec));
}

/// Open a terminal running what `argv` builds, or resume the one the request's
/// `Cctop-Resume` header names — see [`crate::resume`].
///
/// Called only once the route's own checks have passed, so a resume is let in
/// exactly as an open of the same route would be. A resume never runs `argv`:
/// for a new agent's route, that would start a second agent.
fn open_or_resume(stream: &mut TcpStream, request: &Request, argv: impl FnOnce() -> Vec<String>) {
    let key = request.header("sec-websocket-key");
    if key.is_empty() {
        return http::respond_error(stream, Some(request), 400, "no Sec-WebSocket-Key");
    }
    let size = |name: &str, default: u16| {
        request
            .query
            .get(name)
            .and_then(|v| v.parse::<u16>().ok())
            .filter(|n| (1..=4000).contains(n))
            .unwrap_or(default)
    };
    let size = (size("cols", 80), size("rows", 24));
    // Compressed when the tab offers it, on this socket alone: a resume
    // agrees afresh, and the ring keeps what the pty said, uncompressed.
    let deflate = ws::accept_deflate(request.header("sec-websocket-extensions"));
    let resume = request.header(ws::RESUME_HEADER).trim();
    if resume.is_empty() || resume == ws::RESUME_NEW {
        let resumable = !resume.is_empty();
        return terminal(stream, request, key, &argv(), size, resumable, deflate);
    }
    let Some((token, offset)) = resume
        .split_once(' ')
        .and_then(|(token, offset)| Some((token, offset.trim().parse::<u64>().ok()?)))
    else {
        return http::respond_error(stream, Some(request), 400, "not a resume cctop understands");
    };
    let term = match resume::claim(token, &request.path) {
        Ok(term) => term,
        Err(why) => return http::respond_error(stream, Some(request), 410, why),
    };
    // Out of the ring, the gap cannot be filled from here; the terminal is
    // ended with this answer, since the tab will not come back to it.
    let Some(missed) = term.ring.since(offset) else {
        return http::respond_error(
            stream,
            Some(request),
            410,
            "too much was said while you were away to replay it",
        );
    };
    // The tab may have been resized while it was away.
    cctop_core::shim::set_pty_size(&term.master, size.0, size.1);
    if upgrade(stream, key, term.token(), deflate.as_ref()).is_ok() {
        run(term, stream, &missed, deflate.map(|(_, d)| d));
    }
}

/// Spawn `argv` on a pty, upgrade to a WebSocket, and pump the one through
/// the other. `resumable` when the tab asked for a token to come back with.
fn terminal(
    stream: &mut TcpStream,
    request: &Request,
    key: &str,
    argv: &[String],
    size: (u16, u16),
    resumable: bool,
    deflate: Option<(String, ws::Deflater)>,
) {
    // A server started by systemd has no TERM, and an attach with none draws
    // nothing a terminal can read.
    let env = match std::env::var_os("TERM") {
        Some(_) => Vec::new(),
        None => vec![("TERM".to_string(), "xterm-256color".to_string())],
    };
    let (child, master) = match cctop_core::shim::spawn_pty(argv, size, &env) {
        Ok(spawned) => spawned,
        Err(error) => {
            return http::respond_error(stream, Some(request), 500, &error.to_string());
        }
    };
    let term = resume::Term::new(child, master, resumable.then_some(request.path.as_str()));
    if upgrade(stream, key, term.token(), deflate.as_ref()).is_ok() {
        run(term, stream, &[], deflate.map(|(_, d)| d));
    }
}

/// Answer the handshake, with the token to resume by when there is one, and
/// the compression agreed to when there is any.
fn upgrade(
    stream: &mut TcpStream,
    key: &str,
    token: Option<&str>,
    deflate: Option<&(String, ws::Deflater)>,
) -> std::io::Result<()> {
    let resume = match token {
        Some(token) => format!("{}: {token}\r\n", ws::RESUME_HEADER),
        None => String::new(),
    };
    let deflate = match deflate {
        Some((answer, _)) => format!("Sec-WebSocket-Extensions: {answer}\r\n"),
        None => String::new(),
    };
    let accept = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n{resume}{deflate}\r\n",
        ws::accept_key(key)
    );
    stream.write_all(accept.as_bytes())
}

/// Pump `term` through `stream` — `missed` first, what the tab has not seen —
/// and when the socket drops, park it for the tab to come back to. Returns
/// once the terminal is over, or another connection has it.
fn run(
    mut term: resume::Term,
    stream: &mut TcpStream,
    missed: &[u8],
    deflate: Option<ws::Deflater>,
) {
    let mut out = Output(deflate);
    // A terminal sits quiet for minutes; the request deadlines would cut it.
    let _ = stream.set_read_timeout(None);
    // But a write that makes no progress is a socket nobody is reading, and a
    // thread stuck in it could neither park the terminal nor hand it over.
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let sent = missed
        .chunks(16 * 1024)
        .try_for_each(|chunk| stream.write_all(&out.frame(chunk)));
    let pumped = match sent {
        Ok(()) => pump(stream, &mut term, &mut out),
        Err(_) => Pumped::Dropped,
    };
    match pumped {
        Pumped::Ended => {}
        // Refused only when the asker gave up waiting, and then the terminal
        // ends with the send.
        Pumped::Claimed(reply) => {
            let _ = reply.send(term);
        }
        Pumped::Dropped => resume::park(term, resume::GRACE),
    }
}

/// How a terminal's output is framed for one socket: compressed when the tab
/// agreed to it.
struct Output(Option<ws::Deflater>);

impl Output {
    fn frame(&mut self, bytes: &[u8]) -> Vec<u8> {
        match &mut self.0 {
            Some(deflater) => deflater.binary(bytes),
            None => frame(Op::Binary, bytes, false),
        }
    }

    /// The decoder for what the tab sends: it may compress too, once
    /// deflate is agreed, though a cctop tab never does.
    fn decoder(&self) -> ws::Decoder {
        match self.0 {
            Some(_) => ws::Decoder::default().inflating(),
            None => ws::Decoder::default(),
        }
    }
}

/// How long a write to the tab may make no progress before the socket is
/// taken for dropped. Well inside `resume`'s wait for a claim, so a terminal
/// stuck writing to a dead socket is still handed over.
pub(crate) const WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

const NO_SESSION: &str = "no such session on this machine";

/// What the pty runs: this binary's `mux open`.
#[cfg(not(test))]
fn open_argv(provider: &str, id: &str) -> Vec<String> {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "cctop".into());
    vec![exe, "mux".into(), "open".into(), provider.into(), id.into()]
}

/// What the pty runs for a new agent: this binary's `mux new`.
#[cfg(not(test))]
fn new_argv(spec: &cctop_core::remote_open::NewAgent) -> Vec<String> {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "cctop".into());
    let mut argv = vec![exe, "mux".into(), "new".into()];
    argv.extend(spec.args());
    argv
}

/// What this machine cannot start, asked of the machine itself.
#[cfg(not(test))]
fn problem_here(spec: &cctop_core::remote_open::NewAgent) -> Option<String> {
    spec.problem_here()
}

/// Under test no agent need be installed, so only the directory and the
/// account are asked about — the half of the check that is about this
/// machine's disk and its accounts.
#[cfg(test)]
fn problem_here(spec: &cctop_core::remote_open::NewAgent) -> Option<String> {
    let dir = cctop_core::util::untildify(&spec.dir);
    if !spec.dir.is_empty() && !std::path::Path::new(&dir).is_dir() {
        return Some(format!("{} is not a directory on this machine", spec.dir));
    }
    spec.profile_here().err()
}

/// Under test a shell stands in for `mux new`, as for [`open_argv`]: it says
/// what it was asked to start, and waits for a line.
#[cfg(test)]
fn new_argv(spec: &cctop_core::remote_open::NewAgent) -> Vec<String> {
    let mut argv = vec![
        "sh".into(),
        "-c".into(),
        r#"echo "new $*"; read line; echo "got $line""#.into(),
        "sh".into(),
    ];
    argv.extend(spec.args());
    argv
}

/// Under test this binary is the test harness, which would read `mux open`
/// as test filters: a shell stands in, saying what it was asked and at what
/// size, then echoing a line back — enough to see both directions and the
/// resize arrive.
///
/// Two session ids get other programs, for the resume tests: `count` says
/// numbered lines in two bursts either side of a pause, and `flood` says more
/// than [`resume::RING`] holds; each then echoes a line back.
#[cfg(test)]
fn open_argv(provider: &str, id: &str) -> Vec<String> {
    let script = match id {
        "count" => {
            r#"i=0; while [ $i -lt 600 ]; do echo "line $i"; i=$((i+1)); [ $i = 300 ] && sleep 0.2; done; read line; echo "got $line""#
        }
        "flood" => {
            r#"head -c 200000 /dev/zero | tr '\0' x; echo; echo flooded; read line; echo "got $line""#
        }
        _ => {
            r#"echo "opened $0 $1 $(stty size)"; read line; echo "got $line"; read line; echo "now $(stty size)""#
        }
    };
    vec![
        "sh".into(),
        "-c".into(),
        script.into(),
        provider.into(),
        id.into(),
    ]
}

/// How a socket's turn with a terminal ended.
enum Pumped {
    /// The terminal is over: closed by the tab, or its program ended.
    Ended,
    /// The socket went without a Close: the tab may come back.
    Dropped,
    /// A resume on another connection wants the terminal.
    Claimed(std::sync::mpsc::Sender<resume::Term>),
}

/// Pump the pty and the socket into each other until either closes, the
/// socket drops, or a resume claims the terminal.
fn pump(stream: &mut TcpStream, term: &mut resume::Term, out: &mut Output) -> Pumped {
    let mut decoder = out.decoder();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let mut fds = [
            libc::pollfd {
                fd: stream.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: term.master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: term.wake_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: three valid pollfds (or -1, which poll skips) on fds owned
        // for this call.
        if unsafe { libc::poll(fds.as_mut_ptr(), 3, -1) } < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Pumped::Dropped;
        }
        // The tab is back on another connection: this one was dead and had
        // not said so, which is how a network that went away ends a socket.
        if fds[2].revents != 0
            && let Some(reply) = term.take_claim()
        {
            return Pumped::Claimed(reply);
        }
        if fds[0].revents != 0 {
            match stream.read(&mut buf) {
                Ok(n) if n > 0 => decoder.push(&buf[..n]),
                _ => return Pumped::Dropped,
            }
            loop {
                match decoder.frame() {
                    Ok(Some((Op::Close, _))) => {
                        let _ = stream.write_all(&frame(Op::Close, b"", false));
                        return Pumped::Ended;
                    }
                    // Said, with the protocol-error code, so the tab shows why
                    // instead of closing as though the terminal had ended.
                    Err(why) => {
                        let mut payload = 1002u16.to_be_bytes().to_vec();
                        payload.extend(why.bytes().take(120));
                        let _ = stream.write_all(&frame(Op::Close, &payload, false));
                        return Pumped::Ended;
                    }
                    Ok(Some((Op::Ping, payload))) => {
                        if stream.write_all(&frame(Op::Pong, &payload, false)).is_err() {
                            return Pumped::Dropped;
                        }
                    }
                    Ok(Some((Op::Pong, _))) => {}
                    // Text is a resize, or a resumed tab asking for a redraw;
                    // nothing else is sent as text.
                    Ok(Some((Op::Text, text))) => {
                        if let Some((cols, rows)) = ws::parse_resize(&text) {
                            cctop_core::shim::set_pty_size(&term.master, cols, rows);
                        } else if ws::is_redraw(&text) {
                            redraw(&term.master);
                        }
                    }
                    Ok(Some((Op::Binary | Op::Other(_), keys))) => {
                        if (&term.master).write_all(&keys).is_err() {
                            let _ = stream.write_all(&frame(Op::Close, b"", false));
                            return Pumped::Ended;
                        }
                    }
                    Ok(None) => break,
                }
            }
        }
        if fds[1].revents != 0 {
            match (&term.master).read(&mut buf) {
                Ok(n) if n > 0 => {
                    // Kept before it is sent, so a socket that fails partway
                    // through the frame loses nothing a resume cannot replay.
                    term.ring.push(&buf[..n]);
                    if stream.write_all(&out.frame(&buf[..n])).is_err() {
                        return Pumped::Dropped;
                    }
                }
                // EIO once the program on it has exited: the far tab is told
                // the terminal closed, and shows what it last said.
                _ => {
                    let _ = stream.write_all(&frame(Op::Close, b"", false));
                    return Pumped::Ended;
                }
            }
        }
    }
}

/// Have the program on the pty draw its screen again, as it would for a
/// resize: SIGWINCH to the pty's foreground process group. A resumed tab asks
/// for it when it wrote over its own screen while it was away. Setting the
/// same size again would not do: the kernel only signals a size that changed.
fn redraw(master: &std::fs::File) {
    // SAFETY: `master` is a live pty master; both calls only read their
    // arguments.
    unsafe {
        let group = libc::tcgetpgrp(master.as_raw_fd());
        if group > 0 {
            libc::kill(-group, libc::SIGWINCH);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Snapshot;
    use crate::tests::{response_of, session, shared, status_of};
    use std::sync::Arc;

    /// A terminal is opened only for a full link with actions on, as a
    /// WebSocket, onto a session this machine has under exactly that name.
    /// Each refusal is before anything is spawned.
    #[test]
    fn only_a_full_link_opens_a_terminal_and_only_onto_a_session_here() {
        let mut s = shared("full", "ro");
        *s.latest.lock().unwrap() = Arc::new(Snapshot {
            version: 1,
            json: "[]".into(),
            rows: Vec::new(),
            sessions: vec![session("abc")],
            host_errors: Vec::new(),
        });
        let ws = "Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: x\r\n";
        let status =
            |s: &crate::Shared, target: &str, headers: &str| status_of(s, "GET", target, headers);
        assert!(status(&s, "/api/term/claude/abc?t=ro", ws).contains("403"));
        assert!(status(&s, "/api/term/claude/abc?t=full", "").contains("400"));
        for unknown in [
            "/api/term/claude/nope?t=full",
            "/api/term/codex/abc?t=full",
            "/api/term/claude?t=full",
            "/api/term/claude/-abc?t=full",
        ] {
            assert!(status(&s, unknown, ws).contains("404"), "{unknown}");
        }
        s.actions = false;
        assert!(status(&s, "/api/term/claude/abc?t=full", ws).contains("403"));
    }

    /// A new agent is started only for a full link with actions on, as a
    /// WebSocket, from a launch that reads back whole — and a directory this
    /// machine does not have is refused with that reason before anything
    /// is spawned.
    #[test]
    fn only_a_full_link_starts_an_agent_and_only_one_that_checks_out() {
        let mut s = shared("full", "ro");
        let ws = "Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: x\r\n";
        let status =
            |s: &crate::Shared, target: &str, headers: &str| status_of(s, "GET", target, headers);
        assert!(status(&s, "/api/new-term/claude/ask/-/-?t=ro", ws).contains("403"));
        assert!(status(&s, "/api/new-term/claude/ask/-/-?t=full", "").contains("400"));
        for bad in [
            "/api/new-term/sh/ask/-/-?t=full",
            "/api/new-term/claude/root/-/-?t=full",
            "/api/new-term/codex/plan/-/-?t=full",
            "/api/new-term/claude/ask/-?t=full",
            "/api/new-term/claude/ask/-/zz?t=full",
        ] {
            assert!(status(&s, bad, ws).contains("400"), "{bad}");
        }
        let nowhere = cctop_core::remote_open::NewAgent {
            agent: "claude".into(),
            dir: "/no/such/dir/here".into(),
            options: Default::default(),
            account: None,
        };
        let said = status(&s, &format!("{}?t=full", nowhere.route()), ws);
        assert!(said.contains("404"), "{said}");

        // Under an account: the name checked as strictly as a model, and
        // looked up among this machine's — one it does not have is refused,
        // never started under the default instead.
        for bad in [
            "/api/new-term-as/claude/ask/-/-?t=full",
            "/api/new-term-as/claude/ask/-/-/2d78?t=full",
            "/api/new-term-as/claude/ask/-/-/zz?t=full",
            "/api/new-term-as/opencode/ask/-/-/776f726b?t=full",
            "/api/new-term-as/claude/ask/-/-/776f726b/x?t=full",
        ] {
            assert!(status(&s, bad, ws).contains("400"), "{bad}");
        }
        let stranger = cctop_core::remote_open::NewAgent {
            agent: "claude".into(),
            dir: String::new(),
            options: Default::default(),
            account: Some("cctop-test-no-such-account".into()),
        };
        let said = response_of(&s, "GET", &format!("{}?t=full", stranger.route()), ws);
        assert!(said.starts_with("HTTP/1.1 404"), "{said}");
        assert!(said.contains("no claude account"), "{said}");
        s.actions = false;
        assert!(status(&s, "/api/new-term/claude/ask/-/-?t=full", ws).contains("403"));
    }

    /// What a sibling's launcher learns this machine's accounts from: names
    /// per harness, for a link that could then start an agent, and nothing
    /// for one that could not — a read-only link, or a serve with actions off.
    #[test]
    fn this_machines_accounts_are_named_for_a_full_link_only() {
        cctop_core::config::add_launchable(cctop_core::config::Profile {
            provider: cctop_core::pricing::Provider::Codex,
            name: "cctop-test-listed-acct".into(),
            dir: std::env::temp_dir().join(".codex-cctop-test-listed-acct"),
            source: cctop_core::config::AccountSource::Directory,
        });
        let mut s = shared("full", "ro");
        assert!(status_of(&s, "GET", "/api/accounts?t=ro", "").contains("403"));
        let said = response_of(&s, "GET", "/api/accounts?t=full", "");
        assert!(said.starts_with("HTTP/1.1 200"), "{said}");
        let body: serde_json::Value =
            serde_json::from_str(said.split("\r\n\r\n").nth(1).expect("body")).expect("json");
        let codex = body["codex"].as_array().expect("codex names");
        assert!(
            codex.iter().any(|n| n == "cctop-test-listed-acct"),
            "{body}"
        );
        assert!(body["claude"].is_array(), "{body}");
        assert!(!said.contains(".codex-cctop-test"), "a directory went out");
        s.actions = false;
        assert!(status_of(&s, "GET", "/api/accounts?t=full", "").contains("403"));
    }

    /// End to end, as a sibling's launcher reaches it: signed, upgraded, and
    /// the far side handed the launch as words — agent, directory, settings.
    #[test]
    fn a_sibling_starts_a_new_agent_here_with_its_settings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let launch = cctop_core::remote_open::NewAgent {
            agent: "claude".into(),
            dir: dir.path().to_string_lossy().into_owned(),
            options: cctop_core::launch_opts::Options {
                model: Some("opus".into()),
                permissions: cctop_core::launch_opts::Permissions::Plan,
            },
            account: None,
        };
        let want = format!("new claude {} --model opus --permissions plan", launch.dir);
        sibling_launch_shows(&launch, &want);
    }

    /// The same, under one of this machine's accounts: the name reaches the
    /// far side's `mux new`, which puts the agent under it.
    #[test]
    fn a_sibling_starts_a_new_agent_here_under_an_account_of_this_machines() {
        let dir = tempfile::tempdir().expect("tempdir");
        cctop_core::config::add_launchable(cctop_core::config::Profile {
            provider: cctop_core::pricing::Provider::Claude,
            name: "cctop-test-serve-acct".into(),
            dir: dir.path().join(".claude-cctop-test-serve-acct"),
            source: cctop_core::config::AccountSource::Directory,
        });
        let launch = cctop_core::remote_open::NewAgent {
            agent: "claude".into(),
            dir: dir.path().to_string_lossy().into_owned(),
            options: Default::default(),
            account: Some("cctop-test-serve-acct".into()),
        };
        assert!(
            launch
                .route()
                .starts_with(cctop_core::remote_open::NEW_AS_ROUTE)
        );
        let want = format!("new claude {} --account cctop-test-serve-acct", launch.dir);
        sibling_launch_shows(&launch, &want);
    }

    /// Open `launch` on a serve here as a sibling would, signed, and read the
    /// terminal until it shows `want`.
    fn sibling_launch_shows(launch: &cctop_core::remote_open::NewAgent, want: &str) {
        use cctop_core::ws::{Decoder, Op};
        use std::net::TcpListener;
        const SECRET: &str = "eyJhIjoi-made-up-tunnel-token";
        let s = Arc::new(crate::Shared {
            peer_secret: SECRET.to_string(),
            ..shared("full", "ro")
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = {
            let s = Arc::clone(&s);
            std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                crate::serve_connection(&s, &mut stream);
            })
        };
        let peer =
            cctop_core::peer::Peer::fixed("box", &format!("http://127.0.0.1:{port}"), SECRET);
        let key = cctop_core::ws::new_key();
        let headers = [
            ("Upgrade".to_string(), "websocket".to_string()),
            ("Connection".to_string(), "Upgrade".to_string()),
            ("Sec-WebSocket-Key".to_string(), key.clone()),
            ("Sec-WebSocket-Version".to_string(), "13".to_string()),
        ];
        let mut up = peer
            .socket(&launch.route(), "cols=100&rows=30", &headers)
            .expect("socket");
        let mut got = Vec::new();
        let end = loop {
            if let Some(at) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                break at + 4;
            }
            assert!(up.read_ready(&mut got).unwrap(), "closed before the head");
        };
        let head = String::from_utf8_lossy(&got[..end]).into_owned();
        assert!(head.starts_with("HTTP/1.1 101"), "{head}");
        let mut decoder = Decoder::default();
        decoder.push(&got[end..]);
        let mut screen = String::new();
        while !screen.contains(want) {
            while let Some((op, payload)) = decoder.frame().unwrap() {
                if op == Op::Binary {
                    screen.push_str(&String::from_utf8_lossy(&payload));
                }
            }
            if screen.contains(want) {
                break;
            }
            let mut more = Vec::new();
            assert!(up.read_ready(&mut more).unwrap(), "closed: {screen}");
            decoder.push(&more);
        }
        up.write_all(&cctop_core::ws::frame(Op::Close, b"", true))
            .unwrap();
        server.join().unwrap();
    }

    /// End to end over a socket, as a sibling's tab reaches it: signed with
    /// the tunnel token, upgraded, the far program started at the size asked
    /// for, keystrokes in, output out, and a resize that the program sees.
    #[test]
    fn a_sibling_gets_a_terminal_it_can_type_into_and_resize() {
        use cctop_core::ws::{self, Decoder, Op, frame};
        use std::net::TcpListener;
        const SECRET: &str = "eyJhIjoi-made-up-tunnel-token";
        let s = Arc::new(crate::Shared {
            peer_secret: SECRET.to_string(),
            ..shared("full", "ro")
        });
        *s.latest.lock().unwrap() = Arc::new(Snapshot {
            version: 1,
            json: "[]".into(),
            rows: Vec::new(),
            sessions: vec![session("abc")],
            host_errors: Vec::new(),
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = {
            let s = Arc::clone(&s);
            std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                crate::serve_connection(&s, &mut stream);
            })
        };
        let peer =
            cctop_core::peer::Peer::fixed("box", &format!("http://127.0.0.1:{port}"), SECRET);
        let key = ws::new_key();
        let headers = [
            ("Upgrade".to_string(), "websocket".to_string()),
            ("Connection".to_string(), "Upgrade".to_string()),
            ("Sec-WebSocket-Key".to_string(), key.clone()),
            ("Sec-WebSocket-Version".to_string(), "13".to_string()),
        ];
        let mut up = peer
            .socket("/api/term/claude/abc", "cols=100&rows=30", &headers)
            .expect("socket");

        let mut got = Vec::new();
        let end = loop {
            if let Some(at) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                break at + 4;
            }
            assert!(up.read_ready(&mut got).unwrap(), "closed before the head");
        };
        let head = String::from_utf8_lossy(&got[..end]).into_owned();
        assert!(head.starts_with("HTTP/1.1 101"), "{head}");
        assert!(head.contains(&ws::accept_key(&key)), "{head}");

        let mut decoder = Decoder::default();
        decoder.push(&got[end..]);
        let mut screen = String::new();
        let mut until = |up: &mut cctop_core::peer::Upstream, want: &str| {
            while !screen.contains(want) {
                while let Some((op, payload)) = decoder.frame().unwrap() {
                    // The program's exit closes the socket behind its last
                    // words, which may arrive in the same read.
                    if op == Op::Close {
                        break;
                    }
                    assert_eq!(op, Op::Binary, "{screen}");
                    screen.push_str(&String::from_utf8_lossy(&payload));
                }
                if screen.contains(want) {
                    break;
                }
                let mut more = Vec::new();
                assert!(up.read_ready(&mut more).unwrap(), "closed: {screen}");
                decoder.push(&more);
            }
        };
        until(&mut up, "opened claude abc 30 100");
        up.write_all(&frame(Op::Binary, b"hello\r", true)).unwrap();
        until(&mut up, "got hello");
        up.write_all(&frame(
            Op::Text,
            ws::resize_message(120, 40).as_bytes(),
            true,
        ))
        .unwrap();
        up.write_all(&frame(Op::Binary, b"\r", true)).unwrap();
        until(&mut up, "now 40 120");
        up.write_all(&frame(Op::Close, b"", true)).unwrap();
        server.join().unwrap();
    }

    /// A server answering `n` connections, each on its own thread as the real
    /// accept loop would, with the sessions `ids` known.
    fn serving(ids: &[&str], n: usize) -> (u16, Arc<crate::Shared>) {
        use std::net::TcpListener;
        let s = Arc::new(crate::Shared {
            peer_secret: SECRET.to_string(),
            ..shared("full", "ro")
        });
        *s.latest.lock().unwrap() = Arc::new(Snapshot {
            version: 1,
            json: "[]".into(),
            rows: Vec::new(),
            sessions: ids.iter().map(|id| session(id)).collect(),
            host_errors: Vec::new(),
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = Arc::clone(&s);
        std::thread::spawn(move || {
            for _ in 0..n {
                let (mut stream, _) = listener.accept().unwrap();
                let s = Arc::clone(&server);
                std::thread::spawn(move || crate::serve_connection(&s, &mut stream));
            }
        });
        (port, s)
    }

    const SECRET: &str = "eyJhIjoi-made-up-tunnel-token";

    /// A sibling's tab, as the bridge keeps it: the socket, what it has been
    /// handed, and how to come back.
    struct Tab {
        up: cctop_core::peer::Upstream,
        decoder: cctop_core::ws::Decoder,
        token: Option<String>,
        /// Output bytes handed over, which a resume asks the rest from.
        received: u64,
        screen: Vec<u8>,
        /// Bytes off the socket after the head: what crossed the wire.
        wire: u64,
        /// Whether the server agreed to compress.
        deflated: bool,
    }

    /// Open `path` (resuming with `resume` when given), and the answer's head
    /// when it is not an upgrade.
    fn tab(port: u16, path: &str, resume: &str) -> Result<Tab, String> {
        tab_offering(port, path, resume, "")
    }

    /// [`tab`], offering `extensions` as a tab's handshake does — or none,
    /// as an older tab's did, when empty.
    fn tab_offering(port: u16, path: &str, resume: &str, extensions: &str) -> Result<Tab, String> {
        use cctop_core::ws;
        let peer =
            cctop_core::peer::Peer::fixed("box", &format!("http://127.0.0.1:{port}"), SECRET);
        let mut headers = vec![
            ("Upgrade".to_string(), "websocket".to_string()),
            ("Connection".to_string(), "Upgrade".to_string()),
            ("Sec-WebSocket-Key".to_string(), ws::new_key()),
            ("Sec-WebSocket-Version".to_string(), "13".to_string()),
            (ws::RESUME_HEADER.to_string(), resume.to_string()),
        ];
        if !extensions.is_empty() {
            headers.push((
                "Sec-WebSocket-Extensions".to_string(),
                extensions.to_string(),
            ));
        }
        let mut up = peer
            .socket(path, "cols=100&rows=30", &headers)
            .expect("socket");
        let mut got = Vec::new();
        let end = loop {
            if let Some(at) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                break at + 4;
            }
            if !up.read_ready(&mut got).unwrap() {
                return Err(String::from_utf8_lossy(&got).into_owned());
            }
        };
        let head = String::from_utf8_lossy(&got[..end]).into_owned();
        if !head.starts_with("HTTP/1.1 101") {
            // The body, which says why, follows the head.
            while up.read_ready(&mut got).unwrap_or(false) {}
            return Err(String::from_utf8_lossy(&got).into_owned());
        }
        let token = head.lines().find_map(|line| {
            let (k, v) = line.split_once(':')?;
            k.eq_ignore_ascii_case(ws::RESUME_HEADER)
                .then(|| v.trim().to_string())
        });
        let deflated = head.lines().any(|line| {
            line.split_once(':').is_some_and(|(k, v)| {
                k.eq_ignore_ascii_case("sec-websocket-extensions") && ws::deflate_agreed(v)
            })
        });
        let mut decoder = ws::Decoder::default();
        if deflated {
            decoder = decoder.inflating();
        }
        decoder.push(&got[end..]);
        Ok(Tab {
            up,
            decoder,
            token,
            received: 0,
            screen: Vec::new(),
            wire: (got.len() - end) as u64,
            deflated,
        })
    }

    impl Tab {
        /// Read until the screen holds `want`, counting what is handed over.
        fn until(&mut self, want: &str) {
            use cctop_core::ws::Op;
            loop {
                while let Some((op, payload)) = self.decoder.frame().unwrap() {
                    if op == Op::Binary {
                        self.received += payload.len() as u64;
                        self.screen.extend_from_slice(&payload);
                    }
                }
                if String::from_utf8_lossy(&self.screen).contains(want) {
                    return;
                }
                let mut more = Vec::new();
                let open = self.up.read_ready(&mut more).unwrap();
                self.wire += more.len() as u64;
                assert!(
                    open || !more.is_empty(),
                    "closed: {}",
                    String::from_utf8_lossy(&self.screen)
                );
                self.decoder.push(&more);
            }
        }

        fn resume_header(&self) -> String {
            format!(
                "{} {}",
                self.token.as_deref().expect("a token"),
                self.received
            )
        }

        fn send(&mut self, keys: &[u8]) {
            use cctop_core::ws::{Op, frame};
            self.up.write_all(&frame(Op::Binary, keys, true)).unwrap();
        }

        fn close(mut self) {
            use cctop_core::ws::{Op, frame};
            let _ = self.up.write_all(&frame(Op::Close, b"", true));
        }
    }

    /// The whole of what `count` says, as a pty hands it over.
    fn counted() -> String {
        (0..600).map(|i| format!("line {i}\r\n")).collect()
    }

    /// The socket drops partway through the output — whatever was in flight
    /// with it is lost — and the tab comes back with how much it had: it is
    /// handed exactly the rest, nothing twice and nothing missing, and it is
    /// the same program on the same pty, which answers what is typed next.
    #[test]
    fn a_dropped_tab_resumes_where_it_left_off_on_the_same_terminal() {
        use cctop_core::ws::RESUME_NEW;
        let (port, _s) = serving(&["count"], 2);
        let deflate = cctop_core::ws::DEFLATE;
        let mut first =
            tab_offering(port, "/api/term/claude/count", RESUME_NEW, deflate).expect("open");
        assert!(first.deflated);
        first.until("line 5\r\n");
        // Dropped mid-stream: the frames behind the one read are thrown away
        // with the socket, as a network that went away throws them.
        let resume = first.resume_header();
        let before = std::mem::take(&mut first.screen);
        let received = first.received;
        drop(first);
        let mut back =
            tab_offering(port, "/api/term/claude/count", &resume, deflate).expect("resumed");
        back.received = received;
        back.until("line 599\r\n");
        let mut whole = before;
        whole.extend_from_slice(&back.screen);
        assert_eq!(String::from_utf8_lossy(&whole), counted());
        back.send(b"again\r");
        back.until("got again");
        back.close();
    }

    /// A resume the server cannot honour is answered `410 Gone`, and nothing
    /// else changes: a token with the wrong secret, or for another route,
    /// leaves the terminal parked for the right one; a fresh open still works.
    #[test]
    fn a_wrong_token_is_gone_and_the_right_one_still_resumes() {
        use cctop_core::ws::RESUME_NEW;
        let (port, _s) = serving(&["abc", "count"], 6);
        let mut first = tab(port, "/api/term/claude/abc", RESUME_NEW).expect("open");
        first.until("opened claude abc");
        let resume = first.resume_header();
        drop(first);
        let (token, offset) = resume.split_once(' ').unwrap();
        let (id, secret) = token.split_once('.').unwrap();
        // One hex digit off.
        let first = if secret.starts_with('0') { '1' } else { '0' };
        let wrong = format!("{id}.{first}{} {offset}", &secret[1..]);
        let said = tab(port, "/api/term/claude/abc", &wrong)
            .err()
            .expect("refused");
        assert!(said.starts_with("HTTP/1.1 410"), "{said}");
        let said = tab(port, "/api/term/claude/count", &resume)
            .err()
            .expect("another route is refused");
        assert!(said.starts_with("HTTP/1.1 410"), "{said}");
        let said = tab(port, "/api/term/claude/abc", "nonsense")
            .err()
            .expect("garbled");
        assert!(said.starts_with("HTTP/1.1 400"), "{said}");
        let mut back = tab(port, "/api/term/claude/abc", &resume).expect("resumed");
        back.send(b"hello\r");
        back.until("got hello");
        back.close();
        // An old tab, which asks for nothing, is given no token.
        let old = tab(port, "/api/term/claude/abc", "").expect("open");
        assert_eq!(old.token, None);
        old.close();
    }

    /// More was said while the tab was away than the ring holds: the resume
    /// is `410`, the terminal behind it ended, and a fresh open — the
    /// fallback — is a terminal again.
    #[test]
    fn a_resume_past_what_the_ring_holds_is_gone() {
        use cctop_core::ws::RESUME_NEW;
        let (port, _s) = serving(&["flood"], 3);
        let mut first = tab(port, "/api/term/claude/flood", RESUME_NEW).expect("open");
        // Read all of it, but say only the first byte was received: the rest
        // is what a resume would have to replay.
        first.until("flooded");
        assert!(first.received > super::resume::RING as u64);
        let resume = format!("{} 1", first.token.clone().unwrap());
        drop(first);
        let said = tab(port, "/api/term/claude/flood", &resume)
            .err()
            .expect("past the ring");
        assert!(said.starts_with("HTTP/1.1 410"), "{said}");
        let token = resume.split(' ').next().unwrap();
        assert!(!super::resume::registered(token), "ended with the answer");
        let mut fresh = tab(port, "/api/term/claude/flood", RESUME_NEW).expect("reopened");
        fresh.until("flooded");
        fresh.close();
    }

    /// A tab that offers deflate is sent its output compressed — the same
    /// bytes on its screen for a fraction on the wire — and one that offers
    /// nothing, as every tab before this did, is sent it as it always was.
    #[test]
    fn output_is_compressed_for_a_tab_that_offers_it_and_only_then() {
        use cctop_core::ws::{DEFLATE, RESUME_NEW};
        let (port, _s) = serving(&["flood"], 2);
        let mut new =
            tab_offering(port, "/api/term/claude/flood", RESUME_NEW, DEFLATE).expect("open");
        assert!(new.deflated);
        new.until("flooded");
        assert!(
            new.wire * 20 < new.received,
            "{} on the wire for {}",
            new.wire,
            new.received
        );
        new.send(b"hi\r");
        new.until("got hi");
        new.close();

        let mut old = tab(port, "/api/term/claude/flood", "").expect("open");
        assert!(!old.deflated);
        old.until("flooded");
        assert!(
            old.wire > old.received,
            "{} on the wire for {}",
            old.wire,
            old.received
        );
        old.close();
    }

    /// A new agent's tab resumes like any other — and a resume never starts
    /// the agent again: the program that answers is the one already running.
    #[test]
    fn a_new_agents_tab_resumes_without_starting_a_second() {
        use cctop_core::ws::RESUME_NEW;
        let (port, _s) = serving(&[], 2);
        let dir = tempfile::tempdir().expect("tempdir");
        let launch = cctop_core::remote_open::NewAgent {
            agent: "claude".into(),
            dir: dir.path().to_string_lossy().into_owned(),
            options: Default::default(),
            account: None,
        };
        let mut first = tab(port, &launch.route(), RESUME_NEW).expect("open");
        first.until("new claude");
        let resume = first.resume_header();
        drop(first);
        let mut back = tab(port, &launch.route(), &resume).expect("resumed");
        back.send(b"one\r");
        back.until("got one");
        let screen = String::from_utf8_lossy(&back.screen).into_owned();
        assert!(!screen.contains("new claude"), "started again: {screen}");
        back.close();
    }
}
