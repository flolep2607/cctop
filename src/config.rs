//! Filesystem locations and process-wide constants.

use crate::pricing::Provider;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// This user's home: where cctop reads its own sessions and writes everything
/// it writes.
///
/// For root it is root's home as `/etc/passwd` gives it, not `$HOME`. A plain
/// `sudo` can keep the invoking user's `$HOME` (`env_keep`, `sudo -E`, older
/// defaults), and trusting it would make that user's home "mine": their rows
/// would go unnamed, and root's cache, UI prefs and any hook it installed
/// would land in their home as files owned by root — which their next
/// unprivileged cctop could then not rewrite. Their home is still read, as one
/// of the [`OTHER_HOMES`], under their own name.
pub static HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    if running_as_root()
        && let Some(home) = std::fs::read_to_string("/etc/passwd")
            .ok()
            .and_then(|text| passwd_home_of(&text, 0))
    {
        return home;
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
});

/// A directory named by the environment, unless root inherited it from
/// somebody else.
///
/// The same `sudo` that keeps `$HOME` keeps `$CLAUDE_CONFIG_DIR`,
/// `$XDG_CACHE_HOME` and the rest, all pointing into the invoking user's home.
/// Reading through them would only show that user's rows under the wrong name
/// — their home is swept anyway — but writing through them is writing into
/// someone else's home, which root mode never does. So as root a directory
/// inside another user's home is treated as unset, and the caller falls back to
/// the conventional place under root's own [`HOME`].
fn own_dir(dir: Option<PathBuf>) -> Option<PathBuf> {
    dir.filter(|d| !foreign_to_root(d))
}

/// [`own_dir`] for an environment variable.
pub fn env_dir(var: &str) -> Option<PathBuf> {
    own_dir(std::env::var_os(var).map(PathBuf::from))
}

/// The XDG base directories, with [`own_dir`]'s guard: `dirs` reads `$HOME`
/// and `$XDG_*` straight from the environment, so under a `sudo` that kept them
/// it answers for the invoking user.
pub fn config_base() -> PathBuf {
    own_dir(dirs::config_dir()).unwrap_or_else(|| HOME.join(".config"))
}

pub fn data_base() -> PathBuf {
    own_dir(dirs::data_dir()).unwrap_or_else(|| HOME.join(".local").join("share"))
}

fn cache_base() -> PathBuf {
    own_dir(dirs::cache_dir()).unwrap_or_else(|| HOME.join(".cache"))
}

/// The runtime directory, else the cache directory: where the sockets live
/// that hooks and shims reach a running cctop through.
///
/// Guarded like the rest because a root cctop binds its socket here, and
/// without a runtime directory a `sudo` that kept `$HOME` would have it bind
/// one inside the invoking user's `~/.cache`.
pub fn runtime_base() -> PathBuf {
    own_dir(dirs::runtime_dir()).unwrap_or_else(cache_base)
}

/// `$CLAUDE_CONFIG_DIR`, falling back to `~/.claude`.
pub static CLAUDE_CONFIG_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| env_dir("CLAUDE_CONFIG_DIR").unwrap_or_else(|| claude_config_dir_in(&HOME)));

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
    /// Whether there is a directory behind this account, or only a token.
    pub source: AccountSource,
}

/// What cctop knows an account from.
///
/// Nearly always a directory. The other case exists because a second
/// subscription does not have to mean a second `~/.claude-work`: Claude Code
/// takes an account from `$CLAUDE_CODE_OAUTH_TOKEN` while still keeping its
/// history, its settings and its project trust in the one `~/.claude` — so a
/// user can switch which subscription they are spending without splitting the
/// transcripts they resume from in two. cctop could not report such an account
/// at all, because it had no directory to find it by. A token in cctop's own
/// config names it instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountSource {
    /// A `~/.claude*` or `~/.codex*` directory holding the harness's
    /// credentials. Its sessions are its own, and the launcher can start an
    /// agent under it by pointing the harness's env var at the directory.
    Directory,
    /// A token in cctop's `config.toml` and nothing else. It owns no
    /// transcripts to label — its sessions live in the default directory — but
    /// it has a column in the limits panel and can be launched under, through
    /// `cctop as <name>`, which puts the token in the child's environment. The
    /// token itself never goes in an argv, which every `ps` on the machine can
    /// read.
    Token,
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
    // A token account names no directory of its own, and the token itself must
    // not become an argument: `ps` and `/proc/<pid>/cmdline` are readable by
    // every user on a Linux box. So the argv carries the account's *name*, and
    // `cctop as` looks the token up and hands it over in the environment, which
    // only the owner can read. The sessions run under the default directory,
    // which is where that account's history is anyway.
    if profile.source == AccountSource::Token {
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "cctop".to_string());
        let mut out = vec![exe, "as".to_string(), profile.name.clone()];
        out.extend(argv);
        return out;
    }
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

/// The agent's own argv, with whatever a profile launch put in front of it —
/// `env VAR=value …` or `cctop as <name> …` — taken off again.
///
/// One function because the tab label and the handoff both need to know which
/// agent a launch runs, and each used to strip the `env` form on its own.
pub fn without_launch_prefix(argv: &[String]) -> &[String] {
    let mut rest = argv;
    // By prefix, since the running binary is what gets named and a test build
    // is `cctop-<hash>`.
    let is_cctop = |a: &String| {
        a.rsplit(['/', '\\'])
            .next()
            .is_some_and(|base| base.starts_with("cctop"))
    };
    if rest.len() > 3 && is_cctop(&rest[0]) && rest[1] == "as" {
        rest = &rest[3..];
    }
    if rest.first().map(String::as_str) == Some("env") {
        let mut after = &rest[1..];
        while after
            .first()
            .is_some_and(|a| a.contains('=') && !a.starts_with('-'))
        {
            after = &after[1..];
        }
        // Only when a command is left. `env` alone is a command in its own
        // right, and naming it after nothing is worse than naming it oddly.
        if !after.is_empty() {
            rest = after;
        }
    }
    rest
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
            source: AccountSource::Directory,
        })
        .collect();
    // The default first, so the picker opens on the one most people mean, then
    // by name so the order does not depend on how the directory was read.
    out.sort_by(|a, b| (a.name != "default", &a.name).cmp(&(b.name != "default", &b.name)));
    out
}

/// Every profile of this user's, across every profiled harness, each one's env
/// var included even when it points somewhere discovery would never have looked.
fn discover_mine() -> Vec<Profile> {
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
                    source: AccountSource::Directory,
                },
            );
        }
        out.extend(found);
    }
    out
}

/// Every profile in every other home cctop is sweeping.
fn discover_others() -> Vec<Profile> {
    let mut out = Vec::new();
    for other in OTHER_HOMES.iter() {
        for (provider, _, _) in PROFILED {
            out.extend(profiles_in(&other.home, provider));
        }
    }
    out
}

/// The profiles found as of the last [`refresh_profiles`].
///
/// Two lists because they answer different questions: `mine` is what the
/// launcher can start an agent under, and `mine` then `others` is what
/// attribution has to cover — every home the walk reaches, or a row read out
/// of somebody else's `.claude-work` would come back unlabelled.
#[derive(Clone, Copy)]
struct Found {
    mine: &'static [Profile],
    others: &'static [Profile],
}

impl Found {
    fn in_view(self) -> impl Iterator<Item = &'static Profile> {
        self.mine.iter().chain(self.others)
    }
}

/// Found once at startup, and again whenever [`refresh_profiles`] is asked.
///
/// Not a `LazyLock<Vec<_>>` alone, which is what it was: the popup makes
/// `~/.claude-<name>` and logs it in while cctop runs, and a list fixed at
/// startup left that account's sessions unlabelled, unresumable under it, and
/// unwalked until a restart.
///
/// Leaked slices behind the lock rather than a `Vec`, because callers hold
/// `&'static Profile`s and [`profile_for`] runs once per transcript path: a
/// read is a shared lock and a copy of two pointers, and no caller iterates
/// with the lock held. A slice is leaked only when a refresh finds something
/// different, so the leak grows with accounts added and removed rather than
/// with time, and the slice it replaces stays valid for whoever still holds a
/// reference into it — the same bargain [`LAUNCHABLE`] makes.
static FOUND: LazyLock<std::sync::RwLock<Found>> = LazyLock::new(|| {
    std::sync::RwLock::new(Found {
        mine: Box::leak(discover_mine().into_boxed_slice()),
        others: Box::leak(discover_others().into_boxed_slice()),
    })
});

fn found() -> Found {
    // Poisoned only by a panic between two pointer stores, after which each
    // pointer is still a whole, valid slice.
    *FOUND
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Look for profiles again, so one created while cctop runs is labelled,
/// resumable and walked without a restart.
///
/// Asked by the walk, through its roots, and by the limits poller. It costs a
/// readdir of each home in view per harness and a stat per `.claude*` or
/// `.codex*` entry — small beside the walk that follows, which reads every
/// project directory under those same profiles.
pub fn refresh_profiles() {
    // Looked for outside the lock, so a slow home never stalls a reader.
    let mine = discover_mine();
    let others = discover_others();
    let mut found = FOUND
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    adopt(&mut found.mine, mine);
    adopt(&mut found.others, others);
}

/// Replace `slot` with `fresh`, keeping the order `slot` already had.
///
/// Profiles still present stay where they were and new ones go on the end, so
/// a picker or a column never reshuffles under someone because an account was
/// added. One no longer found is dropped, which is what a restart would do: a
/// logged-out account must not go on being launched under. Answers whether
/// anything changed — and leaks only when it did.
fn adopt(slot: &mut &'static [Profile], fresh: Vec<Profile>) -> bool {
    let mut next: Vec<Profile> = slot.iter().filter(|p| fresh.contains(p)).cloned().collect();
    for profile in fresh {
        if !next.contains(&profile) {
            next.push(profile);
        }
    }
    if next.as_slice() == *slot {
        return false;
    }
    *slot = Box::leak(next.into_boxed_slice());
    true
}

/// One named profile of this user's, when it still exists.
///
/// A name is what gets remembered — in the prefs file, on a session row — and
/// a directory is what a launch needs. This turns one into the other, and
/// answers `None` for an account that has since been logged out of, so a stale
/// name cannot start an agent under somebody else's subscription.
pub fn profile_named(provider: Provider, name: &str) -> Option<&'static Profile> {
    found()
        .mine
        .iter()
        .find(|p| p.provider == provider && p.name == name)
}

/// This user's profiles for one harness, in the order the launcher offers them.
pub fn profiles_for(provider: Provider) -> Vec<&'static Profile> {
    found()
        .mine
        .iter()
        .filter(|p| p.provider == provider)
        .collect()
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

/// The names under `[accounts]` in cctop's config that carry a token.
fn token_account_names() -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(&*CONFIG_FILE) else {
        return Vec::new();
    };
    account_names_in(&text)
}

/// The same, from the file's text — the half that is worth testing.
fn account_names_in(text: &str) -> Vec<String> {
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    let Some(accounts) = doc.get("accounts").and_then(|a| a.as_table_like()) else {
        return Vec::new();
    };
    let mut out: Vec<String> = accounts
        .iter()
        .filter(|(_, item)| {
            // An empty token is a half-finished edit, and an account with none
            // at all is a table someone opened and did not fill: neither can be
            // polled, so neither earns a column that would only ever say
            // "not signed in".
            item.as_table_like()
                .and_then(|t| t.get("token"))
                .and_then(|t| t.as_str())
                .is_some_and(|t| !t.trim().is_empty())
        })
        .map(|(name, _)| name.to_string())
        .collect();
    // By name, as `profiles_in` sorts the directories: the limits panel draws a
    // column per account in this order, and one that followed the layout of a
    // hand-edited file would rearrange itself when the file was tidied.
    out.sort();
    out
}

/// cctop's own config, holding the account tokens a user added by hand.
///
/// Separate from `CACHE_DIR` on purpose: a cache is something cctop may delete
/// to recover, and `--clear-cache` does. A token the user typed in is not.
pub static CONFIG_FILE: LazyLock<PathBuf> =
    LazyLock::new(|| config_base().join("cctop").join("config.toml"));

/// `$CODEX_HOME`, falling back to `~/.codex`.
pub static CODEX_HOME: LazyLock<PathBuf> =
    LazyLock::new(|| env_dir("CODEX_HOME").unwrap_or_else(|| HOME.join(".codex")));

pub static CODEX_SESSIONS_ROOT: LazyLock<PathBuf> = LazyLock::new(|| CODEX_HOME.join("sessions"));

/// `$CURSOR_HOME`, falling back to `~/.cursor`.
pub static CURSOR_HOME: LazyLock<PathBuf> =
    LazyLock::new(|| env_dir("CURSOR_HOME").unwrap_or_else(|| HOME.join(".cursor")));

/// Cursor's native agent transcripts, grouped by project slug.
pub static CURSOR_PROJECTS_ROOT: LazyLock<PathBuf> = LazyLock::new(|| CURSOR_HOME.join("projects"));

/// `$PI_CODING_AGENT_DIR`, falling back to `~/.pi/agent`.
pub static PI_AGENT_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    env_dir("PI_CODING_AGENT_DIR").unwrap_or_else(|| HOME.join(".pi").join("agent"))
});

/// `$PI_CODING_AGENT_SESSION_DIR`, falling back to Pi's standard session root.
pub static PI_SESSIONS_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    env_dir("PI_CODING_AGENT_SESSION_DIR").unwrap_or_else(|| PI_AGENT_DIR.join("sessions"))
});

/// `$GEMINI_DIR`, falling back to `~/.gemini`.
pub static GEMINI_HOME: LazyLock<PathBuf> =
    LazyLock::new(|| env_dir("GEMINI_DIR").unwrap_or_else(|| HOME.join(".gemini")));

/// Gemini CLI files its chats under a scratch directory, one subtree per
/// project: `tmp/<project>/chats/session-*.json{,l}`. The name reads like
/// something disposable, but it is where the transcripts actually live.
pub static GEMINI_CHATS_ROOT: LazyLock<PathBuf> = LazyLock::new(|| GEMINI_HOME.join("tmp"));

/// Windsurf keeps per-workspace editor state where its VS Code base does.
///
/// `$WINDSURF_USER_DIR` overrides the whole `User` directory, which is what a
/// portable install moves; without it, the XDG config directory.
pub static WINDSURF_USER_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    if let Some(dir) = env_dir("WINDSURF_USER_DIR") {
        return dir;
    }
    config_base().join("Windsurf").join("User")
});

pub static WINDSURF_WORKSPACE_STORAGE: LazyLock<PathBuf> =
    LazyLock::new(|| WINDSURF_USER_DIR.join("workspaceStorage"));

/// OpenCode follows the platform data directory (`~/.local/share` on Linux).
pub static OPENCODE_DATA_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| env_dir("OPENCODE_DATA_DIR").unwrap_or_else(|| data_base().join("opencode")));

/// Devin CLI stores sessions in the platform data directory.
pub static DEVIN_CLI_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    env_dir("CHISEL_SESSION_DB")
        .and_then(|db_path| db_path.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| data_base().join("devin").join("cli"))
});

/// Devin CLI's user-level configuration directory — `~/.config/devin` — which
/// is a different directory from [`DEVIN_CLI_DIR`]: sessions and transcripts
/// are *data*, while AGENTS.md, config.json and mcp_config.json are *config*.
/// Confusing the two is how the Access panel once looked for a `config.toml`
/// that has never existed next to the session database.
pub static DEVIN_CONFIG_DIR: LazyLock<PathBuf> = LazyLock::new(|| config_base().join("devin"));

/// Devin's SQLite database containing session metadata.
pub static DEVIN_SESSIONS_DB: LazyLock<PathBuf> =
    LazyLock::new(|| DEVIN_CLI_DIR.join("sessions.db"));

/// Devin's transcript JSON files directory.
pub static DEVIN_TRANSCRIPTS_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| DEVIN_CLI_DIR.join("transcripts"));

/// Where OpenCode reads its own configuration and global plugins, which is the
/// *config* directory rather than the data one its sessions live in.
pub static OPENCODE_CONFIG_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    env_dir("OPENCODE_CONFIG_DIR").unwrap_or_else(|| config_base().join("opencode"))
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

pub static CACHE_DIR: LazyLock<PathBuf> = LazyLock::new(|| cache_base().join("cctop"));

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
    /// Login name, for the USER column, the `user:` filter and the tree's top
    /// level.
    pub user: String,
    /// Who owns the directory, which is how a process is tied back to it: a
    /// process carries a uid, never a login name or a home. Read off the
    /// directory rather than out of `passwd` because a home found by listing
    /// `/home` has no `passwd` line to read it from.
    pub uid: Option<u32>,
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

fn running_as_root() -> bool {
    // SAFETY: geteuid reads process state and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

/// Every person's home on the machine besides this user's, whether or not the
/// sweep is on.
///
/// Split from [`OTHER_HOMES`] because it answers two questions: which homes to
/// read, when the sweep is on, and which directories root must never write
/// into ([`foreign_to_root`]), which holds when it is off too.
static PEOPLES_HOMES: LazyLock<Vec<OtherHome>> = LazyLock::new(|| {
    let mut seen: HashSet<PathBuf> = HashSet::from([HOME.clone()]);
    let mut out = Vec::new();
    // `/etc/passwd` first, since it is the only source that pairs a home with
    // the login name that owns it.
    if let Ok(passwd) = std::fs::read_to_string("/etc/passwd") {
        for (home, user) in passwd_homes(&passwd) {
            push_home(&mut out, &mut seen, home, user);
        }
    }

    // ponytail: users served by LDAP, SSSD or another directory are not in
    // `/etc/passwd`, and enumerating them properly means getpwent(3) and a
    // libc call per entry — not something a static musl build can make answer
    // for NSS anyway. Listing `/home` catches them in the shape that actually
    // occurs: one directory per user, named after them.
    let parent = Path::new("/home");
    for name in list_dir(parent) {
        push_home(&mut out, &mut seen, parent.join(&name), name);
    }
    out
});

/// Every home besides this user's that cctop reads sessions out of.
///
/// Empty in the ordinary case, which is what keeps the single-user cost at
/// zero: each provider's root list is then just its own root, unchanged.
///
/// Read-only by construction: nothing cctop writes — its cache, its prefs, a
/// hook, a shell alias — is placed through this list. Those all hang off
/// [`HOME`], which for root is root's own.
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
    if all_users_wanted() {
        for other in PEOPLES_HOMES.iter() {
            if seen.insert(other.home.clone()) {
                out.push(other.clone());
            }
        }
    }
    out
});

/// Whether `path` is inside another person's home while cctop runs as root.
///
/// Only ever true for root: an unprivileged user's environment names their own
/// directories, and if it names someone else's the permissions already decide.
fn foreign_to_root(path: &Path) -> bool {
    running_as_root()
        && !path.starts_with(&*HOME)
        && PEOPLES_HOMES.iter().any(|o| path.starts_with(&o.home))
}

/// Lowest uid a login account gets, below which an entry is a service account.
///
/// The convention the distributions set in `/etc/login.defs`. Reading
/// `login.defs` to learn the local value would be more correct and would change
/// nothing: no `daemon` or `www-data` has ever run a coding agent, and a site
/// that lowered `UID_MIN` still keeps its service accounts below the default.
const UID_MIN: u32 = 1000;

/// One `passwd` line's `(name, uid, home)`.
///
/// Field 0 is the name, 2 the uid and 5 the home; a line with fewer fields is
/// a comment or a truncated write and is skipped rather than half-read.
fn passwd_entries(text: &str) -> impl Iterator<Item = (&str, u32, &str)> {
    text.lines().filter_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        let [user, _, uid, _, _, home, ..] = fields[..] else {
            return None;
        };
        let uid: u32 = uid.parse().ok()?;
        (!user.is_empty() && !home.is_empty()).then_some((user, uid, home))
    })
}

/// `(home, user)` for every `passwd` line belonging to a person.
///
/// Only root and the login accounts are kept — a system account's home exists,
/// is readable as root, and holds nothing, so sweeping the couple of dozen of
/// them is pure noise in the doctor report and pure stat calls at startup. A
/// home under `/home` counts as a person's whatever its uid, since a site that
/// numbers its people from 500 still gives them homes there.
fn passwd_homes(text: &str) -> Vec<(PathBuf, String)> {
    passwd_entries(text)
        .filter(|(_, uid, home)| *uid == 0 || *uid >= UID_MIN || home.starts_with("/home/"))
        .map(|(user, _, home)| (PathBuf::from(home), user.to_string()))
        .collect()
}

fn passwd_home_of(text: &str, uid: u32) -> Option<PathBuf> {
    passwd_entries(text)
        .find(|(_, u, _)| *u == uid)
        .map(|(_, _, home)| PathBuf::from(home))
}

fn passwd_name_of(text: &str, uid: u32) -> Option<String> {
    passwd_entries(text)
        .find(|(_, u, _)| *u == uid)
        .map(|(user, _, _)| user.to_string())
}

/// Record `home` as `user`'s if it is a real directory nobody claimed yet.
fn push_home(out: &mut Vec<OtherHome>, seen: &mut HashSet<PathBuf>, home: PathBuf, user: String) {
    use std::os::unix::fs::MetadataExt;
    // `/` is what the system accounts carry; taking it would put every path on
    // the machine under one "user" and make the sweep recurse the filesystem.
    if home.parent().is_none() || seen.contains(&home) {
        return;
    }
    let Ok(meta) = std::fs::metadata(&home) else {
        return;
    };
    if !meta.is_dir() {
        return;
    }
    seen.insert(home.clone());
    out.push(OtherHome {
        home,
        user,
        uid: Some(meta.uid()),
    });
}

/// The login name of whoever is running cctop, for the rows [`owner_of`]
/// leaves unnamed because they are this user's own.
///
/// Only asked for once other homes are in view — a table of one person's rows
/// never names them. `passwd` first, since `$USER` is whatever the shell said;
/// then `$USER`, for an account `passwd` does not list; then the bare uid,
/// which is at least not somebody else's name.
pub static MY_USER: LazyLock<String> = LazyLock::new(|| {
    // SAFETY: getuid reads process state and cannot fail.
    let uid = unsafe { libc::getuid() };
    std::fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|text| passwd_name_of(&text, uid))
        .or_else(|| std::env::var("USER").ok().filter(|u| !u.is_empty()))
        .unwrap_or_else(|| format!("uid {uid}"))
});

/// The name a row is shown and filtered under: its owner, or this user's own
/// name for a row that has none.
pub fn user_label(owner: Option<&str>) -> &str {
    owner.unwrap_or(MY_USER.as_str())
}

/// Whose sessions a process owned by `uid` may be matched to, in the terms of
/// [`Session::owner`](crate::session::Session::owner): `None` for this user's
/// own, else the owning home's user.
///
/// With no other homes in view every process cctop keeps is its user's own —
/// the process scan drops the rest — so the answer is always `None` and a
/// single-user machine pays nothing for the scoping.
pub fn owner_for_uid(uid: u32) -> Option<String> {
    // SAFETY: getuid reads process state and cannot fail.
    owner_among(uid, unsafe { libc::getuid() }, &OTHER_HOMES)
}

/// [`owner_for_uid`] with its inputs passed in, so it can be tested without
/// being root.
///
/// A uid that owns no home in view still gets a name, just not one any session
/// carries: its process then matches nothing and shows as a row of its own,
/// which is the truth — cctop can see the agent and not its transcript. Falling
/// back to `None` instead would hand it to *this* user's sessions in the same
/// directory.
pub fn owner_among(uid: u32, me: u32, homes: &[OtherHome]) -> Option<String> {
    if uid == me {
        return None;
    }
    Some(
        homes
            .iter()
            .find(|o| o.uid == Some(uid))
            .map(|o| o.user.clone())
            .unwrap_or_else(|| format!("uid {uid}")),
    )
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
    // Here because the walk asks for its roots every time it runs, so an
    // account made since the last walk is read on this one.
    refresh_profiles();
    let found = found();
    let mut roots: Vec<PathBuf> = found
        .mine
        .iter()
        .filter(|p| p.provider == provider)
        .map(|p| p.dir.join(leaf))
        .collect();
    for other in OTHER_HOMES.iter() {
        let theirs: Vec<&Profile> = found
            .others
            .iter()
            .filter(|p| p.provider == provider && p.dir.parent() == Some(other.home.as_path()))
            .collect();
        match theirs.as_slice() {
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

/// Devin's CLI directories — `sessions.db` beside `transcripts/` — across
/// homes. A directory rather than a transcript root, because a Devin session is
/// half database row and half transcript and both halves live in it.
pub fn devin_cli_dirs() -> Vec<PathBuf> {
    roots_across_homes(&DEVIN_CLI_DIR, |h| data_dir_in(h).join("devin").join("cli"))
}

/// The data directory *for another home*, which `dirs::data_dir` can only
/// answer for the calling user.
fn data_dir_in(home: &Path) -> PathBuf {
    home.join(".local").join("share")
}

fn config_dir_in(home: &Path) -> PathBuf {
    home.join(".config")
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
    found()
        .in_view()
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
            found()
                .in_view()
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
    // `get`, not a slice: the stem is any filename under the sessions root, and
    // 36 bytes from its end can land inside a multi-byte character.
    let tail = stem.get(stem.len().checked_sub(36)?..)?;
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

    /// The bug this closes: the popup logged `~/.claude-work` in while cctop
    /// ran, and its sessions stayed unlabelled until a restart because the
    /// profile list was read once. A refresh has to find it, keep the order a
    /// picker already showed, drop what was logged out of, and leave every
    /// reference handed out before it pointing at the account it named.
    #[test]
    fn a_profile_made_while_running_is_adopted_in_place() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path();
        let login = |name: &str| {
            let path = home.join(name);
            std::fs::create_dir_all(&path).expect("mkdir");
            std::fs::write(path.join(".credentials.json"), "{}").expect("write");
        };
        let names =
            |slot: &[Profile]| -> Vec<String> { slot.iter().map(|p| p.name.clone()).collect() };
        login(".claude");
        let mut slot: &'static [Profile] = &[];
        assert!(adopt(&mut slot, profiles_in(home, Provider::Claude)));
        let default: &'static Profile = &slot[0];
        // Nothing new is nothing leaked: the walk asks this every time.
        assert!(!adopt(&mut slot, profiles_in(home, Provider::Claude)));

        login(".claude-work");
        login(".claude-alpha");
        assert!(adopt(&mut slot, profiles_in(home, Provider::Claude)));
        // Appended in discovery order, not re-sorted in among the rest.
        assert_eq!(names(slot), ["default", "alpha", "work"]);
        login(".claude-beta");
        assert!(adopt(&mut slot, profiles_in(home, Provider::Claude)));
        assert_eq!(names(slot), ["default", "alpha", "work", "beta"]);

        std::fs::remove_file(home.join(".claude-work").join(".credentials.json")).expect("rm");
        assert!(adopt(&mut slot, profiles_in(home, Provider::Claude)));
        assert_eq!(names(slot), ["default", "alpha", "beta"]);
        assert_eq!(default.name, "default");
        assert_eq!(default.dir, home.join(".claude"));
    }

    /// The `[accounts]` table names the token-only accounts, and only the ones
    /// that could actually be polled: an empty token, or a table with none, is
    /// a half-finished edit rather than a second subscription.
    #[test]
    fn token_accounts_are_the_named_ones_with_a_token() {
        let text = "\
            [accounts.work]\n\
            token = \"sk-ant-oat01-w\"\n\
            [accounts.blank]\n\
            token = \"  \"\n\
            [accounts.empty]\n\
            [accounts.side]\n\
            token = \"sk-ant-oat01-s\"\n";
        assert_eq!(account_names_in(text), ["side", "work"]);
        assert!(account_names_in("").is_empty());
        assert!(account_names_in("not = toml [").is_empty());
        assert!(account_names_in("accounts = 3").is_empty());
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

    /// A person numbered below `UID_MIN` still has a home under `/home`, and
    /// is swept; a service account with its home elsewhere is not.
    #[test]
    fn passwd_parsing_keeps_a_low_uid_with_a_home_under_home() {
        let homes = passwd_homes(
            "old:x:500:500::/home/old:/bin/sh\n\
             svc:x:500:500::/srv/svc:/bin/sh\n",
        );
        assert_eq!(homes, vec![(PathBuf::from("/home/old"), "old".to_string())]);
    }

    /// Root's own home comes from `passwd`, not `$HOME`, which a `sudo` may
    /// have kept pointing at the invoking user's.
    #[test]
    fn passwd_answers_a_uid_with_its_home_and_name() {
        let text = "ana:x:1000:1000::/home/ana:/bin/zsh\n\
                    root:x:0:0:root:/var/root:/bin/sh\n";
        assert_eq!(passwd_home_of(text, 0), Some(PathBuf::from("/var/root")));
        assert_eq!(passwd_name_of(text, 1000).as_deref(), Some("ana"));
        assert_eq!(passwd_home_of(text, 42), None);
    }

    /// A home carries the uid that owns it, which is what ties a process to it.
    #[test]
    fn a_home_records_who_owns_it() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        push_home(
            &mut out,
            &mut HashSet::new(),
            dir.path().to_path_buf(),
            "me".into(),
        );
        let uid = std::fs::metadata(dir.path()).unwrap().uid();
        assert_eq!(out[0].uid, Some(uid));
        assert_eq!(owner_among(uid, uid, &out), None, "my own process is mine");
        assert_eq!(owner_among(uid, uid + 1, &out).as_deref(), Some("me"));
    }

    /// An unprivileged run never mistakes a directory for someone else's: the
    /// guard only exists for root, and the suite does not run as root.
    #[test]
    fn only_root_rejects_an_inherited_directory() {
        if running_as_root() {
            return;
        }
        let elsewhere = PathBuf::from("/home/somebody-else/.cache");
        assert_eq!(own_dir(Some(elsewhere.clone())), Some(elsewhere));
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

    /// Regression: any `.jsonl` under the Codex sessions root is offered here,
    /// and a stem whose 36th-from-last byte fell inside a multi-byte character
    /// panicked the slice — on a rayon worker, which takes the whole load down.
    #[test]
    fn uuid_extraction_survives_a_non_ascii_stem() {
        let stem = format!("é{}", "a".repeat(35));
        assert_eq!(trailing_uuid(&stem), None);
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
