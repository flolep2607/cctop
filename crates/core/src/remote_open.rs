//! Opening a session that lives on another machine, in a tab on this one.
//!
//! A remote row used to be read-only: its process, its transcript and its
//! terminal are all over there. Reading it was enough to watch; to *work* in
//! it you had to go to that machine. This is the way over. The tab here runs a
//! transport, and the cctop on the far side decides what the tab shows there,
//! because only it can see what it decides from:
//!
//! - the session is live in that cctop's rmux: the tab attaches to it;
//! - it has ended: it is resumed there, in its project, inside that cctop's
//!   rmux — so it survives the connection dropping, and the next open
//!   reattaches instead of starting a rival;
//! - it is live in some terminal cctop did not start: nothing can take that
//!   terminal over, so the tab says so and is a shell in its project instead;
//! - the far cctop is missing or too old to do any of this: the tab says what
//!   to update and is a shell there.
//!
//! The deciding is [`open_here`], run on the far side as `cctop mux open`. The
//! transport is one of two, whichever [`crate::reach`] says the row's machine
//! is reached by — the same answer the conversation reader goes by:
//!
//! - **A sibling on the Cloudflare account** ([`crate::peer`]) needs no ssh
//!   route at all, which between a laptop and a server behind someone else's
//!   firewall is often the one that does not exist. The tab
//!   runs [`bridge`] (`cctop mux reach`), which opens a signed WebSocket to the
//!   sibling's `/api/term/<provider>/<id>`; the sibling runs `cctop mux open`
//!   on a pty and pumps it through ([`crate::ws`] has the framing).
//! - **An ssh `--host`** ([`crate::fleet`]): `ssh -t` over the launcher's
//!   ControlMaster socket when one is up, so a host that wants a password asks
//!   for it in the tab, once.
//!
//! Every name in here — the host, the session id, the provider — came out of
//! another machine's answer, so each is checked before it goes anywhere near a
//! command line: [`remote_launch::host_ok`] for an ssh destination, [`id_ok`]
//! for a session id, and everything bound for a shell through
//! [`sh_quote`](crate::sandbox::sh_quote).

use crate::fleet::Host;
use crate::reach::Reach;
use crate::remote_launch::host_ok;
use crate::sandbox::sh_quote;
use crate::session::Session;
use std::path::{Path, PathBuf};

/// Whether `id` may be handed on as a session id.
///
/// Every harness's ids are uuids, `rollout-<time>-<uuid>`, `ses_…` or cctop's
/// own `_pid_<n>`: letters, digits and `-_.`. Anything else did not come from
/// a harness, and a dash in front could read as an option to whatever is
/// handed it — so it goes nowhere.
pub fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && !id.starts_with(['-', '.'])
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// The local rmux session a remote tab's transport runs in.
///
/// Named after the session and its machine, so opening the same row twice
/// switches to the tab already there, and a cctop restarted here finds the tab
/// still open. The machine is in the name because two machines can each hold a
/// session of one id — a transcript copied between them — and those are two
/// tabs.
pub fn tab_name(host: &str, provider: &str, id: &str) -> String {
    crate::rmux::name_for_session(&format!("{provider}-on-{host}"), id)
}

/// What a tab here runs to open `provider`/`id` on the machine `reach` names.
///
/// `dir` is the session's project as the far side reported it: the shell the
/// tab falls back to starts there. `exe` is this cctop, which the peer
/// transport runs as its bridge.
pub fn argv(
    reach: &Reach,
    exe: &Path,
    provider: &str,
    id: &str,
    dir: &str,
) -> Result<Vec<String>, String> {
    if !id_ok(id) {
        return Err(format!("{id:?} is not a session id cctop will pass on"));
    }
    if crate::pricing::Provider::parse(provider).is_none() {
        return Err(format!("{provider:?} is not a harness cctop knows"));
    }
    match reach {
        Reach::Peer(peer) => {
            let machine = &peer.machine;
            if !host_ok(machine) {
                return Err(format!("{machine:?} is not a machine name"));
            }
            Ok(vec![
                exe.to_string_lossy().into_owned(),
                "mux".into(),
                "reach".into(),
                machine.clone(),
                provider.into(),
                id.into(),
            ])
        }
        Reach::Ssh(host) => ssh_argv(host, provider, id, dir),
    }
}

/// The far side's half of an ssh open: run `cctop mux open`, and when that
/// cctop cannot — absent, or older than `mux open` — say what to update and
/// be a shell in the project instead.
///
/// Asked with `--check` first rather than read off the version: the version a
/// command arrived in is a guess about what a build supports, and this asks
/// the build. An old cctop answers the unknown word with a usage error, and a
/// missing one with the shell's "not found", and both fall through alike.
///
/// The command is the host's `--host box:/path/to/cctop` spelling, which the
/// user wrote and the poll already runs as typed — so it is `eval`ed, as the
/// poll's command line is read by the far shell. Everything after it arrived
/// from that machine and is passed as an argument, never read as code.
const SSH_SCRIPT: &str = r#"c=$1; shift
if eval "$c mux open --check" >/dev/null 2>&1; then eval "exec $c mux open \"\$@\""; fi
d=$3
printf '%s\r\n' "cctop is missing here, or too old to open sessions for another machine." "Update it on this machine (cctop --update) and open the row again." "Meanwhile, a shell in $d:" >&2
case $d in "~"|"") d=$HOME ;; "~/"*) d=$HOME/${d#"~/"} ;; esac
cd -- "$d" 2>/dev/null || cd
exec "${SHELL:-sh}" -l"#;

fn ssh_argv(host: &Host, provider: &str, id: &str, dir: &str) -> Result<Vec<String>, String> {
    if !host_ok(&host.target) {
        return Err(format!("{:?} is not an ssh host", host.target));
    }
    let dir = match dir.trim() {
        "" => "~",
        dir => dir,
    };
    let mut argv = vec!["ssh".to_string(), "-t".to_string()];
    // Over the launcher's master when it is up, so a host that asked for a
    // password once does not ask again; `auto` connects afresh otherwise, and
    // asks in the tab.
    if let Some(socket) = crate::ssh_master::socket_for(&host.target) {
        argv.extend([
            "-S".to_string(),
            socket.to_string_lossy().into_owned(),
            "-o".to_string(),
            "ControlMaster=auto".to_string(),
        ]);
    }
    argv.extend([
        host.target.clone(),
        "--".to_string(),
        format!(
            "exec sh -c {} cctop-open {} {} {} {}",
            sh_quote(SSH_SCRIPT),
            sh_quote(&host.command),
            sh_quote(provider),
            sh_quote(id),
            sh_quote(dir),
        ),
    ]);
    Ok(argv)
}

/// What the far side does with an open, decided from what it can see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Become a client of this rmux session.
    Attach(String),
    /// Start the harness back up on the transcript, in rmux, then attach.
    Resume,
    /// Nothing to attach to or resume: say why, and be a shell.
    Shell(String),
}

/// Decide an open on the machine that has the session.
///
/// `holding` is [`rmux::holding`](crate::rmux::holding) and `exists`
/// [`rmux::exists`](crate::rmux::exists), passed in so the decision can be
/// tested without a daemon.
pub fn decide(
    found: Option<&Session>,
    id: &str,
    holding: impl Fn(u32) -> Option<String>,
    exists: impl Fn(&str) -> bool,
) -> Step {
    let Some(session) = found else {
        return Step::Shell(format!(
            "No session {id} on this machine any more — it may have been deleted."
        ));
    };
    // A resume from this or any machine's cctop is named this way, so it is
    // the first place a live agent is.
    let resumed = crate::rmux::name_for_session(session.provider.as_str(), &session.session_id);
    if exists(&resumed) {
        return Step::Attach(resumed);
    }
    if session.is_running() {
        // Launched by cctop under a name of its own (`cctop-claude-2`), and
        // found by the agent's pid instead.
        if let Some(name) = session.root_pid().and_then(&holding) {
            return Step::Attach(name);
        }
        return Step::Shell(format!(
            "{} is running in a terminal cctop did not start, so its screen cannot be taken \
             over from another machine. Start sessions with cctop to be able to.",
            session.display_label()
        ));
    }
    Step::Resume
}

/// `cctop mux open PROVIDER ID [DIR]`: on the machine that has the session,
/// attach to it, resume it, or say why not and be a shell — see the module
/// docs. `--check` answers `ok`, which is how the ssh transport tells a cctop
/// that has this from one that does not.
///
/// Returns only on failure, or when the attach ends: every success path either
/// becomes the rmux client or `exec`s the shell.
pub fn open_here(args: &[String]) -> Result<(), String> {
    let (provider, id, dir) = match args {
        [flag] if flag == "--check" => {
            println!("ok");
            return Ok(());
        }
        [provider, id] => (provider, id, None),
        [provider, id, dir] => (provider, id, Some(dir.as_str())),
        _ => return Err("usage: cctop mux open PROVIDER SESSION-ID [DIR]".into()),
    };
    if !id_ok(id) {
        return Err(format!("{id:?} is not a session id"));
    }
    let fallback_dir = dir.map(|d| PathBuf::from(crate::util::untildify(d)));
    eprint!("Finding the session on this machine…\r\n");
    let mut loader = crate::loader::Loader::new();
    let sessions = loader.load(crate::pricing::Plan::Retail);
    loader.store().save();
    let found = sessions
        .iter()
        .find(|s| s.remote.is_none() && s.provider.as_str() == provider && s.session_id == *id);
    let step = decide(found, id, crate::rmux::holding, crate::rmux::exists);
    let work_dir = found.and_then(Session::work_dir).or(fallback_dir);
    match step {
        Step::Attach(name) => attach(&name),
        Step::Shell(why) => shell_in(work_dir.as_deref(), &why),
        Step::Resume => {
            let session = found.expect("Resume is only decided for a session found");
            match crate::actions::resume(session) {
                Ok(done) => match done.rmux {
                    Some(name) => attach(&name),
                    None => shell_in(work_dir.as_deref(), &done.message),
                },
                Err((_, why)) => shell_in(work_dir.as_deref(), &format!("Could not resume: {why}")),
            }
        }
    }
}

fn attach(name: &str) -> Result<(), String> {
    match crate::mux::main(&["attach".to_string(), name.to_string()]) {
        0 => Ok(()),
        // `mux::main` has already said why on stderr.
        _ => Err(format!("could not attach to {name}")),
    }
}

/// Say `why`, then become a login shell in `dir` (or the home, when that is
/// gone too) — the tab is still somewhere useful on that machine.
fn shell_in(dir: Option<&Path>, why: &str) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let dir = dir
        .filter(|d| d.is_dir())
        .map(Path::to_path_buf)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("/"));
    eprint!(
        "{why}\r\nHere is a shell in {} instead.\r\n",
        crate::util::tildify(&dir.to_string_lossy())
    );
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "sh".into());
    let error = std::process::Command::new(&shell)
        .arg("-l")
        .current_dir(&dir)
        .exec();
    Err(format!("could not start {shell}: {error}"))
}

/// How often the bridge says something when the terminal says nothing.
///
/// Cloudflare's edge closes a WebSocket that has carried nothing for 100
/// seconds, and an agent waiting for input is silent for far longer than that.
const KEEPALIVE: std::time::Duration = std::time::Duration::from_secs(30);

/// The most of a refusal the bridge will read before giving up on the rest.
const MAX_HEAD: usize = 64 * 1024;

/// `cctop mux reach MACHINE PROVIDER ID`: the tab's end of a sibling's
/// terminal. Finds the sibling, opens `/api/term/<provider>/<id>` there,
/// signed as this machine, and pumps this terminal through it until either
/// side closes.
pub fn bridge(args: &[String]) -> Result<(), String> {
    let [machine, provider, id] = args else {
        return Err("usage: cctop mux reach MACHINE PROVIDER SESSION-ID".into());
    };
    if !host_ok(machine) || !id_ok(id) || crate::pricing::Provider::parse(provider).is_none() {
        return Err("that is not a machine, harness and session cctop will pass on".into());
    }
    eprint!("Reaching {machine}…\r\n");
    let peer = crate::peer::lookup(machine)?;
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let key = crate::ws::new_key();
    let headers = [
        ("Upgrade".to_string(), "websocket".to_string()),
        ("Connection".to_string(), "Upgrade".to_string()),
        ("Sec-WebSocket-Key".to_string(), key.clone()),
        ("Sec-WebSocket-Version".to_string(), "13".to_string()),
    ];
    let path = format!("/api/term/{provider}/{id}");
    let mut up = peer.socket(&path, &format!("cols={cols}&rows={rows}"), &headers)?;

    let mut got = Vec::new();
    let end = loop {
        if let Some(at) = got.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        if got.len() > MAX_HEAD
            || !up
                .read_ready(&mut got)
                .map_err(|e| format!("{machine} hung up: {e}"))?
        {
            return Err(format!("{machine} hung up before it answered"));
        }
    };
    let head = String::from_utf8_lossy(&got[..end]).into_owned();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    if status != 101 {
        let mut body = got[end..].to_vec();
        let wanted = header(&head, "content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
            .min(MAX_HEAD);
        while body.len() < wanted && up.read_ready(&mut body).unwrap_or(false) {}
        return Err(refusal(machine, status, &String::from_utf8_lossy(&body)));
    }
    if header(&head, "sec-websocket-accept") != Some(crate::ws::accept_key(&key).as_str()) {
        return Err(format!(
            "{machine} answered with something that is not a terminal"
        ));
    }
    let mut decoder = crate::ws::Decoder::default();
    decoder.push(&got[end..]);
    pump(&mut up, &mut decoder, (cols, rows))
}

/// A header's value out of a response head, by name.
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// What to tell the person in the tab when the sibling said no.
///
/// A sibling that has never heard of the route answers it like any unknown
/// page, which is the one refusal that means "update that machine" rather
/// than whatever it says.
fn refusal(machine: &str, status: u16, body: &str) -> String {
    let said = body.lines().next().unwrap_or("").trim();
    match status {
        404 if said.ends_with("no such page") || said.is_empty() => format!(
            "{machine}'s cctop is too old to open its sessions on another machine — \
             update it there (cctop --update) and open the row again"
        ),
        _ => format!("{machine} refused: {said}"),
    }
}

/// Restores the terminal however the bridge ends.
struct Raw(bool);

impl Drop for Raw {
    fn drop(&mut self) {
        if self.0 {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
}

fn pump(
    up: &mut crate::peer::Upstream,
    decoder: &mut crate::ws::Decoder,
    mut size: (u16, u16),
) -> Result<(), String> {
    use crate::ws::{Op, frame};
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    // Raw, so every key goes to the far side as typed — Ctrl+C included,
    // which is the agent's to see and not this bridge's to die of.
    let _raw = Raw(crossterm::terminal::enable_raw_mode().is_ok());
    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut said = std::time::Instant::now();
    let mut buf = [0u8; 16 * 1024];
    let mut incoming = Vec::new();
    // Anything that arrived with the handshake's answer.
    let mut deliver = |decoder: &mut crate::ws::Decoder,
                       up: &mut crate::peer::Upstream|
     -> Result<bool, String> {
        while let Some((op, payload)) = decoder.frame()? {
            match op {
                Op::Close => return Ok(false),
                Op::Ping => up
                    .write_all(&frame(Op::Pong, &payload, true))
                    .map_err(|e| e.to_string())?,
                Op::Pong => {}
                Op::Text | Op::Binary | Op::Other(_) => {
                    stdout.write_all(&payload).map_err(|e| e.to_string())?;
                }
            }
        }
        stdout.flush().map_err(|e| e.to_string())?;
        Ok(true)
    };
    if !deliver(decoder, up)? {
        return Ok(());
    }
    loop {
        let mut fds = [
            libc::pollfd {
                fd: stdin.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: up.socket().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: two valid pollfds on fds that outlive the call.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, 1000) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(std::io::Error::last_os_error().to_string());
        }
        // A resize is polled for rather than caught: SIGWINCH would need a
        // handler for one number that a second's lag does not hurt.
        if let Ok(now) = crossterm::terminal::size()
            && now != size
        {
            size = now;
            let resize = crate::ws::resize_message(now.0, now.1);
            up.write_all(&frame(Op::Text, resize.as_bytes(), true))
                .map_err(|e| e.to_string())?;
            said = std::time::Instant::now();
        }
        if said.elapsed() >= KEEPALIVE {
            up.write_all(&frame(Op::Ping, b"", true))
                .map_err(|e| e.to_string())?;
            said = std::time::Instant::now();
        }
        if fds[0].revents != 0 {
            match stdin.read(&mut buf) {
                Ok(n) if n > 0 => {
                    up.write_all(&frame(Op::Binary, &buf[..n], true))
                        .map_err(|e| e.to_string())?;
                    said = std::time::Instant::now();
                }
                // This terminal went away: the far side's attach is detached
                // by the socket closing, and the agent there carries on.
                _ => {
                    let _ = up.write_all(&frame(Op::Close, b"", true));
                    return Ok(());
                }
            }
        }
        if fds[1].revents != 0 {
            incoming.clear();
            let open = up.read_ready(&mut incoming).map_err(|e| e.to_string())?;
            decoder.push(&incoming);
            if !deliver(decoder, up)? || !open {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sibling(machine: &str) -> Reach {
        Reach::Peer(std::sync::Arc::new(crate::peer::Peer::fixed(
            machine,
            "http://127.0.0.1:9",
            "made-up",
        )))
    }

    fn host(target: &str, command: &str) -> Host {
        Host {
            target: target.into(),
            command: command.into(),
        }
    }

    #[test]
    fn a_session_id_is_letters_digits_and_dashes() {
        for good in [
            "4ebf1ab4-2ef8-4fb2-a7d5-d445b5026dc9",
            "rollout-2025-01-01T10-00-00-4ebf1ab4",
            "ses_abc123",
            "_pid_4242",
        ] {
            assert!(id_ok(good), "{good}");
        }
        for bad in [
            "",
            "-oProxyCommand=x",
            "--check",
            "..",
            "a/b",
            "a b",
            "$(touch /tmp/x)",
            "a;b",
            "a'b",
            "a\nb",
        ] {
            assert!(!id_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_hostile_host_or_id_builds_nothing() {
        let exe = Path::new("/usr/bin/cctop");
        let ssh = Reach::Ssh(host("-oProxyCommand=touch /tmp/pwned", "cctop"));
        assert!(argv(&ssh, exe, "claude", "abc", "/w").is_err());
        let peer = sibling("-x");
        assert!(argv(&peer, exe, "claude", "abc", "/w").is_err());
        let fine = Reach::Ssh(host("box", "cctop"));
        assert!(argv(&fine, exe, "claude", "$(reboot)", "/w").is_err());
        assert!(argv(&fine, exe, "nonsense", "abc", "/w").is_err());
    }

    #[test]
    fn a_peer_tab_runs_this_cctops_bridge() {
        let got = argv(
            &sibling("computer36"),
            Path::new("/usr/bin/cctop"),
            "claude",
            "4ebf1ab4",
            "/srv/x",
        )
        .expect("argv");
        assert_eq!(
            got,
            [
                "/usr/bin/cctop",
                "mux",
                "reach",
                "computer36",
                "claude",
                "4ebf1ab4"
            ]
        );
    }

    /// The ssh line hands the far shell one script and four arguments; the
    /// path, however hostile, is an argument. Run here with a fake `cctop`
    /// that records what it was asked, the far side is checked end to end.
    #[test]
    fn the_ssh_line_runs_mux_open_and_falls_back_to_a_shell() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = dir.path().join("log");
        let fake = dir.path().join("cctop");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\nprintf '%s|' \"$@\" >> {}\necho >> {}\n",
                log.display(),
                log.display()
            ),
        )
        .expect("fake");
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");
        let hostile = "/w/$(touch pwned)'; touch pwned2";
        let line = ssh_argv(
            &host("box", &fake.to_string_lossy()),
            "claude",
            "abc-1",
            hostile,
        )
        .expect("argv");
        assert_eq!(line[..2], ["ssh", "-t"]);
        let remote = line.last().expect("command");
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(remote)
            .current_dir(dir.path())
            .output()
            .expect("sh");
        assert!(out.status.success(), "{out:?}");
        let said = std::fs::read_to_string(&log).expect("log");
        assert_eq!(
            said,
            format!("mux|open|--check|\nmux|open|claude|abc-1|{hostile}|\n")
        );
        assert!(!dir.path().join("pwned").exists());
        assert!(!dir.path().join("pwned2").exists());

        // A cctop that does not know `mux open` — here, one that fails every
        // command — gets the message and a shell in the project instead.
        let project = dir.path().join("proj");
        std::fs::create_dir(&project).expect("mkdir");
        let shell = dir.path().join("fake-shell");
        std::fs::write(&shell, "#!/bin/sh\npwd\n").expect("fake shell");
        std::fs::set_permissions(&shell, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");
        let line = ssh_argv(
            &host("box", "false"),
            "claude",
            "abc-1",
            &project.to_string_lossy(),
        )
        .expect("argv");
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(line.last().expect("command"))
            .env("SHELL", &shell)
            .output()
            .expect("sh");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("cctop --update"), "{err}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            project.to_string_lossy()
        );
    }

    #[test]
    fn the_far_side_attaches_resumes_or_explains() {
        let mut s = Session::new(crate::pricing::Provider::Claude, "abc".into());
        let resumed = crate::rmux::name_for_session("claude", "abc");

        // Gone: a shell, and why.
        assert!(matches!(
            decide(None, "abc", |_| None, |_| false),
            Step::Shell(why) if why.contains("abc")
        ));
        // Ended: resumed.
        assert_eq!(decide(Some(&s), "abc", |_| None, |_| false), Step::Resume);
        // Already resumed by some cctop: attached.
        assert_eq!(
            decide(Some(&s), "abc", |_| None, |n| n == resumed),
            Step::Attach(resumed.clone())
        );
        // Live in a plain terminal: no takeover.
        s.inferred_running = true;
        assert!(matches!(
            decide(Some(&s), "abc", |_| Some("x".into()), |_| false),
            Step::Shell(why) if why.contains("did not start")
        ));
    }

    #[test]
    fn a_refusal_from_an_old_sibling_says_to_update_it() {
        let old = refusal("pc", 404, "404 Not Found: no such page\n");
        assert!(old.contains("cctop --update"), "{old}");
        let gone = refusal(
            "pc",
            404,
            "404 Not Found: no such session on this machine\n",
        );
        assert!(gone.contains("no such session"), "{gone}");
    }
}
