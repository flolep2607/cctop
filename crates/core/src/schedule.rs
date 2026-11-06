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
    /// Why the file could not be read at all, when it could not: missing
    /// permissions, or text that is not TOML — a typo anywhere in it, or an
    /// editor's half-written save caught by the reload. Not the same answer as
    /// "no schedules": read as that, a sync would uninstall every timer.
    pub unreadable: Option<String>,
}

impl Schedules {
    pub fn load() -> Schedules {
        Schedules::load_from(&crate::config::CONFIG_FILE)
    }

    pub fn load_from(path: &Path) -> Schedules {
        match std::fs::read_to_string(path) {
            Ok(text) => Schedules::parse(&text),
            // No file is no schedules; that one is true.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Schedules::default(),
            Err(e) => Schedules {
                unreadable: Some(format!("{} could not be read: {e}", path.display())),
                ..Schedules::default()
            },
        }
    }

    pub fn parse(text: &str) -> Schedules {
        let mut out = Schedules::default();
        let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
            // `Settings` already says how the file is not TOML; this only
            // keeps the answer from being taken for "no schedules".
            out.unreadable =
                Some("config.toml is not valid TOML; schedules left as they were".into());
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

/// Where a schedule's previous run is, as far as the next run cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Previous {
    /// No tab, or its agent has exited.
    Gone,
    /// The agent is at its prompt, its turn done: finished, for a schedule.
    /// An interactive agent never exits by itself, so without this a daily
    /// schedule ran once and was skipped every day after.
    Idle,
    /// Working, or waiting on a person, or nobody can tell which.
    Busy,
}

/// The previous run of the schedule whose tab is `tab`.
pub fn previous(tab: &str) -> Previous {
    if !crate::rmux::live(tab) {
        return Previous::Gone;
    }
    match crate::peek::named(tab).map(|s| s.signal) {
        Some(crate::hook::Signal::Idle) => Previous::Idle,
        _ => Previous::Busy,
    }
}

/// What a run does instead of starting the agent, if anything.
///
/// By hand (`--now`) a disabled schedule still runs — the person asked for
/// this one — but a schedule whose last agent is still at work never doubles
/// up, whoever asks. One that finished and sits at its prompt is replaced.
pub fn skip_reason(schedule: &Schedule, previous: Previous, by_hand: bool) -> Option<String> {
    if previous == Previous::Busy {
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
    let before = previous(&tab);
    let result = match skip_reason(schedule, before, by_hand) {
        Some(why) => {
            // Told, not only logged: a run that did not happen is otherwise
            // noticed days later, by the work that was not done.
            notify(
                &format!("Skipped: {}", schedule.name),
                &format!("Its previous run is still at work in tab {tab}"),
            );
            Ok(format!("skipped: {why}"))
        }
        None => {
            if before == Previous::Idle {
                // Its turn is over and its conversation is on disk; the tab
                // is the next run's to use.
                let _ = crate::rmux::kill(&tab);
            }
            start(schedule, &tab)
        }
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
fn announce(schedule: &Schedule, dir: &Path) {
    notify(
        &format!("Scheduled: {}", schedule.name),
        &format!(
            "{} started in {}",
            schedule.harness,
            crate::util::tildify(&dir.to_string_lossy())
        ),
    );
}

/// A push to the person's phone, about a schedule.
///
/// Through the browsers subscribed on `cctop serve`'s page, read from the same
/// sealed store. Sent inline, not on [`push_all`](crate::webpush::push_all)'s
/// thread: this process exits as soon as the agent is up, and would take the
/// thread with it. Nobody subscribed is the usual case, and costs nothing.
fn notify(title: &str, body: &str) {
    if crate::under_test() {
        return;
    }
    let store = crate::secrets::Store::current();
    let subscriptions = crate::webpush::load_subscriptions(&store);
    let Some(vapid) = crate::webpush::Vapid::load(&store) else {
        return;
    };
    let payload = crate::webpush::Payload {
        title: title.to_string(),
        body: body.to_string(),
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
/// The user manager's environment is a bare system one, so the person's is
/// carried over: `PATH`, because the harnesses live in `~/.local/bin` and the
/// like, and without it `claude` is "not installed" at 08:00 and installed at
/// 08:01; and [`ENV_FILE`], written at sync time, for what picks the account
/// and the credentials. `XDG_RUNTIME_DIR` is `%t`, the manager's own, which is
/// where cctop's rmux keeps its socket.
///
/// `KillMode=process`: a oneshot's processes are killed when its command
/// exits, the whole cgroup at systemd's default. A run that had to start
/// cctop's rmux daemon itself — no cctop open at 08:00, which is what
/// schedules are for — would have its agent killed a second after the push
/// saying it started. [`crate::mux::start`] also moves a daemon it starts from
/// a service into a scope of its own; this is the second guard for a machine
/// where that cannot be done.
pub fn service_text(schedule: &Schedule, exe: &Path, path: &str, env: &Path) -> String {
    format!(
        "{MARKER}\n\
         [Unit]\n\
         Description=cctop scheduled agent: {name}\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         KillMode=process\n\
         Environment={path_env}\n\
         Environment=XDG_RUNTIME_DIR=%t\n\
         EnvironmentFile=-{env_file}\n\
         ExecStart={exe} schedule run {name}\n",
        name = schedule.name,
        path_env = quote(&format!("PATH={path}")),
        // A path, not a list of words: systemd does not unquote it, so only
        // its specifier character is escaped.
        env_file = env.to_string_lossy().replace('%', "%%"),
        exe = quote(&exe.to_string_lossy()),
    )
}

/// Where the environment a scheduled run needs is written at sync time.
///
/// Its own file, private, rather than `Environment=` lines in the unit: an API
/// key has no business in a world-readable unit file.
pub fn env_file() -> PathBuf {
    crate::config::config_base()
        .join("cctop")
        .join("schedule.env")
}

/// The variables of this environment a scheduled agent needs and the user
/// manager does not have: which account each harness uses, the keys some
/// accounts are, and the locale.
fn carried(name: &str) -> bool {
    matches!(name, "LANG" | "LC_ALL" | "LC_CTYPE" | "TZ")
        || [
            "CLAUDE_",
            "CODEX_",
            "ANTHROPIC_",
            "OPENAI_",
            "OPENCODE_",
            "DEVIN_",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// [`env_file`]'s text, from `vars`: one `NAME="value"` line each, quoted as
/// systemd reads an environment file, and sorted so a sync that changed
/// nothing writes the same bytes.
pub fn env_text(vars: impl IntoIterator<Item = (String, String)>) -> String {
    let mut lines: Vec<String> = vars
        .into_iter()
        .filter(|(name, value)| carried(name) && !value.contains('\n'))
        .map(|(name, value)| {
            let value = value.replace('\\', "\\\\").replace('"', "\\\"");
            format!("{name}=\"{value}\"")
        })
        .collect();
    lines.sort();
    lines.iter().map(|l| format!("{l}\n")).collect()
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
///
/// Through [`config_base`](crate::config::config_base), whose guard keeps a
/// root cctop under a `sudo` that kept `HOME` out of the user's directories: a
/// root with no schedules would otherwise remove every timer the user has.
pub fn unit_dir() -> PathBuf {
    crate::config::config_base().join("systemd/user")
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
        wanted.insert(
            format!("{unit}.service"),
            service_text(schedule, exe, path, &env_file()),
        );
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

/// The cctop a timer runs: the one on `PATH`, else this one.
///
/// Not simply this process's binary. A dashboard left open across
/// `cctop --update` is running a deleted file, whose path reads
/// `/usr/local/bin/cctop (deleted)` — a timer pointed there fails every run,
/// silently — and a build run from a worktree is a file the worktree's
/// cleanup removes. `None` when there is no cctop to point at.
pub fn timer_exe() -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|dir| dir.join("cctop"))
        .find(|exe| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(exe).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
        .or_else(crate::mux::installed_exe)
}

/// [`sync`] against the real systemd and the installed cctop, with this
/// environment's account variables written to [`env_file`] first.
pub fn sync_now(schedules: &[Schedule]) -> Result<Synced, String> {
    let exe =
        timer_exe().ok_or("there is no cctop on PATH, nor at the path this one was run from")?;
    let path = std::env::var("PATH").unwrap_or_default();
    write_env_file()?;
    sync(
        schedules,
        &unit_dir(),
        &exe,
        &path,
        &analyze,
        &mut |args: &[&str]| systemctl(args),
    )
}

/// Write [`env_file`] from this process's environment, private to the user.
fn write_env_file() -> Result<(), String> {
    let file = env_file();
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    crate::util::write_atomic(&file, env_text(std::env::vars()).as_bytes(), true)
        .map_err(|e| format!("{}: {e}", file.display()))
}

/// Sync if the schedules or the environment they run in changed since the
/// last sync this machine did, and say what changed — `None` when nothing
/// needed doing.
///
/// What the dashboard and `cctop serve` call when they read `config.toml`, so a
/// schedule written into the file is installed without a second command. A
/// machine with no schedules, and none installed, never asks systemd anything.
/// A file that cannot be read changes nothing: see [`Schedules::unreadable`].
/// A sync that failed is not tried again until something changes, so a
/// machine with no user systemd says so once, not on every reload.
pub fn sync_if_changed() -> Option<String> {
    if crate::under_test() {
        return None;
    }
    let schedules = Schedules::load();
    if schedules.unreadable.is_some() {
        return None;
    }
    let exe = timer_exe()?;
    let fingerprint = format!(
        "{:?}|{}|{}|{}",
        schedules.entries,
        exe.display(),
        std::env::var("PATH").unwrap_or_default(),
        env_text(std::env::vars()),
    );
    let stamp = crate::config::CACHE_DIR.join("schedule-synced");
    let failed = crate::config::CACHE_DIR.join("schedule-sync-failed");
    let same = |file: &Path| std::fs::read_to_string(file).is_ok_and(|old| old == fingerprint);
    if same(&stamp) || same(&failed) {
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
            let _ = std::fs::remove_file(&failed);
            synced
                .changed()
                .then(|| format!("Schedules: {}", synced.summary()))?
        }
        Err(why) => {
            let _ = std::fs::write(&failed, &fingerprint);
            format!("Schedules not installed: {why}")
        }
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
            if let Some(why) = &schedules.unreadable {
                eprintln!("{why}");
                return 1;
            }
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
                // Checked now rather than when the file is read: a directory
                // can be made after the schedule is written, and a run checks
                // again anyway. Said here so it is not found out at 08:00.
                if let Err(why) = crate::drive::check_dir(&s.dir) {
                    println!("{:<20} won't run: {why}", "");
                }
            }
            for problem in &schedules.problems {
                eprintln!("{problem}");
            }
            0
        }
        ["sync"] => {
            let schedules = Schedules::load();
            if let Some(why) = &schedules.unreadable {
                eprintln!("{why}");
                return 1;
            }
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

    /// A previous run still at work is never doubled; one that finished and
    /// sits at its prompt is replaced, or a daily schedule would run once.
    #[test]
    fn a_running_previous_run_is_never_doubled() {
        let mut s = deps();
        assert_eq!(skip_reason(&s, Previous::Gone, false), None);
        assert_eq!(skip_reason(&s, Previous::Idle, false), None);
        assert!(
            skip_reason(&s, Previous::Busy, false)
                .unwrap()
                .contains("still going")
        );
        assert!(
            skip_reason(&s, Previous::Busy, true)
                .unwrap()
                .contains("still going")
        );
        s.enabled = false;
        assert!(
            skip_reason(&s, Previous::Gone, false)
                .unwrap()
                .contains("disabled")
        );
        assert_eq!(skip_reason(&s, Previous::Gone, true), None);
    }

    /// A file that is not TOML, or cannot be read, is not "no schedules":
    /// read as that, the next sync would remove every timer.
    #[test]
    fn a_broken_file_is_not_an_empty_one() {
        let broken = Schedules::parse("[[schedule]]\nname = \"deps\"\nwhen = ");
        assert!(broken.unreadable.is_some());
        assert!(broken.entries.is_empty());
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Schedules::load_from(&dir.path().join("absent.toml")),
            Schedules::default(),
            "no file really is no schedules"
        );
        // A directory where the file should be: there, but unreadable.
        assert!(Schedules::load_from(dir.path()).unreadable.is_some());
        assert!(Schedules::parse(FILE).unreadable.is_none());
    }

    /// The account variables a scheduled agent needs are carried, quoted for
    /// systemd; nothing else of the environment is.
    #[test]
    fn the_environment_file_carries_accounts_and_nothing_else() {
        let vars = [
            ("CLAUDE_CONFIG_DIR", "/home/a/.claude-work"),
            ("ANTHROPIC_API_KEY", "sk-\"x\\y"),
            ("HOME", "/home/a"),
            ("SSH_AUTH_SOCK", "/tmp/s"),
            ("LANG", "en_NZ.UTF-8"),
            ("CODEX_HOME", "two\nlines"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        assert_eq!(
            env_text(vars),
            "ANTHROPIC_API_KEY=\"sk-\\\"x\\\\y\"\n\
             CLAUDE_CONFIG_DIR=\"/home/a/.claude-work\"\n\
             LANG=\"en_NZ.UTF-8\"\n"
        );
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
        // `KillMode=process` is what keeps the agent alive once the run that
        // started cctop's rmux exits; without it systemd kills the cgroup.
        assert_eq!(
            service_text(
                &s,
                exe,
                "/home/a%b/bin:/usr/bin",
                Path::new("/home/a%b/.config/cctop/schedule.env")
            ),
            format!(
                "{MARKER}\n[Unit]\nDescription=cctop scheduled agent: deps\n\n[Service]\n\
                 Type=oneshot\nKillMode=process\nEnvironment=\"PATH=/home/a%%b/bin:/usr/bin\"\n\
                 Environment=XDG_RUNTIME_DIR=%t\n\
                 EnvironmentFile=-/home/a%%b/.config/cctop/schedule.env\n\
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
