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
/// Silent, also deliberately. This is called with the dashboard on screen as
/// often as from the command line, and a line written to stderr under a TUI is
/// painted straight over it — `cctop: opening a trycloudflare tunnel…` sat
/// across somebody's session list for exactly as long as the registration took.
/// Whoever called says so on the surface they own: the command line prints it,
/// and the dashboard spins. A fallback is reported the same way, through
/// [`Tunnel::fallback`].
pub fn start(port: u16, want: Want) -> anyhow::Result<Tunnel> {
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
                return Ok(Tunnel {
                    url: handle.url().to_string(),
                    kind: Kind::Account,
                    fallback: None,
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
    pub dns_record_ids: Vec<String>,
    pub api_token: Option<String>,
    /// Whether this came from `CCTOP_TUNNEL_TOKEN` rather than the file — and
    /// so is not cctop's to remove.
    pub from_env: bool,
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
        from_env: false,
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
        if !account.dns_record_ids.is_empty() {
            let ids: toml_edit::Array = account.dns_record_ids.iter().collect();
            table.insert("dns_record_ids", toml_edit::value(ids));
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
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| anyhow::anyhow!("{} is not valid TOML ({e}); fix it first", path.display()))?;
    edit(&mut doc);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = crate::quota::temp_beside(path, "toml");
    std::fs::write(&tmp, doc.to_string())?;
    crate::quota::restrict(&tmp)?;
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
            from_env: false,
        }
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
        assert!(text.contains("token = \"keep-me\" # a comment"), "{text}");
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
        assert!(!shown.contains("made-up"), "{shown}");
        assert!(shown.contains("cctop.example.test"), "{shown}");
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
    fn each_failure_says_what_to_do() {
        let refused = why_not(&cctop_tunnel::Error::Refused("Unauthorized".into()));
        assert!(refused.contains("cctop tunnel setup"), "{refused}");
        let nameless = why_not(&cctop_tunnel::Error::NoHostname);
        assert!(nameless.contains("CCTOP_TUNNEL_HOSTNAME"), "{nameless}");
        let unreachable = why_not(&cctop_tunnel::Error::Discovery("no DNS".into()));
        assert!(unreachable.contains("could not reach"), "{unreachable}");
    }
}
