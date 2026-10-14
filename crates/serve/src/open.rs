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
//! Who may: a full link with actions on — the same line `relay_terminal`
//! draws for a tab's terminal, since typing into an agent is acting on it. A
//! sibling's signature is a full link (see the peer check in `handle`).

use crate::http::{self, Request};
use crate::{Access, Shared, current};
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
    let argv = open_argv(provider, id);
    // A server started by systemd has no TERM, and an attach with none draws
    // nothing a terminal can read.
    let env = match std::env::var_os("TERM") {
        Some(_) => Vec::new(),
        None => vec![("TERM".to_string(), "xterm-256color".to_string())],
    };
    let (mut child, master) = match cctop_core::shim::spawn_pty(&argv, size, &env) {
        Ok(spawned) => spawned,
        Err(error) => {
            return http::respond_error(stream, Some(request), 500, &error.to_string());
        }
    };
    let accept = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        ws::accept_key(key)
    );
    if stream.write_all(accept.as_bytes()).is_ok() {
        pump(stream, master);
    }
    // The pty's program is an rmux client, or a shell started for this
    // socket: ending it detaches, and the agent in rmux carries on.
    let _ = child.kill();
    let _ = child.wait();
}

const NO_SESSION: &str = "no such session on this machine";

/// What the pty runs: this binary's `mux open`.
#[cfg(not(test))]
fn open_argv(provider: &str, id: &str) -> Vec<String> {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "cctop".into());
    vec![exe, "mux".into(), "open".into(), provider.into(), id.into()]
}

/// Under test this binary is the test harness, which would read `mux open`
/// as test filters: a shell stands in, saying what it was asked and at what
/// size, then echoing a line back — enough to see both directions and the
/// resize arrive.
#[cfg(test)]
fn open_argv(provider: &str, id: &str) -> Vec<String> {
    vec![
        "sh".into(),
        "-c".into(),
        r#"echo "opened $0 $1 $(stty size)"; read line; echo "got $line"; read line; echo "now $(stty size)""#.into(),
        provider.into(),
        id.into(),
    ]
}

/// Pump the pty and the socket into each other until either closes.
fn pump(stream: &mut TcpStream, master: std::fs::File) {
    // A terminal sits quiet for minutes; the request deadlines would cut it.
    let _ = stream.set_read_timeout(None);
    let _ = stream.set_write_timeout(None);
    let mut pty_out = &master;
    let mut pty_in = &master;
    let mut decoder = ws::Decoder::default();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let mut fds = [
            libc::pollfd {
                fd: stream.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: two valid pollfds on fds both owned for this call.
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        if fds[0].revents != 0 {
            match stream.read(&mut buf) {
                Ok(n) if n > 0 => decoder.push(&buf[..n]),
                _ => return,
            }
            loop {
                match decoder.frame() {
                    Ok(Some((Op::Close, _))) => {
                        let _ = stream.write_all(&frame(Op::Close, b"", false));
                        return;
                    }
                    // Said, with the protocol-error code, so the tab shows why
                    // instead of closing as though the terminal had ended.
                    Err(why) => {
                        let mut payload = 1002u16.to_be_bytes().to_vec();
                        payload.extend(why.bytes().take(120));
                        let _ = stream.write_all(&frame(Op::Close, &payload, false));
                        return;
                    }
                    Ok(Some((Op::Ping, payload))) => {
                        if stream.write_all(&frame(Op::Pong, &payload, false)).is_err() {
                            return;
                        }
                    }
                    Ok(Some((Op::Pong, _))) => {}
                    // Text is the resize; nothing else is sent as text.
                    Ok(Some((Op::Text, text))) => {
                        if let Some((cols, rows)) = ws::parse_resize(&text) {
                            cctop_core::shim::set_pty_size(&master, cols, rows);
                        }
                    }
                    Ok(Some((Op::Binary | Op::Other(_), keys))) => {
                        if pty_in.write_all(&keys).is_err() {
                            return;
                        }
                    }
                    Ok(None) => break,
                }
            }
        }
        if fds[1].revents != 0 {
            match pty_out.read(&mut buf) {
                Ok(n) if n > 0 => {
                    if stream
                        .write_all(&frame(Op::Binary, &buf[..n], false))
                        .is_err()
                    {
                        return;
                    }
                }
                // EIO once the program on it has exited: the far tab is told
                // the terminal closed, and shows what it last said.
                _ => {
                    let _ = stream.write_all(&frame(Op::Close, b"", false));
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Snapshot;
    use crate::tests::{session, shared, status_of};
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
}
