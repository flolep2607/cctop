//! Filesystem locations and process-wide constants.

use crate::pricing::Provider;
// Where each agent keeps its files, whose home is whose, and which accounts
// exist: moved to `cctop-discovery`, which finds sessions by them. Re-exported
// whole so every `config::X` in cctop still resolves.
pub use cctop_discovery::paths::*;
use std::path::PathBuf;
use std::sync::LazyLock;

fn cache_base() -> PathBuf {
    own_dir(dirs::cache_dir()).unwrap_or_else(|| HOME.join(".cache"))
}

/// The runtime directory, else the cache directory: where the sockets live
/// that hooks and shims reach a running cctop through.
///
/// Guarded like the rest because a root cctop binds its socket here, and
/// without a runtime directory a `sudo` that kept `$HOME` would have it bind
/// one inside the invoking user's `~/.cache`.
///
/// "Has one" means one this process can write to, not merely one the
/// environment names. `$XDG_RUNTIME_DIR` is inherited across `su`, `sudo -u` and
/// some ssh setups, where it still points at `/run/user/<somebody else>` — and
/// every tab then failed to open with a bare "Permission denied", because the
/// shim could not create its socket directory. The cache directory is a
/// perfectly good home for sockets (it is what the fallback already was), so an
/// unusable runtime directory is treated as an absent one.
pub fn runtime_base() -> PathBuf {
    #[cfg(any(test, feature = "test-support"))]
    if crate::under_test()
        && let Some(dir) = test_runtime_base()
    {
        return dir;
    }
    own_dir(dirs::runtime_dir())
        .filter(|dir| writable_dir(dir))
        .unwrap_or_else(cache_base)
}

/// A runtime directory of this test's own, installed for as long as the guard is
/// held.
///
/// The runtime directory is shared with every cctop actually running on the
/// machine, and what the tests put in it is real: a bound socket a hook event is
/// delivered to, and a claims file rewritten from empty. Left pointed at the
/// real one, the suite delivers its own events into a live dashboard and
/// truncates the `agents.json` it uses to remember process trees.
///
/// Thread-local rather than one directory for the run, because two tests sharing
/// one is the same collision a smaller: a fake address left for the cleanup test
/// to find is a second live listener's socket to somebody else. The fallback for
/// a test that claims nothing is still a temporary directory, so no test reaches
/// the real one by forgetting.
#[cfg(any(test, feature = "test-support"))]
pub struct RuntimeBase {
    previous: Option<PathBuf>,
    #[allow(dead_code)]
    dir: PathBuf,
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static TEST_RUNTIME_BASE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// A directory no test outside this process can see, as the fallback for a test
/// that claims none of its own. Kept rather than removed so a failing run leaves
/// something to look at.
#[cfg(any(test, feature = "test-support"))]
static TEST_RUNTIME_ROOT: LazyLock<PathBuf> =
    LazyLock::new(|| std::env::temp_dir().join(format!("cctop-runtime-{}", std::process::id())));

#[cfg(any(test, feature = "test-support"))]
fn test_runtime_base() -> Option<PathBuf> {
    let claimed = TEST_RUNTIME_BASE.with(|dir| dir.borrow().clone());
    if claimed.is_none() {
        let _ = std::fs::create_dir_all(&*TEST_RUNTIME_ROOT);
    }
    Some(claimed.unwrap_or_else(|| TEST_RUNTIME_ROOT.clone()))
}

/// Point this thread's runtime directory at a subdirectory of its own.
///
/// `name` is only a label: two tests that pass the same one are in the same
/// directory, which is what makes it a collision rather than a sharing.
#[cfg(any(test, feature = "test-support"))]
pub fn claim_test_runtime_base(name: &str) -> RuntimeBase {
    let dir = TEST_RUNTIME_ROOT.join(name);
    std::fs::create_dir_all(&dir).expect("a runtime directory for this test");
    let previous = TEST_RUNTIME_BASE.with(|claimed| claimed.replace(Some(dir.clone())));
    RuntimeBase { previous, dir }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for RuntimeBase {
    fn drop(&mut self) {
        TEST_RUNTIME_BASE.with(|claimed| *claimed.borrow_mut() = self.previous.take());
    }
}

fn writable_dir(dir: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `path` is a valid NUL-terminated string that outlives the call.
    dir.is_dir() && unsafe { libc::access(path.as_ptr(), libc::W_OK | libc::X_OK) } == 0
}

/// Every account of `provider`'s the limits panel should report: the
/// directories, then the accounts that exist only as a token in cctop's config.
///
/// The wider list of the two. [`profiles_for`] answers the questions that need
/// a directory — where to walk for transcripts, which account a transcript came
/// out of, where to launch — and a token account has no answer to any of them.
/// Its subscription is real all the same, and reporting how much of it is left
/// is the whole reason the token was typed in.
///
/// Read fresh rather than held: this is asked once per polling interval, on a
/// file of a few lines, and the alternative is that an account added while
/// cctop is running does not appear until it is restarted.
pub fn accounts_for(provider: Provider) -> Vec<Profile> {
    // Directories are looked for again too, for the same reason: the popup
    // makes `~/.claude-<name>` and logs it in while cctop runs.
    refresh_profiles();
    let mut out: Vec<Profile> = profiles_for(provider).into_iter().cloned().collect();
    // Claude's alone: a token account is one `$CLAUDE_CODE_OAUTH_TOKEN` could
    // have named, and Codex has no such variable to make the same promise.
    if provider != Provider::Claude {
        return out;
    }
    for name in token_account_names() {
        // A name that is also a directory is that directory's account, not a
        // second one — the token is read as its credentials (see
        // `quota::read_claude_credential_in`) and one account is one column.
        if out.iter().any(|p| p.name == name) {
            continue;
        }
        out.push(Profile {
            provider,
            name,
            // The default directory, because that is where such an account's
            // sessions really are: the token changes which subscription is
            // spent, not where Claude Code keeps its history.
            dir: CLAUDE_CONFIG_DIR.clone(),
            source: AccountSource::Token,
        });
    }
    out
}

/// Every account the launcher offers for `provider`: [`accounts_for`], as of
/// the last [`refresh_launchable`].
///
/// Held rather than read fresh because the launcher remembers its choice as an
/// index into this list and hands out `&'static` profiles. New accounts are
/// appended, so an index already chosen keeps pointing at the same account.
pub fn launchable_for(provider: Provider) -> Vec<&'static Profile> {
    if LAUNCHABLE.lock().map(|l| l.is_empty()).unwrap_or(false) {
        refresh_launchable();
    }
    LAUNCHABLE
        .lock()
        .map(|l| {
            l.iter()
                .copied()
                .filter(|p| p.provider == provider)
                .collect()
        })
        .unwrap_or_default()
}

/// The names of every account the launcher offers, per harness that takes
/// one, in the launcher's order: what this machine tells a sibling's launcher
/// (`/api/accounts`), so an agent started here from there can be put under
/// one. Names only — a directory or a token is this machine's business.
pub fn launchable_names() -> Vec<(Provider, Vec<String>)> {
    refresh_launchable();
    PROFILED
        .iter()
        .map(|(provider, _, _)| {
            let names = launchable_for(*provider)
                .into_iter()
                .map(|p| p.name.clone())
                .collect();
            (*provider, names)
        })
        .collect()
}

/// Add `profile` to the accounts the launcher offers — for a test of a launch
/// under one, which cannot count on the machine running it having any.
#[cfg(any(test, feature = "test-support"))]
pub fn add_launchable(profile: Profile) {
    if LAUNCHABLE.lock().map(|l| l.is_empty()).unwrap_or(false) {
        refresh_launchable();
    }
    if let Ok(mut known) = LAUNCHABLE.lock()
        && !known.iter().any(|p| **p == profile)
    {
        known.push(Box::leak(Box::new(profile)));
    }
}

/// One account the launcher offers for `provider`, by name.
///
/// The launchable counterpart of [`profile_named`], which knows directories
/// only: a handoff names its receiving account the way the launcher lists it,
/// and a token account is as much a place to send work as a directory is —
/// more often, since it is the cheap way to hold a second subscription.
pub fn launchable_named(provider: Provider, name: &str) -> Option<&'static Profile> {
    launchable_for(provider)
        .into_iter()
        .find(|p| p.name == name)
}

/// Pick up accounts added since the launcher last looked.
///
/// ponytail: an account removed from the config stays launchable until
/// restart, and each one added is a small leak for the life of the process.
pub fn refresh_launchable() {
    let Ok(mut known) = LAUNCHABLE.lock() else {
        return;
    };
    for provider in PROFILED.map(|(provider, _, _)| provider) {
        for account in accounts_for(provider) {
            if !known.iter().any(|p| **p == account) {
                known.push(Box::leak(Box::new(account)));
            }
        }
    }
}

static LAUNCHABLE: std::sync::Mutex<Vec<&'static Profile>> = std::sync::Mutex::new(Vec::new());

/// Where the popup puts a full login named `name`: the directory discovery
/// already knows to look in, so the account is found the way one made by hand
/// would be.
pub fn claude_login_dir(name: &str) -> PathBuf {
    HOME.join(format!(".claude-{name}"))
}

/// The token accounts: every name with a token stored for it, sealed or still
/// in plain text in the config (see [`crate::secrets`]).
pub(crate) fn token_account_names() -> Vec<String> {
    account_names_in(&crate::secrets::all())
}

/// The same, from the secrets — the half that is worth testing.
pub(crate) fn account_names_in(secrets: &crate::secrets::Secrets) -> Vec<String> {
    // An empty token is a half-finished edit, and cannot be polled, so it
    // does not earn a column that would only ever say "not signed in";
    // `account_tokens` leaves those out.
    let mut out: Vec<String> = crate::secrets::account_tokens(secrets)
        .map(|(name, _)| name.to_string())
        .collect();
    // By name, as `profiles_in` sorts the directories: the limits panel draws a
    // column per account in this order, and one that followed the layout of a
    // hand-edited file would rearrange itself when the file was tidied.
    out.sort();
    out
}

/// cctop's own config. The tokens it once held are in `secrets.enc` beside
/// it now (see [`crate::secrets`]).
///
/// Separate from `CACHE_DIR` on purpose: a cache is something cctop may delete
/// to recover, and `--clear-cache` does. A token the user typed in is not.
pub static CONFIG_FILE: LazyLock<PathBuf> =
    LazyLock::new(|| config_base().join("cctop").join("config.toml"));

/// Everything cctop may delete and rebuild: the cost cache, the pricing table,
/// the topical index and the model it was built with, preferences, logs.
///
/// A test gets a directory of its process's own instead. The real one is the
/// developer's, and tests did write it: `cctop recall`'s end-to-end test loaded
/// the fetched 30 MB model from it — the slowest test of a cold run, and only
/// on machines that had fetched one — and then saved its three fixture sessions
/// over the real `embeddings.bin`. Guarding each writer, as [`UiPrefs::save`]
/// does, misses the next one; one redirect here covers every path below.
///
/// [`UiPrefs::save`]: crate::cache::UiPrefs::save
pub static CACHE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    #[cfg(any(test, feature = "test-support"))]
    if crate::under_test() {
        return std::env::temp_dir().join(format!("cctop-cache-{}", std::process::id()));
    }
    cache_base().join("cctop")
});

pub static COST_CACHE_FILE: LazyLock<PathBuf> = LazyLock::new(|| CACHE_DIR.join("cost-cache.json"));
pub static PRICING_CACHE_FILE: LazyLock<PathBuf> =
    LazyLock::new(|| CACHE_DIR.join("litellm-pricing.json"));
pub static UI_PREFS_FILE: LazyLock<PathBuf> = LazyLock::new(|| CACHE_DIR.join("ui-prefs.json"));
/// Chunk vectors for the topical search, and the fingerprints they were built
/// from. A cache in the full sense: deleting it costs the second it takes to
/// build again.
pub static EMBEDDING_INDEX_FILE: LazyLock<PathBuf> =
    LazyLock::new(|| CACHE_DIR.join("embeddings.bin"));
/// Where a fetched embedding model is unpacked.
pub static EMBEDDING_MODEL_DIR: LazyLock<PathBuf> = LazyLock::new(|| CACHE_DIR.join("models"));
/// Readings of each account's rate-limit windows, kept because the provider
/// reports only the current figure and a reset destroys the evidence. See
/// [`crate::burn`].
pub static BURN_LOG_FILE: LazyLock<PathBuf> = LazyLock::new(|| CACHE_DIR.join("burn-log.json"));

pub const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

pub const PRICING_CACHE_MAX_AGE_SECS: u64 = 24 * 60 * 60;

/// Claude Code triggers auto-compaction at ~83.5% of the context window.
/// Overridable via `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` (an integer percentage),
/// which is what Claude Code itself reads, else `compact_threshold` in cctop's
/// config for a user whose agents are configured some other way.
pub static COMPACT_THRESHOLD: LazyLock<f64> = LazyLock::new(|| {
    std::env::var("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|p| p / 100.0)
        .or_else(|| crate::settings::Settings::load().compact_threshold)
        .unwrap_or(0.835)
});

// The line parsers in `cctop-discovery` skip by it, so it lives there.
pub use cctop_discovery::session::MAX_JSONL_LINE_BYTES;

/// Cap on retained per-tool invocation details, keeping the newest.
pub const MAX_TOOL_DETAILS: usize = 200;

/// Cap on retained invocation details across *all* tools in one session.
///
/// `MAX_TOOL_DETAILS` is per tool name, so a session touching fifteen tools can
/// hold three thousand details at roughly a kilobyte each — measured at 95-99%
/// of a cache entry. The Tools panel shows a recent slice, never thousands of
/// rows, so the extra history costs disk and deserialization time and buys
/// nothing.
pub const MAX_SESSION_TOOL_DETAILS: usize = 400;

/// Cap on a retained detail's full text (characters, not bytes).
///
/// `full` exists so the clipboard gets the untruncated command or prompt; a
/// whole file's contents pasted into a tool call is not that.
pub const MAX_TOOL_DETAIL_CHARS: usize = 800;

/// Cap on diff lines kept per edit, so a large refactor doesn't bloat the cache.
pub const MAX_DIFF_LINES: usize = 60;

/// Cap on a single retained diff line, which minified or generated files can
/// otherwise make arbitrarily long.
pub const MAX_DIFF_LINE_CHARS: usize = 300;

// --- Other users' homes -----------------------------------------------------
//
// Everything below exists for one case: cctop running as root, which is a
// request to see the whole machine rather than root's own (usually empty)
// home. An unprivileged user cannot read another's transcripts, so there is
// nothing to offer them here and the ordinary path stays exactly as it was —
// one home, the statics above, no extra directory reads.

/// Cursor's global databases, or none under test, so no test ever opens the
/// developer's own; tests hand theirs in. The search is cctop-discovery's.
pub fn cursor_global_dbs() -> Vec<PathBuf> {
    if crate::under_test() {
        return Vec::new();
    }
    cctop_discovery::paths::cursor_global_dbs()
}

pub const CLAUDE_DEFAULT_CTX: u64 = 200_000;
/// A decimal million, not a mebi-token. Anthropic advertises the large window as
/// 1M tokens and LiteLLM's `max_input_tokens` says 1000000 for the models that
/// have it; `1 << 20` would put cctop 4.9% above the only figure anyone else
/// publishes, and 4.9% away from what LiteLLM tells us for the same model.
pub const CLAUDE_1M_CTX: u64 = 1_000_000;
pub const CODEX_DEFAULT_CTX: u64 = 258_400;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The cache every writer below `CACHE_DIR` uses is not the developer's.
    /// Asked of the derived paths too, since those are what the writers name.
    #[test]
    fn a_test_never_writes_the_real_cache() {
        let real = super::cache_base().join("cctop");
        for path in [
            &*super::CACHE_DIR,
            &*super::EMBEDDING_INDEX_FILE,
            &*super::EMBEDDING_MODEL_DIR,
            &*super::COST_CACHE_FILE,
            &*super::PRICING_CACHE_FILE,
            &*super::UI_PREFS_FILE,
            &*super::BURN_LOG_FILE,
        ] {
            assert!(
                !path.starts_with(&real),
                "{} is the real cache",
                path.display()
            );
            assert!(path.starts_with(std::env::temp_dir()), "{}", path.display());
        }
    }

    /// A profile is a directory with credentials in it. The nested `.claude` a
    /// profile can end up containing is the same login, not a second one, so
    /// only the home's own children count.
    #[test]
    fn profiles_are_the_directories_holding_credentials() {
        let dir = std::env::temp_dir().join(format!("cctop-prof-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let make = |path: &Path, creds: bool| {
            std::fs::create_dir_all(path).unwrap();
            if creds {
                std::fs::write(path.join(".credentials.json"), "{}").unwrap();
            }
        };
        make(&dir.join(".claude"), true);
        make(&dir.join(".claude-work"), true);
        // Signed out, or never signed in: a folder, not an account.
        make(&dir.join(".claude-empty"), false);
        // A profile's own nested config, which is the same login again.
        make(&dir.join(".claude-work").join(".claude"), true);
        // Not a profile at all.
        make(&dir.join(".config"), true);

        let found = profiles_in(&dir, Provider::Claude);
        let names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();

        assert_eq!(names, ["default", "work"], "{found:?}");
        assert_eq!(found[0].dir, dir.join(".claude"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `~/.claude` is what every install starts with, so it is the one a picker
    /// should open on however the directory happened to be read.
    /// The attribution a row depends on, over the layout profiles actually
    /// take: siblings in one home, told apart by their directory name.
    #[test]
    fn a_transcript_is_attributed_to_the_profile_it_lives_under() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path();
        for name in [".claude", ".claude-work"] {
            let profile = home.join(name);
            std::fs::create_dir_all(profile.join("projects")).expect("mkdir");
            std::fs::write(profile.join(".credentials.json"), "{}").expect("write");
        }
        let found = profiles_in(home, Provider::Claude);
        let named = |path: &Path| -> Option<String> {
            found
                .iter()
                .filter(|p| path.starts_with(&p.dir))
                .max_by_key(|p| p.dir.as_os_str().len())
                .map(|p| p.name.clone())
        };

        // `.claude-work` must not be read as living under `.claude`. Path
        // prefixes compare by component, which is the whole reason this holds —
        // a string prefix test would put every work session on the default.
        assert_eq!(
            named(&home.join(".claude/projects/repo/a.jsonl")).as_deref(),
            Some("default")
        );
        assert_eq!(
            named(&home.join(".claude-work/projects/repo/a.jsonl")).as_deref(),
            Some("work")
        );
        // A path under neither belongs to neither.
        assert_eq!(named(&home.join(".codex/sessions/a.jsonl")), None);
    }

    /// The same rule for Codex, whose accounts are `auth.json` rather than
    /// `.credentials.json` — the reason discovery is parameterised at all.
    ///
    /// The bug this closes: a session started under `~/.codex-work` was read out
    /// of nobody's sessions directory, so it showed as a row with a process and
    /// no model, no cost and no tokens.
    #[test]
    fn codex_profiles_are_the_directories_holding_auth() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path();
        for (name, signed_in) in [
            (".codex", true),
            (".codex-work", true),
            // Logged out: a folder, not an account.
            (".codex-old", false),
        ] {
            let profile = home.join(name);
            std::fs::create_dir_all(profile.join("sessions")).expect("mkdir");
            if signed_in {
                std::fs::write(profile.join("auth.json"), "{}").expect("write");
            }
        }
        // Claude's credential file does not make a Codex profile, and the two
        // harnesses do not see each other's directories.
        std::fs::create_dir_all(home.join(".claude")).expect("mkdir");
        std::fs::write(home.join(".claude").join(".credentials.json"), "{}").expect("write");

        let found = profiles_in(home, Provider::Codex);
        let names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["default", "work"], "{found:?}");
        assert_eq!(found[0].dir, home.join(".codex"));
        assert!(found.iter().all(|p| p.provider == Provider::Codex));

        // And a transcript is attributed to the account it lives under, by the
        // same longest-match rule the Claude side documents.
        let named = |path: &Path| -> Option<String> {
            found
                .iter()
                .filter(|p| path.starts_with(&p.dir))
                .max_by_key(|p| p.dir.as_os_str().len())
                .map(|p| p.name.clone())
        };
        assert_eq!(
            named(&home.join(".codex-work/sessions/2026/a.jsonl")).as_deref(),
            Some("work")
        );
        assert_eq!(
            named(&home.join(".codex/sessions/2026/a.jsonl")).as_deref(),
            Some("default")
        );
    }

    /// The column exists to tell one account from another, so what decides it
    /// is whether a single harness has two — not how many harnesses there are.
    ///
    /// The bug this closes: counting every profile in view meant an ordinary
    /// machine with Claude Code and Codex each signed in once had a PROFILE
    /// column reading `default` on every row.
    #[test]
    fn the_profile_column_answers_to_one_harness_holding_two_accounts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path();
        let make = |name: &str, credential: &str| {
            let path = home.join(name);
            std::fs::create_dir_all(&path).expect("mkdir");
            std::fs::write(path.join(credential), "{}").expect("write");
        };
        // One account each: nothing to distinguish.
        make(".claude", ".credentials.json");
        make(".codex", "auth.json");
        let most = |home: &Path| {
            PROFILED
                .iter()
                .map(|(provider, _, _)| profiles_in(home, *provider).len())
                .max()
                .unwrap_or(0)
        };
        assert_eq!(most(home), 1);

        // A second Codex subscription is the case the column is for.
        make(".codex-work", "auth.json");
        assert_eq!(most(home), 2);
    }

    #[test]
    fn the_default_profile_leads_and_the_rest_are_ordered() {
        let dir = std::env::temp_dir().join(format!("cctop-prof2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for name in [".claude-zeta", ".claude-alpha", ".claude"] {
            let path = dir.join(name);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join(".credentials.json"), "{}").unwrap();
        }
        let names: Vec<String> = profiles_in(&dir, Provider::Claude)
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["default", "alpha", "zeta"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The stored tokens name the token-only accounts, and only the ones
    /// that could actually be polled: an empty token is a half-finished edit
    /// rather than a second subscription, and the tunnel's is no account.
    #[test]
    fn token_accounts_are_the_named_ones_with_a_token() {
        let secrets: crate::secrets::Secrets = [
            ("accounts.work.token", "sk-ant-oat01-w"),
            ("accounts.blank.token", "  "),
            ("accounts.side.token", "sk-ant-oat01-s"),
            ("tunnel.token", "eyJhIjoi-t"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        assert_eq!(account_names_in(&secrets), ["side", "work"]);
        assert!(account_names_in(&Default::default()).is_empty());
    }

    /// A token account launches through `cctop as <name>`, under the
    /// directory it shares with everyone else, and never carries its token
    /// into an argv — `ps` is world-readable. The tab and the handoff still see
    /// the agent underneath.
    #[test]
    fn a_token_account_launches_by_name_not_by_token() {
        let argv = vec!["claude".to_string(), "--resume".to_string()];
        let token = Profile {
            provider: Provider::Claude,
            name: "work".into(),
            dir: PathBuf::from("/home/x/.claude-work"),
            source: AccountSource::Token,
        };
        let launched = argv_under_profile(argv.clone(), &token);
        assert_eq!(launched[1..], ["as", "work", "claude", "--resume"]);
        assert!(!launched.iter().any(|a| a.contains("CLAUDE")));
        assert_eq!(without_launch_prefix(&launched), argv);
        // `cctop as` with nothing after the name is not stripped to nothing.
        let bare: Vec<String> = ["cctop", "as", "work"].map(String::from).into();
        assert_eq!(without_launch_prefix(&bare), bare);

        // The same directory as a real profile is prefixed as it always was.
        let dir = Profile {
            source: AccountSource::Directory,
            ..token
        };
        assert_eq!(
            argv_under_profile(argv, &dir),
            [
                "env",
                "CLAUDE_CONFIG_DIR=/home/x/.claude-work",
                "claude",
                "--resume"
            ]
        );
    }

    /// Regression: `R` on an ordinary Claude session opened a fresh, logged-out
    /// agent asking which theme to use. The session was stamped `default`, so
    /// the resume ran `env CLAUDE_CONFIG_DIR=~/.claude claude --resume <id>` —
    /// and Claude Code reads its `.claude.json` from inside that directory once
    /// the variable is set, where an ordinary install has never written one.
    #[test]
    fn the_profile_a_launch_would_have_used_anyway_gets_no_prefix() {
        let (var, inherited) = profile_env(Provider::Claude).unwrap();
        let default = Profile {
            provider: Provider::Claude,
            name: "default".to_string(),
            dir: inherited.to_path_buf(),
            source: AccountSource::Directory,
        };
        assert_eq!(
            argv_under_profile(vec!["claude".to_string()], &default),
            ["claude"]
        );
        // A directory the child would not have picked still has to be named.
        let other = Profile {
            dir: inherited.with_file_name(".claude-work"),
            ..default
        };
        let argv = argv_under_profile(vec!["claude".to_string()], &other);
        assert_eq!(argv[0], "env");
        assert!(argv[1].starts_with(&format!("{var}=")), "{argv:?}");
    }
}
