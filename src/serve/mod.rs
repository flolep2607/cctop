//! `cctop serve` — the same table, in a browser.
//!
//! The dashboard's headline is which session is waiting on you, and that only
//! pays off while someone is looking at the terminal. Agents sit amber for
//! twenty minutes because the person who could answer them is in a meeting.
//! This is the same data on a surface they have with them: a page that streams
//! the rows over SSE, a per-session report that answers the question the live
//! table cannot — where an afternoon's money went — the conversation itself,
//! what the session has edited, what it is allowed to reach, and a prompt box
//! for answering the agent that was waiting.
//!
//! # It can act, and that is a decision with a cost
//!
//! This started read-only, and the argument for that is still true: a bug in a
//! route that only reads is a disclosure, and the same bug in one that writes is
//! someone else's agent taking an action in your repository. What changed is
//! what the page is for. A dashboard that says an agent has been waiting twenty
//! minutes, on a phone, from a meeting, and cannot answer it, has shown you a
//! problem and withheld the fix. So [`actions`] can type a prompt at a live
//! session, resume a dead one, and hand one's work to a different harness —
//! nothing that destroys work, and nothing the terminal UI could not already do.
//!
//! The whole authorisation is the token in the URL, and that is worth stating
//! plainly rather than burying: **whoever holds the link can drive the agents on
//! this machine.** Hence the defaults — loopback, a token, and both of
//! `--no-token` and `--no-actions` able to take the acting half away. On top of
//! the token, an action must be a `POST` carrying a JSON body, which is what
//! stops a page in another tab from firing one with a token it cannot read; see
//! [`http`] for why that pairing and not a header of our own invention.
//!
//! # What guards the socket
//!
//! - **Loopback by default.** `--bind` is how it reaches the network and
//!   `--tunnel` is how it reaches the internet, and both say so on stderr when
//!   they do. Nobody exposes this by not reading a flag.
//! - **A token on every request**, generated per run, carried in the URL. The
//!   URL is therefore the credential, which is what makes the link shareable
//!   over whatever channel the user already trusts — and what makes `--no-token`
//!   a thing to justify rather than a convenience. It also gates the actions:
//!   no token, no acting, because there would be nothing left to authorise with.
//! - **Nothing destructive.** No route stops an agent, kills a process or
//!   deletes a transcript. Those stay in the terminal, where the confirmation
//!   prompt is.
//! - **Bounded everything.** [`MAX_CONNECTIONS`] threads, one snapshot shared
//!   between them, and reads deadlined in [`http`]. The expensive work — a full
//!   transcript parse — happens on the routes that need one, for one session, on
//!   request: the report, the conversation, and a handoff's brief.
//!
//! # Why a thread per connection
//!
//! Because there are at most a handful. This serves one person's browsers, and
//! an SSE stream is idle between snapshots, so the async runtime that would
//! make this scale is dependency weight bought against a load that does not
//! exist. The connection cap is what makes the arithmetic safe rather than
//! optimistic.
//!
//! ponytail: no TLS of its own. A loopback socket does not want it, and
//! terminating TLS for the LAN case means certificates cctop has no business
//! managing. `--tunnel` is the answer for reaching this from elsewhere — see
//! [`tunnel`], which borrows Cloudflare's certificate and Cloudflare's edge, and
//! is therefore encryption in transit rather than a private channel. For a LAN,
//! the tunnel the user already has (ssh, Tailscale) is still better than
//! anything here, because it authenticates rather than merely encrypting.

mod actions;
mod chat;
mod http;
mod report;
mod tunnel;

use crate::cli;
use crate::fleet;
use crate::loader::Loader;
use crate::pricing::Plan;
use crate::session::Session;
use crate::watch::Watch;
use http::{EventStream, Request};
use std::collections::HashMap;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// The port `cctop serve` prefers.
///
/// Above the range a privileged process would want and clear of the numbers
/// development servers habitually take (3000, 5173, 8000, 8080), so the default
/// works without a flag on a machine already running the things a person who
/// wants this is likely to be running.
const DEFAULT_PORT: u16 = 7777;

/// How many ports past the default to try before giving up.
///
/// Only when the port was not asked for. Someone who wrote `--port 8080` meant
/// that port, and quietly serving on 8081 gives them a link that works and a
/// reverse proxy that does not.
const PORT_SEARCH: u16 = 16;

/// Concurrent connections served before further ones are refused.
///
/// Each is a thread. A browser opens perhaps six — the page, its fetches, and
/// one SSE stream it holds — so this is several tabs' worth and still a bound
/// small enough that a peer opening sockets in a loop achieves nothing.
const MAX_CONNECTIONS: usize = 32;

/// How often a full walk of every provider directory happens.
///
/// The light refresh re-reads the rows that can still change, which is nearly
/// everything that matters; only a session that did not exist before needs the
/// walk. [`Watch`] usually reports those as they appear, and this is the safety
/// net for when it cannot — a provider directory that was not watchable, or an
/// inotify budget the machine had already spent.
const FULL_WALK: Duration = Duration::from_secs(30);

/// How long an SSE stream waits for a new snapshot before sending a comment.
///
/// Idle connections have to produce traffic or the hops in between drop them,
/// and a browser that has gone away only surfaces as a write error once
/// something is written at it.
const SSE_KEEPALIVE: Duration = Duration::from_secs(20);

/// What a request naming no session, or an ambiguous prefix of one, is told.
///
/// One string because four routes say it, and four spellings of one refusal is
/// how a page comes to show three different messages for the same mistake.
const NO_SUCH_SESSION: &str = "no session with that id, or the prefix matches more than one";

/// The dashboard, and the report page, with their assets already inlined.
///
/// `include_str!` rather than a directory served off disk: an installed cctop
/// is one binary, and a page that loads its own CSS is a page that breaks the
/// moment the binary is moved. It is also what lets the response promise a
/// content policy that forbids loading anything at all.
const DASHBOARD_HTML: &str = include_str!("assets/dashboard.html");
const REPORT_HTML: &str = include_str!("assets/report.html");

/// The stylesheet both pages share, substituted into each at send time.
///
/// One file rather than two copies, and still not a second request: the content
/// policy this server sends forbids loading anything at all, which is only
/// affordable because everything is already in the page.
const COMMON_CSS: &str = include_str!("assets/common.css");

/// Everything a connection thread needs, shared behind one `Arc`.
struct Shared {
    /// The per-run access token, or empty under `--no-token`.
    token: String,
    /// Whether the routes that act on a session answer at all.
    ///
    /// The token is what makes them safe to have, so `--no-token` turns them
    /// off and `--no-actions` turns them off for someone who wants the page and
    /// not the buttons. Read on every action request rather than only when the
    /// page is built: a page cached from a run that had them on must not be able
    /// to act on a run that has them off.
    actions: bool,
    plan: Plan,
    /// The latest snapshot, and a version that only ever increases.
    ///
    /// SSE threads wait on the condvar for a version past the one they last
    /// sent, which is what makes an idle stream cost nothing: no polling, and
    /// one wakeup per refresh rather than one per connection per tick.
    latest: Mutex<Arc<Snapshot>>,
    updated: Condvar,
    /// Transcript parses for the report route.
    ///
    /// Its own store rather than the refresh thread's, because that one is
    /// behind a `&mut Loader` on a thread that is usually mid-walk, and a
    /// report should not queue behind a directory sweep. It is never saved:
    /// two owners writing one cache file is how a cache file gets corrupted,
    /// and the refresh thread is the owner that has something worth keeping.
    store: crate::cache::Store,
}

/// One publish of the whole table.
struct Snapshot {
    version: u64,
    /// The `--json` document, already serialised — every SSE client sends the
    /// same bytes, so they are rendered once per refresh rather than once per
    /// client.
    json: String,
    /// The rows behind it, kept so the report route can find its session
    /// without re-walking the disk.
    sessions: Vec<Session>,
    /// Hosts named with `--host` that could not be read, and why.
    host_errors: Vec<(String, String)>,
}

/// Remote rows, written by the host pollers and read by the refresh thread.
#[derive(Default)]
struct Remotes {
    rows: HashMap<String, Vec<Session>>,
    errors: HashMap<String, String>,
}

pub const HELP: &str = "\
cctop serve — the session table, and the sessions themselves, in a browser

USAGE:
  cctop serve [OPTIONS]

OPTIONS:
  --bind <ADDR>    Address to listen on [default: 127.0.0.1]. Anything other
                   than a loopback address puts the page on your network, which
                   is announced on stderr when it happens
  --port <PORT>    Port to listen on [default: 7777]. Without this flag a busy
                   port is stepped past; with it, a busy port is an error
  --no-token       Serve without an access token. Every process and user on the
                   machine can then read your sessions — and since the token is
                   what authorises an action, this also turns actions off
  --no-actions     Serve the pages without the buttons: no prompts, no resuming,
                   no handing a session to another agent
  --tunnel         Also reach the page from anywhere, over a trycloudflare quick
                   tunnel. Needs nothing installed, lasts as long as this
                   process, and puts the link on the public internet — so the
                   token is what stands between it and your agents
  --plan <PLAN>    Billing plan for cost figures: retail, max, or included
                   [default: retail]
  --delay <SECS>   Seconds between refreshes [default: 2]
  --host <HOST>    Also serve the sessions on another machine, over ssh.
                   Repeatable; same syntax as `cctop --host`
  -h, --help       Print this help

The page shows each session's conversation, what it edited, and what it can
reach, and it can send a prompt to a live session, resume a dead one, or hand
one to a different agent. Whoever holds the link can do all of that, which is
why the link carries a token and the default is loopback only.
";

/// Parse `cctop serve`'s own flags and run the server until interrupted.
///
/// Flags are parsed here rather than in [`crate::cli::Args`] for the reason
/// `doctor` and `attach` are: cctop takes no positionals, so clap answers a
/// bare `serve` with a usage error before it can reach any of this.
pub fn run(argv: &[String]) -> anyhow::Result<i32> {
    let mut bind = "127.0.0.1".to_string();
    let mut port = DEFAULT_PORT;
    let mut port_given = false;
    let mut no_token = false;
    let mut no_actions = false;
    let mut want_tunnel = false;
    let mut plan = Plan::Retail;
    let mut delay = Duration::from_secs(2);
    let mut hosts: Vec<String> = Vec::new();

    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("{flag} needs a value"))
        };
        match flag.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(0);
            }
            "--bind" => bind = value()?,
            "--port" => {
                port = value()?
                    .parse()
                    .map_err(|_| anyhow::anyhow!("--port takes a number from 1 to 65535"))?;
                port_given = true;
            }
            "--no-token" => no_token = true,
            "--no-actions" => no_actions = true,
            "--tunnel" => want_tunnel = true,
            "--plan" => {
                let given = value()?;
                plan = Plan::parse(&given).ok_or_else(|| {
                    anyhow::anyhow!("unsupported plan '{given}'; use retail, max or included")
                })?;
            }
            "--delay" => {
                let secs: f64 = value()?
                    .parse()
                    .map_err(|_| anyhow::anyhow!("--delay takes a number of seconds"))?;
                if !secs.is_finite() || !(0.5..3600.0).contains(&secs) {
                    anyhow::bail!("--delay must be between 0.5 and 3600 seconds");
                }
                delay = Duration::from_secs_f64(secs);
            }
            "--host" => hosts.push(value()?),
            other => anyhow::bail!("unknown option '{other}'\n\n{HELP}"),
        }
    }

    let listener = listen(&bind, port, port_given)?;
    let addr = listener.local_addr()?;
    if let Some(why) = tunnel_objection(want_tunnel, addr.ip().is_loopback(), no_token) {
        anyhow::bail!("{why}");
    }
    let token = if no_token { String::new() } else { new_token() };
    // The token is the whole authorisation story for an action, so there are no
    // actions without one. `--no-token` is already documented as "every process
    // and user on this machine can read your sessions"; letting that also mean
    // "and type at your agents" is a different sentence, and not one anybody
    // reads a flag expecting.
    let actions = !no_actions && !no_token;
    // Held until the process ends: dropping it unregisters from the edge, which
    // is what makes a tunnel last exactly as long as the serve that asked for
    // one. Started before the announcement so the printed link works when it is
    // read, and before the accept loop because there is nothing to reach yet.
    let tunnel = match want_tunnel {
        true => Some(tunnel::start(addr.port())?),
        false => None,
    };
    announce(&addr, &bind, &token, no_token, tunnel.as_ref());
    if !actions {
        eprintln!(
            "  actions are off{} — the page can read this machine's sessions and \
             nothing else",
            match no_token {
                true => " (no token, so nothing may act)",
                false => "",
            }
        );
    }

    let shared = Arc::new(Shared {
        token,
        actions,
        plan,
        latest: Mutex::new(Arc::new(Snapshot {
            version: 0,
            json: "[]".to_string(),
            sessions: Vec::new(),
            host_errors: Vec::new(),
        })),
        updated: Condvar::new(),
        store: crate::cache::Store::new(),
    });

    let remotes = Arc::new(Mutex::new(Remotes::default()));
    for host in fleet::Host::collect(&hosts) {
        spawn_host_poller(host, Arc::clone(&remotes));
    }
    spawn_refresher(Arc::clone(&shared), Arc::clone(&remotes), plan, delay);

    let live = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };

        // Checked before the thread is spawned, so refusing a connection costs
        // a response rather than a thread — which is the point of a cap.
        if live.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
            http::respond_error(&mut stream, None, 503, "too many open connections");
            continue;
        }
        let slot = Connection::take(&live);
        let shared = Arc::clone(&shared);
        // A spawn failure must not take the accept loop down with it: the
        // machine is out of threads, which the next connection may not be. The
        // slot is released either way — dropping the closure unspawned drops
        // the guard inside it, and so does a handler that panics.
        let _ = std::thread::Builder::new()
            .name("cctop-serve".into())
            .spawn(move || {
                let _slot = slot;
                serve_connection(&shared, &mut stream);
            });
    }
    Ok(0)
}

/// One of [`MAX_CONNECTIONS`], released when the handler ends however it ends.
///
/// A guard rather than a decrement at the bottom of the thread: a handler that
/// panics would otherwise leak its slot, and enough of those turn the cap into
/// a server that refuses everything and cannot be talked out of it.
struct Connection(Arc<AtomicUsize>);

impl Connection {
    fn take(live: &Arc<AtomicUsize>) -> Connection {
        live.fetch_add(1, Ordering::Relaxed);
        Connection(Arc::clone(live))
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Bind the listener, stepping past a busy port only when none was asked for.
fn listen(bind: &str, port: u16, port_given: bool) -> anyhow::Result<TcpListener> {
    let last = match port_given {
        true => port,
        false => port.saturating_add(PORT_SEARCH),
    };
    let mut last_error = None;
    for candidate in port..=last {
        let addr = (bind, candidate)
            .to_socket_addrs()
            .map_err(|e| anyhow::anyhow!("could not resolve --bind {bind}: {e}"))?
            .next()
            .ok_or_else(|| anyhow::anyhow!("--bind {bind} resolved to no address"))?;
        match TcpListener::bind(addr) {
            Ok(listener) => return Ok(listener),
            Err(e) => last_error = Some(e),
        }
    }
    let why = last_error.map_or_else(|| "no port to try".to_string(), |e| e.to_string());
    match port_given {
        true => anyhow::bail!("could not listen on {bind}:{port}: {why}"),
        false => anyhow::bail!(
            "could not listen on {bind}, ports {port} to {last}: {why}\n\
             Use --port to name a free one."
        ),
    }
}

/// Why this `--tunnel` is refused, if it is.
///
/// Both refusals are combinations that read as one wish and mean another, and
/// each is cheaper to say than to explain afterwards:
///
/// - **`--tunnel` with a token turned off.** The tunnel's whole job is to put
///   the page somewhere anyone can reach; the token is then the only thing
///   between the internet and a prompt box wired to a live agent. `--no-token`
///   is defensible on loopback and indefensible here, so it is an error rather
///   than a warning nobody scrolls back to.
/// - **`--tunnel` with a non-loopback `--bind`.** The tunnel dials out from this
///   machine, so it reaches a loopback listener perfectly well. Binding wider
///   also publishes the page on the local network, which is a second decision
///   and not the one that was asked for.
///
/// Split out from [`run`] because it is the whole of the policy and wants a test
/// rather than a careful reading.
fn tunnel_objection(want_tunnel: bool, loopback: bool, no_token: bool) -> Option<&'static str> {
    if !want_tunnel {
        return None;
    }
    if no_token {
        return Some(
            "--tunnel with --no-token would publish your sessions, and a prompt \
             box for your agents, to anyone who finds the URL.\n\
             Drop one of the two: the token is what makes the link a credential.",
        );
    }
    if !loopback {
        return Some(
            "--tunnel does not need --bind: the tunnel connects out from this \
             machine, so it reaches a loopback listener.\n\
             Binding wider would put the page on your local network as well, \
             which --tunnel is not asking for.",
        );
    }
    None
}

/// Print where to point a browser, and say plainly when the page is reachable
/// beyond this machine.
///
/// On stderr, so `cctop serve > /dev/null` still tells someone what happened,
/// and so the URL is not mistaken for output a script should parse.
///
/// With a tunnel the public URL is printed first and the loopback one after it:
/// the public one is what someone asked for and what goes to the phone, and the
/// loopback one is still the right link from this machine. What follows both is
/// the sentence that matters — that the first link is a way in, not a view.
fn announce(
    addr: &SocketAddr,
    bind: &str,
    token: &str,
    no_token: bool,
    tunnel: Option<&tunnel::Tunnel>,
) {
    let query = match token.is_empty() {
        true => String::new(),
        false => format!("?t={token}"),
    };
    if let Some(tunnel) = tunnel {
        eprintln!("cctop: serving on {}/{query}", tunnel.url);
        eprintln!("cctop: also on http://127.0.0.1:{}/{query}", addr.port());
        eprintln!(
            "cctop: that first link is on the public internet. Anyone who has it \
             can read every session on this machine — and, unless --no-actions, \
             type at your agents, which runs commands as you. Cloudflare carries \
             the traffic and can read it. The tunnel ends when this process does."
        );
        let _ = std::io::stderr().flush();
        return;
    }
    // `0.0.0.0` and `::` mean "every interface", which is not a host anyone can
    // type. Printing one as though it were a link is how an address that cannot
    // work gets sent to somebody, so those say the port and leave the host to
    // the person who knows which of their interfaces they meant.
    if addr.ip().is_unspecified() {
        eprintln!(
            "cctop: serving on every interface, port {} — open \
             http://<this machine>:{}/{query}",
            addr.port(),
            addr.port()
        );
    } else {
        let host = match addr.ip().is_loopback() {
            true => "127.0.0.1".to_string(),
            false => bind.to_string(),
        };
        eprintln!("cctop: serving on http://{host}:{}/{query}", addr.port());
    }

    if !addr.ip().is_loopback() {
        eprintln!(
            "cctop: this is reachable from your network — anyone who can reach \
             that address can read every session on this machine."
        );
    }
    if no_token {
        eprintln!(
            "cctop: --no-token, so nothing is checked — every process and user \
             on this machine can read your sessions too."
        );
    }
    let _ = std::io::stderr().flush();
}

/// A token for this run.
///
/// See [`crate::util::random_bytes`] for where the entropy comes from and what
/// it is worth: enough that a token cannot be guessed from across a network,
/// which is why the default bind is loopback and this is defence in depth
/// rather than the defence.
fn new_token() -> String {
    crate::util::random_hex(TOKEN_BYTES)
}

/// Bytes of entropy behind a token, which is twice as many hex characters.
const TOKEN_BYTES: usize = 16;

/// Compare a presented token against the real one without leaking where they
/// diverged.
///
/// The timing channel is narrow over a network and completely open over
/// loopback, which is exactly where an unprivileged local process would be
/// standing. Lengths are compared separately and do leak, which tells an
/// attacker something they can already read out of `--help`.
fn token_matches(expected: &str, given: &str) -> bool {
    if expected.len() != given.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in expected.bytes().zip(given.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// Poll one host forever, publishing whatever it last said.
///
/// A failed poll keeps the previous rows and records why: the ssh connection
/// dropping has not stopped those agents, and blanking the machine would make
/// the totals look complete when they are not — the same call [`crate::ui`]
/// makes for the same reason.
fn spawn_host_poller(host: fleet::Host, remotes: Arc<Mutex<Remotes>>) {
    std::thread::spawn(move || {
        loop {
            let snapshot = host.poll();
            if let Ok(mut remotes) = remotes.lock() {
                match snapshot {
                    fleet::Snapshot::Rows(rows) => {
                        remotes.errors.remove(&host.target);
                        remotes.rows.insert(host.target.clone(), rows);
                    }
                    fleet::Snapshot::Failed(why) => {
                        remotes.errors.insert(host.target.clone(), why);
                    }
                }
            }
            std::thread::sleep(fleet::POLL);
        }
    });
}

/// Walk, refresh, and publish, forever.
///
/// The cadence mirrors the UI's: a light refresh every `delay` that re-reads
/// only rows that can still change, and a full walk when [`Watch`] says a
/// session file appeared or [`FULL_WALK`] passes without it saying anything.
/// A walk on every tick would re-read every provider directory on disk to
/// discover, almost always, nothing.
fn spawn_refresher(shared: Arc<Shared>, remotes: Arc<Mutex<Remotes>>, plan: Plan, delay: Duration) {
    std::thread::spawn(move || {
        // Blocking, once, before the first snapshot: a dashboard whose first
        // frame shows every cost as zero is worse than one that appears a
        // second later with the truth.
        crate::pricing::refresh_pricing_blocking();

        let mut loader = Loader::new();
        let watch = Watch::start();
        let mut rows = loader.load(plan);
        let mut walked = Instant::now();
        let mut version = 0u64;
        publish(&shared, &remotes, &rows, plan, &loader, &mut version);

        loop {
            std::thread::sleep(delay);

            let appeared = watch.as_ref().is_some_and(|w| {
                w.took_structural_change()
                    || w.awaiting_discovery(|path| {
                        rows.iter().any(|s| s.data_file.as_deref() == Some(path))
                    })
            });
            // With no watcher there is nothing to tell us a session appeared,
            // so the timer is the only trigger and has to carry the whole job.
            if appeared || walked.elapsed() >= FULL_WALK {
                rows = loader.load(plan);
                walked = Instant::now();
            } else {
                loader.refresh_live(plan, &mut rows);
            }
            publish(&shared, &remotes, &rows, plan, &loader, &mut version);
        }
    });
}

/// Render one snapshot and wake everyone waiting on it.
/// `loader` is the refresh thread's own, and has to be: the document is built
/// by reading each row's cached extraction, and a fresh loader would have an
/// empty cache and re-parse every transcript on disk once per refresh.
fn publish(
    shared: &Shared,
    remotes: &Mutex<Remotes>,
    local: &[Session],
    plan: Plan,
    loader: &Loader,
    version: &mut u64,
) {
    let (mut sessions, host_errors) = match remotes.lock() {
        Ok(remotes) => {
            let mut merged = local.to_vec();
            for rows in remotes.rows.values() {
                merged.extend(rows.iter().cloned());
            }
            let mut errors: Vec<(String, String)> = remotes
                .errors
                .iter()
                .map(|(h, why)| (h.clone(), why.clone()))
                .collect();
            errors.sort();
            (merged, errors)
        }
        Err(_) => (local.to_vec(), Vec::new()),
    };
    // Newest first, so the page never has to decide what order means. Remote
    // rows arrive on their own schedule and would otherwise land wherever the
    // merge happened to put them.
    sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));

    // The same document `--json` prints and `--host` parses, from the same
    // builder: a browser being shown different figures than the terminal is a
    // bug nobody would think to look for. See [`cli::json_sessions`].
    let document = cli::json_sessions(&sessions, plan, loader);
    let json = serde_json::to_string(&document).unwrap_or_else(|_| "[]".to_string());

    *version += 1;
    let snapshot = Arc::new(Snapshot {
        version: *version,
        json,
        sessions,
        host_errors,
    });
    if let Ok(mut latest) = shared.latest.lock() {
        *latest = snapshot;
    }
    shared.updated.notify_all();
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

/// Read one request, answer it, and let the connection close.
fn serve_connection(shared: &Shared, stream: &mut TcpStream) {
    let request = match Request::parse(stream) {
        Ok(request) => request,
        Err((status, why)) => return http::respond_error(stream, None, status, why),
    };

    // Before the route, so a wrong token cannot be used to find out which
    // routes exist. Every path is behind it, including the ones that only
    // return HTML.
    if !shared.token.is_empty() && !token_matches(&shared.token, request.token()) {
        return http::respond_error(
            stream,
            Some(&request),
            403,
            "missing or wrong access token — open the link cctop printed",
        );
    }

    let path = request.path.clone();
    match path.as_str() {
        "/" => page(shared, stream, &request, DASHBOARD_HTML),
        "/api/sessions" => {
            let snapshot = current(shared);
            http::respond(
                stream,
                Some(&request),
                200,
                "application/json; charset=utf-8",
                snapshot.json.as_bytes(),
            );
        }
        "/api/hosts" => {
            let snapshot = current(shared);
            let body = serde_json::to_string(&snapshot.host_errors).unwrap_or_default();
            http::respond(
                stream,
                Some(&request),
                200,
                "application/json; charset=utf-8",
                body.as_bytes(),
            );
        }
        "/api/events" => events(shared, stream, &request),
        // What can be handed a session's work, and whether this run will act at
        // all. The page asks once and hides the controls it cannot use, rather
        // than offering buttons that answer 404.
        "/api/agents" => {
            let body = serde_json::json!({
                "actions": shared.actions,
                "agents": actions::agents(),
            });
            http::respond(
                stream,
                Some(&request),
                200,
                "application/json; charset=utf-8",
                body.to_string().as_bytes(),
            );
        }
        _ if path.starts_with("/session/") => page(shared, stream, &request, REPORT_HTML),
        _ if path.starts_with("/api/report/") => {
            api_report(shared, stream, &request, &path["/api/report/".len()..]);
        }
        _ if path.starts_with("/api/chat/") => {
            api_chat(shared, stream, &request, &path["/api/chat/".len()..]);
        }
        _ if path.starts_with("/api/access/") => {
            api_access(shared, stream, &request, &path["/api/access/".len()..]);
        }
        _ if path.starts_with("/api/act/") => {
            api_act(shared, stream, &request, &path["/api/act/".len()..]);
        }
        _ => http::respond_error(stream, Some(&request), 404, "no such page"),
    }
}

/// Serve the conversation for one session.
fn api_chat(shared: &Shared, stream: &mut TcpStream, request: &Request, id: &str) {
    let snapshot = current(shared);
    let Some(session) = find(&snapshot.sessions, id) else {
        return http::respond_error(stream, Some(request), 404, NO_SUCH_SESSION);
    };
    json(stream, request, &chat::build(session));
}

/// Serve what one session can reach.
fn api_access(shared: &Shared, stream: &mut TcpStream, request: &Request, id: &str) {
    let snapshot = current(shared);
    let Some(session) = find(&snapshot.sessions, id) else {
        return http::respond_error(stream, Some(request), 404, NO_SUCH_SESSION);
    };
    // The tool counts are the one part of this that needs the transcript, and
    // the rest of the answer is worth having without it — so a session whose
    // extraction fails still reports its instructions, skills and servers.
    let data = shared.store.session_data_fresh(session);
    json(stream, request, &crate::access::build(session, Some(&data)));
}

/// Do something to a session: `/api/act/<verb>/<id>`.
///
/// One route for the three verbs rather than three, because the guards in front
/// of them are the whole security surface and they are identical — a route added
/// later that forgets one of them is the bug this shape prevents.
fn api_act(shared: &Shared, stream: &mut TcpStream, request: &Request, rest: &str) {
    if !shared.actions {
        return http::respond_error(
            stream,
            Some(request),
            403,
            "this cctop serve is read-only — restart it without --no-actions, \
             and with a token, to act on a session",
        );
    }
    // A `POST` with a JSON body, both of which are load-bearing: see the
    // module docs in `http` for why a form on another origin cannot be one.
    if !request.wants_json() {
        return http::respond_error(
            stream,
            Some(request),
            405,
            "an action is a POST with a JSON body",
        );
    }
    let body = match request.json() {
        Ok(body) => body,
        Err((status, why)) => return http::respond_error(stream, Some(request), status, why),
    };
    let Some((verb, id)) = rest.split_once('/') else {
        return http::respond_error(stream, Some(request), 404, "no such action");
    };

    let snapshot = current(shared);
    let Some(session) = find(&snapshot.sessions, id) else {
        return http::respond_error(stream, Some(request), 404, NO_SUCH_SESSION);
    };

    let field = |name: &str| {
        body.get(name)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let outcome = match verb {
        "send" => actions::send(session, &field("text")),
        "resume" => actions::resume(session),
        "handoff" => {
            // The brief is built from the extraction, so this one pays for a
            // transcript read — the same read the report route makes, and for
            // the same reason: a brief assembled from the row alone carries the
            // header and none of the work.
            let data = shared.store.session_data_fresh(session);
            actions::handoff(session, Some(&data), &field("agent"))
        }
        _ => return http::respond_error(stream, Some(request), 404, "no such action"),
    };
    match outcome {
        Ok(done) => json(stream, request, &done),
        Err((status, why)) => http::respond_error(stream, Some(request), status, &why),
    }
}

/// Send a value as JSON, or say why it could not be rendered.
fn json<T: serde::Serialize>(stream: &mut TcpStream, request: &Request, value: &T) {
    match serde_json::to_string(value) {
        Ok(body) => http::respond(
            stream,
            Some(request),
            200,
            "application/json; charset=utf-8",
            body.as_bytes(),
        ),
        Err(e) => http::respond_error(
            stream,
            Some(request),
            503,
            &format!("could not render that: {e}"),
        ),
    }
}

/// The current snapshot, cloned out of the lock rather than held under it.
fn current(shared: &Shared) -> Arc<Snapshot> {
    match shared.latest.lock() {
        Ok(latest) => Arc::clone(&latest),
        // A panicked refresh thread would poison this. The stale snapshot in
        // there is still true of some moment, and is a better answer than 500.
        Err(poisoned) => Arc::clone(&poisoned.into_inner()),
    }
}

/// Serve one of the two HTML pages, with the token stitched in.
///
/// The page needs the token to make its own requests, and it cannot read the
/// one in its URL without either parsing `location` in script — which is fine —
/// or being handed it. It is handed it, because the same page is fetched with
/// no token at all under `--no-token` and a single substitution keeps both
/// cases on one code path.
fn page(shared: &Shared, stream: &mut TcpStream, request: &Request, html: &str) {
    // JSON-encoded rather than pasted between quotes: the token is hex today,
    // and a literal substituted into script is exactly the shape of bug that
    // outlives the reason it was safe.
    let token = serde_json::to_string(&shared.token).unwrap_or_else(|_| "\"\"".to_string());
    // The report page links back to the table, and the link has to carry the
    // credential or it lands on a 403. Hex, so nothing in it needs escaping —
    // asserted by the tests rather than assumed, since the generator could change.
    let back = match shared.token.is_empty() {
        true => String::new(),
        false => format!("?t={}", shared.token),
    };
    // The page spells working directories with `~` the way the table does, and
    // cannot work out where home is on its own — the browser may not even be on
    // this machine. Escaped as JSON for the same reason the token is: it is a
    // path, and paths are allowed to contain the characters that end a string.
    let home = serde_json::to_string(
        &dirs::home_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
    .unwrap_or_else(|_| "\"\"".to_string());
    let body = html
        .replace("__CCTOP_CSS__", COMMON_CSS)
        .replace(
            "\"__CCTOP_ACTIONS__\"",
            match shared.actions {
                true => "true",
                false => "false",
            },
        )
        .replace("\"__CCTOP_TOKEN__\"", &token)
        .replace("\"__CCTOP_HOME__\"", &home)
        .replace("__CCTOP_BACK__", &back)
        .replace("__CCTOP_VERSION__", env!("CARGO_PKG_VERSION"));
    http::respond(
        stream,
        Some(request),
        200,
        "text/html; charset=utf-8",
        body.as_bytes(),
    );
}

/// Hold an SSE stream open, sending each new snapshot as it lands.
fn events(shared: &Shared, stream: &mut TcpStream, _request: &Request) {
    let Ok(mut sse) = EventStream::open(stream) else {
        return;
    };

    let mut sent = 0u64;
    loop {
        // The wait is what makes an idle stream free: no polling, and one
        // wakeup per refresh rather than one per connection per tick.
        let snapshot = {
            let Ok(latest) = shared.latest.lock() else {
                return;
            };
            let (latest, _) = match shared
                .updated
                .wait_timeout_while(latest, SSE_KEEPALIVE, |s| s.version <= sent)
            {
                Ok(pair) => pair,
                Err(_) => return,
            };
            Arc::clone(&latest)
        };

        // Nothing new within the keepalive window, so send the bytes that keep
        // the connection counted as alive — and that surface a client which
        // quietly went away, since a write is the only thing that can.
        if snapshot.version <= sent {
            if sse.keepalive().is_err() {
                return;
            }
            continue;
        }

        if sse.send("sessions", &snapshot.json).is_err() {
            return;
        }
        sent = snapshot.version;
    }
}

/// Build and send one session's report.
fn api_report(shared: &Shared, stream: &mut TcpStream, request: &Request, id: &str) {
    let snapshot = current(shared);
    let Some(session) = find(&snapshot.sessions, id) else {
        return http::respond_error(stream, Some(request), 404, NO_SUCH_SESSION);
    };

    // A remote row names a transcript on the machine it came from. Parsing the
    // same path here would report whatever happens to live at it locally, which
    // is the failure mode `Session::remote` exists to prevent.
    if let Some(remote) = &session.remote {
        return http::respond_error(
            stream,
            Some(request),
            404,
            &format!(
                "this session is on {} — run cctop serve there to report on it",
                remote.host
            ),
        );
    }

    // The one expensive call in the whole server, and the reason it is on this
    // route alone: the report is built from tool arguments and per-request
    // context readings, neither of which the cache carries.
    let data = shared.store.session_data_fresh(session);
    let built = report::build(session, &data, shared.plan);
    match serde_json::to_string(&built) {
        Ok(body) => http::respond(
            stream,
            Some(request),
            200,
            "application/json; charset=utf-8",
            body.as_bytes(),
        ),
        Err(e) => http::respond_error(
            stream,
            Some(request),
            503,
            &format!("could not render the report: {e}"),
        ),
    }
}

/// Find a session by id, or by a prefix that matches exactly one.
///
/// Prefixes because session ids are uuids and the report link is a thing people
/// paste into chat. An ambiguous prefix resolves to nothing rather than to the
/// first match — the wrong session's costs is a worse answer than none.
fn find<'a>(sessions: &'a [Session], id: &str) -> Option<&'a Session> {
    if id.is_empty() {
        return None;
    }
    if let Some(exact) = sessions.iter().find(|s| s.session_id == id) {
        return Some(exact);
    }
    let mut matches = sessions.iter().filter(|s| s.session_id.starts_with(id));
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str) -> Session {
        Session::new(crate::pricing::Provider::Claude, id.into())
    }

    #[test]
    fn a_token_matches_only_itself() {
        let token = new_token();
        assert!(token_matches(&token, &token));
        assert!(!token_matches(&token, ""));
        assert!(!token_matches(&token, &token[..token.len() - 1]));

        let mut wrong = token.clone();
        // Last byte, which is where a comparison that returned early would have
        // stopped leaking and started being wrong.
        wrong.pop();
        wrong.push(if token.ends_with('a') { 'b' } else { 'a' });
        assert!(!token_matches(&token, &wrong));
    }

    #[test]
    fn tokens_differ_between_runs() {
        // Not a randomness test — that belongs to the OS — but it does catch a
        // fallback that has stopped varying at all.
        assert_ne!(new_token(), new_token());
        assert_eq!(new_token().len(), TOKEN_BYTES * 2);
    }

    #[test]
    fn sessions_resolve_by_id_and_by_unambiguous_prefix() {
        let rows = vec![session("abc123"), session("abd999"), session("zz")];
        assert_eq!(find(&rows, "abc123").unwrap().session_id, "abc123");
        assert_eq!(find(&rows, "abc").unwrap().session_id, "abc123");
        assert_eq!(find(&rows, "zz").unwrap().session_id, "zz");
    }

    #[test]
    fn an_ambiguous_prefix_resolves_to_nothing() {
        let rows = vec![session("abc123"), session("abd999")];
        // `ab` matches both. Answering with either one would put another
        // session's costs under this one's heading.
        assert!(find(&rows, "ab").is_none());
        assert!(find(&rows, "").is_none());
        assert!(find(&rows, "nope").is_none());
    }

    #[test]
    fn a_tunnel_refuses_the_two_combinations_that_undo_it() {
        // A tunnel with no token is a public prompt box wired to a live agent.
        assert!(tunnel_objection(true, true, true).is_some());
        // A tunnel plus a wider bind is two exposures for one request.
        assert!(tunnel_objection(true, false, false).is_some());
        // And what it is actually for.
        assert!(tunnel_objection(true, true, false).is_none());
        // Without --tunnel neither of those is this function's business:
        // --bind and --no-token are each documented on their own terms.
        assert!(tunnel_objection(false, false, true).is_none());
    }

    #[test]
    fn the_pages_carry_the_placeholders_the_server_substitutes() {
        // If an asset is edited and the placeholder goes with it, the page ships
        // with no token and fails at the first fetch — in the browser, where
        // nothing here would have noticed.
        for html in [DASHBOARD_HTML, REPORT_HTML] {
            assert!(html.contains("\"__CCTOP_TOKEN__\""));
            assert!(html.contains("__CCTOP_CSS__"));
            assert!(html.contains("__CCTOP_VERSION__"));
            // Without this one a page decides for itself that the action routes
            // exist, draws the controls, and every one of them answers 403.
            assert!(html.contains("\"__CCTOP_ACTIONS__\""));
        }
        assert!(REPORT_HTML.contains("__CCTOP_BACK__"));
        assert!(DASHBOARD_HTML.contains("\"__CCTOP_HOME__\""));
        // The stylesheet is pasted into a `<style>` element, so a `</style>` in
        // it would end the block early and spill CSS into the document.
        assert!(!COMMON_CSS.contains("</style>"));
    }

    #[test]
    fn a_token_is_safe_to_paste_into_a_url_unescaped() {
        // `page` builds the back-link by interpolation rather than by escaping.
        // That is only correct while the generator stays hex.
        let token = new_token();
        assert!(
            token.chars().all(|c| c.is_ascii_hexdigit()),
            "token {token} would need escaping in a URL"
        );
    }

    #[test]
    fn a_named_port_is_not_stepped_past() {
        // Bind one, then ask for the same one: with `--port` given that is an
        // error, and without it the search moves on.
        let held = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let taken = held.local_addr().unwrap().port();

        assert!(listen("127.0.0.1", taken, true).is_err());
        let stepped = listen("127.0.0.1", taken, false).expect("the search finds a free port");
        assert_ne!(stepped.local_addr().unwrap().port(), taken);
    }
}
