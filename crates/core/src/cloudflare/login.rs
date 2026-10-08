//! Connecting by logging in through the browser, the way `cloudflared tunnel
//! login` does — nothing to make, nothing to paste.
//!
//! The flow, as cloudflared's source has it (`cmd/cloudflared/tunnel/login.go`
//! and `token/transfer.go`):
//!
//! 1. A random key is made, and the browser is sent to
//!    `https://dash.cloudflare.com/argotunnel?aud=&callback=<store><key>`,
//!    where the user logs in and picks a domain.
//! 2. Meanwhile `GET <store><key>` is polled, the store being
//!    `https://login.cloudflareaccess.org/`. Anything but a 200 or a 5xx means
//!    "not yet"; a 5xx is a failure; a 200 carries the certificate.
//! 3. The certificate, `cert.pem` to cloudflared, is PEM. Of its blocks only
//!    `ARGO TUNNEL TOKEN` matters: JSON with `zoneID`, `accountID` and
//!    `apiToken` (`credentials/origin_cert.go`). The private key and
//!    certificate blocks beside it are legacy, and cloudflared ignores them too.
//!
//! cloudflared's key is a NaCl public key, but for this resource it asks for
//! no encryption, so the key is only ever a name in a URL; 32 random bytes in
//! the same URL-safe base64 are the same thing to the store.
//!
//! What the token can do is less than a pasted one can. Cloudflare documents
//! it as managing the account's tunnels; cloudflared uses it to create a
//! tunnel, fetch its token, and route a hostname through
//! `PUT /zones/<zone>/tunnels/<id>/routes`. [`super::Api`] keeps to those
//! calls for it, and treats anything else it tries — reading the zone's name,
//! the ingress list the dashboard shows, finding a record to delete — as a
//! nicety that may be refused.
//!
//! Over ssh the browser is not on this machine, so the URL is printed (or
//! drawn, with a QR code) and the user opens it wherever they are: the store
//! is polled from here either way, which is why cloudflared works over ssh.
//!
//! Only the token is kept, in `config.toml`'s `[tunnel]` table, owner-only like
//! the pasted ones and removed with it. The certificate itself is never written
//! to disk or logged; the URL carries the key that fetches it, so it is shown
//! to the user and nowhere else.

use std::fmt;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{Auth, Error, percent_encode};

/// The dashboard's authorize page, cloudflared's `baseLoginURL`.
pub const LOGIN_URL: &str = "https://dash.cloudflare.com/argotunnel";

/// Where the certificate is fetched from, cloudflared's `callbackURL`.
pub const CALLBACK_STORE: &str = "https://login.cloudflareaccess.org/";

/// One poll's limit, cloudflared's `clientTimeout`: the store holds a request
/// open while it waits, so a poll is slow by design.
const POLL_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the user altogether: cloudflared's ten polls of up to
/// a minute. Measured as time rather than counted as polls, so a store that
/// answers "not yet" at once does not use them up in seconds.
const WAIT: Duration = Duration::from_secs(600);

/// The least time between two polls, for a store that answers at once.
const GAP: Duration = Duration::from_secs(2);

/// What a removed login leaves on Cloudflare: the token it made, which only
/// the dashboard can revoke — a token cannot delete itself, and Cloudflare's
/// documentation sends cloudflared users to the same page.
pub const LEFT: &str =
    "the API token the browser login made: revoke it at dash.cloudflare.com/profile/api-tokens";

/// The largest certificate read; cloudflared's are about 2 KB.
const CERT_MAX: u64 = 64 * 1024;

/// Where the login goes: Cloudflare, except in a build made for tests, where
/// `CCTOP_CLOUDFLARE_LOGIN` can name a [`fake`] server serving both halves.
/// Absent from a release build, since a certificate fetched from a store of
/// someone's choosing is one they wrote.
fn endpoints() -> (String, String) {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(base) = std::env::var("CCTOP_CLOUDFLARE_LOGIN")
        .ok()
        .filter(|b| !b.is_empty())
    {
        let base = base.trim_end_matches('/');
        return (format!("{base}/argotunnel"), format!("{base}/callback/"));
    }
    (LOGIN_URL.to_string(), CALLBACK_STORE.to_string())
}

/// A login under way: the URL to open, and the store to poll.
pub struct Login {
    url: String,
    store: String,
}

/// Neither field: both carry the key that fetches the certificate.
impl fmt::Debug for Login {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Login").finish_non_exhaustive()
    }
}

impl Login {
    /// A login against Cloudflare, with a fresh key.
    #[allow(clippy::new_without_default)] // each is a new key, not a default
    pub fn new() -> Login {
        let (login, store) = endpoints();
        Login::at(&login, &store)
    }

    /// Against another authorize page and store: the [`fake`] in tests.
    pub(crate) fn at(login: &str, store: &str) -> Login {
        let key: String = crate::util::b64_encode(&crate::util::random_bytes(32))
            .chars()
            .map(|c| match c {
                '+' => '-',
                '/' => '_',
                c => c,
            })
            .collect();
        let store = format!("{store}{key}");
        // cloudflared's query, in the order Go's `url.Values.Encode` sorts it.
        let url = format!("{login}?aud=&callback={}", percent_encode(&store));
        Login { url, store }
    }

    /// Against a [`fake`] server at `base`, for another crate's tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn fake(base: &str) -> Login {
        Login::at(&format!("{base}/argotunnel"), &format!("{base}/callback/"))
    }

    /// The page to open in a browser. Shown to the user and nowhere else.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Wait for the user to log in and pick a domain, and return what the
    /// certificate holds. `cancelled` is asked between polls; a poll in flight
    /// is left to finish on its own.
    pub fn wait(&self, cancelled: &dyn Fn() -> bool) -> Result<OriginCert, Error> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(POLL_TIMEOUT))
            .http_status_as_error(false)
            .user_agent(concat!("cctop/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        let deadline = Instant::now() + WAIT;
        loop {
            if cancelled() {
                return Err(Error::Login("The login was cancelled.".to_string()));
            }
            let started = Instant::now();
            match agent.get(&self.store).call() {
                // A held poll that ran out is the store still waiting.
                Err(ureq::Error::Timeout(_)) => {}
                Err(e) => {
                    return Err(Error::Login(format!(
                        "Could not reach Cloudflare's login service ({e})."
                    )));
                }
                Ok(mut response) => {
                    let status = response.status().as_u16();
                    if status >= 500 {
                        return Err(Error::Login(format!(
                            "Cloudflare's login service answered HTTP {status}; try again."
                        )));
                    }
                    if status == 200 {
                        let body = response
                            .body_mut()
                            .with_config()
                            .limit(CERT_MAX)
                            .read_to_vec()
                            .map_err(|e| {
                                Error::Login(format!("The certificate did not arrive whole ({e})."))
                            })?;
                        return decode(&body);
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(Error::Login(
                    "No login arrived within ten minutes; start again.".to_string(),
                ));
            }
            while started.elapsed() < GAP {
                if cancelled() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// What a login's certificate holds. Its `Debug` prints no token.
#[derive(Clone, PartialEq, Eq)]
pub struct OriginCert {
    /// The domain picked in the browser.
    pub zone_id: String,
    pub account_id: String,
    token: String,
}

impl fmt::Debug for OriginCert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OriginCert")
            .field("zone_id", &self.zone_id)
            .field("account_id", &self.account_id)
            .finish_non_exhaustive()
    }
}

impl OriginCert {
    /// The token, for an [`super::Api`] that knows where it came from.
    pub fn auth(&self) -> Auth {
        Auth::Login(self.token.clone())
    }
}

/// Read the certificate the way cloudflared's `decodeOriginCert` does: the
/// legacy key and certificate blocks are passed over, any other block is
/// refused, and there must be exactly one token with a zone and a token in
/// it. cctop needs the account too, to create the tunnel on.
pub fn decode(pem: &[u8]) -> Result<OriginCert, Error> {
    let bad = |why: &str| Error::Login(format!("The certificate Cloudflare sent {why}."));
    let text = std::str::from_utf8(pem).map_err(|_| bad("is not text"))?;
    let mut token: Option<Value> = None;
    let mut block: Option<(String, String)> = None;
    for line in text.lines().map(str::trim) {
        if let Some(kind) = line
            .strip_prefix("-----BEGIN ")
            .and_then(|l| l.strip_suffix("-----"))
        {
            block = Some((kind.to_string(), String::new()));
        } else if let Some(kind) = line
            .strip_prefix("-----END ")
            .and_then(|l| l.strip_suffix("-----"))
        {
            let Some((open, body)) = block.take().filter(|(open, _)| open == kind) else {
                return Err(bad("is not well-formed PEM"));
            };
            match open.as_str() {
                "PRIVATE KEY" | "CERTIFICATE" => {}
                "ARGO TUNNEL TOKEN" => {
                    if token.is_some() {
                        return Err(bad("holds more than one token"));
                    }
                    let json = crate::util::b64_decode(&body).ok_or_else(|| bad("is garbled"))?;
                    token = Some(serde_json::from_slice(&json).map_err(|_| bad("is garbled"))?);
                }
                other => return Err(bad(&format!("has an unknown block ({other})"))),
            }
        } else if let Some((_, body)) = block.as_mut() {
            // PEM headers (`Key: value`) are not base64, and cloudflared
            // writes none; skipped all the same.
            if !line.contains(':') {
                body.push_str(line);
            }
        }
    }
    let token = token.ok_or_else(|| bad("holds no token"))?;
    let field = |key: &str| {
        token[key]
            .as_str()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
    };
    match (field("zoneID"), field("accountID"), field("apiToken")) {
        (Some(zone_id), Some(account_id), Some(token)) => Ok(OriginCert {
            zone_id,
            account_id,
            token,
        }),
        _ => Err(bad("is missing its zone, account or token")),
    }
}

/// A stand-in for both halves of the login on loopback: the authorize page and
/// the store. Opening the page counts as logging in and picking the domain
/// `zone1` on `acct1`, so a test "browser" is one GET; until then the store
/// answers 404, as the real one does. The token in the certificate is made up,
/// and the [`super::fake`] API accepts it.
#[cfg(any(test, feature = "test-support"))]
pub mod fake {
    use std::collections::HashSet;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// The token inside the fake certificate.
    pub const TOKEN: &str = "made-up-login-token";

    /// How the fake store behaves.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Store {
        /// 404 until the page is opened, then the certificate.
        Working,
        /// 500 to every poll.
        Broken,
    }

    /// A certificate in cloudflared's layout, for `zone`, `account` and
    /// `token`: the legacy blocks around the one that matters.
    pub fn cert(zone: &str, account: &str, token: &str) -> String {
        let json = serde_json::json!({"zoneID": zone, "accountID": account, "apiToken": token});
        let body = crate::util::b64_encode(json.to_string().as_bytes());
        let lines: Vec<&str> = body
            .as_bytes()
            .chunks(64)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        format!(
            "-----BEGIN PRIVATE KEY-----\nbWFkZS11cA==\n-----END PRIVATE KEY-----\n\
             -----BEGIN CERTIFICATE-----\nbWFkZS11cA==\n-----END CERTIFICATE-----\n\
             -----BEGIN ARGO TUNNEL TOKEN-----\n{}\n-----END ARGO TUNNEL TOKEN-----\n",
            lines.join("\n")
        )
    }

    /// Start one; returns its base, which `CCTOP_CLOUDFLARE_LOGIN` or
    /// [`super::Login::at`] (with `/argotunnel` and `/callback/`) takes.
    pub fn server(store: Store) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let authorized: Arc<Mutex<HashSet<String>>> = Arc::default();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let path = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                        break;
                    }
                }
                let (status, body) = if let Some(query) = path.strip_prefix("/argotunnel?") {
                    // The page: the key is the callback's last segment.
                    let callback = query
                        .split('&')
                        .find_map(|kv| kv.strip_prefix("callback="))
                        .map(percent_decode)
                        .unwrap_or_default();
                    let key = callback.rsplit('/').next().unwrap_or_default().to_string();
                    let has_aud = query.split('&').any(|kv| kv == "aud=");
                    match key.is_empty() || !has_aud {
                        true => (400, "no callback".to_string()),
                        false => {
                            authorized.lock().unwrap().insert(key);
                            (200, "Logged in to a made-up Cloudflare.".to_string())
                        }
                    }
                } else if let Some(key) = path.strip_prefix("/callback/") {
                    match (store, authorized.lock().unwrap().remove(key)) {
                        (Store::Broken, _) => (500, "made-up failure".to_string()),
                        (Store::Working, true) => (200, cert("zone1", "acct1", TOKEN)),
                        (Store::Working, false) => (404, String::new()),
                    }
                } else {
                    (404, String::new())
                };
                let mut stream = reader.into_inner();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        base
    }

    fn percent_decode(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'%' if i + 2 < bytes.len() => {
                    let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                    out.push(u8::from_str_radix(hex, 16).unwrap_or(b'?'));
                    i += 3;
                }
                b => {
                    out.push(b);
                    i += 1;
                }
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// "The user logged in": what a browser opening `url` would do.
    pub fn open(url: &str) {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let status = agent.get(url).call().unwrap().status().as_u16();
        assert_eq!(status, 200, "the fake authorize page refused {url}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(base: &str) -> Login {
        Login::at(&format!("{base}/argotunnel"), &format!("{base}/callback/"))
    }

    #[test]
    fn the_url_is_cloudflareds() {
        let login = Login::at(LOGIN_URL, CALLBACK_STORE);
        let url = login.url();
        let prefix = "https://dash.cloudflare.com/argotunnel?aud=&callback=\
                      https%3A%2F%2Flogin.cloudflareaccess.org%2F";
        assert!(url.starts_with(prefix), "{url}");
        // The key: 32 bytes of URL-safe base64, its padding escaped.
        let key = &url[prefix.len()..];
        assert!(key.ends_with("%3D"), "{key}");
        let key = key.trim_end_matches("%3D");
        assert_eq!(key.len(), 43, "{key}");
        assert!(
            key.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{key}"
        );
        assert_eq!(
            login.store,
            format!(
                "{CALLBACK_STORE}{}",
                login.store.rsplit('/').next().unwrap()
            )
        );
        assert_ne!(
            Login::at(LOGIN_URL, CALLBACK_STORE).url(),
            url,
            "a fresh key each"
        );
        assert!(!format!("{login:?}").contains("cloudflareaccess"));
    }

    #[test]
    fn the_certificate_arrives_once_the_page_is_opened() {
        let base = fake::server(fake::Store::Working);
        let login = login(&base);
        let url = login.url().to_string();
        // The browser, a moment later; the polls before it are told "not yet".
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            fake::open(&url);
        });
        let cert = login.wait(&|| false).expect("a certificate");
        assert_eq!(cert.zone_id, "zone1");
        assert_eq!(cert.account_id, "acct1");
        assert_eq!(cert.auth(), Auth::Login(fake::TOKEN.to_string()));
        assert!(!format!("{cert:?}").contains(fake::TOKEN));
    }

    #[test]
    fn a_broken_store_and_a_cancel_each_end_the_wait() {
        let base = fake::server(fake::Store::Broken);
        let err = login(&base).wait(&|| false).unwrap_err();
        assert!(err.to_string().contains("HTTP 500"), "{err}");

        let base = fake::server(fake::Store::Working);
        let err = login(&base).wait(&|| true).unwrap_err();
        assert!(err.to_string().contains("cancelled"), "{err}");
    }

    #[test]
    fn a_certificate_is_read_as_cloudflared_reads_it() {
        let cert = decode(fake::cert("z", "a", "made-up").as_bytes()).unwrap();
        assert_eq!(
            (cert.zone_id.as_str(), cert.account_id.as_str()),
            ("z", "a")
        );
        assert_eq!(cert.auth(), Auth::Login("made-up".into()));

        let missing = decode(fake::cert("", "a", "made-up").as_bytes()).unwrap_err();
        assert!(missing.to_string().contains("missing"), "{missing}");
        let twice = format!(
            "{}{}",
            fake::cert("z", "a", "made-up"),
            fake::cert("z", "a", "made-up")
        );
        assert!(decode(twice.as_bytes()).is_err());
        let unknown = "-----BEGIN SOMETHING-----\nAA==\n-----END SOMETHING-----\n";
        assert!(decode(unknown.as_bytes()).is_err());
        assert!(decode(b"").is_err());
        // No error repeats the token.
        let garbled = fake::cert("z", "a", "made-up").replace("-----END ARGO", "-----END ARGH");
        let err = decode(garbled.as_bytes()).unwrap_err().to_string();
        assert!(!err.contains("made-up"), "{err}");
    }
}
