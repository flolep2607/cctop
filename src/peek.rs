//! Reading an agent's screen without holding a pane on it.
//!
//! The dashboard reads each tab's screen off the vt100 parser it already owns
//! ([`Pane::read_screen`](crate::ui::tabs::Pane::read_screen)). A detached rmux
//! tab, and every session a standalone `cctop serve` watches, has no parser in
//! this process — the screen has to be borrowed from whoever holds it. Two
//! holders answer: the `cctop run` shim, which replays what the agent last drew
//! to whoever attaches, and rmux, which keeps the pane's screen for
//! `capture-pane`.
//!
//! What is read feeds the same [`screen_state`](crate::ui::tabs::screen_state)
//! recognizer the panes feed, so a permission prompt no hook reported still
//! puts Allow and Deny on the page.
//!
//! ponytail: a peek is one snapshot, so a screen none of the phrases match says
//! nothing. The still-screen "must be idle" inference belongs to
//! [`Pane::read_screen`](crate::ui::tabs::Pane::read_screen), which watches the
//! screen go still; a snapshot has no history to hold still against, and a
//! transcript already reads a finished turn well enough on its own.

use std::process::Command;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

/// What one read of an agent's screen found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Screened {
    /// What it is doing, plainly — asking, or working.
    pub signal: crate::hook::Signal,
    /// What a prompt on screen is asking for, when it can be seen: the `$ …`
    /// command under Codex's question, the tool box over Claude's. `None`
    /// when the prompt carries nothing readable — never something made up.
    pub ask: Option<String>,
    /// Whether the prompt on screen is a question with choices rather than a
    /// permission prompt; see [`screen_question`](crate::ui::tabs::screen_question).
    pub question: bool,
}

/// A tick's worth of screen borrows, sharing the lookups that cost.
///
/// The pane list and the process table are a sweep each, and a reader asking
/// about every running session pays them once rather than once per session.
/// They age, though: an agent started after the sweep is a pid the map cannot
/// name, so the sweeps are redone once they are [`SWEEP_STALE`] rather than on
/// every read.
pub(crate) struct Peek {
    /// Every rmux pane as `(pane_pid, pane_id)`, or `None` where there is no
    /// server to ask — the common case worth an early answer of its own.
    panes: Option<Vec<(u32, String)>>,
    sys: System,
    swept_at: std::time::Instant,
}

/// How old the pane list and process table may be before a read re-takes
/// them — the difference between noticing a new agent late and paying a sweep
/// for every session, every tick.
const SWEEP_STALE: std::time::Duration = std::time::Duration::from_secs(5);

impl Peek {
    pub(crate) fn new() -> Peek {
        // Swept lazily on the first read rather than eagerly here, so asking
        // for a Peek where nothing is running costs nothing.
        Peek {
            panes: None,
            sys: System::new(),
            swept_at: std::time::Instant::now() - SWEEP_STALE,
        }
    }

    /// What the screen holding `pid`'s agent says it is doing.
    ///
    /// `None` for a harness with no footer row to match, for an agent nobody
    /// can lend a screen of — a plain terminal is neither shim nor pane — and
    /// for a screen no phrase matched. The caller falls back to hooks, then to
    /// the transcript, in that order.
    pub(crate) fn read(&mut self, harness: &str, pid: u32) -> Option<Screened> {
        // Checked before the borrow: a harness with no footer row to match has
        // no screen worth capturing.
        if !crate::ui::tabs::screenable(harness) {
            return None;
        }
        if self.swept_at.elapsed() >= SWEEP_STALE {
            self.sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                false,
                ProcessRefreshKind::nothing(),
            );
            self.panes = crate::inject::list_panes();
            self.swept_at = std::time::Instant::now();
        }
        let rows = self.screen(pid)?;
        finish(harness, &rows)
    }

    /// The screen holding `pid`, as text rows.
    ///
    /// Shim first: the socket answers for every agent `cctop run` started,
    /// wherever it is running, and a refused connect is the cheapest of the
    /// ways to learn a pid is nobody cctop can read.
    fn screen(&mut self, pid: u32) -> Option<Vec<String>> {
        if let Some(rows) = shim_screen(pid) {
            return Some(rows);
        }
        let pane = crate::inject::pane_in(&self.sys, self.panes.as_deref()?, pid)?;
        capture(&pane)
    }
}

/// The same read for a rmux session named rather than found by pid — a
/// detached tab knows its session's name, and `rmux::capture` resolves it to
/// the exact pane rather than prefix-matching a neighbour. Free of [`Peek`]'s
/// sweeps because it needs neither.
pub(crate) fn named(name: &str) -> Option<Screened> {
    let capture = crate::rmux::capture(name)?;
    let rows = replay(&capture);
    finish(crate::ui::tabs::harness_of(name), &rows)
}

/// The harness name, a recognized screen, and nothing else in the answer.
fn finish(harness: &str, rows: &[String]) -> Option<Screened> {
    if !crate::ui::tabs::screenable(harness) {
        return None;
    }
    let signal = crate::ui::tabs::screen_state(harness, rows)?;
    // A prompt's detail is only worth reading when there is a prompt: the
    // extractor reads the same footer window the recognizer did.
    let asking = signal == crate::hook::Signal::NeedsInput;
    let ask = asking
        .then(|| crate::ui::tabs::screen_ask(harness, rows))
        .flatten();
    let question = asking && crate::ui::tabs::screen_question(harness, rows);
    Some(Screened {
        signal,
        ask,
        question,
    })
}

/// What `cctop run`'s shim replays to a watcher, read once and dropped.
///
/// Attaching asks nothing of the agent — a watcher that never names a size
/// never resizes it — so this is a borrow, not a look that changes what is
/// looked at.
fn shim_screen(pid: u32) -> Option<Vec<String>> {
    let mut attach = crate::attach::attach(pid)?;
    // The shim writes the size and the whole replay under one lock, so it all
    // arrives together — but not necessarily before the connect returns. Give
    // the socket a beat to deliver it, or an agent sitting at a prompt, which
    // never draws again, would read as an empty screen.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
    while !attach.pump() {
        if attach.closed() || std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    attach.pump();
    let screen = attach.parser.screen();
    let (_, cols) = screen.size();
    Some(screen.rows(0, cols).collect())
}

/// The visible screen of a rmux target — a `%id` — as text rows.
///
/// `-p` prints rather than captures to a buffer, and `-J` keeps a wrapped line
/// one line so a footer hint cannot split mid-phrase on a narrow pane.
fn capture(target: &str) -> Option<Vec<String>> {
    let out = Command::new(crate::rmux::BIN)
        .args(["capture-pane", "-p", "-J", "-t", target])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(String::from)
            .collect(),
    )
}

/// A [`crate::rmux::Capture`] replayed into a parser of its own size, then read
/// back as rows — the same shape an attached pane's screen comes in.
fn replay(capture: &crate::rmux::Capture) -> Vec<String> {
    let mut parser = vt100::Parser::new(capture.rows, capture.cols, 0);
    parser.process(&capture.bytes);
    parser.screen().rows(0, capture.cols).collect()
}
