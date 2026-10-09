//! The other cctops on this Cloudflare account, found and read with no host
//! list.
//!
//! Every machine that ran `cctop tunnel setup` against one account has a
//! tunnel there named `cctop-<machine>`, at a hostname of its own. So the
//! account already *is* the list: this lists its `cctop-*` tunnels, reads each
//! one's page hostname off its ingress, and polls that page's `/api/sessions`
//! over the public URL — the same document `--host` reads over ssh, parsed by
//! the same [`fleet::parse`], merged as the same [`Remote`](crate::session::Remote)
//! rows.
//!
//! # How one cctop proves itself to another
//!
//! Being on the same account is the trust; a request needs proof of it, and
//! the tunnel token gives that for free. Each machine holds its own tunnel's
//! token ([`Account::token`](crate::tunnel::Account)), a secret; any API token
//! on the account — a browser login's included — can fetch any tunnel's token
//! (`GET /accounts/<acct>/cfd_tunnel/<id>/token`). So a caller fetches its
//! sibling's token once and signs each request with it:
//!
//! ```text
//! Authorization: cctop-peer <unix secs>.<hex HMAC-SHA256(token, secs + method + path)>
//! ```
//!
//! and the sibling checks it against its own token, within [`SKEW_SECS`]. Nobody
//! off the account can get that token, so a valid signature means "same
//! account". A rotated token invalidates the cached one: the refusal that
//! follows refetches it once ([`Peer::send`]).
//!
//! The path signed is the decoded path without its query, which is what the
//! receiver's request parser hands it; the query rides inside the TLS that
//! Cloudflare's edge terminates, as the token links' own query does.
//!
//! A signature is worth a full link on the far side — what the hub relays for
//! the web page (the server's `/api/peer/<machine>/…`) includes the actions,
//! and the hub decides who may ask for those. It is never handed a
//! token, so it cannot outlive the request it signed.
//!
//! ponytail: one thread polls every sibling in turn. A sibling that is up but
//! slow delays the others by up to [`HTTP_TIMEOUT`]; a thread per sibling is
//! the upgrade when an account holds enough machines for that to show.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::fd::AsRawFd;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::cloudflare::{Api, Auth};
use crate::fleet::{self, Snapshot};

/// The `Authorization` scheme a sibling signs with.
pub const SCHEME: &str = "cctop-peer";

/// How far a signature's clock may be from the receiver's, either way. Wide
/// enough for two machines' clocks that NTP keeps roughly right, narrow
/// enough that a captured header is soon worth nothing.
pub const SKEW_SECS: u64 = 60;

/// How long a request to a sibling may take, whole. A poll is a memory read
/// over there; a forwarded conversation is a transcript parse, which is the
/// slow end this is sized for.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// How often the account's tunnels are listed again. Machines join and leave
/// on a scale of days; a sibling's tunnel going up or down is read off the
/// same listing, so this also bounds how stale an offline dot can be.
const REDISCOVER: Duration = Duration::from_secs(120);

/// The prefix `cctop tunnel setup` names every machine's tunnel with.
const TUNNEL_PREFIX: &str = "cctop-";

/// Sign a request to a sibling whose tunnel token is `secret`: the whole
/// `Authorization` value.
pub fn sign(secret: &str, ts: u64, method: &str, path: &str) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
    let tag = ring::hmac::sign(&key, format!("{ts}{method}{path}").as_bytes());
    let hex: String = tag.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    format!("{SCHEME} {ts}.{hex}")
}

/// Whether `header` is a signature of `method` and `path` by whoever holds
/// `secret`, made within [`SKEW_SECS`] of `now`. An empty secret — a serve not
/// on the account's tunnel — accepts nothing.
pub fn verify(secret: &str, header: &str, method: &str, path: &str, now: u64) -> bool {
    if secret.is_empty() {
        return false;
    }
    let Some((ts, mac)) = header
        .strip_prefix(SCHEME)
        .and_then(|rest| rest.strip_prefix(' '))
        .and_then(|rest| rest.trim().split_once('.'))
    else {
        return false;
    };
    let Ok(ts) = ts.parse::<u64>() else {
        return false;
    };
    if now.abs_diff(ts) > SKEW_SECS || mac.len() != 64 {
        return false;
    }
    let Some(mac) = (0..mac.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(mac.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()
    else {
        return false;
    };
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
    // `verify` compares in constant time, so the tag cannot be guessed a byte
    // at a time from how long a refusal took.
    ring::hmac::verify(&key, format!("{ts}{method}{path}").as_bytes(), &mac).is_ok()
}

/// Seconds since the epoch, the clock a signature is stamped with.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// What a sibling answered.
#[derive(Debug, Clone)]
pub struct Answer {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Where a sibling's token comes from: the account's API, or — in a test — a
/// fixed string.
enum Source {
    Api {
        api: Arc<Api>,
        account_id: String,
        tunnel_id: String,
    },
    #[cfg(any(test, feature = "test-support"))]
    Fixed,
}

/// Another cctop on the account.
pub struct Peer {
    /// Its tunnel's name less `cctop-`: what the HOST column shows and what
    /// `/api/peer/<machine>/` names.
    pub machine: String,
    /// The origin its page answers on, `https://<hostname>`.
    pub url: String,
    /// Whether its tunnel had a connector at the last listing. Nothing
    /// answers on a tunnel without one, so a poll of it says so instead of
    /// waiting on the edge's error page.
    up: bool,
    source: Source,
    /// Its tunnel token, fetched on first use and kept until a refusal says
    /// it changed.
    token: Mutex<Option<String>>,
    /// Why the last poll failed, or `None` when it answered — for a page's
    /// offline dot, whoever is serving it.
    last_error: Mutex<Option<String>>,
}

impl Peer {
    /// A sibling at `url` that signs with `secret`, for a test.
    #[cfg(any(test, feature = "test-support"))]
    pub fn fixed(machine: &str, url: &str, secret: &str) -> Peer {
        Peer {
            machine: machine.to_string(),
            url: url.trim_end_matches('/').to_string(),
            up: true,
            source: Source::Fixed,
            token: Mutex::new(Some(secret.to_string())),
            last_error: Mutex::new(None),
        }
    }

    /// The tunnel token to sign with: the cached one, or a fresh fetch.
    fn secret(&self, fresh: bool) -> Result<String, String> {
        if !fresh && let Some(token) = self.token.lock().ok().and_then(|t| t.clone()) {
            return Ok(token);
        }
        let token = match &self.source {
            Source::Api {
                api,
                account_id,
                tunnel_id,
            } => api
                .tunnel_token(account_id, tunnel_id)
                .map_err(|e| format!("could not fetch its tunnel token: {e}"))?,
            #[cfg(any(test, feature = "test-support"))]
            Source::Fixed => return self.secret(false),
        };
        if let Ok(mut slot) = self.token.lock() {
            *slot = Some(token.clone());
        }
        Ok(token)
    }

    /// Send one request, signed, and return what came back — any status.
    ///
    /// A 401 or 403 with a token that was cached is retried once with a fresh
    /// one: that is what a rotated tunnel token looks like from here.
    pub fn send(
        &self,
        method: &str,
        path: &str,
        query: &str,
        body: Option<&[u8]>,
    ) -> Result<Answer, String> {
        let cached = self.token.lock().is_ok_and(|t| t.is_some());
        let answer = self.attempt(&self.secret(false)?, method, path, query, body)?;
        if cached && matches!(answer.status, 401 | 403) && self.can_refetch() {
            return self.attempt(&self.secret(true)?, method, path, query, body);
        }
        Ok(answer)
    }

    fn can_refetch(&self) -> bool {
        matches!(self.source, Source::Api { .. })
    }

    fn attempt(
        &self,
        secret: &str,
        method: &str,
        path: &str,
        query: &str,
        body: Option<&[u8]>,
    ) -> Result<Answer, String> {
        static AGENT: std::sync::LazyLock<ureq::Agent> = std::sync::LazyLock::new(|| {
            ureq::Agent::config_builder()
                .timeout_global(Some(HTTP_TIMEOUT))
                // Every status is an answer to pass on: the page shows the
                // sibling's own refusal, not a transport error about it.
                .http_status_as_error(false)
                .build()
                .into()
        });
        let url = match query {
            "" => format!("{}{}", self.url, encode_path(path)),
            q => format!("{}{}?{q}", self.url, encode_path(path)),
        };
        let auth = sign(secret, now(), method, path);
        let sent = match (method, body) {
            ("POST", body) => AGENT
                .post(&url)
                .header("Authorization", &auth)
                .content_type("application/json")
                .send(body.unwrap_or_default()),
            _ => AGENT.get(&url).header("Authorization", &auth).call(),
        };
        let mut response = sent.map_err(|e| format!("could not reach {} ({e})", self.url))?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let body = response
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| format!("{} stopped answering midway ({e})", self.url))?;
        Ok(Answer {
            status,
            content_type,
            body,
        })
    }

    /// Read this sibling's sessions once.
    pub fn poll(&self) -> Snapshot {
        let snapshot = self.read();
        if let Ok(mut last) = self.last_error.lock() {
            *last = match &snapshot {
                Snapshot::Rows(_) => None,
                Snapshot::Failed(why) => Some(why.clone()),
            };
        }
        snapshot
    }

    fn read(&self) -> Snapshot {
        if !self.up {
            return Snapshot::Failed(
                "offline — its tunnel has no connector, so cctop is not serving there".into(),
            );
        }
        match self.send("GET", "/api/sessions", "", None) {
            Ok(answer) if answer.status == 200 => match String::from_utf8(answer.body) {
                Ok(json) => match fleet::parse(&self.machine, &json) {
                    Ok(rows) => Snapshot::Rows(rows),
                    Err(why) => Snapshot::Failed(why),
                },
                Err(_) => Snapshot::Failed("its answer was not UTF-8".into()),
            },
            Ok(answer) => Snapshot::Failed(refusal(&answer)),
            Err(why) => Snapshot::Failed(why),
        }
    }

    /// Open a WebSocket to `path` on this sibling, signed, carrying the
    /// browser's handshake headers that make it one — and nothing else of the
    /// browser's: its cookie is this machine's credential, not the sibling's.
    ///
    /// The `Origin` is the sibling's own, which is the frontend its share was
    /// minted for when the hub asked for the terminal ([`relayed_link`]).
    pub fn socket(
        &self,
        path: &str,
        query: &str,
        headers: &[(String, String)],
    ) -> Result<Upstream, String> {
        let (tls, rest) = match self.url.split_once("://") {
            Some(("https", rest)) => (true, rest),
            Some(("http", rest)) => (false, rest),
            _ => return Err(format!("{} is not an http origin", self.url)),
        };
        let host = rest.split('/').next().unwrap_or(rest);
        let mut head = format!(
            "GET {}{} HTTP/1.1\r\nHost: {host}\r\nOrigin: {}\r\nAuthorization: {}\r\n",
            encode_path(path),
            match query {
                "" => String::new(),
                q => format!("?{q}"),
            },
            self.url,
            sign(&self.secret(false)?, now(), "GET", path),
        );
        for (name, value) in headers {
            let lower = name.to_ascii_lowercase();
            if lower.starts_with("sec-websocket-") || lower == "upgrade" || lower == "connection" {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
        }
        head.push_str("\r\n");
        let mut upstream = Upstream::connect(host, tls)?;
        upstream
            .write_all(head.as_bytes())
            .map_err(|e| format!("could not reach {} ({e})", self.url))?;
        Ok(upstream)
    }
}

/// A refusal in the sibling's own words when it gave some.
fn refusal(answer: &Answer) -> String {
    let said = String::from_utf8_lossy(&answer.body);
    let said = said.lines().next().unwrap_or("").trim();
    match said.is_empty() || answer.content_type.starts_with("text/html") {
        true => format!("answered HTTP {}", answer.status),
        false => format!("answered {}", said.chars().take(200).collect::<String>()),
    }
}

/// `path` with everything a URL path cannot carry as-is escaped.
fn encode_path(path: &str) -> String {
    path.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The terminal link a sibling handed back, with its socket pointed at the
/// hub's relay instead: `e=wss://<sibling>/rmux-ws/…` becomes
/// `e=wss://<page host>/api/peer/<machine>/rmux-ws/…`, `ws` for a page on
/// plain http. Everything else in the link — the share's keys — is the
/// sibling's, untouched. `None` when the link has no socket shaped like that.
pub fn relayed_link(url: &str, sibling: &str, page_origin: &str, machine: &str) -> Option<String> {
    let (base, fragment) = url.split_once('#')?;
    let sibling_host = sibling.split_once("://")?.1;
    let (scheme, page_host) = match page_origin.split_once("://")? {
        ("https", rest) => ("wss", rest.split('/').next()?),
        ("http", rest) => ("ws", rest.split('/').next()?),
        _ => return None,
    };
    let mut moved = false;
    let params: Vec<String> = fragment
        .split('&')
        .map(|param| {
            let rest = param
                .strip_prefix("e=wss://")
                .or_else(|| param.strip_prefix("e=ws://"))
                .and_then(|r| r.strip_prefix(sibling_host))
                .and_then(|r| r.strip_prefix("/rmux-ws/"));
            match rest {
                Some(rest) => {
                    moved = true;
                    format!("e={scheme}://{page_host}/api/peer/{machine}/rmux-ws/{rest}")
                }
                None => param.to_string(),
            }
        })
        .collect();
    moved.then(|| format!("{base}#{}", params.join("&")))
}

/// A connection to a sibling: plain TCP for an `http://` one (a test's), TLS
/// for every real one.
pub enum Upstream {
    Plain(TcpStream),
    Tls(Box<rustls::ClientConnection>, TcpStream),
}

impl Upstream {
    fn connect(host: &str, tls: bool) -> Result<Upstream, String> {
        let address = match (host.contains(':'), tls) {
            (true, _) => host.to_string(),
            (false, true) => format!("{host}:443"),
            (false, false) => format!("{host}:80"),
        };
        let stream =
            TcpStream::connect(&address).map_err(|e| format!("could not reach {host} ({e})"))?;
        if !tls {
            return Ok(Upstream::Plain(stream));
        }
        static CONFIG: std::sync::LazyLock<Option<Arc<rustls::ClientConfig>>> =
            std::sync::LazyLock::new(|| {
                let roots = rustls::RootCertStore {
                    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
                };
                let config = rustls::ClientConfig::builder_with_provider(Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .with_safe_default_protocol_versions()
                .ok()?
                .with_root_certificates(roots)
                .with_no_client_auth();
                Some(Arc::new(config))
            });
        let config = CONFIG.clone().ok_or("TLS could not be set up")?;
        let name = rustls::pki_types::ServerName::try_from(
            host.split(':').next().unwrap_or(host).to_string(),
        )
        .map_err(|_| format!("{host} is not a name TLS can check"))?;
        let conn = rustls::ClientConnection::new(config, name).map_err(|e| e.to_string())?;
        Ok(Upstream::Tls(Box::new(conn), stream))
    }

    fn socket(&self) -> &TcpStream {
        match self {
            Upstream::Plain(s) | Upstream::Tls(_, s) => s,
        }
    }

    /// Write `data`, and for TLS whatever records are owed — the handshake's
    /// first flight included, since rustls holds plaintext written before the
    /// handshake until it is done.
    fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self {
            Upstream::Plain(s) => s.write_all(data),
            Upstream::Tls(conn, s) => {
                conn.writer().write_all(data)?;
                flush(conn, s)
            }
        }
    }

    /// After the socket polled readable: append what it carried to `out`, and
    /// say whether the far side is still open. One read of the socket, so it
    /// never blocks past what `poll` promised.
    fn read_ready(&mut self, out: &mut Vec<u8>) -> std::io::Result<bool> {
        let mut buf = [0u8; 16 * 1024];
        match self {
            Upstream::Plain(s) => {
                let n = s.read(&mut buf)?;
                out.extend_from_slice(&buf[..n]);
                Ok(n > 0)
            }
            Upstream::Tls(conn, s) => {
                if conn.read_tls(s)? == 0 {
                    return Ok(false);
                }
                conn.process_new_packets()
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                loop {
                    match conn.reader().read(&mut buf) {
                        Ok(0) => return Ok(false),
                        Ok(n) => out.extend_from_slice(&buf[..n]),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }
                // The handshake's replies, and the plaintext held behind it.
                flush(conn, s)?;
                Ok(true)
            }
        }
    }

    /// Pump bytes between `client` and this connection until either side
    /// closes. One thread and `poll(2)` rather than a thread per direction,
    /// because a TLS connection is one state machine that both directions
    /// drive and cannot be split in two.
    pub fn pump(mut self, client: &mut TcpStream) {
        // A terminal sits quiet for minutes; the request deadlines that guard
        // every other route would cut it off mid-session.
        let _ = client.set_read_timeout(None);
        let _ = client.set_write_timeout(None);
        let mut buf = [0u8; 16 * 1024];
        let mut out = Vec::new();
        loop {
            let mut fds = [
                libc::pollfd {
                    fd: client.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: self.socket().as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: two valid pollfds on fds both owned for this call.
            if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            if fds[0].revents != 0 {
                match client.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        if self.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            if fds[1].revents != 0 {
                out.clear();
                let open = self.read_ready(&mut out);
                if client.write_all(&out).is_err() || !matches!(open, Ok(true)) {
                    break;
                }
            }
        }
        let _ = client.shutdown(std::net::Shutdown::Both);
        let _ = self.socket().shutdown(std::net::Shutdown::Both);
    }
}

fn flush(conn: &mut rustls::ClientConnection, s: &mut TcpStream) -> std::io::Result<()> {
    while conn.wants_write() {
        conn.write_tls(s)?;
    }
    Ok(())
}

/// The siblings found at the last listing, for whoever has to reach one by
/// name — the hub's relay — whichever thread found them.
static KNOWN: Mutex<Vec<Arc<Peer>>> = Mutex::new(Vec::new());

/// The sibling called `machine`, if the last listing had one.
pub fn find(machine: &str) -> Option<Arc<Peer>> {
    KNOWN
        .lock()
        .ok()?
        .iter()
        .find(|p| p.machine == machine)
        .cloned()
}

/// Every sibling whose last poll failed, and why: the offline dots.
pub fn offline() -> Vec<(String, String)> {
    KNOWN
        .lock()
        .map(|known| {
            known
                .iter()
                .filter_map(|p| {
                    let why = p.last_error.lock().ok()?.clone()?;
                    Some((p.machine.clone(), why))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Stand `peers` in as the known siblings — for a test of whoever reaches one
/// by name.
#[cfg(any(test, feature = "test-support"))]
pub fn set_known(peers: Vec<Arc<Peer>>) {
    if let Ok(mut known) = KNOWN.lock() {
        *known = peers;
    }
}

/// The account's other cctops: every `cctop-*` tunnel but this machine's own,
/// at the hostname its ingress lists first — or at that one's `-link`
/// sibling when it has one, since that is the hostname outside Cloudflare
/// Access when Access fronts the page, and a server has nobody to log in.
///
/// A tunnel whose ingress cannot be read is left out rather than failing the
/// rest: one machine's oddity is not every machine's.
///
/// ponytail: a sibling whose page is behind Access with public token links
/// off has no hostname outside Access, and its poll fails at the edge with
/// the login page — shown as offline, with that reason.
fn discover(
    api: &Arc<Api>,
    account_id: &str,
    own_tunnel: Option<&str>,
    held: &[Arc<Peer>],
) -> Result<Vec<Arc<Peer>>, crate::cloudflare::Error> {
    let mut found = Vec::new();
    for tunnel in api.tunnels(account_id)? {
        let Some(machine) = tunnel.name.strip_prefix(TUNNEL_PREFIX) else {
            continue;
        };
        if Some(tunnel.id.as_str()) == own_tunnel || machine.is_empty() {
            continue;
        }
        let Ok(hosts) = api.ingress_hostnames(account_id, &tunnel.id) else {
            continue;
        };
        let Some(page) = hosts.first() else {
            continue;
        };
        let link = crate::cloudflare::link_hostname(page);
        let host = match hosts.contains(&link) {
            true => link,
            false => page.clone(),
        };
        // A token already fetched for this tunnel is kept across listings:
        // fetching it again every listing would be a call per sibling per
        // listing for an answer that only changes when someone rotates it.
        let token = held
            .iter()
            .find(|p| matches!(&p.source, Source::Api { tunnel_id, .. } if *tunnel_id == tunnel.id))
            .and_then(|p| p.token.lock().ok().and_then(|t| t.clone()));
        found.push(Arc::new(Peer {
            machine: machine.to_string(),
            url: format!("https://{host}"),
            up: tunnel.up,
            source: Source::Api {
                api: Arc::clone(api),
                account_id: account_id.to_string(),
                tunnel_id: tunnel.id.clone(),
            },
            token: Mutex::new(token),
            last_error: Mutex::new(None),
        }));
    }
    Ok(found)
}

/// Find and poll this account's other cctops on a thread of their own, for as
/// long as `report` keeps returning `true`, handing it each sibling's
/// snapshot as it lands. A sibling that leaves the account is reported once
/// more, as failed, so its rows are not left looking current.
///
/// `false` when there is nothing to find them with — no account, or one
/// connected by tunnel token alone, which holds no API token to list
/// anything — and in a test, which must never reach a real account. Once per
/// process: the dashboard and a server inside it share one poller.
pub fn spawn(mut report: impl FnMut(&str, Snapshot) -> bool + Send + 'static) -> bool {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if crate::under_test() {
        return false;
    }
    let Some(account) = crate::tunnel::account() else {
        return false;
    };
    let (Some(api_token), Some(account_id)) = (account.api_token.clone(), account.account_id)
    else {
        return false;
    };
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return false;
    }
    let own = cctop_tunnel::cloudflare::Credentials::from_token(&account.token)
        .ok()
        .map(|c| c.tunnel_id.to_string());
    let auth = match account.login {
        true => Auth::Login(api_token),
        false => Auth::Pasted(api_token),
    };
    let api = Arc::new(Api::with(&auth));
    std::thread::Builder::new()
        .name("cctop-peers".into())
        .spawn(move || {
            let mut listed: Option<Instant> = None;
            loop {
                if listed.is_none_or(|at| at.elapsed() >= REDISCOVER) {
                    let held = KNOWN.lock().map(|k| k.clone()).unwrap_or_default();
                    match discover(&api, &account_id, own.as_deref(), &held) {
                        Ok(found) => {
                            for gone in held
                                .iter()
                                .filter(|p| !found.iter().any(|f| f.machine == p.machine))
                            {
                                if !report(
                                    &gone.machine,
                                    Snapshot::Failed(
                                        "its tunnel is no longer on the account".into(),
                                    ),
                                ) {
                                    return;
                                }
                            }
                            if let Ok(mut known) = KNOWN.lock() {
                                *known = found;
                            }
                        }
                        Err(why) => crate::elog::event(
                            "peer",
                            "discover",
                            serde_json::json!({"failed": why.to_string()}),
                        ),
                    }
                    listed = Some(Instant::now());
                }
                let peers = KNOWN.lock().map(|k| k.clone()).unwrap_or_default();
                for peer in peers {
                    if !report(&peer.machine, peer.poll()) {
                        return;
                    }
                }
                std::thread::sleep(fleet::POLL);
            }
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "eyJhIjoi-made-up-tunnel-token";

    #[test]
    fn a_signature_checks_out_only_for_what_it_signed() {
        let header = sign(SECRET, 1_000, "GET", "/api/sessions");
        assert!(header.starts_with("cctop-peer 1000."));
        assert!(verify(SECRET, &header, "GET", "/api/sessions", 1_000));
        // Another path, method, or token is another signature.
        assert!(!verify(SECRET, &header, "GET", "/api/config", 1_000));
        assert!(!verify(SECRET, &header, "POST", "/api/sessions", 1_000));
        assert!(!verify(
            "eyJhIjoi-another",
            &header,
            "GET",
            "/api/sessions",
            1_000
        ));
        // A serve with no tunnel token takes no signature at all — not even
        // one made with the empty string.
        let empty = sign("", 1_000, "GET", "/api/sessions");
        assert!(!verify("", &empty, "GET", "/api/sessions", 1_000));
        for junk in [
            "",
            "cctop-peer",
            "cctop-peer 1000",
            "Bearer x",
            "cctop-peer x.y",
        ] {
            assert!(!verify(SECRET, junk, "GET", "/", 1_000), "{junk}");
        }
    }

    #[test]
    fn a_signature_is_good_for_a_minute_either_way() {
        let header = sign(SECRET, 1_000, "GET", "/api/sessions");
        assert!(verify(SECRET, &header, "GET", "/api/sessions", 1_060));
        assert!(verify(SECRET, &header, "GET", "/api/sessions", 940));
        assert!(!verify(SECRET, &header, "GET", "/api/sessions", 1_061));
        assert!(!verify(SECRET, &header, "GET", "/api/sessions", 939));
    }

    #[test]
    fn a_terminal_link_moves_its_socket_to_the_hubs_relay() {
        let url =
            "https://box.example.test/term/#e=wss://box.example.test/rmux-ws/4123/s/abc&k=key";
        assert_eq!(
            relayed_link(
                url,
                "https://box.example.test",
                "https://hub.example.test",
                "box"
            )
            .as_deref(),
            Some(
                "https://box.example.test/term/#e=wss://hub.example.test/api/peer/box/rmux-ws/4123/s/abc&k=key"
            )
        );
        // A hub page on plain http gets a plain socket.
        assert_eq!(
            relayed_link(
                url,
                "https://box.example.test",
                "http://127.0.0.1:7777",
                "box"
            )
            .as_deref(),
            Some(
                "https://box.example.test/term/#e=ws://127.0.0.1:7777/api/peer/box/rmux-ws/4123/s/abc&k=key"
            )
        );
        // A socket somewhere else is not the sibling's to relay.
        let elsewhere = "https://x/term/#e=wss://other.example.test/rmux-ws/1/s&k=key";
        assert_eq!(
            relayed_link(elsewhere, "https://box.example.test", "https://hub", "box"),
            None
        );
    }

    /// The TLS pump end to end against a real host: the request written
    /// before the handshake has to go out after it, and the answer has to
    /// come back through `poll`. Needs the network, so run by hand:
    /// `cargo test -p cctop-core peer::tests::the_tls_pump -- --ignored`.
    #[test]
    #[ignore]
    fn the_tls_pump_carries_a_whole_exchange() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut reader = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut client, _) = listener.accept().unwrap();
        let mut upstream = Upstream::connect("example.com", true).expect("connects");
        upstream
            .write_all(b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
            .unwrap();
        let pump = std::thread::spawn(move || upstream.pump(&mut client));
        let mut got = String::new();
        let _ = reader.read_to_string(&mut got);
        pump.join().unwrap();
        assert!(got.starts_with("HTTP/1.1 200"), "{got}");
        assert!(got.contains("</html>"), "the whole body came through");
    }

    #[test]
    fn discovery_finds_the_other_cctop_tunnels_and_their_page_hostnames() {
        use crate::cloudflare::fake::{api as fake_api, ok};
        use serde_json::json;
        let (base, _seen) = fake_api(|_method, path, _body| match path {
            p if p.starts_with("/accounts/acct1/cfd_tunnel?") => ok(json!([
                {"id": "t-own", "name": "cctop-here", "status": "healthy"},
                {"id": "t-box", "name": "cctop-box", "status": "healthy"},
                {"id": "t-old", "name": "cctop-old", "status": "down"},
                {"id": "t-gated", "name": "cctop-gated", "status": "degraded"},
                {"id": "t-media", "name": "media-stack", "status": "healthy"},
            ])),
            "/accounts/acct1/cfd_tunnel/t-box/configurations" => ok(json!({"config": {"ingress": [
                {"hostname": "cctop-box.example.test"},
                {"hostname": "cctop-box-share.example.test"},
                {"service": "http_status:404"},
            ]}})),
            "/accounts/acct1/cfd_tunnel/t-old/configurations" => ok(json!({"config": {"ingress": [
                {"hostname": "cctop-old.example.test"},
            ]}})),
            "/accounts/acct1/cfd_tunnel/t-gated/configurations" => {
                ok(json!({"config": {"ingress": [
                    {"hostname": "cctop-gated.example.test"},
                    {"hostname": "cctop-gated-share.example.test"},
                    {"hostname": "cctop-gated-link.example.test"},
                ]}}))
            }
            _ => (
                404,
                json!({"success": false, "errors": [{"code": 7003, "message": "no route"}]}),
            ),
        });
        let api = Arc::new(Api::fake(&base, "made-up-api-token"));
        let found = discover(&api, "acct1", Some("t-own"), &[]).expect("lists");
        let seen: Vec<(&str, &str, bool)> = found
            .iter()
            .map(|p| (p.machine.as_str(), p.url.as_str(), p.up))
            .collect();
        assert_eq!(
            seen,
            [
                ("box", "https://cctop-box.example.test", true),
                ("old", "https://cctop-old.example.test", false),
                // Access fronts its page, so the hostname outside it.
                ("gated", "https://cctop-gated-link.example.test", true),
            ]
        );
        // A tunnel with no connector is offline without a request being made.
        assert!(matches!(found[1].poll(), Snapshot::Failed(why) if why.starts_with("offline")));
    }
}
