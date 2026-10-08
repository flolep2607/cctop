//! YOLO: a session whose every permission prompt cctop answers with "allow".
//!
//! Switched on per session, from the page or the table, by someone who has
//! decided this one agent may do whatever it asks to. It is not a harness
//! setting and not a hook decision — `cctop hook` never returns one, see its
//! module docs. It is cctop pressing the same key a person clicking Allow
//! presses, through the same [`crate::actions::answer`], with every
//! guard that has: only while the row says it is asking, never at a question
//! with choices, never at a harness whose menu cctop has not driven, never at
//! a row from another machine.
//!
//! # Where the switch lives
//!
//! In one small file in the runtime directory, `yolo.json`, which every cctop
//! on the machine reads: the dashboard, the serve it hosts, a standalone
//! `cctop serve`. The runtime directory because a session does not outlive a
//! boot, and neither should permission to answer for it. Written through a
//! temporary file and a rename, under an `flock` on `yolo.lock`, so a reader
//! sees the old set or the new one and two writers cannot lose each other's
//! change. An id is dropped once its session has stopped — "until the session
//! ends" is part of what the person agreed to.
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

/// One session with YOLO on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// When it was switched on, RFC 3339.
    pub since: String,
    /// What has been allowed since, oldest first, at most [`KEEP`].
    #[serde(default)]
    pub allowed: Vec<Allowed>,
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
    std::fs::create_dir_all(dir)?;
    let lock = open_lock(&dir.join(LOCK))?;
    let started = Instant::now();
    while !flock(&lock) {
        if started.elapsed() >= LOCK_PATIENCE {
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

/// Switch YOLO on or off for `session_id`, for every cctop on the machine.
///
/// The caller decides whether the session may have it — see
/// [`crate::actions::yolo`]. Switching on a session already on keeps
/// its history; switching off forgets it.
pub fn set(session_id: &str, on: bool) -> std::io::Result<()> {
    set_in(&dir(), session_id, on)
}

fn set_in(dir: &Path, session_id: &str, on: bool) -> std::io::Result<()> {
    let id = session_id.to_string();
    let result = update(dir, |state| match on {
        true => {
            state.sessions.entry(id).or_insert_with(|| Entry {
                since: stamp_now(),
                allowed: Vec::new(),
            });
        }
        false => {
            state.sessions.remove(&id);
        }
    });
    crate::elog::event(
        "yolo",
        if on { "on" } else { "off" },
        serde_json::json!({ "session": session_id, "ok": result.is_ok() }),
    );
    result
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

/// Whether `session`'s prompt is one to answer now, given what was last
/// answered for it.
///
/// Pure, so the once-only rule can be read and tested without a process to
/// press keys at. The caller records the answer in `seen` *before* pressing:
/// a press that fails is not retried, because "at most once" is the promise
/// and a prompt left up is something a person can see and answer.
fn due(seen: &mut HashMap<String, Seen>, session: &Session, now: Instant) -> bool {
    let asking = session.activity_state == ActivityState::Asking && !session.asking_question;
    if !asking || session.yolo.is_none() {
        if let Some(prev) = seen.get_mut(&session.session_id) {
            prev.cleared = true;
        }
        return false;
    }
    match seen.get(&session.session_id) {
        None => true,
        // The prompt that was answered, still up: the agent has not said
        // anything since the key went in. Pressing again is the stray `1`.
        Some(prev) if !prev.cleared && prev.ask == session.asking_for => false,
        Some(prev) if session.asking_for.is_none() => {
            now.duration_since(prev.at) >= NAMELESS_COOLDOWN
        }
        Some(_) => true,
    }
}

/// The YOLO switch as one cctop process holds it: the file read and stamped
/// on rows, and — in the process that owns the job — the prompts answered.
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
}

/// A press, as the auto-answer sees it: `Ok` when the key went in.
type Press<'a> = dyn FnMut(&Session) -> Result<(), String> + 'a;

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
    /// another machine's to answer.
    pub fn stamp(&self, sessions: &mut [Session]) -> bool {
        let mut changed = false;
        for session in sessions {
            let want = match session.remote.is_none() && session.is_running() {
                true => self.entries.get(&session.session_id).cloned(),
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
    /// the job — answer every YOLO session's open permission prompt and drop
    /// the ids whose session has ended. Returns whether any row changed.
    pub fn tick(&mut self, sessions: &mut [Session]) -> bool {
        self.tick_with(sessions, &mut |session| {
            crate::actions::answer(session, "allow")
                .map(|_| ())
                .map_err(|(_, why)| why)
        })
    }

    fn tick_with(&mut self, sessions: &mut [Session], press: &mut Press<'_>) -> bool {
        let mut changed = self.reload();
        changed |= self.stamp(sessions);
        self.seen
            .retain(|id, _| sessions.iter().any(|s| &s.session_id == id));
        if self.entries.is_empty() || !self.may_answer || !self.own() {
            return changed;
        }

        let now = Instant::now();
        let mut allowed: Vec<(String, Allowed)> = Vec::new();
        for session in sessions.iter_mut() {
            if !due(&mut self.seen, session, now) {
                continue;
            }
            self.seen.insert(
                session.session_id.clone(),
                Seen {
                    ask: session.asking_for.clone(),
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
                        serde_json::json!({ "session": session.session_id, "ask": ask }),
                    );
                    // Down now rather than at the next report: the page would
                    // otherwise offer Allow for a tick on a prompt that has
                    // been answered.
                    session.activity_state = ActivityState::Working;
                    session.asking_for = None;
                    session.asking_question = false;
                    allowed.push((
                        session.session_id.clone(),
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
                    serde_json::json!({ "session": session.session_id, "ask": ask, "why": why }),
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
                        entry.allowed.push(answer);
                        let over = entry.allowed.len().saturating_sub(KEEP);
                        entry.allowed.drain(..over);
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

    /// The ids whose session is no longer running here, past the grace.
    fn ended(&self, sessions: &[Session]) -> Vec<String> {
        let now = chrono::Utc::now();
        self.entries
            .iter()
            .filter(|(id, entry)| {
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
                !running && old
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

    /// A running local session, asking `ask`.
    fn asking(id: &str, ask: Option<&str>) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.process = Some(crate::proc::ProcInfo {
            pids: 1,
            process_list: vec![crate::proc::ProcEntry {
                pid: 999_999_999,
                cpu: 0.0,
                memory: 0,
                args: "claude".into(),
                is_root: true,
                ghost: false,
            }],
            ..Default::default()
        });
        s.activity_state = ActivityState::Asking;
        s.asking_for = ask.map(str::to_string);
        s
    }

    fn quiet(s: &mut Session) {
        s.activity_state = ActivityState::Working;
        s.asking_for = None;
    }

    /// One prompt seen on several ticks is answered once; the next prompt —
    /// in other words, or in the same words after the row stopped asking —
    /// is answered again.
    #[test]
    fn a_prompt_is_answered_once_and_the_next_one_again() {
        let mut seen = HashMap::new();
        let mut s = asking("a", Some("Bash: ls"));
        s.yolo = Some(Arc::default());
        let t = Instant::now();
        let answer = |seen: &mut HashMap<String, Seen>, s: &Session, at| {
            let due = due(seen, s, at);
            if due {
                seen.insert(
                    s.session_id.clone(),
                    Seen {
                        ask: s.asking_for.clone(),
                        at,
                        cleared: false,
                    },
                );
            }
            due
        };
        assert!(answer(&mut seen, &s, t));
        for _ in 0..5 {
            assert!(
                !answer(&mut seen, &s, t),
                "the same prompt was pressed twice"
            );
        }
        // A different prompt, before the row ever stopped asking.
        s.asking_for = Some("Bash: rm -rf build".into());
        assert!(answer(&mut seen, &s, t));
        // The same words again, after the row stopped asking in between.
        quiet(&mut s);
        assert!(!answer(&mut seen, &s, t));
        s.activity_state = ActivityState::Asking;
        s.asking_for = Some("Bash: rm -rf build".into());
        assert!(answer(&mut seen, &s, t));
        // A nameless prompt straight after an answer is the notification for
        // the one just answered; later, it is a prompt.
        quiet(&mut s);
        assert!(!answer(&mut seen, &s, t));
        s.activity_state = ActivityState::Asking;
        assert!(!answer(&mut seen, &s, t + Duration::from_secs(1)));
        assert!(answer(&mut seen, &s, t + NAMELESS_COOLDOWN));
    }

    /// A question with choices is the person's, and so is any prompt on a
    /// session without YOLO.
    #[test]
    fn questions_and_sessions_without_yolo_are_left_alone() {
        let mut seen = HashMap::new();
        let mut s = asking("q", Some("Which framework?"));
        s.yolo = Some(Arc::default());
        s.asking_question = true;
        assert!(!due(&mut seen, &s, Instant::now()));
        let plain = asking("p", Some("Bash: ls"));
        assert!(!due(&mut seen, &plain, Instant::now()));
    }

    /// The switch survives the trip through the file, and a session that has
    /// stopped loses it — but not one merely not seen yet.
    #[test]
    fn the_switch_round_trips_and_ends_with_its_session() {
        let dir = scratch("roundtrip");
        set_in(&dir, "a", true).unwrap();
        set_in(&dir, "b", true).unwrap();
        set_in(&dir, "c", true).unwrap();
        set_in(&dir, "c", false).unwrap();
        let state = read_state(&dir);
        assert_eq!(state.sessions.keys().collect::<Vec<_>>(), ["a", "b"]);

        let mut auto = Auto::in_dir(dir.clone(), true);
        let mut running = asking("a", None);
        quiet(&mut running);
        let stopped = Session::new(Provider::Claude, "b".into());
        let mut rows = vec![running, stopped];
        let mut never = |_: &Session| -> Result<(), String> { panic!("nothing is asking") };
        assert!(auto.tick_with(&mut rows, &mut never));
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
        auto.tick_with(&mut rows, &mut never);
        assert_eq!(
            read_state(&dir).sessions.keys().collect::<Vec<_>>(),
            ["a"],
            "the ended session kept its switch"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One process answers; a second one reading the same file does not
    /// press, until the first is gone.
    #[test]
    fn only_one_process_answers() {
        let dir = scratch("owner");
        set_in(&dir, "a", true).unwrap();
        let mut first = Auto::in_dir(dir.clone(), true);
        let mut second = Auto::in_dir(dir.clone(), true);
        let mut presses = 0;
        let mut rows = vec![asking("a", Some("Bash: ls"))];
        first.tick_with(&mut rows, &mut |_| {
            presses += 1;
            Ok(())
        });
        // flock belongs to the open file, so two in one process exclude each
        // other exactly as two processes would.
        let mut rows = vec![asking("a", Some("Bash: make"))];
        second.tick_with(&mut rows, &mut |_| {
            presses += 10;
            Ok(())
        });
        assert_eq!(presses, 1, "the second cctop pressed too");
        assert!(rows[0].yolo.is_some(), "the second cctop still shows it");

        // The owner goes; the next one to look takes over.
        drop(first);
        second.tick_with(&mut rows, &mut |_| {
            presses += 10;
            Ok(())
        });
        assert_eq!(presses, 11);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A serve that may not act shows the switch and never presses.
    #[test]
    fn a_read_only_process_never_presses() {
        let dir = scratch("readonly");
        set_in(&dir, "a", true).unwrap();
        let mut auto = Auto::in_dir(dir.clone(), false);
        let mut rows = vec![asking("a", Some("Bash: ls"))];
        auto.tick_with(&mut rows, &mut |_| panic!("a read-only serve pressed"));
        assert!(rows[0].yolo.is_some());
        assert!(auto.owner.is_none(), "it competed for the job");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Each answer is written down for the page, and the row stops asking.
    #[test]
    fn an_answer_is_recorded_and_the_prompt_comes_down() {
        let dir = scratch("record");
        set_in(&dir, "a", true).unwrap();
        let mut auto = Auto::in_dir(dir.clone(), true);
        let mut rows = vec![asking("a", Some("Bash: cargo test"))];
        assert!(auto.tick_with(&mut rows, &mut |_| Ok(())));
        assert_eq!(rows[0].activity_state, ActivityState::Working);
        let entry = rows[0].yolo.clone().expect("still on");
        assert_eq!(entry.allowed.len(), 1);
        assert_eq!(entry.allowed[0].ask, "Bash: cargo test");
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
        let pid = 999_999_999u32;
        let path = crate::shim::socket_path(pid).unwrap();
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

        set("e2e", true).unwrap();
        let mut auto = Auto::new(true);
        let mut row = asking("e2e", Some("Bash: ls"));
        for _ in 0..4 {
            // The row is rebuilt as asking on every tick, the way a report
            // that has not been superseded rebuilds it.
            let mut rows = vec![row.clone()];
            auto.tick(&mut rows);
        }
        assert_eq!(drain(&listener), b"1", "one prompt, one press");

        quiet(&mut row);
        auto.tick(&mut [row.clone()]);
        row.activity_state = ActivityState::Asking;
        row.asking_for = Some("Bash: ls -la".into());
        for _ in 0..3 {
            auto.tick(&mut [row.clone()]);
        }
        assert_eq!(drain(&listener), b"1", "the next prompt, one more press");
        let allowed = &auto.entries["e2e"].allowed;
        assert_eq!(allowed.len(), 2);
        let _ = std::fs::remove_file(&path);
    }
}
