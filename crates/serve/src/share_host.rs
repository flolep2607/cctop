//! The server behind the account tunnel's share hostnames.
//!
//! A `W` link on a connected account reads `https://<share host>/#e=wss://
//! <share host>/share&t=…`: rmux's browser app and the socket it opens, both on
//! the one hostname. This is what answers that hostname, and the whole of what
//! it answers is:
//!
//! - rmux's static app — the pinned copy [`term`] serves for the page's frames,
//!   here at the root ([`term::share_file`]);
//! - a WebSocket to `/share`, relayed to rmux's share listener, whose own
//!   token, PIN and end-to-end encryption are the door — the same door the
//!   link had when it opened on `share.rmux.io`.
//!
//! Nothing else, and that is the point of it being a listener of its own
//! rather than a branch in the page's router: no route of the dashboard's is
//! reachable through it with any token, because there is no route here to
//! reach and no token is ever read. A share link travels — over chat, over
//! mail — and whoever ends up holding one must hold a terminal at most, never
//! the page. The page's server keeps a second lock beside this one: it refuses
//! any request whose `Host` is a share hostname
//! ([`cctop_core::tunnel::is_share_host`]), so a misrouted request is a 404
//! there as well.
//!
//! The tunnel lands share hostnames here ([`cctop_core::tunnel::start`]'s
//! `share_front`) and tells this front which port rmux listens on
//! ([`cctop_core::tunnel::share_upstream`]). One front for every share
//! hostname, a custom agent name included: which terminal a link opens is the
//! token in its fragment, not the hostname it arrived on.

use super::http::{self, Request};
use super::{Connection, MAX_CONNECTIONS, relay_socket, term};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// What a share hostname answers for anything that is not the app or its
/// socket. One sentence for every refusal, so the answer says nothing about
/// which paths exist on the page's server.
pub const NOT_HERE: &str = "this address serves a shared terminal and nothing else";

/// The path rmux's listener takes the share socket on, as rmux writes it into
/// every link's `e=` endpoint.
const SOCKET: &str = "/share";

/// Where the socket goes: rmux's listener port, when a share is up.
type Upstream = Arc<dyn Fn() -> Option<u16> + Send + Sync>;

/// A running share front. Dropping it stops the accept loop.
pub struct Front {
    port: u16,
    running: Arc<AtomicBool>,
}

impl Front {
    /// Listen on a loopback port of the system's choosing, relaying the
    /// socket to whatever port `upstream` names when one arrives.
    pub fn start(upstream: Upstream) -> anyhow::Result<Front> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let running = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&running);
        std::thread::Builder::new()
            .name("cctop-share-front".into())
            .spawn(move || accept_loop(listener, upstream, flag))?;
        Ok(Front { port, running })
    }

    /// The loopback port the tunnel routes share hostnames to.
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Front {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        // Wakes the loop parked in `accept`, as `Serving`'s drop does.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// Answer connections until `running` goes false, under the same cap as the
/// page's server and with its own count: a share link hammered from outside
/// must not take the page's connections with it.
fn accept_loop(listener: TcpListener, upstream: Upstream, running: Arc<AtomicBool>) {
    let live = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        if !running.load(Ordering::Relaxed) {
            return;
        }
        let Ok(mut stream) = stream else { continue };
        if live.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
            http::respond_error(&mut stream, None, 503, "too many open connections");
            continue;
        }
        let slot = Connection::take(&live);
        let upstream = Arc::clone(&upstream);
        let _ = std::thread::Builder::new()
            .name("cctop-share".into())
            .spawn(move || {
                let _slot = slot;
                serve(&mut stream, &*upstream);
            });
    }
}

/// What a share hostname does with one request.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    /// A file of rmux's app.
    File,
    /// The share socket, to rmux's listener.
    Socket,
    /// Anything else.
    NotHere,
}

/// The whole routing table, as a function of the three things it looks at.
/// No token, cookie or header but the upgrade is consulted: there is nothing
/// a credential could open here.
fn answer(method: &str, path: &str, websocket: bool) -> Answer {
    match (method, websocket) {
        ("GET", true) if path == SOCKET => Answer::Socket,
        ("GET" | "HEAD", false) if term::share_file(path).is_some() => Answer::File,
        _ => Answer::NotHere,
    }
}

fn serve(stream: &mut TcpStream, upstream: &(dyn Fn() -> Option<u16> + Send + Sync)) {
    let request = match Request::parse(stream) {
        Ok(request) => request,
        Err((status, why)) => return http::respond_error(stream, None, status, why),
    };
    match answer(&request.method, &request.path, request.is_websocket()) {
        Answer::File => {
            let Some(file) = term::share_file(&request.path) else {
                return http::respond_error(stream, Some(&request), 404, NOT_HERE);
            };
            http::respond_packed(
                stream,
                &request,
                file.content_type,
                file.body,
                file.cache,
                "",
                Some(http::TERM_POLICY),
            );
        }
        Answer::Socket => match upstream() {
            Some(port) => relay_socket(stream, &request, port, SOCKET),
            None => http::respond_error(
                stream,
                Some(&request),
                502,
                "the terminal's share has closed",
            ),
        },
        Answer::NotHere => http::respond_error(stream, Some(&request), 404, NOT_HERE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};

    /// One request to `port`, and the whole raw response.
    fn ask(port: u16, request: &str) -> String {
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client.write_all(request.as_bytes()).unwrap();
        let mut raw = String::new();
        let _ = client.read_to_string(&mut raw);
        raw
    }

    fn status(raw: &str) -> &str {
        raw.lines().next().unwrap_or_default()
    }

    /// The property this module exists for: a share hostname serves rmux's app
    /// and its socket, and not one route of the dashboard's — with the page's
    /// real token presented every way the page accepts one. The same requests
    /// are sent to a real page server holding that token first, so the token
    /// is shown to open them there and nothing here.
    #[test]
    fn a_share_host_serves_no_dashboard_route_even_with_the_pages_token() {
        let serving = crate::start(crate::Options {
            port: 0,
            port_given: true,
            scan: false,
            ..crate::Options::default()
        })
        .unwrap();
        let token = serving.shared.token.clone();
        assert!(!token.is_empty());
        let front = Front::start(Arc::new(|| None)).unwrap();

        let credentials = [
            (format!("?t={token}"), String::new()),
            (String::new(), format!("Authorization: Bearer {token}\r\n")),
            (
                String::new(),
                format!("Cookie: {}={token}\r\n", crate::cookie_name(serving.port)),
            ),
        ];
        let dashboard = [
            "/api/sessions",
            "/api/config",
            "/api/tabs",
            "/api/analytics",
            "/api/events",
            "/favicon.svg",
            "/manifest.webmanifest",
            "/term/",
            "/rmux-ws/1/share",
            "/metrics",
        ];
        for path in dashboard {
            for (query, header) in &credentials {
                let request = format!("GET {path}{query} HTTP/1.1\r\nHost: x\r\n{header}\r\n");
                if path == "/api/sessions" || path == "/api/config" {
                    let page = ask(serving.port, &request);
                    assert!(status(&page).contains(" 200 "), "{path}: {}", status(&page));
                }
                let there = ask(front.port(), &request);
                assert!(
                    status(&there).contains(" 404 "),
                    "{path}: {}",
                    status(&there)
                );
                assert!(there.contains(NOT_HERE), "{path}: {there}");
                assert!(!there.contains(&token), "{path} echoed the token");
            }
        }
        // Not a page of the dashboard either, and no write of any kind.
        for (method, path) in [
            ("GET", "/session/abc"),
            ("GET", "/workspace"),
            ("POST", "/api/act/abc/prompt"),
            ("POST", "/"),
            ("PUT", "/share"),
        ] {
            let request = format!(
                "{method} {path}?t={token} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{{}}"
            );
            let there = ask(front.port(), &request);
            // The request parser refuses a method no route of cctop's takes
            // with 405 before any routing, which is as closed as a 404.
            let refused = [" 404 ", " 405 "]
                .iter()
                .any(|s| status(&there).contains(s));
            assert!(refused, "{method} {path}: {there}");
        }

        // What it does serve: rmux's app, at the root.
        let app = ask(front.port(), "GET / HTTP/1.1\r\nHost: x\r\n\r\n");
        assert!(status(&app).contains(" 200 "), "{app}");
        assert!(app.contains("/_astro/"), "not rmux's app: {app}");
        assert!(!app.contains("cctop-config"), "the dashboard's page: {app}");
        let named = std::str::from_utf8(term::share_file("/").unwrap().body.decoded())
            .unwrap()
            .split('"')
            .filter(|p| p.starts_with("/_astro/"))
            .map(String::from)
            .collect::<Vec<_>>();
        assert_eq!(named.len(), 2);
        for path in named {
            let file = ask(
                front.port(),
                &format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n"),
            );
            assert!(status(&file).contains(" 200 "), "{path}: {}", status(&file));
        }
        // The socket, with no share up, is a closed share — not a 404, and
        // not anything of the page's.
        let socket = ask(
            front.port(),
            "GET /share HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
        );
        assert!(status(&socket).contains(" 502 "), "{socket}");
    }

    #[test]
    fn only_the_app_and_its_socket_are_routed() {
        assert_eq!(answer("GET", "/", false), Answer::File);
        assert_eq!(answer("HEAD", "/", false), Answer::File);
        assert_eq!(answer("GET", "/crabs/lime-dark.svg", false), Answer::File);
        assert_eq!(answer("GET", "/share", true), Answer::Socket);
        // A socket anywhere else, or a plain GET of the socket's path, is not.
        assert_eq!(answer("GET", "/rmux-ws/1/share", true), Answer::NotHere);
        assert_eq!(answer("GET", "/", true), Answer::NotHere);
        assert_eq!(answer("GET", "/share", false), Answer::NotHere);
        assert_eq!(answer("POST", "/", false), Answer::NotHere);
        // The page's frame layout has no business here.
        assert_eq!(answer("GET", "/term/", false), Answer::NotHere);
        assert_eq!(answer("GET", "/term", false), Answer::NotHere);
        assert_eq!(answer("GET", "/api/sessions", false), Answer::NotHere);
    }

    /// The socket reaches rmux's listener as rmux expects it: the same path,
    /// the browser's `Origin` kept — rmux checks it against the link's
    /// frontend — and `Host` rewritten to the loopback it is. Then bytes flow
    /// both ways.
    #[test]
    fn the_socket_is_relayed_to_rmuxs_listener() {
        let rmux = TcpListener::bind("127.0.0.1:0").unwrap();
        let rmux_port = rmux.local_addr().unwrap().port();
        let front = Front::start(Arc::new(move || Some(rmux_port))).unwrap();
        let far = std::thread::spawn(move || {
            let (stream, _) = rmux.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut head = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                head.push_str(&line);
            }
            let mut stream = reader.into_inner();
            stream
                .write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\nfrom-rmux")
                .unwrap();
            let mut got = [0u8; 9];
            stream.read_exact(&mut got).unwrap();
            (head, got)
        });
        let mut client = TcpStream::connect(("127.0.0.1", front.port())).unwrap();
        client
            .write_all(
                b"GET /share HTTP/1.1\r\nHost: cctop-share.example.test\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nOrigin: https://cctop-share.example.test\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            )
            .unwrap();
        let mut answer = vec![0u8; 256];
        let mut seen = String::new();
        while !seen.contains("from-rmux") {
            let n = client.read(&mut answer).unwrap();
            assert_ne!(n, 0, "closed early: {seen}");
            seen.push_str(&String::from_utf8_lossy(&answer[..n]));
        }
        assert!(seen.starts_with("HTTP/1.1 101"), "{seen}");
        client.write_all(b"to-rmux!!").unwrap();
        let (head, got) = far.join().unwrap();
        assert!(head.starts_with("GET /share HTTP/1.1\r\n"), "{head}");
        assert!(
            head.contains(&format!("Host: 127.0.0.1:{rmux_port}")),
            "{head}"
        );
        assert!(
            head.contains("Origin: https://cctop-share.example.test"),
            "{head}"
        );
        assert_eq!(&got, b"to-rmux!!");
    }
}
