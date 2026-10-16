//! How a remote tab's connection is doing, from the bridge that carries it to
//! the dashboard that shows it.
//!
//! The bridge (`cctop mux reach` / `mux launch`, see [`crate::remote_open`])
//! runs inside the tab's rmux session, a process of its own, and the TUI that
//! draws the tab is another. They share no memory and no socket, so the bridge
//! writes a few numbers to a file named after its rmux session, about once a
//! second and whenever it connects or loses the link, and the TUI reads the
//! focused tab's on its own tick. A file rather than an rmux option: the
//! numbers change every second, and an option is a round trip through the
//! daemon every reader and writer would pay for, where a file nobody reads
//! costs one rename.
//!
//! The file is the bridge's word only while the bridge is alive to keep it
//! true. It is removed on exit, and one left behind by a bridge that was
//! killed is aged out by [`STALE_AFTER`] rather than believed.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The variable a tab's rmux session carries its own name in, so a program
/// inside it can say which tab it is. Set on every session cctop makes — see
/// [`mux::new_session`](crate::mux::new_session) — because the bridge cannot
/// ask rmux which session it is in without a client of its own.
pub const TAB_ENV: &str = "CCTOP_TAB";

/// How often the bridge rewrites its file while nothing changes.
pub const EVERY: Duration = Duration::from_secs(1);

/// A file older than this is a bridge that stopped writing — hung, or killed
/// before it could remove it — and says nothing about the link now. Five
/// beats of [`EVERY`], so a bridge blocked on a slow write for a moment is not
/// mistaken for a dead one.
pub const STALE_AFTER: Duration = Duration::from_secs(5);

/// Whether the link is carrying the tab, or being dialled again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Connected,
    Reconnecting,
    /// Dialling again was given up on: the machine is not answering, and the
    /// tab is waiting to be told to try again or to close. The tab bar marks
    /// it, because a dead machine's tab otherwise looks like any other until
    /// it is opened.
    Lost,
    /// A state a newer bridge wrote that this build has no word for.
    ///
    /// The file is shared between builds: a bridge started by one cctop is
    /// read by whichever dashboard is open, which `cctop --update` may have
    /// moved on — or not yet. Without this, one new state made the whole file
    /// unreadable, and the corner lost its round trip and byte counts with
    /// it. With it, the rest still reads, and the state itself is drawn as
    /// not known: no mark on the tab, `?` in the corner.
    #[serde(other)]
    Unknown,
}

/// What the bridge says about its link.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub state: State,
    /// The round trip as the local echo estimates it — see
    /// [`predict`](crate::predict) — or `None` before the first sample.
    pub rtt_ms: Option<u32>,
    /// How many times this tab's link has been dialled again since it opened.
    pub reconnects: u32,
    /// Terminal output received, as it arrived (`wire`) and as it decompressed
    /// to (`raw`). Only the far side compresses — see [`crate::ws`] — so the
    /// ratio is the inbound one.
    pub raw_in: u64,
    pub wire_in: u64,
    /// Keystrokes sent. Never compressed, so raw and wire agree;
    /// kept for whoever reads the file by hand.
    pub raw_out: u64,
    pub wire_out: u64,
    /// When the bridge started, in unix seconds.
    pub since: u64,
    /// When this was written, in unix milliseconds: what staleness is measured
    /// from. Written into the file rather than read off its mtime so a test
    /// can age one without touching the filesystem's clock.
    pub at: u64,
}

impl Stats {
    /// Inbound bytes as received over bytes they stood for: `4.1` for output
    /// that compressed to a quarter. `None` until a few kilobytes have arrived,
    /// because a ratio over one prompt redraw is noise.
    pub fn ratio(&self) -> Option<f64> {
        (self.wire_in >= 4096).then(|| self.raw_in as f64 / self.wire_in as f64)
    }

    /// Whether this was written recently enough to still be true at `now`
    /// (unix milliseconds).
    pub fn is_fresh(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.at) <= STALE_AFTER.as_millis() as u64
    }
}

/// Unix milliseconds now.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Where the stats for the tab in rmux session `session` live.
///
/// `None` for a name that is not one rmux would give a cctop session —
/// letters, digits, `-` and `_` — so nothing read from the environment can
/// point the write outside the directory.
pub fn path(session: &str) -> Option<PathBuf> {
    path_in(&crate::config::runtime_base(), session)
}

fn path_in(base: &Path, session: &str) -> Option<PathBuf> {
    let ok = !session.is_empty()
        && session.len() <= 200
        && session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    ok.then(|| {
        base.join("cctop")
            .join("bridge")
            .join(format!("{session}.json"))
    })
}

/// Write `stats` for `session`, atomically and readable by this user only.
/// Best effort: a dashboard short of a number is no reason to fail a tab.
pub fn write(session: &str, stats: &Stats) {
    if let Some(path) = path(session) {
        write_to(&path, stats);
    }
}

fn write_to(path: &Path, stats: &Stats) {
    if let Ok(json) = serde_json::to_vec(stats) {
        let _ = crate::util::write_atomic(path, &json, true);
    }
}

/// Remove `session`'s file, for a bridge on its way out.
pub fn remove(session: &str) {
    if let Some(path) = path(session) {
        let _ = std::fs::remove_file(path);
    }
}

/// `session`'s stats, if a live bridge wrote them: a file that is missing,
/// unreadable or older than [`STALE_AFTER`] is `None` alike.
pub fn read(session: &str) -> Option<Stats> {
    let stats: Stats = serde_json::from_slice(&std::fs::read(path(session)?).ok()?).ok()?;
    stats.is_fresh(now_ms()).then_some(stats)
}

/// The bridge's side: counts as it goes, and writes them out on a beat of its
/// own.
///
/// The beat is a thread rather than the bridge's loop, because the loop is not
/// always turning: dialling the sibling again can wait out a handshake for
/// seconds, and a file left unwritten that long would read as a dead bridge
/// just when "reconnecting" is the thing worth showing.
///
/// Held for the bridge's whole life, across reconnects. Dropped, it stops the
/// beat and removes the file, so a bridge that ends by returning or by
/// unwinding leaves nothing behind to be read as a link.
pub struct Writer {
    shared: Option<std::sync::Arc<Shared>>,
    beat: Option<std::thread::JoinHandle<()>>,
}

struct Shared {
    /// Resolved once, by the thread that made the writer: the beat's thread
    /// is not the one whose runtime directory was asked for.
    path: PathBuf,
    stats: std::sync::Mutex<Stats>,
    /// Set on drop; the beat checks it each time it wakes.
    done: std::sync::atomic::AtomicBool,
}

impl Shared {
    fn flush(&self) {
        let mut stats = self.stats.lock().unwrap_or_else(|e| e.into_inner());
        stats.at = now_ms();
        write_to(&self.path, &stats);
    }

    fn update(&self, change: impl FnOnce(&mut Stats)) {
        change(&mut self.stats.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

impl Writer {
    /// A writer for the tab this process is in, as [`TAB_ENV`] names it; one
    /// that writes nothing when the variable is absent, as it is for a bridge
    /// run by hand or in a session an older cctop made.
    pub fn from_env() -> Writer {
        Writer::for_session(std::env::var(TAB_ENV).ok().filter(|s| path(s).is_some()))
    }

    pub fn for_session(session: Option<String>) -> Writer {
        let Some(path) = session.as_deref().and_then(path) else {
            return Writer {
                shared: None,
                beat: None,
            };
        };
        let now = now_ms();
        let shared = std::sync::Arc::new(Shared {
            path,
            stats: std::sync::Mutex::new(Stats {
                state: State::Connected,
                rtt_ms: None,
                reconnects: 0,
                raw_in: 0,
                wire_in: 0,
                raw_out: 0,
                wire_out: 0,
                since: now / 1000,
                at: now,
            }),
            done: std::sync::atomic::AtomicBool::new(false),
        });
        shared.flush();
        let beat = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("link-health".into())
                .spawn(move || {
                    use std::sync::atomic::Ordering;
                    while !shared.done.load(Ordering::Relaxed) {
                        std::thread::park_timeout(EVERY);
                        if shared.done.load(Ordering::Relaxed) {
                            break;
                        }
                        shared.flush();
                    }
                })
                .ok()
        };
        Writer {
            shared: Some(shared),
            beat,
        }
    }

    fn update(&self, change: impl FnOnce(&mut Stats)) {
        if let Some(shared) = &self.shared {
            shared.update(change);
        }
    }

    /// The link is up, down and being dialled again, or given up on. Written
    /// at once: a state change is the one thing worth a write off the beat.
    pub fn set_state(&self, state: State) {
        let mut changed = false;
        self.update(|stats| {
            if stats.state != state {
                if state == State::Reconnecting {
                    stats.reconnects += 1;
                }
                stats.state = state;
                changed = true;
            }
        });
        if changed && let Some(shared) = &self.shared {
            shared.flush();
        }
    }

    /// The round trip as of now, as the local echo estimates it.
    pub fn rtt(&self, rtt: Option<Duration>) {
        let ms = rtt.map(|d| d.as_millis().min(u32::MAX as u128) as u32);
        self.update(|stats| stats.rtt_ms = ms);
    }

    /// `raw` bytes of terminal output, after decompression.
    pub fn received_raw(&self, raw: usize) {
        self.update(|stats| stats.raw_in += raw as u64);
    }

    /// `wire` bytes off the socket, framing and compression as they came.
    pub fn received_wire(&self, wire: usize) {
        self.update(|stats| stats.wire_in += wire as u64);
    }

    /// `bytes` of keystrokes sent, which go uncompressed.
    pub fn sent(&self, bytes: usize) {
        self.update(|stats| {
            stats.raw_out += bytes as u64;
            stats.wire_out += bytes as u64;
        });
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let Some(shared) = &self.shared else {
            return;
        };
        shared
            .done
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(beat) = self.beat.take() {
            beat.thread().unpark();
            let _ = beat.join();
        }
        let _ = std::fs::remove_file(&shared.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> Stats {
        Stats {
            state: State::Connected,
            rtt_ms: Some(118),
            reconnects: 2,
            raw_in: 41_000,
            wire_in: 10_000,
            raw_out: 300,
            wire_out: 300,
            since: 1_700_000_000,
            at: now_ms(),
        }
    }

    #[test]
    fn stats_round_trip_through_their_file_owner_only() {
        let _base = crate::config::claim_test_runtime_base(module_path!());
        let session = "cctop-claude-on-desk-abc";
        write(session, &stats());
        let file = path(session).unwrap();
        let mode = std::os::unix::fs::PermissionsExt::mode(&file.metadata().unwrap().permissions());
        assert_eq!(mode & 0o777, 0o600, "mode was {mode:o}");
        let back = read(session).expect("a fresh file reads back");
        assert_eq!(back, stats_at(back.at));
        assert_eq!(back.ratio().map(|r| (r * 10.0).round()), Some(41.0));
        remove(session);
        assert_eq!(read(session), None, "a removed file is no link");
    }

    fn stats_at(at: u64) -> Stats {
        Stats { at, ..stats() }
    }

    #[test]
    fn a_file_nobody_has_rewritten_for_five_seconds_is_not_believed() {
        let _base = crate::config::claim_test_runtime_base(module_path!());
        let now = now_ms();
        assert!(stats_at(now - 4_000).is_fresh(now));
        assert!(!stats_at(now - 6_000).is_fresh(now));
        write("cctop-old", &stats_at(now - 6_000));
        assert_eq!(read("cctop-old"), None);
        assert_eq!(read("cctop-never-written"), None);
    }

    #[test]
    fn a_name_that_is_not_a_session_names_no_file() {
        let base = Path::new("/run/x");
        assert_eq!(
            path_in(base, "cctop-a_1"),
            Some(PathBuf::from("/run/x/cctop/bridge/cctop-a_1.json"))
        );
        for bad in ["", "../etc/passwd", "a/b", "a.b", "a b"] {
            assert_eq!(path_in(base, bad), None, "{bad:?}");
        }
    }

    /// A state a newer bridge added still reads, as not known, with the rest
    /// of the file intact.
    #[test]
    fn a_state_from_a_newer_bridge_still_reads() {
        let mut v = serde_json::to_value(stats()).unwrap();
        v["state"] = "something-new".into();
        let read: Stats = serde_json::from_value(v).expect("still reads");
        assert_eq!(read.state, State::Unknown);
        assert_eq!((read.rtt_ms, read.reconnects), (Some(118), 2));
    }

    #[test]
    fn the_ratio_waits_for_enough_bytes_to_mean_something() {
        let mut s = stats();
        s.wire_in = 100;
        s.raw_in = 1000;
        assert_eq!(s.ratio(), None);
    }

    #[test]
    fn the_writer_counts_reconnects_and_removes_its_file() {
        let _base = crate::config::claim_test_runtime_base(module_path!());
        let session = "cctop-writer-test";
        let w = Writer::for_session(Some(session.into()));
        let seen = read(session).expect("written as soon as it is made");
        assert_eq!((seen.state, seen.reconnects), (State::Connected, 0));
        w.set_state(State::Reconnecting);
        let seen = read(session).unwrap();
        assert_eq!((seen.state, seen.reconnects), (State::Reconnecting, 1));
        w.set_state(State::Connected);
        w.received_raw(8000);
        w.received_wire(2000);
        w.sent(3);
        w.rtt(Some(Duration::from_millis(42)));
        // The beat writes it within a second, with no call from the bridge.
        let seen = crate::test_wait::wait_until(Duration::from_secs(5), || {
            read(session).filter(|s| s.rtt_ms.is_some())
        })
        .expect("the beat wrote the new numbers");
        assert_eq!(seen.ratio(), None, "too few bytes for a ratio yet");
        assert_eq!((seen.raw_in, seen.wire_in, seen.wire_out), (8000, 2000, 3));
        assert_eq!(
            (seen.state, seen.reconnects, seen.rtt_ms),
            (State::Connected, 1, Some(42))
        );
        drop(w);
        assert!(!path(session).unwrap().exists(), "dropped, the file goes");
    }
}
