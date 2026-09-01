//! Account-level usage limits, read from each provider's usage endpoint.

use crate::config;
use crate::util;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// One rate-limit window.
#[derive(Debug, Clone)]
pub struct Window {
    pub label: &'static str,
    pub pct: u32,
    /// Fixed length of this window, when the provider documents one.
    pub duration: Option<Duration>,
    /// Reset time as a Unix timestamp in seconds.
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderQuota {
    pub plan: Option<String>,
    pub windows: Vec<Window>,
    pub limit_reached: bool,
}

/// Why a provider's usage figures are or aren't available.
///
/// The reasons are worth distinguishing: an expired sign-in needs the user to do
/// something, a rate-limit resolves on its own, and "not signed in" is neither.
/// Collapsing them all into one message tells the user nothing actionable.
#[derive(Debug, Clone, Default)]
pub enum ProviderStatus {
    #[default]
    Pending,
    Ok(ProviderQuota),
    /// Credential is an API key, so usage is billed rather than capped.
    ApiBilling,
    NotSignedIn,
    /// Sign-in expired; the user must re-authenticate.
    Expired,
    /// Throttled. `retry_at` is a Unix timestamp when known.
    RateLimited {
        retry_at: Option<i64>,
    },
    Unavailable(String),
}

#[derive(Debug, Clone)]
pub struct Quota {
    pub fetched: bool,
    /// One entry per Claude profile, in the order
    /// [`config::profiles_for`] lists them.
    ///
    /// A profile is an account with its own subscription and its own limits, so
    /// one figure cannot stand for all of them. It used to: the poller read
    /// whichever credentials `$CLAUDE_CONFIG_DIR` named and put that on every
    /// pane, so a tab running as somebody's work login showed their personal
    /// account's usage — wrong in the direction that matters, since the number
    /// is consulted to decide whether there is room to keep working.
    pub claude: Vec<ProfileQuota>,
    /// One entry per Codex profile, for the same reason: a second
    /// subscription exists because the first one runs out of window, so the
    /// figure that decides whether there is room to keep working has to be the
    /// one belonging to the account the pane is running as.
    pub codex: Vec<ProfileQuota>,
}

/// One account's limits, and which profile they belong to.
#[derive(Debug, Clone)]
pub struct ProfileQuota {
    pub profile: String,
    pub status: ProviderStatus,
    /// Carried so the panel can name the right repair for a sign-in that has
    /// gone: `claude login` fixes a directory's credentials and does nothing at
    /// all for a token in cctop's config, which only `--add-account` replaces.
    pub source: config::AccountSource,
}

/// Every profile of `provider`'s, pending — so a panel can say it is checking
/// rather than claiming an account has no limits before it has looked.
fn pending_for(provider: crate::pricing::Provider) -> Vec<ProfileQuota> {
    config::accounts_for(provider)
        .iter()
        .map(|p| ProfileQuota {
            profile: p.name.clone(),
            status: ProviderStatus::Pending,
            source: p.source,
        })
        .collect()
}

impl Default for Quota {
    fn default() -> Self {
        Quota {
            fetched: false,
            claude: pending_for(crate::pricing::Provider::Claude),
            codex: pending_for(crate::pricing::Provider::Codex),
        }
    }
}

impl Quota {
    /// The limits for `profile`, or the default profile's when a pane does not
    /// name one — which is every pane started before there was a choice to make.
    pub fn claude_for(&self, profile: Option<&str>) -> Option<&ProviderStatus> {
        Self::lookup(&self.claude, profile)
    }

    /// The same, for Codex.
    pub fn codex_for(&self, profile: Option<&str>) -> Option<&ProviderStatus> {
        Self::lookup(&self.codex, profile)
    }

    fn lookup<'a>(qs: &'a [ProfileQuota], profile: Option<&str>) -> Option<&'a ProviderStatus> {
        match profile {
            Some(name) => qs.iter().find(|q| q.profile == name).map(|q| &q.status),
            None => qs.first().map(|q| &q.status),
        }
    }
}

/// How the outcome of a fetch should pace the next one.
impl ProviderStatus {
    /// Seconds to wait before polling this provider again.
    pub fn retry_delay_secs(&self, default: u64) -> u64 {
        match self {
            // Honour the server's own backoff, with a small margin. Ignoring it
            // is what got us throttled in the first place.
            ProviderStatus::RateLimited { retry_at: Some(at) } => {
                let remaining = at - chrono::Utc::now().timestamp();
                (remaining.max(0) as u64 + 15).max(default)
            }
            ProviderStatus::RateLimited { retry_at: None } => default.max(900),
            // Nothing will change until the user acts, so stop hammering.
            ProviderStatus::Expired | ProviderStatus::NotSignedIn => 900,
            ProviderStatus::ApiBilling => 3600,
            _ => default,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Account {
    pub email: Option<String>,
    pub organization: Option<String>,
}

// ---------------------------------------------------------------------------
// Credentials
// ---------------------------------------------------------------------------

enum Credential {
    OAuth(String),
    ApiKey,
    None,
}

/// API keys and OAuth access tokens both start with `sk-ant-`, so the longer
/// `sk-ant-api` prefix is what distinguishes them. Getting this wrong would
/// report subscription accounts as API-billed.
fn is_api_key(token: &str) -> bool {
    token.starts_with("sk-ant-api")
}

/// Pull an access token out of the stored credential blob.
///
/// Current Claude Code stores JSON; very old builds stored a bare token string.
fn extract_claude_token(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    match serde_json::from_str::<Value>(s) {
        Ok(v) => {
            let oauth = v.get("claudeAiOauth");
            for path in [
                oauth.and_then(|o| o.get("accessToken")),
                oauth.and_then(|o| o.get("access_token")),
                v.get("accessToken"),
                v.get("access_token"),
            ] {
                if let Some(t) = path.and_then(Value::as_str) {
                    return Some(t.to_string());
                }
            }
            None
        }
        Err(_) => Some(s.to_string()),
    }
}

fn read_claude_credential_in(profile: &config::Profile, is_default: bool) -> Credential {
    // Claude Code prefers this variable over anything it has stored, so a
    // session started with it spends an account cctop would otherwise never
    // read. One variable is one value per machine, though — the same argument
    // that keeps the keychain and `~/.claude.json` to the default profile below
    // — so it answers for that profile alone rather than being reported as
    // every profile's usage.
    if is_default && let Ok(tok) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        let tok = tok.trim();
        if !tok.is_empty() {
            return classify(tok);
        }
    }

    // A token the user typed in beats one that was discovered: it is the only
    // way to say "poll this account, not whatever is in that directory", and a
    // silently-ignored token is worse than none.
    if let Some(tok) = stored_token(&profile.name) {
        return classify(&tok);
    }

    // macOS keychain. Current builds use "Claude Code-credentials"; older ones
    // used "Claude Code".
    // Only for the profile Claude Code itself would use. The keychain holds one
    // entry per machine, not one per config directory, so consulting it for a
    // second profile would hand back the first profile's token — the exact
    // confusion this function was parameterised to end.
    #[cfg(target_os = "macos")]
    for service in ["Claude Code-credentials", "Claude Code"]
        .into_iter()
        .filter(|_| is_default)
    {
        let out = std::process::Command::new("security")
            .args(["find-generic-password", "-s", service, "-w"])
            .output();
        if let Ok(out) = out
            && out.status.success()
            && let Some(tok) = extract_claude_token(&String::from_utf8_lossy(&out.stdout))
        {
            return if is_api_key(&tok) {
                Credential::ApiKey
            } else {
                Credential::OAuth(tok)
            };
        }
    }

    // `~/.claude.json` is the account file Claude Code writes beside its config
    // directory, and there is only one of it — so it answers for the default
    // profile alone. A named profile has to prove itself from its own
    // credentials or count as signed out.
    let mut candidates = vec![profile.dir.join(".credentials.json")];
    if is_default {
        candidates.push(config::HOME.join(".claude.json"));
    }
    for path in candidates {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        // ~/.claude.json carries a different shape.
        if let Ok(v) = serde_json::from_str::<Value>(&text)
            && v.get("oauthAccount").is_some()
        {
            if v.get("primaryApiKey")
                .and_then(Value::as_str)
                .is_some_and(is_api_key)
            {
                return Credential::ApiKey;
            }
            let tok = v
                .get("oauthAccount")
                .and_then(|o| o.get("accessToken").or_else(|| o.get("access_token")))
                .and_then(Value::as_str);
            if let Some(tok) = tok {
                return if is_api_key(tok) {
                    Credential::ApiKey
                } else {
                    Credential::OAuth(tok.to_string())
                };
            }
            // No token under `oauthAccount` — fall through to the generic
            // extraction below rather than skipping the file entirely.
        }
        if let Some(tok) = extract_claude_token(&text) {
            return if is_api_key(&tok) {
                Credential::ApiKey
            } else {
                Credential::OAuth(tok)
            };
        }
    }
    Credential::None
}

fn classify(token: &str) -> Credential {
    if is_api_key(token) {
        Credential::ApiKey
    } else {
        Credential::OAuth(token.to_string())
    }
}

/// The token stored for `profile` in cctop's own config, if any.
///
/// Keyed by profile name so it reads as the same list of accounts everything
/// else shows — `[accounts.work]` is the token for the `work` profile, the one
/// the PROFILE column and the limits panel already call `work`.
fn pick_token(text: &str, profile: &str) -> Option<String> {
    let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    let tok = doc
        .get("accounts")?
        .as_table_like()?
        .get(profile)?
        .as_table_like()?
        .get("token")?
        .as_str()?
        .trim();
    (!tok.is_empty()).then(|| tok.to_string())
}

fn stored_token(profile: &str) -> Option<String> {
    pick_token(
        &std::fs::read_to_string(&*config::CONFIG_FILE).ok()?,
        profile,
    )
}

/// Store a token for `profile`, reading it from stdin so it never lands in the
/// shell's history the way an argument would.
pub fn add_account(profile: &str) -> anyhow::Result<()> {
    let path = &*config::CONFIG_FILE;
    eprintln!("Paste a token from `claude setup-token`, then Enter:");
    let mut token = String::new();
    std::io::stdin().read_line(&mut token)?;
    let token = token.trim();
    if token.is_empty() {
        anyhow::bail!("no token given; nothing written");
    }

    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    // Same refusal as the harness config writers: a file cctop cannot parse is
    // one it must not rewrite, and `toml_edit` keeps the comments and layout of
    // a file the user is expected to open.
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| anyhow::anyhow!("{} is not valid TOML ({e}); fix it first", path.display()))?;
    // Built as real tables rather than by indexing straight through, which
    // toml_edit renders as a single inline `accounts = { work = { … } }` line.
    if !doc.contains_key("accounts") {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        doc.insert("accounts", toml_edit::Item::Table(table));
    }
    let accounts = doc["accounts"]
        .as_table_like_mut()
        .ok_or_else(|| anyhow::anyhow!("`accounts` in {} is not a table", path.display()))?;
    if accounts.get(profile).is_none() {
        accounts.insert(profile, toml_edit::Item::Table(toml_edit::Table::new()));
    }
    accounts
        .get_mut(profile)
        .and_then(|a| a.as_table_like_mut())
        .ok_or_else(|| anyhow::anyhow!("account '{profile}' in {} is not a table", path.display()))?
        .insert("token", toml_edit::value(token));

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Written through a temporary file so an interrupted write cannot truncate
    // the config, and created unreadable to anyone else from the start:
    // widening a file that already holds a secret is a window, however short.
    let tmp = path.with_extension("toml.cctop-tmp");
    std::fs::write(&tmp, doc.to_string())?;
    restrict(&tmp)?;
    std::fs::rename(&tmp, path)?;
    eprintln!(
        "Stored a token for profile '{profile}' in {}.",
        path.display()
    );

    if !is_api_key(token) && !token.starts_with("sk-ant-oat") {
        eprintln!("! That does not look like a `claude setup-token` token.");
    }
    // A name with no `~/.claude-<name>` behind it is the ordinary case for a
    // token, not a mistake: it is an account whose sessions live in the one
    // `~/.claude` with everything else. Say which of the two happened, because
    // it decides whether the account will ever label a row.
    if config::profile_named(crate::pricing::Provider::Claude, profile).is_none() {
        eprintln!(
            "  No `~/.claude-{profile}` directory, so this is a token-only account: its\n\
             \x20 limits get a column of their own, and its sessions stay in ~/.claude\n\
             \x20 with the rest — start one with CLAUDE_CODE_OAUTH_TOKEN set to spend it."
        );
    }
    Ok(())
}

/// Owner-only permissions, where the platform has them.
///
/// ponytail: Windows keeps whatever the profile directory grants, which is
/// normally the user alone. Tightening a Windows ACL means a DACL dance for a
/// file already outside other users' reach by default.
fn restrict(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn read_codex_token_in(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("auth.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("tokens")
        .and_then(|t| t.get("access_token"))
        .or_else(|| v.get("access_token"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// Account identity
// ---------------------------------------------------------------------------

pub fn claude_account() -> Option<Account> {
    let text = std::fs::read_to_string(config::HOME.join(".claude.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let oa = v.get("oauthAccount")?;
    Some(Account {
        email: oa
            .get("emailAddress")
            .and_then(Value::as_str)
            .map(str::to_string),
        organization: oa
            .get("organizationName")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// Decode the identity claims from a JWT payload without verifying it.
///
/// This only reads the local token to display who is signed in; it is never
/// used as an authorization decision, so signature verification isn't relevant.
fn jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = util::b64_decode(payload)?;
    serde_json::from_slice(&bytes).ok()
}

pub fn codex_account() -> Option<Account> {
    let text = std::fs::read_to_string(config::CODEX_HOME.join("auth.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let id_token = v.get("tokens")?.get("id_token")?.as_str()?;
    let claims = jwt_claims(id_token)?;
    Some(Account {
        email: claims
            .get("email")
            .and_then(Value::as_str)
            .map(str::to_string),
        organization: None,
    })
}

// ---------------------------------------------------------------------------
// Fetching
// ---------------------------------------------------------------------------

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        // ureq treats non-2xx as `Err` by default, which would route 429 and 401
        // into the generic transport-error arm and lose the distinction between
        // "throttled" and "signed out". We want to inspect the status ourselves.
        .http_status_as_error(false)
        .build()
        .into()
}

/// Outcome of a usage request, keeping the status so the UI can explain itself.
enum Fetched {
    Body(Value),
    Failed(ProviderStatus),
}

fn get_json(req: ureq::RequestBuilder<ureq::typestate::WithoutBody>) -> Fetched {
    let mut resp = match req.call() {
        Ok(r) => r,
        Err(e) => return Fetched::Failed(ProviderStatus::Unavailable(short_error(&e))),
    };
    let status = resp.status().as_u16();
    if status == 429 {
        // `retry-after` is seconds-from-now; store it as an absolute instant so
        // the UI can count down without knowing when the request happened.
        let retry_at = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<i64>().ok())
            .map(|secs| chrono::Utc::now().timestamp() + secs);
        return Fetched::Failed(ProviderStatus::RateLimited { retry_at });
    }
    if status == 401 || status == 403 {
        return Fetched::Failed(ProviderStatus::Expired);
    }
    if !(200..300).contains(&status) {
        return Fetched::Failed(ProviderStatus::Unavailable(format!("HTTP {status}")));
    }
    match resp.body_mut().read_to_string() {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(v) => Fetched::Body(v),
            Err(_) => Fetched::Failed(ProviderStatus::Unavailable("bad response".into())),
        },
        Err(e) => Fetched::Failed(ProviderStatus::Unavailable(short_error(&e))),
    }
}

/// Keep transport errors to something that fits on one line, without discarding
/// the informative tail (a bare "http status" says nothing).
fn short_error(e: &impl std::fmt::Display) -> String {
    let text = e.to_string();
    let one_line = text.lines().next().unwrap_or(&text).trim();
    crate::util::truncate(one_line, 40)
}

/// Accept numeric epochs (seconds or milliseconds) and RFC 3339 timestamps.
///
/// Claude's OAuth endpoint uses the latter while Codex uses the former.
fn as_epoch_secs(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Number(n) => {
            let n = n.as_f64()?;
            Some(if n > 1e12 {
                (n / 1000.0) as i64
            } else {
                n as i64
            })
        }
        Value::String(timestamp) => util::parse_ts(timestamp).map(|dt| dt.timestamp()),
        _ => None,
    }
}

pub fn fetch_claude(profile: &config::Profile) -> ProviderStatus {
    // A token account shares the default directory but is not the default
    // account: it was named precisely to be a second one, so the keychain and
    // the environment variable — both of which hold exactly one account, the
    // one the harness would use unasked — must not answer for it.
    let is_default = profile.source == config::AccountSource::Directory
        && config::profiles_for(crate::pricing::Provider::Claude)
            .first()
            .is_some_and(|first| first.dir == profile.dir);
    let token = match read_claude_credential_in(profile, is_default) {
        Credential::ApiKey => return ProviderStatus::ApiBilling,
        Credential::None => return ProviderStatus::NotSignedIn,
        Credential::OAuth(t) => t,
    };

    let data = match get_json(
        agent()
            .get("https://api.anthropic.com/api/oauth/usage")
            .header("Authorization", &format!("Bearer {token}"))
            .header("anthropic-beta", "oauth-2025-04-20"),
    ) {
        Fetched::Body(v) => v,
        Fetched::Failed(status) => return status,
    };

    let mut q = ProviderQuota {
        plan: data
            .get("rate_limit_tier")
            .and_then(Value::as_str)
            .map(str::to_string),
        ..Default::default()
    };
    for (key, label, duration) in [
        ("five_hour", "5h", Duration::from_secs(5 * 60 * 60)),
        ("seven_day", "7d", Duration::from_secs(7 * 24 * 60 * 60)),
    ] {
        if let Some(w) = data.get(key)
            && let Some(util) = w.get("utilization").and_then(Value::as_f64)
        {
            q.windows.push(Window {
                label,
                pct: util.round() as u32,
                duration: Some(duration),
                resets_at: as_epoch_secs(w.get("resets_at")),
            });
        }
    }
    if q.windows.is_empty() {
        return ProviderStatus::Unavailable("no windows reported".into());
    }
    ProviderStatus::Ok(q)
}

pub fn fetch_codex(profile: &config::Profile) -> ProviderStatus {
    let Some(token) = read_codex_token_in(&profile.dir) else {
        return ProviderStatus::NotSignedIn;
    };
    let data = match get_json(
        agent()
            .get("https://chatgpt.com/backend-api/wham/usage")
            .header("Authorization", &format!("Bearer {token}")),
    ) {
        Fetched::Body(v) => v,
        Fetched::Failed(status) => return status,
    };
    let Some(rl) = data.get("rate_limit") else {
        return ProviderStatus::Unavailable("no rate_limit in response".into());
    };

    let mut q = ProviderQuota {
        plan: data
            .get("plan_type")
            .and_then(Value::as_str)
            .map(str::to_string),
        limit_reached: rl
            .get("limit_reached")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        ..Default::default()
    };

    // Only the primary window is a real quota. `secondary_window.used_percent`
    // measures short-term throttle pressure, not consumption against a cap, so
    // showing it next to the 5h figure would read as a limit that isn't one.
    if let Some(w) = rl.get("primary_window") {
        q.windows.push(Window {
            label: "5h",
            pct: w
                .get("used_percent")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .round() as u32,
            duration: Some(Duration::from_secs(5 * 60 * 60)),
            resets_at: as_epoch_secs(w.get("reset_at")),
        });
    }
    if let Some(crl) = data.get("code_review_rate_limit")
        && let Some(w) = crl.get("primary_window")
    {
        q.windows.push(Window {
            label: "cr",
            pct: w
                .get("used_percent")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .round() as u32,
            duration: Some(Duration::from_secs(5 * 60 * 60)),
            resets_at: as_epoch_secs(w.get("reset_at")),
        });
        if crl.get("limit_reached").and_then(Value::as_bool) == Some(true) {
            q.limit_reached = true;
        }
    }
    if q.windows.is_empty() {
        return ProviderStatus::Unavailable("no windows reported".into());
    }
    ProviderStatus::Ok(q)
}

#[cfg(test)]
mod tests {

    /// The bug this closes: one figure stood for every account, so a pane
    /// running as somebody's work login showed their personal usage. The number
    /// is read to decide whether there is room to keep working, so being wrong
    /// about which account it describes is worse than showing nothing.
    #[test]
    fn each_profile_reports_its_own_limits() {
        let of = |profile: &str, pct: u32| ProfileQuota {
            profile: profile.to_string(),
            source: config::AccountSource::Directory,
            status: ProviderStatus::Ok(ProviderQuota {
                plan: None,
                windows: vec![Window {
                    label: "5h",
                    pct,
                    duration: None,
                    resets_at: None,
                }],
                limit_reached: false,
            }),
        };
        let quota = Quota {
            fetched: true,
            claude: vec![of("default", 10), of("work", 90)],
            codex: vec![of("default", 30), of("work", 60)],
        };
        let pct = |status: Option<&ProviderStatus>| match status {
            Some(ProviderStatus::Ok(q)) => q.windows.first().map(|w| w.pct),
            _ => None,
        };

        assert_eq!(pct(quota.claude_for(Some("work"))), Some(90));
        assert_eq!(pct(quota.claude_for(Some("default"))), Some(10));
        // A pane started before there was a choice to make gets the default.
        assert_eq!(pct(quota.claude_for(None)), Some(10));
        // A profile that has since gone reports nothing rather than somebody
        // else's remaining budget.
        assert!(quota.claude_for(Some("deleted")).is_none());

        // Codex splits the same way, and the two harnesses do not answer for
        // each other: `work` names a different subscription in each.
        assert_eq!(pct(quota.codex_for(Some("work"))), Some(60));
        assert_eq!(pct(quota.codex_for(None)), Some(30));
        assert!(quota.codex_for(Some("deleted")).is_none());
    }
    use super::*;

    /// Regression: the status arms below are only reachable because the agent is
    /// built with `http_status_as_error(false)`.
    #[test]
    fn rate_limit_backoff_honours_retry_after() {
        let at = chrono::Utc::now().timestamp() + 600;
        let s = ProviderStatus::RateLimited { retry_at: Some(at) };
        let delay = s.retry_delay_secs(300);
        assert!((600..=630).contains(&delay), "got {delay}");

        // With no hint, wait well past the default rather than hammering.
        let s = ProviderStatus::RateLimited { retry_at: None };
        assert!(s.retry_delay_secs(300) >= 900);
    }

    #[test]
    fn dead_credentials_stop_being_polled_hard() {
        assert_eq!(ProviderStatus::Expired.retry_delay_secs(300), 900);
        assert_eq!(ProviderStatus::NotSignedIn.retry_delay_secs(300), 900);
        // A healthy provider keeps the baseline cadence.
        assert_eq!(
            ProviderStatus::Ok(ProviderQuota::default()).retry_delay_secs(300),
            300
        );
    }

    #[test]
    fn short_error_keeps_the_informative_part() {
        assert_eq!(short_error(&"http status: 429"), "http status: 429");
    }

    #[test]
    fn a_stored_token_is_read_for_the_profile_it_names() {
        let text = "# mine\n[accounts.work]\ntoken = \"sk-ant-oat01-w\"\n\n[accounts.blank]\ntoken = \"  \"\n";
        assert_eq!(pick_token(text, "work").as_deref(), Some("sk-ant-oat01-w"));
        // Another profile's token is never handed out as this one's.
        assert_eq!(pick_token(text, "default"), None);
        assert_eq!(pick_token(text, "blank"), None);
        assert_eq!(pick_token("", "work"), None);
        assert_eq!(pick_token("not = toml [", "work"), None);
    }

    #[test]
    fn oauth_tokens_are_not_mistaken_for_api_keys() {
        assert!(is_api_key("sk-ant-api03-abc"));
        assert!(!is_api_key("sk-ant-oat01-abc"));
    }

    #[test]
    fn token_extraction_handles_both_shapes() {
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-xyz"}}"#;
        assert_eq!(
            extract_claude_token(json).as_deref(),
            Some("sk-ant-oat01-xyz")
        );
        // Legacy bare-string form.
        assert_eq!(
            extract_claude_token("  sk-ant-oat01-bare  ").as_deref(),
            Some("sk-ant-oat01-bare")
        );
        assert_eq!(extract_claude_token("   "), None);
    }

    #[test]
    fn epoch_normalises_seconds_and_millis() {
        assert_eq!(
            as_epoch_secs(Some(&serde_json::json!(1782705509))),
            Some(1782705509)
        );
        assert_eq!(
            as_epoch_secs(Some(&serde_json::json!(1782705509000i64))),
            Some(1782705509)
        );
        assert_eq!(as_epoch_secs(None), None);
    }

    #[test]
    fn epoch_accepts_claude_rfc3339_reset_times() {
        assert_eq!(
            as_epoch_secs(Some(&serde_json::json!("2026-08-06T12:34:56Z"))),
            Some(1_786_019_696)
        );
    }

    #[test]
    fn jwt_payload_decoding() {
        // {"email":"a@b.com"} base64url, unpadded.
        let token = format!(
            "h.{}.sig",
            util::b64_encode(br#"{"email":"a@b.com"}"#).replace('=', "")
        );
        let claims = jwt_claims(&token).unwrap();
        assert_eq!(claims["email"], "a@b.com");
    }

    #[test]
    fn base64_roundtrip() {
        for case in ["", "a", "ab", "abc", "hello world!", "\u{1F600}"] {
            let enc = util::b64_encode(case.as_bytes());
            assert_eq!(util::b64_decode(&enc).unwrap(), case.as_bytes());
        }
    }
}
