//! Filesystem locations and process-wide constants.

use crate::pricing::Provider;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub static HOME: LazyLock<PathBuf> =
    LazyLock::new(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")));

/// `$CLAUDE_CONFIG_DIR`, falling back to `~/.claude`.
pub static CLAUDE_CONFIG_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| claude_config_dir_in(&HOME))
});

pub static CLAUDE_PROJECTS_ROOT: LazyLock<PathBuf> =
    LazyLock::new(|| CLAUDE_CONFIG_DIR.join("projects"));

/// One harness configuration directory: its own credentials, settings, and
/// transcripts.
///
/// `$CLAUDE_CONFIG_DIR` lets one machine hold several accounts side by side —
/// a personal login and a work one — and each keeps its own `projects/`. cctop
/// read only the one the env var named, so the sessions of every other profile
/// were invisible: on the machine this was written for, three of them, one of
/// which was running at the time and showed as a row with a process and no
/// model, no cost and no tokens, because its transcript was somewhere cctop
/// was not looking.
///
/// Codex has the same shape under a different name — `$CODEX_HOME`, an
/// `auth.json` instead of a `.credentials.json` — and the same reason to hold
/// more than one: a subscription runs out of window before the day does. So a
/// profile is keyed by the harness it belongs to rather than being Claude's
/// alone, and the launcher, the walk, the PROFILE column and the limits panel
/// all read the one list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// Which harness reads this directory. The env var that selects it, and
    /// the file that proves an account is logged in, differ per harness.
    pub provider: Provider,
    /// What to call it on screen: `default` for the conventional directory,
    /// else the suffix — `~/.claude-work` and `~/.codex-work` are both `work`.
    pub name: String,
    pub dir: PathBuf,
}

/// Every harness whose accounts cctop can tell apart, and how to recognise one:
/// the directory prefix under a home, and the file that makes such a directory
/// an account rather than a folder that happens to be named like one.
///
/// A harness belongs here once launching it under a chosen directory is a
/// promise the env var can keep. The rest of the providers cctop reads have no
/// such variable, and offering a profile for them would be a setting that
/// silently did nothing.
const PROFILED: [(Provider, &str, &str); 2] = [
    (Provider::Claude, ".claude", ".credentials.json"),
    (Provider::Codex, ".codex", "auth.json"),
];

/// How `provider` names its directories and proves one is logged in.
fn conventions(provider: Provider) -> Option<(&'static str, &'static str)> {
    PROFILED
        .iter()
        .find(|(p, _, _)| *p == provider)
        .map(|(_, prefix, credential)| (*prefix, *credential))
}

/// The env var that points `provider` at one of its directories, and where it
/// points today.
pub fn profile_env(provider: Provider) -> Option<(&'static str, &'static Path)> {
    match provider {
        Provider::Claude => Some(("CLAUDE_CONFIG_DIR", CLAUDE_CONFIG_DIR.as_path())),
        Provider::Codex => Some(("CODEX_HOME", CODEX_HOME.as_path())),
        _ => None,
    }
}

/// `argv` prefixed with the environment that points a harness at `profile`.
///
/// `env VAR=dir <argv>` rather than a variable set on the child process,
/// because the argv is what gets handed to rmux — which runs it directly, with
/// no shell to carry an environment for it — and what cctop later reads back
/// off a running tab to say which account it was started under.
///
/// A provider with no such variable is returned unchanged: there is one
/// directory, and pretending to select it would put an `env` prefix on every
/// launch for nothing.
///
/// So is the profile the child would have used anyway, and that one is not a
/// tidiness matter. `CLAUDE_CONFIG_DIR` does not only say where the transcripts
/// are: with it set, Claude Code keeps its `.claude.json` — the login, the
/// onboarding, the per-project trust — inside that directory instead of at
/// `~/.claude.json`. Naming `~/.claude` explicitly therefore points it at a
/// `~/.claude/.claude.json` that no ordinary install has, and the agent comes up
/// asking which theme you would like, logged out, with no session to resume. So
/// `R` on a Claude session did not resume it; it onboarded a stranger.
pub fn argv_under_profile(argv: Vec<String>, profile: &Profile) -> Vec<String> {
    let Some((var, inherited)) = profile_env(profile.provider) else {
        return argv;
    };
    if profile.dir == inherited {
        return argv;
    }
    let mut out = vec![
        "env".to_string(),
        format!("{var}={}", profile.dir.display()),
    ];
    out.extend(argv);
    out
}

/// The name a profile directory goes by, given the prefix its harness uses.
fn profile_name(dir: &Path, prefix: &str) -> String {
    let raw = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match raw.strip_prefix(prefix) {
        // The conventional directory itself, the one every install starts with.
        Some("") => "default".to_string(),
        Some(rest) => rest.trim_start_matches(['-', '_', '.']).to_string(),
        // Somewhere else entirely, named by the env var.
        None => raw,
    }
}

/// Every one of `provider`'s profiles in `home`, the conventional one leading.
///
/// A profile is a directory holding the harness's credential file: that file is
/// what makes a directory an account rather than a folder that happens to be
/// called `.claude-something`. Only the home's immediate children are
/// considered — a profile can contain a nested `.claude` of its own, and
/// treating that as a second account would list one login twice.
pub fn profiles_in(home: &Path, provider: Provider) -> Vec<Profile> {
    let Some((prefix, credential)) = conventions(provider) else {
        return Vec::new();
    };
    let mut out: Vec<Profile> = list_dir(home)
        .into_iter()
        .filter(|name| name.starts_with(prefix))
        .map(|name| home.join(name))
        .filter(|dir| dir.join(credential).is_file())
        .map(|dir| Profile {
            provider,
            name: profile_name(&dir, prefix),
            dir,
        })
        .collect();
    // The default first, so the picker opens on the one most people mean, then
    // by name so the order does not depend on how the directory was read.
    out.sort_by(|a, b| (a.name != "default", &a.name).cmp(&(b.name != "default", &b.name)));
    out
}

/// Every profile of this user's, across every profiled harness, each one's env
/// var included even when it points somewhere discovery would never have looked.
pub static PROFILES: LazyLock<Vec<Profile>> = LazyLock::new(|| {
    let mut out = Vec::new();
    for (provider, prefix, _) in PROFILED {
        let mut found = profiles_in(&HOME, provider);
        if let Some((_, named)) = profile_env(provider)
            && !found.iter().any(|p| p.dir == named)
        {
            found.insert(
                0,
                Profile {
                    provider,
                    name: profile_name(named, prefix),
                    dir: named.to_path_buf(),
                },
            );
        }
        out.extend(found);
    }
    out
});

/// One named profile of this user's, when it still exists.
///
/// A name is what gets remembered — in the prefs file, on a session row — and
/// a directory is what a launch needs. This turns one into the other, and
/// answers `None` for an account that has since been logged out of, so a stale
/// name cannot start an agent under somebody else's subscription.
pub fn profile_named(provider: Provider, name: &str) -> Option<&'static Profile> {
    PROFILES
        .iter()
        .find(|p| p.provider == provider && p.name == name)
}

/// This user's profiles for one harness, in the order the launcher offers them.
pub fn profiles_for(provider: Provider) -> Vec<&'static Profile> {
    PROFILES.iter().filter(|p| p.provider == provider).collect()
}

/// `$CODEX_HOME`, falling back to `~/.codex`.
pub static CODEX_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| HOME.join(".codex"))
});

pub static CODEX_SESSIONS_ROOT: LazyLock<PathBuf> = LazyLock::new(|| CODEX_HOME.join("sessions"));

/// `$CURSOR_HOME`, falling back to `~/.cursor`.
pub static CURSOR_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("CURSOR_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| HOME.join(".cursor"))
});

/// Cursor's native agent transcripts, grouped by project slug.
pub static CURSOR_PROJECTS_ROOT: LazyLock<PathBuf> = LazyLock::new(|| CURSOR_HOME.join("projects"));

/// `$PI_CODING_AGENT_DIR`, falling back to `~/.pi/agent`.
pub static PI_AGENT_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| HOME.join(".pi").join("agent"))
});

/// `$PI_CODING_AGENT_SESSION_DIR`, falling back to Pi's standard session root.
pub static PI_SESSIONS_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("PI_CODING_AGENT_SESSION_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PI_AGENT_DIR.join("sessions"))
});

/// `$GEMINI_DIR`, falling back to `~/.gemini`.
pub static GEMINI_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("GEMINI_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| HOME.join(".gemini"))
});

/// Gemini CLI files its chats under a scratch directory, one subtree per
/// project: `tmp/<project>/chats/session-*.json{,l}`. The name reads like
/// something disposable, but it is where the transcripts actually live.
pub static GEMINI_CHATS_ROOT: LazyLock<PathBuf> = LazyLock::new(|| GEMINI_HOME.join("tmp"));

/// Windsurf keeps per-workspace editor state where its VS Code base does.
///
/// `$WINDSURF_USER_DIR` overrides the whole `User` directory, which is what a
/// portable install moves; without it, follow the platform convention.
pub static WINDSURF_USER_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    if let Some(dir) = std::env::var_os("WINDSURF_USER_DIR") {
        return PathBuf::from(dir);
    }
    let base = if cfg!(target_os = "macos") {
        HOME.join("Library").join("Application Support")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| HOME.join("AppData").join("Roaming"))
    } else {
        dirs::config_dir().unwrap_or_else(|| HOME.join(".config"))
    };
    base.join("Windsurf").join("User")
});

pub static WINDSURF_WORKSPACE_STORAGE: LazyLock<PathBuf> =
    LazyLock::new(|| WINDSURF_USER_DIR.join("workspaceStorage"));

/// OpenCode follows the platform data directory (`~/.local/share` on Linux).
pub static OPENCODE_DATA_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("OPENCODE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| HOME.join(".local").join("share"))
                .join("opencode")
        })
});

/// Where OpenCode reads its own configuration and global plugins, which is the
/// *config* directory rather than the data one its sessions live in.
pub static OPENCODE_CONFIG_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("OPENCODE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::config_dir()
                .unwrap_or_else(|| HOME.join(".config"))
                .join("opencode")
        })
});

/// Cowork (VM) sessions. macOS only.
pub static CLAUDE_MAC_COWORK_ROOT: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    cfg!(target_os = "macos").then(|| {
        HOME.join("Library")
            .join("Application Support")
            .join("Claude")
            .join("local-agent-mode-sessions")
    })
});

/// Claude Code sessions launched from the desktop app. macOS only.
pub static CLAUDE_MAC_CODE_ROOT: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    cfg!(target_os = "macos").then(|| {
        HOME.join("Library")
            .join("Application Support")
            .join("Claude")
            .join("claude-code-sessions")
    })
});

pub static CACHE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    dirs::cache_dir()
        .unwrap_or_else(|| HOME.join(".cache"))
        .join("cctop")
});

pub static COST_CACHE_FILE: LazyLock<PathBuf> = LazyLock::new(|| CACHE_DIR.join("cost-cache.json"));
pub static PRICING_CACHE_FILE: LazyLock<PathBuf> =
    LazyLock::new(|| CACHE_DIR.join("litellm-pricing.json"));
pub static UI_PREFS_FILE: LazyLock<PathBuf> = LazyLock::new(|| CACHE_DIR.join("ui-prefs.json"));

pub const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

pub const PRICING_CACHE_MAX_AGE_SECS: u64 = 24 * 60 * 60;

/// Claude Code triggers auto-compaction at ~83.5% of the context window.
/// Overridable via `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` (an integer percentage).
pub static COMPACT_THRESHOLD: LazyLock<f64> = LazyLock::new(|| {
    std::env::var("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|p| p / 100.0)
        .unwrap_or(0.835)
});

/// Lines above this size are almost always base64 image payloads carrying no
/// token, cost, or tool data. Skipping them keeps large transcripts cheap.
pub const MAX_JSONL_LINE_BYTES: usize = 512 * 1024;

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

/// Another user's home directory, and whose it is.
#[derive(Debug, Clone)]
pub struct OtherHome {
    pub home: PathBuf,
    /// Login name, used for the USER column and nothing else.
    pub user: String,
}

/// `$CCTOP_ALL_USERS`: `0`/`false`/`no` forces the single-home behaviour even
/// for root, anything else forces the sweep on. Unset means "on when root".
///
/// The off switch is for a root shell that only wants its own rows; the on
/// switch is for a non-root user who genuinely can read the homes (a shared
/// group, an NFS export).
fn all_users_wanted() -> bool {
    match std::env::var("CCTOP_ALL_USERS") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no"
        ),
        Err(_) => running_as_root(),
    }
}

/// Homes named outright in `$CCTOP_HOMES`, `:`-separated as a `PATH` is.
///
/// For the machines whose homes are not under `/home` and not in the local
/// `passwd` file — an NFS export mounted at `/export/people`, a container's
/// bind mount — where discovery has nothing to go on and the operator does.
/// Naming any implies the sweep, since asking for a home is asking to read it.
fn named_homes() -> Vec<(PathBuf, String)> {
    let Some(list) = std::env::var_os("CCTOP_HOMES") else {
        return Vec::new();
    };
    std::env::split_paths(&list)
        .filter(|p| !p.as_os_str().is_empty())
        .map(|path| {
            // The directory is named after its user in every layout this is
            // for; there is nothing else to read a login name off.
            let user = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (path, user)
        })
        .collect()
}

#[cfg(unix)]
fn running_as_root() -> bool {
    // SAFETY: geteuid reads process state and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
fn running_as_root() -> bool {
    false
}

/// Every home besides this user's that cctop reads sessions out of.
///
/// Empty in the ordinary case, which is what keeps the single-user cost at
/// zero: each provider's root list is then just its own root, unchanged.
pub static OTHER_HOMES: LazyLock<Vec<OtherHome>> = LazyLock::new(|| {
    let named = named_homes();
    if named.is_empty() && !all_users_wanted() {
        return Vec::new();
    }
    let mut seen: HashSet<PathBuf> = HashSet::from([HOME.clone()]);
    let mut out = Vec::new();

    for (home, user) in named {
        push_home(&mut out, &mut seen, home, user);
    }
    if !all_users_wanted() {
        return out;
    }

    // `/etc/passwd` first, since it is the only source that pairs a home with
    // the login name that owns it.
    if let Ok(passwd) = std::fs::read_to_string("/etc/passwd") {
        for (home, user) in passwd_homes(&passwd) {
            push_home(&mut out, &mut seen, home, user);
        }
    }

    // ponytail: users served by LDAP, SSSD or another directory are not in
    // `/etc/passwd`, and enumerating them properly means getpwent(3) and a
    // libc call per entry. Listing the home parents catches them in the shape
    // that actually occurs — one directory per user, named after them.
    for parent in ["/home", "/Users"].map(PathBuf::from) {
        for name in list_dir(&parent) {
            push_home(&mut out, &mut seen, parent.join(&name), name);
        }
    }
    out
});

/// Lowest uid a login account gets, below which an entry is a service account.
///
/// The convention the distributions set in `/etc/login.defs`; macOS starts its
/// human accounts at 500. Reading `login.defs` to learn the local value would be
/// more correct and would change nothing: no `daemon` or `www-data` has ever
/// run a coding agent, and a site that lowered `UID_MIN` still keeps its
/// service accounts below the default.
const UID_MIN: u32 = if cfg!(target_os = "macos") { 500 } else { 1000 };

/// `(home, user)` for every `passwd` line belonging to a person.
///
/// Field 0 is the name, 2 the uid and 5 the home; a line with fewer fields is
/// a comment or a truncated write and is skipped rather than half-read. Only
/// root and the login accounts are kept — a system account's home exists, is
/// readable as root, and holds nothing, so sweeping the couple of dozen of
/// them is pure noise in the doctor report and pure stat calls at startup.
fn passwd_homes(text: &str) -> Vec<(PathBuf, String)> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            let [user, _, uid, _, _, home, ..] = fields[..] else {
                return None;
            };
            let uid: u32 = uid.parse().ok()?;
            let person = uid == 0 || uid >= UID_MIN;
            (person && !user.is_empty() && !home.is_empty())
                .then(|| (PathBuf::from(home), user.to_string()))
        })
        .collect()
}

/// Record `home` as `user`'s if it is a real directory nobody claimed yet.
fn push_home(out: &mut Vec<OtherHome>, seen: &mut HashSet<PathBuf>, home: PathBuf, user: String) {
    // `/` is what the system accounts carry; taking it would put every path on
    // the machine under one "user" and make the sweep recurse the filesystem.
    if home.parent().is_none() || !seen.insert(home.clone()) || !home.is_dir() {
        return;
    }
    out.push(OtherHome { home, user });
}

/// `primary` plus the same location under every other scanned home.
///
/// `primary` is passed in rather than derived because only this user's home
/// honours the `$CLAUDE_CONFIG_DIR`-style overrides: those name one directory,
/// not a pattern that could be applied to somebody else's home.
pub fn roots_across_homes(primary: &Path, derive: impl Fn(&Path) -> PathBuf) -> Vec<PathBuf> {
    let mut roots = vec![primary.to_path_buf()];
    roots.extend(OTHER_HOMES.iter().map(|o| derive(&o.home)));
    roots
}

pub fn claude_config_dir_in(home: &Path) -> PathBuf {
    home.join(".claude")
}

/// Per-provider session roots, this user's first.
/// Every directory a profiled harness writes transcripts to, across every
/// profile and every home in view.
///
/// One entry per profile rather than one per home: a machine with a personal
/// and a work login has two, and reading only the one the env var happens to
/// name is how a running session ends up with a row and no figures.
///
/// `leaf` is where the harness keeps them inside a profile — `projects` for
/// Claude Code, `sessions` for Codex.
fn profile_roots(provider: Provider, leaf: &str) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = profiles_for(provider)
        .iter()
        .map(|p| p.dir.join(leaf))
        .collect();
    for other in OTHER_HOMES.iter() {
        match profiles_in(&other.home, provider).as_slice() {
            // A home cctop cannot read the inside of still has the one
            // conventional location worth trying.
            [] => {
                if let Some((prefix, _)) = conventions(provider) {
                    roots.push(other.home.join(prefix).join(leaf));
                }
            }
            found => roots.extend(found.iter().map(|p| p.dir.join(leaf))),
        }
    }
    roots.dedup();
    roots
}

pub fn claude_projects_roots() -> Vec<PathBuf> {
    profile_roots(Provider::Claude, "projects")
}

pub fn codex_sessions_roots() -> Vec<PathBuf> {
    profile_roots(Provider::Codex, "sessions")
}

pub fn cursor_projects_roots() -> Vec<PathBuf> {
    roots_across_homes(&CURSOR_PROJECTS_ROOT, |h| {
        h.join(".cursor").join("projects")
    })
}

pub fn pi_sessions_roots() -> Vec<PathBuf> {
    roots_across_homes(&PI_SESSIONS_ROOT, |h| {
        h.join(".pi").join("agent").join("sessions")
    })
}

pub fn gemini_chats_roots() -> Vec<PathBuf> {
    roots_across_homes(&GEMINI_CHATS_ROOT, |h| h.join(".gemini").join("tmp"))
}

/// The platform data directory *for another home*, which `dirs::data_dir` can
/// only answer for the calling user.
fn data_dir_in(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else {
        home.join(".local").join("share")
    }
}

fn config_dir_in(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else {
        home.join(".config")
    }
}

pub fn opencode_data_roots() -> Vec<PathBuf> {
    roots_across_homes(&OPENCODE_DATA_DIR, |h| data_dir_in(h).join("opencode"))
}

pub fn windsurf_workspace_roots() -> Vec<PathBuf> {
    roots_across_homes(&WINDSURF_WORKSPACE_STORAGE, |h| {
        config_dir_in(h)
            .join("Windsurf")
            .join("User")
            .join("workspaceStorage")
    })
}

/// Mac-only roots, empty off macOS exactly as their statics are `None` there.
pub fn claude_mac_roots(primary: &Option<PathBuf>, leaf: &str) -> Vec<PathBuf> {
    let Some(primary) = primary.as_ref() else {
        return Vec::new();
    };
    roots_across_homes(primary, |h| {
        h.join("Library")
            .join("Application Support")
            .join("Claude")
            .join(leaf)
    })
}

/// Which user's home `path` lives under, when it is not this user's.
///
/// `None` means "mine", which is why the USER column is blank rather than
/// repeating the operator's own name on every row.
pub fn owner_of(path: &Path) -> Option<&'static str> {
    OTHER_HOMES
        .iter()
        .find(|o| path.starts_with(&o.home))
        .map(|o| o.user.as_str())
}

/// Every profile in view: this user's, and each other home's when cctop
/// is sweeping them.
///
/// Separate from [`PROFILES`], which is this user's alone and is what
/// the launcher offers to start an agent under. Attribution has to cover every
/// home the walk reaches, or a row read out of somebody else's `.claude-work`
/// would come back unlabelled.
static PROFILES_IN_VIEW: LazyLock<Vec<Profile>> = LazyLock::new(|| {
    let mut out = PROFILES.clone();
    for other in OTHER_HOMES.iter() {
        for (provider, _, _) in PROFILED {
            out.extend(profiles_in(&other.home, provider));
        }
    }
    out
});

/// Which Claude profile `path` was read out of.
///
/// The counterpart of [`owner_of`] for the other axis a machine splits on: one
/// user can hold several logins, each with its own subscription, its own limits
/// and its own `projects/`. Until a row says which, a personal session and a
/// work one are the same row twice — the same confusion USER exists to remove
/// between two people.
///
/// The longest match wins. Profiles are normally siblings, so any prefix test
/// would do; `$CLAUDE_CONFIG_DIR` can name a directory inside another one, and
/// there the specific answer is the true one.
pub fn profile_for(path: &Path) -> Option<&'static str> {
    PROFILES_IN_VIEW
        .iter()
        .filter(|p| path.starts_with(&p.dir))
        .max_by_key(|p| p.dir.as_os_str().len())
        .map(|p| p.name.as_str())
}

/// The most accounts any one harness has in view.
///
/// One is the ordinary case, and a column repeating `default` on every row
/// tells nobody anything — so the table asks this before drawing one.
///
/// Per harness rather than a total across them: every machine with Claude Code
/// and Codex installed has two profiles in view and nothing to tell apart,
/// which a plain count would read as a reason to draw the column.
pub fn profile_count() -> usize {
    PROFILED
        .iter()
        .map(|(provider, _, _)| {
            PROFILES_IN_VIEW
                .iter()
                .filter(|p| p.provider == *provider)
                .count()
        })
        .max()
        .unwrap_or(0)
}

pub const CLAUDE_DEFAULT_CTX: u64 = 200_000;
/// A decimal million, not a mebi-token. Anthropic advertises the large window as
/// 1M tokens and LiteLLM's `max_input_tokens` says 1000000 for the models that
/// have it; `1 << 20` would put cctop 4.9% above the only figure anyone else
/// publishes, and 4.9% away from what LiteLLM tells us for the same model.
pub const CLAUDE_1M_CTX: u64 = 1_000_000;
pub const CODEX_DEFAULT_CTX: u64 = 258_400;

/// `true` if `s` is exactly a lowercase hyphenated UUID.
pub fn is_full_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *c != b'-' {
                    return false;
                }
            }
            _ => {
                if !c.is_ascii_hexdigit() || c.is_ascii_uppercase() {
                    return false;
                }
            }
        }
    }
    true
}

/// Extract a trailing UUID from a Codex rollout filename stem.
pub fn trailing_uuid(stem: &str) -> Option<&str> {
    if stem.len() < 36 {
        return None;
    }
    let tail = &stem[stem.len() - 36..];
    is_full_uuid(tail).then_some(tail)
}

pub fn dir_exists(p: &Path) -> bool {
    p.is_dir()
}

/// Directory entry names, sorted. Returns empty on any IO error.
pub fn list_dir(p: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(p) else {
        return Vec::new();
    };
    let mut out: Vec<String> = rd
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    out.sort();
    out
}

/// Recursively collect files under `dir` whose name ends with `ext`.
pub fn rglob(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in rd.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            let path = entry.path();
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() && path.to_string_lossy().ends_with(ext) {
                out.push(path);
            }
        }
    }
    out
}

/// Modification time in milliseconds since the Unix epoch, or 0 if unavailable.
pub fn file_mtime_ms(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {

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

    /// A home with no readable profiles still gets the conventional location
    /// tried, or another user's sessions would vanish the moment cctop could
    /// not list their home.
    #[test]
    fn an_unreadable_home_still_offers_the_usual_place() {
        let missing = Path::new("/nonexistent-home-cctop");
        assert!(profiles_in(missing, Provider::Claude).is_empty());
        assert_eq!(
            claude_config_dir_in(missing).join("projects"),
            missing.join(".claude").join("projects")
        );
    }
    use super::*;

    #[test]
    fn uuid_validation() {
        assert!(is_full_uuid("7026d578-8cba-4880-b464-9700f1b77b71"));
        assert!(!is_full_uuid("7026D578-8CBA-4880-B464-9700F1B77B71")); // uppercase
        assert!(!is_full_uuid("7026d578-8cba-4880-b464-9700f1b77b7")); // short
        assert!(!is_full_uuid("7026d5788cba4880b4649700f1b77b71")); // no hyphens
    }

    #[test]
    fn passwd_parsing_takes_the_home_field() {
        let homes = passwd_homes(
            "root:x:0:0:root:/root:/bin/bash\n\
             ana:x:1000:1000:Ana,,,:/home/ana:/bin/zsh\n\
             www-data:x:33:33:www-data:/var/www:/usr/sbin/nologin\n\
             # comment\n\
             truncated:x:1001",
        );
        assert_eq!(
            homes,
            vec![
                (PathBuf::from("/root"), "root".to_string()),
                (PathBuf::from("/home/ana"), "ana".to_string()),
            ]
        );
    }

    #[test]
    fn homes_are_deduped_and_must_exist() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("ana");
        std::fs::create_dir(&real).unwrap();

        let mut out = Vec::new();
        let mut seen = HashSet::new();
        push_home(&mut out, &mut seen, real.clone(), "ana".into());
        // The same home under a second login name, a missing one, and `/` —
        // which every system account carries and which would put the whole
        // filesystem under one user.
        push_home(&mut out, &mut seen, real, "ana-again".into());
        push_home(&mut out, &mut seen, dir.path().join("gone"), "gone".into());
        push_home(&mut out, &mut seen, PathBuf::from("/"), "sync".into());

        let users: Vec<&str> = out.iter().map(|o| o.user.as_str()).collect();
        assert_eq!(users, ["ana"]);
    }

    #[test]
    fn uuid_extraction() {
        let stem = "rollout-2026-06-29T10-59-07-019f1075-3f22-7ad0-b496-73dcda6a7a25";
        assert_eq!(
            trailing_uuid(stem),
            Some("019f1075-3f22-7ad0-b496-73dcda6a7a25")
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
