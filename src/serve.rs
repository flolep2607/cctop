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
//! **A session cctop owns can be watched, not just counted.** `/t/<pid>` is
//! xterm.js over a WebSocket bridged to the shim socket [`attach`] already
//! speaks, so the browser sees the agent's real screen. Only sessions started
//! with `cctop run` have that socket, which is why only some rows offer it.
//!
//! **It is read-only unless you say otherwise.** Plain `--serve` has no route
//! that starts, stops, or types at anything — the line [`mcp`](crate::mcp)
//! draws, for the same reason. `--allow-input` adds exactly one that does, and
//! it is a flag rather than a default because typing at a coding agent is
//! running commands as whoever started it: with it on, the token stops guarding
//! a view of the machine and starts guarding a way into it.

use crate::cli::Args;
use crate::loader::Loader;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The dashboard, inlined into the binary — a tunnel URL that needed a second
/// request for a stylesheet would be a second thing to get wrong.
const PAGE: &str = include_str!("serve.html");

/// The live-terminal page. Separate from [`PAGE`] because it is a different
/// screen with a different job, and because it is the only one that pays for
/// xterm.js.
const TERMINAL_PAGE: &str = include_str!("serve_terminal.html");

/// xterm.js, vendored under `src/vendor/` — MIT, see `xterm.LICENSE` beside it.
///
/// Not inlined into the page the way the dashboard's CSS is: half a megabyte
/// would be re-sent on every navigation, where a route of its own is fetched
/// once and cached. It is the terminal emulator itself — the bytes cctop
/// forwards from the pty are raw ANSI, and something has to turn them into a
/// screen. Doing that server-side would mean shipping a rendered grid on every
/// repaint; ANSI is already the compact form of exactly that.
const XTERM_JS: &str = include_str!("vendor/xterm.js");
const XTERM_CSS: &str = include_str!("vendor/xterm.css");

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
    ///
    /// Not the same pid as a terminal's: this one is the agent, because
    /// [`crate::inject::send_line`] reaches it through tmux when it has to.
    /// A terminal needs the shim's socket, which is a different process.
    pids: HashMap<String, u32>,
    /// The agents a shim can show, already serialised. See [`terminals_json`].
    terminals: String,
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
        terminals: "[]".into(),
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
                *snapshot.lock().unwrap() = Snapshot {
                    json,
                    pids: live_pids(&recent),
                    terminals: terminals_json(),
                };
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

/// The agents a shim can hand over, as the terminal list the page draws.
///
/// Deliberately *not* derived from the session rows. A session knows about its
/// own process; the socket belongs to whatever the shim is holding, which for
/// anything started as `cctop claude` is the tmux client rather than the agent.
/// Matching one to the other means guessing, and guessing here produced a link
/// to a pid with no socket behind it — so the list is the sockets themselves,
/// exactly what `cctop attach` prints.
fn terminals_json() -> String {
    #[cfg(unix)]
    let rows: Vec<Value> = crate::attach::attachable()
        .into_iter()
        .map(|a| json!({ "pid": a.pid, "command": a.command, "cwd": a.cwd }))
        .collect();
    // No ptys, so no shim ever held one and the list is empty by construction.
    #[cfg(not(unix))]
    let rows: Vec<Value> = Vec::new();
    serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
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
        ("GET", "terminals.json") => {
            let body = snapshot.lock().unwrap().terminals.clone();
            json_response(&mut stream, "200 OK", body.as_bytes(), "")
        }

        // Version-pinned by the binary they came out of, so they never need
        // revalidating — unlike the page, which is `no-store`.
        ("GET", "xterm.js") => cached(&mut stream, "text/javascript", XTERM_JS, head.gzip),
        ("GET", "xterm.css") => cached(&mut stream, "text/css", XTERM_CSS, head.gzip),

        // `/t/<pid>` is the page; `/ws/<pid>` is what it opens. Splitting them
        // keeps the upgrade off any path a browser might navigate to.
        ("GET", p) if p.starts_with("t/") => respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            TERMINAL_PAGE.as_bytes(),
        ),
        ("GET", p) if p.starts_with("ws/") => {
            let pid = p.trim_start_matches("ws/").parse::<u32>().ok();
            match (pid, head.ws_key.as_deref()) {
                (Some(pid), Some(key)) => terminal(stream, reader, pid, key),
                // Not a websocket request, or not a pid. Either way there is
                // nothing here for an ordinary GET.
                _ => not_found(&mut stream),
            }
        }

        // Keystrokes and resizes for a live terminal. Over HTTP rather than
        // back up the WebSocket the screen arrives on — see `keys` for why.
        ("POST", p) if allow_input && p.starts_with("keys/") => {
            let body = read_body(&mut reader, head.length)?;
            match p.trim_start_matches("keys/").parse::<u32>() {
                Ok(pid) => typed(&mut stream, pid, &body),
                Err(_) => not_found(&mut stream),
            }
        }
        ("POST", p) if allow_input && p.starts_with("size/") => {
            let body = read_body(&mut reader, head.length)?;
            match p.trim_start_matches("size/").parse::<u32>() {
                Ok(pid) => resized(&mut stream, pid, &body),
                Err(_) => not_found(&mut stream),
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
    /// `Sec-WebSocket-Key`, present only on an upgrade request. Read solely by
    /// the terminal bridge, which is unix-only — there is no shim to bridge to
    /// anywhere else, so on Windows this is collected and never looked at.
    #[cfg_attr(not(unix), allow(dead_code))]
    ws_key: Option<String>,
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
    let mut ws_key = None;
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
            } else if name.eq_ignore_ascii_case("sec-websocket-key") {
                ws_key = Some(value.trim().to_string());
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
        ws_key,
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

/// A vendored asset: immutable, because it changes only when the binary does.
fn cached(stream: &mut TcpStream, ctype: &str, body: &str, gzip: bool) -> std::io::Result<()> {
    let extra = "Cache-Control: public, max-age=31536000, immutable\r\n";
    if !gzip {
        return write_response(stream, "200 OK", ctype, body.as_bytes(), extra);
    }
    use flate2::{Compression, write::GzEncoder};
    // `best`, not `fast`, unlike the poll payload: this is compressed once per
    // process and then cached by the browser for a year, so the CPU is spent
    // once and the saving is paid back on every cold load.
    let mut enc = GzEncoder::new(Vec::new(), Compression::best());
    let packed = enc.write_all(body.as_bytes()).and_then(|()| enc.finish());
    let Ok(packed) = packed else {
        return write_response(stream, "200 OK", ctype, body.as_bytes(), extra);
    };
    // 488KB of xterm.js becomes about 120KB, which is the difference between a
    // terminal that opens on a phone and one that appears to hang.
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Encoding: gzip\r\n\
         Content-Length: {}\r\n{extra}X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n",
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

/// Shim connections kept open for typing, one per agent.
///
/// Opening one costs a full screen replay the shim sends to every new watcher,
/// which is a few hundred kilobytes to throw away — per keystroke, if this were
/// opened per request. So the first key for an agent pays it and the rest ride
/// along.
#[cfg(unix)]
static TYPING: std::sync::LazyLock<Mutex<HashMap<u32, std::os::unix::net::UnixStream>>> =
    std::sync::LazyLock::new(Default::default);

/// Send an already-encoded shim frame to `pid`, reconnecting once if the socket
/// we kept has since died.
///
/// A shim that exited leaves its socket refusing connections rather than
/// vanishing, so a stale entry fails on write and the retry is what tells a
/// restarted agent apart from a gone one.
#[cfg(unix)]
fn to_shim(pid: u32, framed: &[u8]) -> bool {
    let mut open = lock(&TYPING);
    for attempt in 0..2 {
        if attempt == 1 || !open.contains_key(&pid) {
            open.remove(&pid);
            match crate::attach::connect(pid) {
                Some(sock) => {
                    open.insert(pid, sock);
                }
                None => return false,
            }
        }
        if let Some(sock) = open.get_mut(&pid)
            && sock.write_all(framed).and_then(|()| sock.flush()).is_ok()
        {
            return true;
        }
    }
    false
}

/// Type raw bytes into a live terminal.
///
/// **Why this is not the WebSocket the screen comes back on.** It should be,
/// and on localhost it works. Through a trycloudflare quick tunnel it does not:
/// the edge delivers the 101 and streams the agent's output back perfectly, but
/// bytes sent the other way after the upgrade never reach the origin. Verified
/// by pinging through the tunnel — no pong, and the connection stays open, so
/// nothing was rejected here; it simply never arrived.
///
/// A phone on a tunnel is the whole reason the terminal exists, so input takes
/// the path that demonstrably works everywhere. POSTs cost a round trip that a
/// frame would not, which is invisible next to how fast anyone types, and the
/// page coalesces whatever piles up while one is in flight.
///
/// ponytail: one input path, not two, even though the WebSocket would be better
/// where it works. If the tunnel crate learns to pump edge → origin after an
/// upgrade, this route and its socket map are what to delete.
#[cfg(unix)]
fn typed(stream: &mut TcpStream, pid: u32, body: &[u8]) -> std::io::Result<()> {
    use crate::attach::frame;
    if body.is_empty() {
        return json_response(stream, "400 Bad Request", br#"{"ok":false}"#, "");
    }
    match to_shim(pid, &frame::encode(frame::KEYS, body)) {
        true => json_response(stream, "200 OK", br#"{"ok":true}"#, ""),
        false => json_response(stream, "404 Not Found", br#"{"ok":false}"#, ""),
    }
}

/// Ask the agent to take a size. Same transport and the same reason as [`typed`].
#[cfg(unix)]
fn resized(stream: &mut TcpStream, pid: u32, body: &[u8]) -> std::io::Result<()> {
    use crate::attach::frame;
    let size = serde_json::from_slice::<Value>(body).ok().and_then(|v| {
        let cols = v.get("cols").and_then(Value::as_u64)?;
        let rows = v.get("rows").and_then(Value::as_u64)?;
        // A pty's size is two u16s, and the shim rejects a zero either way.
        (cols > 0 && rows > 0).then(|| (cols.min(1000) as u16, rows.min(1000) as u16))
    });
    let Some((cols, rows)) = size else {
        return json_response(stream, "400 Bad Request", br#"{"ok":false}"#, "");
    };
    match to_shim(pid, &frame::size(frame::RESIZE, cols, rows)) {
        true => json_response(stream, "200 OK", br#"{"ok":true}"#, ""),
        false => json_response(stream, "404 Not Found", br#"{"ok":false}"#, ""),
    }
}

#[cfg(not(unix))]
fn typed(stream: &mut TcpStream, _pid: u32, _body: &[u8]) -> std::io::Result<()> {
    not_found(stream)
}

#[cfg(not(unix))]
fn resized(stream: &mut TcpStream, _pid: u32, _body: &[u8]) -> std::io::Result<()> {
    not_found(stream)
}

/// Bridge a browser's WebSocket to the shim that owns `pid`'s pty.
///
/// The wire is deliberately thin, because both ends already speak the same
/// thing. The shim sends `OUTPUT` frames of raw pty bytes and xterm.js consumes
/// raw pty bytes, so a binary WebSocket message is that payload verbatim —
/// nothing parses the ANSI on the way through. Keystrokes come back the same
/// way and become `KEYS` frames. Only the size is text, as JSON, because it is
/// the one thing the two sides describe differently.
///
/// Two directions need two threads. This one reads the browser; a spawned one
/// reads the shim. They share the socket behind a mutex rather than splitting
/// it, because a pong written mid-repaint must not land inside another frame.
///
/// Watching needs nothing; typing needs `--allow-input` and goes to [`typed`].
/// Resizing counts as typing: the pty takes the size of the smallest window
/// watching it, so a read-only viewer that resized would reach through the page
/// and reshape somebody's terminal.
#[cfg(unix)]
fn terminal(
    stream: TcpStream,
    mut reader: BufReader<TcpStream>,
    pid: u32,
    key: &str,
) -> std::io::Result<()> {
    use crate::attach::frame;
    use std::io::Read;

    let mut stream = stream;
    // `connect` is the authoritative test that this pid is an agent cctop owns:
    // there is a socket for it or there is not, and a pid that is anything else
    // on this machine has none.
    let Some(sock) = crate::attach::connect(pid) else {
        return not_found(&mut stream);
    };

    // A terminal is idle for minutes at a time and must not be reaped for it.
    stream.set_read_timeout(None)?;
    let accept = ws::accept_key(key);
    stream.write_all(
        format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
        )
        .as_bytes(),
    )?;
    stream.flush()?;

    let out = Arc::new(Mutex::new(stream));
    let feeder = Arc::clone(&out);
    let mut from_shim = sock.try_clone()?;
    std::thread::spawn(move || {
        let mut decoder = frame::Decoder::default();
        let mut buf = [0u8; 8192];
        while let Ok(n) = from_shim.read(&mut buf) {
            if n == 0 {
                break;
            }
            decoder.push(&buf[..n]);
            while let Some((kind, payload)) = decoder.next() {
                let sent = match kind {
                    frame::OUTPUT => ws::write(&mut *lock(&feeder), ws::BINARY, &payload),
                    frame::SIZE => match frame::parse_size(&payload) {
                        Some((cols, rows)) => ws::write(
                            &mut *lock(&feeder),
                            ws::TEXT,
                            json!({ "cols": cols, "rows": rows }).to_string().as_bytes(),
                        ),
                        None => Ok(()),
                    },
                    _ => Ok(()),
                };
                if sent.is_err() {
                    return;
                }
            }
        }
        // The agent exited or the shim went away. Say so rather than leaving a
        // dead terminal that looks merely quiet.
        let _ = ws::write(&mut *lock(&feeder), ws::CLOSE, &[]);
    });

    // This socket carries the screen outwards and nothing inwards: keystrokes
    // arrive as POSTs, for the reason [`typed`] sets out. What is left to read
    // is the protocol's own housekeeping, and a client that stops answering is
    // how we learn the browser is gone.
    while let Some((opcode, payload)) = ws::read(&mut reader)? {
        match opcode {
            ws::PING => ws::write(&mut *lock(&out), ws::PONG, &payload)?,
            ws::CLOSE => break,
            // Data frames are not an error, just unused — a client written
            // against the obvious design should not be disconnected for it.
            _ => {}
        }
    }
    // Dropping our end wakes the feeder thread out of its read.
    let _ = sock.shutdown(std::net::Shutdown::Both);
    Ok(())
}

#[cfg(not(unix))]
fn terminal(
    mut stream: TcpStream,
    _reader: BufReader<TcpStream>,
    _pid: u32,
    _key: &str,
) -> std::io::Result<()> {
    // No ptys and no unix sockets, so no agent was ever started under a shim
    // here and there is nothing on the other side of this route.
    not_found(&mut stream)
}

/// A poisoned mutex here means a writer thread panicked mid-frame. The stream
/// is then desynchronised either way, so taking the lock and letting the next
/// write fail is the same outcome as unwinding, minus the panic.
#[cfg(unix)]
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Enough of RFC 6455 to carry a terminal.
///
/// Not a WebSocket library: no extensions, no permessage-deflate, no
/// fragmentation on the way out. What it does implement is what a browser
/// actually sends — masked client frames, continuation for a paste large enough
/// to be split, and ping — because those arrive whether or not they are
/// convenient.
#[cfg(unix)]
mod ws {
    use std::io::{BufRead, Write};

    /// The constant RFC 6455 §1.3 fixes into the handshake. It is not a secret
    /// and not a key; it exists so a plain HTTP server cannot be talked into
    /// answering an upgrade by accident.
    const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

    pub const TEXT: u8 = 1;
    pub const BINARY: u8 = 2;
    pub const CLOSE: u8 = 8;
    pub const PING: u8 = 9;
    pub const PONG: u8 = 10;

    /// A paste is the only client message that can be large; this is far past
    /// any of them and short of anything worth allocating for a stranger.
    const MAX_MESSAGE: usize = 1 << 20;

    /// The `Sec-WebSocket-Accept` answering a client's key.
    pub fn accept_key(key: &str) -> String {
        crate::util::b64_encode(&crate::util::sha1(format!("{key}{GUID}").as_bytes()))
    }

    /// Write one unfragmented, unmasked server frame.
    pub fn write(out: &mut impl Write, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
        let mut head = vec![0x80 | opcode];
        match payload.len() {
            n if n < 126 => head.push(n as u8),
            n if n <= u16::MAX as usize => {
                head.push(126);
                head.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                head.push(127);
                head.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        out.write_all(&head)?;
        out.write_all(payload)?;
        out.flush()
    }

    /// The next complete message, reassembled across continuation frames.
    ///
    /// `None` means the peer hung up. A control frame is returned as it arrives:
    /// they are never fragmented, and a ping waiting behind a half-sent paste
    /// would be answered too late to be worth anything.
    pub fn read(inp: &mut impl BufRead) -> std::io::Result<Option<(u8, Vec<u8>)>> {
        let mut message: Vec<u8> = Vec::new();
        let mut kind = 0u8;
        loop {
            let mut head = [0u8; 2];
            if inp.read_exact(&mut head).is_err() {
                return Ok(None);
            }
            let fin = head[0] & 0x80 != 0;
            let opcode = head[0] & 0x0f;
            let masked = head[1] & 0x80 != 0;

            let len = match head[1] & 0x7f {
                126 => {
                    let mut b = [0u8; 2];
                    inp.read_exact(&mut b)?;
                    u16::from_be_bytes(b) as usize
                }
                127 => {
                    let mut b = [0u8; 8];
                    inp.read_exact(&mut b)?;
                    u64::from_be_bytes(b) as usize
                }
                n => n as usize,
            };
            // A client frame must be masked, and an oversized one is either a
            // desynchronised stream or someone asking for the allocation.
            if !masked || len > MAX_MESSAGE || message.len() + len > MAX_MESSAGE {
                return Ok(None);
            }

            let mut mask = [0u8; 4];
            inp.read_exact(&mut mask)?;
            let mut payload = vec![0u8; len];
            inp.read_exact(&mut payload)?;
            for (i, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[i % 4];
            }

            // Control frames (>= 8) interleave with a fragmented message and
            // are answered on their own.
            if opcode >= 8 {
                return Ok(Some((opcode, payload)));
            }
            if opcode != 0 {
                kind = opcode;
            }
            message.extend_from_slice(&payload);
            if fin {
                return Ok(Some((kind, message)));
            }
        }
    }
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
            terminals: "[]".into(),
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
            terminals: "[]".into(),
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

    /// The handshake a browser checks before it will open the socket. The
    /// vector is RFC 6455's own worked example.
    #[cfg(unix)]
    #[test]
    fn the_handshake_answers_what_rfc_6455_says_it_must() {
        assert_eq!(
            ws::accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    /// The three length encodings, at their boundaries — a frame written with
    /// the wrong one desynchronises the stream rather than failing loudly.
    #[cfg(unix)]
    #[test]
    fn frames_are_written_with_the_length_form_their_size_requires() {
        let write = |n: usize| {
            let mut out = Vec::new();
            ws::write(&mut out, ws::BINARY, &vec![b'x'; n]).unwrap();
            out
        };

        assert_eq!(&write(5)[..2], &[0x82, 5]);
        // 125 is the last length that fits in the seven bits.
        assert_eq!(&write(125)[..2], &[0x82, 125]);
        assert_eq!(&write(126)[..4], &[0x82, 126, 0, 126]);
        assert_eq!(&write(70_000)[..2], &[0x82, 127]);
        assert_eq!(write(70_000).len(), 2 + 8 + 70_000);
    }

    /// What a browser actually sends: masked payloads, a message split across
    /// continuation frames, and a control frame that must not be mistaken for
    /// either.
    #[cfg(unix)]
    #[test]
    fn a_masked_and_fragmented_message_arrives_whole() {
        // Build client frames the way a browser would, mask included.
        let client = |opcode: u8, payload: &[u8], fin: bool| {
            let mask = [0xA1, 0xB2, 0xC3, 0xD4];
            let mut out = vec![
                if fin { 0x80 | opcode } else { opcode },
                0x80 | payload.len() as u8,
            ];
            out.extend_from_slice(&mask);
            out.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
            out
        };

        let mut wire = client(ws::TEXT, b"hel", false);
        wire.extend(client(0, b"lo", true));
        wire.extend(client(ws::PING, b"hb", true));
        let mut wire = std::io::Cursor::new(wire);

        assert_eq!(
            ws::read(&mut wire).unwrap(),
            Some((ws::TEXT, b"hello".to_vec())),
            "continuation frames should reassemble into one message"
        );
        assert_eq!(
            ws::read(&mut wire).unwrap(),
            Some((ws::PING, b"hb".to_vec()))
        );
        assert_eq!(ws::read(&mut wire).unwrap(), None, "clean EOF is a hangup");
    }

    /// An unmasked client frame is a protocol violation, and the only things
    /// that send one are broken or probing. Either way the answer is to stop.
    #[cfg(unix)]
    #[test]
    fn an_unmasked_client_frame_ends_the_conversation() {
        let unmasked = vec![0x81, 0x02, b'h', b'i'];
        let mut wire = std::io::Cursor::new(unmasked);
        assert_eq!(ws::read(&mut wire).unwrap(), None);
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
