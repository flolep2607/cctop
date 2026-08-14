//! `cctop doctor` — why is it not showing what you expected?
//!
//! Nearly every question cctop gets asked is one of a handful: my sessions are
//! missing, the costs are all zero, `s` does nothing, the other machine never
//! appeared. Each has a cause that is perfectly visible from inside the process
//! and completely invisible from the outside — a `CLAUDE_CONFIG_DIR` pointing
//! somewhere else, a pricing table that never downloaded, hooks installed
//! against a binary that has since moved.
//!
//! So this prints them. One line per check, worst news first within a section,
//! and a suggested fix attached to anything that is not fine — a diagnosis with
//! no next step just moves the question along.
//!
//! It is deliberately *not* the hooks panel with more in it. [`crate::hook`]
//! already answers "are the agents reporting", and that answer is quoted here
//! whole rather than reimplemented; the rest of these checks have never had a
//! home.

use crate::config;
use crate::pricing::Provider;
use std::io::IsTerminal;
use std::path::Path;

/// How bad a finding is.
///
/// The distinction that matters is the last one: `Fail` means cctop will do
/// something visibly wrong — miss sessions, price them at zero, drop a machine
/// you asked for. Everything a user might reasonably choose to live without is
/// a `Warn`, so that a clean run means clean rather than merely quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

impl Level {
    fn glyph(self) -> &'static str {
        match self {
            Level::Ok => "✓",
            Level::Warn => "!",
            Level::Fail => "✗",
        }
    }

    /// ANSI colour, or nothing when the output is not a terminal — doctor
    /// output gets pasted into issues, and escape codes there help nobody.
    fn colour(self, tty: bool) -> &'static str {
        match (tty, self) {
            (false, _) => "",
            (true, Level::Ok) => "\x1b[32m",
            (true, Level::Warn) => "\x1b[33m",
            (true, Level::Fail) => "\x1b[31m",
        }
    }
}

/// One finding: what was checked, what was found, and what to do about it.
pub struct Check {
    pub level: Level,
    pub label: String,
    pub detail: String,
    /// Shown indented under a `Warn` or `Fail`. `None` for anything that needs
    /// no action, which is every `Ok` and the handful of findings that are
    /// merely worth knowing.
    pub fix: Option<String>,
}

fn ok(label: impl Into<String>, detail: impl Into<String>) -> Check {
    Check {
        level: Level::Ok,
        label: label.into(),
        detail: detail.into(),
        fix: None,
    }
}

fn warn(label: impl Into<String>, detail: impl Into<String>, fix: &str) -> Check {
    Check {
        level: Level::Warn,
        label: label.into(),
        detail: detail.into(),
        fix: Some(fix.to_string()),
    }
}

fn fail(label: impl Into<String>, detail: impl Into<String>, fix: &str) -> Check {
    Check {
        level: Level::Fail,
        label: label.into(),
        detail: detail.into(),
        fix: Some(fix.to_string()),
    }
}

/// A titled group of findings.
pub struct Section {
    pub title: &'static str,
    pub checks: Vec<Check>,
}

/// Parse doctor's own arguments.
///
/// Its own, rather than clap's, because `doctor` is intercepted before clap
/// ever runs — cctop takes no positionals, so a subcommand cannot be declared
/// there. Unknown flags are refused rather than ignored: a mistyped `--hosts`
/// that silently checked nothing would be read as "the remote is fine".
fn parse_args(args: &[String]) -> Result<Vec<String>, String> {
    let mut hosts = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.split_once('=') {
            Some(("--host", value)) => hosts.push(value.to_string()),
            _ if arg == "--host" => match rest.next() {
                Some(value) => hosts.push(value.clone()),
                None => return Err("--host needs a value".into()),
            },
            _ => return Err(format!("unknown argument '{arg}'")),
        }
    }
    Ok(hosts)
}

/// Run every check and print the report. The return value is the exit code.
///
/// `--host` specs are checked by actually reading them: the ssh path has more
/// ways to fail than the rest of cctop put together, and the only honest test
/// of it is to make the round trip.
pub fn run(args: &[String]) -> i32 {
    let hosts = match parse_args(args) {
        Ok(hosts) => hosts,
        Err(why) => {
            eprintln!("cctop doctor: {why}");
            eprintln!("usage: cctop doctor [--host [user@]host[:command]]…");
            return 2;
        }
    };
    let hosts = &hosts;
    let sections = vec![
        cctop_itself(),
        environment(),
        sources(),
        pricing(),
        cache(),
        hooks(),
        typing(),
        remotes(hosts),
    ];

    let tty = std::io::stdout().is_terminal();
    let reset = if tty { "\x1b[0m" } else { "" };

    for section in &sections {
        if section.checks.is_empty() {
            continue;
        }
        println!("\n{}", section.title);
        for check in &section.checks {
            // An empty label means the detail is already a whole sentence and
            // should not be squeezed into a column — the hooks report writes
            // that way, and chopping its lines in two to fill a label misaligns
            // every one of them.
            match check.label.is_empty() {
                true => println!(
                    "  {}{}{reset} {}",
                    check.level.colour(tty),
                    check.level.glyph(),
                    check.detail
                ),
                false => println!(
                    "  {}{}{reset} {:<22} {}",
                    check.level.colour(tty),
                    check.level.glyph(),
                    check.label,
                    check.detail
                ),
            }
            if let Some(fix) = &check.fix {
                println!("      → {fix}");
            }
        }
    }

    let counts = |want: Level| {
        sections
            .iter()
            .flat_map(|s| &s.checks)
            .filter(|c| c.level == want)
            .count()
    };
    let (warns, fails) = (counts(Level::Warn), counts(Level::Fail));
    println!();
    match (fails, warns) {
        (0, 0) => println!("All checks passed."),
        (0, w) => println!("{w} warning(s). Nothing is broken."),
        (f, w) => println!("{f} problem(s), {w} warning(s)."),
    }

    // Non-zero only for `Fail`. A warning is a thing someone chose not to set
    // up, and a doctor that exits non-zero for those cannot be used in a script
    // to mean "is this installation sound".
    i32::from(fails > 0)
}

/// Ordered by severity, so a caller can take the worst of a set.
impl PartialOrd for Level {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Level {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        fn rank(l: Level) -> u8 {
            match l {
                Level::Ok => 0,
                Level::Warn => 1,
                Level::Fail => 2,
            }
        }
        rank(*self).cmp(&rank(*other))
    }
}

// ---------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------

fn cctop_itself() -> Section {
    let mut checks = vec![ok("version", crate::update::current_version())];

    checks.push(match std::env::current_exe() {
        Ok(path) => ok("binary", path.display().to_string()),
        // Not fatal — everything except the hook installer works fine without
        // knowing where the binary is — but it is the first thing to know when
        // hooks turn out to point somewhere unexpected.
        Err(e) => warn(
            "binary",
            format!("could not resolve this executable's path: {e}"),
            "hook installs record an absolute path, so they may be wrong",
        ),
    });

    // The cached answer only. A doctor that blocks on GitHub to tell you your
    // config is fine has misjudged what it is for.
    if let Some(latest) = crate::update::cached_latest_version()
        && latest != crate::update::current_version()
    {
        checks.push(warn(
            "update",
            format!("v{latest} is available"),
            "cctop --update",
        ));
    }
    Section {
        title: "cctop",
        checks,
    }
}

/// Environment variables that move where cctop looks.
///
/// Only the ones actually set. These are the single most common reason for "my
/// sessions are missing" — a `CLAUDE_CONFIG_DIR` left over from an experiment
/// sends discovery somewhere with nothing in it, and nothing else on screen
/// would ever mention it.
fn environment() -> Section {
    const VARS: &[&str] = &[
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "CURSOR_HOME",
        "GEMINI_DIR",
        "OPENCODE_DATA_DIR",
        "PI_CODING_AGENT_DIR",
        "PI_CODING_AGENT_SESSION_DIR",
        "WINDSURF_USER_DIR",
        "CCTOP_ALL_USERS",
        "CCTOP_HOMES",
        "CCTOP_HOSTS",
        "CCTOP_COLUMNS_HIDE",
    ];
    let checks = VARS
        .iter()
        .filter_map(|name| {
            let value = std::env::var(name).ok()?;
            Some(ok(*name, value))
        })
        .collect();
    Section {
        title: "Environment overrides",
        checks,
    }
}

/// Where each harness's sessions live, and how many were found.
///
/// A directory that is missing is reported as such rather than as zero
/// sessions: not having Windsurf installed is not a fault, and the two need to
/// look different or every report has five red lines in it.
fn sources() -> Section {
    let sessions = crate::session::list_all();
    let roots: &[(Provider, &str, std::path::PathBuf)] = &[
        (
            Provider::Claude,
            "Claude Code",
            config::CLAUDE_PROJECTS_ROOT.clone(),
        ),
        (
            Provider::Codex,
            "Codex",
            config::CODEX_SESSIONS_ROOT.clone(),
        ),
        (
            Provider::Cursor,
            "Cursor",
            config::CURSOR_PROJECTS_ROOT.clone(),
        ),
        (
            Provider::Gemini,
            "Gemini CLI",
            config::GEMINI_CHATS_ROOT.clone(),
        ),
        (
            Provider::OpenCode,
            "OpenCode",
            config::OPENCODE_DATA_DIR.clone(),
        ),
        (Provider::Pi, "Pi", config::PI_SESSIONS_ROOT.clone()),
        (
            Provider::Windsurf,
            "Windsurf",
            config::WINDSURF_USER_DIR.clone(),
        ),
    ];

    let mut checks: Vec<Check> = roots
        .iter()
        .map(|(provider, name, root)| {
            let found = sessions.iter().filter(|s| s.provider == *provider).count();
            // Sessions can come from another user's home while this one has no
            // such directory at all, which is the ordinary case for root.
            match (config::dir_exists(root) || found > 0, found) {
                (false, _) => ok(*name, format!("not installed ({})", root.display())),
                (true, 0) => warn(
                    *name,
                    format!(
                        "directory exists but holds no sessions ({})",
                        root.display()
                    ),
                    "if that is wrong, check the environment overrides above",
                ),
                (true, n) => ok(*name, format!("{n} session(s)")),
            }
        })
        .collect();

    let others = &*config::OTHER_HOMES;
    if !others.is_empty() {
        let names: Vec<&str> = others.iter().map(|o| o.user.as_str()).collect();
        checks.push(ok(
            "all users",
            format!(
                "also reading {} other home(s): {}",
                names.len(),
                names.join(", ")
            ),
        ));
    }

    if sessions.is_empty() {
        checks.push(warn(
            "total",
            "no sessions found in any harness",
            "run an agent once, then try again — cctop reads what they leave on disk",
        ));
    } else {
        let running = sessions.iter().filter(|s| s.is_running()).count();
        checks.push(ok(
            "total",
            format!("{} session(s), {running} running now", sessions.len()),
        ));
    }
    Section {
        title: "Session sources",
        checks,
    }
}

/// Whether costs will be real numbers.
///
/// The failure this exists for is silent: with no table and no cache every
/// session prices at `$0.00`, which looks like a well-behaved free plan rather
/// than like a missing download.
///
/// The check *is* the load. Nothing has loaded pricing by the time doctor runs
/// — only the interactive path does that — so asking [`crate::pricing`] whether
/// a table is installed would report "no" every time and say nothing about
/// whether one could be. Reading the cache is both the question and the answer.
fn pricing() -> Section {
    let path: &Path = &config::PRICING_CACHE_FILE;
    let fresh = crate::pricing::load_cached_pricing();
    let loaded = crate::pricing::pricing_epoch() != 0;
    let age = cache_age_secs(path);

    let checks = vec![match (loaded, fresh, age) {
        (true, true, _) => ok("LiteLLM table", "cached and fresh"),
        (true, false, Some(secs)) => warn(
            "LiteLLM table",
            format!(
                "cached but {} old",
                crate::util::long_duration((secs * 1000) as i64)
            ),
            "the next interactive run refreshes it; \
             costs use the stale rates until then",
        ),
        (true, false, None) => ok("LiteLLM table", "loaded"),
        (false, _, _) => fail(
            "LiteLLM table",
            format!("no usable pricing cache at {}", path.display()),
            "check network access to raw.githubusercontent.com; until it downloads, \
             models outside the built-in tables price at $0.00, \
             which is indistinguishable from a free plan",
        ),
    }];
    Section {
        title: "Pricing",
        checks,
    }
}

/// The extraction cache: where it is, how big, and whether it can be written.
///
/// Writability is a real `Fail`. cctop still works without it, but it re-parses
/// every transcript on every launch, which on a corpus of any size is the
/// difference between a second and a minute.
fn cache() -> Section {
    let dir: &Path = &config::CACHE_DIR;
    let mut checks = Vec::new();

    match writable(dir) {
        Ok(()) => checks.push(ok("directory", dir.display().to_string())),
        Err(e) => checks.push(fail(
            "directory",
            format!("{} is not writable: {e}", dir.display()),
            "without it every transcript is re-parsed on each launch",
        )),
    }

    let bytes = dir_size(dir);
    if bytes > 0 {
        checks.push(ok("size", crate::util::compact_bytes(bytes)));
    }
    Section {
        title: "Cache",
        checks,
    }
}

/// The hooks report, quoted from the module that owns it.
///
/// `crate::hook::status` already decides what is wrong with an install and how
/// to say it; restating that here would give two answers to one question. Only
/// the severity is added, and conservatively — an uninstalled hook is a feature
/// someone has not turned on.
fn hooks() -> Section {
    let cwd = std::env::current_dir().ok();
    let checks = crate::hook::status(cwd.as_deref(), None)
        .lines()
        .into_iter()
        .map(|(text, problem)| {
            // The listener is the socket the *UI* binds to receive events.
            // doctor is not the UI, so its absence here is a tautology rather
            // than a finding — reporting it as one sends people to reinstall
            // hooks that were never the problem.
            if text.starts_with("Listener") {
                return Check {
                    level: Level::Ok,
                    label: String::new(),
                    detail: "Listener: only runs inside the cctop UI, so not expected here".into(),
                    fix: None,
                };
            }
            Check {
                level: if problem { Level::Warn } else { Level::Ok },
                label: String::new(),
                detail: text,
                fix: problem.then(|| "cctop --install-hooks".to_string()),
            }
        })
        .collect();
    Section {
        title: "Agent hooks",
        checks,
    }
}

/// What `s` and `a` can reach on this machine.
///
/// Three backends with three different preconditions, none of which announces
/// itself: the answer to "why does `s` do nothing" is always one of these lines
/// and has never been printable before.
fn typing() -> Section {
    let mut checks = vec![
        match alias_installed() {
            true => ok(
                "shell aliases",
                "installed; agents started from a shell run under cctop",
            ),
            false => warn(
                "shell aliases",
                "not installed",
                "cctop --install-alias — or start agents as `cctop claude`; \
                 this is what lets `s` and `a` reach a session",
            ),
        },
        match crate::tmux::available() {
            true => ok(
                "tmux",
                "available; panes survive cctop and can be typed into",
            ),
            false => warn(
                "tmux",
                "not installed",
                "optional: without it cctop's tabs die with cctop, and tmux-hosted \
                 sessions cannot be typed into",
            ),
        },
    ];

    checks.extend(tiocsti_check());

    Section {
        title: "Typing into sessions",
        checks,
    }
}

/// The `TIOCSTI` backend's availability, where the kernel has one at all.
///
/// Split into two functions returning an `Option` rather than a `#[cfg]`'d
/// `push`, so the vector above is mutated on every platform. A `push` that only
/// compiles on Linux leaves `let mut checks` unused elsewhere, which `-D
/// warnings` rejects — the same trap as a `cfg`'d-out caller making its callee
/// dead code, and it only shows up on the macOS and Windows runners.
#[cfg(target_os = "linux")]
fn tiocsti_check() -> Option<Check> {
    Some(match is_root() {
        true => ok(
            "TIOCSTI",
            "running as root; sessions in plain terminals can be typed into",
        ),
        false => ok(
            "TIOCSTI",
            "unavailable (not root) — the last-resort backend only, \
             and not needed if the aliases above are installed",
        ),
    })
}

/// No `TIOCSTI` outside Linux, so there is nothing to report rather than a
/// line saying a backend this platform never had is missing.
#[cfg(not(target_os = "linux"))]
fn tiocsti_check() -> Option<Check> {
    None
}

/// Actually read every configured host.
///
/// The only check here that does real work, and the one most worth doing: ssh
/// has more ways to fail than everything above put together — a key that needs
/// a passphrase, a cctop that a non-interactive shell cannot find, a host that
/// no longer resolves — and none of them are visible until the table stays
/// empty. Failing outright is right: these were asked for by name.
fn remotes(hosts: &[String]) -> Section {
    let checks = crate::fleet::Host::collect(hosts)
        .iter()
        .map(|host| match host.poll() {
            crate::fleet::Snapshot::Rows(rows) => ok(
                host.target.clone(),
                format!("{} session(s) via `{}`", rows.len(), host.command),
            ),
            crate::fleet::Snapshot::Failed(why) => {
                fail(host.target.clone(), why.clone(), remote_hint(&why))
            }
        })
        .collect();
    Section {
        title: "Remote hosts",
        checks,
    }
}

/// The fix to suggest for one host's failure.
///
/// Matched on ssh's own words rather than given one generic line, because the
/// generic line is wrong most of the time and a wrong suggestion costs more
/// than none: pointing someone at their `PATH` when the hostname did not
/// resolve sends them to edit a file that was never involved.
fn remote_hint(why: &str) -> &'static str {
    let lower = why.to_ascii_lowercase();
    if lower.contains("not found") || lower.contains("no such file") {
        return "a non-interactive ssh gets a different PATH than a login shell; \
                name the binary with --host host:/path/to/cctop";
    }
    if lower.contains("permission denied") || lower.contains("publickey") {
        return "ssh runs with BatchMode=yes, so a key that needs a passphrase \
                cannot be used; add it to an agent or use one without";
    }
    if lower.contains("resolve") || lower.contains("timed out") || lower.contains("refused") {
        return "cctop passes the target to ssh unchanged — if `ssh <host>` does not \
                work by hand, it will not work here either";
    }
    if lower.contains("host key") || lower.contains("known_hosts") {
        return "BatchMode=yes will not accept a new host key; \
                ssh to it once by hand first";
    }
    "run the same command by hand to see it in full: ssh <host> cctop --json"
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Seconds since a file was last written, or `None` if it is not there.
fn cache_age_secs(path: &Path) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.elapsed().ok()?.as_secs())
}

/// Prove the directory can be written, by writing.
///
/// A permissions check would be a guess: the directory may not exist yet, may
/// be on a read-only mount, or may be owned by the root that ran cctop once.
/// Creating and removing a file answers all three at once.
fn writable(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let probe = dir.join(format!(".cctop-doctor-{}", std::process::id()));
    std::fs::write(&probe, b"").map_err(|e| e.to_string())?;
    std::fs::remove_file(&probe).map_err(|e| e.to_string())
}

/// Bytes held by a directory, one level deep.
///
/// The cache is flat, so recursing would only add a way to spend a long time in
/// a directory someone has pointed at their home by mistake.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Whether a shell startup file carries cctop's managed block.
///
/// Duplicated from [`crate::alias`] rather than exported: that one is private
/// because nothing outside the module had a reason to ask, and the marker is
/// the stable part of the contract in any case.
fn alias_installed() -> bool {
    let home = &*config::HOME;
    let fish = home.join(".config/fish/conf.d/cctop.fish");
    if fish.is_file() {
        return true;
    }
    [".zshrc", ".bashrc"].iter().any(|name| {
        std::fs::read_to_string(home.join(name)).is_ok_and(|t| t.contains("# >>> cctop >>>"))
    })
}

#[cfg(target_os = "linux")]
fn is_root() -> bool {
    // SAFETY: `geteuid` reads a field of the calling process and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exit code is the whole contract for anything scripting this: zero
    /// means sound, and a warning is not unsound. A doctor that failed on "you
    /// have not installed the optional thing" could not be used in CI.
    #[test]
    fn only_a_real_problem_is_worth_a_non_zero_exit() {
        assert!(Level::Fail > Level::Warn);
        assert!(Level::Warn > Level::Ok);
    }

    /// A wrong suggestion costs more than none — it sends someone to edit a
    /// file that was never involved. Each of ssh's common refusals has a
    /// different cause and has to get its own.
    #[test]
    fn each_ssh_failure_gets_the_fix_that_matches_it() {
        let hint = |why: &str| remote_hint(why);
        assert!(hint("bash: cctop: command not found").contains("PATH"));
        assert!(hint("Permission denied (publickey).").contains("passphrase"));
        assert!(
            hint("ssh: Could not resolve hostname box: Name or service not known")
                .contains("by hand")
        );
        assert!(hint("Host key verification failed.").contains("host key"));
        // Anything unrecognised still says something actionable rather than
        // repeating the error back.
        assert!(hint("something new and strange").contains("ssh <host> cctop --json"));
    }

    #[test]
    fn a_missing_file_has_no_age_and_a_written_one_does() {
        let dir = std::env::temp_dir().join(format!("cctop-doctor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        assert!(cache_age_secs(&dir.join("absent")).is_none());

        let file = dir.join("present");
        std::fs::write(&file, b"x").expect("write");
        assert!(cache_age_secs(&file).is_some());

        // And the writability probe leaves nothing behind, which matters
        // because it runs against the user's real cache directory.
        assert!(writable(&dir).is_ok());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        assert_eq!(dir_size(&dir), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// An unwritable cache is the one storage fault worth failing on, so the
    /// probe has to actually report it rather than assume the happy path.
    #[test]
    fn an_unwritable_directory_is_reported() {
        // A path whose parent is a *file* cannot be created, on every platform,
        // which is the portable way to ask for a directory that will not work.
        let file = std::env::temp_dir().join(format!("cctop-doctor-file-{}", std::process::id()));
        std::fs::write(&file, b"x").expect("write");
        assert!(writable(&file.join("under-a-file")).is_err());
        std::fs::remove_file(&file).ok();
    }
}
