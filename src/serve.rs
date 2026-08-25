//! The read-only web dashboard, and the tunnel that can put it on the internet.
//!
//! Everything else in cctop answers "what is happening on this machine" to
//! someone sitting at this machine. This answers it to a phone. The data is the
//! same data `--json` prints — one page polls it and draws a table — so nothing
//! here knows anything about sessions beyond asking [`crate::cli`] for the JSON.
//!
//! Three decisions worth the words:
//!
//! **The snapshot is built on a timer, not per request.** A full walk stats
//! every transcript on the machine, and a public URL is something anyone can
//! hammer. One background thread refreshes on the same interval the UI uses and
//! every request serves whatever string it last produced, so the cost of being
//! visible is fixed rather than proportional to who is looking.
//!
//! **The token is the whole of the authentication, and it lives in the path.**
//! A quick tunnel hands out a URL and nothing else — no headers to set, no
//! login to type on a phone. So the URL *is* the credential: `/<token>/` serves
//! the page, everything else is a 404 that says nothing. `--tunnel` refuses to
//! start without one, because the alternative is publishing every project path,
//! title and account email on this machine to whoever guesses the hostname.
//!
//! **It is read-only unless you say otherwise.** Plain `--serve` has no route
//! that starts, stops, or types at anything — the line [`mcp`](crate::mcp)
//! draws, for the same reason. `--allow-input` adds exactly one that does, and
//! it is a flag rather than a default because typing at a coding agent is
//! running commands as whoever started it: with it on, the token stops guarding
//! a view of the machine and starts guarding a way into it.

use crate::cli::Args;
use crate::loader::Loader;
use serde_json::json;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The dashboard, inlined into the binary — a tunnel URL that needed a second
/// request for a stylesheet would be a second thing to get wrong.
const PAGE: &str = include_str!("serve.html");

/// How long between full walks of every provider's session directory.
///
/// Between them the refresh thread only re-reads the rows that are moving, the
/// same bargain [`Loader::refresh_live`] exists for. New sessions are rare
/// compared to how often a running one changes, and a minute is well under how
/// long anyone looks at a dashboard before wondering why it is empty.
const FULL_WALK: Duration = Duration::from_secs(60);

/// A request line plus headers, past which the connection is not a browser.
const MAX_HEADER_BYTES: usize = 8 * 1024;

/// How many sessions the dashboard is given, most recently active first.
///
/// The same problem [`mcp`](crate::mcp) caps for, and for a sharper reason: a
/// machine that has run agents for months has a thousand dead sessions, and the
/// whole list came to 1.6MB here — served every two seconds, to a phone, over a
/// tunnel. A hundred is more than anyone scrolls and two orders of magnitude
/// less data.
const LIMIT: usize = 100;

/// Longest line `--allow-input` will type. Past any real prompt, and nowhere
/// near enough to matter to the pty on the other end.
const MAX_LINE: usize = 2_000;

/// Ceiling on a request body, so `Content-Length` cannot ask for an allocation.
const MAX_BODY_BYTES: usize = 8 * 1024;

/// What the refresh thread publishes and the request handlers read.
///
/// The pid map is only consulted by `--allow-input`, but it is built either way:
/// it comes free from sessions already in hand, and a route that had to walk
/// processes itself would be a second answer to a question the loader already
/// answers.
#[derive(Default)]
struct Snapshot {
    json: String,
    /// Session id → the live agent root, for `POST /send`. A session absent
    /// here is not running.
    pids: HashMap<String, u32>,
}

/// Serve the dashboard until killed, optionally behind a trycloudflare URL.
pub fn run(args: &Args) -> anyhow::Result<()> {
    // A dashboard with no prices on it is not worth serving, and unlike the TUI
    // there is no second frame to correct — the page shows whatever the first
    // snapshot said until the next tick.
    crate::pricing::refresh_pricing_blocking();

    let token = match (&args.token, args.tunnel) {
        (Some(t), _) if t.is_empty() => anyhow::bail!("--token cannot be empty"),
        (Some(t), _) => Some(t.clone()),
        // See the module docs: a public URL without one publishes everything.
        (None, true) => Some(random_token()),
        (None, false) => None,
    };

    // 127.0.0.1, never 0.0.0.0: the tunnel connects from this machine, so
    // binding wider would expose the dashboard to the local network as well —
    // which is a second decision, and not the one that was asked for.
    let listener = TcpListener::bind(("127.0.0.1", args.port))
        .map_err(|e| anyhow::anyhow!("cannot listen on 127.0.0.1:{}: {e}", args.port))?;
    let port = listener.local_addr()?.port();

    let snapshot = spawn_refresh(args.plan, Duration::from_secs_f64(args.delay));

    // Held for the life of the process: dropping it unregisters from the edge.
    let _tunnel = match args.tunnel {
        true => Some(tunnel::start(port)?),
        false => None,
    };
    let base = match &_tunnel {
        Some(t) => t.url.clone(),
        None => format!("http://127.0.0.1:{port}"),
    };
    match &token {
        Some(t) => eprintln!("cctop dashboard: {base}/{t}/"),
        None => eprintln!("cctop dashboard: {base}/"),
    }
    match (args.allow_input, args.tunnel) {
        // Worth the shouting: typing at a coding agent is running commands as
        // the person who started it, so this URL is not a view of the machine
        // any more — it is a way in.
        (true, true) => eprintln!(
            "!! --allow-input: anyone with this URL can type into your agents, \
             which means run commands as you. The URL is the only thing \
             stopping them."
        ),
        (true, false) => eprintln!("--allow-input: this page can type into running sessions."),
        _ => eprintln!("Read-only. Ctrl-C to stop."),
    }

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let snapshot = Arc::clone(&snapshot);
        let token = token.clone();
        // A thread per connection: browsers open two or three and hold them.
        // ponytail: no pool and no cap — the listener is on loopback and the
        // work per request is copying one already-built string. A cap belongs
        // here the day a route does real work.
        let allow_input = args.allow_input;
        std::thread::spawn(move || {
            let _ = handle(stream, token.as_deref(), allow_input, &snapshot);
        });
    }
    Ok(())
}

/// Keep one JSON snapshot up to date on a background thread.
///
/// Returns the shared cell the request handlers read; the thread outlives them
/// all and is never joined, because the only way out of [`run`] is the process
/// ending.
fn spawn_refresh(plan: crate::pricing::Plan, period: Duration) -> Arc<Mutex<Snapshot>> {
    let snapshot = Arc::new(Mutex::new(Snapshot {
        json: "[]".into(),
        ..Snapshot::default()
    }));
    let out = Arc::clone(&snapshot);
    std::thread::spawn(move || {
        let mut loader = Loader::new();
        let mut sessions = loader.load(plan);
        let mut walked = Instant::now();
        loop {
            // The full list stays behind, because `refresh_live` needs it and
            // discovery is what a full walk is for; only the slice the page can
            // use is serialised.
            let mut recent: Vec<_> = sessions.clone();
            recent.sort_by(|a, b| b.last_active.cmp(&a.last_active));
            recent.truncate(LIMIT);
            if let Ok(json) = crate::cli::sessions_json(&recent, plan, &loader, false) {
                let pids = live_pids(&recent);
                *snapshot.lock().unwrap() = Snapshot { json, pids };
            }
            loader.store().save();
            std::thread::sleep(period);

            if walked.elapsed() >= FULL_WALK {
                sessions = loader.load(plan);
                walked = Instant::now();
            } else {
                loader.refresh_live(plan, &mut sessions);
            }
        }
    });
    out
}

/// Session id to the process a typed line would reach.
///
/// Built from the same slice that becomes the payload, so what the page can see
/// and what a request can name are the same set by construction. A session with
/// no live root — stopped, or a ghost the process sweep is still holding — is
/// simply absent, which is what turns `POST /send` into a 404 rather than a
/// write to a pid that has been recycled.
fn live_pids(sessions: &[crate::session::Session]) -> HashMap<String, u32> {
    sessions
        .iter()
        .filter_map(|s| crate::ui::session_root_pid(s).map(|pid| (s.session_id.clone(), pid)))
        .collect()
}

/// Answer one request. Errors are the client hanging up, and mean nothing here.
fn handle(
    mut stream: TcpStream,
    token: Option<&str>,
    allow_input: bool,
    snapshot: &Mutex<Snapshot>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    // The reader outlives the head on purpose: `BufReader` may already have
    // pulled the first bytes of a POST body into its buffer while reading the
    // headers, so the body has to be taken from this one and not a fresh one.
    let mut reader = BufReader::new(stream.try_clone()?);
    let Some(head) = read_head(&mut reader)? else {
        return respond(&mut stream, "400 Bad Request", "text/plain", b"bad request");
    };
    let path = head.path.clone();

    // Strip the token prefix, or refuse. Everything below sees the same paths
    // whether or not a token is in play.
    let rest = match token {
        None => path.as_str(),
        Some(t) => {
            let tail = path.strip_prefix('/').unwrap_or(&path);
            match tail.split_once('/') {
                Some((given, rest)) if constant_time_eq(given, t) => rest,
                // `/<token>` with no trailing slash: the page's relative fetch
                // of `sessions.json` would resolve to the root and 404, so send
                // the browser to the form that works rather than serving a
                // page that cannot load its own data.
                None if constant_time_eq(tail, t) => {
                    let head = format!(
                        "HTTP/1.1 301 Moved Permanently\r\nLocation: /{tail}/\r\n\
                         Content-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    return stream.write_all(head.as_bytes());
                }
                // Says nothing about whether the token was wrong or the path
                // was: a 401 here would turn the URL into an oracle for
                // guessing the token one prefix at a time.
                _ => return not_found(&mut stream),
            }
        }
    };

    match (head.method.as_str(), rest.trim_start_matches('/')) {
        ("GET", "" | "index.html") => respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            PAGE.as_bytes(),
        ),
        ("GET", "sessions.json") => {
            let body = snapshot.lock().unwrap().json.clone();
            // How the page learns whether to draw a send box. A header on the
            // poll it already makes, rather than a second route or a page
            // templated per run — and it cannot be wrong, because the same
            // flag decides both this and whether the route below exists.
            let extra = match allow_input {
                true => "X-Cctop-Input: on\r\n",
                false => "",
            };
            match head.gzip {
                // ~10x on this payload, and `flate2` is already in the tree for
                // the updater's tarballs — so the mobile connection this exists
                // for pays 30KB a poll instead of 130KB.
                true => compressed(&mut stream, body.as_bytes(), extra),
                false => json_response(&mut stream, "200 OK", body.as_bytes(), extra),
            }
        }
        // Absent, not forbidden, when the flag is off: a dashboard that answers
        // 403 here advertises that some other cctop somewhere would have typed.
        ("POST", "send") if allow_input => {
            let body = read_body(&mut reader, head.length)?;
            let (status, reply) = send_line(&body, snapshot);
            json_response(&mut stream, status, reply.as_bytes(), "")
        }
        _ => not_found(&mut stream),
    }
}

/// What a `POST /send` carries: which session, and the line to type at it.
#[derive(serde::Deserialize)]
struct SendRequest {
    id: String,
    text: String,
}

/// Type one line into a running session, and say what happened.
///
/// The validation is the interesting part. This is the only route that does
/// anything, and what it does is hand text to a coding agent — which is a shell
/// by another name. So the line is checked before a pid is even looked up:
///
/// * **One line, and only one.** [`crate::inject::send_line`] presses Enter
///   itself; a `\n` in the middle would submit early and turn one request into
///   several commands. Rejecting them keeps a request and a command the same
///   thing.
/// * **No other control characters.** Escape would drive the agent's TUI rather
///   than type at it. Tab survives because it is ordinary in a typed line.
/// * **Bounded.** [`MAX_LINE`] is far past any real prompt and far short of
///   what would sit in the pty buffer.
///
/// A session that is not in the snapshot's pid map is not running, or is running
/// somewhere cctop cannot reach — either way there is nothing to type into, and
/// the 404 does not distinguish an unknown id from a stopped one.
fn send_line(body: &[u8], snapshot: &Mutex<Snapshot>) -> (&'static str, String) {
    let fail = |status, msg: &str| (status, json!({ "ok": false, "error": msg }).to_string());

    let Ok(req) = serde_json::from_slice::<SendRequest>(body) else {
        return fail("400 Bad Request", "expected {\"id\": …, \"text\": …}");
    };
    let text = req.text.trim_end_matches(' ');
    if text.is_empty() {
        return fail("400 Bad Request", "nothing to send");
    }
    if text.chars().count() > MAX_LINE {
        return fail("400 Bad Request", "line too long");
    }
    if text.contains(['\r', '\n']) {
        return fail("400 Bad Request", "one line per request");
    }
    if text.chars().any(|c| c.is_control() && c != '\t') {
        return fail("400 Bad Request", "control characters are not typed");
    }

    let Some(pid) = snapshot.lock().unwrap().pids.get(&req.id).copied() else {
        return fail("404 Not Found", "no running session with that id");
    };

    // The same call and the same failures the `s` key gets — including "this
    // session was not started in a way anything can type into", which is a
    // property of the session rather than of the dashboard.
    match crate::inject::send_line(pid, text) {
        Ok(()) => ("200 OK", json!({ "ok": true, "pid": pid }).to_string()),
        Err(e) => (
            "503 Service Unavailable",
            json!({ "ok": false, "error": e }).to_string(),
        ),
    }
}

/// Read exactly `length` bytes of body, refusing anything implausible.
///
/// The cap is what keeps a `Content-Length` header from being an allocation
/// request: the only body this server accepts is a line of text.
fn read_body(reader: &mut impl BufRead, length: usize) -> std::io::Result<Vec<u8>> {
    let mut body = vec![0u8; length.min(MAX_BODY_BYTES)];
    reader.read_exact(&mut body)?;
    Ok(body)
}

/// The parts of a request this server routes on.
struct Head {
    method: String,
    path: String,
    gzip: bool,
    length: usize,
}

/// Read the request line and headers.
///
/// Anything but GET and POST is refused here rather than in the router: the two
/// verbs the dashboard has are the two it reads a body for, and a method it does
/// not know is one whose framing it cannot trust.
fn read_head(reader: &mut impl BufRead) -> std::io::Result<Option<Head>> {
    let mut line = String::new();
    let mut read = reader.read_line(&mut line)?;

    let (method, path) = match line.split_whitespace().collect::<Vec<_>>().as_slice() {
        [verb @ ("GET" | "POST"), path, ..] => (
            verb.to_string(),
            // The dashboard takes no query parameters, and a fragment never
            // reaches the server — so anything after `?` is noise to drop
            // rather than something to route on.
            path.split(['?', '#']).next().unwrap_or("/").to_string(),
        ),
        _ => return Ok(None),
    };

    // The headers have to be drained whether or not they are wanted: a browser
    // does not consider the request sent until they are written, and a socket
    // nobody reads stalls it.
    let mut gzip = false;
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        read += n;
        if n == 0 || header == "\r\n" || header == "\n" || read > MAX_HEADER_BYTES {
            break;
        }
        // Substring, not a parse of the q-values: the only question asked is
        // whether gzip is allowed at all, and every browser that lists it means
        // yes. A client that spelled out `gzip;q=0` gets it anyway — and can
        // decode it, because it named the encoding.
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("accept-encoding") {
                gzip = value.to_ascii_lowercase().contains("gzip");
            } else if name.eq_ignore_ascii_case("content-length") {
                // An unparsable length reads as no body, which `read_body` then
                // turns into an empty one and the router rejects. Nothing here
                // trusts the number beyond `MAX_BODY_BYTES` anyway.
                length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    Ok(Some(Head {
        method,
        path,
        gzip,
        length,
    }))
}

fn respond(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    write_response(stream, status, ctype, body, "")
}

/// [`respond`] for the JSON routes, which are the ones that carry extra headers.
fn json_response(
    stream: &mut TcpStream,
    status: &str,
    body: &[u8],
    extra: &str,
) -> std::io::Result<()> {
    write_response(stream, status, "application/json", body, extra)
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    ctype: &str,
    body: &[u8],
    extra: &str,
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         {extra}Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// The same as [`respond`], gzipped. Falls back to plain on a compressor error
/// rather than failing the request over a size optimisation.
fn compressed(stream: &mut TcpStream, body: &[u8], extra: &str) -> std::io::Result<()> {
    use flate2::{Compression, write::GzEncoder};
    let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
    let packed = enc.write_all(body).and_then(|()| enc.finish());
    let Ok(packed) = packed else {
        return json_response(stream, "200 OK", body, extra);
    };
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n{extra}Connection: close\r\n\r\n",
        packed.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&packed)?;
    stream.flush()
}

fn not_found(stream: &mut TcpStream) -> std::io::Result<()> {
    respond(stream, "404 Not Found", "text/plain", b"not found")
}

/// Compare without letting the time taken say how much of the token was right.
///
/// The tunnel puts this on the public internet, where the timing signal is
/// buried under the network — but the cost of not being able to argue about
/// that is four lines.
fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// A URL-safe token with the OS's randomness behind it.
///
/// `RandomState` is seeded from the OS per process and is what every `HashMap`
/// in the program already relies on not being predictable, so it is the
/// strongest source in the tree without adding one.
///
/// ponytail: 128 bits from two SipHash outputs over the same secret keys, not a
/// CSPRNG. Ample for a URL that lives as long as one `--serve` run; if these
/// tokens ever need to outlive the process, take a real random crate.
fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut token = String::with_capacity(32);
    for salt in 0..2u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(salt);
        h.write_u32(std::process::id());
        token.push_str(&format!("{:016x}", h.finish()));
    }
    token
}

/// Putting the local port on `*.trycloudflare.com`.
///
/// A quick tunnel needs no Cloudflare account and no DNS: the client asks
/// `api.trycloudflare.com` for a hostname and a secret, dials the argotunnel
/// edge, and registers. It stops existing when the process does. That is the
/// right shape for "let me look at this from my phone for ten minutes" and the
/// wrong shape for anything permanent — the URL changes every run, which is
/// also why it is not a substitute for the token.
///
/// The client is [`cloudflare_quick_tunnel`], which speaks that protocol itself
/// — QUIC to the edge and capnp-RPC over it — rather than shelling out to
/// `cloudflared` and reading a URL off its stderr. So `--tunnel` needs nothing
/// installed and cctop stays one binary, which is the whole reason it ships as
/// one.
///
/// The consequence worth knowing: the tunnel's data path is *inside this
/// process*. Every request from the internet arrives as a QUIC stream, gets
/// proxied to 127.0.0.1 by a task on the runtime below, and lands on the same
/// listener a local browser would use.
mod tunnel {
    use cloudflare_quick_tunnel::{QuickTunnelHandle, QuickTunnelManager};

    /// The live tunnel. Dropping it unregisters and closes, so the caller holds
    /// it for as long as the dashboard is meant to be reachable.
    pub struct Tunnel {
        pub url: String,
        /// Declared before the runtime so it drops first: the handle signals
        /// the edge reactors to wind down, which needs the runtime they are
        /// still running on.
        _handle: QuickTunnelHandle,
        /// Not a handle to park — this *is* the tunnel. The tasks it drives
        /// accept the edge's streams and proxy them; if it stops turning, the
        /// public URL stops answering.
        _runtime: tokio::runtime::Runtime,
    }

    pub fn start(port: u16) -> anyhow::Result<Tunnel> {
        // Two workers, because the proxying happens here rather than in
        // somebody else's process: one accepts streams while the other is
        // still writing a response.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        eprintln!("Opening trycloudflare tunnel…");
        // ponytail: the crate's default HA connection count, untuned. It trades
        // a second QUIC connection for masking a single-POP reconnect, and the
        // page reports a gap of its own anyway.
        let handle = runtime
            .block_on(QuickTunnelManager::new(port).start())
            .map_err(|e| anyhow::anyhow!("{e}\nDrop --tunnel to serve on localhost only."))?;
        Ok(Tunnel {
            url: handle.url.clone(),
            _handle: handle,
            _runtime: runtime,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_compare_is_exact() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "ab"));
        assert!(!constant_time_eq("", "a"));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn tokens_differ_and_are_hex() {
        let (a, b) = (random_token(), random_token());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// The routing decisions, end to end over a real socket: the page, the
    /// data, a wrong token, and a path outside the token's prefix.
    #[test]
    fn routes_are_gated_by_the_token() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().unwrap().port();
        let snapshot = Arc::new(Mutex::new(Snapshot {
            json: r#"[{"provider":"claude"}]"#.into(),
            pids: HashMap::new(),
        }));

        std::thread::spawn(move || {
            for stream in listener.incoming().take(8) {
                let Ok(stream) = stream else { continue };
                let snapshot = Arc::clone(&snapshot);
                std::thread::spawn(move || {
                    // `false`: the write route is absent unless --allow-input,
                    // which is what the last assertion below checks.
                    let _ = handle(stream, Some("s3cret"), false, &snapshot);
                });
            }
        });

        let send = |req: String| -> String {
            use std::io::Read;
            let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
            s.write_all(req.as_bytes()).unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        let get = |path: &str| send(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n"));
        let post = |path: &str, body: &str| {
            send(format!(
                "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ))
        };

        assert!(get("/s3cret/").contains("200 OK"));
        assert!(get("/s3cret/sessions.json").contains(r#"[{"provider":"claude"}]"#));
        assert!(get("/s3cret").contains("301"));
        // A wrong token and an unprefixed path are the same answer on purpose.
        assert!(get("/wrong/sessions.json").contains("404"));
        assert!(get("/sessions.json").contains("404"));
        // Right token, right path, but the flag is off: the route is not there.
        assert!(post("/s3cret/send", r#"{"id":"a","text":"hi"}"#).contains("404"));
    }

    /// Every way a line can be refused before a pid is looked up. An empty pid
    /// map stands in for "nothing is running", which is what the last case
    /// checks — and what keeps this test from typing at anything real.
    #[test]
    fn a_line_is_vetted_before_any_session_is_touched() {
        let snapshot = Mutex::new(Snapshot {
            json: "[]".into(),
            pids: HashMap::new(),
        });
        let post = |body: &str| send_line(body.as_bytes(), &snapshot);

        assert_eq!(post("not json").0, "400 Bad Request");
        assert_eq!(post(r#"{"id":"a","text":"   "}"#).0, "400 Bad Request");
        // Enter is `send_line`'s to press, so a line may not contain one.
        assert!(
            post(r#"{"id":"a","text":"ls\nrm -rf /"}"#)
                .1
                .contains("one line")
        );
        // Escape would drive the agent's UI instead of typing at it.
        assert!(
            post(r#"{"id":"a","text":"a\u001b[Bb"}"#)
                .1
                .contains("control")
        );
        let long = format!(r#"{{"id":"a","text":"{}"}}"#, "x".repeat(MAX_LINE + 1));
        assert!(post(&long).1.contains("too long"));
        // Well-formed, but nothing is running under that id.
        assert_eq!(post(r#"{"id":"ghost","text":"hello"}"#).0, "404 Not Found");
    }

    /// Only a session with a live agent root is addressable, and it is keyed by
    /// the same id the payload carries.
    #[test]
    fn only_a_session_with_a_live_root_can_be_addressed() {
        use crate::proc::{ProcEntry, ProcInfo};
        use crate::session::Session;

        let entry = |pid, is_root, ghost| ProcEntry {
            pid,
            is_root,
            ghost,
            cpu: 0.0,
            memory: 0,
            args: String::new(),
        };

        let mut live = Session::new(crate::pricing::Provider::Claude, "live".into());
        live.process = Some(ProcInfo {
            // The child and the ghost are both wrong answers to "who would the
            // line reach"; only the live root is.
            process_list: vec![
                entry(2, false, false),
                entry(9, true, true),
                entry(7, true, false),
            ],
            ..ProcInfo::default()
        });
        let stopped = Session::new(crate::pricing::Provider::Claude, "stopped".into());

        let pids = live_pids(&[live, stopped]);
        assert_eq!(pids.get("live"), Some(&7));
        assert!(
            !pids.contains_key("stopped"),
            "a stopped session is unaddressable"
        );
    }

    /// A tab is ordinary in a typed line; the check must not sweep it up with
    /// the escape sequences it is there to stop.
    #[test]
    fn a_tab_is_not_a_control_character_worth_refusing() {
        let snapshot = Mutex::new(Snapshot::default());
        let (status, _) = send_line(br#"{"id":"ghost","text":"a\tb"}"#, &snapshot);
        assert_eq!(
            status, "404 Not Found",
            "a tab should reach the pid lookup, not be refused as a control char"
        );
    }
}
