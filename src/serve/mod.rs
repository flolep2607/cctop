//! `cctop serve` — the same table, in a browser.
//!
//! The dashboard's headline is which session is waiting on you, and that only
//! pays off while someone is looking at the terminal. Agents sit amber for
//! twenty minutes because the person who could answer them is in a meeting.
//! This is the same data on a surface they have with them: a page that streams
//! the rows over SSE, a per-session report that answers the question the live
//! table cannot — where an afternoon's money went — the conversation itself,
//! what the session has edited, what it is allowed to reach, a prompt box
//! for answering the agent that was waiting, and an analytics page for the
//! questions no single row answers: when the fleet works, and on what.
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
//!   Beside it a second token is minted — the read-only link. It opens every
//!   page and every GET, and `/api/act/*` answers it 403: read-only is a
//!   property of the credential, not a claim a request can make about itself.
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
/// Cross-session aggregation behind `/api/analytics` — the data the
/// analytics page charts, built from the snapshot plus cached extractions.
mod analytics;
/// The conversation reader. Public to the crate because a handoff brief
/// carries what was said as well as what was done — see [`crate::handoff`].
pub mod chat;
/// Routes that only exist in a build that asked for them. Not a default
/// feature, so a released cctop contains none of this — see the module docs.
#[cfg(feature = "debug")]
mod debug;
mod http;
mod notify;
mod quota;
mod report;
mod search;
pub mod tunnel;

use crate::cli;
use crate::fleet;
use crate::loader::Loader;
use crate::pricing::Plan;
use crate::session::Session;
use crate::watch::Watch;
use http::{EventStream, Request};
use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
const ANALYTICS_HTML: &str = include_str!("assets/analytics.html");

/// The stylesheet both pages share, substituted into each at send time.
///
/// One file rather than two copies, and still not a second request: the content
/// policy this server sends forbids loading anything at all, which is only
/// affordable because everything is already in the page.
const COMMON_CSS: &str = include_str!("assets/common.css");

/// The theme picker all three pages share, inlined for the same reason the
/// stylesheet is — and placed early in each page so a chosen theme is on
/// `<html>` before the first paint rather than one frame behind it.
const THEME_JS: &str = include_str!("assets/theme.js");

/// A favicon small enough to keep inline: the table's dark tile with the amber
/// dot it draws on a session that is waiting.
///
/// Served rather than embedded into the HTML — a `<link rel="icon">` is one
/// request more, and embedding SVG in the pages would repeat it into both.
/// `img-src 'self'` in the content policy is what lets the browser fetch it.
const FAVICON: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\">\
    <rect width=\"64\" height=\"64\" rx=\"14\" fill=\"#16151a\"/>\
    <circle cx=\"32\" cy=\"32\" r=\"10\" fill=\"#e0a52c\"/></svg>";

/// The web manifest, for a browser that wants to install the page as an app.
///
/// Its own route rather than a data: URL in a `<link>` because manifest fetches
/// are governed by `manifest-src`, which `data:` is not a part of here.
const MANIFEST: &str = concat!(
    r#"{"name":"cctop","short_name":"cctop","display":"standalone","#,
    r#""start_url":"/","icons":[{"src":"/favicon.svg","sizes":"any","#,
    // `r##` rather than `r#` here: the colour hex begins `"#`, which is the
    // short delimiter's own terminator.
    r##""type":"image/svg+xml"}],"theme_color":"#16151a","background_color":"#16151a"}"##
);

/// Baseline gap between usage checks for a standalone `serve`.
///
/// A minute rather than the session refresh's seconds: quota moves slowly and
/// the endpoints throttle aggressively — a 30s poll was once enough to earn a
/// sustained 429 with a ~15 minute `retry-after`. When a provider asks for
/// longer, `retry_delay_secs` honours that instead.
const QUOTA_INTERVAL_SECS: u64 = 60;

/// How often the quota poller wakes to see whether either provider is due.
const QUOTA_TICK: Duration = Duration::from_secs(10);

/// Everything a connection thread needs, shared behind one `Arc`.
struct Shared {
    /// The per-run access token, or empty under `--no-token`.
    token: String,
    /// The second token minted each run, and the link it makes possible.
    ///
    /// A token is the whole authorisation a request carries, so "read-only"
    /// cannot be a flag a request sends — it has to be a different credential.
    /// This one opens every page and every GET, and `/api/act/*` answers it
    /// with 403. Empty under `--no-token`, which has nothing to withhold.
    readonly: String,
    /// Whether the routes that act on a session answer at all.
    ///
    /// The token is what makes them safe to have, so `--no-token` turns them
    /// off and `--no-actions` turns them off for someone who wants the page and
    /// not the buttons. Read on every action request rather than only when the
    /// page is built: a page cached from a run that had them on must not be able
    /// to act on a run that has them off.
    actions: bool,
    /// The port being served, because it is part of the access cookie's name —
    /// cookies are scoped to the host and ignore the port, so two serves on
    /// one machine would otherwise hand each other's credential back.
    port: u16,
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
    /// The `/api/quota` document, already serialised.
    ///
    /// Rendered by whoever produced the numbers — the standalone serve's own
    /// poller, or the dashboard feeding [`Serving::publish_with_quota`] — so
    /// the route itself is a memory read and never touches the rate-limited
    /// usage endpoints on a browser's schedule.
    quota: Mutex<String>,
    /// The topical tier behind `/api/search`, built lazily on the first query
    /// that wants it.
    topics: Mutex<search::Topics>,
    /// The webhook `--notify` points at, when one was asked for.
    notify: Option<notify::Webhook>,
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
  --notify <URL>   POST a JSON event to this URL when a session crosses into
                   waiting or asking — once per crossing, on a short deadline.
                   [env: CCTOP_NOTIFY_URL]
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
/// What a run of the server needs to know, however it was asked for.
///
/// The CLI fills this from flags and the dashboard fills it from a keypress,
/// which is the point: one server, reached two ways, rather than a second
/// implementation behind a key.
pub struct Options {
    pub bind: String,
    pub port: u16,
    /// Whether `port` was asked for. A port nobody chose steps past a busy one;
    /// a port somebody chose is an error when it is taken, because silently
    /// serving somewhere else is worse than saying so.
    pub port_given: bool,
    pub no_token: bool,
    pub no_actions: bool,
    pub tunnel: bool,
    pub plan: Plan,
    pub delay: Duration,
    pub hosts: Vec<String>,
    /// Where to POST when a session crosses into waiting or asking.
    ///
    /// `None` still falls back to `CCTOP_NOTIFY_URL` inside [`start`], which is
    /// how a dashboard-hosted serve — no flag to have asked with — gets one.
    pub notify: Option<String>,
    /// Whether the server scans for sessions itself.
    ///
    /// True for `cctop serve`, which is the only thing running. False for the
    /// dashboard, which already walks every session five times a second and
    /// would otherwise do it twice in one process — and worse, show a page that
    /// disagrees with the table beside it. The dashboard feeds
    /// [`Serving::publish`] instead.
    pub scan: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            bind: "127.0.0.1".to_string(),
            port: DEFAULT_PORT,
            port_given: false,
            no_token: false,
            no_actions: false,
            tunnel: false,
            plan: Plan::Retail,
            delay: Duration::from_secs(2),
            hosts: Vec::new(),
            notify: None,
            scan: true,
        }
    }
}

/// A server that is up, and the links that reach it.
///
/// Dropping it revokes the tunnel and stops the accept loop, so a dashboard
/// that stops serving stops being reachable — including from a link somebody
/// has already opened.
pub struct Serving {
    /// The loopback link, always present.
    pub local: String,
    /// The public one, when a tunnel was asked for and registered.
    pub public: Option<String>,
    /// The same page, read-only: every route answers but the ones that act.
    ///
    /// Carries the run's second token, on whichever origin is the one to hand
    /// out — the tunnel's when there is one. Empty when the run has no token,
    /// since a tokenless serve has nothing for a second credential to withhold.
    pub readonly: String,
    pub actions: bool,
    shared: Arc<Shared>,
    remotes: Arc<Mutex<Remotes>>,
    plan: Plan,
    version: Mutex<u64>,
    /// Held so dropping this unregisters from Cloudflare's edge.
    _tunnel: Option<tunnel::Tunnel>,
    /// Cleared on drop; the accept loop reads it after every connection and
    /// stops when it is false.
    running: Arc<AtomicBool>,
    port: u16,
}

impl Serving {
    /// The link to hand somebody: the public one when there is one.
    pub fn best(&self) -> &str {
        self.public.as_deref().unwrap_or(&self.local)
    }

    /// Show the page these rows and this usage reading, replacing whatever it
    /// was showing.
    ///
    /// For a dashboard-hosted server, which has both already. Cheap enough to
    /// call on every refresh: it costs one JSON encode of what the table is
    /// already holding, and it is what wakes the event stream. The quota comes
    /// along because the dashboard polls the usage endpoints for its own
    /// panes — the page shares that reading rather than standing up a second
    /// poller against the same rate-limited endpoints, and the two can never
    /// disagree.
    pub fn publish_with_quota(&self, sessions: &[Session], quota: &crate::quota::Quota) {
        let Ok(mut version) = self.version.lock() else {
            return;
        };
        publish(
            &self.shared,
            &self.remotes,
            sessions,
            self.plan,
            &self.shared.store,
            &mut version,
            Some(quota),
        );
    }
}

/// Hand `url` to whatever this desktop opens links with.
///
/// Best effort by design: there is no answer worth waiting for and plenty of
/// machines with no browser to give it to — a headless box, an ssh session, a
/// container. `false` means nothing was launched, which the caller says on the
/// status line so the link can be copied instead.
///
/// Spawned and forgotten rather than waited on: `xdg-open` on some desktops
/// does not return until the browser it started exits, and a dashboard frozen
/// behind somebody's Firefox is not a trade worth making.
pub fn open_in_browser(url: &str) -> bool {
    // `xdg-open` is what every freedesktop environment answers to.
    std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

impl Drop for Serving {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        // The accept loop is parked inside `accept`, so clearing the flag is not
        // enough to end it — one connection of our own wakes it, it sees the
        // flag, and returns. Loopback whatever the server is bound to: it is
        // listening there too for any bind cctop allows, and a refused connect
        // here costs a thread that ends when the process does.
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// Bring the server up and hand back the links, without blocking.
///
/// Everything `cctop serve` does apart from printing and parking. The accept
/// loop moves to a thread of its own so a dashboard can go on drawing, which is
/// the whole reason this is split out of [`run`].
pub fn start(options: Options) -> anyhow::Result<Serving> {
    let listener = listen(&options.bind, options.port, options.port_given)?;
    let addr = listener.local_addr()?;
    if let Some(why) = tunnel_objection(options.tunnel, addr.ip().is_loopback(), options.no_token) {
        anyhow::bail!("{why}");
    }
    let token = match options.no_token {
        true => String::new(),
        false => new_token(),
    };
    // A second credential for the same pages minus the actions — see
    // `Shared::readonly` for why read-only is a token rather than a flag.
    let readonly = match token.is_empty() {
        true => String::new(),
        false => new_token(),
    };
    // The token is the whole authorisation story for an action, so there are no
    // actions without one. `--no-token` is already documented as "every process
    // and user on this machine can read your sessions"; letting that also mean
    // "and type at your agents" is a different sentence, and not one anybody
    // reads a flag expecting.
    let actions = !options.no_actions && !options.no_token;
    // Started before anything is announced, so the link works when it is read,
    // and before the accept loop because there is nothing to reach yet.
    let tunnel = match options.tunnel {
        true => Some(tunnel::start(addr.port())?),
        false => None,
    };

    // The origin a notification's link is built on: the tunnel's when there is
    // one, since a webhook that names loopback is a link that works only on the
    // machine that sent it.
    let origin = tunnel
        .as_ref()
        .map(|t| t.url.clone())
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", addr.port()));

    let shared = Arc::new(Shared {
        token: token.clone(),
        readonly,
        actions,
        port: addr.port(),
        plan: options.plan,
        latest: Mutex::new(Arc::new(Snapshot {
            version: 0,
            json: "[]".to_string(),
            sessions: Vec::new(),
            host_errors: Vec::new(),
        })),
        updated: Condvar::new(),
        store: crate::cache::Store::new(),
        quota: Mutex::new(quota::EMPTY.to_string()),
        topics: Mutex::new(search::Topics::default()),
        notify: options
            .notify
            .or_else(|| {
                // The dashboard builds its Options without a flag, so the
                // environment is answered here — where every way of starting
                // a serve passes through — rather than only in `run`'s parser.
                std::env::var("CCTOP_NOTIFY_URL")
                    .ok()
                    .filter(|url| !url.is_empty())
            })
            .map(|target| notify::Webhook::new(target, origin.clone(), token.clone())),
    });

    let remotes = Arc::new(Mutex::new(Remotes::default()));
    for host in fleet::Host::collect(&options.hosts) {
        spawn_host_poller(host, Arc::clone(&remotes));
    }
    if options.scan {
        spawn_refresher(
            Arc::clone(&shared),
            Arc::clone(&remotes),
            options.plan,
            options.delay,
        );
        // The dashboard's own poller feeds the page through
        // `publish_with_quota`; a standalone serve has to ask the endpoints
        // itself, on the slow cadence they demand.
        spawn_quota_poller(Arc::clone(&shared));
    }

    let query = match token.is_empty() {
        true => String::new(),
        false => format!("?t={token}"),
    };
    crate::elog::event(
        "serve",
        "listen",
        serde_json::json!({
            "addr": addr.to_string(),
            "token": !token.is_empty(),
            "actions": actions,
            "tunnel": options.tunnel,
            "notify": shared.notify.is_some(),
        }),
    );
    let running = Arc::new(AtomicBool::new(true));
    {
        let (shared, running) = (Arc::clone(&shared), Arc::clone(&running));
        std::thread::Builder::new()
            .name("cctop-serve-accept".into())
            .spawn(move || accept_loop(listener, shared, running))?;
    }

    // The read-only link rides the same origin as whichever link is the one to
    // hand out — public when a tunnel registered, loopback otherwise.
    let readonly_link = match shared.readonly.is_empty() {
        true => String::new(),
        false => format!("{origin}/?t={}", shared.readonly),
    };

    Ok(Serving {
        local: format!("http://127.0.0.1:{}/{query}", addr.port()),
        public: tunnel.as_ref().map(|t| format!("{}/{query}", t.url)),
        readonly: readonly_link,
        actions,
        shared,
        remotes,
        plan: options.plan,
        version: Mutex::new(0),
        _tunnel: tunnel,
        running,
        port: addr.port(),
    })
}

/// Answer connections until `running` goes false.
fn accept_loop(listener: TcpListener, shared: Arc<Shared>, running: Arc<AtomicBool>) {
    let live = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        if !running.load(Ordering::Relaxed) {
            return;
        }
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
}

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
    // `None` here still honours CCTOP_NOTIFY_URL — the fallback lives in
    // `start`, so it also covers a dashboard, which has no flag to give.
    let mut notify = None;

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
            "--notify" => notify = Some(value()?),
            other => anyhow::bail!("unknown option '{other}'\n\n{HELP}"),
        }
    }

    // Said before the call and not inside it: registering with the edge takes a
    // second or more, and `serve` has nothing else on screen to show for it.
    // The dashboard, which calls the same code, has a spinner instead — see
    // [`tunnel::start`] for why that one must not print.
    if want_tunnel {
        eprintln!("cctop: opening a trycloudflare tunnel…");
        let _ = std::io::stderr().flush();
    }
    let serving = start(Options {
        bind: bind.clone(),
        port,
        port_given,
        no_token,
        no_actions,
        tunnel: want_tunnel,
        plan,
        delay,
        hosts,
        notify,
        // The only thing in this process, so it does its own walking.
        scan: true,
    })?;
    announce(&serving, &bind, no_token);

    // Nothing left to do on this thread: the accept loop has its own. Parking
    // rather than returning is what keeps the process — and with it the tunnel
    // and every thread above — alive, since `serve` is the whole command.
    loop {
        std::thread::park();
    }
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
fn announce(serving: &Serving, bind: &str, no_token: bool) {
    if let Some(public) = &serving.public {
        eprintln!("cctop: serving on {public}");
        eprintln!("cctop: also on {}", serving.local);
        // The second link is a different credential, and labelled as such so
        // it is never handed out as the full one: a link that can only read is
        // only useful if the person holding it knows that is what they have.
        if !serving.readonly.is_empty() {
            eprintln!("cctop: read-only link — no actions: {}", serving.readonly);
        }
        eprintln!(
            "cctop: that first link is on the public internet. Anyone who has it \
             can read every session on this machine — and, unless --no-actions, \
             type at your agents, which runs commands as you. Cloudflare carries \
             the traffic and can read it. The tunnel ends when this process does."
        );
        let _ = std::io::stderr().flush();
    } else {
        eprintln!("cctop: serving on {}", serving.local);
        if !serving.readonly.is_empty() {
            eprintln!("cctop: read-only link — no actions: {}", serving.readonly);
        }
    }
    if bind != "127.0.0.1" && serving.public.is_none() {
        eprintln!(
            "cctop: bound to {bind}, so this is on your network and not just this \
             machine"
        );
    }
    if no_token {
        eprintln!(
            "cctop: no token — every process and user on this machine can read \
             your sessions"
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

/// Poll each provider's usage endpoint on the slow cadence they demand,
/// storing the rendered `/api/quota` document each time one answers.
///
/// Each provider is paced by its own last outcome — a throttled one backs off
/// without stalling the other — which is the same arithmetic the dashboard's
/// poller runs (`spawn_quota_poller` in `src/ui/runloop.rs`), minus the burn
/// log: a serve records nothing, it only reports.
fn spawn_quota_poller(shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let mut claude: Vec<crate::quota::ProfileQuota> = Vec::new();
        let mut codex: Vec<crate::quota::ProfileQuota> = Vec::new();
        let (mut claude_due, mut codex_due) = (Instant::now(), Instant::now());
        loop {
            let now = Instant::now();
            let mut changed = false;
            // Each profile is its own account with its own limits, so each is
            // asked separately. They share one due time: the interval exists
            // to be polite to the provider, and a machine with two logins is
            // not entitled to twice the requests.
            if now >= claude_due {
                claude = crate::config::accounts_for(crate::pricing::Provider::Claude)
                    .iter()
                    .map(|profile| crate::quota::ProfileQuota {
                        profile: profile.name.clone(),
                        status: crate::quota::fetch_claude(profile),
                        source: profile.source,
                    })
                    .collect();
                // Paced by whichever account is most throttled, so backing off
                // for one does not keep asking on behalf of another.
                let delay = claude
                    .iter()
                    .map(|q| q.status.retry_delay_secs(QUOTA_INTERVAL_SECS))
                    .max()
                    .unwrap_or(QUOTA_INTERVAL_SECS);
                claude_due = now + Duration::from_secs(delay);
                changed = true;
            }
            if now >= codex_due {
                codex = crate::config::accounts_for(crate::pricing::Provider::Codex)
                    .iter()
                    .map(|profile| crate::quota::ProfileQuota {
                        profile: profile.name.clone(),
                        status: crate::quota::fetch_codex(profile),
                        source: profile.source,
                    })
                    .collect();
                let delay = codex
                    .iter()
                    .map(|q| q.status.retry_delay_secs(QUOTA_INTERVAL_SECS))
                    .max()
                    .unwrap_or(QUOTA_INTERVAL_SECS);
                codex_due = now + Duration::from_secs(delay);
                changed = true;
            }
            if changed && let Ok(mut slot) = shared.quota.lock() {
                *slot = serde_json::to_string(&quota::document(&crate::quota::Quota {
                    fetched: true,
                    claude: claude.clone(),
                    codex: codex.clone(),
                }))
                .unwrap_or_else(|_| quota::EMPTY.to_string());
            }
            std::thread::sleep(QUOTA_TICK);
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
        publish(
            &shared,
            &remotes,
            &rows,
            plan,
            loader.store(),
            &mut version,
            None,
        );

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
            publish(
                &shared,
                &remotes,
                &rows,
                plan,
                loader.store(),
                &mut version,
                None,
            );
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
    store: &crate::cache::Store,
    version: &mut u64,
    usage: Option<&crate::quota::Quota>,
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
    let document = cli::json_sessions(&sessions, plan, store);
    let json = serde_json::to_string(&document).unwrap_or_else(|_| "[]".to_string());

    // Written through the same lock as the snapshot, so the page's quota panel
    // can never be newer than the rows it sits beside.
    if let Some(usage) = usage
        && let Ok(mut slot) = shared.quota.lock()
    {
        *slot = serde_json::to_string(&quota::document(usage))
            .unwrap_or_else(|_| quota::EMPTY.to_string());
    }

    *version += 1;
    let snapshot = Arc::new(Snapshot {
        version: *version,
        json,
        sessions,
        host_errors,
    });
    crate::elog::event(
        "scan",
        "snapshot",
        serde_json::json!({
            "version": snapshot.version,
            "sessions": snapshot.sessions.len(),
            "running": snapshot.sessions.iter().filter(|s| s.is_running()).count(),
            "remote_hosts": snapshot.host_errors.len(),
        }),
    );
    // The crossing is read off the pair of snapshots as they meet, which is the
    // one place both feeds — this server's own refresher and the dashboard's —
    // pass through. A webhook asked for here therefore covers both.
    let mut crossed = Vec::new();
    if let Ok(mut latest) = shared.latest.lock() {
        if let Some(hook) = &shared.notify {
            crossed = hook
                .crossed(&latest.sessions, &snapshot.sessions)
                .into_iter()
                .map(|(event, s)| (event, s.clone()))
                .collect();
        }
        *latest = snapshot;
    }
    shared.updated.notify_all();
    if let Some(hook) = &shared.notify {
        for (event, session) in crossed {
            hook.post(event, &session);
        }
    }
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

/// Which of this run's two credentials a request presented.
///
/// The distinction lives at the router rather than inside each route: the
/// check is the same for everything the read-only link may not do, and a route
/// added later that forgets it is the bug the single gate prevents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    /// The full link: every page, every GET, and the actions when they are on.
    Full,
    /// The read-only link: everything but `/api/act/*`.
    ReadOnly,
}

/// The cookie a page hands back for the run's token. Named with the port
/// because a cookie is scoped to the host alone — `127.0.0.1:7777` and
/// `127.0.0.1:9999` share one jar — and a second serve would otherwise spend
/// its visits overwriting the first one's credential.
fn cookie_name(port: u16) -> String {
    format!("cctop_access_{port}")
}

/// What the presented token buys, or `None` when it buys nothing.
fn access_for(shared: &Shared, presented: &str) -> Option<Access> {
    if shared.token.is_empty() || token_matches(&shared.token, presented) {
        return Some(Access::Full);
    }
    // The emptiness check is load-bearing: `token_matches` answers true for
    // two empty strings, and an absent read-only credential must never be one
    // a request can present.
    if !shared.readonly.is_empty() && token_matches(&shared.readonly, presented) {
        return Some(Access::ReadOnly);
    }
    None
}

/// Read one request, answer it, and let the connection close.
fn serve_connection(shared: &Shared, stream: &mut TcpStream) {
    let request = match Request::parse(stream) {
        Ok(request) => request,
        Err((status, why)) => return http::respond_error(stream, None, status, why),
    };

    // Before the route, so a wrong token cannot be used to find out which
    // routes exist. Every path is behind it, including the ones that only
    // return HTML. Either minted token opens the door; which one it was
    // decides what lies behind it. The cookie is the same credential on its
    // second visit: `?t=` gets a page in once, the page's `Set-Cookie` is what
    // a reload — which has no query left — presents instead. It adds no new
    // way in: the cookie only ever repeats a token that was already minted.
    let Some(access) = access_for(shared, request.token()).or_else(|| {
        access_for(
            shared,
            request.cookie(&cookie_name(shared.port)).unwrap_or(""),
        )
    }) else {
        crate::elog::event(
            "http",
            "request",
            serde_json::json!({"method": request.method, "path": request.path, "access": "denied"}),
        );
        return http::respond_error(
            stream,
            Some(&request),
            403,
            "missing or wrong access token — open the link cctop printed",
        );
    };
    crate::elog::event(
        "http",
        "request",
        serde_json::json!({
            "method": request.method,
            "path": request.path,
            "access": match access {
                Access::Full => "full",
                Access::ReadOnly => "readonly",
            },
        }),
    );

    // Before the router, so an armed fault covers every API route rather than
    // the handful somebody remembered to touch.
    #[cfg(feature = "debug")]
    if debug::intercept(stream, &request) {
        return;
    }

    let path = request.path.clone();
    #[cfg(feature = "debug")]
    if let Some(rest) = path.strip_prefix("/api/debug/")
        && debug::route(shared, stream, &request, rest)
    {
        return;
    }
    match path.as_str() {
        "/" => page(shared, stream, &request, DASHBOARD_HTML, access),
        "/favicon.svg" => http::respond(
            stream,
            Some(&request),
            200,
            "image/svg+xml",
            FAVICON.as_bytes(),
        ),
        "/manifest.webmanifest" => http::respond(
            stream,
            Some(&request),
            200,
            "application/manifest+json",
            MANIFEST.as_bytes(),
        ),
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
        "/api/quota" => {
            // Served from memory: the document is rendered by whoever polled
            // the endpoints last, on their schedule rather than the browser's.
            let body = shared
                .quota
                .lock()
                .map(|q| q.clone())
                .unwrap_or_else(|_| quota::EMPTY.to_string());
            http::respond(
                stream,
                Some(&request),
                200,
                "application/json; charset=utf-8",
                body.as_bytes(),
            );
        }
        "/api/search" => api_search(shared, stream, &request),
        "/api/events" => events(shared, stream, &request),
        "/insight/optimize" => api_insight(shared, stream, &request, "optimize"),
        "/insight/compare" => api_insight(shared, stream, &request, "compare"),
        "/analytics" => page(shared, stream, &request, ANALYTICS_HTML, access),
        // The whole fleet's history in one document — the analytics page
        // filters and charts it client-side, so this one read-only route is
        // all the server owes it. Untrimmed buckets are affordable here
        // because the page fetches on its own cadence, not every refresh.
        "/api/analytics" => {
            let snapshot = current(shared);
            let built = analytics::build(&snapshot.sessions, shared.plan, &shared.store);
            json(stream, &request, &built);
        }
        // What can be handed a session's work, and whether this run will act at
        // all. The page asks once and hides the controls it cannot use, rather
        // than offering buttons that answer 404.
        "/api/agents" => {
            let body = serde_json::json!({
                "actions": shared.actions && access == Access::Full,
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
        // Not under /api/act/<id>/ — a launch names no session because there is
        // none yet. The guards are the shared ones: same link, same flag, same
        // insistence on a JSON POST.
        "/api/launch" => {
            let Some(body) = may_act(shared, stream, &request, access) else {
                return;
            };
            let agent = body
                .get("agent")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let cwd = body.get("cwd").and_then(serde_json::Value::as_str);
            match actions::launch_agent(agent, cwd) {
                Ok(done) => json(stream, &request, &done),
                Err((status, why)) => http::respond_error(stream, Some(&request), status, &why),
            }
        }
        _ if path.starts_with("/session/") => page(shared, stream, &request, REPORT_HTML, access),
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
            api_act(shared, stream, &request, &path["/api/act/".len()..], access);
        }
        _ => http::respond_error(stream, Some(&request), 404, "no such page"),
    }
}

/// Both transcript-search tiers over the current snapshot.
fn api_search(shared: &Shared, stream: &mut TcpStream, request: &Request) {
    let needle = request
        .query
        .get("q")
        .map(|q| q.trim().to_lowercase())
        .unwrap_or_default();
    // Under a few characters a query is a keystroke rather than a question;
    // answering one costs a scan of every transcript for a result that is
    // nearly all of them.
    if needle.chars().count() < search::MIN_CHARS {
        return json(stream, request, &serde_json::json!({ "hits": [] }));
    }
    let snapshot = current(shared);
    let hits = search::run(&shared.topics, &snapshot.sessions, &needle);
    json(stream, request, &serde_json::json!({ "hits": hits }))
}

/// The text `cctop optimize` or `cctop compare` would print, as plain text.
///
/// Priced the way the report route is: the analysis re-parses every transcript
/// because the tool calls it reasons about are never cached, so it happens on
/// request rather than on the refresh clock — the same cost the CLI pays when
/// asked the same question.
fn api_insight(shared: &Shared, stream: &mut TcpStream, request: &Request, which: &str) {
    let analyses = crate::insight::scan(shared.plan);
    let selected = crate::insight::only(&analyses, None);
    let text = match which {
        "optimize" => crate::insight::optimize::report(&selected),
        _ => crate::insight::compare::report(&selected),
    };
    http::respond(
        stream,
        Some(request),
        200,
        "text/plain; charset=utf-8",
        text.as_bytes(),
    );
}

/// Serve the conversation for one session.
///
/// `?before=<n>` pages backwards: the window returned ends just before turn
/// `n` rather than at the newest, which is how the page reaches turns the
/// first response counted but did not send.
fn api_chat(shared: &Shared, stream: &mut TcpStream, request: &Request, id: &str) {
    let snapshot = current(shared);
    let Some(session) = find(&snapshot.sessions, id) else {
        return http::respond_error(stream, Some(request), 404, NO_SUCH_SESSION);
    };
    let before = request
        .query
        .get("before")
        .and_then(|v| v.parse::<usize>().ok());
    json(stream, request, &chat::build(session, before));
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

/// The guards every acting route shares, answered here once: a read-only link,
/// a `--no-actions` serve, and a request that is not a JSON POST each get their
/// own answer rather than a generic refusal. Returns the parsed body when the
/// request may act — a route that forgets to go through this is the bug the
/// shared shape exists to prevent.
fn may_act(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    access: Access,
) -> Option<serde_json::Value> {
    // Before every other guard, because it is not one: this is the link doing
    // what it was minted to do, and the answer names that rather than leaning
    // on a flag the read-only page never had.
    if access == Access::ReadOnly {
        http::respond_error(stream, Some(request), 403, "this link is read-only");
        return None;
    }
    if !shared.actions {
        http::respond_error(
            stream,
            Some(request),
            403,
            "this cctop serve is read-only — restart it without --no-actions, \
             and with a token, to act on a session",
        );
        return None;
    }
    // A `POST` with a JSON body, both of which are load-bearing: see the
    // module docs in `http` for why a form on another origin cannot be one.
    if !request.wants_json() {
        http::respond_error(
            stream,
            Some(request),
            405,
            "an action is a POST with a JSON body",
        );
        return None;
    }
    match request.json() {
        Ok(body) => Some(body),
        Err((status, why)) => {
            http::respond_error(stream, Some(request), status, why);
            None
        }
    }
}

/// Do something to a session: `/api/act/<verb>/<id>`.
///
/// One route for the three verbs rather than three, because the guards in front
/// of them are the whole security surface and they are identical — a route added
/// later that forgets one of them is the bug this shape prevents.
fn api_act(shared: &Shared, stream: &mut TcpStream, request: &Request, rest: &str, access: Access) {
    let Some(body) = may_act(shared, stream, request, access) else {
        return;
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
    // Also out of shape, and also not about the session it names: an image is
    // filed on this machine and the answer is where. The session id rides along
    // so the route is the same shape as its neighbours — one place that checks
    // the token, insists on a JSON POST, and refuses a read-only serve.
    if verb == "image" {
        return match actions::image(&field("data")) {
            Ok(filed) => json(stream, request, &filed),
            Err((status, why)) => http::respond_error(stream, Some(request), status, &why),
        };
    }
    // Answered before the others because it does not answer in their shape: a
    // terminal is a link and a reach, not a sentence about what was done.
    if verb == "terminal" {
        return match actions::terminal(session) {
            Ok(terminal) => json(stream, request, &terminal),
            Err((status, why)) => http::respond_error(stream, Some(request), status, &why),
        };
    }
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
fn page(shared: &Shared, stream: &mut TcpStream, request: &Request, html: &str, access: Access) {
    // Which credential the page carries — and whether it may act — is decided
    // by the one the request arrived with, not by the run: the read-only link
    // opens the same page wired to the narrower token, so a page it hands out
    // can neither act nor leak the token that could.
    let (credential, actions) = match access {
        Access::Full => (shared.token.as_str(), shared.actions),
        Access::ReadOnly => (shared.readonly.as_str(), false),
    };
    // JSON-encoded rather than pasted between quotes: the token is hex today,
    // and a literal substituted into script is exactly the shape of bug that
    // outlives the reason it was safe.
    let token = serde_json::to_string(&credential).unwrap_or_else(|_| "\"\"".to_string());
    // The report page links back to the table, and the link has to carry the
    // credential or it lands on a 403. Hex, so nothing in it needs escaping —
    // asserted by the tests rather than assumed, since the generator could change.
    let back = match credential.is_empty() {
        true => String::new(),
        false => format!("?t={credential}"),
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
        .replace("__CCTOP_THEME__", THEME_JS)
        .replace(
            "\"__CCTOP_ACTIONS__\"",
            match actions {
                true => "true",
                false => "false",
            },
        )
        .replace("\"__CCTOP_TOKEN__\"", &token)
        .replace("\"__CCTOP_HOME__\"", &home)
        .replace("__CCTOP_BACK__", &back)
        .replace("__CCTOP_VERSION__", env!("CARGO_PKG_VERSION"));

    // Hand the credential back as a cookie so a reload — which has no `?t=`
    // left, the page having stripped it — still gets in. Only a request that
    // presented a token mints one: a page that got in on the cookie already
    // has it, and a read-only link does not push a full cookie out of a jar
    // that holds one.
    let mut headers = String::new();
    if !request.token().is_empty() && !credential.is_empty() {
        let held = request
            .cookie(&cookie_name(shared.port))
            .and_then(|c| access_for(shared, c));
        if !(access == Access::ReadOnly && held == Some(Access::Full)) {
            headers = format!(
                "Set-Cookie: {}={credential}; Path=/; HttpOnly; SameSite=Strict\r\n",
                cookie_name(shared.port)
            );
        }
    }
    http::respond_extra(
        stream,
        Some(request),
        200,
        "text/html; charset=utf-8",
        body.as_bytes(),
        &headers,
    );
}

/// Hold an SSE stream open, sending each new snapshot as it lands.
fn events(shared: &Shared, stream: &mut TcpStream, _request: &Request) {
    let Ok(mut sse) = EventStream::open(stream) else {
        return;
    };
    crate::elog::event("sse", "open", serde_json::json!({}));

    let mut sent = 0u64;
    // Which write lost the client is the difference between "browser closed"
    // and "network went" — the reason is kept rather than collapsed.
    let by = loop {
        // The wait is what makes an idle stream free: no polling, and one
        // wakeup per refresh rather than one per connection per tick.
        let snapshot = {
            let Ok(latest) = shared.latest.lock() else {
                break "lock";
            };
            let (latest, _) = match shared
                .updated
                .wait_timeout_while(latest, SSE_KEEPALIVE, |s| s.version <= sent)
            {
                Ok(pair) => pair,
                Err(_) => break "wait",
            };
            Arc::clone(&latest)
        };

        // Nothing new within the keepalive window, so send the bytes that keep
        // the connection counted as alive — and that surface a client which
        // quietly went away, since a write is the only thing that can.
        if snapshot.version <= sent {
            if sse.keepalive().is_err() {
                break "keepalive";
            }
            continue;
        }

        if sse.send("sessions", &snapshot.json).is_err() {
            break "send";
        }
        sent = snapshot.version;
    };
    crate::elog::event(
        "sse",
        "close",
        serde_json::json!({ "by": by, "sent": sent }),
    );
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
        for html in [DASHBOARD_HTML, REPORT_HTML, ANALYTICS_HTML] {
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

    fn shared(token: &str, readonly: &str) -> Shared {
        Shared {
            token: token.to_string(),
            readonly: readonly.to_string(),
            actions: true,
            port: 7777,
            plan: Plan::Retail,
            latest: Mutex::new(Arc::new(Snapshot {
                version: 0,
                json: "[]".to_string(),
                sessions: Vec::new(),
                host_errors: Vec::new(),
            })),
            updated: Condvar::new(),
            store: crate::cache::Store::new(),
            quota: Mutex::new(quota::EMPTY.to_string()),
            topics: Mutex::new(search::Topics::default()),
            notify: None,
        }
    }

    #[test]
    fn the_readonly_token_opens_the_page_but_not_the_actions() {
        // `access_for` is the single gate the whole distinction hangs on, so
        // its table is what is worth pinning: both minted tokens get in, and
        // which one it was is what `/api/act/*` later refuses on.
        let guarded = shared("full", "view");
        assert_eq!(access_for(&guarded, "full"), Some(Access::Full));
        assert_eq!(access_for(&guarded, "view"), Some(Access::ReadOnly));
        assert_eq!(access_for(&guarded, "wrong"), None);
        assert_eq!(access_for(&guarded, ""), None);

        // A tokenless run has nothing to withhold, so it grants full access
        // to everything — the same answer `--no-token` has always given.
        let open = shared("", "");
        assert_eq!(access_for(&open, ""), Some(Access::Full));
        assert_eq!(access_for(&open, "anything"), Some(Access::Full));
    }

    /// Ask `api_search` the way the router does — a request parsed off a real
    /// socket — and hand back whatever it wrote.
    fn api_search_body(shared: &Shared, target: &str) -> String {
        use std::io::Read;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        client
            .write_all(format!("GET {target} HTTP/1.1\r\n\r\n").as_bytes())
            .unwrap();
        let request = Request::parse(&server).unwrap();
        api_search(shared, &mut server, &request);
        // Closing the answering half is what lets the client read the response
        // to its end rather than waiting out the connection.
        drop(server);
        let mut raw = String::new();
        client.read_to_string(&mut raw).unwrap();
        raw
    }

    /// A request with no `q` — or one shorter than a question — is an empty
    /// answer, not an error and not a scan of every transcript.
    #[test]
    fn a_search_without_a_real_query_finds_nothing() {
        let shared = shared("", "");
        for target in ["/api/search", "/api/search?q=", "/api/search?q=ab"] {
            let body = api_search_body(&shared, target);
            assert!(body.contains("\"hits\":[]"), "{target}: {body}");
        }
    }

    /// The route end to end, minus the listener: a session whose transcript
    /// holds the word comes back under `session_id`, the field the page joins
    /// its rows on.
    ///
    /// Only the hit direction is pinned here. A miss asks the topical tier,
    /// which on a machine with the model fetched would load it and rewrite the
    /// real embedding cache — a side effect no unit test should have; the
    /// literal miss itself is covered in `session::search`'s tests.
    #[test]
    fn a_search_names_the_session_whose_transcript_matched() {
        let path =
            std::env::temp_dir().join(format!("cctop-serve-search-{}.jsonl", std::process::id()));
        std::fs::write(&path, "{\"text\":\"please fix the flywheel\"}\n").unwrap();
        let mut s = Session::new(crate::pricing::Provider::Claude, "sess-1".into());
        s.data_file = Some(path.clone());

        let shared = shared("", "");
        *shared.latest.lock().unwrap() = Arc::new(Snapshot {
            version: 1,
            json: "[]".to_string(),
            sessions: vec![s],
            host_errors: Vec::new(),
        });

        let body = api_search_body(&shared, "/api/search?q=flywheel");
        let _ = std::fs::remove_file(path);
        assert!(body.contains("\"session_id\":\"sess-1\""), "{body}");
        assert!(body.contains("flywheel"), "{body}");
    }
}
