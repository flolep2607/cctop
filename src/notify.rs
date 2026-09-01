//! Telling you when a session needs you.
//!
//! cctop is a monitor you look away from, so the one thing it owes you is a
//! nudge when an agent stops working and starts waiting. Both channels are the
//! terminal's own: `BEL`, which rmux turns into a `monitor-bell` window flag,
//! and OSC 9, which iTerm2, Ghostty, kitty, WezTerm and Windows Terminal raise
//! as a real desktop notification. Neither needs a daemon, a D-Bus connection,
//! or a crate.
//!
//! The ring is an edge, never a level. A session waiting for input is still
//! waiting for input on the next refresh, and on the one after that — ringing
//! for the state rather than the crossing would be an alarm clock, not a
//! notification. So [`Notifier`] keeps last refresh's state for each *running*
//! session and fires only where the two disagree.

use crate::session::{ActivityState, Session};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long the session that rang keeps its marker in the table.
///
/// Long enough to still be there when you look back at the window, short enough
/// that it doesn't linger into the next thing you do.
pub const MARK_FOR: Duration = Duration::from_secs(30);

/// What a session was doing at a refresh, as far as ringing is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Busy,
    /// The turn is over and the prompt is the user's.
    Waiting,
    /// The agent is blocked on a question and cannot go on until it is
    /// answered. Rung for separately, because "your move" and "I am stuck" are
    /// not the same news.
    Asking,
    Stopped,
}

/// Why a session rang. The two are worth different sentences: one of them is an
/// agent holding a question for you, the other is an agent that is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    NeedsInput,
    /// Blocked on a permission prompt or an elicitation.
    Asking,
    Stopped,
}

/// The session that rang, kept around so the bell can be traced to a row.
#[derive(Debug, Clone)]
pub struct Rang {
    pub key: String,
    pub label: String,
    pub reason: Reason,
    pub at: Instant,
}

#[derive(Default)]
pub struct Notifier {
    /// Opt-in, and persisted: an unasked-for bell in a shared office is worse
    /// than a missed one.
    pub enabled: bool,
    /// Last state of every session that was running at the previous refresh,
    /// with the label to name it by. The label is carried because a session
    /// that exits between refreshes still has to be nameable, and by then the
    /// only thing left of it is this entry.
    watched: HashMap<String, (State, String)>,
    /// The most recent ring.
    pub last: Option<Rang>,
}

impl Notifier {
    pub fn new(enabled: bool) -> Self {
        Notifier {
            enabled,
            ..Default::default()
        }
    }

    /// Fold a refresh into the state machine, ringing for whatever crossed.
    ///
    /// Runs whether or not notifications are enabled: the machine has to stay
    /// warm. Seeding it only on `n` would mean the first refresh after the
    /// toggle sees every idle session as a fresh transition and rings for all
    /// of them at once.
    pub fn observe(&mut self, sessions: &[Session]) {
        let mut crossed: Vec<Rang> = Vec::new();
        // Only running sessions are tracked, which is what keeps this cheap:
        // the map holds a handful of entries, not one per transcript ever
        // written, and only those few need a formatted key per refresh.
        let mut next = HashMap::with_capacity(self.watched.len());
        for session in sessions.iter().filter(|s| s.is_running()) {
            let key = session.key();
            let state = state_of(session);
            let label = session.display_label().to_string();
            // Removing as we go leaves `watched` holding exactly the sessions
            // that were running last time and are not running now.
            // Only out of `Busy`, so answering a permission prompt and having
            // the turn end a moment later is one ring rather than two: the
            // second crossing is `Asking` to `Waiting`, and it is the same turn
            // you have already been told about.
            if let Some((State::Busy, _)) = self.watched.remove(&key)
                && let Some(reason) = reason_for(state)
            {
                crossed.push(Rang {
                    key: key.clone(),
                    label: label.clone(),
                    reason,
                    at: Instant::now(),
                });
            }
            // Back to work is the definition of answered, however it happened —
            // through cctop, or in the terminal the agent actually lives in.
            if state == State::Busy && self.last.as_ref().is_some_and(|r| r.key == key) {
                self.last = None;
            }
            next.insert(key, (state, label));
        }

        for (key, (state, label)) in self.watched.drain() {
            // A session that was already waiting when it exited has rung once
            // already; ringing again for the exit would double-report the same
            // turn ending.
            if state == State::Busy {
                crossed.push(Rang {
                    key,
                    label,
                    reason: Reason::Stopped,
                    at: Instant::now(),
                });
            }
        }
        self.watched = next;

        if !self.enabled || crossed.is_empty() {
            return;
        }
        // One bell for the refresh, no matter how many sessions finished in it:
        // three bells in a row is noise, and the count says the same thing.
        let extra = crossed.len() - 1;
        let rang = crossed.swap_remove(0);
        ring(&desktop_text(&rang, extra));
        self.last = Some(rang);
    }

    /// True while this session's row should still carry the bell marker.
    pub fn rang_recently(&self, key: &str) -> bool {
        self.last
            .as_ref()
            .is_some_and(|r| r.key == key && r.at.elapsed() < MARK_FOR)
    }

    /// The footer's reminder of who rang, or `None` when there is nothing to
    /// answer.
    ///
    /// It disappears once the selection is on that session, which is the
    /// cheapest possible definition of "answered": you are looking at it, so
    /// the footer has done its job and does not need any state of its own to
    /// know that.
    pub fn footer(&self, selected: Option<&str>) -> Option<String> {
        let rang = self.last.as_ref()?;
        if selected == Some(rang.key.as_str()) {
            return None;
        }
        let secs = rang.at.elapsed().as_secs();
        let ago = if secs < 60 {
            format!("{secs}s")
        } else {
            format!("{}m", secs / 60)
        };
        Some(format!(
            "Bell: ◉ {} · {} · {ago} ago · b jumps to it",
            rang.label,
            match rang.reason {
                Reason::NeedsInput => "waiting for input",
                Reason::Asking => "needs permission",
                Reason::Stopped => "stopped",
            }
        ))
    }
}

/// Which crossings are worth a bell, and what to call them.
fn reason_for(state: State) -> Option<Reason> {
    match state {
        State::Asking => Some(Reason::Asking),
        State::Waiting => Some(Reason::NeedsInput),
        State::Busy | State::Stopped => None,
    }
}

/// The session's state as the bell sees it.
///
/// Both waiting states arrive from the row, which is where a hook report has
/// already been stamped — see
/// [`App::apply_reports`](crate::ui::App::apply_reports). That matters most for
/// the permission prompt: it is the moment an agent is most obviously blocked
/// and the one a transcript cannot see at all, so before the hooks it read as
/// ordinary work and never rang.
///
/// ponytail: with no hooks installed, an agent that has simply finished its
/// turn still reads as `Working` here for the harnesses whose transcripts do
/// not mark the end of one, so the ring is missed. Ceiling accepted rather than
/// guessed at: a quiet-timer on `last_active` would fire in the middle of every
/// long reasoning turn. `cctop hook --install` is the fix, and the doctor says
/// so.
fn state_of(session: &Session) -> State {
    if !session.is_running() {
        return State::Stopped;
    }
    match session.activity_state {
        ActivityState::Asking => State::Asking,
        ActivityState::WaitingForInput => State::Waiting,
        // An API error is the agent's problem, not yet the user's: it retries,
        // and the red dot in the table is already saying so.
        ActivityState::Working | ActivityState::ApiError => State::Busy,
    }
}

fn desktop_text(rang: &Rang, extra: usize) -> String {
    let what = match rang.reason {
        Reason::NeedsInput => "is waiting for input",
        Reason::Asking => "needs permission",
        Reason::Stopped => "stopped",
    };
    let more = if extra > 0 {
        format!(" (+{extra} more)")
    } else {
        String::new()
    };
    format!("cctop: {} {what}{more}", rang.label)
}

/// Ring the terminal and raise a desktop notification.
///
/// Safe from the UI thread between frames, and from nowhere else. ratatui
/// buffers a whole frame and flushes it at the end of `Terminal::draw`, so
/// stdout is quiescent in between; and neither BEL nor an OSC string paints a
/// cell or moves the cursor, so what is on screen — alternate screen included —
/// is untouched. Written from the worker thread instead, it could land in the
/// middle of a flush and cut somebody's escape sequence in half.
fn ring(text: &str) {
    use std::io::Write;
    // The state-machine tests drive real crossings, and stdout under `cargo
    // test` is the developer's terminal: without this the suite beeps at them
    // and leaves escape sequences among the results.
    if cfg!(test) {
        return;
    }
    let mut out = std::io::stdout();
    let _ = write!(out, "\x07\x1b]9;{}\x07", sanitize(text));
    let _ = out.flush();
}

/// Strip what would end the OSC string early.
///
/// The text carries a session label, which is a directory name the user chose.
/// A BEL in it would terminate the notification and hand the remainder to the
/// terminal as commands; an ESC would do worse.
fn sanitize(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    fn session(id: &str, running: bool, state: ActivityState) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.label_source = format!("/home/x/{id}");
        s.abbrev_label = id.into();
        s.activity_state = state;
        if running {
            s.process = Some(crate::proc::ProcInfo::default());
        }
        s
    }

    /// The complaint this split exists for: before it, an agent holding a
    /// permission prompt and an agent whose turn was simply over were the same
    /// fact — the same colour on the row, and the same sentence in the bell.
    #[test]
    fn a_held_permission_prompt_is_not_a_finished_turn() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        n.observe(&[session("a", true, ActivityState::Working)]);
        n.observe(&[session("a", true, ActivityState::Asking)]);
        let rang = n.last.as_ref().expect("a blocked agent rings");
        assert_eq!(rang.reason, Reason::Asking);
        assert!(
            desktop_text(rang, 0).contains("needs permission"),
            "the bell says which kind of waiting it is: {}",
            desktop_text(rang, 0)
        );

        // And the other kind still reads as the other kind.
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        n.observe(&[session("a", true, ActivityState::Working)]);
        n.observe(&[session("a", true, ActivityState::WaitingForInput)]);
        assert_eq!(n.last.as_ref().map(|r| r.reason), Some(Reason::NeedsInput));
    }

    /// Answering the prompt and having the turn end a moment later is one
    /// event, not two: only a crossing out of `Busy` rings, so the second
    /// transition is silent.
    #[test]
    fn answering_a_prompt_does_not_ring_again_when_the_turn_ends() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        n.observe(&[session("a", true, ActivityState::Working)]);
        n.observe(&[session("a", true, ActivityState::Asking)]);
        n.last = None;
        n.observe(&[session("a", true, ActivityState::WaitingForInput)]);
        assert!(n.last.is_none(), "the same turn, reported twice");
    }

    /// The whole point of the feature: one ring on the crossing, silence on
    /// every refresh after it, however long the session sits there waiting.
    #[test]
    fn a_session_that_stops_working_rings_once_and_not_again() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        let busy = vec![session("a", true, ActivityState::Working)];
        n.observe(&busy);
        assert!(n.last.is_none(), "still working, nothing to say");

        let waiting = vec![session("a", true, ActivityState::WaitingForInput)];
        n.observe(&waiting);
        let first = n.last.clone().expect("the crossing rings");
        assert_eq!(first.reason, Reason::NeedsInput);

        n.observe(&waiting);
        assert_eq!(
            n.last.as_ref().map(|r| r.at),
            Some(first.at),
            "a session that is still waiting must not ring again"
        );
    }

    #[test]
    fn a_busy_session_that_disappears_rings_as_stopped() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        n.observe(&[session("a", true, ActivityState::Working)]);
        // Same session, its process gone.
        n.observe(&[session("a", false, ActivityState::Working)]);
        let rang = n.last.as_ref().expect("an agent that exits is news");
        assert_eq!(rang.reason, Reason::Stopped);
        assert_eq!(rang.label, "a");

        // Gone from the table entirely: nothing left to cross.
        n.last = None;
        n.observe(&[]);
        assert!(n.last.is_none());
    }

    /// A session already idle when cctop starts — or when `n` is pressed — has
    /// not transitioned in front of us, so it must stay silent.
    #[test]
    fn a_session_first_seen_idle_never_rings_for_being_idle() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        let waiting = vec![session("a", true, ActivityState::WaitingForInput)];
        n.observe(&waiting);
        n.observe(&waiting);
        assert!(n.last.is_none());
    }

    #[test]
    fn the_state_machine_stays_warm_while_notifications_are_off() {
        let mut n = Notifier::default();
        n.observe(&[session("a", true, ActivityState::Working)]);
        n.enabled = true;
        n.observe(&[session("a", true, ActivityState::WaitingForInput)]);
        assert!(
            n.last.is_some(),
            "the busy state seen before the toggle still counts"
        );
    }

    #[test]
    fn going_back_to_work_answers_the_bell() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        n.observe(&[session("a", true, ActivityState::Working)]);
        n.observe(&[session("a", true, ActivityState::WaitingForInput)]);
        assert!(n.last.is_some());
        n.observe(&[session("a", true, ActivityState::Working)]);
        assert!(n.last.is_none(), "answered, so stop naming it");
    }

    #[test]
    fn the_footer_names_the_session_until_it_is_selected() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        n.observe(&[session("a", true, ActivityState::Working)]);
        n.observe(&[session("a", true, ActivityState::WaitingForInput)]);
        let key = n.last.as_ref().unwrap().key.clone();
        assert!(n.footer(None).is_some_and(|t| t.contains('a')));
        assert!(n.footer(Some("claude:other")).is_some());
        assert!(n.footer(Some(&key)).is_none());
        assert!(n.rang_recently(&key));
        assert!(!n.rang_recently("claude:other"));
    }

    /// The label is a path the user chose, and it is interpolated into an
    /// escape sequence. Anything that could close that sequence early has to be
    /// gone before it reaches the terminal.
    #[test]
    fn a_label_cannot_break_out_of_the_notification() {
        let text = sanitize("proj\x07\x1b]0;pwned\x07");
        assert!(!text.contains('\x07'));
        assert!(!text.contains('\x1b'));
        assert_eq!(text, "proj]0;pwned");
    }

    #[test]
    fn one_bell_for_a_refresh_that_finishes_several_sessions() {
        let rang = Rang {
            key: "claude:a".into(),
            label: "alpha".into(),
            reason: Reason::NeedsInput,
            at: Instant::now(),
        };
        assert_eq!(desktop_text(&rang, 0), "cctop: alpha is waiting for input");
        assert_eq!(
            desktop_text(&rang, 2),
            "cctop: alpha is waiting for input (+2 more)"
        );
    }
}
