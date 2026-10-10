//! Putting the local port on a public URL: a quick tunnel, or the user's own
//! Cloudflare account.
//!
//! A quick tunnel needs no Cloudflare account and no DNS: the client asks
//! `api.trycloudflare.com` for a hostname and a secret, dials the argotunnel
//! edge, and registers. It stops existing when the process does. That is the
//! right shape for "let me look at this from my phone for ten minutes" and the
//! wrong shape for anything permanent — the URL changes every run, which is also
//! why it is not a substitute for the token.
//!
//! An account tunnel is the permanent shape: a named tunnel on the user's own
//! free Cloudflare account, at a hostname on their own domain, connected with
//! `cctop tunnel setup` and remembered in the `[tunnel]` table of
//! `config.toml` (or given as `CCTOP_TUNNEL_TOKEN`). It lifts what Cloudflare
//! documents as a quick tunnel's limits — no Server-Sent Events, which the live
//! table is; 200 requests in flight; no uptime promise; a new name each run.
//! When one is connected, `--tunnel` uses it, and a quick tunnel stands in only
//! when it cannot come up, with the reason said ([`Tunnel::fallback`]).
//!
//! The client for both is [`cctop_tunnel`], which speaks the edge's protocol
//! itself — QUIC to the edge and capnp-RPC over it — rather than shelling out
//! to `cloudflared` and reading a URL off its stderr. So `--tunnel` needs
//! nothing installed and cctop stays one binary, which is the whole reason it
//! ships as one.
//!
//! Not inside `serve`, though `cctop serve --tunnel` is its first user: a terminal
//! share from rmux opens one too ([`crate::rmux`]), with no server of cctop's
//! around it.
//!
//! The consequence worth knowing: the tunnel's data path is *inside this
//! process*. Every request from the internet arrives as a QUIC stream, gets
//! proxied to the loopback listener by a task on the runtime below, and lands on
//! the same socket a local browser would use. So the connection cap, the token
//! check and the deadlines in `serve::http` all still apply — the tunnel adds
//! a route in, not a second server.
//!
//! # One connector per tunnel
//!
//! Two processes registering the same named tunnel do not conflict — they
//! become replicas, and the edge splits requests between them, so half of
//! them would reach the wrong cctop. An `flock` keyed by the tunnel's id makes
//! the second one a quick tunnel instead, and says why.
//!
//! # What it does not do
//!
//! Cloudflare terminates the TLS. The traffic is encrypted from the browser to
//! the edge and from the edge to here, and readable in between by the party
//! carrying it — which is worth saying because the opposite is easy to assume of
//! anything with a `https://` URL. A tunnel is a way to reach your own machine
//! from a phone, not a private channel, and the announcement `cctop serve`
//! prints says so where someone will actually read it. That is as true of the
//! account's tunnel as of a quick one.

use std::fmt;
use std::fs::File;
use std::path::Path;

use cctop_tunnel::cloudflare::{Credentials, Named, Quick};
use cctop_tunnel::{Provider, Routes};

use crate::config;

/// Which tunnel `--tunnel` asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Want {
    /// The account's when one is connected, a quick one otherwise.
    #[default]
    Auto,
    /// A quick tunnel whatever is connected: `--tunnel=quick`, and every
    /// caller that must not take the account's hostname for itself.
    Quick,
}

/// Which tunnel a [`Tunnel`] turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Quick,
    Account,
}

/// A live tunnel, reachable at [`url`](Tunnel::url) until it is dropped.
pub struct Tunnel {
    /// The `https://` origin: the account's hostname, or the
    /// `…trycloudflare.com` one the edge assigned this run.
    pub url: String,
    pub kind: Kind,
    /// Why a quick tunnel is standing in for the account's, when one is.
    pub fallback: Option<String>,
    /// Declared before the runtime so it drops first: dropping it signals the
    /// edge reactors to wind down, which needs the runtime they are still
    /// running on.
    _handle: cctop_tunnel::Tunnel,
    /// Held for as long as this process is the tunnel's connector.
    _lock: Option<File>,
    /// This tunnel's entry in [`SHARES`], when it is the account's and has a
    /// share hostname: gone with the tunnel, so a share is never handed a
    /// hostname nothing carries any more.
    _shares: Option<Lent>,
    /// Not a handle to park — this *is* the tunnel. The tasks it drives accept
    /// the edge's streams and proxy them; if it stops turning, the public URL
    /// stops answering.
    _runtime: tokio::runtime::Runtime,
}

impl Tunnel {
    /// What to call it in a sentence.
    pub fn describe(&self) -> &'static str {
        match self.kind {
            Kind::Quick => "a trycloudflare quick tunnel",
            Kind::Account => "your Cloudflare tunnel",
        }
    }
}

/// Register a tunnel to `port` and return it, or say why not.
///
/// Blocking, and deliberately so: there is nothing to serve over a tunnel that
/// does not exist yet, and a URL printed before the edge has the registration is
/// a link that 404s for whoever opens it first.
///
/// `share_front` is the loopback port of the server that answers the
/// account's share hostnames — rmux's static frontend and the share socket,
/// nothing of the page's (`cctop_serve`'s `share_host` module). Without one no
/// share hostname is routed, and `W` takes a quick tunnel as it would with no
/// account at all.
///
/// Silent, also deliberately. This is called with the dashboard on screen as
/// often as from the command line, and a line written to stderr under a TUI is
/// painted straight over it — `cctop: opening a trycloudflare tunnel…` sat
/// across somebody's session list for exactly as long as the registration took.
/// Whoever called says so on the surface they own: the command line prints it,
/// and the dashboard spins. A fallback is reported the same way, through
/// [`Tunnel::fallback`].
pub fn start(port: u16, want: Want, share_front: Option<u16>) -> anyhow::Result<Tunnel> {
    // Two workers, because the proxying happens here rather than in somebody
    // else's process: one accepts streams while the other is still writing a
    // response.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;

    let account = match want {
        Want::Auto => account(),
        Want::Quick => None,
    };
    let mut fallback = None;
    if let Some(account) = account {
        match open_account(&runtime, port, &account) {
            Ok((handle, lock)) => {
                // Share hostnames only with a front to send them to: one
                // routed anywhere else would be a road nobody vetted.
                let share = account.share_hostname.as_deref().zip(share_front);
                let shares = Some(SHARES.lend(handle.routes().clone(), handle.hostname(), share));
                sync_link();
                watch_link(SHARES.generation());
                return Ok(Tunnel {
                    url: handle.url().to_string(),
                    kind: Kind::Account,
                    fallback: None,
                    _shares: shares,
                    _handle: handle,
                    _lock: Some(lock),
                    _runtime: runtime,
                });
            }
            Err(why) => fallback = Some(why),
        }
    }

    // ponytail: the crate's default HA connection count, untuned. It trades a
    // second QUIC connection for masking a single-POP reconnect, and the page
    // reports a gap of its own anyway.
    let handle = runtime
        .block_on(Quick::new().open(Routes::new(port)))
        .map_err(|e| match &fallback {
            Some(why) => anyhow::anyhow!("{why}, and a quick tunnel failed too: {e}"),
            None => anyhow::anyhow!("{e}\nDrop --tunnel to serve on this machine only."),
        })?;
    Ok(Tunnel {
        url: handle.url().to_string(),
        kind: Kind::Quick,
        fallback,
        _shares: None,
        _handle: handle,
        _lock: None,
        _runtime: runtime,
    })
}

/// Bring up the account's tunnel, or the sentence saying why not.
fn open_account(
    runtime: &tokio::runtime::Runtime,
    port: u16,
    account: &Account,
) -> Result<(cctop_tunnel::Tunnel, File), String> {
    let credentials = Credentials::from_token(&account.token)
        .map_err(|e| format!("the Cloudflare tunnel token cctop has is not usable ({e})"))?;
    let lock = claim(&credentials).ok_or_else(|| {
        "another cctop on this machine is already serving your Cloudflare tunnel".to_string()
    })?;
    let named = Named::new(credentials, account.hostname.clone());
    let handle = runtime
        .block_on(named.open(Routes::new(port)))
        .map_err(|e| why_not(&e))?;
    Ok((handle, lock))
}

/// The account tunnel's failure, in the words the user acts on.
fn why_not(error: &cctop_tunnel::Error) -> String {
    match error {
        cctop_tunnel::Error::Refused(_) => "your Cloudflare tunnel was deleted or its token \
             revoked; run `cctop tunnel setup` again"
            .to_string(),
        cctop_tunnel::Error::NoHostname => "your Cloudflare tunnel has no hostname yet; run \
             `cctop tunnel setup`, or set CCTOP_TUNNEL_HOSTNAME"
            .to_string(),
        other => format!("your Cloudflare tunnel could not reach Cloudflare's edge ({other})"),
    }
}

/// Whether a cctop on this machine is connected as `account`'s tunnel right
/// now — so that removing it would pull it out from under that process.
pub fn in_use(account: &Account) -> bool {
    match Credentials::from_token(&account.token) {
        Ok(credentials) => claim(&credentials).is_none(),
        Err(_) => false,
    }
}

/// Become the one connector of this tunnel on this machine, or `None` when
/// another process already is.
fn claim(credentials: &Credentials) -> Option<File> {
    claim_in(&config::runtime_base(), &credentials.tunnel_id.to_string())
}

fn claim_in(dir: &Path, tunnel_id: &str) -> Option<File> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let _ = std::fs::create_dir_all(dir);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(dir.join(format!("tunnel-{tunnel_id}.lock")))
        // A lock that cannot be taken at all is not a reason to refuse the
        // tunnel; it is a reason to say nothing and go without the guard.
        .ok()?;
    // SAFETY: flock on a descriptor this function owns for the call.
    match unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } {
        0 => Some(file),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Terminal shares on the account's second hostname
// ---------------------------------------------------------------------------

/// The account tunnel this process holds, lent to terminal shares.
///
/// `cctop tunnel setup` makes two hostnames: the page's, and a `-share` one
/// for `W`'s links. A share cannot ride the page's hostname — every route
/// there wants the page's token, and a cold share link must not carry it — but
/// it can ride the same tunnel on the other hostname, since the tunnel routes
/// by `Host`. So whoever brings the account tunnel up (`cctop serve`, or the
/// dashboard's serve) lends its routing table here, and [`crate::rmux`] asks
/// for a route instead of registering a second, quick tunnel.
///
/// A share hostname goes to the *share front*, not to rmux's listener: a
/// loopback server of its own (`cctop_serve`'s `share_host` module) that
/// answers rmux's static frontend and relays the share socket to
/// [`share_upstream`], and has no other route at all. So a link reads
/// `https://<share host>/#…` — the app and its socket on the one hostname —
/// rather than sending the reader to `share.rmux.io` first, and a share
/// hostname still cannot become a way into the dashboard: the page's server
/// never sees its requests, and refuses them by `Host` if it ever does
/// ([`is_share_host`]).
static SHARES: Shares = Shares::new();

/// The share route for rmux's listener on `port`: `https://<share hostname>`,
/// with that hostname sent to the share front from the next request on, and
/// the front relaying to `port`. `None` when this process holds no account
/// tunnel with a share hostname — no account, no serve running, the account
/// fell back to a quick tunnel, or it came from `CCTOP_TUNNEL_TOKEN` with
/// nothing to name the share hostname — and then the caller opens its own
/// quick tunnel, as before.
///
/// A daemon restarted on a new port asks again and the upstream moves; old
/// links to the old port are refused by rmux's own token, which is correct.
pub fn share_origin(port: u16) -> Option<String> {
    SHARES.route(port, None)
}

/// [`share_origin`] on `host` rather than the default share hostname: an
/// agent's own name ([`Account::share_host_for`]). Routed to the same front,
/// which tells shares apart by the token in the link, not by the hostname.
pub fn share_origin_at(port: u16, host: &str) -> Option<String> {
    SHARES.route(port, Some(host))
}

/// Stop answering `host`, an agent's name that was renamed or cleared. Only
/// a name the front answers, and never the default share hostname, which
/// other agents' links still use.
pub fn forget_share_host(host: &str) {
    SHARES.forget(host);
}

/// The hostname the page is reached on over the account's tunnel right now,
/// which a rename can move while the tunnel is up.
pub fn page_host() -> Option<String> {
    SHARES.page_host()
}

/// Send the page's requests on `new` from now on and stop answering `old`:
/// a renamed dashboard, without re-registering the tunnel. Nothing when the
/// tunnel held is not on `old`.
pub fn move_page(old: &str, new: &str) {
    SHARES.move_page(old, new);
}

/// The rmux listener port the share front relays the socket to, while this
/// process lends a tunnel and a share has been routed on it.
pub fn share_upstream() -> Option<u16> {
    SHARES.upstream()
}

/// Whether `host` is one of the hostnames routed to the share front. The
/// page's server refuses such a request outright, whatever token it carries:
/// the tunnel never sends one there, and if a misrouting ever did, a share
/// hostname must still not open the dashboard.
pub fn is_share_host(host: &str) -> bool {
    SHARES.is_share_host(host)
}

/// Whether `host` reaches the page over the account's tunnel this process
/// holds: the dashboard's hostname, or the token hostname. A request whose
/// `Host` is one of these came through the tunnel — the edge only forwards
/// a hostname the table routes, and writes it into `Host` itself.
pub fn is_tunnel_host(host: &str) -> bool {
    SHARES.is_page_route(host)
}

/// Route the token hostname to the page, or stop, as `config.toml` says now
/// ([`crate::cloudflare::access::Settings::link_hostname`] while public links
/// are on). Called when the tunnel is lent, after an edit made here, and by
/// [`watch_link`] for an edit made by another cctop.
pub fn sync_link() {
    let want = access_settings()
        .filter(|s| s.public_links)
        .and_then(|s| s.link_hostname);
    SHARES.set_link(want);
}

/// Follow `config.toml` while the tunnel lent as `generation` is up, so a
/// `cctop tunnel access links on` in another terminal routes the token
/// hostname here without a restart. A stat every two seconds; the file is
/// read only when it changed. The route is a convenience — the server's own
/// lock is what refuses a token with links off, at its next request.
fn watch_link(generation: u64) {
    let _ = std::thread::Builder::new()
        .name("cctop-link-route".into())
        .spawn(move || {
            let stamp = || {
                std::fs::metadata(&*config::CONFIG_FILE)
                    .ok()
                    .map(|m| (m.modified().ok(), m.len()))
            };
            let mut seen = stamp();
            while SHARES.generation() == generation {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let now = stamp();
                if now != seen && SHARES.generation() == generation {
                    seen = now;
                    sync_link();
                }
            }
        });
}

/// Changes whenever the lent tunnel comes or goes, so a share minted on it
/// can tell it outlived the tunnel it was minted on.
pub fn share_generation() -> u64 {
    SHARES.generation()
}

/// One loan: the routing table, the page's hostname on it, and — when there
/// is one — the default share hostname and the share front's loopback port.
struct Lease {
    generation: u64,
    routes: Routes,
    page: String,
    share: Option<(String, u16)>,
    /// The token hostname, while it is routed to the page.
    link: Option<String>,
}

struct Shares {
    lent: std::sync::Mutex<Option<Lease>>,
    generation: std::sync::atomic::AtomicU64,
    /// rmux's listener port, 0 until a share has been routed.
    upstream: std::sync::atomic::AtomicU16,
}

/// The receipt for a lent routing table; dropping it takes the loan back.
struct Lent {
    shares: &'static Shares,
    generation: u64,
}

impl Shares {
    const fn new() -> Shares {
        Shares {
            lent: std::sync::Mutex::new(None),
            generation: std::sync::atomic::AtomicU64::new(0),
            upstream: std::sync::atomic::AtomicU16::new(0),
        }
    }

    fn lend(&'static self, routes: Routes, page: &str, share: Option<(&str, u16)>) -> Lent {
        let generation = self.bump();
        *self.locked() = Some(Lease {
            generation,
            routes,
            page: page.to_string(),
            share: share.map(|(host, front)| (host.to_string(), front)),
            link: None,
        });
        Lent {
            shares: self,
            generation,
        }
    }

    /// Route `host` — the default share hostname when `None` — to the front,
    /// with the front relaying to rmux's listener on `port`.
    fn route(&self, port: u16, host: Option<&str>) -> Option<String> {
        let lent = self.locked();
        let lease = lent.as_ref()?;
        let (default, front) = lease.share.as_ref()?;
        let host = host.unwrap_or(default);
        // Never the page's own hostname, whatever a stored name says: that
        // would hand the page's address to the front, and the page's links
        // would open a terminal app instead.
        if same_host(host, &lease.page) {
            return None;
        }
        // Nor the token hostname, nor anything else that reaches the page:
        // a share hostname is never one, whichever way round it is asked.
        if lease.routes.port_for(host) == Some(lease.routes.primary()) {
            return None;
        }
        lease.routes.insert(host, *front);
        self.upstream
            .store(port, std::sync::atomic::Ordering::SeqCst);
        Some(format!("https://{host}"))
    }

    fn upstream(&self) -> Option<u16> {
        self.locked().as_ref()?;
        match self.upstream.load(std::sync::atomic::Ordering::SeqCst) {
            0 => None,
            port => Some(port),
        }
    }

    fn is_share_host(&self, host: &str) -> bool {
        self.locked().as_ref().is_some_and(|lease| {
            lease
                .share
                .as_ref()
                .is_some_and(|(_, front)| lease.routes.port_for(host) == Some(*front))
        })
    }

    fn forget(&self, host: &str) {
        let lent = self.locked();
        let Some(Lease {
            routes,
            share: Some((default, front)),
            ..
        }) = lent.as_ref()
        else {
            return;
        };
        if !same_host(host, default) && routes.port_for(host) == Some(*front) {
            routes.remove(host);
        }
    }

    fn page_host(&self) -> Option<String> {
        self.locked().as_ref().map(|lease| lease.page.clone())
    }

    fn is_page_route(&self, host: &str) -> bool {
        let host = host.split(':').next().unwrap_or_default();
        self.locked()
            .as_ref()
            .is_some_and(|lease| lease.routes.port_for(host) == Some(lease.routes.primary()))
    }

    /// Route `want` to the page in place of the token hostname routed
    /// before. Never onto a hostname the share front answers, nor the page's
    /// own: those keep what they go to.
    fn set_link(&self, want: Option<String>) {
        let mut lent = self.locked();
        let Some(lease) = lent.as_mut() else {
            return;
        };
        if lease.link == want {
            return;
        }
        let primary = lease.routes.primary();
        if let Some(old) = lease.link.take()
            && !same_host(&old, &lease.page)
            && lease.routes.port_for(&old) == Some(primary)
        {
            lease.routes.remove(&old);
        }
        if let Some(host) = want {
            let front = lease.share.as_ref().map(|(_, front)| *front);
            let taken = lease.routes.port_for(&host).filter(|p| *p != primary);
            if same_host(&host, &lease.page) || (taken.is_some() && taken == front) {
                return;
            }
            lease.routes.insert(&host, primary);
            lease.link = Some(host);
        }
    }

    fn move_page(&self, old: &str, new: &str) {
        let mut lent = self.locked();
        let Some(lease) = lent.as_mut() else {
            return;
        };
        if !same_host(&lease.page, old) {
            return;
        }
        // The new one first, so there is no moment with neither answering.
        lease.routes.insert(new, lease.routes.primary());
        lease.routes.remove(old);
        lease.page = new.to_string();
    }

    fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn bump(&self) -> u64 {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, Option<Lease>> {
        // A poisoned slot is one a panic left holding a routing table, which
        // is still a routing table.
        self.lent.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Hostnames compare without case or a trailing dot.
fn same_host(a: &str, b: &str) -> bool {
    a.trim_end_matches('.')
        .eq_ignore_ascii_case(b.trim_end_matches('.'))
}

impl Drop for Lent {
    fn drop(&mut self) {
        let mut lent = self.shares.locked();
        // Only its own loan: one process holds one account tunnel (the lock
        // sees to that), but a test lends twice.
        if lent
            .as_ref()
            .is_some_and(|lease| lease.generation == self.generation)
        {
            *lent = None;
            drop(lent);
            self.shares
                .upstream
                .store(0, std::sync::atomic::Ordering::SeqCst);
            self.shares.bump();
        }
    }
}

// ---------------------------------------------------------------------------
// The connected account
// ---------------------------------------------------------------------------

/// What `cctop tunnel setup` connected: the `[tunnel]` table of `config.toml`.
///
/// `token` is the connector's credential, and all a tunnel needs to run. The
/// ids beside it are what `cctop tunnel remove` deletes, and are only there
/// when cctop created the tunnel itself from an API token, which is kept with
/// them so removal needs nothing pasted again.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Account {
    pub token: String,
    pub hostname: Option<String>,
    pub share_hostname: Option<String>,
    pub account_id: Option<String>,
    pub zone_id: Option<String>,
    pub tunnel_id: Option<String>,
    /// What [`crate::cloudflare`] deletes each record by: its id, or for an
    /// account connected by browser login the hostname itself, since the
    /// route that makes the record there answers with no id.
    pub dns_record_ids: Vec<String>,
    pub api_token: Option<String>,
    /// Whether `api_token` came from a browser login's certificate rather
    /// than a paste, which decides how cctop routes and deletes a hostname.
    pub login: bool,
    /// Agents' own share addresses, by the session id of the agent: `W` on
    /// that agent goes out on `<label>.<zone>` instead of the default share
    /// hostname.
    ///
    /// Kept here, with the account, rather than in a file of their own:
    /// each is a DNS record cctop created on this account, and this table is
    /// what `cctop tunnel remove` and the dashboard's disconnect read to
    /// delete what cctop made — so a name is deleted with the account and
    /// forgotten with it, and never outlives the one thing that can remove
    /// its record.
    ///
    /// ponytail: a session deleted from history keeps its name and record
    /// until it is renamed, cleared, or the account is removed.
    pub share_names: std::collections::BTreeMap<String, ShareName>,
    /// Cloudflare Access in front of the page, when `cctop tunnel access on`
    /// put it there: who may log in, and the ids of what cctop created for
    /// it, which `cctop tunnel remove` deletes with the rest. Boxed: most
    /// accounts have none, and an account is carried by value in the
    /// dashboard's connect steps.
    pub access: Option<Box<crate::cloudflare::access::Settings>>,
    /// Whether this came from `CCTOP_TUNNEL_TOKEN` rather than the file — and
    /// so is not cctop's to remove.
    pub from_env: bool,
}

/// One agent's share address: the DNS label under the account's zone, and
/// the id of the record cctop created for it — the only record a rename or
/// a removal ever deletes on its behalf.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShareName {
    pub label: String,
    pub record_id: Option<String>,
}

/// Why an account cannot have its addresses chosen: it was connected with a
/// tunnel token, so cctop holds nothing that can write DNS.
pub const TOKEN_ONLY: &str =
    "Connected with a tunnel token — reconnect with an API token to choose addresses";

impl Account {
    /// The domain the account's hostnames are under: everything after the
    /// page's first label, since setup only ever makes one-label names.
    pub fn zone_name(&self) -> Option<&str> {
        self.hostname
            .as_deref()?
            .split_once('.')
            .map(|(_, zone)| zone)
    }

    /// The hostname `W` on `session_id` goes out on: its own name when it
    /// has one, else the default share hostname.
    pub fn share_host_for(&self, session_id: &str) -> Option<String> {
        self.named_share_host(session_id)
            .or_else(|| self.share_hostname.clone())
    }

    /// `session_id`'s own share hostname, when it has one.
    pub fn named_share_host(&self, session_id: &str) -> Option<String> {
        let name = self.share_names.get(session_id)?;
        Some(format!("{}.{}", name.label, self.zone_name()?))
    }

    /// Whether cctop can write this account's DNS, or the sentence saying
    /// why not.
    pub fn can_rename(&self) -> Result<(), &'static str> {
        let writable = !self.from_env
            && self.api_token.is_some()
            && self.zone_id.is_some()
            && self.account_id.is_some()
            && self.tunnel_id.is_some()
            && self.zone_name().is_some();
        match writable {
            true => Ok(()),
            false => Err(TOKEN_ONLY),
        }
    }
}

/// By hand, because a derived one prints both credentials.
impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Account")
            .field("token", &"[redacted]")
            .field("hostname", &self.hostname)
            .field("share_hostname", &self.share_hostname)
            .field("account_id", &self.account_id)
            .field("zone_id", &self.zone_id)
            .field("tunnel_id", &self.tunnel_id)
            .field("dns_record_ids", &self.dns_record_ids)
            .field("api_token", &self.api_token.as_ref().map(|_| "[redacted]"))
            .field("login", &self.login)
            .field("share_names", &self.share_names)
            .field("access", &self.access)
            .field("from_env", &self.from_env)
            .finish()
    }
}

/// The connected account: `CCTOP_TUNNEL_TOKEN` when it is set, else the
/// `[tunnel]` table, else none. `CCTOP_TUNNEL_HOSTNAME` names the hostname
/// over either, for when the edge's pushed configuration is not wanted.
pub fn account() -> Option<Account> {
    let file = std::fs::read_to_string(&*config::CONFIG_FILE).ok();
    account_from(
        std::env::var("CCTOP_TUNNEL_TOKEN").ok().as_deref(),
        std::env::var("CCTOP_TUNNEL_HOSTNAME").ok().as_deref(),
        file.as_deref(),
    )
}

/// The Access settings in `config.toml`, whatever connected the tunnel: a
/// service running on `CCTOP_TUNNEL_TOKEN` can still put its page behind
/// Access by hand. `None` when there are none, or none that are complete.
pub fn access_settings() -> Option<crate::cloudflare::access::Settings> {
    access_settings_in(&std::fs::read_to_string(&*config::CONFIG_FILE).ok()?)
}

pub fn access_settings_in(text: &str) -> Option<crate::cloudflare::access::Settings> {
    let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    let table = doc.get("tunnel")?.get("access")?.as_table_like()?;
    access_from_table(table).filter(|s| s.usable())
}

/// [`account`] from its three sources, which is the half worth testing.
pub(crate) fn account_from(
    env_token: Option<&str>,
    env_hostname: Option<&str>,
    file: Option<&str>,
) -> Option<Account> {
    let nonempty = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let mut account = match nonempty(env_token) {
        Some(token) => Account {
            token,
            from_env: true,
            ..Account::default()
        },
        None => from_table(file?)?,
    };
    if let Some(hostname) = nonempty(env_hostname) {
        account.hostname = Some(hostname);
    }
    Some(account)
}

fn from_table(text: &str) -> Option<Account> {
    let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    let table = doc.get("tunnel")?.as_table_like()?;
    let text = |key: &str| {
        table
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
    };
    Some(Account {
        token: text("token")?,
        hostname: text("hostname"),
        share_hostname: text("share_hostname"),
        account_id: text("account_id"),
        zone_id: text("zone_id"),
        tunnel_id: text("tunnel_id"),
        dns_record_ids: table
            .get("dns_record_ids")
            .and_then(|v| v.as_array())
            .map(|ids| {
                ids.iter()
                    .filter_map(|v| v.as_str())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
        api_token: text("api_token"),
        login: table
            .get("login")
            .and_then(|v| v.as_str())
            .is_some_and(|v| v == "browser"),
        share_names: table
            .get("share_names")
            .and_then(|v| v.as_table_like())
            .map(|names| {
                names
                    .iter()
                    .filter_map(|(session, entry)| {
                        let entry = entry.as_table_like()?;
                        let field = |key: &str| {
                            entry
                                .get(key)
                                .and_then(|v| v.as_str())
                                .map(str::trim)
                                .filter(|v| !v.is_empty())
                                .map(String::from)
                        };
                        Some((
                            session.to_string(),
                            ShareName {
                                label: field("label")?,
                                record_id: field("record_id"),
                            },
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        access: table
            .get("access")
            .and_then(|v| v.as_table_like())
            .and_then(access_from_table)
            .map(Box::new),
        from_env: false,
    })
}

/// The `[tunnel.access]` table. An invite whose `who` or `level` does not
/// read is dropped rather than guessed at: a rule nobody can say the meaning
/// of should not let anyone in.
fn access_from_table(
    table: &dyn toml_edit::TableLike,
) -> Option<crate::cloudflare::access::Settings> {
    use crate::cloudflare::access::{Invite, Level, Settings, parse_who};
    let text = |key: &str| {
        table
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
    };
    let invites = table
        .get("invites")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|entry| {
                    let entry = entry.as_inline_table()?;
                    Some(Invite {
                        who: parse_who(entry.get("who")?.as_str()?)?,
                        level: Level::from_word(entry.get("level")?.as_str()?)?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Settings {
        team: text("team")?,
        aud: text("aud")?,
        owner: text("owner")?.to_ascii_lowercase(),
        invites,
        app_id: text("app_id"),
        policy_id: text("policy_id"),
        idp_id: text("idp_id"),
        // Only a `false` written there turns them off: a table from before
        // the switch existed keeps the token links it had.
        public_links: table
            .get("public_links")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        link_hostname: text("link_hostname").map(|h| h.to_ascii_lowercase()),
        link_record_id: text("link_record_id"),
    })
}

/// Write `account` as the `[tunnel]` table, replacing any there.
pub fn save_account(account: &Account) -> anyhow::Result<()> {
    save_account_in(&config::CONFIG_FILE, account)
}

pub(crate) fn save_account_in(path: &Path, account: &Account) -> anyhow::Result<()> {
    edit_config(path, |doc| {
        let mut table = toml_edit::Table::new();
        table.insert("token", toml_edit::value(&account.token));
        let optional = [
            ("hostname", &account.hostname),
            ("share_hostname", &account.share_hostname),
            ("account_id", &account.account_id),
            ("zone_id", &account.zone_id),
            ("tunnel_id", &account.tunnel_id),
            ("api_token", &account.api_token),
        ];
        for (key, value) in optional {
            if let Some(value) = value {
                table.insert(key, toml_edit::value(value));
            }
        }
        // A word rather than a boolean, so the file says what it means.
        if account.login {
            table.insert("login", toml_edit::value("browser"));
        }
        if !account.dns_record_ids.is_empty() {
            let ids: toml_edit::Array = account.dns_record_ids.iter().collect();
            table.insert("dns_record_ids", toml_edit::value(ids));
        }
        if !account.share_names.is_empty() {
            let mut names = toml_edit::Table::new();
            // `[tunnel.share_names."<id>"]` sections, not one long line.
            names.set_implicit(true);
            for (session, name) in &account.share_names {
                let mut entry = toml_edit::Table::new();
                entry.insert("label", toml_edit::value(&name.label));
                if let Some(id) = &name.record_id {
                    entry.insert("record_id", toml_edit::value(id));
                }
                names.insert(session, toml_edit::Item::Table(entry));
            }
            table.insert("share_names", toml_edit::Item::Table(names));
        }
        if let Some(access) = &account.access {
            let mut entry = toml_edit::Table::new();
            for (key, value) in [
                ("team", Some(&access.team)),
                ("aud", Some(&access.aud)),
                ("owner", Some(&access.owner)),
                ("app_id", access.app_id.as_ref()),
                ("policy_id", access.policy_id.as_ref()),
                ("idp_id", access.idp_id.as_ref()),
                ("link_hostname", access.link_hostname.as_ref()),
                ("link_record_id", access.link_record_id.as_ref()),
            ] {
                if let Some(value) = value {
                    entry.insert(key, toml_edit::value(value));
                }
            }
            entry.insert("public_links", toml_edit::value(access.public_links));
            // One `{ who, level }` per line, so the list reads as a list.
            let mut invites = toml_edit::Array::new();
            for invite in &access.invites {
                let mut one = toml_edit::InlineTable::new();
                one.insert("who", invite.who.as_str().into());
                one.insert("level", invite.level.word().into());
                invites.push(one);
            }
            for value in invites.iter_mut() {
                value.decor_mut().set_prefix("\n  ");
            }
            invites.set_trailing("\n");
            invites.set_trailing_comma(true);
            entry.insert("invites", toml_edit::value(invites));
            table.insert("access", toml_edit::Item::Table(entry));
        }
        doc.insert("tunnel", toml_edit::Item::Table(table));
    })
}

/// Remove the `[tunnel]` table, leaving the rest of the file as it was.
pub fn clear_account() -> anyhow::Result<()> {
    clear_account_in(&config::CONFIG_FILE)
}

pub(crate) fn clear_account_in(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    edit_config(path, |doc| {
        doc.remove("tunnel");
    })
}

/// Rewrite `config.toml` through `edit`, the way the account tokens are
/// written: refused when it does not parse, comments and layout kept by
/// `toml_edit`, and through a temporary file that is owner-only from the
/// start — the file holds secrets, and widening it even briefly is a window.
fn edit_config(path: &Path, edit: impl FnOnce(&mut toml_edit::DocumentMut)) -> anyhow::Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let mut doc = crate::quota::parse_toml(path, &text)?;
    edit(&mut doc);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = crate::quota::temp_beside(path, "toml");
    crate::quota::write_secret(&tmp, &doc.to_string())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn full() -> Account {
        Account {
            token: "eyJhIjoi-made-up".into(),
            hostname: Some("cctop.example.test".into()),
            share_hostname: Some("cctop-share.example.test".into()),
            account_id: Some("acct".into()),
            zone_id: Some("zone".into()),
            tunnel_id: Some("6ff42ae2-765d-4adf-8112-31c55c1551ef".into()),
            dns_record_ids: vec!["rec1".into(), "rec2".into()],
            api_token: Some("made-up-api-token".into()),
            login: true,
            share_names: [
                (
                    "8f14e45f-ceea-467f-a0e6-0d1c6e1b0a11".to_string(),
                    ShareName {
                        label: "myagent".into(),
                        record_id: Some("rec3".into()),
                    },
                ),
                (
                    "rollout-2026-10-08T10-00-00-0199".to_string(),
                    ShareName {
                        label: "codex-one".into(),
                        record_id: Some("rec4".into()),
                    },
                ),
            ]
            .into(),
            access: Some(Box::new(crate::cloudflare::access::Settings {
                app_id: Some("app1".into()),
                policy_id: Some("pol1".into()),
                idp_id: None,
                public_links: false,
                link_hostname: Some("cctop-link.example.test".into()),
                link_record_id: Some("rec5".into()),
                ..crate::cloudflare::access::fake::settings()
            })),
            from_env: false,
        }
    }

    #[test]
    fn an_access_table_from_before_the_switch_keeps_its_token_links() {
        let text = "[tunnel.access]\nteam = \"t.cloudflareaccess.com\"\naud = \"a\"\n\
                    owner = \"owner@example.test\"\n";
        let settings = access_settings_in(text).unwrap();
        assert!(settings.public_links);
        assert_eq!(settings.link_hostname, None);
        let off = format!("{text}public_links = false\n");
        assert!(!access_settings_in(&off).unwrap().public_links);
    }

    #[test]
    fn the_table_round_trips_and_keeps_the_rest_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "# my accounts\n[accounts.work]\ntoken = \"keep-me\" # a comment\n",
        )
        .unwrap();
        save_account_in(&path, &full()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my accounts"), "{text}");
        assert!(
            text.contains("[tunnel.share_names.8f14e45f-ceea-467f-a0e6-0d1c6e1b0a11]"),
            "{text}"
        );
        assert!(text.contains("token = \"keep-me\" # a comment"), "{text}");
        assert!(
            text.contains("\n  { who = \"@example.test\", level = \"read\" },\n]"),
            "{text}"
        );
        assert_eq!(account_from(None, None, Some(&text)), Some(full()));
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        clear_account_in(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("[tunnel]"), "{text}");
        assert!(text.contains("keep-me"), "{text}");
        assert_eq!(account_from(None, None, Some(&text)), None);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn a_file_that_does_not_parse_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "this is [not toml").unwrap();
        assert!(save_account_in(&path, &full()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "this is [not toml");
    }

    #[test]
    fn the_environment_wins_over_the_file() {
        let file = "[tunnel]\ntoken = \"from-file\"\nhostname = \"file.example.test\"\n";
        let from_file = account_from(None, None, Some(file)).unwrap();
        assert_eq!(from_file.token, "from-file");
        assert!(!from_file.from_env);

        let from_env = account_from(Some("from-env"), None, Some(file)).unwrap();
        assert_eq!(from_env.token, "from-env");
        assert!(from_env.from_env);
        assert_eq!(
            from_env.hostname, None,
            "the file's hostname is the file's tunnel's"
        );

        let renamed = account_from(None, Some("env.example.test"), Some(file)).unwrap();
        assert_eq!(renamed.token, "from-file");
        assert_eq!(renamed.hostname.as_deref(), Some("env.example.test"));

        // No config file at all: the environment alone is enough.
        assert!(account_from(Some("from-env"), None, None).is_some());
        // An empty variable is not set.
        assert_eq!(account_from(Some("  "), None, None), None);
    }

    #[test]
    fn debug_prints_neither_credential() {
        let shown = format!("{:?}", full());
        // The two credentials by name: the Access team and AUD tag beside
        // them are made up too, and are not secrets.
        assert!(!shown.contains("eyJhIjoi-made-up"), "{shown}");
        assert!(!shown.contains("made-up-api-token"), "{shown}");
        assert!(shown.contains("cctop.example.test"), "{shown}");
        assert!(shown.contains("myagent"), "{shown}");
    }

    #[test]
    fn an_agent_goes_out_on_its_own_name_or_the_default() {
        let account = full();
        assert_eq!(account.zone_name(), Some("example.test"));
        assert_eq!(
            account
                .share_host_for("8f14e45f-ceea-467f-a0e6-0d1c6e1b0a11")
                .as_deref(),
            Some("myagent.example.test")
        );
        assert_eq!(
            account.share_host_for("unnamed").as_deref(),
            Some("cctop-share.example.test")
        );
        assert_eq!(account.can_rename(), Ok(()));
        let pasted = Account {
            api_token: None,
            ..full()
        };
        assert_eq!(pasted.can_rename(), Err(TOKEN_ONLY));
        let env = Account {
            from_env: true,
            ..full()
        };
        assert_eq!(env.can_rename(), Err(TOKEN_ONLY));
    }

    #[test]
    fn a_second_claim_on_the_same_tunnel_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let first = claim_in(dir.path(), "6ff42ae2-765d-4adf-8112-31c55c1551ef");
        assert!(first.is_some());
        assert!(claim_in(dir.path(), "6ff42ae2-765d-4adf-8112-31c55c1551ef").is_none());
        assert!(claim_in(dir.path(), "another-tunnel").is_some());
        drop(first);
        assert!(claim_in(dir.path(), "6ff42ae2-765d-4adf-8112-31c55c1551ef").is_some());
    }

    #[test]
    fn a_share_rides_the_held_account_tunnel_and_nothing_else() {
        static LOCAL: Shares = Shares::new();
        const FRONT: u16 = 5555;
        // Nothing held: the caller opens its own quick tunnel.
        assert_eq!(LOCAL.route(4000, None), None);
        assert_eq!(LOCAL.upstream(), None);

        let routes = Routes::new(7777);
        routes.insert("cctop.example.test", 7777);
        let before = LOCAL.generation();
        let lent = LOCAL.lend(
            routes.clone(),
            "cctop.example.test",
            Some(("cctop-share.example.test", FRONT)),
        );
        assert_ne!(LOCAL.generation(), before);
        assert_eq!(LOCAL.upstream(), None, "nothing shared yet");
        assert_eq!(
            LOCAL.route(4000, None).as_deref(),
            Some("https://cctop-share.example.test")
        );
        // The share hostname goes to the front, which relays to rmux — never
        // to the page, and never to rmux's listener bare.
        assert_eq!(routes.port_for("cctop-share.example.test"), Some(FRONT));
        assert_eq!(LOCAL.upstream(), Some(4000));
        assert!(LOCAL.is_share_host("CCTOP-share.example.test:443"));
        // The page's hostname is left where it was, and is not a share host.
        assert_eq!(routes.port_for("cctop.example.test"), Some(7777));
        assert!(!LOCAL.is_share_host("cctop.example.test"));
        assert!(!LOCAL.is_share_host("127.0.0.1:7777"));

        // A daemon back on a new port moves the upstream, not the route.
        LOCAL.route(4100, None);
        assert_eq!(routes.port_for("cctop-share.example.test"), Some(FRONT));
        assert_eq!(LOCAL.upstream(), Some(4100));

        // An agent's own name goes to the same front, and forgetting it
        // leaves the default alone.
        assert_eq!(
            LOCAL.route(4100, Some("myagent.example.test")).as_deref(),
            Some("https://myagent.example.test")
        );
        assert_eq!(routes.port_for("myagent.example.test"), Some(FRONT));
        LOCAL.forget("myagent.example.test");
        assert_eq!(routes.port_for("myagent.example.test"), None);
        LOCAL.forget("cctop-share.example.test");
        LOCAL.forget("cctop.example.test");
        assert_eq!(routes.port_for("cctop-share.example.test"), Some(FRONT));
        assert_eq!(routes.port_for("cctop.example.test"), Some(7777));
        // A stored name can never take the page's own hostname.
        assert_eq!(LOCAL.route(4100, Some("CCTOP.example.test")), None);
        assert_eq!(routes.port_for("cctop.example.test"), Some(7777));

        // A renamed dashboard moves the page's route and nothing else.
        assert_eq!(LOCAL.page_host().as_deref(), Some("cctop.example.test"));
        LOCAL.move_page("cctop.example.test", "home.example.test");
        assert_eq!(routes.port_for("home.example.test"), Some(7777));
        assert_eq!(routes.port_for("cctop.example.test"), None);
        assert_eq!(LOCAL.page_host().as_deref(), Some("home.example.test"));
        assert_eq!(routes.port_for("cctop-share.example.test"), Some(FRONT));
        LOCAL.move_page("cctop.example.test", "elsewhere.example.test");
        assert_eq!(
            routes.port_for("elsewhere.example.test"),
            None,
            "not on old"
        );

        let held = LOCAL.generation();
        drop(lent);
        assert_eq!(LOCAL.route(4000, None), None, "the tunnel is gone");
        assert_eq!(LOCAL.upstream(), None);
        assert!(!LOCAL.is_share_host("cctop-share.example.test"));
        assert_ne!(LOCAL.generation(), held, "shares minted on it are stale");
    }

    #[test]
    fn the_token_hostname_reaches_the_page_and_never_the_share_front() {
        static LOCAL: Shares = Shares::new();
        const FRONT: u16 = 5555;
        let routes = Routes::new(7777);
        routes.insert("cctop.example.test", 7777);
        let _lent = LOCAL.lend(
            routes.clone(),
            "cctop.example.test",
            Some(("cctop-share.example.test", FRONT)),
        );
        LOCAL.route(4000, None);
        assert!(LOCAL.is_page_route("cctop.example.test:443"));
        assert!(!LOCAL.is_page_route("cctop-link.example.test"));
        assert!(!LOCAL.is_page_route("127.0.0.1:7777"));

        LOCAL.set_link(Some("cctop-link.example.test".into()));
        assert_eq!(routes.port_for("cctop-link.example.test"), Some(7777));
        assert!(LOCAL.is_page_route("cctop-link.example.test"));
        assert!(!LOCAL.is_share_host("cctop-link.example.test"));
        // A share can never be routed onto it, nor it onto a share's.
        assert_eq!(LOCAL.route(4000, Some("cctop-link.example.test")), None);
        assert_eq!(routes.port_for("cctop-link.example.test"), Some(7777));
        LOCAL.set_link(Some("cctop-share.example.test".into()));
        assert_eq!(routes.port_for("cctop-share.example.test"), Some(FRONT));
        assert_eq!(routes.port_for("cctop-link.example.test"), None);
        assert!(!LOCAL.is_page_route("cctop-share.example.test"));

        // Public links off: the route goes, and the page's stays.
        LOCAL.set_link(Some("cctop-link.example.test".into()));
        LOCAL.set_link(None);
        assert_eq!(routes.port_for("cctop-link.example.test"), None);
        assert_eq!(routes.port_for("cctop.example.test"), Some(7777));
    }

    #[test]
    fn each_failure_says_what_to_do() {
        let refused = why_not(&cctop_tunnel::Error::Refused("Unauthorized".into()));
        assert!(refused.contains("cctop tunnel setup"), "{refused}");
        let nameless = why_not(&cctop_tunnel::Error::NoHostname);
        assert!(nameless.contains("CCTOP_TUNNEL_HOSTNAME"), "{nameless}");
        let unreachable = why_not(&cctop_tunnel::Error::Discovery("no DNS".into()));
        assert!(unreachable.contains("could not reach"), "{unreachable}");
    }
}
