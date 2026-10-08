//! Connecting a Cloudflare account: what `cctop tunnel setup` and `remove` do.
//!
//! The user pastes one of two things, told apart by shape:
//!
//! - an **API token**, made from a pre-filled link with three permissions.
//!   cctop then lists their domains, creates a tunnel, points it at a hostname
//!   and writes the DNS records itself — one paste and picking a domain;
//! - a **tunnel token**, the string the dashboard shows for a tunnel made
//!   there. Nothing is created: the token is stored, and the hostname comes
//!   from the configuration the edge pushes when the tunnel first connects.
//!
//! The one thing cctop cannot do for anyone is the domain. A named tunnel is
//! reached through a DNS record, so it needs a domain whose DNS is on
//! Cloudflare, and the setup finds out early and says so plainly rather than
//! failing at the last call.
//!
//! Everything here is blocking: the command line calls it directly, and a
//! screen calls it off its UI thread. No call writes to stdout or stderr, and
//! no error carries a token — Cloudflare's own messages are passed on, and
//! they never quote the credential.
//!
//! Only what cctop created is ever deleted, by the ids it stored when it
//! created them; a DNS record cctop did not make is never overwritten.

use std::fmt;
use std::time::Duration;

use serde_json::{Value, json};

use crate::tunnel::Account;

/// Cloudflare's API.
const API_BASE: &str = "https://api.cloudflare.com/client/v4";

const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

/// The three permissions a setup token needs, as the dashboard names them.
pub const PERMISSIONS: [&str; 3] = [
    "Account · Cloudflare Tunnel · Edit",
    "Zone · DNS · Edit",
    "Zone · Zone · Read",
];

/// The dashboard's create-token page with those permissions and the name
/// `cctop` filled in, through the documented template parameters
/// (`permissionGroupKeys`, `accountId`, `zoneId`, `name`).
///
/// ponytail: `argotunnel` is the Tunnel permission's key as far as could be
/// found; the documentation's table does not list it. If the page opens
/// without it, the three permissions printed beside the link still say what
/// to tick.
pub fn token_link() -> String {
    let keys = r#"[{"key":"argotunnel","type":"edit"},{"key":"dns","type":"edit"},{"key":"zone","type":"read"}]"#;
    format!(
        "https://dash.cloudflare.com/profile/api-tokens?permissionGroupKeys={}&accountId=*&zoneId=all&name=cctop",
        percent_encode(keys)
    )
}

/// Where a domain is added to Cloudflare.
pub const ADD_SITE_LINK: &str = "https://dash.cloudflare.com/?to=/:account/add-site";

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// What was pasted.
#[derive(Clone, PartialEq, Eq)]
pub enum Pasted {
    Api(String),
    Tunnel(String),
}

impl fmt::Debug for Pasted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pasted::Api(_) => f.write_str("Pasted::Api([redacted])"),
            Pasted::Tunnel(_) => f.write_str("Pasted::Tunnel([redacted])"),
        }
    }
}

/// Tell the two kinds of token apart by shape: a tunnel token is base64 of
/// JSON and starts `eyJhIjoi`; anything else is taken for an API token.
pub fn classify(pasted: &str) -> Pasted {
    let pasted = pasted.trim().to_string();
    match cctop_tunnel::cloudflare::looks_like_tunnel_token(&pasted) {
        true => Pasted::Tunnel(pasted),
        false => Pasted::Api(pasted),
    }
}

/// Whether a pasted tunnel token decodes, or why not in the user's words —
/// never repeating it.
pub fn check_tunnel_token(token: &str) -> Result<(), String> {
    cctop_tunnel::cloudflare::Credentials::from_token(token)
        .map(drop)
        .map_err(|e| e.to_string())
}

/// Why a step did not work, each with the sentence that says what to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The API token did not authenticate.
    TokenRefused,
    /// It authenticated, and one call was refused for want of `permission`.
    MissingPermission(&'static str),
    /// No zone at all on the account.
    NoDomain,
    /// Zones, but none active: their nameservers have not moved yet.
    ZonePending(String),
    /// A hostname that is not exactly one label under the zone.
    Hostname(String),
    /// Network trouble, or Cloudflare said no for its own reason.
    Api(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TokenRefused => write!(
                f,
                "That token was refused: check that all of it was pasted, and that it \
                 has not expired or been deleted."
            ),
            Error::MissingPermission(permission) => write!(
                f,
                "That token lacks the permission \"{permission}\". Make one with all \
                 three from {}",
                token_link()
            ),
            Error::NoDomain => write!(
                f,
                "A stable tunnel needs a domain whose DNS is on Cloudflare, and this \
                 account has none. Add one at {ADD_SITE_LINK} (a cheap domain works; \
                 DNS on Cloudflare is free), then run this again."
            ),
            Error::ZonePending(zone) => write!(
                f,
                "{zone} is on Cloudflare but not active yet: its nameservers have not \
                 been switched to Cloudflare's. Once they are, run this again."
            ),
            Error::Hostname(why) => f.write_str(why),
            Error::Api(message) => write!(f, "Cloudflare said: {message}"),
        }
    }
}

impl std::error::Error for Error {}

/// A domain on the account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    pub id: String,
    pub name: String,
    pub account_id: String,
    pub active: bool,
}

/// An authenticated client for Cloudflare's API. Its `Debug` prints no token.
pub struct Api {
    base: String,
    token: String,
    agent: ureq::Agent,
}

impl fmt::Debug for Api {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Api")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl Api {
    pub fn new(token: &str) -> Api {
        Api::at(API_BASE, token)
    }

    /// Against another base URL: a fake API in tests. Private to the crate,
    /// since a token sent to a base of someone's choosing is a token given
    /// away.
    pub(crate) fn at(base: &str, token: &str) -> Api {
        Api {
            base: base.trim_end_matches('/').to_string(),
            token: token.trim().to_string(),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(HTTP_TIMEOUT))
                // The status is read here: 401 and 403 mean different things.
                .http_status_as_error(false)
                .build()
                .into(),
        }
    }

    /// One call, with the permission it needs named for a 403. Returns the
    /// envelope's `result`.
    fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        needs: &'static str,
    ) -> Result<Value, Error> {
        let url = format!("{}{path}", self.base);
        let auth = format!("Bearer {}", self.token);
        let response = match (method, body) {
            ("GET", _) => self.agent.get(&url).header("Authorization", &auth).call(),
            ("DELETE", _) => self
                .agent
                .delete(&url)
                .header("Authorization", &auth)
                .call(),
            (method, body) => {
                let body = body.unwrap_or(Value::Null).to_string();
                let request = match method {
                    "POST" => self.agent.post(&url),
                    _ => self.agent.put(&url),
                };
                request
                    .header("Authorization", &auth)
                    .content_type("application/json")
                    .send(body)
            }
        };
        let mut response = response.map_err(|e| Error::Api(format!("could not reach it ({e})")))?;
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().unwrap_or_default();
        let envelope: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let messages: Vec<String> = envelope["errors"]
            .as_array()
            .map(|errors| {
                errors
                    .iter()
                    .filter_map(|e| e["message"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        match status {
            401 => return Err(Error::TokenRefused),
            403 => return Err(Error::MissingPermission(needs)),
            _ => {}
        }
        // Cloudflare answers a token it cannot authenticate with 400 and code
        // 1000 ("Invalid API Token") on some routes rather than a 401.
        let invalid_token = envelope["errors"].as_array().is_some_and(|errors| {
            errors
                .iter()
                .any(|e| matches!(e["code"].as_i64(), Some(1000 | 6003 | 6111 | 9109)))
        });
        if invalid_token {
            return Err(Error::TokenRefused);
        }
        if !(200..300).contains(&status) || envelope["success"] == Value::Bool(false) {
            return Err(Error::Api(match messages.is_empty() {
                true => format!("HTTP {status}"),
                false => messages.join("; "),
            }));
        }
        Ok(envelope["result"].clone())
    }

    /// Whether the token authenticates at all.
    pub fn verify(&self) -> Result<(), Error> {
        let result = self.call("GET", "/user/tokens/verify", None, PERMISSIONS[2])?;
        match result["status"].as_str() {
            Some("active") | None => Ok(()),
            Some(_) => Err(Error::TokenRefused),
        }
    }

    /// Every zone the token can read, active or not.
    pub fn zones(&self) -> Result<Vec<Zone>, Error> {
        let result = self.call("GET", "/zones?per_page=50", None, PERMISSIONS[2])?;
        Ok(result
            .as_array()
            .map(|zones| {
                zones
                    .iter()
                    .filter_map(|z| {
                        Some(Zone {
                            id: z["id"].as_str()?.to_string(),
                            name: z["name"].as_str()?.to_string(),
                            account_id: z["account"]["id"].as_str()?.to_string(),
                            active: z["status"].as_str() == Some("active"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Whether `name` already has a DNS record in `zone`.
    pub fn record_exists(&self, zone: &Zone, name: &str) -> Result<bool, Error> {
        let path = format!(
            "/zones/{}/dns_records?name={}",
            zone.id,
            percent_encode(name)
        );
        let result = self.call("GET", &path, None, PERMISSIONS[1])?;
        Ok(result.as_array().is_some_and(|r| !r.is_empty()))
    }

    fn create_tunnel(&self, account_id: &str, name: &str) -> Result<String, Error> {
        let result = self.call(
            "POST",
            &format!("/accounts/{account_id}/cfd_tunnel"),
            Some(json!({"name": name, "config_src": "cloudflare"})),
            PERMISSIONS[0],
        )?;
        result["id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| Error::Api("the new tunnel came back without an id".into()))
    }

    fn tunnel_token(&self, account_id: &str, tunnel_id: &str) -> Result<String, Error> {
        let result = self.call(
            "GET",
            &format!("/accounts/{account_id}/cfd_tunnel/{tunnel_id}/token"),
            None,
            PERMISSIONS[0],
        )?;
        result
            .as_str()
            .map(String::from)
            .ok_or_else(|| Error::Api("the tunnel's token did not come back".into()))
    }

    /// The tunnel's remote ingress rules. cctop routes by its own table and
    /// ignores the `service`s, but the dashboard shows these, so they say
    /// something true: the page's usual port.
    fn configure(
        &self,
        account_id: &str,
        tunnel_id: &str,
        hostnames: &[&str],
    ) -> Result<(), Error> {
        let mut ingress: Vec<Value> = hostnames
            .iter()
            .map(|h| json!({"hostname": h, "service": "http://localhost:7777"}))
            .collect();
        ingress.push(json!({"service": "http_status:404"}));
        self.call(
            "PUT",
            &format!("/accounts/{account_id}/cfd_tunnel/{tunnel_id}/configurations"),
            Some(json!({"config": {"ingress": ingress}})),
            PERMISSIONS[0],
        )?;
        Ok(())
    }

    fn add_cname(&self, zone: &Zone, name: &str, tunnel_id: &str) -> Result<String, Error> {
        let result = self.call(
            "POST",
            &format!("/zones/{}/dns_records", zone.id),
            Some(json!({
                "type": "CNAME",
                "name": name,
                "content": format!("{tunnel_id}.cfargotunnel.com"),
                "proxied": true,
                "comment": "cctop tunnel; removed by `cctop tunnel remove`",
            })),
            PERMISSIONS[1],
        )?;
        result["id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| Error::Api("the DNS record came back without an id".into()))
    }

    fn delete_record(&self, zone_id: &str, record_id: &str) -> Result<(), Error> {
        self.call(
            "DELETE",
            &format!("/zones/{zone_id}/dns_records/{record_id}"),
            None,
            PERMISSIONS[1],
        )
        .map(drop)
    }

    fn delete_tunnel(&self, account_id: &str, tunnel_id: &str) -> Result<(), Error> {
        self.call(
            "DELETE",
            &format!("/accounts/{account_id}/cfd_tunnel/{tunnel_id}"),
            None,
            PERMISSIONS[0],
        )
        .map(drop)
    }
}

/// The zones a tunnel can go on, or why there are none. Pending zones are
/// named when they are all there is, since that is the one thing in the way.
pub fn usable(zones: Vec<Zone>) -> Result<Vec<Zone>, Error> {
    if zones.is_empty() {
        return Err(Error::NoDomain);
    }
    let pending = zones.iter().find(|z| !z.active).map(|z| z.name.clone());
    let active: Vec<Zone> = zones.into_iter().filter(|z| z.active).collect();
    match (active.is_empty(), pending) {
        (true, Some(zone)) => Err(Error::ZonePending(zone)),
        (true, None) => Err(Error::NoDomain),
        (false, _) => Ok(active),
    }
}

/// This machine's name as a DNS label: lowercase letters, digits and `-`.
pub fn machine_label() -> String {
    let raw = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    label_of(&raw)
}

fn label_of(raw: &str) -> String {
    let first = raw
        .trim()
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let label: String = first
        .chars()
        .map(|c| match c.is_ascii_alphanumeric() {
            true => c,
            false => '-',
        })
        .collect();
    let label: String = label.trim_matches('-').chars().take(30).collect();
    match label.trim_end_matches('-') {
        "" => "machine".to_string(),
        label => label.to_string(),
    }
}

/// Refuse a hostname that is not exactly one label under `zone`. Free
/// Universal SSL covers `*.example.com` and not `a.b.example.com`, so a deeper
/// name would come up with a certificate error nobody could explain.
pub fn check_hostname(hostname: &str, zone: &Zone) -> Result<String, Error> {
    let hostname = hostname.trim().trim_end_matches('.').to_ascii_lowercase();
    let suffix = format!(".{}", zone.name);
    let Some(label) = hostname.strip_suffix(&suffix) else {
        return Err(Error::Hostname(format!(
            "{hostname} is not under {}; pick a name like cctop.{}",
            zone.name, zone.name
        )));
    };
    if label.contains('.') {
        return Err(Error::Hostname(format!(
            "{hostname} is two levels under {zone}: Cloudflare's free certificate covers \
             one (like cctop.{zone}), not more. Pick a single label.",
            zone = zone.name
        )));
    }
    let valid = !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    match valid {
        true => Ok(hostname),
        false => Err(Error::Hostname(format!(
            "{label} is not a usable DNS label: letters, digits and - only"
        ))),
    }
}

/// The name to offer: `cctop.<zone>`, or — when another machine has that one
/// — `cctop-<machine>.<zone>`, then the same with a number, never a name that
/// already has a record. The share hostname beside it must be free too.
pub fn suggest_hostname(api: &Api, zone: &Zone, machine: &str) -> Result<String, Error> {
    let candidates = std::iter::once("cctop".to_string())
        .chain(std::iter::once(format!("cctop-{machine}")))
        .chain((2..20).map(|n| format!("cctop-{machine}-{n}")));
    for label in candidates {
        let name = format!("{label}.{}", zone.name);
        if !api.record_exists(zone, &name)? && !api.record_exists(zone, &share_hostname(&name))? {
            return Ok(name);
        }
    }
    Err(Error::Hostname(format!(
        "every name cctop would suggest in {} is taken; type one",
        zone.name
    )))
}

/// The hostname `W` shares are served on, beside `hostname` on the same
/// tunnel: `cctop.example.com` → `cctop-share.example.com`. One label, like
/// the page's, for the certificate's sake.
pub fn share_hostname(hostname: &str) -> String {
    match hostname.split_once('.') {
        Some((label, zone)) => format!("{label}-share.{zone}"),
        None => format!("{hostname}-share"),
    }
}

/// Create the tunnel, its configuration and its DNS records, and return the
/// account to store. Nothing is left behind on failure: whatever was made
/// before the failing call is deleted again, so a retry starts clean.
pub fn create(api: &Api, zone: &Zone, hostname: &str, machine: &str) -> Result<Account, Error> {
    let hostname = check_hostname(hostname, zone)?;
    let share = share_hostname(&hostname);
    for name in [&hostname, &share] {
        if api.record_exists(zone, name)? {
            return Err(Error::Hostname(format!(
                "{name} already has a DNS record that cctop did not make, and it will \
                 not be overwritten. Pick another name."
            )));
        }
    }
    let tunnel_id = api.create_tunnel(&zone.account_id, &format!("cctop-{machine}"))?;
    let mut records: Vec<String> = Vec::new();
    let built = (|| {
        let token = api.tunnel_token(&zone.account_id, &tunnel_id)?;
        api.configure(&zone.account_id, &tunnel_id, &[&hostname, &share])?;
        for name in [&hostname, &share] {
            records.push(api.add_cname(zone, name, &tunnel_id)?);
        }
        Ok(token)
    })();
    let token = match built {
        Ok(token) => token,
        Err(e) => {
            // Best effort, newest first; the error worth reporting is the one
            // that stopped the setup.
            for record in records.iter().rev() {
                let _ = api.delete_record(&zone.id, record);
            }
            let _ = api.delete_tunnel(&zone.account_id, &tunnel_id);
            return Err(e);
        }
    };
    Ok(Account {
        token,
        hostname: Some(hostname),
        share_hostname: Some(share),
        account_id: Some(zone.account_id.clone()),
        zone_id: Some(zone.id.clone()),
        tunnel_id: Some(tunnel_id),
        dns_record_ids: records,
        api_token: Some(api.token.clone()),
        from_env: false,
    })
}

/// What `remove` could not delete, for the user to delete by hand.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Leftovers(pub Vec<String>);

/// Delete what [`create`] made — the DNS records by id, then the tunnel —
/// and nothing else. A tunnel stored from a pasted tunnel token was made in
/// the dashboard, so it is left there and only forgotten.
pub fn remove(account: &Account) -> Leftovers {
    remove_with(account, Api::new)
}

pub(crate) fn remove_with(account: &Account, api: impl Fn(&str) -> Api) -> Leftovers {
    let mut left = Vec::new();
    let (Some(token), Some(account_id), Some(tunnel_id)) =
        (&account.api_token, &account.account_id, &account.tunnel_id)
    else {
        return Leftovers(left);
    };
    let api = api(token);
    if let Some(zone_id) = &account.zone_id {
        for record in &account.dns_record_ids {
            if let Err(e) = api.delete_record(zone_id, record) {
                left.push(format!("the DNS record {record} ({e})"));
            }
        }
    }
    if let Err(e) = api.delete_tunnel(account_id, tunnel_id) {
        left.push(format!("the tunnel {tunnel_id} ({e})"));
    }
    Leftovers(left)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// One call the fake API saw: method, path, body.
    type Seen = Arc<Mutex<Vec<(String, String, String)>>>;

    /// A stand-in for api.cloudflare.com on loopback. `answer` maps a call to
    /// a status and the JSON body; every call is recorded. Every token the
    /// tests send it is made up.
    fn fake_api(
        answer: impl Fn(&str, &str, &str) -> (u16, Value) + Send + Sync + 'static,
    ) -> (String, Seen) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen: Seen = Arc::default();
        let log = seen.clone();
        let answer = Arc::new(answer);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();
                let mut length = 0;
                let mut authorized = false;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                        break;
                    }
                    let lower = header.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    if lower.starts_with("authorization: bearer made-up") {
                        authorized = true;
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                let body = String::from_utf8_lossy(&body).into_owned();
                log.lock()
                    .unwrap()
                    .push((method.clone(), path.clone(), body.clone()));
                let (status, json) = match authorized {
                    true => answer(&method, &path, &body),
                    false => (
                        401,
                        json!({"success": false, "errors": [{"code": 10000, "message": "Authentication error"}]}),
                    ),
                };
                let text = json.to_string();
                let mut stream = reader.into_inner();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        (base, seen)
    }

    fn ok(result: Value) -> (u16, Value) {
        (
            200,
            json!({"success": true, "errors": [], "result": result}),
        )
    }

    fn zone() -> Zone {
        Zone {
            id: "zone1".into(),
            name: "example.test".into(),
            account_id: "acct1".into(),
            active: true,
        }
    }

    const TUNNEL_ID: &str = "6ff42ae2-765d-4adf-8112-31c55c1551ef";

    /// A Cloudflare that has nothing in the zone and accepts every creation,
    /// except `fail_on`, a path prefix answered 500.
    fn cloudflare(fail_on: Option<&'static str>) -> impl Fn(&str, &str, &str) -> (u16, Value) {
        move |method: &str, path: &str, _body: &str| {
            if let Some(prefix) = fail_on
                && path.starts_with(prefix)
                && method == "POST"
            {
                return (
                    500,
                    json!({"success": false, "errors": [{"code": 1, "message": "made-up failure"}]}),
                );
            }
            match (method, path) {
                ("GET", "/user/tokens/verify") => ok(json!({"status": "active"})),
                ("GET", p) if p.starts_with("/zones?") => ok(json!([
                    {"id": "zone1", "name": "example.test", "status": "active", "account": {"id": "acct1"}},
                ])),
                ("GET", p) if p.contains("/dns_records?name=") => ok(json!([])),
                ("POST", "/accounts/acct1/cfd_tunnel") => ok(json!({"id": TUNNEL_ID})),
                ("GET", p) if p.ends_with("/token") => ok(json!("eyJhIjoi-made-up-tunnel-token")),
                ("PUT", p) if p.ends_with("/configurations") => ok(json!({})),
                ("POST", "/zones/zone1/dns_records") => {
                    ok(json!({"id": format!("rec-{}", rand_id())}))
                }
                ("DELETE", _) => ok(json!({})),
                _ => (
                    404,
                    json!({"success": false, "errors": [{"code": 7003, "message": "no route"}]}),
                ),
            }
        }
    }

    fn rand_id() -> u32 {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(1);
        N.fetch_add(1, Ordering::Relaxed)
    }

    fn calls(seen: &Seen) -> Vec<String> {
        seen.lock()
            .unwrap()
            .iter()
            .map(|(m, p, _)| format!("{m} {p}"))
            .collect()
    }

    #[test]
    fn the_happy_path_makes_the_tunnel_its_configuration_and_two_records() {
        let (base, seen) = fake_api(cloudflare(None));
        let api = Api::at(&base, "made-up-api-token");
        api.verify().unwrap();
        let zones = usable(api.zones().unwrap()).unwrap();
        assert_eq!(zones, [zone()]);
        let hostname = suggest_hostname(&api, &zones[0], "laptop").unwrap();
        assert_eq!(hostname, "cctop.example.test");
        seen.lock().unwrap().clear();

        let account = create(&api, &zones[0], &hostname, "laptop").unwrap();
        assert_eq!(account.hostname.as_deref(), Some("cctop.example.test"));
        assert_eq!(
            account.share_hostname.as_deref(),
            Some("cctop-share.example.test")
        );
        assert_eq!(account.token, "eyJhIjoi-made-up-tunnel-token");
        assert_eq!(account.tunnel_id.as_deref(), Some(TUNNEL_ID));
        assert_eq!(account.dns_record_ids.len(), 2);
        assert_eq!(account.api_token.as_deref(), Some("made-up-api-token"));

        let made = calls(&seen);
        let creating: Vec<&String> = made.iter().filter(|c| !c.contains("?name=")).collect();
        assert_eq!(
            creating,
            [
                "POST /accounts/acct1/cfd_tunnel",
                &format!("GET /accounts/acct1/cfd_tunnel/{TUNNEL_ID}/token"),
                &format!("PUT /accounts/acct1/cfd_tunnel/{TUNNEL_ID}/configurations"),
                "POST /zones/zone1/dns_records",
                "POST /zones/zone1/dns_records",
            ]
        );
        let bodies = seen.lock().unwrap().clone();
        let cname = bodies
            .iter()
            .find(|(m, p, _)| m == "POST" && p.ends_with("dns_records"))
            .unwrap();
        assert!(
            cname.2.contains(&format!("{TUNNEL_ID}.cfargotunnel.com")),
            "{}",
            cname.2
        );
        assert!(cname.2.contains("\"proxied\":true"), "{}", cname.2);
    }

    #[test]
    fn no_zone_is_the_no_domain_message() {
        let (base, _) = fake_api(|_, path: &str, _| match path {
            p if p.starts_with("/zones?") => ok(json!([])),
            _ => ok(json!({"status": "active"})),
        });
        let api = Api::at(&base, "made-up-api-token");
        let err = usable(api.zones().unwrap()).unwrap_err();
        assert_eq!(err, Error::NoDomain);
        let said = err.to_string();
        assert!(said.contains("domain whose DNS is on Cloudflare"), "{said}");
        assert!(said.contains(ADD_SITE_LINK), "{said}");
    }

    #[test]
    fn a_pending_zone_is_named() {
        let zones = vec![Zone {
            active: false,
            ..zone()
        }];
        assert_eq!(
            usable(zones),
            Err(Error::ZonePending("example.test".into()))
        );
    }

    #[test]
    fn a_refused_token_and_a_missing_permission_are_told_apart() {
        let (base, _) = fake_api(|method: &str, path: &str, _| match (method, path) {
            ("POST", p) if p.ends_with("/cfd_tunnel") => (
                403,
                json!({"success": false, "errors": [{"code": 10000, "message": "Authentication error"}]}),
            ),
            _ => ok(json!({"status": "active"})),
        });
        let refused = Api::at(&base, "not-the-made-up-one").verify().unwrap_err();
        assert_eq!(refused, Error::TokenRefused);
        assert!(refused.to_string().contains("refused"));

        let api = Api::at(&base, "made-up-api-token");
        let missing = create(&api, &zone(), "cctop.example.test", "laptop").unwrap_err();
        assert_eq!(missing, Error::MissingPermission(PERMISSIONS[0]));
        let said = missing.to_string();
        assert!(said.contains("Cloudflare Tunnel"), "{said}");
        assert!(
            said.contains("dash.cloudflare.com/profile/api-tokens"),
            "{said}"
        );
    }

    #[test]
    fn a_taken_name_is_never_overwritten_and_the_next_one_is_offered() {
        let (base, seen) = fake_api(|method: &str, path: &str, _| match (method, path) {
            ("GET", p) if p.contains("?name=cctop.example.test") => ok(json!([{"id": "theirs"}])),
            ("GET", p) if p.contains("?name=") => ok(json!([])),
            _ => ok(json!({})),
        });
        let api = Api::at(&base, "made-up-api-token");
        assert_eq!(
            suggest_hostname(&api, &zone(), "laptop").unwrap(),
            "cctop-laptop.example.test"
        );
        let err = create(&api, &zone(), "cctop.example.test", "laptop").unwrap_err();
        assert!(err.to_string().contains("will not be overwritten"), "{err}");
        assert!(
            !calls(&seen)
                .iter()
                .any(|c| c.starts_with("POST") || c.starts_with("PUT")),
            "{:?}",
            calls(&seen)
        );
    }

    #[test]
    fn a_failed_record_undoes_the_tunnel_and_the_record_before_it() {
        // The first CNAME succeeds and the second fails: both the record and
        // the tunnel must go.
        let (base, seen) = fake_api({
            let inner = cloudflare(None);
            let posted = Arc::new(Mutex::new(0));
            move |method: &str, path: &str, body: &str| {
                if method == "POST" && path == "/zones/zone1/dns_records" {
                    let mut n = posted.lock().unwrap();
                    *n += 1;
                    if *n == 2 {
                        return (
                            500,
                            json!({"success": false, "errors": [{"code": 1, "message": "made-up failure"}]}),
                        );
                    }
                }
                inner(method, path, body)
            }
        });
        let api = Api::at(&base, "made-up-api-token");
        let err = create(&api, &zone(), "cctop.example.test", "laptop").unwrap_err();
        assert_eq!(err, Error::Api("made-up failure".into()));
        let made = calls(&seen);
        let deletes: Vec<&String> = made.iter().filter(|c| c.starts_with("DELETE")).collect();
        assert_eq!(deletes.len(), 2, "{made:?}");
        assert!(
            deletes[0].starts_with("DELETE /zones/zone1/dns_records/rec-"),
            "{made:?}"
        );
        assert_eq!(
            deletes[1],
            &format!("DELETE /accounts/acct1/cfd_tunnel/{TUNNEL_ID}")
        );
    }

    #[test]
    fn a_failed_tunnel_creation_leaves_nothing_to_undo() {
        let (base, seen) = fake_api(cloudflare(Some("/accounts/")));
        let api = Api::at(&base, "made-up-api-token");
        assert!(create(&api, &zone(), "cctop.example.test", "laptop").is_err());
        assert!(!calls(&seen).iter().any(|c| c.starts_with("DELETE")));
    }

    #[test]
    fn remove_deletes_by_the_stored_ids_and_only_those() {
        let (base, seen) = fake_api(cloudflare(None));
        let account = Account {
            token: "eyJhIjoi-made-up".into(),
            api_token: Some("made-up-api-token".into()),
            account_id: Some("acct1".into()),
            zone_id: Some("zone1".into()),
            tunnel_id: Some(TUNNEL_ID.into()),
            dns_record_ids: vec!["rec-a".into(), "rec-b".into()],
            ..Account::default()
        };
        let left = remove_with(&account, |token| Api::at(&base, token));
        assert_eq!(left, Leftovers::default());
        assert_eq!(
            calls(&seen),
            [
                "DELETE /zones/zone1/dns_records/rec-a",
                "DELETE /zones/zone1/dns_records/rec-b",
                &format!("DELETE /accounts/acct1/cfd_tunnel/{TUNNEL_ID}"),
            ]
        );
    }

    #[test]
    fn remove_says_what_is_left_when_the_token_is_revoked() {
        let (base, _) = fake_api(cloudflare(None));
        let account = Account {
            token: "eyJhIjoi-made-up".into(),
            api_token: Some("revoked".into()),
            account_id: Some("acct1".into()),
            zone_id: Some("zone1".into()),
            tunnel_id: Some(TUNNEL_ID.into()),
            dns_record_ids: vec!["rec-a".into()],
            ..Account::default()
        };
        let left = remove_with(&account, |token| Api::at(&base, token));
        assert_eq!(left.0.len(), 2, "{left:?}");
        assert!(left.0[0].contains("rec-a"));
        assert!(left.0[1].contains(TUNNEL_ID));
    }

    #[test]
    fn hostnames_are_one_label_under_the_zone() {
        let zone = zone();
        assert_eq!(
            check_hostname("CCTOP.example.test.", &zone).unwrap(),
            "cctop.example.test"
        );
        let deep = check_hostname("a.b.example.test", &zone)
            .unwrap_err()
            .to_string();
        assert!(deep.contains("two levels"), "{deep}");
        assert!(check_hostname("cctop.other.test", &zone).is_err());
        assert!(check_hostname("-x.example.test", &zone).is_err());
        assert!(check_hostname("a_b.example.test", &zone).is_err());
        assert_eq!(
            share_hostname("cctop.example.test"),
            "cctop-share.example.test"
        );
    }

    #[test]
    fn the_two_kinds_of_token_are_told_apart_by_shape() {
        assert!(matches!(classify("  eyJhIjoiYWJj  "), Pasted::Tunnel(t) if t == "eyJhIjoiYWJj"));
        assert!(matches!(
            classify("Abcdefghij0123456789_-Abcdefghij01234567"),
            Pasted::Api(_)
        ));
        assert!(!format!("{:?}", classify("eyJhIjoiYWJj")).contains("YWJj"));
    }

    #[test]
    fn the_token_link_carries_the_three_permissions_and_the_name() {
        let link = token_link();
        assert!(
            link.starts_with("https://dash.cloudflare.com/profile/api-tokens?permissionGroupKeys=")
        );
        for key in ["argotunnel", "dns", "zone"] {
            assert!(link.contains(&format!("%22{key}%22")), "{link}");
        }
        assert!(link.ends_with("&name=cctop"), "{link}");
    }

    #[test]
    fn a_machine_name_becomes_a_label() {
        assert_eq!(label_of("Flo's Laptop.local\n"), "flo-s-laptop");
        assert_eq!(label_of(""), "machine");
        assert_eq!(label_of("---"), "machine");
    }
}
