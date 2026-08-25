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
//! **It is read-only, like [`mcp`](crate::mcp) and for the same reason.** There
//! is no route here that starts, stops, or types at anything.

use crate::cli::Args;
use crate::loader::Loader;
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
    eprintln!("Read-only. Ctrl-C to stop.");

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let snapshot = Arc::clone(&snapshot);
        let token = token.clone();
        // A thread per connection: browsers open two or three and hold them.
        // ponytail: no pool and no cap — the listener is on loopback and the
        // work per request is copying one already-built string. A cap belongs
        // here the day a route does real work.
        std::thread::spawn(move || {
            let _ = handle(stream, token.as_deref(), &snapshot);
        });
    }
    Ok(())
}

/// Keep one JSON snapshot up to date on a background thread.
///
/// Returns the shared cell the request handlers read; the thread outlives them
/// all and is never joined, because the only way out of [`run`] is the process
/// ending.
fn spawn_refresh(plan: crate::pricing::Plan, period: Duration) -> Arc<Mutex<String>> {
    let snapshot = Arc::new(Mutex::new(String::from("[]")));
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
                *snapshot.lock().unwrap() = json;
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

/// Answer one request. Errors are the client hanging up, and mean nothing here.
fn handle(
    mut stream: TcpStream,
    token: Option<&str>,
    snapshot: &Mutex<String>,
) -> std::io::Result<()> {
    let Some((path, gzip)) = read_request(&mut stream)? else {
        return respond(&mut stream, "400 Bad Request", "text/plain", b"bad request");
    };

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

    match rest.trim_start_matches('/') {
        "" | "index.html" => respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            PAGE.as_bytes(),
        ),
        "sessions.json" => {
            let body = snapshot.lock().unwrap().clone();
            match gzip {
                // ~10x on this payload, and `flate2` is already in the tree for
                // the updater's tarballs — so the mobile connection this exists
                // for pays 12KB a poll instead of 110KB.
                true => compressed(&mut stream, body.as_bytes()),
                false => respond(&mut stream, "200 OK", "application/json", body.as_bytes()),
            }
        }
        _ => not_found(&mut stream),
    }
}

/// Read the request line and headers, returning the path and whether the client
/// said it takes gzip.
///
/// Only GET is answered — nothing here changes state, so a POST is either a
/// mistake or somebody probing, and both get the same 404 from the router.
fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<(String, bool)>> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    let mut read = reader.read_line(&mut line)?;

    let path = match line.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["GET", path, ..] => {
            // The dashboard takes no query parameters, and a fragment never
            // reaches the server — so anything after `?` is noise to drop
            // rather than something to route on.
            path.split(['?', '#']).next().unwrap_or("/").to_string()
        }
        _ => return Ok(None),
    };

    // The headers have to be drained whether or not they are wanted: a browser
    // does not consider the request sent until they are written, and a socket
    // nobody reads stalls it.
    let mut gzip = false;
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
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("accept-encoding")
        {
            gzip = value.to_ascii_lowercase().contains("gzip");
        }
    }
    Ok(Some((path, gzip)))
}

fn respond(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// The same as [`respond`], gzipped. Falls back to plain on a compressor error
/// rather than failing the request over a size optimisation.
fn compressed(stream: &mut TcpStream, body: &[u8]) -> std::io::Result<()> {
    use flate2::{Compression, write::GzEncoder};
    let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
    let packed = enc.write_all(body).and_then(|()| enc.finish());
    let Ok(packed) = packed else {
        return respond(stream, "200 OK", "application/json", body);
    };
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
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
        let snapshot = Arc::new(Mutex::new(String::from(r#"[{"provider":"claude"}]"#)));

        std::thread::spawn(move || {
            for stream in listener.incoming().take(5) {
                let Ok(stream) = stream else { continue };
                let snapshot = Arc::clone(&snapshot);
                std::thread::spawn(move || {
                    let _ = handle(stream, Some("s3cret"), &snapshot);
                });
            }
        });

        let get = |path: &str| -> String {
            use std::io::Read;
            let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
            write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };

        assert!(get("/s3cret/").contains("200 OK"));
        assert!(get("/s3cret/sessions.json").contains(r#"[{"provider":"claude"}]"#));
        assert!(get("/s3cret").contains("301"));
        // A wrong token and an unprefixed path are the same answer on purpose.
        assert!(get("/wrong/sessions.json").contains("404"));
        assert!(get("/sessions.json").contains("404"));
    }
}
