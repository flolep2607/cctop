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
//!   `--token-file` keeps both across restarts; see [`tokens`]. The one path
//!   outside the gate is `GET /metrics`, which carries aggregates and no
//!   transcript — see [`metrics`] for why a scrape needs no secret.
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

/// `/api/address`: choosing the dashboard's address, and an agent's.
mod address;
/// Cross-session aggregation behind `/api/analytics` — the data the
/// analytics page charts, built from the snapshot plus cached extractions.
mod analytics;
/// The session API's handlers: search, the conversation, the report, the
/// acting routes and the provider-status document.
mod api;
/// Routes that only exist in a build that asked for them. Not a default
/// feature, so a released cctop contains none of this — see the module docs.
#[cfg(feature = "debug")]
mod debug;
/// `/api/events`: the table and a session's marks, streamed as what changed.
mod events;
mod http;
/// A Cloudflare Access login as a way in, beside the tokens.
mod identity;
/// `/api/access`: who may log in, changed from the page.
mod invites;
/// Replies rendered from markdown to HTML for the conversation page.
mod markdown;
/// `/metrics`, the snapshot in Prometheus's text format.
mod metrics;
/// The `--notify` webhook. Its send is `cctop_core::notify::post`, which the
/// TUI POSTs crossings to `$CCTOP_NOTIFY_URL` through too — one transport, two
/// triggers.
mod notify;
/// The threads that keep the snapshot, the quota and the provider status fresh.
mod pollers;
mod quota;
/// The session routes answered by asking another machine: an ssh host or a
/// sibling serve.
mod remote;
mod search;
/// The account tunnel's share hostnames: rmux's app and the share socket,
/// on a listener of their own that has no other route.
mod share_host;
mod ssh;
/// The TUI's tabs, read from rmux, for `/api/tabs`.
mod tabs;
/// rmux's terminal app, served here so a session page can frame it.
mod term;
/// The run's credentials, and `--token-file`, which keeps them across runs.
pub mod tokens;
/// The web UI's TypeScript types, generated from the structs above and checked
/// against `web/src/lib/gen/`.
#[cfg(test)]
mod ts_types;

// What the page reads and does to a session is core's, shared with the
// dashboard and the command line: the conversation, the report, the export,
// the handoff brief, and the actions.
use api::{
    api_access, api_act, api_chat, api_chat_markdown, api_insight, api_report, api_search, api_tab,
    find, may_act, provider_status_route,
};
use cctop_core::actions;
use cctop_core::fleet;
use cctop_core::pricing::Plan;
use cctop_core::session::Session;
use cctop_core::tunnel;
use events::events;
use http::{Request, json};
use pollers::{
    lock_provider_status, publish, remember, spawn_host_poller, spawn_provider_status_poller,
    spawn_quota_poller, spawn_refresher,
};
use remote::{forward, peer_of};
use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

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

/// The longest an SSE stream goes without writing anything before it sends a
/// `ping` event.
///
/// Idle connections have to produce traffic or the hops in between drop them,
/// and a browser that has gone away only surfaces as a write error once
/// something is written at it. The page holds the other end of this bargain:
/// a stream it has heard nothing on for a little over twice this is one it
/// closes and reopens (`STALE_AFTER` in `web/src/hooks/use-live.ts`), so the
/// two numbers move together.
const SSE_KEEPALIVE: Duration = Duration::from_secs(15);

/// What a request naming no session, or an ambiguous prefix of one, is told.
///
/// One string because four routes say it, and four spellings of one refusal is
/// how a page comes to show three different messages for the same mistake.
const NO_SUCH_SESSION: &str = "no session with that id, or the prefix matches more than one";

/// The web UI: the React app in `web/`, built by `npm run build` into one HTML
/// file with every script, stylesheet and font inlined, and committed.
///
/// `include_str!` rather than a directory served off disk: an installed cctop
/// is one binary, and a page that loads its own files is a page that breaks the
/// moment the binary is moved. It is also what lets the response promise a
/// content policy that forbids loading anything at all. What the server knows
/// and the page cannot it fetches from `/api/config`; see [`app_config`]. Every
/// page — the table, a session, the workspace, analytics — is a route inside
/// it. Compressed by `build.rs`, so the binary carries it as brotli and gzip.
static APP_PAGE: std::sync::LazyLock<http::Packed> =
    std::sync::LazyLock::new(|| http::built!("src/assets/app/index.html"));

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
static FAVICON_P: std::sync::LazyLock<http::Packed> =
    std::sync::LazyLock::new(|| http::Packed::plain(FAVICON.as_bytes()));

static MANIFEST_P: std::sync::LazyLock<http::Packed> =
    std::sync::LazyLock::new(|| http::Packed::plain(MANIFEST.as_bytes()));

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
    store: cctop_core::cache::Store,
    /// The `/api/quota` document, already serialised.
    ///
    /// Rendered by whoever produced the numbers — the standalone serve's own
    /// poller, or the dashboard feeding [`Serving::publish_table`] — so
    /// the route itself is a memory read and never touches the rate-limited
    /// usage endpoints on a browser's schedule.
    quota: Mutex<String>,
    /// What the vendors' status pages last said.
    ///
    /// Written like the quota, by whoever polled — the standalone serve's own
    /// poller, or the dashboard through [`Serving::publish_table`] — and read
    /// by `/api/provider-status`, which sets it against the rows of the moment
    /// rather than the rows of the last poll. Kept unrendered for that reason:
    /// the alerts depend on which sessions are failing *now*.
    provider_status: Mutex<cctop_core::provider_status::Status>,
    /// The topical tier behind `/api/search`, built lazily on the first query
    /// that wants it.
    topics: Mutex<search::Topics>,
    /// The webhook `--notify` points at, when one was asked for.
    notify: Option<notify::Webhook>,
    /// The `--host` machines by target name, so a route for a remote row can
    /// ask the cctop that actually holds the transcript — see [`remote_json`].
    /// Present even when `scan` is off: a dashboard-hosted serve still owes
    /// its remote rows an answer.
    hosts: HashMap<String, fleet::Host>,
    /// The web launcher's ssh: what it may connect to and what it learned.
    /// Reached only past `may_act` — see [`ssh`].
    ssh: ssh::Reach,
    /// Whether a `Host` is one of the account tunnel's share hostnames,
    /// which this server refuses whatever token comes with it — see
    /// [`share_host`]. A function rather than the call itself so a test can
    /// name a share host without lending a tunnel.
    is_share_host: fn(&str) -> bool,
    /// Whether a `Host` is one the account's tunnel routes to the page — the
    /// dashboard's hostname or the token hostname — for the lock that
    /// refuses a token through the tunnel while public token links are off.
    is_tunnel_host: fn(&str) -> bool,
    /// The connected account and its renames, for `/api/address` — see
    /// [`address`]. A trait object so a test can rename against a fake
    /// Cloudflare without touching `config.toml`.
    addresses: Arc<dyn address::Addresses>,
    /// Who a Cloudflare Access login is, and what the list lets them do —
    /// see [`identity`]. A trait object so a test can log in with a key of
    /// its own instead of a real team's.
    identities: Arc<dyn identity::Identities>,
    /// This machine's tunnel token, when it serves on the account's tunnel:
    /// what a sibling on the account signs with to be let in — see
    /// [`cctop_core::peer`]. Empty otherwise, which lets no signature in.
    peer_secret: String,
}

/// One publish of the whole table.
struct Snapshot {
    version: u64,
    /// The `--json` document, already serialised — every SSE client sends the
    /// same bytes, so they are rendered once per refresh rather than once per
    /// client.
    json: String,
    /// Each row of `json` on its own, by session id, for a stream that follows
    /// one session — see [`events`]. `json` is these joined, so the two cannot
    /// disagree.
    rows: Vec<(String, String)>,
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
  --tunnel         Also reach the page from anywhere: over your own Cloudflare
                   tunnel when one is connected (`cctop tunnel setup`), at a
                   hostname that stays the same, else over a trycloudflare
                   quick tunnel at a new one each run. Needs nothing installed,
                   lasts as long as this process, and puts the link on the
                   public internet — so the token is what stands between it
                   and your agents. With your own tunnel the tokens are kept
                   across restarts, as --token-file does, so a bookmark works
  --tunnel=quick   A quick tunnel even when your own is connected
  --plan <PLAN>    Billing plan for cost figures: retail, max, or included
                   [default: retail]
  --delay <SECS>   Seconds between refreshes [default: 2]
  --host <HOST>    Also serve the sessions on another machine, over ssh.
                   Repeatable; same syntax as `cctop --host`
  --notify <URL>   POST a JSON event to this URL when a session crosses into
                   waiting or asking — once per crossing, on a short deadline.
                   [env: CCTOP_NOTIFY_URL]
  --token-file <PATH>
                   Keep the tokens across restarts: read them from PATH, or
                   mint them and write PATH (mode 600) when it does not exist.
                   A file other users can read, or do not own, is refused
  --rotate-token   With --token-file: replace the tokens in it, revoking every
                   link built on the old ones
  -h, --help       Print this help

The page shows each session's conversation, what it edited, and what it can
reach, and it can send a prompt to a live session, resume a dead one, or hand
one to a different agent. Whoever holds the link can do all of that, which is
why the link carries a token and the default is loopback only.
";

/// Parse `cctop serve`'s own flags and run the server until interrupted.
///
/// Flags are parsed here rather than in `cli::Args` for the reason
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
    /// With `tunnel`: a quick tunnel even when an account is connected.
    pub quick_tunnel: bool,
    pub plan: Plan,
    pub delay: Duration,
    /// The machines whose sessions this serve shows, already parsed.
    ///
    /// `Host`s rather than spec strings because the dashboard's copy was
    /// collected once at startup — passing specs back through `collect` here
    /// would merge `$CCTOP_HOSTS` a second time for it.
    pub hosts: Vec<fleet::Host>,
    /// Where to POST when a session crosses into waiting or asking.
    ///
    /// `None` still falls back to `CCTOP_NOTIFY_URL` inside [`start`], which is
    /// how a dashboard-hosted serve — no flag to have asked with — gets one.
    pub notify: Option<String>,
    /// Credentials to serve with instead of minting fresh ones — `--token-file`
    /// after [`tokens::load_or_create`] has read or written it.
    ///
    /// Loaded by the caller rather than here so `run` can say which of the two
    /// happened. The dashboard passes the account's when it serves over the
    /// account's tunnel, and none otherwise.
    pub tokens: Option<tokens::Tokens>,
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
            quick_tunnel: false,
            plan: Plan::Retail,
            delay: Duration::from_secs(2),
            hosts: Vec::new(),
            notify: None,
            tokens: None,
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
    /// Why a quick tunnel stands in for the account's, when one does — to be
    /// said wherever the link is, since it is not the link that was expected.
    pub tunnel_fallback: Option<String>,
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
    /// The share hostnames' server, after the tunnel so it outlives the
    /// routes that point at it.
    _front: Option<share_host::Front>,
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

    /// Show the page these rows, this usage reading and these status pages,
    /// replacing whatever it was showing.
    ///
    /// For a dashboard-hosted server, which has all three already. Cheap enough
    /// to call on every refresh: it costs one JSON encode of what the table is
    /// already holding, and it is what wakes the event stream. The quota and
    /// the status pages come along because the dashboard polls both for its
    /// own panes — the page shares those readings rather than standing up a
    /// second poller against the same endpoints, and the two can never
    /// disagree.
    pub fn publish_table(
        &self,
        sessions: &[Session],
        quota: &cctop_core::quota::Quota,
        provider_status: &cctop_core::provider_status::Status,
    ) {
        lock_provider_status(&self.shared).clone_from(provider_status);
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
    // A second credential for the same pages minus the actions — see
    // `Shared::readonly` for why read-only is a token rather than a flag.
    let tokens = match (options.no_token, options.tokens) {
        (true, Some(_)) => anyhow::bail!(
            "--token-file with --no-token names tokens and then serves without them. \
             Drop one of the two."
        ),
        (true, None) => tokens::Tokens::none(),
        (false, Some(given)) => given,
        (false, None) => tokens::Tokens::fresh(),
    };
    let token = tokens.full;
    // The token is the whole authorisation story for an action, so there are no
    // actions without one. `--no-token` is already documented as "every process
    // and user on this machine can read your sessions"; letting that also mean
    // "and type at your agents" is a different sentence, and not one anybody
    // reads a flag expecting.
    let actions = !options.no_actions && !options.no_token;
    cctop_core::telemetry::record(cctop_core::telemetry::Feature::Serve);
    if options.tunnel {
        cctop_core::telemetry::record(cctop_core::telemetry::Feature::Tunnel);
    }
    // Started before anything is announced, so the link works when it is read,
    // and before the accept loop because there is nothing to reach yet.
    // The account's share hostnames land on a front of their own, which is
    // up before the tunnel can route anything to it. Only for a tunnel that
    // may be the account's: a quick one has no share hostname.
    let front = match options.tunnel && !options.quick_tunnel {
        true => Some(share_host::Front::start(Arc::new(tunnel::share_upstream))?),
        false => None,
    };
    let share_front = front.as_ref().map(share_host::Front::port);
    let tunnel = match (options.tunnel, options.quick_tunnel) {
        (true, true) => Some(tunnel::start(addr.port(), tunnel::Want::Quick, None)?),
        (true, false) => Some(tunnel::start(addr.port(), tunnel::Want::Auto, share_front)?),
        (false, _) => None,
    };

    // The origin a link to hand out is built on: the tunnel's when there is
    // one, since a link that names loopback works only on the machine that
    // made it — which is also why a notification's link is built on it.
    //
    // Behind Access, a token link goes on the token hostname — the
    // dashboard's own asks for a login first — and with public links off
    // there is no token link through the tunnel at all: the page's address
    // is handed out bare, for whoever can log in.
    let access = tunnel.as_ref().and_then(|_| tunnel::access_settings());
    let token_origin: Option<String> = tunnel.as_ref().and_then(|t| match &access {
        Some(settings) if t.kind == tunnel::Kind::Account => settings.token_origin(&t.url),
        Some(settings) => settings.public_links.then(|| t.url.clone()),
        None => Some(t.url.clone()),
    });
    let origin = token_origin
        .clone()
        .or_else(|| tunnel.as_ref().map(|t| t.url.clone()))
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", addr.port()));
    let query = match token.is_empty() {
        true => String::new(),
        false => format!("?t={token}"),
    };
    let local = format!("http://127.0.0.1:{}/{query}", addr.port());
    let public = tunnel.as_ref().map(|t| match &token_origin {
        Some(origin) => format!("{origin}/{query}"),
        None => format!("{}/", t.url),
    });
    // The webhook links what `Serving::best` hands out, for the reason
    // `cctop_core::notify::session_link` gives.
    let link_base = public.clone().unwrap_or_else(|| local.clone());

    let shared = Arc::new(Shared {
        token: token.clone(),
        readonly: tokens.readonly,
        actions,
        port: addr.port(),
        plan: options.plan,
        latest: Mutex::new(Arc::new(Snapshot {
            version: 0,
            json: "[]".to_string(),
            rows: Vec::new(),
            sessions: Vec::new(),
            host_errors: Vec::new(),
        })),
        updated: Condvar::new(),
        store: cctop_core::cache::Store::new(),
        quota: Mutex::new(quota::EMPTY.to_string()),
        provider_status: Mutex::default(),
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
            .map(|target| notify::Webhook::new(target, link_base)),
        hosts: options
            .hosts
            .iter()
            .map(|host| (host.target.clone(), host.clone()))
            .collect(),
        ssh: ssh::Reach::ssh(),
        is_share_host: tunnel::is_share_host,
        is_tunnel_host: tunnel::is_tunnel_host,
        addresses: Arc::new(address::Connected),
        identities: Arc::new(identity::Configured::new()),
        // Only the account's own tunnel brings a sibling here: a quick
        // tunnel's hostname is on no account, and loopback is not the road a
        // sibling takes.
        peer_secret: tunnel
            .as_ref()
            .filter(|t| t.kind == tunnel::Kind::Account)
            .and_then(|_| tunnel::account())
            .map(|a| a.token)
            .unwrap_or_default(),
    });

    let remotes = Arc::new(Mutex::new(Remotes::default()));
    // A dashboard-hosted serve leaves the polling to the dashboard, which is
    // already running one loop per host — spawning these anyway would poll
    // every remote twice.
    if options.scan {
        for host in &options.hosts {
            spawn_host_poller(host.clone(), Arc::clone(&remotes));
        }
        // The account's other cctops, found rather than named. The same
        // keep-the-last-rows-and-say-why as an ssh host's poller.
        let remotes = Arc::clone(&remotes);
        cctop_core::peer::spawn(move |machine, snapshot| {
            remember(&remotes, machine, snapshot);
            true
        });
    }
    if options.scan {
        spawn_refresher(
            Arc::clone(&shared),
            Arc::clone(&remotes),
            options.plan,
            options.delay,
        );
        // The dashboard's own poller feeds the page through
        // `publish_table`; a standalone serve has to ask the endpoints
        // itself, on the slow cadence they demand.
        spawn_quota_poller(Arc::clone(&shared));
        // Likewise the status pages: the dashboard hands over its own answers,
        // and only a serve with no dashboard behind it asks the vendor.
        spawn_provider_status_poller(Arc::clone(&shared));
    }

    cctop_core::elog::event(
        "serve",
        "listen",
        serde_json::json!({
            "addr": addr.to_string(),
            "token": !token.is_empty(),
            "actions": actions,
            "tunnel": options.tunnel,
            "tunnel_kind": tunnel.as_ref().map(|t| match t.kind {
                tunnel::Kind::Quick => "quick",
                tunnel::Kind::Account => "account",
            }),
            "tunnel_fallback": tunnel.as_ref().is_some_and(|t| t.fallback.is_some()),
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
    let readonly_link =
        match shared.readonly.is_empty() || (tunnel.is_some() && token_origin.is_none()) {
            true => String::new(),
            false => format!("{origin}/?t={}", shared.readonly),
        };
    Ok(Serving {
        local,
        public,
        tunnel_fallback: tunnel.as_ref().and_then(|t| t.fallback.clone()),
        readonly: readonly_link,
        actions,
        shared,
        remotes,
        plan: options.plan,
        version: Mutex::new(0),
        _tunnel: tunnel,
        _front: front,
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
    let mut quick_tunnel = false;
    let mut plan = Plan::Retail;
    let mut delay = Duration::from_secs(2);
    let mut hosts: Vec<String> = Vec::new();
    // `None` here still honours CCTOP_NOTIFY_URL — the fallback lives in
    // `start`, so it also covers a dashboard, which has no flag to give.
    let mut notify = None;
    let mut token_file: Option<std::path::PathBuf> = None;
    let mut rotate_token = false;

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
            "--tunnel=quick" => {
                want_tunnel = true;
                quick_tunnel = true;
            }
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
            "--token-file" => token_file = Some(value()?.into()),
            "--rotate-token" => rotate_token = true,
            other => anyhow::bail!("unknown option '{other}'\n\n{HELP}"),
        }
    }

    // Said before the call and not inside it: registering with the edge takes a
    // second or more, and `serve` has nothing else on screen to show for it.
    // The dashboard, which calls the same code, has a spinner instead — see
    // [`tunnel::start`] for why that one must not print.
    // Before the listener, so a refused file costs no socket and the reason is
    // the only thing on screen.
    // A stable hostname with a fresh token every run is still a dead
    // bookmark, so a connected account keeps the tokens the way --token-file
    // does, in a file of its own — the same one the dashboard's serve keeps
    // them in, so a bookmark opens whichever of the two is up.
    let account_tunnel = want_tunnel && !quick_tunnel && tunnel::account().is_some();
    if account_tunnel && token_file.is_none() && !no_token {
        token_file = Some(account_token_file());
    }
    let tokens = match &token_file {
        // `start` refuses this too; asked here first so the answer is the
        // contradiction, not whatever was wrong with the file.
        Some(_) if no_token => anyhow::bail!(
            "--token-file with --no-token names tokens and then serves without them. \
             Drop one of the two."
        ),
        Some(path) => {
            let (tokens, how) = tokens::load_or_create(path, rotate_token)?;
            match how {
                tokens::Loaded::Reused => eprintln!(
                    "cctop: tokens from {} — links from earlier runs still work",
                    path.display()
                ),
                tokens::Loaded::Created => eprintln!(
                    "cctop: new tokens written to {} — they outlive this run; \
                     --rotate-token replaces them",
                    path.display()
                ),
            }
            Some(tokens)
        }
        None if rotate_token => anyhow::bail!(
            "--rotate-token rotates the tokens in a --token-file; without one, \
             every run already mints new tokens"
        ),
        None => None,
    };
    if want_tunnel {
        match account_tunnel {
            true => eprintln!("cctop: opening your Cloudflare tunnel…"),
            false => eprintln!("cctop: opening a trycloudflare quick tunnel…"),
        }
        let _ = std::io::stderr().flush();
    }
    let serving = start(Options {
        bind: bind.clone(),
        port,
        port_given,
        no_token,
        no_actions,
        tunnel: want_tunnel,
        quick_tunnel,
        plan,
        delay,
        hosts: fleet::Host::collect(&hosts),
        notify,
        tokens,
        // The only thing in this process, so it does its own walking.
        scan: true,
    })?;
    announce(&serving, &bind, no_token);
    // Only someone at a terminal can have read the notice; a serve under
    // systemd sends nothing until one has.
    if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        cctop_core::telemetry::notice_once();
    }
    cctop_core::telemetry::start();
    // Hooks brought in line with this binary, as the dashboard does at every
    // start, and off this thread for the same reason: nothing about serving
    // waits on five agents' config files. Silent when there was nothing to do.
    std::thread::spawn(|| {
        for line in cctop_core::hook::repair(None).terminal_lines() {
            eprintln!("{line}");
        }
    });
    // The masters the web launcher connected to are given back on the way
    // out, as the dashboard gives back its own.
    ssh::release_on_signal();

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

/// Where a connected account keeps `cctop serve`'s tokens between runs: what
/// `--token-file` would name, chosen for the user. Beside the config, which is
/// already the user's own and owner-only.
pub fn account_token_file() -> std::path::PathBuf {
    cctop_core::config::config_base()
        .join("cctop")
        .join("serve-tokens")
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
    if let Some(why) = &serving.tunnel_fallback {
        eprintln!(
            "cctop: {why}. Serving over a quick tunnel instead, at an address \
             that changes every run."
        );
    }
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
/// See [`cctop_core::util::random_bytes`] for where the entropy comes from and what
/// it is worth: enough that a token cannot be guessed from across a network,
/// which is why the default bind is loopback and this is defence in depth
/// rather than the defence.
fn new_token() -> String {
    cctop_core::util::random_hex(TOKEN_BYTES)
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

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

/// Which of this run's two credentials a request presented.
///
/// The distinction lives at the router rather than inside each route: the
/// check is the same for everything the read-only link may not do, and a route
/// added later that forgets it is the bug the single gate prevents.
///
/// Ordered by what each is worth, read-only first, so the better of two ways
/// in is their `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Access {
    /// The read-only link: everything but `/api/act/*`.
    ReadOnly,
    /// The full link: every page, every GET, and the actions when they are on.
    Full,
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

/// Whether `request` came in through a Cloudflare tunnel rather than from
/// this machine: its `Host` is one the account's tunnel routes to the page,
/// or it carries the headers the edge adds to every request it forwards —
/// which a quick tunnel's do too. Anything on loopback can write those
/// headers, but writing them only takes its own token away.
fn through_tunnel(shared: &Shared, request: &Request) -> bool {
    (shared.is_tunnel_host)(request.host())
        || request.headers().iter().any(|(k, _)| {
            k.eq_ignore_ascii_case("cf-ray") || k.eq_ignore_ascii_case("cf-connecting-ip")
        })
}

/// What the access cookie's `Secure` attribute is, empty when it may not
/// carry one.
///
/// A `Secure` cookie is dropped by any browser that received it over plain
/// HTTP — and this listener speaks plain HTTP either way, since the tunnel
/// terminates TLS at Cloudflare's edge and forwards here, and a machine's
/// own page is opened at `http://127.0.0.1:7777`. So the attribute goes on
/// only when the browser's side of that hop was HTTPS: the edge forwards
/// `X-Forwarded-Proto: https`, and a request [`through_tunnel`] came through
/// the edge however it is labelled. Anything that can reach this port can
/// write those headers, but writing them only takes its own cookie away —
/// the browser refuses a `Secure` cookie set over plain HTTP, so a spoofed
/// `https` loses the reload the cookie exists for and nothing else.
fn secure_attribute(shared: &Shared, request: &Request) -> &'static str {
    let forwarded = request.headers().iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("x-forwarded-proto") && v.eq_ignore_ascii_case("https")
    });
    match forwarded || through_tunnel(shared, request) {
        true => "; Secure",
        false => "",
    }
}

/// What a Cloudflare Access login on `request` is worth, with whose it is,
/// or `None` when it carries none or one that does not check out. A refusal
/// is logged with its reason, never with the token.
fn access_login(shared: &Shared, request: &Request) -> Option<(Access, String)> {
    let jwt = request
        .headers()
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(identity::HEADER))
        .map(|(_, v)| v.trim())
        .filter(|v| !v.is_empty())?;
    match shared.identities.check(jwt) {
        Ok((email, cctop_core::cloudflare::access::Level::Full)) => Some((Access::Full, email)),
        Ok((email, cctop_core::cloudflare::access::Level::Read)) => Some((Access::ReadOnly, email)),
        // Every request through the edge carries one, so a serve with no
        // Access set up would log every request: only a real refusal is.
        Err(identity::Why::NotSetUp) => None,
        Err(why) => {
            cctop_core::elog::event(
                "http",
                "access-login",
                serde_json::json!({"refused": format!("{why:?}")}),
            );
            None
        }
    }
}

/// Read one request, answer it, and let the connection close.
fn serve_connection(shared: &Shared, stream: &mut TcpStream) {
    let request = match Request::parse(stream) {
        Ok(request) => request,
        Err((status, why)) => return http::respond_error(stream, None, status, why),
    };

    // A share hostname is not a way in, with any token or none. The tunnel
    // sends those to the share front and never here; this is the second
    // lock, for the day something routes one here anyway. Before `/metrics`,
    // which is outside the token's gate but not outside this one.
    if (shared.is_share_host)(request.host()) {
        return http::respond_error(stream, Some(&request), 404, share_host::NOT_HERE);
    }

    // The one route in front of the gate. It is aggregate counts, costs and
    // short session ids — no transcript, title or prompt — and what it says is
    // meant to be copied into a metrics store, so a scrape config should not
    // have to hold a secret to say it. GET and HEAD only: it is a read, and a
    // POST here is not a scrape. Answering it here rather than in the router
    // keeps the gate below unconditional for every other path.
    if request.path == "/metrics" {
        if request.method == "POST" {
            return http::respond_error(stream, Some(&request), 405, "/metrics answers GET");
        }
        let snapshot = current(shared);
        let body = metrics::render(
            &snapshot.sessions,
            shared.plan,
            &shared.store,
            snapshot.host_errors.len(),
        );
        return http::respond(
            stream,
            Some(&request),
            200,
            metrics::CONTENT_TYPE,
            body.as_bytes(),
        );
    }

    // Before the route, so a wrong token cannot be used to find out which
    // routes exist. Every path is behind it, including the ones that only
    // return HTML. Either minted token opens the door; which one it was
    // decides what lies behind it. The cookie is the same credential on its
    // second visit: `?t=` gets a page in once, the page's `Set-Cookie` is what
    // a reload — which has no query left — presents instead. It adds no new
    // way in: the cookie only ever repeats a token that was already minted.
    // `Authorization: Bearer` is the same credential again, in the form a
    // script or HTTP client sends rather than a browser.
    //
    // With Access on and public token links off, a token that came through
    // the tunnel is not a way in on any hostname: only a login is. This is
    // the second lock — the first is that the token hostname's record and
    // route are gone — and loopback keeps its token.
    let gate = shared.identities.settings();
    let tokens_refused =
        gate.as_ref().is_some_and(|g| !g.public_links) && through_tunnel(shared, &request);
    let presented = match tokens_refused {
        true => None,
        false => access_for(shared, request.token())
            .or_else(|| access_for(shared, request.bearer()))
            .or_else(|| {
                access_for(
                    shared,
                    request.cookie(&cookie_name(shared.port)).unwrap_or(""),
                )
            }),
    };
    // A Cloudflare Access login, beside the tokens rather than instead of
    // them: whichever is worth more decides. Only the signed assertion is
    // read — see [`identity`] for why the email header beside it is not.
    // Never on the token hostname, which is outside the Access application:
    // what gets in there is a token and only a token, so nothing a login is
    // worth can be mixed into it.
    let on_link_host = gate
        .as_ref()
        .is_some_and(|g| g.is_link_host(request.host()));
    let (login, owner) = match on_link_host {
        true => (None, false),
        false => match access_login(shared, &request) {
            Some((level, email)) => (
                Some(level),
                gate.as_ref().is_some_and(|g| g.is_owner(&email)),
            ),
            None => (None, false),
        },
    };
    // Another cctop on the account, signing with this machine's own tunnel
    // token — see [`cctop_core::peer`]. Under the same lock as the tokens: it
    // is a credential that came through the tunnel, and with public links
    // off nothing but a login comes through. Full, because the hub relays
    // the actions of whoever it already let act; never `handed` and never
    // `admin`, so a sibling cannot carry a token away or change who logs in.
    let from_peer = !tokens_refused
        && cctop_core::peer::verify(
            &shared.peer_secret,
            request.header("authorization"),
            &request.method,
            &request.path,
            cctop_core::peer::now(),
        );
    let peer = from_peer.then_some(Access::Full);
    // Who may change who logs in: the full token, or the owner's own login.
    // A full invite is not enough — it may act on sessions, not hand out
    // access.
    let admin = presented == Some(Access::Full) || owner;
    let Some(access) = presented.max(login).max(peer) else {
        // ponytail: no backoff on a wrong token, and the denial is logged as
        // it arrives. A token here is 128 bits of entropy (`new_token`), so a
        // guess is not a way in at any rate a refusal could slow it down to —
        // the work is done by the comparison, which is already constant-time
        // for equal lengths — and the one thing a delay would buy, masking
        // which of two supplied tokens was wrong, an attacker who can already
        // read the length leak has no use for. A wrong token is refused the
        // same way with or without it: immediately, and once per request.
        cctop_core::elog::event(
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
    cctop_core::elog::event(
        "http",
        "request",
        serde_json::json!({
            "method": request.method,
            "path": request.path,
            "access": match access {
                Access::Full => "full",
                Access::ReadOnly => "readonly",
            },
            "via": match (presented >= login, from_peer && presented.max(login) < peer) {
                (_, true) => "peer",
                (true, false) => "token",
                (false, false) => "access",
            },
        }),
    );
    // What the page may be handed back: the token this request presented,
    // and only when that token is what let it in. A login that got in on
    // its identity is given no token at all — one would outlive the invite,
    // and work on the token hostname for whoever it was passed to.
    let handed = presented.filter(|p| *p == access);

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
    // A terminal's socket, for a page that reached cctop from elsewhere; see
    // [`relay_terminal`]. Before the app's files because it is not one.
    if let Some(rest) = path.strip_prefix("/rmux-ws/") {
        return relay_terminal(shared, stream, &request, rest, access);
    }
    // A sibling on the account, through this server — see [`forward`]. The
    // page never talks to another origin, so the content policy stays as it
    // is and a terminal frame keeps its same-origin URL.
    if let Some(rest) = path.strip_prefix("/api/peer/") {
        let (machine, tail) = rest.split_once('/').unwrap_or((rest, ""));
        let Some(peer) = cctop_core::peer::find(machine) else {
            return http::respond_error(
                stream,
                Some(&request),
                404,
                "no machine by that name on this Cloudflare account",
            );
        };
        return forward(shared, stream, &request, &peer, &format!("/{tail}"), access);
    }
    // A session route for a row a sibling holds goes to that sibling, the
    // same way: what it reads and does is about *its* disk and processes.
    if let Some(peer) = peer_of(shared, &path) {
        return forward(shared, stream, &request, &peer, &path, access);
    }
    // rmux's terminal app, for a session page to frame — see [`term`]. Static
    // and behind the same token as everything else, so only someone already
    // holding the link can load it.
    if let Some(file) = term::file(&path) {
        return http::respond_packed(
            stream,
            &request,
            file.content_type,
            file.body,
            file.cache,
            "",
            Some(http::TERM_POLICY),
        );
    }
    match path.as_str() {
        "/" => app_page(shared, stream, &request, handed),
        // Fixed for a build but not named by one, so revalidated rather than
        // kept: a `304` is as cheap as a cache hit and survives an upgrade.
        "/favicon.svg" => http::respond_packed(
            stream,
            &request,
            "image/svg+xml",
            &FAVICON_P,
            http::REVALIDATE,
            "",
            None,
        ),
        "/manifest.webmanifest" => http::respond_packed(
            stream,
            &request,
            "application/manifest+json",
            &MANIFEST_P,
            http::REVALIDATE,
            "",
            None,
        ),
        "/api/config" => config_route(shared, stream, &request, access, handed),
        "/api/sessions" => {
            // Tagged, because a page whose event stream is not getting through
            // — a quick tunnel, which does not carry SSE — polls this instead,
            // and an unchanged table should cost it a `304`. Still `no-store`:
            // the rows carry titles and prompts.
            let snapshot = current(shared);
            // A sibling gets this machine's own rows and no one else's: the
            // rest are either its own, coming back, or another machine's it
            // can read for itself.
            let local;
            let body = match from_peer {
                true => {
                    local = own_rows(&snapshot);
                    local.as_str()
                }
                false => snapshot.json.as_str(),
            };
            http::respond_tagged_unkept(stream, &request, http::JSON, body.as_bytes());
        }
        "/api/hosts" => {
            let snapshot = current(shared);
            // Siblings from the process-wide poller as well as the hosts this
            // server polled: inside the dashboard it is the dashboard's
            // poller that reads them, and only it knows which are down.
            let mut failed = snapshot.host_errors.clone();
            for (machine, why) in cctop_core::peer::offline() {
                if !failed.iter().any(|(h, _)| *h == machine) {
                    failed.push((machine, why));
                }
            }
            json(stream, &request, &failed);
        }
        "/api/quota" => {
            // Served from memory: the document is rendered by whoever polled
            // the endpoints last, on their schedule rather than the browser's.
            let body = shared
                .quota
                .lock()
                .map(|q| q.clone())
                .unwrap_or_else(|_| quota::EMPTY.to_string());
            http::respond_json(stream, &request, &body);
        }
        "/api/provider-status" => provider_status_route(shared, stream, &request),
        "/api/search" => api_search(shared, stream, &request),
        "/api/events" => events(shared, stream, &request),
        "/insight/optimize" => api_insight(shared, stream, &request, "optimize"),
        "/insight/compare" => api_insight(shared, stream, &request, "compare"),
        "/analytics" => app_page(shared, stream, &request, handed),
        // Every tab's terminal, tiled. The page itself is harmless to a
        // read-only link — opening a terminal is the action, and that route
        // checks the credential as every action does.
        "/workspace" => {
            cctop_core::telemetry::record(cctop_core::telemetry::Feature::Workspace);
            app_page(shared, stream, &request, handed)
        }
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
            let agents = actions::agents();
            // Which of them cannot go to an ssh host, and the sentence that
            // says so, for the launcher to grey out once a host is typed.
            let local_only: serde_json::Map<String, serde_json::Value> = agents
                .iter()
                .filter(|a| cctop_core::sandbox::reach(a) == cctop_core::sandbox::Reach::Local)
                .map(|a| (a.clone(), cctop_core::sandbox::local_only(a).into()))
                .collect();
            let body = serde_json::json!({
                "actions": shared.actions && access == Access::Full,
                "agents": agents,
                "local_only": local_only,
            });
            json(stream, &request, &body);
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
            // `host:path` is a folder on an ssh host, read as the terminal's
            // field reads it.
            let launched = match cwd.map(cctop_core::remote_launch::parse) {
                Some(cctop_core::remote_launch::Typed::Remote { host, path }) => {
                    actions::launch_remote(agent, host, path, || {
                        ssh::launch_problem(&shared.ssh, host, path)
                    })
                }
                _ => actions::launch_agent(agent, cwd),
            };
            match launched {
                Ok(done) => json(stream, &request, &done),
                Err((status, why)) => http::respond_error(stream, Some(&request), status, &why),
            }
        }
        // The web launcher's hosts, completion and folder check: ssh started
        // by a request, so the full token only — see [`ssh`].
        _ if path.starts_with("/api/ssh/") => {
            let Some(body) = may_act(shared, stream, &request, access) else {
                return;
            };
            ssh::route(
                &shared.ssh,
                stream,
                &request,
                &path["/api/ssh/".len()..],
                &body,
            );
        }
        _ if path.starts_with("/session/") => app_page(shared, stream, &request, handed),
        // A popped-out terminal: the same app, drawing only the terminal.
        _ if path.starts_with("/window/") => app_page(shared, stream, &request, handed),
        _ if path.starts_with("/api/report/") => {
            api_report(shared, stream, &request, &path["/api/report/".len()..]);
        }
        _ if path.starts_with("/api/chat/") && path.ends_with("/markdown") => {
            let id = &path["/api/chat/".len()..path.len() - "/markdown".len()];
            api_chat_markdown(shared, stream, &request, id);
        }
        _ if path.starts_with("/api/chat/") => {
            api_chat(shared, stream, &request, &path["/api/chat/".len()..]);
        }
        _ if path.starts_with("/api/access/") => {
            api_access(shared, stream, &request, &path["/api/access/".len()..]);
        }
        // Where this session can be handed: every (agent, account) pair but
        // the one it is on. Per session rather than folded into /api/agents,
        // because the pair that is left out is this session's — and empty for
        // a link that could not act on the answer, as /api/agents says
        // `actions: false` to the same link.
        _ if path.starts_with("/api/handoff/") => {
            let snapshot = current(shared);
            let id = &path["/api/handoff/".len()..];
            let Some(session) = find(&snapshot.sessions, id) else {
                return http::respond_error(stream, Some(&request), 404, NO_SUCH_SESSION);
            };
            let targets = match shared.actions && access == Access::Full && session.remote.is_none()
            {
                true => cctop_core::handoff::targets(session),
                false => Vec::new(),
            };
            let targets: Vec<serde_json::Value> = targets
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "agent": t.agent,
                        "account": t.account,
                        "label": t.label(),
                    })
                })
                .collect();
            json(stream, &request, &serde_json::json!({ "targets": targets }))
        }
        // The size rmux has fitted a tab's window to. The page's terminal asks
        // when you come back to it, compares it with its own grid, and if the
        // window is some other client's — the TUI's, typically — nudges its
        // own size so rmux hands the window over; rmux's `latest` does not
        // follow input on its own. Read-only, and only for cctop's tabs.
        _ if path.starts_with("/api/window/") => {
            let name = &path["/api/window/".len()..];
            if !actions::is_tab(&cctop_core::rmux::running(), name) {
                return http::respond_error(
                    stream,
                    Some(&request),
                    404,
                    "no open tab by that name",
                );
            }
            match cctop_core::rmux::window_size(name) {
                // The browsers' own sizes ride along, so the page can tell the
                // window is already a browser's without measuring its drawing.
                Some((cols, rows)) => {
                    let web: Vec<[u16; 2]> = cctop_core::rmux::clients(name)
                        .into_iter()
                        .filter(|(browser, _, _)| *browser)
                        .map(|(_, c, r)| [c, r])
                        .collect();
                    json(
                        stream,
                        &request,
                        &serde_json::json!({ "cols": cols, "rows": rows, "web": web }),
                    )
                }
                None => http::respond_error(stream, Some(&request), 404, "that tab has no window"),
            }
        }
        "/api/tabs" => {
            let snapshot = current(shared);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let list = tabs::build(
                cctop_core::rmux::running_in_tab_order(),
                &snapshot.sessions,
                now,
            );
            // Polled every few seconds by every open page and nearly always
            // the same answer, so it goes out tagged and comes back a `304`.
            // Tab names, titles and states — what the page shows in its bar,
            // nothing from a transcript — so it may sit in the browser cache.
            let body = serde_json::json!({ "tabs": list }).to_string();
            http::respond_revalidated(stream, &request, http::JSON, body.as_bytes());
        }
        _ if path.starts_with("/api/tab/") => {
            api_tab(shared, stream, &request, &path["/api/tab/".len()..], access);
        }
        _ if path.starts_with("/api/act/") => {
            api_act(shared, stream, &request, &path["/api/act/".len()..], access);
        }
        "/api/address" => address::route(shared, stream, &request, "", access),
        "/api/access" => invites::route(shared, stream, &request, access, admin),
        _ if path.starts_with("/api/address/") => {
            address::route(
                shared,
                stream,
                &request,
                &path["/api/address/".len()..],
                access,
            );
        }
        _ => http::respond_error(stream, Some(&request), 404, "no such page"),
    }
}

/// The table document with this machine's own rows only.
fn own_rows(snapshot: &Snapshot) -> String {
    let remote: std::collections::HashSet<&str> = snapshot
        .sessions
        .iter()
        .filter(|s| s.remote.is_some())
        .map(|s| s.session_id.as_str())
        .collect();
    let rows: Vec<&str> = snapshot
        .rows
        .iter()
        .filter(|(id, _)| !remote.contains(id.as_str()))
        .map(|(_, row)| row.as_str())
        .collect();
    format!("[{}]", rows.join(","))
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

/// Serve the app page, which is the same for every run of a build.
///
/// It used to carry this run's config — the token, whether it may act — stitched
/// in where `__CCTOP_CONFIG__` stands, which made it a different page per run
/// and per credential, compressed by this process on its first request. The
/// config is fetched from [`config_route`] instead, so the page can be
/// compressed when cctop is built, and the brotli encoder that compressed it
/// here is no longer in the binary. The page starts that fetch from its
/// `<head>`, before its script is parsed, so the round trip it adds runs
/// alongside work the browser had to do anyway.
///
/// Cached as `private, no-cache` with an ETag:
///
/// - **Not `no-store`.** That made every reload download the whole UI again —
///   300 KB over a tunnel to a phone — for a page that had not changed. With
///   an ETag a reload is a `304` and no body.
/// - **Not `max-age`.** The page is the one thing that knows which cctop it
///   was built by; kept without asking, it would go on running the old UI
///   against an upgraded server. `no-cache` asks every time, and the answer is
///   free when nothing moved.
/// - **`private`.** Every route here is behind the token, and a shared cache —
///   the tunnel's CDN, a proxy — that kept a copy would hand it to requests the
///   gate never saw.
///
/// The tag names the build, so it is the same for both credentials, which is
/// now right: the page carries neither.
fn app_page(shared: &Shared, stream: &mut TcpStream, request: &Request, handed: Option<Access>) {
    let credential = match handed {
        Some(Access::Full) => shared.token.as_str(),
        Some(Access::ReadOnly) => shared.readonly.as_str(),
        None => "",
    };

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
        if !(handed == Some(Access::ReadOnly) && held == Some(Access::Full)) {
            headers = format!(
                "Set-Cookie: {}={credential}; Path=/; HttpOnly; SameSite=Strict{}\r\n",
                cookie_name(shared.port),
                secure_attribute(shared, request)
            );
        }
    }
    http::respond_packed(
        stream,
        request,
        "text/html; charset=utf-8",
        &APP_PAGE,
        http::REVALIDATE,
        &headers,
        None,
    );
}

/// Carry a terminal's WebSocket from the page to rmux's loopback endpoint.
///
/// A page opened from another machine — the LAN, or `--tunnel`'s
/// trycloudflare host — cannot reach `ws://127.0.0.1:<port>`, which is where
/// rmux serves a share. It used to get a second tunnel for that, through
/// localhost.run, which dropped often enough to be most of the refused frames.
/// Instead the share's endpoint is rewritten to this route at the page's own
/// origin (see `rmux::relay_link`), and the socket rides whatever road the page
/// came in on, the quick tunnel included: it carries WebSocket upgrades.
///
/// Only to a port a share of cctop's named — anything else would make this a
/// way into every local service that trusts localhost — and only for a link
/// that may act, since the far end is a shell. The handshake is passed on
/// whole, Host rewritten, and then the bytes are pumped both ways until either
/// side closes. The connection holds one of [`MAX_CONNECTIONS`] while it lives.
fn relay_terminal(
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
    let Some((port, tail)) = rest.split_once('/') else {
        return http::respond_error(stream, Some(request), 404, "no such terminal");
    };
    let Some(port) = port
        .parse::<u16>()
        .ok()
        .filter(|p| cctop_core::rmux::relayable(*p))
    else {
        return http::respond_error(stream, Some(request), 404, "no such terminal");
    };
    relay_socket(stream, request, port, &format!("/{tail}"));
}

/// Pass a WebSocket handshake to rmux's listener on loopback `port` at
/// `path`, and pump bytes both ways until either side closes.
///
/// The handshake goes on whole, `Host` rewritten — rmux checks `Origin`,
/// which is the browser's and left alone. Shared by the page's relay and the
/// share front, which differ in who may ask, never in how the bytes move.
fn relay_socket(stream: &mut TcpStream, request: &Request, port: u16, path: &str) {
    use std::io::{Read, Write};
    let Ok(mut upstream) = TcpStream::connect(("127.0.0.1", port)) else {
        return http::respond_error(
            stream,
            Some(request),
            502,
            "the terminal's share has closed",
        );
    };
    let query = match request.raw_query() {
        "" => String::new(),
        q => format!("?{q}"),
    };
    let mut head = format!("GET {path}{query} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
    for (name, value) in request.headers() {
        if !name.eq_ignore_ascii_case("host") {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    head.push_str("\r\n");
    if upstream.write_all(head.as_bytes()).is_err() {
        return http::respond_error(
            stream,
            Some(request),
            502,
            "the terminal's share has closed",
        );
    }
    // A terminal sits quiet for minutes at a time; the request deadlines that
    // guard every other route would cut it off mid-session.
    let _ = stream.set_read_timeout(None);
    let _ = stream.set_write_timeout(None);
    let (Ok(mut down_read), Ok(mut up_write)) = (stream.try_clone(), upstream.try_clone()) else {
        return;
    };
    let pump = std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n) = upstream.read(&mut buf) {
            if n == 0 || down_read.write_all(&buf[..n]).is_err() {
                break;
            }
        }
        let _ = down_read.shutdown(std::net::Shutdown::Both);
    });
    let mut buf = [0u8; 16 * 1024];
    while let Ok(n) = stream.read(&mut buf) {
        if n == 0 || up_write.write_all(&buf[..n]).is_err() {
            break;
        }
    }
    let _ = up_write.shutdown(std::net::Shutdown::Both);
    let _ = pump.join();
}

/// Answer `/api/config`: what the app page is told about this run.
///
/// Which credential it carries — and whether it may act — is decided by the one
/// the request arrived with, not by the run: the read-only link opens the same
/// page, which asks with the read-only token and is told only that one, so a
/// page it hands out can neither act nor leak the token that could. The page
/// asks with the `?t=` it was opened with, ahead of the cookie, so a read-only
/// link opened in a browser holding the full cookie gets the read-only answer.
///
/// `no-store` like every other route that carries the token.
///
/// A page let in by an Access login is told no token (`handed` is `None`):
/// every request it makes comes through the edge with the login again.
fn config_route(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    access: Access,
    handed: Option<Access>,
) {
    let credential = match handed {
        Some(Access::Full) => shared.token.as_str(),
        Some(Access::ReadOnly) => shared.readonly.as_str(),
        None => "",
    };
    let actions = access == Access::Full && shared.actions;
    http::respond_json(stream, request, &app_config(credential, actions));
}

/// The config [`config_route`] answers with, as JSON.
fn app_config(credential: &str, actions: bool) -> String {
    let home = dirs::home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    serde_json::json!({
        "token": credential,
        "actions": actions,
        "home": home,
        "version": cctop_core::update::current_version(),
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::tests::put;

    pub(crate) fn session(id: &str) -> Session {
        Session::new(cctop_core::pricing::Provider::Claude, id.into())
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
    fn the_app_page_is_built_and_fetches_its_config() {
        // The built app is committed (web/ is its source); a checkout where it
        // is missing or stale-shaped would serve a page that cannot start.
        let page = std::str::from_utf8(APP_PAGE.decoded()).expect("utf-8");
        assert!(page.contains("<div id=\"root\"></div>"));
        // The page is the same for every run, so nothing is substituted into
        // it: the config comes from the route, asked for from the head.
        assert!(!page.contains("__CCTOP_CONFIG__"));
        let head = &page[..page.find("</head>").expect("a head")];
        assert!(
            head.contains("/api/config"),
            "the head does not ask for the config"
        );
        // Everything inlined: a page that loads a file of its own breaks under
        // the content policy, which allows no URL at all.
        assert!(!page.contains(" src=\"/assets/"));
        assert!(!page.contains("<link rel=\"stylesheet\""));
    }

    /// Each link is told its own credential and nothing more, whichever way it
    /// arrived — and a read-only link opened where the full cookie is held is
    /// told the read-only one, since the page asks with the `?t=` it opened
    /// with.
    #[test]
    fn each_link_is_told_only_its_own_credential() {
        let guarded = shared("full", "view");
        let config = |target: &str, headers: &str| {
            let (head, body) = exchange(&guarded, target, headers);
            assert!(head.starts_with("HTTP/1.1 200 "), "{target}: {head}");
            assert_eq!(header(&head, "Cache-Control"), Some("no-store"), "{target}");
            let body = String::from_utf8(body).unwrap();
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            (v, body)
        };
        let (full, _) = config("/api/config?t=full", "");
        assert_eq!(full["token"], "full");
        assert_eq!(full["actions"], true);
        assert_eq!(full["version"], cctop_core::update::current_version());
        for (target, headers) in [
            ("/api/config?t=view", ""),
            ("/api/config", "Authorization: Bearer view\r\n"),
            ("/api/config", "Cookie: cctop_access_7777=view\r\n"),
            ("/api/config?t=view", "Cookie: cctop_access_7777=full\r\n"),
        ] {
            let (v, body) = config(target, headers);
            assert_eq!(v["token"], "view", "{target} {headers}");
            assert_eq!(v["actions"], false, "{target} {headers}");
            assert!(!body.contains("full"), "{target} {headers}: {body}");
        }
        let (head, _) = exchange(&guarded, "/api/config", "");
        assert!(head.starts_with("HTTP/1.1 403 "), "{head}");
    }

    /// Every refusal of the main page — 403 at the gate, 404 past it, 405 for
    /// a method no route takes — repeats the status and a fixed sentence, and
    /// never the credential that asked. A share hostname's 404 is held to the
    /// same rule (`share_host.rs`), and this is its counterpart here: the token
    /// arrives in the query line, so a message that quoted the request would
    /// print it.
    #[test]
    fn a_refusal_never_echoes_the_token_it_refused() {
        // Hex, and long enough that no sentence cctop writes could contain one
        // by accident: a leak cannot hide behind a word the prose also uses.
        let full = "0123456789abcdef0123456789abcdef";
        let wrong = "fedcba9876543210fedcba9876543210";
        let guarded = shared(full, "view");
        let asked = |method: &str, target: &str, headers: &str| -> String {
            let raw = response_of(&guarded, method, target, headers);
            for leaked in [full, wrong] {
                assert!(
                    !raw.contains(leaked),
                    "{method} {target} leaked a credential"
                );
            }
            raw
        };
        // 403: at the gate, with a credential that is neither of this run's —
        // in the query, in the header, and in both at once. A *successful*
        // page's `Set-Cookie` is the one answer that carries a credential, and
        // a refusal is not that answer.
        for headers in [String::new(), format!("Authorization: Bearer {wrong}\r\n")] {
            let raw = asked("GET", &format!("/?t={wrong}"), &headers);
            assert!(
                raw.lines().next().unwrap_or_default().contains(" 403 "),
                "{raw}"
            );
        }
        // 404: past the gate — the right token, a page that is not there.
        let raw = asked("GET", &format!("/no-such-page?t={full}"), "");
        assert!(
            raw.lines().next().unwrap_or_default().contains(" 404 "),
            "{raw}"
        );
        // 405: a method no route of cctop's takes, credential in the query.
        // A body's worth of length, so the refusal is the route's and not the
        // parser's `411`.
        let raw = asked(
            "POST",
            &format!("/metrics?t={full}"),
            "Content-Length: 2\r\n\r\n{}",
        );
        assert!(
            raw.lines().next().unwrap_or_default().contains(" 405 "),
            "{raw}"
        );
        // And a read-only link refused an action: 403, with the credential
        // that would have been enough if this were not an act.
        let raw = asked("GET", "/api/act/x/prompt?t=view", "");
        assert!(
            raw.lines().next().unwrap_or_default().contains(" 403 "),
            "{raw}"
        );
    }

    #[test]
    fn a_token_is_safe_to_paste_into_a_url_unescaped() {
        // The links cctop prints, and the `Set-Cookie` value `app_page` writes,
        // carry the token by interpolation rather than by escaping. That is
        // only correct while the generator stays hex.
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

    pub(crate) fn shared(token: &str, readonly: &str) -> Shared {
        Shared {
            token: token.to_string(),
            readonly: readonly.to_string(),
            actions: true,
            port: 7777,
            plan: Plan::Retail,
            latest: Mutex::new(Arc::new(Snapshot {
                version: 0,
                json: "[]".to_string(),
                rows: Vec::new(),
                sessions: Vec::new(),
                host_errors: Vec::new(),
            })),
            updated: Condvar::new(),
            store: cctop_core::cache::Store::new(),
            quota: Mutex::new(quota::EMPTY.to_string()),
            provider_status: Mutex::default(),
            topics: Mutex::new(search::Topics::default()),
            notify: None,
            hosts: HashMap::new(),
            ssh: ssh::Reach::nowhere(),
            is_share_host: |_| false,
            is_tunnel_host: |_| false,
            addresses: Arc::new(address::Nowhere),
            identities: Arc::new(identity::Nowhere),
            peer_secret: String::new(),
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

    /// A serve with both tokens and Access set up for `owner@example.test`
    /// and `@example.test` read-only, logging in against the tests' own key.
    fn behind_access() -> Shared {
        Shared {
            identities: Arc::new(identity::Fixed::new(
                cctop_core::cloudflare::access::fake::settings(),
            )),
            ..shared("full", "view")
        }
    }

    /// `/api/config` as a request with `headers` sees it: the status line
    /// and, on a 200, the JSON.
    fn config_with(shared: &Shared, target: &str, headers: &str) -> (String, serde_json::Value) {
        let raw = response_of(shared, "GET", target, headers);
        let status = raw.lines().next().unwrap_or_default().to_string();
        let body = raw.split_once("\r\n\r\n").map_or("", |(_, b)| b);
        (status, serde_json::from_str(body).unwrap_or_default())
    }

    #[test]
    fn a_forged_access_header_opens_nothing() {
        use cctop_core::cloudflare::access::fake;
        let shared = behind_access();
        let owner = fake::claims("owner@example.test");
        let mut expired = owner.clone();
        expired["exp"] = serde_json::json!(cctop_core::cloudflare::access::now() - 3600);
        let mut other_app = owner.clone();
        other_app["aud"] = serde_json::json!(["some-other-app"]);
        // The email header is what the edge adds beside the JWT, and what
        // anything on loopback can write: alone, or beside an assertion that
        // does not check out, it is worth nothing.
        let email = "Cf-Access-Authenticated-User-Email: owner@example.test\r\n";
        let forgeries = [
            email.to_string(),
            format!(
                "{email}Cf-Access-Jwt-Assertion: {}\r\n",
                fake::forged(&owner)
            ),
            format!(
                "{email}Cf-Access-Jwt-Assertion: {}\r\n",
                fake::unsigned(&owner)
            ),
            format!(
                "{email}Cf-Access-Jwt-Assertion: {}\r\n",
                fake::token(&expired)
            ),
            format!(
                "{email}Cf-Access-Jwt-Assertion: {}\r\n",
                fake::token(&other_app)
            ),
            format!("{email}Cf-Access-Jwt-Assertion: not.a.jwt\r\n"),
            format!(
                "{email}Cookie: CF_Authorization={}\r\n",
                fake::forged(&owner)
            ),
        ];
        for headers in &forgeries {
            for target in ["/", "/api/config", "/api/sessions"] {
                assert_eq!(
                    status_of(&shared, "GET", target, headers),
                    "HTTP/1.1 403 Forbidden",
                    "{target} with {headers}"
                );
            }
        }
        // A genuine login by someone nobody invited is refused the same way.
        let stranger = format!(
            "Cf-Access-Jwt-Assertion: {}\r\n",
            fake::token(&fake::claims("someone@elsewhere.test"))
        );
        assert_eq!(
            status_of(&shared, "GET", "/api/config", &stranger),
            "HTTP/1.1 403 Forbidden"
        );
    }

    #[test]
    fn an_access_login_gets_what_its_invite_says_and_no_token() {
        use cctop_core::cloudflare::access::fake;
        let shared = behind_access();
        let login = |email: &str| {
            format!(
                "Cf-Access-Jwt-Assertion: {}\r\n",
                fake::token(&fake::claims(email))
            )
        };

        let (status, owner) = config_with(&shared, "/api/config", &login("owner@example.test"));
        assert_eq!(status, "HTTP/1.1 200 OK");
        assert_eq!(owner["actions"], true);
        // No token handed to a page that got in on its identity: it would
        // outlive the invite.
        assert_eq!(owner["token"], "");

        let colleague = login("colleague@example.test");
        let (status, read) = config_with(&shared, "/api/config", &colleague);
        assert_eq!(status, "HTTP/1.1 200 OK");
        assert_eq!(read["actions"], false);
        assert_eq!(read["token"], "");
        // Exactly what the read-only token gets: the actions are refused.
        let act = response_of(
            &shared,
            "POST",
            "/api/act/kill/anything",
            &format!("{colleague}Content-Type: application/json\r\nContent-Length: 2\r\n\r\n{{}}"),
        );
        assert!(act.starts_with("HTTP/1.1 403"), "{act}");
        assert!(act.contains("this link is read-only"), "{act}");

        // The page gets no cookie either: nothing to replay without the login.
        let (head, _) = exchange(&shared, "/", &login("owner@example.test"));
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert_eq!(header(&head, "Set-Cookie"), None, "{head}");
    }

    /// A serve behind Access with `settings`, on the account's tunnel at
    /// `cctop.example.test` and `cctop-link.example.test`.
    fn behind(settings: cctop_core::cloudflare::access::Settings) -> Shared {
        Shared {
            identities: Arc::new(identity::Fixed::new(settings)),
            is_tunnel_host: |host| host.ends_with(".example.test"),
            ..shared("full", "view")
        }
    }

    fn jwt_of(email: &str) -> String {
        use cctop_core::cloudflare::access::fake;
        format!(
            "Cf-Access-Jwt-Assertion: {}\r\n",
            fake::token(&fake::claims(email))
        )
    }

    #[test]
    fn with_public_links_off_a_token_alone_gets_nothing_through_the_tunnel() {
        let off = behind(cctop_core::cloudflare::access::Settings {
            public_links: false,
            ..cctop_core::cloudflare::access::fake::settings()
        });
        for token in ["full", "view"] {
            for how in [
                format!("Host: cctop.example.test\r\n{}", bearer_line(token)),
                format!("Host: cctop-link.example.test\r\n{}", bearer_line(token)),
                format!("Host: cctop.example.test\r\nCookie: cctop_access_7777={token}\r\n"),
                // Through a quick tunnel, or any other, the edge's headers say so.
                format!(
                    "Host: 127.0.0.1:7777\r\nCf-Ray: 8a1b2c3d-AKL\r\n{}",
                    bearer_line(token)
                ),
            ] {
                for target in [
                    "/",
                    "/api/config",
                    "/api/sessions",
                    &format!("/api/config?t={token}"),
                ] {
                    assert_eq!(
                        status_of(&off, "GET", target, &how),
                        "HTTP/1.1 403 Forbidden",
                        "{target} with {how}"
                    );
                }
            }
        }
        // Loopback keeps its token.
        let local = format!("Host: 127.0.0.1:7777\r\n{}", bearer_line("full"));
        assert_eq!(
            status_of(&off, "GET", "/api/config", &local),
            "HTTP/1.1 200 OK"
        );
        // And a login still gets in from outside.
        let owner = format!(
            "Host: cctop.example.test\r\n{}",
            jwt_of("owner@example.test")
        );
        assert_eq!(
            status_of(&off, "GET", "/api/config", &owner),
            "HTTP/1.1 200 OK"
        );

        // On, the same token through the tunnel works as it always did.
        let on = behind(cctop_core::cloudflare::access::fake::settings());
        let through = format!("Host: cctop.example.test\r\n{}", bearer_line("full"));
        assert_eq!(
            status_of(&on, "GET", "/api/config", &through),
            "HTTP/1.1 200 OK"
        );
    }

    #[test]
    fn a_login_is_worth_nothing_on_the_token_hostname() {
        let shared = behind(cctop_core::cloudflare::access::Settings {
            link_hostname: Some("cctop-link.example.test".into()),
            link_record_id: Some("rec-link".into()),
            ..cctop_core::cloudflare::access::fake::settings()
        });
        let link = "Host: cctop-link.example.test\r\n";
        let owner = jwt_of("owner@example.test");
        assert_eq!(
            status_of(&shared, "GET", "/api/config", &format!("{link}{owner}")),
            "HTTP/1.1 403 Forbidden"
        );
        // A read-only token there stays read-only, owner's login or not.
        let (status, read) = config_with(&shared, "/api/config?t=view", &format!("{link}{owner}"));
        assert_eq!(status, "HTTP/1.1 200 OK");
        assert_eq!(read["actions"], false);
        assert_eq!(read["token"], "view");
        // The token link itself works as today.
        let (status, full) = config_with(&shared, "/api/config?t=full", link);
        assert_eq!(status, "HTTP/1.1 200 OK");
        assert_eq!(full["token"], "full");
        // And the same login on the dashboard's hostname is the owner's.
        let (_, there) = config_with(
            &shared,
            "/api/config",
            &format!("Host: cctop.example.test\r\n{owner}"),
        );
        assert_eq!(there["actions"], true);
    }

    #[test]
    fn only_the_full_token_or_the_owner_may_see_or_change_the_invites() {
        let (fake, seen) = address::tests::fake();
        let mut settings = cctop_core::cloudflare::access::fake::settings();
        settings
            .invites
            .push(cctop_core::cloudflare::access::Invite {
                who: "boss@example.test".into(),
                level: cctop_core::cloudflare::access::Level::Full,
            });
        let shared = Shared {
            addresses: Arc::new(fake),
            ..behind(settings)
        };
        let invite = r#"{"op":"invite","who":"guest@elsewhere.test","level":"full"}"#;
        let post_as = |headers: &str, body: &str| {
            response_of(
                &shared,
                "POST",
                "/api/access",
                &format!(
                    "Host: cctop.example.test\r\n{headers}Content-Type: application/json\r\n\
                     Content-Length: {}\r\n\r\n{body}",
                    body.len()
                ),
            )
        };
        // Refused, the list and the edits alike: the read-only token, a
        // read-only login, and a full invite who is not the owner.
        for who in [
            bearer_line("view"),
            jwt_of("colleague@example.test"),
            jwt_of("boss@example.test"),
        ] {
            let read = response_of(
                &shared,
                "GET",
                "/api/access",
                &format!("Host: cctop.example.test\r\n{who}"),
            );
            assert!(read.starts_with("HTTP/1.1 403"), "{who}: {read}");
            let edit = post_as(&who, invite);
            assert!(edit.starts_with("HTTP/1.1 403"), "{who}: {edit}");
        }
        assert!(
            seen.lock().unwrap().is_empty(),
            "nothing reached Cloudflare"
        );

        // The full token turns Access on; the owner's login invites.
        let on = post_as(
            &bearer_line("full"),
            r#"{"op":"on","owner":"owner@example.test"}"#,
        );
        assert!(on.starts_with("HTTP/1.1 200"), "{on}");
        assert!(
            on.contains("\"link_host\":\"cctop-link.example.test\""),
            "{on}"
        );
        let added = post_as(&jwt_of("owner@example.test"), invite);
        assert!(added.starts_with("HTTP/1.1 200"), "{added}");
        assert!(added.contains("guest@elsewhere.test"), "{added}");
        let policy = seen
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(m, p, _)| m == "PUT" && p.contains("/access/policies/"))
            .map(|(_, _, body)| body.clone())
            .expect("the edge's policy was updated");
        assert!(policy.contains("guest@elsewhere.test"), "{policy}");

        // The owner turns token links off; the record goes.
        let off = post_as(
            &jwt_of("owner@example.test"),
            r#"{"op":"links","on":false}"#,
        );
        assert!(off.contains("\"public_links\":false"), "{off}");
        assert!(off.contains("\"link_host\":null"), "{off}");
        // Nonsense is refused by its own sentence.
        let bad = post_as(&bearer_line("full"), r#"{"op":"promote"}"#);
        assert!(bad.starts_with("HTTP/1.1 400"), "{bad}");
    }

    #[test]
    fn a_token_and_a_login_together_get_the_better_of_the_two() {
        use cctop_core::cloudflare::access::fake;
        let access = behind_access();
        let colleague = format!(
            "Cf-Access-Jwt-Assertion: {}\r\n",
            fake::token(&fake::claims("colleague@example.test"))
        );
        // The full token beside a read-only login: the token decides, and
        // the page is handed it back as before.
        let (_, both) = config_with(&access, "/api/config?t=full", &colleague);
        assert_eq!(both["actions"], true);
        assert_eq!(both["token"], "full");
        // The read-only token beside the owner's login: the login decides,
        // and no token is handed back.
        let owner = format!(
            "Cf-Access-Jwt-Assertion: {}\r\n",
            fake::token(&fake::claims("owner@example.test"))
        );
        let (_, both) = config_with(&access, "/api/config?t=view", &owner);
        assert_eq!(both["actions"], true);
        assert_eq!(both["token"], "");
        // And with no Access set up at all, a genuine-looking JWT is ignored
        // and the tokens work exactly as they did.
        let plain = shared("full", "view");
        assert_eq!(
            status_of(&plain, "GET", "/api/config", &owner),
            "HTTP/1.1 403 Forbidden"
        );
        assert_eq!(
            status_of(&plain, "GET", "/api/config?t=view", &owner),
            "HTTP/1.1 200 OK"
        );
    }

    /// A whole request through the router — gate, scope check and route — and
    /// the status line it was answered with.
    pub(crate) fn status_of(shared: &Shared, method: &str, target: &str, headers: &str) -> String {
        let raw = response_of(shared, method, target, headers);
        raw.lines().next().unwrap_or_default().to_string()
    }

    /// A whole request through the router, and the whole raw response.
    pub(crate) fn response_of(
        shared: &Shared,
        method: &str,
        target: &str,
        headers: &str,
    ) -> String {
        use std::io::Read;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        client
            .write_all(format!("{method} {target} HTTP/1.1\r\n{headers}\r\n").as_bytes())
            .unwrap();
        serve_connection(shared, &mut server);
        drop(server);
        let mut raw = String::new();
        client.read_to_string(&mut raw).unwrap();
        raw
    }

    /// The export route end to end: behind the same token as its neighbours,
    /// served as markdown, the whole conversation, and tool results only when
    /// the variant asks for them.
    #[test]
    fn the_markdown_route_serves_the_conversation_behind_the_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sess-md.jsonl");
        let lines = [
            r###"{"type":"user","timestamp":"2026-10-07T10:00:00Z","message":{"content":"## run the tests"}}"###,
            r#"{"type":"assistant","timestamp":"2026-10-07T10:00:05Z","message":{"content":[{"type":"text","text":"Running them."},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]}}"#,
            r#"{"type":"user","timestamp":"2026-10-07T10:00:09Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"test result: ok"}]}}"#,
        ];
        std::fs::write(&path, lines.join("\n")).unwrap();
        let mut s = Session::new(cctop_core::pricing::Provider::Claude, "sess-md".into());
        s.data_file = Some(path);

        let guarded = shared("full", "view");
        *guarded.latest.lock().unwrap() = Arc::new(Snapshot {
            version: 1,
            json: "[]".to_string(),
            rows: Vec::new(),
            sessions: vec![s],
            host_errors: Vec::new(),
        });
        let bearer = "Authorization: Bearer view\r\n";

        let plain = response_of(&guarded, "GET", "/api/chat/sess-md/markdown", bearer);
        assert!(plain.starts_with("HTTP/1.1 200 "), "{plain}");
        assert!(plain.contains("text/markdown"), "{plain}");
        assert!(plain.contains("> ## run the tests"), "{plain}");
        assert!(plain.contains("- **Bash** `cargo test`"), "{plain}");
        assert!(!plain.contains("test result: ok"), "{plain}");

        let tools = response_of(
            &guarded,
            "GET",
            "/api/chat/sess-md/markdown?variant=tools",
            bearer,
        );
        assert!(tools.contains("test result: ok"), "{tools}");

        let missing = status_of(&guarded, "GET", "/api/chat/nope/markdown", bearer);
        assert!(missing.contains(" 404 "), "{missing}");
        let anonymous = status_of(&guarded, "GET", "/api/chat/sess-md/markdown", "");
        assert!(anonymous.contains(" 403 "), "{anonymous}");
    }

    /// A subagent's own conversation, by the id the session lists, behind the
    /// same token; anything the session does not list is a 404, never a path.
    #[test]
    fn the_agent_route_serves_one_listed_subagent() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("sess-ag.jsonl");
        std::fs::write(
            &main,
            r#"{"type":"user","timestamp":"2026-10-08T10:00:00Z","message":{"content":"dummy ask"}}"#,
        )
        .unwrap();
        let sub = dir.path().join("sess-ag").join("subagents");
        std::fs::create_dir_all(&sub).unwrap();
        let agent_lines = [
            r#"{"type":"user","isSidechain":true,"timestamp":"2026-10-08T10:00:01Z","message":{"content":"dummy brief"}}"#,
            r#"{"type":"assistant","isSidechain":true,"timestamp":"2026-10-08T10:00:02Z","message":{"content":[{"type":"text","text":"dummy agent reply"}]}}"#,
        ];
        std::fs::write(sub.join("agent-x1.jsonl"), agent_lines.join("\n")).unwrap();
        let mut s = Session::new(cctop_core::pricing::Provider::Claude, "sess-ag".into());
        s.data_file = Some(main);
        s.subagents = vec![cctop_core::session::Subagent {
            agent_id: "agent-x1".into(),
            agent_type: "Explore".into(),
            description: "dummy".into(),
            model: "?".into(),
            started_at: None,
            last_active: None,
            duration_ms: 0,
            status: cctop_core::session::SubagentStatus::Done,
            cost: 0.0,
            tool_count: 0,
            turns: 0,
            tokens: 0,
            last_text: None,
            tool_use_id: Some("toolu_x".into()),
            context: None,
            ghost: false,
        }];

        let guarded = shared("full", "view");
        *guarded.latest.lock().unwrap() = Arc::new(Snapshot {
            version: 1,
            json: "[]".to_string(),
            rows: Vec::new(),
            sessions: vec![s],
            host_errors: Vec::new(),
        });
        let bearer = "Authorization: Bearer view\r\n";

        let body = response_of(&guarded, "GET", "/api/chat/sess-ag?agent=agent-x1", bearer);
        assert!(body.starts_with("HTTP/1.1 200 "), "{body}");
        assert!(body.contains("dummy agent reply"), "{body}");
        assert!(!body.contains("dummy ask"), "{body}");

        // Narrowed like the main conversation: after the last turn, nothing.
        let after = response_of(
            &guarded,
            "GET",
            "/api/chat/sess-ag?agent=agent-x1&after=2",
            bearer,
        );
        assert!(!after.contains("dummy agent reply"), "{after}");

        for refused in [
            "agent-nope",
            "..%2Fsess-ag",
            "../agent-x1",
            "subagents/agent-x1",
        ] {
            let status = status_of(
                &guarded,
                "GET",
                &format!("/api/chat/sess-ag?agent={refused}"),
                bearer,
            );
            assert!(status.contains(" 404 "), "{refused}: {status}");
        }
        let anonymous = status_of(&guarded, "GET", "/api/chat/sess-ag?agent=agent-x1", "");
        assert!(anonymous.contains(" 403 "), "{anonymous}");
    }

    #[test]
    fn a_bearer_header_is_the_same_check_as_the_query() {
        let guarded = shared("full", "view");
        let bearer = |token: &str| format!("Authorization: Bearer {token}\r\n");
        assert!(status_of(&guarded, "GET", "/api/hosts", &bearer("full")).contains(" 200 "));
        assert!(status_of(&guarded, "GET", "/api/hosts", &bearer("view")).contains(" 200 "));
        assert!(status_of(&guarded, "GET", "/api/hosts", &bearer("wrong")).contains(" 403 "));
        assert!(status_of(&guarded, "GET", "/api/hosts", "").contains(" 403 "));
        // Same scopes whichever way the token arrives: read-only cannot act.
        let act = status_of(
            &guarded,
            "POST",
            "/api/launch",
            &format!(
                "{}Content-Type: application/json\r\nContent-Length: 2\r\n\r\n{{}}",
                bearer("view")
            ),
        );
        assert!(act.contains(" 403 "), "{act}");
    }

    /// A JSON POST to `target` carrying `token`, and the raw response.
    fn post_json(shared: &Shared, target: &str, token: &str, body: &str) -> String {
        response_of(
            shared,
            "POST",
            target,
            &format!(
                "Authorization: Bearer {token}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        )
    }

    /// The web launcher's ssh routes start ssh connections and name internal
    /// hosts, so they are the full token's alone: a read-only link and a
    /// `--no-actions` serve are refused before anything is connected to, and
    /// only a JSON POST is taken. The stand-in host counts its connects, which
    /// is how "refused before ssh" is proved rather than assumed.
    #[test]
    fn the_ssh_routes_answer_the_full_token_only() {
        for route in ["/api/ssh/hosts", "/api/ssh/complete", "/api/ssh/check"] {
            let body = r#"{"host":"devbox","path":"~"}"#;
            let (_home, reach, connects) = ssh::local_host(true);
            let guarded = Shared {
                ssh: reach,
                ..shared("full", "view")
            };
            let full = post_json(&guarded, route, "full", body);
            assert!(full.starts_with("HTTP/1.1 200 "), "{route}: {full}");
            let view = post_json(&guarded, route, "view", body);
            assert!(view.starts_with("HTTP/1.1 403 "), "{route}: {view}");
            assert!(!view.contains("devbox"), "{route}: {view}");
            let get = status_of(&guarded, "GET", route, &bearer_line("full"));
            assert!(get.contains(" 405 "), "{route}: {get}");
            let before = *connects.lock().expect("count");

            let (_home, reach, quiet) = ssh::local_host(true);
            let no_actions = Shared {
                ssh: reach,
                actions: false,
                ..shared("full", "view")
            };
            let refused = post_json(&no_actions, route, "full", body);
            assert!(refused.starts_with("HTTP/1.1 403 "), "{route}: {refused}");
            assert_eq!(*quiet.lock().expect("count"), 0, "{route}");
            // The read-only and GET requests above connected to nothing more.
            assert!(before <= 1, "{route}: {before}");
        }
        // The read-only token cannot launch on a host either.
        let guarded = shared("full", "view");
        let launch = post_json(
            &guarded,
            "/api/launch",
            "view",
            r#"{"agent":"claude","cwd":"devbox:~"}"#,
        );
        assert!(launch.starts_with("HTTP/1.1 403 "), "{launch}");
    }

    fn bearer_line(token: &str) -> String {
        format!("Authorization: Bearer {token}\r\n")
    }

    /// Completion and the folder check, against a host that is a local `sh`
    /// in a temporary home.
    #[test]
    fn a_remote_folder_completes_and_is_checked_before_launch() {
        let (home, reach, _) = ssh::local_host(true);
        for dir in ["api", "apt", "docs"] {
            std::fs::create_dir(home.path().join(dir)).expect("mkdir");
        }
        let guarded = Shared {
            ssh: reach,
            ..shared("full", "view")
        };
        let body_of = |raw: String| -> serde_json::Value {
            let (_, body) = raw.split_once("\r\n\r\n").expect("a body");
            serde_json::from_str(body).expect("json")
        };
        let hosts = body_of(post_json(&guarded, "/api/ssh/hosts", "full", "{}"));
        assert_eq!(hosts["hosts"][0]["name"], "devbox");
        assert_eq!(hosts["hosts"][0]["aliases"][0], "dev");

        let done = body_of(post_json(
            &guarded,
            "/api/ssh/complete",
            "full",
            r#"{"host":"devbox","path":"~/ap"}"#,
        ));
        assert_eq!(done["ready"], true);
        let paths: Vec<&str> = done["hits"]
            .as_array()
            .expect("hits")
            .iter()
            .map(|h| h["path"].as_str().expect("path"))
            .collect();
        assert_eq!(paths, ["~/api", "~/apt"]);

        let fine = body_of(post_json(
            &guarded,
            "/api/ssh/check",
            "full",
            r#"{"host":"devbox","path":"~/docs"}"#,
        ));
        assert_eq!(fine["problem"], serde_json::Value::Null);
        let gone = body_of(post_json(
            &guarded,
            "/api/ssh/check",
            "full",
            r#"{"host":"devbox","path":"~/gone"}"#,
        ));
        assert_eq!(gone["problem"], "does not exist on the host");

        // An option dressed as a host never reaches ssh.
        let option = post_json(
            &guarded,
            "/api/ssh/complete",
            "full",
            r#"{"host":"-oProxyCommand=touch /tmp/x","path":"~"}"#,
        );
        assert!(option.starts_with("HTTP/1.1 400 "), "{option}");
        // A launch is judged on its agent before the host is asked anything:
        // a request naming nothing cctop would run connects nowhere.
        let (_home, reach, connects) = ssh::local_host(true);
        let fresh = Shared {
            ssh: reach,
            ..shared("full", "view")
        };
        let launch = post_json(
            &fresh,
            "/api/launch",
            "full",
            r#"{"agent":"sh -c id","cwd":"devbox:~"}"#,
        );
        assert!(launch.starts_with("HTTP/1.1 400 "), "{launch}");
        assert_eq!(*connects.lock().expect("count"), 0);
    }

    /// A host that would ask for a password is an answer, not an error: the
    /// page shows it offline with why, and the launch goes ahead to ask there.
    #[test]
    fn a_host_that_would_prompt_is_reported_offline() {
        let (_home, reach, _) = ssh::local_host(false);
        let guarded = Shared {
            ssh: reach,
            ..shared("full", "view")
        };
        let raw = post_json(
            &guarded,
            "/api/ssh/complete",
            "full",
            r#"{"host":"devbox","path":"~"}"#,
        );
        assert!(raw.starts_with("HTTP/1.1 200 "), "{raw}");
        assert!(raw.contains(r#""ready":false"#), "{raw}");
        assert!(raw.contains("needs a password or key prompt"), "{raw}");
        assert!(raw.contains(r#""prompt":true"#), "{raw}");
    }

    /// The page's second lock: a request whose `Host` is a share hostname is
    /// refused before the token is looked at, `/metrics` included — so if a
    /// share hostname were ever routed to the page instead of to the share
    /// front, it still would not open the dashboard.
    #[test]
    fn a_share_hostname_never_reaches_the_dashboard_with_any_token() {
        let mut guarded = shared("full", "view");
        guarded.is_share_host = |host| host.starts_with("cctop-share.example.test");
        // Not `/`: the app page is built on first use, which a debug build
        // takes a minute over, and the gate in front of it is the same one.
        for target in ["/api/sessions?t=full", "/api/config?t=full", "/metrics"] {
            let page = status_of(&guarded, "GET", target, "Host: cctop.example.test\r\n");
            assert!(page.contains(" 200 "), "{target}: {page}");
            for host in ["cctop-share.example.test", "cctop-share.example.test:443"] {
                let raw = response_of(&guarded, "GET", target, &format!("Host: {host}\r\n"));
                assert!(
                    raw.starts_with("HTTP/1.1 404 "),
                    "{target} on {host}: {raw}"
                );
                assert!(raw.contains(share_host::NOT_HERE), "{raw}");
            }
        }
        let root = status_of(
            &guarded,
            "GET",
            "/?t=full",
            "Host: cctop-share.example.test\r\n",
        );
        assert!(root.contains(" 404 "), "{root}");
        let bearer = status_of(
            &guarded,
            "GET",
            "/api/sessions",
            "Host: cctop-share.example.test\r\nAuthorization: Bearer full\r\n",
        );
        assert!(bearer.contains(" 404 "), "{bearer}");
    }

    /// A server with one session and an account on a fake Cloudflare.
    fn addressed() -> (Shared, cctop_core::cloudflare::fake::Seen) {
        let (fake, seen) = address::tests::fake();
        let shared = Shared {
            addresses: Arc::new(fake),
            ..shared("full", "view")
        };
        let mut s = Session::new(
            cctop_core::pricing::Provider::Claude,
            "8f14e45f-ceea-467f-a0e6-0d1c6e1b0a11".into(),
        );
        s.abbrev_label = "web".into();
        *shared.latest.lock().unwrap() = Arc::new(Snapshot {
            version: 1,
            json: "[]".to_string(),
            rows: Vec::new(),
            sessions: vec![s],
            host_errors: Vec::new(),
        });
        (shared, seen)
    }

    fn body_of(raw: &str) -> &str {
        raw.split_once("\r\n\r\n").map_or("", |(_, body)| body)
    }

    /// The page renames an agent's address and the dashboard's with the full
    /// token, against a fake Cloudflare, and is told what it needs: the new
    /// hostname, and for the dashboard the origin to go to.
    #[test]
    fn the_full_token_chooses_addresses() {
        let (guarded, seen) = addressed();
        let read = response_of(&guarded, "GET", "/api/address", &bearer_line("full"));
        assert!(read.starts_with("HTTP/1.1 200 "), "{read}");
        assert!(
            body_of(&read).contains(r#""host":"cctop.example.test""#),
            "{read}"
        );
        assert!(body_of(&read).contains(r#""renamable":true"#), "{read}");
        let agent = response_of(
            &guarded,
            "GET",
            "/api/address/8f14e45f",
            &bearer_line("full"),
        );
        assert!(
            body_of(&agent).contains(r#""host":"cctop-share.example.test""#),
            "{agent}"
        );

        let named = post_json(
            &guarded,
            "/api/address/8f14e45f",
            "full",
            r#"{"name":"myagent"}"#,
        );
        assert!(named.starts_with("HTTP/1.1 200 "), "{named}");
        assert!(
            body_of(&named).contains(r#""host":"myagent.example.test""#),
            "{named}"
        );
        let agent = response_of(
            &guarded,
            "GET",
            "/api/address/8f14e45f",
            &bearer_line("full"),
        );
        assert!(
            body_of(&agent).contains(r#""host":"myagent.example.test""#),
            "{agent}"
        );
        assert!(
            seen.lock().unwrap().iter().any(|(m, p, b)| m == "POST"
                && p == "/zones/zone1/dns_records"
                && b.contains("myagent")),
            "no record was made"
        );

        let moved = post_json(&guarded, "/api/address", "full", r#"{"name":"home"}"#);
        assert!(moved.starts_with("HTTP/1.1 200 "), "{moved}");
        assert!(
            body_of(&moved).contains(r#""origin":"https://home.example.test""#),
            "{moved}"
        );
        assert!(!moved.contains("made-up"), "a token in the answer: {moved}");
    }

    /// Each refusal is its own sentence, as the dialog shows it, and nothing
    /// is written for one.
    #[test]
    fn a_refused_address_is_said_in_a_sentence() {
        let (guarded, seen) = addressed();
        let refused = post_json(
            &guarded,
            "/api/address/8f14e45f",
            "full",
            r#"{"name":"myagent_"}"#,
        );
        assert!(refused.starts_with("HTTP/1.1 400 "), "{refused}");
        assert_eq!(
            body_of(&refused),
            "myagent_ is not a usable name: letters, digits and - only"
        );
        let taken = post_json(
            &guarded,
            "/api/address/8f14e45f",
            "full",
            r#"{"name":"taken"}"#,
        );
        assert_eq!(
            body_of(&taken),
            "taken.example.test is already used by another agent's shares"
        );
        let dashboard = post_json(
            &guarded,
            "/api/address",
            "full",
            r#"{"name":"cctop-share"}"#,
        );
        assert!(dashboard.starts_with("HTTP/1.1 400 "), "{dashboard}");
        assert!(
            !seen.lock().unwrap().iter().any(|(m, ..)| m != "GET"),
            "a refusal wrote something"
        );
    }

    /// The read-only link and a `--no-actions` serve cannot choose an address,
    /// nor read the controls' state, and a GET cannot write.
    #[test]
    fn only_the_full_token_on_an_acting_serve_chooses_addresses() {
        let (guarded, seen) = addressed();
        for target in ["/api/address", "/api/address/8f14e45f"] {
            let view = post_json(&guarded, target, "view", r#"{"name":"myagent"}"#);
            assert!(view.starts_with("HTTP/1.1 403 "), "{target}: {view}");
            let read = status_of(&guarded, "GET", target, &bearer_line("view"));
            assert!(read.contains(" 403 "), "{target}: {read}");
        }
        let (fake, _) = address::tests::fake();
        let no_actions = Shared {
            actions: false,
            addresses: Arc::new(fake),
            ..shared("full", "view")
        };
        for target in ["/api/address", "/api/address/8f14e45f"] {
            let refused = post_json(&no_actions, target, "full", r#"{"name":"myagent"}"#);
            assert!(refused.starts_with("HTTP/1.1 403 "), "{target}: {refused}");
            let read = status_of(&no_actions, "GET", target, &bearer_line("full"));
            assert!(read.contains(" 403 "), "{target}: {read}");
        }
        assert!(seen.lock().unwrap().is_empty(), "Cloudflare was asked");
    }

    #[test]
    fn metrics_answers_without_a_token_and_nothing_else_does() {
        let guarded = shared("full", "view");
        for target in ["/metrics", "/metrics?t=wrong"] {
            let status = status_of(&guarded, "GET", target, "");
            assert!(status.contains(" 200 "), "{target}: {status}");
        }
        let wrong = status_of(
            &guarded,
            "GET",
            "/metrics",
            "Authorization: Bearer wrong\r\n",
        );
        assert!(wrong.contains(" 200 "), "{wrong}");
        // The bypass is that one path: its neighbours keep the gate.
        for target in ["/api/sessions", "/metrics/", "/metricsx", "/"] {
            let status = status_of(&guarded, "GET", target, "");
            assert!(status.contains(" 403 "), "{target}: {status}");
        }
    }

    /// One request through the router, as the head and the undecoded body.
    fn exchange(shared: &Shared, target: &str, headers: &str) -> (String, Vec<u8>) {
        use std::io::Read;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        client
            .write_all(format!("GET {target} HTTP/1.1\r\n{headers}\r\n").as_bytes())
            .unwrap();
        // Answered on a thread of its own: the app page is most of a megabyte,
        // more than a socket buffers, and its write would wait on this read.
        let mut raw = Vec::new();
        std::thread::scope(|scope| {
            scope.spawn(move || serve_connection(shared, &mut server));
            client.read_to_end(&mut raw).unwrap();
        });
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("a response head");
        let head = String::from_utf8(raw[..split].to_vec()).unwrap();
        (head, raw[split + 4..].to_vec())
    }

    /// The value of header `name` in a response head.
    fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
        head.lines().find_map(|line| {
            let (k, v) = line.split_once(':')?;
            k.eq_ignore_ascii_case(name).then_some(v.trim())
        })
    }

    const JS_PATH: &str = "/_astro/index.astro_astro_type_script_index_0_lang.CdYCpIGm.js";

    #[test]
    fn hashed_files_are_kept_for_a_year_and_pages_are_revalidated() {
        let guarded = shared("full", "view");
        let auth = "Authorization: Bearer full\r\n";
        for (path, cache) in [
            (JS_PATH, http::IMMUTABLE),
            ("/_astro/index.D2bSaP4U.css", http::IMMUTABLE),
            (
                "/_astro/rmux_web_crypto_wasm_bg.C8R0tHIf.wasm",
                http::IMMUTABLE,
            ),
            ("/term/crabs/lime-dark.svg", http::IMMUTABLE),
            ("/term/", http::REVALIDATE),
            ("/", http::REVALIDATE),
            ("/favicon.svg", http::REVALIDATE),
        ] {
            let (head, _) = exchange(&guarded, path, auth);
            assert!(head.starts_with("HTTP/1.1 200 "), "{path}: {head}");
            assert_eq!(header(&head, "Cache-Control"), Some(cache), "{path}");
            if cache == http::REVALIDATE {
                assert!(header(&head, "ETag").is_some(), "{path} has no ETag");
            }
        }
        // Behind the gate like everything else: caching is not a way round it.
        let (head, _) = exchange(&guarded, JS_PATH, "");
        assert!(head.starts_with("HTTP/1.1 403 "), "{head}");
        // And what holds a transcript is still never kept.
        let (head, _) = exchange(&guarded, "/api/sessions", auth);
        assert_eq!(header(&head, "Cache-Control"), Some("no-store"));
    }

    /// The table a page polls when its stream is held up is never kept, yet
    /// an unchanged one is a `304`: the page remembers the tag, not the cache.
    #[test]
    fn the_polled_table_is_a_304_when_it_has_not_moved_and_still_not_kept() {
        let guarded = shared("full", "view");
        put(&guarded, 1, &[("a", r#"{"a":1}"#)]);
        let auth = "Authorization: Bearer full\r\n";
        let (head, body) = exchange(&guarded, "/api/sessions", auth);
        assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
        assert_eq!(body, br#"[{"a":1}]"#);
        let etag = header(&head, "ETag").expect("an ETag").to_string();
        let held = format!("{auth}If-None-Match: {etag}\r\n");

        let (head, body) = exchange(&guarded, "/api/sessions", &held);
        assert!(head.starts_with("HTTP/1.1 304 "), "{head}");
        assert!(body.is_empty(), "a 304 carries no body");
        assert_eq!(header(&head, "Cache-Control"), Some("no-store"));

        // A table that moved is sent whole, under a new tag.
        put(&guarded, 2, &[("a", r#"{"a":2}"#)]);
        let (head, body) = exchange(&guarded, "/api/sessions", &held);
        assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
        assert_eq!(body, br#"[{"a":2}]"#);
        assert_ne!(header(&head, "ETag"), Some(etag.as_str()));
    }

    #[test]
    fn a_page_the_browser_holds_is_answered_304_and_no_body() {
        let guarded = shared("full", "view");
        for path in ["/?t=full", "/term/", JS_PATH] {
            let (head, body) = exchange(&guarded, path, "Authorization: Bearer full\r\n");
            let etag = header(&head, "ETag").expect("an ETag").to_string();
            assert!(!body.is_empty());
            let (head, body) = exchange(
                &guarded,
                path,
                &format!("Authorization: Bearer full\r\nIf-None-Match: {etag}\r\n"),
            );
            assert!(head.starts_with("HTTP/1.1 304 "), "{path}: {head}");
            assert!(body.is_empty(), "{path}: a 304 carries no body");
            assert_eq!(header(&head, "ETag"), Some(etag.as_str()));
            // A stale tag is answered in full.
            let (head, _) = exchange(
                &guarded,
                path,
                "Authorization: Bearer full\r\nIf-None-Match: W/\"stale\"\r\n",
            );
            assert!(head.starts_with("HTTP/1.1 200 "), "{path}: {head}");
        }
        // The page carries no credential, so both links are one page with
        // one tag: it names the build, not the link.
        let tag = |token: &str| {
            let (head, _) = exchange(&guarded, &format!("/?t={token}"), "");
            header(&head, "ETag").unwrap().to_string()
        };
        assert_eq!(tag("full"), tag("view"));
        // A 304 on a link's first visit still hands its cookie back.
        let full = tag("full");
        let (head, _) = exchange(&guarded, "/?t=full", &format!("If-None-Match: {full}\r\n"));
        assert!(head.starts_with("HTTP/1.1 304 "), "{head}");
        assert!(header(&head, "Set-Cookie").is_some(), "{head}");
    }

    /// The cookie's `Secure` attribute follows the tunnel, not the listener:
    /// a page reached through Cloudflare's edge was HTTPS on the browser's
    /// side and must never fall back to plain HTTP; a page opened on this
    /// machine's own `http://127.0.0.1` would have a `Secure` cookie dropped
    /// by every browser, losing the reload it exists for.
    #[test]
    fn the_cookie_is_secure_only_when_the_browser_came_through_the_tunnel() {
        let tunneled = Shared {
            is_tunnel_host: |host: &str| host.ends_with(".trycloudflare.com"),
            ..shared("full", "view")
        };
        let cookie = |shared: &Shared, headers: &str| {
            let (head, _) = exchange(shared, "/?t=full", headers);
            assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
            header(&head, "Set-Cookie").expect("a cookie").to_string()
        };

        // This machine, over plain HTTP: no Secure, or the browser would
        // refuse to send the cookie back to the address it came from.
        let plain = cookie(&shared("full", "view"), "");
        assert!(plain.contains("HttpOnly"), "{plain}");
        assert!(plain.contains("SameSite=Strict"), "{plain}");
        assert!(!plain.contains("Secure"), "{plain}");

        // The edge says HTTPS, so the attribute goes on.
        let forwarded = cookie(
            &tunneled,
            "Host: dash.trycloudflare.com\r\nX-Forwarded-Proto: https\r\n",
        );
        assert!(forwarded.contains("Secure"), "{forwarded}");

        // And a request through the tunnel carries the edge's headers even
        // where the proxy stripped `X-Forwarded-Proto`.
        let edged = cookie(
            &tunneled,
            "Host: dash.trycloudflare.com\r\nCf-Ray: 8f2c\r\n",
        );
        assert!(edged.contains("Secure"), "{edged}");

        // A proxy in front that says plain keeps the cookie sendable.
        let demoted = cookie(
            &shared("full", "view"),
            "Host: 192.168.1.4:7777\r\nX-Forwarded-Proto: http\r\n",
        );
        assert!(!demoted.contains("Secure"), "{demoted}");
    }

    #[test]
    fn bodies_are_compressed_for_a_client_that_offers_it() {
        use std::io::Read;
        let guarded = shared("full", "view");
        let auth = "Authorization: Bearer full\r\n";
        for (path, source) in [
            (
                JS_PATH,
                "src/assets/rmux-share/_astro/index.astro_astro_type_script_index_0_lang.CdYCpIGm.js",
            ),
            ("/term/", "src/assets/rmux-share/index.html"),
            ("/", "src/assets/app/index.html"),
        ] {
            let file = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(source))
                .unwrap();
            // Each client is sent the file itself in the coding it took, with
            // a length that is the length on the wire.
            for (offer, coding) in [
                ("", None),
                ("Accept-Encoding: gzip\r\n", Some("gzip")),
                ("Accept-Encoding: gzip, deflate, br\r\n", Some("br")),
            ] {
                let (head, body) = exchange(&guarded, path, &format!("{auth}{offer}"));
                assert!(head.starts_with("HTTP/1.1 200 "), "{path}: {head}");
                assert_eq!(header(&head, "Content-Encoding"), coding, "{path} {offer}");
                assert_eq!(header(&head, "Vary"), Some("Accept-Encoding"), "{path}");
                assert_eq!(
                    header(&head, "Content-Length"),
                    Some(body.len().to_string().as_str())
                );
                let mut plain = Vec::new();
                match coding {
                    None => plain = body.clone(),
                    Some("gzip") => {
                        flate2::read::GzDecoder::new(&body[..])
                            .read_to_end(&mut plain)
                            .unwrap();
                    }
                    _ => {
                        brotli_decompressor::Decompressor::new(&body[..], 4096)
                            .read_to_end(&mut plain)
                            .unwrap();
                    }
                }
                assert!(plain == file, "{path} {offer}: not the file");
                if coding.is_some() {
                    assert!(body.len() < file.len(), "{path} {offer}");
                }
            }
        }
        assert_eq!(
            term::file(JS_PATH).unwrap().content_type,
            "application/javascript; charset=utf-8"
        );

        // A route's JSON, built per request, is compressed too.
        let table: Vec<(String, String)> = (0..40)
            .map(|i| {
                (
                    format!("s{i}"),
                    format!(r#"{{"session_id":"s{i}","title":"a session"}}"#),
                )
            })
            .collect();
        let pairs: Vec<(&str, &str)> = table
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        put(&guarded, 1, &pairs);
        // Gzip even to a client that would take brotli: the binary carries no
        // brotli encoder, and the build compressed only the fixed bodies.
        for offer in ["gzip", "gzip, deflate, br"] {
            let (head, _) = exchange(
                &guarded,
                "/api/sessions",
                &format!("{auth}Accept-Encoding: {offer}\r\n"),
            );
            assert_eq!(header(&head, "Content-Encoding"), Some("gzip"), "{head}");
        }
    }

    /// A serve on the account's tunnel whose token is `SIBLING_SECRET`,
    /// holding one row of its own and one it read from somewhere else.
    const SIBLING_SECRET: &str = "eyJhIjoi-made-up-tunnel-token";

    fn sibling() -> Shared {
        let shared = Shared {
            peer_secret: SIBLING_SECRET.to_string(),
            ..shared("full", "view")
        };
        let own = Session::new(cctop_core::pricing::Provider::Claude, "own-row".into());
        let mut read = Session::new(cctop_core::pricing::Provider::Claude, "read-row".into());
        read.remote = Some(cctop_core::session::Remote {
            host: "elsewhere".into(),
            ..Default::default()
        });
        *shared.latest.lock().unwrap() = Arc::new(Snapshot {
            version: 1,
            json: r#"[{"session_id":"own-row"},{"session_id":"read-row","host":"elsewhere"}]"#
                .into(),
            rows: vec![
                ("own-row".into(), r#"{"session_id":"own-row"}"#.into()),
                (
                    "read-row".into(),
                    r#"{"session_id":"read-row","host":"elsewhere"}"#.into(),
                ),
            ],
            sessions: vec![own, read],
            host_errors: Vec::new(),
        });
        shared
    }

    fn signed(method: &str, path: &str, secret: &str, at: u64) -> String {
        format!(
            "Authorization: {}\r\n",
            cctop_core::peer::sign(secret, at, method, path)
        )
    }

    /// The signature is the credential, the far side's token is never handed
    /// to it, and a sibling reading the table gets this machine's own rows —
    /// not the ones this machine read from elsewhere, which would come back
    /// to it as its own.
    #[test]
    fn a_sibling_signed_with_the_tunnel_token_reads_the_local_rows_only() {
        let b = sibling();
        let now = cctop_core::peer::now();
        let ok = signed("GET", "/api/sessions", SIBLING_SECRET, now);
        let raw = response_of(&b, "GET", "/api/sessions", &ok);
        assert!(raw.starts_with("HTTP/1.1 200"), "{raw}");
        assert!(
            raw.contains("own-row") && !raw.contains("read-row"),
            "{raw}"
        );
        // A browser with the token still gets every row.
        let browser = response_of(&b, "GET", "/api/sessions?t=full", "");
        assert!(browser.contains("read-row"));

        for refused in [
            signed("GET", "/api/sessions", "eyJhIjoi-another-token", now),
            signed("GET", "/api/sessions", SIBLING_SECRET, now - 600),
            signed("GET", "/api/config", SIBLING_SECRET, now),
        ] {
            let raw = response_of(&b, "GET", "/api/sessions", &refused);
            assert!(raw.starts_with("HTTP/1.1 403"), "{refused}: {raw}");
        }
        // Not on the account's tunnel: no secret, no signature gets in.
        let raw = response_of(&shared("full", "view"), "GET", "/api/sessions", &ok);
        assert!(raw.starts_with("HTTP/1.1 403"), "{raw}");

        let config = response_of(
            &b,
            "GET",
            "/api/config",
            &signed("GET", "/api/config", SIBLING_SECRET, now),
        );
        let body: serde_json::Value =
            serde_json::from_str(config.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["token"], "", "a sibling is never handed a token");
        assert_eq!(body["actions"], true);
    }

    /// The hub relays a sibling's API for its own page: a read for any link,
    /// an action only for one that may act here, and never with the page's
    /// token on the way out.
    #[test]
    fn the_hub_relays_a_siblings_api_and_draws_the_acting_line_itself() {
        // The sibling, serving for real on loopback.
        let b = Arc::new(sibling());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        {
            let b = Arc::clone(&b);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { return };
                    serve_connection(&b, &mut stream);
                }
            });
        }
        cctop_core::peer::set_known(vec![Arc::new(cctop_core::peer::Peer::fixed(
            "box",
            &format!("http://127.0.0.1:{port}"),
            SIBLING_SECRET,
        ))]);

        let hub = shared("full", "view");
        let read = response_of(&hub, "GET", "/api/peer/box/api/sessions?t=view", "");
        assert!(read.starts_with("HTTP/1.1 200"), "{read}");
        assert!(
            read.contains("own-row") && !read.contains("read-row"),
            "{read}"
        );

        let act = |token: &str| {
            let body = r#"{"text":"hi"}"#;
            response_of(
                &hub,
                "POST",
                &format!("/api/peer/box/api/act/send/own-row?t={token}"),
                &format!(
                    "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                ),
            )
        };
        // The read-only link is stopped here, before anything is sent on.
        assert!(act("view").starts_with("HTTP/1.1 403"));
        // The full link's action reaches the sibling, which answers for its
        // own row (here: nothing is running it) rather than refusing the
        // signature.
        let full = act("full");
        assert!(!full.starts_with("HTTP/1.1 403"), "{full}");
        assert!(!full.starts_with("HTTP/1.1 502"), "{full}");

        // Pages and the event stream are not the sibling's to relay, nor is
        // a machine the account does not have.
        for target in ["/api/peer/box/?t=full", "/api/peer/box/api/events?t=full"] {
            assert!(response_of(&hub, "GET", target, "").starts_with("HTTP/1.1 404"));
        }
        let nobody = response_of(&hub, "GET", "/api/peer/nobody/api/sessions?t=full", "");
        assert!(nobody.starts_with("HTTP/1.1 404"), "{nobody}");
    }
}
