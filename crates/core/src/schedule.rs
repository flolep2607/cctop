//! Agents that start on their own, at times the person wrote down.
//!
//! A schedule is a `[[schedule]]` entry in cctop's `config.toml`: a name, a
//! systemd calendar expression, a directory, a harness and a prompt. It is run
//! by a systemd user timer that `cctop schedule sync` writes, one per entry,
//! and the timer runs `cctop schedule run <name>` — so a schedule fires
//! whether or not any cctop is open, and survives a reboot as any timer does.
//!
//! systemd rather than a timer of cctop's own, because the dashboard is the
//! thing that is *not* running at 08:00: a scheduler that lived in it would
//! fire only on mornings someone had already opened it. And rather than cron,
//! because a timer catches up on a run the machine was off for
//! (`Persistent=true`), says when it will fire next, and keeps a journal of
//! what happened when it did — all things `cctop schedule list` reads back.
//!
//! What a run starts is exactly what [`drive`](crate::drive)'s `spawn_agent`
//! starts, built by the same [`launch_argv`](crate::drive::launch_argv): the
//! harness, the prompt as one argument, an account and a model if named, and
//! nothing that widens what the agent may do. There is no field for a
//! permission mode, by design: an agent started at 3 a.m. with nobody watching
//! is the last one that should run without asking, and one that asks waits in
//! its tab, with a push to say it started.
//!
//! The agent is a tab in cctop's rmux, named for the schedule, so it is in
//! every cctop's tab bar on the machine — whichever is opened first finds it
//! already working. A run whose previous run is still going is skipped rather
//! than stacked: two agents of one schedule in one directory are two agents
//! editing the same files.
//!
//! The unit files are cctop's alone: each starts with [`MARKER`], and `sync`
//! touches no file in the user's unit directory that does not.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// The first line of every unit `sync` writes, and how it knows its own.
pub const MARKER: &str = "# Written by `cctop schedule sync` from [[schedule]] in cctop's config.toml; edit that instead.";

/// What every schedule's units and rmux tab are called after.
const UNIT_PREFIX: &str = "cctop-schedule-";

/// Set on a scheduled agent's environment: which schedule started it.
pub const SCHEDULE_ENV: &str = "CCTOP_SCHEDULE";

/// One `[[schedule]]` entry, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    pub name: String,
    /// A systemd `OnCalendar=` expression, such as `Mon..Fri 08:00`.
    pub when: String,
    pub dir: String,
    pub harness: String,
    pub account: Option<String>,
    pub model: Option<String>,
    pub prompt: String,
    pub enabled: bool,
}

impl Schedule {
    /// The `.service` and `.timer` this schedule is installed as, and the
    /// rmux session its agent runs in — one name for all three, so the tab
    /// says which timer started it.
    pub fn unit(&self) -> String {
        format!("{UNIT_PREFIX}{}", self.name)
    }
}

/// Every entry of the file, and what was wrong with the ones left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Schedules {
    pub entries: Vec<Schedule>,
    /// One sentence per entry that could not be used. Reported, not fatal: a
    /// typo in one schedule must not stop the others from running.
    pub problems: Vec<String>,
}

impl Schedules {
    pub fn load() -> Schedules {
        Schedules::load_from(&crate::config::CONFIG_FILE)
    }

    pub fn load_from(path: &Path) -> Schedules {
        std::fs::read_to_string(path)
            .map(|text| Schedules::parse(&text))
            .unwrap_or_default()
    }

    pub fn parse(text: &str) -> Schedules {
        let mut out = Schedules::default();
        let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
            // `Settings` already says the file is not TOML; once is enough.
            return out;
        };
        let Some(item) = doc.get("schedule") else {
            return out;
        };
        let Some(tables) = item.as_array_of_tables() else {
            out.problems
                .push("`schedule` is not a list of [[schedule]] tables".into());
            return out;
        };
        let mut seen = HashSet::new();
        for (i, table) in tables.iter().enumerate() {
            match entry(table) {
                Ok(schedule) if !seen.insert(schedule.name.clone()) => out.problems.push(format!(
                    "[[schedule]] {:?} is named twice; only the first is used",
                    schedule.name
                )),
                Ok(schedule) => out.entries.push(schedule),
                Err(why) => {
                    let name = table
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map_or_else(|| format!("number {}", i + 1), |n| format!("{n:?}"));
                    out.problems.push(format!("[[schedule]] {name}: {why}"));
                }
            }
        }
        out
    }

    pub fn named(&self, name: &str) -> Option<&Schedule> {
        self.entries.iter().find(|s| s.name == name)
    }
}

/// One table, as a schedule or the first reason it is not one.
fn entry(table: &toml_edit::Table) -> Result<Schedule, String> {
    let text = |key: &str| -> Result<Option<String>, String> {
        match table.get(key) {
            None => Ok(None),
            Some(item) => item
                .as_str()
                .map(|s| Some(s.trim().to_string()))
                .ok_or_else(|| format!("`{key}` is not a string")),
        }
    };
    let needed = |key: &str| -> Result<String, String> {
        text(key)?
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("`{key}` is missing"))
    };
    for key in table.iter().map(|(k, _)| k) {
        if !matches!(
            key,
            "name" | "when" | "dir" | "harness" | "account" | "model" | "prompt" | "enabled"
        ) {
            return Err(format!(
                "`{key}` is not a schedule setting (there is deliberately none for permissions)"
            ));
        }
    }
    let name = needed("name")?;
    check_name(&name)?;
    let when = needed("when")?;
    check_when(&when)?;
    let harness = needed("harness")?;
    if !crate::drive::SPAWNABLE.contains(&harness.as_str()) {
        return Err(format!(
            "{harness:?} is not a harness cctop can start with a prompt; it starts {}",
            crate::drive::SPAWNABLE.join(", ")
        ));
    }
    let prompt = needed("prompt")?;
    crate::drive::check_prompt(&prompt)?;
    let model = text("model")?.filter(|m| !m.is_empty());
    if let Some(model) = &model {
        crate::launch_opts::Options {
            model: Some(model.clone()),
            ..Default::default()
        }
        .flags(&harness)?;
    }
    let enabled = match table.get("enabled") {
        None => true,
        Some(item) => item.as_bool().ok_or("`enabled` is not true or false")?,
    };
    Ok(Schedule {
        name,
        when,
        dir: needed("dir")?,
        harness,
        account: text("account")?.filter(|a| !a.is_empty()),
        model,
        prompt,
        enabled,
    })
}

/// A name that is safe as a unit file name and an rmux session name as it is:
/// a name that needed escaping in either would be a different name there.
fn check_name(name: &str) -> Result<(), String> {
    let ok = name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    match ok {
        true => Ok(()),
        false => Err(format!(
            "the name {name:?} has to be letters, digits, '-' and '_', at most 40 of them"
        )),
    }
}

/// The calendar expression, as far as it can be checked without systemd.
///
/// It is written into a unit file, where a line break would start a directive
/// of the writer's choosing and `%` is a specifier systemd expands; neither
/// has a place in a calendar expression. Whether systemd can read the rest is
/// asked of `systemd-analyze` at sync time, where there is a systemd to ask.
fn check_when(when: &str) -> Result<(), String> {
    if when
        .chars()
        .any(|c| c.is_control() || c == '%' || c == '\\')
    {
        return Err(format!("{when:?} is not a calendar expression"));
    }
    Ok(())
}

/// What a run does instead of starting the agent, if anything.
///
/// By hand (`--now`) a disabled schedule still runs — the person asked for
/// this one — but a schedule whose last agent is still at work never doubles
/// up, whoever asks.
pub fn skip_reason(schedule: &Schedule, still_running: bool, by_hand: bool) -> Option<String> {
    if still_running {
        return Some(format!(
            "{}'s previous run is still going (tab {}); not starting a second agent on the \
             same directory",
            schedule.name,
            schedule.unit()
        ));
    }
    if !schedule.enabled && !by_hand {
        return Some(format!(
            "{} is disabled in config.toml; run it by hand with --now",
            schedule.name
        ));
    }
    None
}

/// What a run starts: the argv, the directory and the environment.
pub fn plan(schedule: &Schedule) -> Result<crate::drive::Plan, String> {
    let dir = crate::drive::check_dir(&schedule.dir)?;
    let argv = crate::drive::launch_argv(
        &schedule.harness,
        &schedule.prompt,
        schedule.account.as_deref(),
        schedule.model.as_deref(),
    )?;
    Ok(crate::drive::Plan {
        harness: schedule.harness.clone(),
        argv,
        env: vec![format!("{SCHEDULE_ENV}={}", schedule.name)],
        dir,
    })
}

/// Run `name` now: start its agent in cctop's rmux, unless [`skip_reason`]
/// says not to. Answers what happened, in a sentence.
pub fn run(name: &str, by_hand: bool) -> Result<String, String> {
    let schedules = Schedules::load();
    let Some(schedule) = schedules.named(name) else {
        let why = schedules
            .problems
            .iter()
            .find(|p| p.contains(&format!("{name:?}")))
            .cloned()
            .unwrap_or_else(|| format!("no [[schedule]] named {name:?} in config.toml"));
        return Err(why);
    };
    let tab = schedule.unit();
    let result = match skip_reason(schedule, crate::rmux::live(&tab), by_hand) {
        Some(why) => Ok(format!("skipped: {why}")),
        None => start(schedule, &tab),
    };
    crate::elog::event(
        "schedule",
        "run",
        serde_json::json!({
            "schedule": name,
            "by_hand": by_hand,
            "outcome": match &result { Ok(said) => said.clone(), Err(why) => why.clone() },
        }),
    );
    result
}

fn start(schedule: &Schedule, tab: &str) -> Result<String, String> {
    let plan = plan(schedule)?;
    if !crate::actions::agents().contains(&plan.harness) {
        return Err(format!("{} is not installed on this machine", plan.harness));
    }
    // A test that got this far would start a real agent, billed to whoever
    // runs the suite.
    if crate::under_test() {
        return Err("not starting a real agent under test".into());
    }
    crate::rmux::start_detached_with(&plan.argv, tab, Some(&plan.dir), &plan.env)
        .map_err(|why| format!("could not start {}: {why}", plan.harness))?;
    crate::rmux::set_label(tab, &schedule.name);
    announce(schedule, &plan.dir);
    Ok(format!(
        "started {} in {} as tab {tab}",
        schedule.harness,
        crate::util::tildify(&plan.dir.to_string_lossy())
    ))
}

/// Tell the person's phone a scheduled agent started.
///
/// Through the browsers subscribed on `cctop serve`'s page, read from the same
/// sealed store. Sent inline, not on [`push_all`](crate::webpush::push_all)'s
/// thread: this process exits as soon as the agent is up, and would take the
/// thread with it. Nobody subscribed is the usual case, and costs nothing.
fn announce(schedule: &Schedule, dir: &Path) {
    let store = crate::secrets::Store::current();
    let subscriptions = crate::webpush::load_subscriptions(&store);
    let Some(vapid) = crate::webpush::Vapid::load(&store) else {
        return;
    };
    let payload = crate::webpush::Payload {
        title: format!("Scheduled: {}", schedule.name),
        body: format!(
            "{} started in {}",
            schedule.harness,
            crate::util::tildify(&dir.to_string_lossy())
        ),
        // No session id yet — the transcript is written after the first
        // turn — so the tap opens the table, where the new tab's row is.
        session: String::new(),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    for subscription in subscriptions {
        if let Ok(request) = crate::webpush::request(&vapid, &subscription, &payload, now) {
            let _ = crate::webpush::send(&request);
        }
    }
}

// ---------------------------------------------------------------------------
// The systemd units
// ---------------------------------------------------------------------------

/// The `.service` that runs `schedule`: `exe schedule run <name>`.
///
/// `PATH` is the one piece of the person's environment carried over, because
/// the user manager's is a bare system one and the harnesses live in
/// `~/.local/bin` and the like; without it `claude` is "not installed" at
/// 08:00 and installed at 08:01. `XDG_RUNTIME_DIR` is `%t`, the manager's
/// own, which is where cctop's rmux keeps its socket.
pub fn service_text(schedule: &Schedule, exe: &Path, path: &str) -> String {
    format!(
        "{MARKER}\n\
         [Unit]\n\
         Description=cctop scheduled agent: {name}\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         Environment={path_env}\n\
         Environment=XDG_RUNTIME_DIR=%t\n\
         ExecStart={exe} schedule run {name}\n",
        name = schedule.name,
        path_env = quote(&format!("PATH={path}")),
        exe = quote(&exe.to_string_lossy()),
    )
}

/// The `.timer` that fires `schedule`'s service on its calendar.
///
/// `Persistent=true`: a laptop that was shut at 08:00 runs the morning's agent
/// when it next wakes, once, rather than skipping the day.
pub fn timer_text(schedule: &Schedule) -> String {
    format!(
        "{MARKER}\n\
         [Unit]\n\
         Description=cctop schedule {name} ({when})\n\
         \n\
         [Timer]\n\
         OnCalendar={when}\n\
         Persistent=true\n\
         Unit={unit}.service\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n",
        name = schedule.name,
        when = schedule.when,
        unit = schedule.unit(),
    )
}

/// A value as one word of a unit file: double-quoted, with what systemd
/// reads inside quotes escaped — the quote, the backslash, and `%`, which it
/// expands as a specifier and would turn `/home/a%20b` into something else.
fn quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

/// The user's systemd unit directory.
pub fn unit_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| crate::config::HOME.join(".config"))
        .join("systemd/user")
}

/// Asks systemd something: `systemctl --user <args>` in the real one, a
/// recording stand-in in the tests, which must never reach the user's systemd.
pub type Ctl<'a> = &'a mut dyn FnMut(&[&str]) -> Result<String, String>;

/// `systemctl --user`, answering stdout or why it failed.
pub fn systemctl(args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|e| format!("systemctl could not be run: {e}"))?;
    match out.status.success() {
        true => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        false => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
    }
}

/// Whether there is a user systemd to install timers into, or why not.
///
/// "degraded" counts: it means some unrelated unit failed, and timers still
/// run. A WSL without systemd, or a session with no user manager, answers an
/// error, and that error is what the person is told.
pub fn available(ctl: Ctl) -> Result<(), String> {
    match ctl(&["is-system-running"]) {
        Ok(_) => Ok(()),
        Err(why) if why.is_empty() || why.contains("degraded") => Ok(()),
        Err(why) => Err(format!(
            "there is no user systemd to run schedules ({why}); on WSL, set `systemd=true` \
             under [boot] in /etc/wsl.conf and restart it"
        )),
    }
}

/// `systemd-analyze calendar` on `when`, if there is one: `Err` with its
/// complaint when it cannot read it.
pub fn analyze(when: &str) -> Result<(), String> {
    let Ok(out) = std::process::Command::new("systemd-analyze")
        .args(["calendar", "--", when])
        .output()
    else {
        return Ok(());
    };
    match out.status.success() {
        true => Ok(()),
        false => Err(format!(
            "systemd cannot read {when:?} as a time: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

/// What a sync changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Synced {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
    pub problems: Vec<String>,
}

impl Synced {
    pub fn changed(&self) -> bool {
        !(self.added.is_empty() && self.updated.is_empty() && self.removed.is_empty())
    }

    /// One line for a status bar or a terminal.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        for (what, names) in [
            ("installed", &self.added),
            ("updated", &self.updated),
            ("removed", &self.removed),
        ] {
            if !names.is_empty() {
                parts.push(format!("{what} {}", names.join(", ")));
            }
        }
        if parts.is_empty() {
            parts.push("schedules already up to date".into());
        }
        parts.extend(self.problems.iter().cloned());
        parts.join("; ")
    }
}

/// Make the timers in `dir` match `schedules`: write the enabled ones' units,
/// remove the units of schedules that are gone or disabled, and tell systemd.
///
/// Only files that start with [`MARKER`] are cctop's. A file of a schedule's
/// name without it is the person's, and is reported rather than overwritten.
pub fn sync(
    schedules: &[Schedule],
    dir: &Path,
    exe: &Path,
    path: &str,
    check_when: &dyn Fn(&str) -> Result<(), String>,
    ctl: Ctl,
) -> Result<Synced, String> {
    available(ctl)?;
    let mut out = Synced::default();
    let mut wanted: BTreeMap<String, String> = BTreeMap::new();
    let mut timers = Vec::new();
    for schedule in schedules.iter().filter(|s| s.enabled) {
        if let Err(why) = check_when(&schedule.when) {
            out.problems.push(format!("{}: {why}", schedule.name));
            continue;
        }
        let unit = schedule.unit();
        wanted.insert(format!("{unit}.service"), service_text(schedule, exe, path));
        wanted.insert(format!("{unit}.timer"), timer_text(schedule));
        timers.push((schedule.name.clone(), format!("{unit}.timer")));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;

    let ours =
        |file: &Path| std::fs::read_to_string(file).is_ok_and(|text| text.starts_with(MARKER));
    for (file, text) in &wanted {
        let at = dir.join(file);
        let name = file
            .strip_prefix(UNIT_PREFIX)
            .and_then(|f| f.rsplit_once('.'))
            .map_or(file.as_str(), |(n, _)| n)
            .to_string();
        match std::fs::read_to_string(&at) {
            Ok(old) if old == *text => continue,
            Ok(_) if !ours(&at) => {
                out.problems.push(format!(
                    "{} is not cctop's (it lacks cctop's first line); left as it is",
                    at.display()
                ));
                continue;
            }
            Ok(_) => push_once(&mut out.updated, &name),
            Err(_) => push_once(&mut out.added, &name),
        }
        crate::util::write_atomic(&at, text.as_bytes(), false)
            .map_err(|e| format!("{}: {e}", at.display()))?;
    }

    let mut gone = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let Ok(entry) = entry else { continue };
        let file = entry.file_name().to_string_lossy().into_owned();
        if !file.starts_with(UNIT_PREFIX) || wanted.contains_key(&file) || !ours(&entry.path()) {
            continue;
        }
        if file.ends_with(".timer") {
            // Stopped before the file goes, so systemd is not left holding a
            // timer whose unit it can no longer read.
            let _ = ctl(&["disable", "--now", &file]);
        }
        gone.push(entry.path());
        if let Some((name, _)) = file
            .strip_prefix(UNIT_PREFIX)
            .and_then(|f| f.rsplit_once('.'))
        {
            push_once(&mut out.removed, name);
        }
    }
    for file in gone {
        let _ = std::fs::remove_file(file);
    }

    if out.changed() {
        ctl(&["daemon-reload"])?;
    }
    for (name, timer) in timers {
        // Every wanted timer, not only the fresh ones: one stopped by hand, or
        // never started because a sync failed halfway, is started again.
        if let Err(why) = ctl(&["enable", "--now", &timer]) {
            out.problems
                .push(format!("{name}: could not start its timer: {why}"));
        }
    }
    Ok(out)
}

fn push_once(list: &mut Vec<String>, name: &str) {
    if !list.iter().any(|n| n == name) {
        list.push(name.to_string());
    }
}

/// [`sync`] against the real systemd and this cctop's own binary.
pub fn sync_now(schedules: &[Schedule]) -> Result<Synced, String> {
    let exe = std::env::current_exe().map_err(|e| format!("where is cctop? {e}"))?;
    let path = std::env::var("PATH").unwrap_or_default();
    sync(
        schedules,
        &unit_dir(),
        &exe,
        &path,
        &analyze,
        &mut |args: &[&str]| systemctl(args),
    )
}

/// Sync if the schedules or this binary changed since the last sync this
/// machine did, and say what changed — `None` when nothing needed doing.
///
/// What the dashboard and `cctop serve` call when they read `config.toml`, so a
/// schedule written into the file is installed without a second command. A
/// machine with no schedules, and none installed, never asks systemd anything.
pub fn sync_if_changed() -> Option<String> {
    if crate::under_test() {
        return None;
    }
    let schedules = Schedules::load();
    let exe = std::env::current_exe().ok()?;
    let fingerprint = format!(
        "{:?}|{}|{}",
        schedules.entries,
        exe.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let stamp = crate::config::CACHE_DIR.join("schedule-synced");
    if std::fs::read_to_string(&stamp).is_ok_and(|old| old == fingerprint) {
        return None;
    }
    let installed = std::fs::read_dir(unit_dir()).is_ok_and(|mut entries| {
        entries.any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().starts_with(UNIT_PREFIX)))
    });
    if schedules.entries.is_empty() && !installed {
        let _ = std::fs::write(&stamp, &fingerprint);
        return None;
    }
    let said = match sync_now(&schedules.entries) {
        Ok(synced) => {
            let _ = std::fs::write(&stamp, &fingerprint);
            synced
                .changed()
                .then(|| format!("Schedules: {}", synced.summary()))?
        }
        Err(why) => format!("Schedules not installed: {why}"),
    };
    Some(said)
}

/// When a schedule's timer fires next and last fired, as systemd says them —
/// `None` for either it has no answer to.
pub fn timer_times(schedule: &Schedule, ctl: Ctl) -> (Option<String>, Option<String>) {
    let timer = format!("{}.timer", schedule.unit());
    let Ok(out) = ctl(&[
        "show",
        &timer,
        "-p",
        "NextElapseUSecRealtime",
        "-p",
        "LastTriggerUSec",
    ]) else {
        return (None, None);
    };
    let field = |key: &str| {
        out.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(str::trim)
            .filter(|v| !v.is_empty() && *v != "n/a")
            .map(str::to_string)
    };
    (field("NextElapseUSecRealtime"), field("LastTriggerUSec"))
}

/// Flip `enabled` on schedule `name` in the file at `path`, keeping everything
/// else in the file — comments, layout, account tokens — as it was.
pub fn set_enabled(path: &Path, name: &str, on: bool) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let mut doc = crate::quota::parse_toml(path, &text)?;
    let tables = doc
        .get_mut("schedule")
        .and_then(|item| item.as_array_of_tables_mut())
        .ok_or_else(|| anyhow::anyhow!("no [[schedule]] in {}", path.display()))?;
    let table = tables
        .iter_mut()
        .find(|t| t.get("name").and_then(|n| n.as_str()) == Some(name))
        .ok_or_else(|| anyhow::anyhow!("no [[schedule]] named {name:?}"))?;
    match on {
        // On is the default, so turning one back on takes the line out
        // rather than leaving `enabled = true` to say what goes without saying.
        true => {
            table.remove("enabled");
        }
        false => {
            table.insert("enabled", toml_edit::value(false));
        }
    }
    crate::util::write_atomic(path, doc.to_string().as_bytes(), true)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// `cctop schedule …`
// ---------------------------------------------------------------------------

/// `cctop schedule list | sync | run <name> [--now]`.
pub fn run_cli(argv: &[String]) -> i32 {
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] | ["list"] => {
            let schedules = Schedules::load();
            if schedules.entries.is_empty() && schedules.problems.is_empty() {
                println!(
                    "No schedules. Add a [[schedule]] to {} — see `cctop schedule help`.",
                    crate::util::tildify(&crate::config::CONFIG_FILE.to_string_lossy())
                );
                return 0;
            }
            let mut ctl = |a: &[&str]| systemctl(a);
            for s in &schedules.entries {
                let (next, last) = timer_times(s, &mut ctl);
                println!(
                    "{:<20} {:<22} {:<9} {}",
                    s.name,
                    s.when,
                    s.harness,
                    crate::util::tildify(&s.dir)
                );
                let state = match s.enabled {
                    true => format!("next {}", next.as_deref().unwrap_or("not installed")),
                    false => "disabled".to_string(),
                };
                println!(
                    "{:<20} {state} · last {}",
                    "",
                    last.as_deref().unwrap_or("never")
                );
            }
            for problem in &schedules.problems {
                eprintln!("{problem}");
            }
            0
        }
        ["sync"] => {
            let schedules = Schedules::load();
            for problem in &schedules.problems {
                eprintln!("{problem}");
            }
            match sync_now(&schedules.entries) {
                Ok(synced) => {
                    println!("{}", synced.summary());
                    i32::from(!synced.problems.is_empty())
                }
                Err(why) => {
                    eprintln!("{why}");
                    1
                }
            }
        }
        ["run", name] | ["run", name, "--now"] | ["run", "--now", name] => {
            let by_hand = args.contains(&"--now");
            match run(name, by_hand) {
                Ok(said) => {
                    println!("{said}");
                    0
                }
                Err(why) => {
                    eprintln!("{why}");
                    1
                }
            }
        }
        ["help" | "--help" | "-h"] => {
            println!("{USAGE}");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

const USAGE: &str = "usage: cctop schedule [list | sync | run NAME [--now]]

A schedule is a [[schedule]] in cctop's config.toml:

[[schedule]]
name = \"deps\"
when = \"Mon..Fri 08:00\"     # systemd calendar time
dir = \"~/src/app\"
harness = \"claude\"          # claude, codex, devin or opencode
prompt = \"Bump outdated dependencies and run the tests\"
# account = \"work\", model = \"sonnet\", enabled = false

list   each schedule, its next and last run
sync   install the systemd timers for what config.toml says now
run    start one now, as its timer would; --now runs it even when switched off";

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"
[settings]
notify = true

# The morning one.
[[schedule]]
name = "deps"
when = "Mon..Fri 08:00"
dir = "~/src/app"
harness = "claude"
model = "sonnet"
prompt = "Bump outdated dependencies and run the tests"

[[schedule]]
name = "triage"
when = "daily"
dir = "/tmp"
harness = "codex"
prompt = "Label new issues"
enabled = false
"#;

    fn deps() -> Schedule {
        Schedules::parse(FILE).entries[0].clone()
    }

    #[test]
    fn schedules_are_read_from_the_file() {
        let s = Schedules::parse(FILE);
        assert_eq!(s.problems, Vec::<String>::new());
        assert_eq!(s.entries.len(), 2);
        assert_eq!(s.entries[0].model.as_deref(), Some("sonnet"));
        assert!(s.entries[0].enabled);
        assert!(!s.entries[1].enabled);
        assert_eq!(s.named("triage").unwrap().harness, "codex");
    }

    /// Each bad entry is left out with a sentence saying why, and the good
    /// ones still run.
    #[test]
    fn a_bad_entry_is_reported_and_the_rest_kept() {
        let bad = |table: &str| {
            let s = Schedules::parse(&format!("{FILE}\n[[schedule]]\n{table}\n"));
            assert_eq!(s.entries.len(), 2, "{table}");
            assert_eq!(s.problems.len(), 1, "{table}");
            s.problems[0].clone()
        };
        let base = "when = \"daily\"\ndir = \"/tmp\"\nharness = \"claude\"\nprompt = \"hi\"";
        assert!(bad(&format!("name = \"a b\"\n{base}")).contains("letters, digits"));
        assert!(bad(&format!("name = \"deps\"\n{base}")).contains("named twice"));
        assert!(
            bad(&format!("name = \"x\"\n{base}\nyolo = true")).contains("not a schedule setting")
        );
        assert!(
            bad("name = \"x\"\nwhen = \"daily\"\ndir = \"/tmp\"\nharness = \"bash\"\nprompt = \"hi\"")
                .contains("not a harness")
        );
        assert!(
            bad("name = \"x\"\nwhen = \"daily\"\ndir = \"/tmp\"\nharness = \"claude\"\nprompt = \"--yolo\"")
                .contains("cannot start with '-'")
        );
        assert!(
            bad("name = \"x\"\nwhen = \"daily\\nExecStartPre=/bin/sh\"\ndir = \"/tmp\"\nharness = \"claude\"\nprompt = \"hi\"")
                .contains("not a calendar expression")
        );
        assert!(
            bad("name = \"x\"\nwhen = \"daily\"\ndir = \"/tmp\"\nharness = \"devin\"\nmodel = \"opus\"\nprompt = \"hi\"")
                .contains("model")
        );
        assert!(bad(&format!("name = \"x\"\n{base}\nenabled = \"yes\"")).contains("`enabled`"));
    }

    #[test]
    fn a_running_previous_run_is_never_doubled() {
        let mut s = deps();
        assert_eq!(skip_reason(&s, false, false), None);
        assert!(
            skip_reason(&s, true, false)
                .unwrap()
                .contains("still going")
        );
        assert!(skip_reason(&s, true, true).unwrap().contains("still going"));
        s.enabled = false;
        assert!(skip_reason(&s, false, false).unwrap().contains("disabled"));
        assert_eq!(skip_reason(&s, false, true), None);
    }

    /// A schedule's agent is started exactly as `spawn_agent` starts one,
    /// with the model as the only thing added.
    #[test]
    fn a_scheduled_launch_is_a_spawn_agent_launch() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = deps();
        s.dir = dir.path().to_string_lossy().into_owned();
        s.model = None;
        let ours = plan(&s).unwrap();
        let theirs = crate::drive::plan(
            &crate::drive::Request {
                harness: s.harness.clone(),
                dir: s.dir.clone(),
                prompt: s.prompt.clone(),
                account: None,
            },
            &crate::drive::Caller {
                depth: 0,
                name: "test".into(),
            },
            0,
        )
        .unwrap();
        assert_eq!(ours.argv, theirs.argv);
        assert_eq!(ours.dir, theirs.dir);
        assert_eq!(ours.env, ["CCTOP_SCHEDULE=deps"]);
        s.model = Some("sonnet".into());
        assert_eq!(
            plan(&s).unwrap().argv,
            [
                "claude",
                "--model",
                "sonnet",
                "Bump outdated dependencies and run the tests"
            ]
        );
        assert!(!crate::drive::escalates(&plan(&s).unwrap().argv));
    }

    #[test]
    fn the_units_are_what_systemd_is_given() {
        let s = deps();
        let exe = Path::new("/usr/local/bin/cctop");
        assert_eq!(
            service_text(&s, exe, "/home/a%b/bin:/usr/bin"),
            format!(
                "{MARKER}\n[Unit]\nDescription=cctop scheduled agent: deps\n\n[Service]\n\
                 Type=oneshot\nEnvironment=\"PATH=/home/a%%b/bin:/usr/bin\"\n\
                 Environment=XDG_RUNTIME_DIR=%t\n\
                 ExecStart=\"/usr/local/bin/cctop\" schedule run deps\n"
            )
        );
        assert_eq!(
            timer_text(&s),
            format!(
                "{MARKER}\n[Unit]\nDescription=cctop schedule deps (Mon..Fri 08:00)\n\n\
                 [Timer]\nOnCalendar=Mon..Fri 08:00\nPersistent=true\n\
                 Unit=cctop-schedule-deps.service\n\n[Install]\nWantedBy=timers.target\n"
            )
        );
    }

    /// Install, change, remove — and a file that is not cctop's is never
    /// touched, even when it has a schedule's name.
    #[test]
    fn sync_installs_updates_and_removes_only_its_own_units() {
        let dir = tempfile::tempdir().unwrap();
        let units = dir.path();
        let exe = Path::new("/bin/cctop");
        let mut asked: Vec<String> = Vec::new();
        let mut ctl = |a: &[&str]| {
            asked.push(a.join(" "));
            Ok(String::new())
        };
        let fine = |_: &str| Ok(());
        let mut all = Schedules::parse(FILE).entries;
        // Someone's own unit beside them, with a name in the same family.
        std::fs::write(units.join("cctop-schedule-mine.timer"), "[Timer]\n").unwrap();

        let first = sync(&all, units, exe, "/usr/bin", &fine, &mut ctl).unwrap();
        assert_eq!(first.added, ["deps"]);
        assert!(units.join("cctop-schedule-deps.timer").exists());
        assert!(
            !units.join("cctop-schedule-triage.timer").exists(),
            "disabled"
        );

        let again = sync(&all, units, exe, "/usr/bin", &fine, &mut ctl).unwrap();
        assert!(!again.changed());

        all[0].when = "daily".into();
        let changed = sync(&all, units, exe, "/usr/bin", &fine, &mut ctl).unwrap();
        assert_eq!(changed.updated, ["deps"]);

        let gone = sync(&[], units, exe, "/usr/bin", &fine, &mut ctl).unwrap();
        assert_eq!(gone.removed, ["deps"]);
        assert!(!units.join("cctop-schedule-deps.timer").exists());
        assert!(!units.join("cctop-schedule-deps.service").exists());
        assert!(units.join("cctop-schedule-mine.timer").exists(), "not ours");

        let asked = asked.join("\n");
        assert!(asked.contains("enable --now cctop-schedule-deps.timer"));
        assert!(asked.contains("disable --now cctop-schedule-deps.timer"));
        assert!(asked.contains("daemon-reload"));
        assert!(!asked.contains("mine"));
    }

    #[test]
    fn a_time_systemd_cannot_read_is_reported_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctl = |_: &[&str]| Ok(String::new());
        let refuse = |when: &str| Err(format!("bad {when}"));
        let synced = sync(
            &Schedules::parse(FILE).entries,
            dir.path(),
            Path::new("/bin/cctop"),
            "",
            &refuse,
            &mut ctl,
        )
        .unwrap();
        assert!(!synced.changed());
        assert!(synced.problems[0].contains("deps: bad Mon..Fri 08:00"));
    }

    #[test]
    fn no_user_systemd_is_said_plainly() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctl = |_: &[&str]| Err("Failed to connect to bus: No medium found".to_string());
        let why = sync(&[], dir.path(), Path::new("/x"), "", &|_| Ok(()), &mut ctl).unwrap_err();
        assert!(why.contains("no user systemd"), "{why}");
        assert!(why.contains("Failed to connect to bus"), "{why}");
        let mut degraded = |_: &[&str]| Err("degraded".to_string());
        assert_eq!(available(&mut degraded), Ok(()));
    }

    #[test]
    fn enabled_is_flipped_in_the_file_and_nothing_else_moves() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, FILE).unwrap();
        set_enabled(&path, "deps", false).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# The morning one."));
        assert!(!Schedules::parse(&text).named("deps").unwrap().enabled);
        set_enabled(&path, "triage", true).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(Schedules::parse(&text).named("triage").unwrap().enabled);
        assert!(!text.contains("enabled = true"));
        assert!(set_enabled(&path, "nope", true).is_err());
    }

    #[test]
    fn timer_times_read_systemds_answer() {
        let mut ctl = |_: &[&str]| {
            Ok("NextElapseUSecRealtime=Thu 2026-11-05 08:00:00 NZDT\nLastTriggerUSec=n/a\n".into())
        };
        assert_eq!(
            timer_times(&deps(), &mut ctl),
            (Some("Thu 2026-11-05 08:00:00 NZDT".into()), None)
        );
    }
}
