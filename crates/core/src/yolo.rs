//! YOLO: a session whose every permission prompt is answered "allow".
//!
//! Switched on per session, from the page or the table, by someone who has
//! decided this one agent may do whatever it asks to. There are two ways the
//! answer gets in, and which one a session gets depends on its harness.
//!
//! # Claude Code: the agent asks cctop, and the dialog never goes up
//!
//! Claude Code runs a `PermissionRequest` hook before it draws a permission
//! dialog, and takes `{"behavior": "allow"}` from one as the answer. So cctop
//! installs a second, separate command for that one event, `cctop yolo-hook`
//! ([`crate::hook::yolo_hook`]), beside the observer `cctop hook` that never
//! decides anything. It prints the allow for a YOLO session and nothing at all
//! for every other one, and every other failure is the same nothing: the
//! dialog appears exactly as it would with no hook installed.
//!
//! What makes that safe is what it matches on. Switching YOLO on records the
//! session id *and* the agent process running it — pid and start time, so a
//! pid the kernel hands out again is not the same process ([`Agent`]). The
//! hook answers only when its payload names that session and the recorded
//! process is one of its own ancestors, which it is exactly when that process
//! spawned it. So `claude --resume` of the same id in a new process is not
//! covered until YOLO is switched on again; no other session ever matches; an
//! entry left behind by a process that has died matches nothing. Subagents run
//! inside the same process and report their parent's id, so they are covered,
//! which is what "this session" means. Nothing needs a cctop to be running for
//! any of it: the process check is what bounds it, not a live owner.
//!
//! # The key press: Codex, and the fallback for Claude
//!
//! Everything else is cctop pressing the same key a person clicking Allow
//! presses, through the same [`crate::actions::answer`], with every guard that
//! has: only while the row says it is asking, never at a question with
//! choices, never at a harness whose menu cctop has not driven, never at a row
//! from another machine. Codex is answered this way always; whether it honours
//! a decision from a hook has not been verified.
//!
//! For Claude it is only a fallback, for a hook that did not answer — a Claude
//! Code too old to take the decision, hooks not reinstalled since an upgrade.
//! A key press at a prompt the hook already allowed is a `1` typed into the
//! composer of an agent that is busy, so the fallback presses only on what it
//! sees: the permission menu on the agent's screen, [`FALLBACK_AFTER`] after
//! the prompt was raised. A screen cctop cannot read gets no key press.
//!
//! # Where the switch lives
//!
//! In one small file in the runtime directory, `yolo.json`, which every cctop
//! on the machine reads — the dashboard, the serve it hosts, a standalone
//! `cctop serve` — and the hook reads too. The runtime directory because a
//! session does not outlive a boot, and neither should permission to answer
//! for it. Written through a temporary file and a rename, under an `flock` on
//! `yolo.lock`, so a reader sees the old set or the new one and two writers
//! cannot lose each other's change; the hook only reads, and needs no lock to.
//! An id is dropped once its session has stopped or its process has gone —
//! "until the session ends" is part of what the person agreed to.
//!
//! # Who presses
//!
//! Exactly one process: whichever holds an exclusive `flock` on `yolo.owner`.
//! Every cctop that could answer tries for it while any session has YOLO on,
//! and the first to get it keeps it for as long as it lives. The kernel drops
//! the lock when that process exits however it exits, so a crashed owner hands
//! over on the next tick of whoever is left, with no stale lock file to clean
//! up and no pid to second-guess. The alternatives were worse: a claim written
//! per prompt needs a key every process agrees on, and the one thing that tells
//! two identical prompts apart — when each went up — is an `Instant` that each
//! process stamps on its own hook event, a few milliseconds apart.
//!
//! Inside the owner, a prompt is answered at most once. It is keyed on the
//! session, what it asks for, and the moment this process first saw it up;
//! the same prompt still showing on the next tick — the agent has not said
//! anything since the key went in — is left alone, because a second `1` is a
//! stray character in the composer of an agent that is already working. See
//! [`due`].

use crate::session::{ActivityState, Session};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How many auto-answers each session keeps for the page to list.
///
/// Enough to show what was waved through over the last stretch of work,
/// bounded because the whole file is rewritten on every answer. The event log
/// has the full record.
const KEEP: usize = 20;

/// How long an id may go unseen as a running session before it is dropped.
///
/// Not zero, because the process that switched it on and the owner that
/// prunes are often different cctops with their own walks: a session the page
/// just saw may not be in the owner's table for another scan. Short, because
/// past it the id is a session that ended, and permission given for it should
/// not be lying around for a new one to inherit.
const UNSEEN_GRACE: Duration = Duration::from_secs(60);

/// How long after an answer a prompt that names nothing is believed to be a
/// new one.
///
/// Claude Code follows a permission request with a notification about the
/// same prompt, which carries no description. Arriving after cctop answered,
/// it puts the row back to asking about nothing in particular — and pressing
/// `1` at that types into the agent's composer. A real second prompt names its
/// tool, and is answered at once whatever this says.
const NAMELESS_COOLDOWN: Duration = Duration::from_secs(10);

/// How long a writer waits for another writer before giving up.
///
/// Writers hold the lock for one small read and one small write, so a wait
/// this long is a writer that is stopped — and a dashboard frozen behind it is
/// worse than a toggle that failed and said so.
const LOCK_PATIENCE: Duration = Duration::from_millis(500);

/// How long the hook waits for another writer before leaving an allow out of
/// the list the page shows.
///
/// Well inside [`crate::hook::DEADLINE`], because the agent is waiting on it.
/// The answer itself does not wait on this: it is decided before the write
/// starts, and a lost line in a list of the last twenty is the cheaper loss.
const HOOK_LOCK_PATIENCE: Duration = Duration::from_millis(100);

/// How long a Claude Code permission prompt is left to the hook before the
/// owner looks for its menu on screen.
///
/// The hook answers before the dialog is drawn, in milliseconds, so a prompt
/// still up after this is one it did not answer. Under
/// [`crate::hook::PERMISSION_GRACE`], because the fallback is for a YOLO
/// session, and a YOLO session's prompt is nobody else's to wait for.
pub const FALLBACK_AFTER: Duration = Duration::from_secs(2);

/// How often the owner may read one session's screen while waiting for a
/// menu that has not shown.
const LOOK_EVERY: Duration = Duration::from_secs(1);

const STATE: &str = "yolo.json";
const LOCK: &str = "yolo.lock";
const OWNER: &str = "yolo.owner";

/// One prompt cctop allowed, for the page to list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allowed {
    /// RFC 3339, like every other time a row carries.
    pub at: String,
    /// What it asked to do, in the words the row showed — or a plain
    /// description when the prompt named nothing.
    pub ask: String,
}

/// The process YOLO was switched on for: a pid, and when it started.
///
/// The pid alone is not an identity. The kernel hands pids out again, and a
/// permission that outlived its process would pass to whatever was given the
/// number next. The start time is the kernel's own (`/proc/<pid>/stat` field
/// 22, clock ticks since boot), so two processes that shared a pid in one boot
/// never share it, and the runtime directory the file lives in does not
/// outlive the boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    pub pid: u32,
    pub start: u64,
}

impl Agent {
    /// The process running as `pid` now, if there is one.
    pub fn of(pid: u32) -> Option<Agent> {
        Some(Agent {
            pid,
            start: start_time(pid)?,
        })
    }

    /// Whether this process is still running.
    pub fn is_alive(&self) -> bool {
        start_time(self.pid) == Some(self.start)
    }
}

/// When `pid` started, in clock ticks since boot.
///
/// Read from `/proc` directly, as [`crate::hook`]'s ancestry walk is: the hook
/// asks this on the agent's time.
fn start_time(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `comm` is parenthesised and may hold spaces and parens of its own, so
    // fields are counted from the last `)`: state is the first after it, and
    // starttime (field 22 of the whole line) the twentieth.
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

/// One session with YOLO on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// When it was switched on, RFC 3339.
    pub since: String,
    /// What has been allowed since, oldest first, at most [`KEEP`].
    #[serde(default)]
    pub allowed: Vec<Allowed>,
    /// The process it was switched on for. An entry without one — written by
    /// a cctop from before the hook — answers nothing and is dropped.
    #[serde(default)]
    pub agent: Option<Agent>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    sessions: BTreeMap<String, Entry>,
}

/// Where the switch and its locks live.
fn dir() -> PathBuf {
    crate::config::runtime_base().join("cctop")
}

fn stamp_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn read_state(dir: &Path) -> State {
    std::fs::read(dir.join(STATE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// An exclusive `flock`, held until the file is dropped.
fn flock(file: &std::fs::File) -> bool {
    use std::os::fd::AsRawFd;
    // SAFETY: flock on a descriptor the caller owns for the call.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

fn open_lock(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
}

/// Read, change and write the switch, with every other writer kept out.
fn update(dir: &Path, change: impl FnOnce(&mut State)) -> std::io::Result<()> {
    update_within(dir, LOCK_PATIENCE, change)
}

fn update_within(
    dir: &Path,
    patience: Duration,
    change: impl FnOnce(&mut State),
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let lock = open_lock(&dir.join(LOCK))?;
    let started = Instant::now();
    while !flock(&lock) {
        if started.elapsed() >= patience {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "another cctop is holding the YOLO file",
            ));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut state = read_state(dir);
    change(&mut state);
    let json = serde_json::to_vec(&state).map_err(std::io::Error::other)?;
    // One name is enough: only the lock holder writes it.
    let tmp = dir.join("yolo.json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, dir.join(STATE))
}

/// Switch YOLO on for `session_id` as run by the process `agent`, or off with
/// `None`, for every cctop on the machine and for the hook.
///
/// The caller decides whether the session may have it — see
/// [`crate::actions::yolo`]. Switching on a session already on keeps its
/// history and takes the new process: that is how a resumed session is
/// covered again. Switching off forgets it.
pub fn set(session_id: &str, agent: Option<u32>) -> std::io::Result<()> {
    set_in(&dir(), session_id, agent)
}

fn set_in(dir: &Path, session_id: &str, agent: Option<u32>) -> std::io::Result<()> {
    let process = match agent {
        Some(pid) => Some(Agent::of(pid).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("process {pid} is not running"),
            )
        })?),
        None => None,
    };
    let id = session_id.to_string();
    let result = update(dir, |state| match process {
        Some(process) => {
            let entry = state.sessions.entry(id).or_insert_with(|| Entry {
                since: stamp_now(),
                ..Entry::default()
            });
            entry.agent = Some(process);
        }
        None => {
            state.sessions.remove(&id);
        }
    });
    crate::elog::event(
        "yolo",
        if agent.is_some() { "on" } else { "off" },
        serde_json::json!({ "session": session_id, "pid": agent, "ok": result.is_ok() }),
    );
    result
}

/// Whether a `PermissionRequest` from `session_id`, fired by a hook whose
/// ancestors are `ancestry`, is one YOLO allows.
///
/// The whole of the hook's decision, and every way it can be no: no file, a
/// file that does not parse, no entry for the id, an entry with no process,
/// a process that is not among the hook's ancestors, or one that is but has
/// a different start time — a pid reused. See the module docs.
pub fn allows(session_id: &str, ancestry: &[u32]) -> bool {
    allows_in(&dir(), session_id, ancestry)
}

fn allows_in(dir: &Path, session_id: &str, ancestry: &[u32]) -> bool {
    if session_id.is_empty() {
        return false;
    }
    let state = read_state(dir);
    let Some(agent) = state.sessions.get(session_id).and_then(|e| e.agent) else {
        return false;
    };
    ancestry.contains(&agent.pid) && agent.is_alive()
}

/// Write down a prompt the hook allowed, for the page's list.
///
/// Best effort, on the hook's clock: see [`HOOK_LOCK_PATIENCE`].
pub fn record_allowed(session_id: &str, ask: Option<String>) {
    record_allowed_in(&dir(), session_id, ask, HOOK_LOCK_PATIENCE);
}

fn record_allowed_in(dir: &Path, session_id: &str, ask: Option<String>, patience: Duration) {
    let ask = ask.unwrap_or_else(|| "a permission prompt".to_string());
    let _ = update_within(dir, patience, |state| {
        if let Some(entry) = state.sessions.get_mut(session_id) {
            push_allowed(
                entry,
                Allowed {
                    at: stamp_now(),
                    ask,
                },
            );
        }
    });
}

/// Add one answer to an entry's list, keeping the last [`KEEP`].
fn push_allowed(entry: &mut Entry, answer: Allowed) {
    entry.allowed.push(answer);
    let over = entry.allowed.len().saturating_sub(KEEP);
    entry.allowed.drain(..over);
}

/// What this process remembers about the last prompt it answered for a
/// session.
#[derive(Debug, Clone)]
struct Seen {
    ask: Option<String>,
    at: Instant,
    /// Whether the row has stopped asking since — the sign that whatever asks
    /// next is a new prompt, even one in the same words.
    cleared: bool,
}

/// Whether a session's prompt is one to answer now, given what was last
/// answered for it. `asking` is whether a permission prompt — not a question
/// with choices — is up, and `ask` what it asks.
///
/// Pure, so the once-only rule can be read and tested without a process to
/// press keys at. The caller records the answer in `seen` *before* pressing:
/// a press that fails is not retried, because "at most once" is the promise
/// and a prompt left up is something a person can see and answer.
fn due(
    seen: &mut HashMap<String, Seen>,
    id: &str,
    asking: bool,
    ask: Option<&str>,
    now: Instant,
) -> bool {
    if !asking {
        if let Some(prev) = seen.get_mut(id) {
            prev.cleared = true;
        }
        return false;
    }
    match seen.get(id) {
        None => true,
        // The prompt that was answered, still up: the agent has not said
        // anything since the key went in. Pressing again is the stray `1`.
        Some(prev) if !prev.cleared && prev.ask.as_deref() == ask => false,
        Some(prev) if ask.is_none() => now.duration_since(prev.at) >= NAMELESS_COOLDOWN,
        Some(_) => true,
    }
}

/// The YOLO switch as one cctop process holds it: the file read and stamped
/// on rows, and — in the process that owns the job — the prompts the hook
/// does not answer pressed.
pub struct Auto {
    dir: PathBuf,
    /// Whether this process may press keys at all. A `--no-actions` serve
    /// reads the switch to show it, and never answers or competes to.
    may_answer: bool,
    /// The held `flock` on [`OWNER`], once this process has it.
    owner: Option<std::fs::File>,
    /// Which version of the file `entries` was read from: inode, length and
    /// modification time. The inode is the reliable part — every write is a
    /// rename, so every write is a new one.
    read: Option<(u64, u64, Option<std::time::SystemTime>)>,
    entries: HashMap<String, Arc<Entry>>,
    seen: HashMap<String, Seen>,
    /// When each Claude session's screen was last read for a menu, so a
    /// prompt the hook answered is not read again on every tick.
    looked: HashMap<String, Instant>,
    /// The screen reader, kept between ticks for the sweeps it caches.
    peek: Option<crate::peek::Peek>,
}

/// A press, as the auto-answer sees it: `Ok` when the key went in.
type Press<'a> = dyn FnMut(&Session) -> Result<(), String> + 'a;

/// A look at a session's screen, as the auto-answer sees it.
type Look<'a> = dyn FnMut(&Session) -> Option<crate::peek::Screened> + 'a;

impl Auto {
    pub fn new(may_answer: bool) -> Auto {
        Auto::in_dir(dir(), may_answer)
    }

    fn in_dir(dir: PathBuf, may_answer: bool) -> Auto {
        Auto {
            dir,
            may_answer,
            owner: None,
            read: None,
            entries: HashMap::new(),
            seen: HashMap::new(),
            looked: HashMap::new(),
            peek: None,
        }
    }

    /// Re-read the file when it has changed since the last read. Returns
    /// whether what it says changed.
    fn reload(&mut self) -> bool {
        use std::os::unix::fs::MetadataExt;
        let stamp = std::fs::metadata(self.dir.join(STATE))
            .ok()
            .map(|m| (m.ino(), m.len(), m.modified().ok()));
        if stamp == self.read {
            return false;
        }
        self.read = stamp;
        let entries: HashMap<String, Arc<Entry>> = read_state(&self.dir)
            .sessions
            .into_iter()
            .map(|(id, entry)| (id, Arc::new(entry)))
            .collect();
        let changed = entries != self.entries;
        self.entries = entries;
        changed
    }

    /// Put each row's YOLO entry on it, as last read. No IO, so it is cheap
    /// enough to run wherever rows are rebuilt. Returns whether a row changed.
    ///
    /// Only a running local row carries one: a stopped session has no prompt
    /// to answer and is about to lose the entry, and a remote row's prompt is
    /// another machine's to answer. And only the row whose agent is the
    /// process the switch was turned on for — a session resumed in a new
    /// process shows YOLO off, because the hook will not answer for it.
    pub fn stamp(&self, sessions: &mut [Session]) -> bool {
        let mut changed = false;
        for session in sessions {
            let want = match session.remote.is_none() && session.is_running() {
                true => self
                    .entries
                    .get(&session.session_id)
                    .filter(|entry| {
                        entry
                            .agent
                            .is_some_and(|agent| Some(agent.pid) == session.root_pid())
                    })
                    .cloned(),
                false => None,
            };
            if want != session.yolo {
                session.yolo = want;
                changed = true;
            }
        }
        changed
    }

    /// Read the switch, stamp it on the rows, and — when this process owns
    /// the job — press for every YOLO session's open permission prompt the
    /// hook has not answered, and drop the ids whose session has ended.
    /// Returns whether any row changed.
    ///
    /// `reports` is what the hooks have said, which is where a Claude
    /// prompt still inside its grace is seen: see [`FALLBACK_AFTER`].
    pub fn tick(&mut self, sessions: &mut [Session], reports: &crate::hook::Reports) -> bool {
        let mut peek = self.peek.take().unwrap_or_default();
        let changed = self.tick_with(
            sessions,
            reports,
            &mut |session| {
                crate::actions::answer(session, "allow")
                    .map(|_| ())
                    .map_err(|(_, why)| why)
            },
            &mut |session| peek.read(session.provider.as_str(), session.root_pid()?),
        );
        self.peek = Some(peek);
        changed
    }

    fn tick_with(
        &mut self,
        sessions: &mut [Session],
        reports: &crate::hook::Reports,
        press: &mut Press<'_>,
        look: &mut Look<'_>,
    ) -> bool {
        let mut changed = self.reload();
        changed |= self.stamp(sessions);
        self.seen
            .retain(|id, _| sessions.iter().any(|s| &s.session_id == id));
        self.looked
            .retain(|id, _| sessions.iter().any(|s| &s.session_id == id));
        if self.entries.is_empty() || !self.may_answer || !self.own() {
            return changed;
        }

        let now = Instant::now();
        let mut allowed: Vec<(String, Allowed)> = Vec::new();
        for session in sessions.iter_mut() {
            let id = session.session_id.clone();
            let Some(entry) = session.yolo.clone() else {
                due(&mut self.seen, &id, false, None, now);
                continue;
            };
            let row_asking =
                session.activity_state == ActivityState::Asking && !session.asking_question;
            let claude = session.provider == crate::pricing::Provider::Claude;
            // A Claude prompt the hook has had its chance at, whether or not
            // the row shows it yet: the grace that holds it back from the row
            // is for auto mode, which a YOLO session has no use for.
            let held = reports.report(&id).filter(|r| {
                claude
                    && r.signal == crate::hook::Signal::NeedsInput
                    && !r.question
                    && r.at.elapsed() >= FALLBACK_AFTER
            });
            let ask = match row_asking {
                true => session.asking_for.clone(),
                false => held.and_then(|r| r.ask.clone()),
            };
            if !due(
                &mut self.seen,
                &id,
                row_asking || held.is_some(),
                ask.as_deref(),
                now,
            ) {
                continue;
            }
            if claude {
                // The hook is the answer for Claude Code, and a prompt it
                // answered never draws its menu. So the menu on screen is the
                // only evidence a key press is wanted — anything else, or a
                // screen cctop cannot read, and the `1` would land in the
                // composer.
                if self
                    .looked
                    .get(&id)
                    .is_some_and(|at| now.duration_since(*at) < LOOK_EVERY)
                {
                    continue;
                }
                self.looked.insert(id.clone(), now);
                let Some(menu) = look(session)
                    .filter(|s| s.signal == crate::hook::Signal::NeedsInput && !s.question)
                else {
                    continue;
                };
                crate::elog::event("yolo", "fallback", serde_json::json!({ "session": id }));
                session.activity_state = ActivityState::Asking;
                session.asking_question = false;
                session.asking_for = ask.clone().or(menu.ask);
            }
            // The process the switch was turned on for, checked to the start
            // time before a key goes in: a pid alone could be a stranger's.
            if !entry.agent.is_some_and(|agent| agent.is_alive()) {
                continue;
            }
            self.seen.insert(
                id.clone(),
                Seen {
                    ask: ask.clone(),
                    at: now,
                    cleared: false,
                },
            );
            let ask = session
                .asking_for
                .clone()
                .unwrap_or_else(|| "a permission prompt".to_string());
            match press(session) {
                Ok(()) => {
                    crate::elog::event(
                        "yolo",
                        "allowed",
                        serde_json::json!({ "session": id, "ask": ask }),
                    );
                    // Down now rather than at the next report: the page would
                    // otherwise offer Allow for a tick on a prompt that has
                    // been answered.
                    session.activity_state = ActivityState::Working;
                    session.asking_for = None;
                    session.asking_question = false;
                    allowed.push((
                        id,
                        Allowed {
                            at: stamp_now(),
                            ask,
                        },
                    ));
                    changed = true;
                }
                Err(why) => crate::elog::event(
                    "yolo",
                    "failed",
                    serde_json::json!({ "session": id, "ask": ask, "why": why }),
                ),
            }
        }

        let ended = self.ended(sessions);
        if !allowed.is_empty() || !ended.is_empty() {
            let result = update(&self.dir, |state| {
                for id in &ended {
                    state.sessions.remove(id);
                }
                for (id, answer) in allowed {
                    if let Some(entry) = state.sessions.get_mut(&id) {
                        push_allowed(entry, answer);
                    }
                }
            });
            if !ended.is_empty() {
                crate::elog::event(
                    "yolo",
                    "ended",
                    serde_json::json!({ "sessions": ended, "ok": result.is_ok() }),
                );
            }
            changed |= self.reload();
            changed |= self.stamp(sessions);
        }
        changed
    }

    /// The ids whose permission has run out: the process it was given to has
    /// gone, or the session is no longer running here past the grace.
    ///
    /// The process is checked outright, with no grace, because the kernel's
    /// answer does not depend on how recently this cctop walked the session.
    fn ended(&self, sessions: &[Session]) -> Vec<String> {
        let now = chrono::Utc::now();
        self.entries
            .iter()
            .filter(|(id, entry)| {
                let gone = !entry.agent.is_some_and(|agent| agent.is_alive());
                let running = sessions
                    .iter()
                    .any(|s| &s.session_id == *id && s.remote.is_none() && s.is_running());
                let old = chrono::DateTime::parse_from_rfc3339(&entry.since)
                    .map(|since| {
                        (now - since.with_timezone(&chrono::Utc))
                            .to_std()
                            .unwrap_or_default()
                            >= UNSEEN_GRACE
                    })
                    .unwrap_or(true);
                gone || (!running && old)
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Take the job of answering, if nobody has it. Kept once taken.
    fn own(&mut self) -> bool {
        if self.owner.is_some() {
            return true;
        }
        let _ = std::fs::create_dir_all(&self.dir);
        let Ok(file) = open_lock(&self.dir.join(OWNER)) else {
            return false;
        };
        if !flock(&file) {
            return false;
        }
        crate::elog::event(
            "yolo",
            "owner",
            serde_json::json!({ "pid": std::process::id() }),
        );
        self.owner = Some(file);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::{Reported, Reports, Signal};
    use crate::peek::Screened;
    use crate::pricing::Provider;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cctop-yolo-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The agent every test's rows run as: this test process, which is alive
    /// and has a start time the kernel will vouch for.
    fn me() -> u32 {
        std::process::id()
    }

    /// Switch YOLO on for `id`, run by this test process.
    fn on(dir: &Path, id: &str) {
        set_in(dir, id, Some(me())).unwrap();
    }

    /// A running local `provider` session, asking `ask`.
    fn asking_as(provider: Provider, id: &str, ask: Option<&str>) -> Session {
        let mut s = Session::new(provider, id.into());
        s.process = Some(crate::proc::ProcInfo {
            pids: 1,
            process_list: vec![crate::proc::ProcEntry {
                pid: me(),
                cpu: 0.0,
                memory: 0,
                args: provider.as_str().into(),
                is_root: true,
                ghost: false,
            }],
            ..Default::default()
        });
        s.activity_state = ActivityState::Asking;
        s.asking_for = ask.map(str::to_string);
        s
    }

    /// A Codex session asking: the harness the key press is still the answer
    /// for, so the once-only rules are tested on it.
    fn asking(id: &str, ask: Option<&str>) -> Session {
        asking_as(Provider::Codex, id, ask)
    }

    fn quiet(s: &mut Session) {
        s.activity_state = ActivityState::Working;
        s.asking_for = None;
    }

    /// No hook has said anything.
    fn silence() -> Reports {
        Reports::default()
    }

    /// Reports holding a `PermissionRequest` for `id` raised `ago`, as the
    /// observer hook reports it.
    fn raised(id: &str, ask: Option<&str>, question: bool, ago: Duration) -> Reports {
        let mut reports = Reports::default();
        reports.hooked.insert(
            id.into(),
            Reported {
                signal: Signal::NeedsInput,
                cwd: "/w".into(),
                permission: None,
                at: Instant::now() - ago,
                provisional: true,
                ask: ask.map(str::to_string),
                question,
            },
        );
        reports
    }

    /// A look that must not happen.
    fn blind(_: &Session) -> Option<Screened> {
        panic!("a screen was read")
    }

    /// Claude Code's permission menu, as the recognizer reads it.
    fn menu(_: &Session) -> Option<Screened> {
        Some(Screened {
            signal: Signal::NeedsInput,
            ask: Some("Bash: rm -rf build".into()),
            question: false,
        })
    }

    /// One prompt seen on several ticks is answered once; the next prompt —
    /// in other words, or in the same words after the row stopped asking —
    /// is answered again.
    #[test]
    fn a_prompt_is_answered_once_and_the_next_one_again() {
        let mut seen = HashMap::new();
        let t = Instant::now();
        let answer = |seen: &mut HashMap<String, Seen>, asking: bool, ask: Option<&str>, at| {
            let due = due(seen, "a", asking, ask, at);
            if due {
                seen.insert(
                    "a".into(),
                    Seen {
                        ask: ask.map(str::to_string),
                        at,
                        cleared: false,
                    },
                );
            }
            due
        };
        assert!(answer(&mut seen, true, Some("Bash: ls"), t));
        for _ in 0..5 {
            assert!(
                !answer(&mut seen, true, Some("Bash: ls"), t),
                "the same prompt was pressed twice"
            );
        }
        // A different prompt, before the row ever stopped asking.
        assert!(answer(&mut seen, true, Some("Bash: rm -rf build"), t));
        // The same words again, after the row stopped asking in between.
        assert!(!answer(&mut seen, false, None, t));
        assert!(answer(&mut seen, true, Some("Bash: rm -rf build"), t));
        // A nameless prompt straight after an answer is the notification for
        // the one just answered; later, it is a prompt.
        assert!(!answer(&mut seen, false, None, t));
        assert!(!answer(&mut seen, true, None, t + Duration::from_secs(1)));
        assert!(answer(&mut seen, true, None, t + NAMELESS_COOLDOWN));
    }

    /// A question with choices is the person's, and so is any prompt on a
    /// session without YOLO.
    #[test]
    fn questions_and_sessions_without_yolo_are_left_alone() {
        let dir = scratch("questions");
        on(&dir, "q");
        let mut auto = Auto::in_dir(dir.clone(), true);
        let mut question = asking("q", Some("Which framework?"));
        question.asking_question = true;
        let plain = asking("p", Some("Bash: ls"));
        let mut rows = vec![question, plain];
        auto.tick_with(
            &mut rows,
            &silence(),
            &mut |s| panic!("pressed at {}", s.session_id),
            &mut blind,
        );
        assert!(rows[0].yolo.is_some() && rows[1].yolo.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The switch survives the trip through the file, and a session that has
    /// stopped loses it — but not one merely not seen yet.
    #[test]
    fn the_switch_round_trips_and_ends_with_its_session() {
        let dir = scratch("roundtrip");
        on(&dir, "a");
        on(&dir, "b");
        on(&dir, "c");
        set_in(&dir, "c", None).unwrap();
        let state = read_state(&dir);
        assert_eq!(state.sessions.keys().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(state.sessions["a"].agent, Agent::of(me()));

        let mut auto = Auto::in_dir(dir.clone(), true);
        let mut running = asking("a", None);
        quiet(&mut running);
        let stopped = Session::new(Provider::Codex, "b".into());
        let mut rows = vec![running, stopped];
        let mut never = |_: &Session| -> Result<(), String> { panic!("nothing is asking") };
        assert!(auto.tick_with(&mut rows, &silence(), &mut never, &mut blind));
        assert!(rows[0].yolo.is_some(), "a running row carries its switch");
        assert!(rows[1].yolo.is_none(), "a stopped row does not");
        // Inside the grace both stay: the owner may not have walked it yet.
        assert_eq!(read_state(&dir).sessions.len(), 2);

        // Past the grace, the stopped one goes and the running one stays.
        update(&dir, |s| {
            for entry in s.sessions.values_mut() {
                entry.since = "2000-01-01T00:00:00Z".into();
            }
        })
        .unwrap();
        auto.tick_with(&mut rows, &silence(), &mut never, &mut blind);
        assert_eq!(
            read_state(&dir).sessions.keys().collect::<Vec<_>>(),
            ["a"],
            "the ended session kept its switch"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Switching on records the process, and refuses one that is not there.
    /// Switching on again takes the new process and keeps the history.
    #[test]
    fn switching_on_records_the_agent_process() {
        let dir = scratch("agent");
        let err = set_in(&dir, "a", Some(u32::MAX)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(read_state(&dir).sessions.is_empty());

        update(&dir, |s| {
            s.sessions.insert(
                "a".into(),
                Entry {
                    since: "2000-01-01T00:00:00Z".into(),
                    allowed: vec![Allowed {
                        at: "2000-01-01T00:00:01Z".into(),
                        ask: "Bash: ls".into(),
                    }],
                    agent: Some(Agent { pid: 1, start: 1 }),
                },
            );
        })
        .unwrap();
        on(&dir, "a");
        let entry = &read_state(&dir).sessions["a"];
        assert_eq!(entry.agent, Agent::of(me()));
        assert_eq!(entry.allowed.len(), 1, "the history went");
        assert_eq!(entry.since, "2000-01-01T00:00:00Z");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The hook's whole decision: the id and the process both, or nothing.
    #[test]
    fn the_hook_allows_only_the_recorded_session_in_the_recorded_process() {
        let dir = scratch("allows");
        // Nothing written yet, then a file that is not JSON.
        assert!(!allows_in(&dir, "a", &[me()]));
        std::fs::write(dir.join(STATE), b"{not json").unwrap();
        assert!(!allows_in(&dir, "a", &[me()]));

        on(&dir, "a");
        let parent = std::os::unix::process::parent_id();
        // Right id, and the recorded process is an ancestor: allowed.
        assert!(allows_in(&dir, "a", &[parent, me(), 1]));
        // Right id, another process: a resumed session, or another agent
        // reporting this id.
        assert!(!allows_in(&dir, "a", &[parent]));
        assert!(!allows_in(&dir, "a", &[]));
        // Another id, from the very process YOLO is on for.
        assert!(!allows_in(&dir, "b", &[me()]));
        assert!(!allows_in(&dir, "", &[me()]));

        // The same pid with another start time is a reused pid.
        update(&dir, |s| {
            let agent = s.sessions.get_mut("a").unwrap().agent.as_mut().unwrap();
            agent.start += 1;
        })
        .unwrap();
        assert!(!allows_in(&dir, "a", &[me()]));
        // A pid nothing runs as.
        update(&dir, |s| {
            s.sessions.get_mut("a").unwrap().agent = Some(Agent {
                pid: u32::MAX,
                start: 1,
            });
        })
        .unwrap();
        assert!(!allows_in(&dir, "a", &[u32::MAX]));
        // An entry from before the process was recorded.
        update(&dir, |s| s.sessions.get_mut("a").unwrap().agent = None).unwrap();
        assert!(!allows_in(&dir, "a", &[me()]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What the hook allowed is listed for the page.
    #[test]
    fn the_hooks_allow_is_listed() {
        let dir = scratch("record");
        on(&dir, "a");
        record_allowed_in(&dir, "a", Some("Bash: ls".into()), HOOK_LOCK_PATIENCE);
        record_allowed_in(&dir, "a", None, HOOK_LOCK_PATIENCE);
        record_allowed_in(&dir, "nobody", Some("Bash: ls".into()), HOOK_LOCK_PATIENCE);
        let state = read_state(&dir);
        let asks: Vec<_> = state.sessions["a"].allowed.iter().map(|a| &a.ask).collect();
        assert_eq!(asks, ["Bash: ls", "a permission prompt"]);
        assert!(!state.sessions.contains_key("nobody"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A switch written without ever opening `yolo.lock`, so the lock a test
    /// then takes is one no earlier descriptor can be holding.
    ///
    /// The reason it matters: an `flock` belongs to the open file description,
    /// and a fork copies every descriptor the process has — `O_CLOEXEC` only
    /// closes them at the child's exec. Any test thread spawning a process
    /// while another test's [`update`] had the lock open gave that child a
    /// copy, and the lock stayed taken after `update` dropped its own, until
    /// the child exec'd. `the_hooks_allow_is_listed_and_never_waits_long` took
    /// the lock straight after three `update`s and failed in CI on exactly
    /// that window. A lock file nobody has opened has no copies to outlive.
    fn switched_on_unlocked(dir: &Path, id: &str) {
        let mut state = State::default();
        state.sessions.insert(
            id.into(),
            Entry {
                since: stamp_now(),
                allowed: Vec::new(),
                agent: Agent::of(me()),
            },
        );
        std::fs::write(dir.join(STATE), serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(!dir.join(LOCK).exists());
    }

    /// Another writer holding the file past the hook's patience leaves the
    /// allow out of the list rather than waited for.
    #[test]
    fn the_hooks_allow_never_waits_long() {
        let dir = scratch("record-busy");
        switched_on_unlocked(&dir, "a");
        let held = open_lock(&dir.join(LOCK)).unwrap();
        assert!(flock(&held), "a lock file nobody else opened was taken");
        let started = Instant::now();
        record_allowed_in(
            &dir,
            "a",
            Some("Bash: make".into()),
            Duration::from_millis(20),
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(read_state(&dir).sessions["a"].allowed.is_empty());
        drop(held);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A row whose agent is not the process the switch was turned on for —
    /// the session resumed, or the process gone and its pid reused — does
    /// not carry the switch and gets no key press, and an entry whose process
    /// is gone is dropped.
    #[test]
    fn another_process_on_the_same_session_is_not_covered() {
        let dir = scratch("resumed");
        on(&dir, "a");
        let mut auto = Auto::in_dir(dir.clone(), true);
        let mut resumed = asking("a", Some("Bash: ls"));
        resumed.process.as_mut().unwrap().process_list[0].pid = u32::MAX;
        let mut rows = vec![resumed];
        auto.tick_with(
            &mut rows,
            &silence(),
            &mut |_| panic!("pressed for a resumed session"),
            &mut blind,
        );
        assert!(rows[0].yolo.is_none());

        // The recorded pid, with a start time the process does not have.
        update(&dir, |s| {
            s.sessions
                .get_mut("a")
                .unwrap()
                .agent
                .as_mut()
                .unwrap()
                .start += 1;
        })
        .unwrap();
        let mut rows = vec![asking("a", Some("Bash: ls"))];
        auto.tick_with(
            &mut rows,
            &silence(),
            &mut |_| panic!("pressed for a reused pid"),
            &mut blind,
        );
        assert!(
            read_state(&dir).sessions.is_empty(),
            "the stale entry stayed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One process answers; a second one reading the same file does not
    /// press, until the first is gone.
    #[test]
    fn only_one_process_answers() {
        let dir = scratch("owner");
        on(&dir, "a");
        let mut first = Auto::in_dir(dir.clone(), true);
        let mut second = Auto::in_dir(dir.clone(), true);
        let mut presses = 0;
        let mut rows = vec![asking("a", Some("Bash: ls"))];
        first.tick_with(
            &mut rows,
            &silence(),
            &mut |_| {
                presses += 1;
                Ok(())
            },
            &mut blind,
        );
        // flock belongs to the open file, so two in one process exclude each
        // other exactly as two processes would.
        let mut rows = vec![asking("a", Some("Bash: make"))];
        second.tick_with(
            &mut rows,
            &silence(),
            &mut |_| {
                presses += 10;
                Ok(())
            },
            &mut blind,
        );
        assert_eq!(presses, 1, "the second cctop pressed too");
        assert!(rows[0].yolo.is_some(), "the second cctop still shows it");

        // The owner goes; the next one to look takes over. Looked for rather
        // than expected at the first look: the lock lives as long as any copy
        // of its descriptor, and a test on another thread that spawns a
        // process holds a copy from its fork until its exec. Under load that
        // window is long enough to be the one this tick lands in.
        drop(first);
        crate::test_wait::eventually_true("the second cctop to take over", || {
            second.tick_with(
                &mut rows,
                &silence(),
                &mut |_| {
                    presses += 10;
                    Ok(())
                },
                &mut blind,
            );
            presses > 1
        });
        assert_eq!(presses, 11);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A serve that may not act shows the switch and never presses.
    #[test]
    fn a_read_only_process_never_presses() {
        let dir = scratch("readonly");
        on(&dir, "a");
        let mut auto = Auto::in_dir(dir.clone(), false);
        let mut rows = vec![asking("a", Some("Bash: ls"))];
        auto.tick_with(
            &mut rows,
            &silence(),
            &mut |_| panic!("a read-only serve pressed"),
            &mut blind,
        );
        assert!(rows[0].yolo.is_some());
        assert!(auto.owner.is_none(), "it competed for the job");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Each answer is written down for the page, and the row stops asking.
    #[test]
    fn an_answer_is_recorded_and_the_prompt_comes_down() {
        let dir = scratch("answered");
        on(&dir, "a");
        let mut auto = Auto::in_dir(dir.clone(), true);
        let mut rows = vec![asking("a", Some("Bash: cargo test"))];
        assert!(auto.tick_with(&mut rows, &silence(), &mut |_| Ok(()), &mut blind));
        assert_eq!(rows[0].activity_state, ActivityState::Working);
        let entry = rows[0].yolo.clone().expect("still on");
        assert_eq!(entry.allowed.len(), 1);
        assert_eq!(entry.allowed[0].ask, "Bash: cargo test");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// For Claude Code the hook is the answer: a row asking with no menu on
    /// screen — the hook allowed it, and the report that says so has not
    /// landed — gets no key press, and neither does a screen cctop cannot
    /// read, or a question on one.
    #[test]
    fn a_claude_prompt_is_pressed_only_with_its_menu_on_screen() {
        let dir = scratch("claude-screen");
        on(&dir, "a");
        let mut auto = Auto::in_dir(dir.clone(), true);
        let row = asking_as(Provider::Claude, "a", Some("Bash: rm -rf build"));
        let screens: [fn(&Session) -> Option<Screened>; 3] = [
            |_| None,
            |_| {
                Some(Screened {
                    signal: Signal::Busy,
                    ask: None,
                    question: false,
                })
            },
            |_| {
                Some(Screened {
                    signal: Signal::NeedsInput,
                    ask: Some("Deploy".into()),
                    question: true,
                })
            },
        ];
        for mut screen in screens {
            // A fresh process each time, so the once-a-second look is not
            // what keeps the key in.
            auto.looked.clear();
            let mut rows = vec![row.clone()];
            auto.tick_with(
                &mut rows,
                &silence(),
                &mut |_| panic!("pressed without the menu on screen"),
                &mut screen,
            );
        }
        let mut presses = 0;
        auto.looked.clear();
        let mut rows = vec![row.clone()];
        auto.tick_with(
            &mut rows,
            &silence(),
            &mut |s| {
                assert_eq!(s.activity_state, ActivityState::Asking);
                presses += 1;
                Ok(())
            },
            &mut menu,
        );
        assert_eq!(presses, 1);
        // And the same prompt still on screen a tick later is not pressed
        // twice.
        auto.looked.clear();
        auto.tick_with(
            &mut [row.clone()],
            &silence(),
            &mut |_| panic!("the same menu was pressed twice"),
            &mut menu,
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A Claude prompt the hook is still inside its time for is left alone,
    /// screen unread; one still held past [`FALLBACK_AFTER`] with its menu on
    /// screen is pressed, without waiting for the grace that would put it on
    /// the row.
    #[test]
    fn a_claude_prompt_falls_back_to_the_key_after_the_hooks_time() {
        let dir = scratch("claude-fallback");
        on(&dir, "a");
        let mut auto = Auto::in_dir(dir.clone(), true);
        let mut row = asking_as(Provider::Claude, "a", None);
        quiet(&mut row);

        let fresh = raised("a", Some("Bash: rm -rf build"), false, Duration::ZERO);
        auto.tick_with(
            &mut [row.clone()],
            &fresh,
            &mut |_| panic!("pressed while the hook could still answer"),
            &mut blind,
        );

        let question = raised("a", Some("Deploy"), true, FALLBACK_AFTER);
        auto.tick_with(
            &mut [row.clone()],
            &question,
            &mut |_| panic!("pressed at a question"),
            &mut blind,
        );

        let stuck = raised("a", Some("Bash: rm -rf build"), false, FALLBACK_AFTER);
        let mut pressed = Vec::new();
        let mut rows = vec![row.clone()];
        auto.tick_with(
            &mut rows,
            &stuck,
            &mut |s| {
                pressed.push(s.asking_for.clone());
                Ok(())
            },
            &mut menu,
        );
        assert_eq!(pressed, [Some("Bash: rm -rf build".to_string())]);
        assert_eq!(
            read_state(&dir).sessions["a"].allowed[0].ask,
            "Bash: rm -rf build"
        );
        // Held on the next tick still — the answer's own report has not
        // landed — and not pressed again.
        auto.looked.clear();
        auto.tick_with(
            &mut [row.clone()],
            &stuck,
            &mut |_| panic!("the fallback pressed twice"),
            &mut menu,
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// End to end against a fake agent: a prompt that stays up for several
    /// ticks gets exactly one keypress, and the next prompt one more — pressed
    /// through `actions::answer` and the real injection path, into a socket
    /// standing where `cctop run`'s shim would.
    #[test]
    fn one_keypress_per_prompt_through_the_real_answer() {
        use std::io::Read;
        let _base = crate::config::claim_test_runtime_base("yolo-e2e");
        let path = crate::shim::socket_path(me()).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let drain = |listener: &std::os::unix::net::UnixListener| {
            let mut keys = Vec::new();
            while let Ok((mut conn, _)) = listener.accept() {
                conn.set_nonblocking(false).unwrap();
                let mut got = Vec::new();
                let _ = conn.read_to_end(&mut got);
                keys.extend(got);
            }
            keys
        };

        set("e2e", Some(me())).unwrap();
        let mut auto = Auto::new(true);
        let mut row = asking("e2e", Some("Bash: ls"));
        for _ in 0..4 {
            // The row is rebuilt as asking on every tick, the way a report
            // that has not been superseded rebuilds it.
            let mut rows = vec![row.clone()];
            auto.tick(&mut rows, &silence());
        }
        assert_eq!(drain(&listener), b"y", "one prompt, one press");

        quiet(&mut row);
        auto.tick(&mut [row.clone()], &silence());
        row.activity_state = ActivityState::Asking;
        row.asking_for = Some("Bash: ls -la".into());
        for _ in 0..3 {
            auto.tick(&mut [row.clone()], &silence());
        }
        assert_eq!(drain(&listener), b"y", "the next prompt, one more press");
        let allowed = &auto.entries["e2e"].allowed;
        assert_eq!(allowed.len(), 2);
        let _ = std::fs::remove_file(&path);
    }
}
