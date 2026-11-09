//! What scheduled runs did while nobody was looking: `schedule-runs.jsonl`,
//! and the summary of each run that the dashboard and `cctop schedule log`
//! read back.
//!
//! A schedule fires when no cctop is open — that is what it is for — so the
//! person finds out what it did afterwards or not at all. A push says a run
//! started; nothing said what came of it. So every `cctop schedule run`
//! appends one line here: which schedule, when, where, under which harness and
//! account, and whether it started, was skipped, or failed to start.
//!
//! # Which session a run started
//!
//! Not known when the run is recorded: the agent writes its transcript after
//! its first turn begins, by which time `schedule run` has exited. What is
//! known is the agent's pid — the pane rmux started — and its directory. So a
//! run is joined to a session when it is read, by [`session_of`]: the session
//! whose agent is that pid, else the first session of that harness to start in
//! that directory shortly after the run did. Derived rather than written back,
//! so nothing has to come round later to fill a field in, and a reader with a
//! fresher table gets a fresher answer.
//!
//! # How it is written
//!
//! Read, appended to and cut to the last [`MAX_RUNS`] under an `flock` on a
//! lock file beside it, then written whole through a rename — so two timers
//! firing in the same second both land, and a reader never sees half a file.
//! A run is a few hundred bytes and fires a few times a day, so rewriting the
//! file is cheaper than any cleverness about rotating it. A write that fails
//! changes nothing about the run itself: the record is for afterwards.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use crate::session::{ActivityState, Session};

pub const FILE: &str = "schedule-runs.jsonl";
/// When the person last looked at the runs: one RFC 3339 timestamp.
pub const SEEN: &str = "schedule-runs.seen";
const LOCK: &str = "schedule-runs.lock";

/// The runs kept. Months of a daily schedule, weeks of several; the view
/// shows the unseen ones first, and nobody reads further back than this.
pub const MAX_RUNS: usize = 200;

/// How long a write waits for another writer. Two timers firing together hold
/// the lock for a read and a write each, which is milliseconds.
const PATIENCE: Duration = Duration::from_secs(2);

/// How long after a run its agent's session can start and still be the run's,
/// when there is no pid to match. The agent is started with its prompt, so its
/// first turn — and its transcript — begins within seconds; a quarter of an
/// hour is room for a slow machine, and short enough that a session the person
/// opened in the same directory later in the morning is not taken for it.
const MATCH_WINDOW_SECS: i64 = 15 * 60;

/// How long a started run with no session yet is "starting" rather than lost.
const STARTING_SECS: i64 = 5 * 60;

/// What `schedule run` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Start {
    Started,
    /// Its previous run was still at work, or it is switched off.
    Skipped,
    /// It was meant to start and could not: no such schedule, a directory
    /// that is not there, a harness that is not installed.
    Failed,
}

/// One line of the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    /// RFC 3339, UTC.
    pub at: String,
    pub schedule: String,
    pub start: Start,
    /// Why it was skipped or failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The directory, resolved as the agent was started in it when it was;
    /// as written in `config.toml` otherwise.
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The rmux tab it runs in.
    #[serde(default)]
    pub tab: String,
    /// Started with `--now` rather than by its timer.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub by_hand: bool,
    /// The agent's process, as rmux started it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Where the file lives.
#[derive(Debug, Clone)]
pub struct Log {
    dir: PathBuf,
}

impl Default for Log {
    /// `~/.local/share/cctop`, beside the YOLO record — or, under test, a
    /// directory of the test process's own: a test must never add a run to,
    /// or mark seen, the developer's real record.
    fn default() -> Log {
        #[cfg(any(test, feature = "test-support"))]
        if crate::under_test() {
            return Log::at(
                std::env::temp_dir()
                    .join(format!("cctop-data-{}", std::process::id()))
                    .join("cctop"),
            );
        }
        Log::at(crate::config::data_base().join("cctop"))
    }
}

impl Log {
    pub fn at(dir: impl Into<PathBuf>) -> Log {
        Log { dir: dir.into() }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE)
    }

    fn create_dir(&self) -> std::io::Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)
    }

    /// Add `run`, keeping the last [`MAX_RUNS`].
    pub fn append(&self, run: &Run) -> std::io::Result<()> {
        self.create_dir()?;
        let _held = crate::util::lock_within(&self.dir.join(LOCK), PATIENCE)?;
        let mut runs = self.read();
        runs.push(run.clone());
        let skip = runs.len().saturating_sub(MAX_RUNS);
        let mut text = String::new();
        for run in &runs[skip..] {
            text.push_str(&serde_json::to_string(run).map_err(std::io::Error::other)?);
            text.push('\n');
        }
        // Private: a prompt is not in it, but a directory and an account are,
        // and nobody else on the machine needs to know when this person's
        // agents run.
        crate::util::write_atomic(&self.path(), text.as_bytes(), true)
    }

    /// Every run kept, oldest first. A line that does not parse is skipped:
    /// one bad line must not hide the rest.
    pub fn read(&self) -> Vec<Run> {
        std::fs::read_to_string(self.path())
            .map(|text| {
                text.lines()
                    .filter_map(|l| serde_json::from_str::<Run>(l).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// When the person last looked at the runs, if ever.
    pub fn seen(&self) -> Option<String> {
        std::fs::read_to_string(self.dir.join(SEEN))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| crate::util::parse_ts(s).is_some())
    }

    /// Record that the person has looked at every run up to `at`.
    pub fn mark_seen(&self, at: &str) -> std::io::Result<()> {
        self.create_dir()?;
        crate::util::write_atomic(&self.dir.join(SEEN), at.as_bytes(), true)
    }

    /// How many runs are newer than the last look.
    pub fn unseen(&self) -> usize {
        let seen = self.seen();
        self.read()
            .iter()
            .filter(|r| is_unseen(r, seen.as_deref()))
            .count()
    }
}

/// Write `run` to the default log, dropping any error: see [`Log::append`].
pub fn record(run: &Run) {
    let _ = Log::default().append(run);
}

fn is_unseen(run: &Run, seen: Option<&str>) -> bool {
    match seen.and_then(crate::util::parse_ts) {
        None => true,
        Some(seen) => crate::util::parse_ts(&run.at).is_some_and(|at| at > seen),
    }
}

// ---------------------------------------------------------------------------
// What came of each run
// ---------------------------------------------------------------------------

/// Where a run's agent is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Started moments ago, and its agent has not written a transcript yet.
    Starting,
    /// Mid-turn.
    Working,
    /// Held on a permission prompt — which, with no permission mode on a
    /// schedule by design, is where an unattended agent most often ends up.
    Permission,
    /// Held on a question it asked.
    Question,
    /// Its turn is over: at its prompt in the tab, or exited.
    Finished,
    /// The provider refused it.
    ApiError,
    Skipped,
    FailedToStart,
    /// Started, but no session can be found for it: the agent died before
    /// its first turn, or its transcript was deleted.
    NoSession,
}

impl State {
    /// A few words for a list.
    pub fn label(self) -> &'static str {
        match self {
            State::Starting => "starting",
            State::Working => "working",
            State::Permission => "needs permission",
            State::Question => "asked a question",
            State::Finished => "finished",
            State::ApiError => "API error",
            State::Skipped => "skipped",
            State::FailedToStart => "failed to start",
            State::NoSession => "no session",
        }
    }

    /// Whether this wants the person: a held agent, or one that never ran.
    pub fn needs_you(self) -> bool {
        matches!(
            self,
            State::Permission | State::Question | State::ApiError | State::FailedToStart
        )
    }
}

/// One run, and what came of it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    #[serde(flatten)]
    pub run: Run,
    pub state: State,
    /// Newer than the person's last look.
    pub unseen: bool,
    /// The session's [`Session::key`], when one was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Its agent is still running.
    pub live: bool,
    /// Dollars, at the plan's prices; `None` when the plan bundles it or no
    /// session was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    /// From its first message to its last.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    /// Files its transcript edited — filled by [`details`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_changed: Option<usize>,
    /// What it last said, on one line — filled by [`details`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_message: Option<String>,
    /// Why it looks stuck in a loop, if it does — see [`crate::stuck`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stuck: Option<String>,
}

fn same_dir(a: &str, b: &str) -> bool {
    let trim = |p: &str| p.trim_end_matches('/').to_string();
    !a.is_empty() && trim(a) == trim(b)
}

/// The session `run` started, if one can be found among `sessions`.
///
/// Its agent's pid first, which is exact while the agent is alive; else the
/// earliest session of the run's harness to start in the run's directory
/// within [`MATCH_WINDOW_SECS`] of it, and before `until` — the next run of
/// the same schedule, whose session would otherwise be claimed by both.
///
/// ponytail: an agent that exits before writing anything, while the person
/// starts one of the same harness in the same directory a minute later, is
/// matched to theirs. Both would have to happen inside the window.
pub fn session_of<'a>(
    run: &Run,
    until: Option<&str>,
    sessions: &'a [Session],
) -> Option<&'a Session> {
    if run.start != Start::Started {
        return None;
    }
    let at = crate::util::parse_ts(&run.at)?;
    let until = until.and_then(crate::util::parse_ts);
    let ours = |s: &&Session| s.remote.is_none() && s.provider.as_str() == run.harness;
    let started = |s: &Session| crate::util::parse_ts(&s.started_at);
    // A pid is reused, so the session must also have begun after the run.
    let slack = chrono::Duration::seconds(60);
    if let Some(pid) = run.pid
        && let Some(s) = sessions
            .iter()
            .filter(ours)
            .find(|s| s.root_pid() == Some(pid) && started(s).is_some_and(|t| t >= at - slack))
    {
        return Some(s);
    }
    sessions
        .iter()
        .filter(ours)
        .filter(|s| same_dir(&s.label_source, &run.dir))
        .filter_map(|s| started(s).map(|t| (t, s)))
        .filter(|(t, _)| {
            *t >= at - chrono::Duration::seconds(30)
                && *t <= at + chrono::Duration::seconds(MATCH_WINDOW_SECS)
                && until.is_none_or(|u| *t < u)
        })
        .min_by_key(|(t, _)| *t)
        .map(|(_, s)| s)
}

/// Where `session`'s agent is, for a run.
fn state_of(session: &Session) -> State {
    if !session.is_running() {
        return match session.activity_state {
            ActivityState::ApiError => State::ApiError,
            _ => State::Finished,
        };
    }
    match session.activity_state {
        ActivityState::Working => State::Working,
        ActivityState::WaitingForInput => State::Finished,
        ActivityState::Asking if session.asking_question => State::Question,
        ActivityState::Asking => State::Permission,
        ActivityState::ApiError => State::ApiError,
    }
}

/// Every run in `runs`, newest first, each with what came of it as far as the
/// table says — everything but [`details`]'s two, which read files.
///
/// `seen` is the person's last look; `now` decides when a run with no session
/// has stopped being "starting".
pub fn summarize(
    runs: &[Run],
    sessions: &[Session],
    seen: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<Summary> {
    let mut out = Vec::with_capacity(runs.len());
    for (i, run) in runs.iter().enumerate() {
        let next = runs[i + 1..]
            .iter()
            .find(|r| r.schedule == run.schedule && r.start == Start::Started)
            .map(|r| r.at.as_str());
        let session = session_of(run, next, sessions);
        let state = match (run.start, session) {
            (Start::Skipped, _) => State::Skipped,
            (Start::Failed, _) => State::FailedToStart,
            (Start::Started, Some(s)) => state_of(s),
            (Start::Started, None) => {
                let recent = crate::util::parse_ts(&run.at)
                    .is_some_and(|at| (now - at).num_seconds() < STARTING_SECS);
                match recent {
                    true => State::Starting,
                    false => State::NoSession,
                }
            }
        };
        let duration_ms = session.and_then(|s| {
            let from = crate::util::parse_ts(&s.started_at)?;
            let to = crate::util::parse_ts(&s.last_active)?;
            Some((to - from).num_milliseconds().max(0))
        });
        out.push(Summary {
            run: run.clone(),
            state,
            unseen: is_unseen(run, seen),
            session: session.map(Session::key),
            session_id: session.map(|s| s.session_id.clone()),
            live: session.is_some_and(Session::is_running),
            cost: session.and_then(|s| s.total_cost),
            duration_ms,
            files_changed: None,
            last_message: None,
            stuck: session.and_then(Session::looping).map(|l| l.reason),
        });
    }
    out.reverse();
    out
}

/// The longest one-line summary kept, in characters.
const MAX_LAST_MESSAGE: usize = 200;

/// The last thing the assistant said in `conversation`, on one line.
pub fn last_message(conversation: &crate::chat::Conversation) -> Option<String> {
    let turn = conversation
        .turns
        .iter()
        .rev()
        .find(|t| t.role == "assistant" && t.kind == "message" && !t.text.trim().is_empty())?;
    let line = turn.text.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(match line.chars().count() > MAX_LAST_MESSAGE {
        true => format!(
            "{}…",
            line.chars().take(MAX_LAST_MESSAGE - 1).collect::<String>()
        ),
        false => line,
    })
}

/// What reading the session's files adds to a summary: how many files it
/// edited, from its parsed transcript, and its last message, from its conversation.
///
/// The transcript's edits rather than the checkout's diff: a schedule's
/// directory is often one the person works in too, and the diff there is
/// everyone's, not the run's.
pub fn details(
    session: &Session,
    data: Option<&crate::session::SessionData>,
) -> (Option<usize>, Option<String>) {
    let files = data.map(|data| crate::handoff::touched_files(&data.metrics.tool_details).len());
    (files, last_message(&crate::chat::build(session, None)))
}

/// Every run kept, summarized and filled in against `sessions`, newest first —
/// what `cctop schedule log --json` prints. `data` reads a session's parsed
/// transcript, from whatever store the caller has.
pub fn report(
    log: &Log,
    sessions: &[Session],
    data: &dyn Fn(&Session) -> Option<std::sync::Arc<crate::session::SessionData>>,
) -> Vec<Summary> {
    let seen = log.seen();
    let mut out = summarize(&log.read(), sessions, seen.as_deref(), chrono::Utc::now());
    for summary in &mut out {
        let Some(key) = &summary.session else {
            continue;
        };
        if let Some(session) = sessions.iter().find(|s| &s.key() == key) {
            let data = data(session);
            (summary.files_changed, summary.last_message) = details(session, data.as_deref());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;
    use chrono::{Duration as Age, Utc};

    fn run(schedule: &str, at: &str, start: Start) -> Run {
        Run {
            at: at.into(),
            schedule: schedule.into(),
            start,
            why: None,
            dir: "/src/app".into(),
            harness: "claude".into(),
            account: None,
            model: None,
            tab: format!("cctop-schedule-{schedule}"),
            by_hand: false,
            pid: None,
        }
    }

    fn session(id: &str, dir: &str, started: &str) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.label_source = dir.into();
        s.started_at = started.into();
        s.last_active = started.into();
        s
    }

    fn live(mut s: Session, pid: u32, state: ActivityState) -> Session {
        s.process = Some(crate::proc::ProcInfo {
            pids: 1,
            cpu: 0.0,
            memory: 0,
            command: "claude".into(),
            process_list: vec![crate::proc::ProcEntry {
                pid,
                cpu: 0.0,
                memory: 0,
                args: "claude".into(),
                is_root: true,
                ghost: false,
            }],
        });
        s.activity_state = state;
        s
    }

    #[test]
    fn the_file_keeps_only_the_last_runs() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::at(dir.path());
        assert!(log.read().is_empty());
        for i in 0..MAX_RUNS + 7 {
            log.append(&run(
                &format!("s{i}"),
                "2026-11-09T08:00:00Z",
                Start::Started,
            ))
            .unwrap();
        }
        let runs = log.read();
        assert_eq!(runs.len(), MAX_RUNS);
        assert_eq!(runs[0].schedule, "s7", "the oldest went first");
        assert_eq!(runs.last().unwrap().schedule, format!("s{}", MAX_RUNS + 6));
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(log.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "only its owner reads it");
    }

    /// Timers that fire together each land: the lock serialises the
    /// read-modify-write, so none overwrites another's line.
    #[test]
    fn runs_recorded_at_once_all_land() {
        let dir = tempfile::tempdir().unwrap();
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let log = Log::at(dir.path());
                std::thread::spawn(move || {
                    for i in 0..10 {
                        log.append(&run(&format!("t{t}-{i}"), &now(), Start::Started))
                            .unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let runs = Log::at(dir.path()).read();
        assert_eq!(runs.len(), 80);
        let mut names: Vec<_> = runs.iter().map(|r| r.schedule.clone()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 80);
    }

    #[test]
    fn a_bad_line_hides_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::at(dir.path());
        log.append(&run("a", "2026-11-09T08:00:00Z", Start::Started))
            .unwrap();
        let mut text = std::fs::read_to_string(log.path()).unwrap();
        text.push_str("{not json\n");
        std::fs::write(log.path(), text).unwrap();
        log.append(&run("b", "2026-11-09T09:00:00Z", Start::Skipped))
            .unwrap();
        let names: Vec<_> = log.read().into_iter().map(|r| r.schedule).collect();
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn unseen_runs_are_the_ones_after_the_last_look() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::at(dir.path());
        log.append(&run("a", "2026-11-09T06:00:00Z", Start::Started))
            .unwrap();
        log.append(&run("b", "2026-11-09T08:00:00Z", Start::Started))
            .unwrap();
        assert_eq!(log.seen(), None);
        assert_eq!(log.unseen(), 2, "never looked: all of them");
        log.mark_seen("2026-11-09T07:00:00Z").unwrap();
        assert_eq!(log.seen().as_deref(), Some("2026-11-09T07:00:00Z"));
        assert_eq!(log.unseen(), 1);
        log.mark_seen("2026-11-09T08:00:00Z").unwrap();
        assert_eq!(log.unseen(), 0);
    }

    /// The pid names the session exactly; without one, the first session of
    /// the harness in the directory after the run — not one from before it,
    /// not the next run's, not another harness's.
    #[test]
    fn a_run_finds_the_session_it_started() {
        let at = "2026-11-09T08:00:00Z";
        let mut by_pid = run("deps", at, Start::Started);
        by_pid.pid = Some(4242);
        let sessions = vec![
            session("before", "/src/app", "2026-11-09T07:00:00Z"),
            live(
                session("theirs", "/elsewhere", "2026-11-09T08:00:05Z"),
                4242,
                ActivityState::Working,
            ),
            session("first", "/src/app/", "2026-11-09T08:00:09Z"),
            session("later", "/src/app", "2026-11-09T08:03:00Z"),
            session("hours-later", "/src/app", "2026-11-09T11:00:00Z"),
        ];
        assert_eq!(
            session_of(&by_pid, None, &sessions).map(|s| s.session_id.as_str()),
            Some("theirs")
        );
        let by_dir = run("deps", at, Start::Started);
        assert_eq!(
            session_of(&by_dir, None, &sessions).map(|s| s.session_id.as_str()),
            Some("first")
        );
        assert!(
            session_of(&by_dir, Some("2026-11-09T08:00:08Z"), &sessions).is_none(),
            "the next run's session is the next run's"
        );
        let mut codex = by_dir.clone();
        codex.harness = "codex".into();
        assert!(session_of(&codex, None, &sessions).is_none());
        let late = run("deps", "2026-11-09T10:00:00Z", Start::Started);
        assert!(
            session_of(&late, None, &sessions).is_none(),
            "a session hours later is the person's own"
        );
        assert!(session_of(&run("deps", at, Start::Skipped), None, &sessions).is_none());
    }

    #[test]
    fn each_run_says_what_came_of_it() {
        let now = Utc::now();
        let t = |mins: i64| (now - Age::minutes(mins)).to_rfc3339();
        let mut runs = vec![
            run("working", &t(120), Start::Started),
            run("asking", &t(110), Start::Started),
            run("question", &t(100), Start::Started),
            run("idle", &t(90), Start::Started),
            run("exited", &t(80), Start::Started),
            run("skipped", &t(70), Start::Skipped),
            run("failed", &t(60), Start::Failed),
            run("lost", &t(50), Start::Started),
            run("fresh", &t(1), Start::Started),
        ];
        for (i, r) in runs.iter_mut().enumerate() {
            r.dir = format!("/src/{}", r.schedule);
            r.pid = Some(100 + i as u32);
        }
        let mut asking_q = live(
            session("q", "/src/question", &t(100)),
            102,
            ActivityState::Asking,
        );
        asking_q.asking_question = true;
        let mut exited = session("x", "/src/exited", &t(80));
        exited.last_active = t(65);
        exited.total_cost = Some(1.25);
        let sessions = vec![
            live(
                session("w", "/src/working", &t(120)),
                100,
                ActivityState::Working,
            ),
            live(
                session("a", "/src/asking", &t(110)),
                101,
                ActivityState::Asking,
            ),
            asking_q,
            live(
                session("i", "/src/idle", &t(90)),
                103,
                ActivityState::WaitingForInput,
            ),
            exited,
        ];
        let seen = t(75);
        let out = summarize(&runs, &sessions, Some(&seen), now);
        let states: Vec<_> = out
            .iter()
            .map(|s| (s.run.schedule.as_str(), s.state, s.unseen))
            .collect();
        assert_eq!(
            states,
            [
                ("fresh", State::Starting, true),
                ("lost", State::NoSession, true),
                ("failed", State::FailedToStart, true),
                ("skipped", State::Skipped, true),
                ("exited", State::Finished, false),
                ("idle", State::Finished, false),
                ("question", State::Question, false),
                ("asking", State::Permission, false),
                ("working", State::Working, false),
            ],
            "newest first"
        );
        let exited = &out[4];
        assert_eq!(exited.session.as_deref(), Some("claude:x"));
        assert!(!exited.live);
        assert_eq!(exited.cost, Some(1.25));
        assert_eq!(exited.duration_ms, Some(15 * 60 * 1000));
        assert!(
            out[5].live,
            "idle at its prompt is finished, and still live"
        );
        assert!(State::Permission.needs_you() && !State::Finished.needs_you());

        // The JSON a page will read: flat, the run's own fields beside what
        // came of it.
        let json = serde_json::to_value(&out[4]).unwrap();
        assert_eq!(json["schedule"], "exited");
        assert_eq!(json["start"], "started");
        assert_eq!(json["state"], "finished");
        assert_eq!(json["session_id"], "x");
    }

    #[test]
    fn the_last_message_is_one_line_of_the_assistants() {
        use crate::chat::{Conversation, Turn};
        let turn = |role: &'static str, kind: &'static str, text: &str| Turn {
            seq: 0,
            role: role.into(),
            kind: kind.into(),
            from: None,
            agent: None,
            ts: String::new(),
            text: text.into(),
            clipped: false,
            tools: Vec::new(),
            html: None,
        };
        let conversation = Conversation {
            turns: vec![
                turn("user", "message", "bump the deps"),
                turn(
                    "assistant",
                    "message",
                    "Bumped 4 crates.\n\nAll   tests pass.",
                ),
                turn("assistant", "reasoning", "thinking about it"),
                turn("assistant", "message", "  "),
                turn("user", "message", "thanks"),
            ],
            ..Conversation::default()
        };
        assert_eq!(
            last_message(&conversation).as_deref(),
            Some("Bumped 4 crates. All tests pass.")
        );
        let long = Conversation {
            turns: vec![turn("assistant", "message", &"word ".repeat(100))],
            ..Conversation::default()
        };
        let cut = last_message(&long).unwrap();
        assert_eq!(cut.chars().count(), MAX_LAST_MESSAGE);
        assert!(cut.ends_with('…'));
        assert_eq!(last_message(&Conversation::default()), None);
    }
}
