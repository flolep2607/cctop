//! Account-level usage limits, read from each provider's usage endpoint.

use crate::config;
use crate::util;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// Baseline gap between usage checks of one account.
///
/// Quota moves slowly, and the endpoints throttle aggressively — a 30s poll was
/// enough to earn a sustained 429 with a ~15 minute `retry-after`. When a
/// provider asks for longer, `retry_delay_secs` honours that instead.
pub const INTERVAL_SECS: u64 = 300;

/// One rate-limit window.
#[derive(Debug, Clone, Serialize)]
pub struct Window {
    pub label: &'static str,
    pub pct: u32,
    /// Fixed length of this window, when the provider documents one.
    pub duration: Option<Duration>,
    /// Reset time as a Unix timestamp in seconds.
    pub resets_at: Option<i64>,
}

/// A [`Window`] as the usage cache holds it. Read through this because a
/// derived `Deserialize` of a `&'static str` could only borrow from input that
/// lives forever, which a file read is not.
#[derive(Deserialize)]
struct StoredWindow {
    label: String,
    pct: u32,
    duration: Option<Duration>,
    resets_at: Option<i64>,
}

impl<'de> Deserialize<'de> for Window {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let w = StoredWindow::deserialize(d)?;
        Ok(Window {
            // Back to the `&'static str` the fetchers would have produced.
            label: match w.label.as_str() {
                "5h" => "5h",
                "7d" => "7d",
                "cr" => "cr",
                _ => "?",
            },
            pct: w.pct,
            duration: w.duration,
            resets_at: w.resets_at,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

/// Add one or more Claude accounts, starting with `first`.
///
/// On a terminal this walks the user through it: pick a full login or a token,
/// sign in as the right account, let cctop run `claude auth login` or `claude
/// setup-token`, see that it works, and go again for the next one. The browser
/// step is the one people get wrong — both commands authorise whichever
/// claude.ai login the browser already has, so a second account run straight
/// after the first silently signs in as the first again.
///
/// Piped, it reads a single token from stdin and stores it, as it always has:
/// `cctop --add-account work < token.txt` is a script's way in. A full login
/// needs a browser and a person at it, so a script is not offered one.
pub fn add_account(first: &str) -> anyhow::Result<()> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        let token = read_line()?;
        if token.is_empty() {
            anyhow::bail!("no token given; nothing written");
        }
        return store_token(first, &token);
    }
    let mut name = first.to_string();
    for n in 1.. {
        let again = if n > 1 {
            " — not the one\n     you just added: sign out, or use a private window"
        } else {
            ""
        };
        // Checked here and not for a piped token, which has always been taken
        // as named: a full login's name becomes a directory under $HOME, and
        // `cctop as <name>` has to survive a shell.
        if !valid_account_name(&name) {
            eprintln!(
                "! '{name}' will not do: an account name is letters, digits, - _ and . only."
            );
        } else {
            match ask_kind(&name)? {
                Choice::Login => add_login(&name, again)?,
                Choice::Token => add_token(&name, again)?,
                Choice::Skip => eprintln!("Nothing added for '{name}'."),
            }
        }
        eprint!("\nAdd another account? Name it, or press Enter to finish: ");
        name = read_line()?;
        if name.is_empty() {
            break;
        }
    }
    eprintln!(
        "Switch accounts in the launcher with `p`, or from a shell with\n\
         \x20 cctop as <name> claude"
    );
    Ok(())
}

/// Which kind of account the walkthrough makes: the two the TUI's `+ account`
/// popup offers, for the same reason — each keeps something the other gives up.
#[derive(Debug, PartialEq, Eq)]
enum Choice {
    /// `claude auth login` into `~/.claude-<name>`.
    Login,
    /// `claude setup-token`, kept in cctop's config.
    Token,
    /// Neither, for now: an empty answer, which is also what a closed stdin
    /// reads as, so it must not be asked again forever.
    Skip,
}

/// An answer to [`ask_kind`], or `None` for one that is neither and wants
/// asking again.
fn parse_choice(answer: &str) -> Option<Choice> {
    match answer.trim().to_ascii_lowercase().as_str() {
        "" => Some(Choice::Skip),
        "f" | "full" | "full login" | "login" => Some(Choice::Login),
        "t" | "token" => Some(Choice::Token),
        _ => None,
    }
}

/// Ask which kind `name` is, in the popup's words, since the tradeoff is the
/// whole of the choice.
fn ask_kind(name: &str) -> anyhow::Result<Choice> {
    eprintln!(
        "\nAdding Claude account '{name}'. Which kind?\n\
         \x20 [f] full login  its own ~/.claude-{name}, everything works\n\
         \x20 [t] token       shares ~/.claude history, but no Remote Control\n\
         \x20                 or claude.ai connectors"
    );
    loop {
        eprint!("f or t (Enter to skip it): ");
        let answer = read_line()?;
        match parse_choice(&answer) {
            Some(choice) => return Ok(choice),
            None => eprintln!("! '{answer}' is neither."),
        }
    }
}

/// How a full login went.
#[derive(Debug, PartialEq, Eq)]
enum Login {
    /// The directory had credentials before anything ran, so nothing did.
    Already,
    Landed,
    /// The command ran and left no credentials behind.
    Missed,
}

/// Log `dir` in with `run`, unless it already is.
///
/// Refusing an account that is already logged in, rather than logging in
/// again, is the point: a second `auth login` into the same directory replaces
/// its credentials with whichever account the browser holds now, which is
/// usually not the one the directory was named for. `run` is the command, so a
/// test can stand in for the browser.
fn log_in(
    dir: &Path,
    again: &str,
    run: impl FnOnce(&Path) -> std::io::Result<std::process::ExitStatus>,
) -> anyhow::Result<Login> {
    if login_landed(dir) {
        return Ok(Login::Already);
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| anyhow::anyhow!("could not create {}: {e}", dir.display()))?;
    eprintln!(
        "  1. In your browser, be signed in to claude.ai as that account{again}.\n\
         \x20 2. cctop runs `claude auth login` for {}: approve it in the browser.",
        crate::util::tildify(&dir.to_string_lossy())
    );
    match run(dir) {
        Ok(status) if !status.success() => {
            eprintln!("! `claude auth login` exited with {status}.")
        }
        Ok(_) => {}
        Err(e) => eprintln!("! Could not run `claude auth login` ({e})."),
    }
    // Judged by what it left behind rather than how it exited: the
    // credentials file is the account, and nothing else says so afterwards.
    Ok(if login_landed(dir) {
        Login::Landed
    } else {
        Login::Missed
    })
}

/// The full-login half of the walkthrough.
fn add_login(name: &str, again: &str) -> anyhow::Result<()> {
    // `--add-account` alone names `default`, and `~/.claude-default` would be
    // discovered as a second account of that name beside `~/.claude`.
    if name == "default" {
        eprintln!(
            "! `default` is ~/.claude itself; log that in with `claude auth login`,\n\
             \x20 or name this account something else."
        );
        return Ok(());
    }
    let dir = config::claude_login_dir(name);
    let shown = crate::util::tildify(&dir.to_string_lossy());
    // The terminal inherited rather than captured: the command prints the
    // sign-in link for when no browser opens, and can ask for a code back.
    let outcome = log_in(&dir, again, |dir| {
        std::process::Command::new("claude")
            .args(["auth", "login", "--claudeai"])
            .env("CLAUDE_CONFIG_DIR", dir)
            .status()
    })?;
    match outcome {
        Login::Already => {
            eprintln!("{shown} is already logged in — it is in the launcher as {name}.")
        }
        Login::Missed => eprintln!(
            "! `claude auth login` ended without logging in; nothing was added for '{name}'."
        ),
        Login::Landed => {
            eprintln!("Logged in: {shown} is account '{name}', with a history of its own.");
            eprintln!("  Checking it against the usage endpoint…");
            let profile = config::Profile {
                provider: crate::pricing::Provider::Claude,
                name: name.to_string(),
                dir,
                source: config::AccountSource::Directory,
            };
            eprintln!("  {}", describe(&fetch_claude(&profile)));
        }
    }
    Ok(())
}

/// The token half of the walkthrough.
fn add_token(name: &str, again: &str) -> anyhow::Result<()> {
    eprintln!(
        "  1. In your browser, be signed in to claude.ai as that account{again}.\n\
         \x20 2. cctop runs `claude setup-token`: approve it in the browser, and it\n\
         \x20    prints a token that lasts a year.\n\
         \x20 3. Paste that token back here."
    );
    eprint!("Press Enter to run `claude setup-token`, or paste a token you already have: ");
    let mut token = read_line()?;
    if token.is_empty() {
        match std::process::Command::new("claude")
            .arg("setup-token")
            .status()
        {
            Ok(status) if !status.success() => {
                eprintln!("! `claude setup-token` exited with {status}.")
            }
            Ok(_) => {}
            Err(e) => eprintln!("! Could not run `claude setup-token` ({e}); run it yourself."),
        }
        eprint!("\nPaste the token it printed: ");
        token = read_line()?;
    }
    if token.is_empty() {
        eprintln!("No token given; nothing written for '{name}'.");
    } else {
        store_token(name, &token)?;
        eprintln!("  Checking it against the usage endpoint…");
        eprintln!("  {}", describe(&cached(&token, || claude_usage(&token))));
    }
    Ok(())
}

fn read_line() -> anyhow::Result<String> {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// One line on what the usage endpoint said about a freshly stored token.
fn describe(status: &ProviderStatus) -> String {
    match status {
        ProviderStatus::Ok(q) => {
            let windows: Vec<String> = q
                .windows
                .iter()
                .map(|w| format!("{} {}%", w.label, w.pct))
                .collect();
            format!("✓ Works — {}", windows.join(", "))
        }
        ProviderStatus::Expired => {
            "! Refused as expired or invalid. Check the whole token was pasted, then run this again.".into()
        }
        ProviderStatus::RateLimited { .. } => {
            "· Stored; the usage endpoint is rate-limiting, so it is unchecked for now.".into()
        }
        ProviderStatus::ApiBilling => "· That is an API key: billed per use, with no limits to show.".into(),
        other => format!("· Stored, but could not check it: {other:?}"),
    }
}

/// Store `token` as `profile`'s, saying on stderr what that means.
fn store_token(profile: &str, token: &str) -> anyhow::Result<()> {
    save_token(profile, token)?;
    eprintln!(
        "Stored a token for profile '{profile}' in {}.",
        config::CONFIG_FILE.display()
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
             \x20 with the rest. Start one with `cctop as {profile} claude`."
        );
    }
    Ok(())
}

/// Store `token` as `profile`'s in cctop's config, printing nothing — the TUI
/// calls this with the terminal in raw mode, where a stray line is garbage on
/// the dashboard.
pub fn save_token(profile: &str, token: &str) -> anyhow::Result<()> {
    let path = &*config::CONFIG_FILE;
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
    // Launchable at once, and polled at once, rather than after a restart and
    // after the next interval: the account was added to be used.
    config::refresh_launchable();
    NUDGE.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Whether `claude auth login` under `dir` finished: it leaves its credentials
/// there, and nothing else says so once the process has gone.
///
/// Makes the account launchable and polled at once when it did, the way
/// [`save_token`] does for a token.
pub fn login_landed(dir: &Path) -> bool {
    if !dir.join(".credentials.json").is_file() {
        return false;
    }
    config::refresh_launchable();
    NUDGE.store(true, std::sync::atomic::Ordering::Relaxed);
    true
}

/// Set when an account has just been added, so the poller asks about it now
/// rather than at its next interval. The usage cache answers for every other
/// account, so this costs one request: the new one's.
pub static NUDGE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A name an account can go by: the TOML key, the launcher entry, and the
/// argument to `cctop as`, so nothing a shell or a table header would trip on.
pub fn valid_account_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The token `claude setup-token` printed, read off its screen.
///
/// The TUI runs `setup-token` in a terminal of its own rather than asking for a
/// paste, so this is how the token gets back. Too short to be whole means it is
/// not there yet — the screen is read on every tick, including mid-draw.
pub fn token_on_screen(screen: &vt100::Screen) -> Option<String> {
    let token = run_on_screen(screen, "sk-ant-oat", |c| {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '_')
    })?;
    (token.len() >= 60).then_some(token)
}

/// The sign-in link `setup-token` prints for when no browser opened — the
/// usual case over ssh or under WSL.
pub fn link_on_screen(screen: &vt100::Screen) -> Option<String> {
    run_on_screen(screen, "https://", |c| !c.is_whitespace())
}

/// The first run of `ok` characters starting with `prefix`, joined across the
/// rows it was wrapped onto.
///
/// Whoever drew it may have wrapped it — the terminal, or the program, which
/// breaks a long word with a real newline at the width it was given. Both look
/// the same from here: a run that reaches the right edge, carried on at the
/// start of the next row. One column of slack, since some renderers leave the
/// last one empty.
fn run_on_screen(
    screen: &vt100::Screen,
    prefix: &str,
    ok: impl Fn(char) -> bool,
) -> Option<String> {
    let (_, width) = screen.size();
    let rows: Vec<String> = screen.rows(0, width).collect();
    let (first, at) = rows
        .iter()
        .enumerate()
        .find_map(|(i, row)| row.find(prefix).map(|at| (i, at)))?;
    let mut out: String = rows[first][at..].chars().take_while(|c| ok(*c)).collect();
    let mut end = rows[first][..at].chars().count() + out.chars().count();
    for row in &rows[first + 1..] {
        if end + 1 < width as usize {
            break;
        }
        let trimmed = row.trim_start();
        let more: String = trimmed.chars().take_while(|c| ok(*c)).collect();
        if more.is_empty() {
            break;
        }
        end = row.len() - trimmed.len() + more.chars().count();
        out.push_str(&more);
    }
    Some(out)
}

/// `cctop as <account> <agent> [args…]`: run the agent spending a stored token
/// account's subscription.
///
/// This is how a token account is launched at all. The launcher cannot put the
/// token in the argv it hands rmux — that is readable by every `ps` on the
/// machine — so it puts this command there instead, naming the account, and
/// the token travels only in the child's environment, which is the owner's
/// alone to read.
pub fn run_as(args: &[String]) -> anyhow::Result<i32> {
    use std::os::unix::process::CommandExt;
    let [name, command, rest @ ..] = args else {
        anyhow::bail!("usage: cctop as <account> <agent> [args…]");
    };
    let mut cmd = std::process::Command::new(command);
    cmd.args(rest);
    // A full login is its directory, and pointing Claude Code at it is all
    // there is to launching under it — the credentials are already in there.
    if stored_token(name).is_none()
        && let Some(account) = config::accounts_for(crate::pricing::Provider::Claude)
            .into_iter()
            .find(|p| p.name == *name && p.source == config::AccountSource::Directory)
    {
        cmd.env("CLAUDE_CONFIG_DIR", &account.dir);
        return Err(anyhow::anyhow!("could not run {command}: {}", cmd.exec()));
    }
    let Some(token) = stored_token(name) else {
        anyhow::bail!("no account named '{name}'; add one with `+ account` in cctop");
    };
    cmd.env("CLAUDE_CODE_OAUTH_TOKEN", token)
        // Claude Code prefers either of these to the OAuth token, so one left
        // in the shell would quietly bill an API key instead of the account
        // that was asked for.
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN");
    Err(anyhow::anyhow!("could not run {command}: {}", cmd.exec()))
}

/// Owner-only permissions: the file holds a token.
pub(crate) fn restrict(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
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

/// One account's last answer, and when it may be asked again.
#[derive(Serialize, Deserialize)]
struct Cached {
    due: i64,
    status: ProviderStatus,
}

/// The usage for `token`, from the cache while it is fresh, else from `fetch`.
///
/// Every cctop polled on its own — the TUI in one terminal, another in a
/// second, `serve` — so running three asked three times as often as one, and
/// earned the 429 the interval exists to avoid. The file makes the interval
/// per account rather than per process: whoever is due first pays for the
/// request, and everyone else reads its answer until the account is due again.
/// A throttled answer is kept until its own `retry-after`, so no cctop on the
/// machine asks early on that account's behalf.
///
/// Keyed by a hash of the token rather than the account's name: a fresh login
/// or a replaced token is then a miss, not fifteen minutes of "expired", and
/// the token itself is not written to a directory `--clear-cache` treats as
/// disposable.
///
/// ponytail: no lock, so two processes due in the same instant both fetch —
/// one extra request per interval at worst.
fn cached(token: &str, fetch: impl FnOnce() -> ProviderStatus) -> ProviderStatus {
    cached_in(
        &config::CACHE_DIR.join("usage.json"),
        token,
        chrono::Utc::now().timestamp(),
        fetch,
    )
}

fn cached_in(
    path: &Path,
    token: &str,
    now: i64,
    fetch: impl FnOnce() -> ProviderStatus,
) -> ProviderStatus {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    token.hash(&mut hasher);
    let key = format!("{:016x}", hasher.finish());
    let read = || -> std::collections::HashMap<String, Cached> {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    };
    if let Some(hit) = read().remove(&key)
        && now < hit.due
    {
        return hit.status;
    }
    let status = fetch();
    // Read again rather than reusing the first read: the request took a while,
    // and another cctop may have stored its own accounts in the meantime.
    let mut all = read();
    // A day past due is a token nobody polls any more.
    all.retain(|_, c| c.due > now - 86_400);
    all.insert(
        key,
        Cached {
            due: now + status.retry_delay_secs(INTERVAL_SECS) as i64,
            status: status.clone(),
        },
    );
    // Best effort: a cache that cannot be written costs a request, not an answer.
    if let (Some(parent), Ok(json)) = (path.parent(), serde_json::to_vec(&all)) {
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        let _ = std::fs::create_dir_all(parent)
            .and_then(|()| std::fs::write(&tmp, json))
            .and_then(|()| std::fs::rename(&tmp, path));
    }
    status
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
    cached(&token, || claude_usage(&token))
}

/// What Claude's usage endpoint says about `token`, asked now.
fn claude_usage(token: &str) -> ProviderStatus {
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
    cached(&token, || codex_usage(&token))
}

/// What Codex's usage endpoint says about `token`, asked now.
fn codex_usage(token: &str) -> ProviderStatus {
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

    /// The walkthrough's question takes the popup's keys and the words they
    /// stand for, and an empty answer — a closed stdin reads as one — skips
    /// rather than asking forever.
    #[test]
    fn the_kind_of_account_is_asked_in_the_popups_terms() {
        use super::{Choice, parse_choice};
        assert_eq!(parse_choice("f"), Some(Choice::Login));
        assert_eq!(parse_choice(" Full Login "), Some(Choice::Login));
        assert_eq!(parse_choice("T"), Some(Choice::Token));
        assert_eq!(parse_choice("token"), Some(Choice::Token));
        assert_eq!(parse_choice(""), Some(Choice::Skip));
        assert_eq!(parse_choice("yes"), None);
    }

    /// Logging a directory in again would swap its credentials for whichever
    /// account the browser holds now, so one already logged in is left alone —
    /// and the command is never run to find that out.
    #[test]
    fn a_full_login_is_not_run_over_one_that_already_landed() {
        use super::{Login, log_in};
        use std::os::unix::process::ExitStatusExt;
        let home = tempfile::tempdir().expect("tempdir");
        let ok = || Ok(std::process::ExitStatus::from_raw(0));

        let done = home.path().join(".claude-done");
        std::fs::create_dir_all(&done).unwrap();
        std::fs::write(done.join(".credentials.json"), "{}").unwrap();
        let outcome = log_in(&done, "", |_| {
            panic!("ran a login over a logged-in account")
        });
        assert_eq!(outcome.unwrap(), Login::Already);

        // Made if missing, and judged by what the command left behind rather
        // than by how it exited.
        let fresh = home.path().join(".claude-fresh");
        let outcome = log_in(&fresh, "", |dir| {
            std::fs::write(dir.join(".credentials.json"), "{}")?;
            ok()
        });
        assert_eq!(outcome.unwrap(), Login::Landed);

        let abandoned = home.path().join(".claude-abandoned");
        assert_eq!(log_in(&abandoned, "", |_| ok()).unwrap(), Login::Missed);
        assert!(abandoned.is_dir());
    }

    /// A login is done when its credentials are on disk, and not before: the
    /// directory is made first, so its existence says nothing.
    #[test]
    fn a_login_has_landed_once_its_credentials_are_written() {
        let dir = std::env::temp_dir().join(format!("cctop-login-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!super::login_landed(&dir));
        std::fs::write(dir.join(".credentials.json"), "{}").unwrap();
        assert!(super::login_landed(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

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

    /// The request is made once per account per interval, however many
    /// cctops ask, and a new token is never answered with an old one's figures.
    #[test]
    fn usage_is_asked_once_per_account_until_due() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.json");
        let asked = std::cell::Cell::new(0);
        let fetch = || {
            asked.set(asked.get() + 1);
            ProviderStatus::Ok(ProviderQuota {
                plan: None,
                windows: vec![Window {
                    label: "5h",
                    pct: 42,
                    duration: None,
                    resets_at: None,
                }],
                limit_reached: false,
            })
        };
        // Now, because `retry_delay_secs` counts a retry-after from the clock.
        let t0 = chrono::Utc::now().timestamp();
        let first = cached_in(&path, "tok-a", t0, fetch);
        let again = cached_in(&path, "tok-a", t0 + 60, fetch);
        assert_eq!(asked.get(), 1);
        // Read back from disk, label and all.
        let ProviderStatus::Ok(q) = again else {
            panic!("got {again:?}")
        };
        assert_eq!((q.windows[0].label, q.windows[0].pct), ("5h", 42));
        assert!(matches!(first, ProviderStatus::Ok(_)));

        // Another account is its own entry.
        cached_in(&path, "tok-b", t0 + 60, fetch);
        assert_eq!(asked.get(), 2);
        // And once due, the first is asked again.
        cached_in(&path, "tok-a", t0 + INTERVAL_SECS as i64, fetch);
        assert_eq!(asked.get(), 3);

        // A throttled answer holds until its own retry-after, past the interval.
        let retry_at = t0 + 2_000;
        cached_in(&path, "tok-c", t0, || ProviderStatus::RateLimited {
            retry_at: Some(retry_at),
        });
        cached_in(&path, "tok-c", t0 + INTERVAL_SECS as i64 + 1, fetch);
        assert_eq!(asked.get(), 3);
    }

    /// The token comes back whole however the screen wrapped it: by the
    /// terminal, or by the program breaking it with a newline of its own.
    #[test]
    fn a_wrapped_token_is_read_back_whole() {
        let token = format!("sk-ant-oat01-{}", "Ab9_-".repeat(20));
        let mut parser = vt100::Parser::new(10, 40, 0);
        parser.process(format!("Your token:\r\n\r\n{token}\r\n\r\nStore it.").as_bytes());
        assert_eq!(token_on_screen(parser.screen()).as_deref(), Some(&*token));

        let mut parser = vt100::Parser::new(10, 40, 0);
        let (a, rest) = token.split_at(40);
        let (b, c) = rest.split_at(40);
        parser.process(format!("{a}\r\n{b}\r\n{c}\r\n\r\nhttps://x.y/z").as_bytes());
        assert_eq!(token_on_screen(parser.screen()).as_deref(), Some(&*token));
        assert_eq!(
            link_on_screen(parser.screen()).as_deref(),
            Some("https://x.y/z")
        );

        // Half drawn is not a token.
        let mut parser = vt100::Parser::new(10, 40, 0);
        parser.process(b"sk-ant-oat01-abc");
        assert_eq!(token_on_screen(parser.screen()), None);
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
