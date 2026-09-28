//! Which finished turns you have not looked at yet.
//!
//! A session whose turn is over is amber in the table and green in the tab
//! bar, and it stays that way for as long as the agent sits at its prompt —
//! which is most of an agent's life. So the colour says "your move" about the
//! turn that ended a minute ago and the one that ended yesterday alike, and a
//! glance at a dozen tabs cannot tell you which of them has news. This is the
//! difference: a session that was *seen working* and then stopped, while you
//! were not looking at it, is "done, unseen" until you do look.
//!
//! # What counts as looking
//!
//! Two things, both of which put the agent's own words in front of you:
//!
//! - its pane is the focused pane of the tab on screen, or
//! - the dashboard is on screen and its row is the selected one — the bottom
//!   panels are about the selected row, so that is where its last reply is.
//!
//! A split's *other* pane is on screen too, but it is not counted: the tab bar
//! already treats the focused pane as the only one you are looking straight at
//! (see [`Tab::attention`](super::tabs::Tab::attention)), and a mark that
//! cleared because a pane was in the corner of your eye would clear on every
//! turn of a split you were never reading.
//!
//! # Local to this process
//!
//! Tab names and colours are shared across every cctop on the machine, written
//! onto the rmux sessions. Seen-ness is not. It is a fact about a *person
//! looking at a screen*, and two cctops are usually two screens — a laptop and
//! a phone over ssh — where having read a reply on one says nothing about the
//! other. The shared-state mechanism that exists (rmux session options) would
//! also make every focus change a write to the daemon, for a flag that is only
//! ever wanted by the screen that set it.
//!
//! ponytail: a cctop started after a turn ended has never seen that session
//! work, so it does not mark it. "Done" means a transition this process
//! watched happen, not a guess about what you have read elsewhere.

use std::collections::HashSet;

/// What a session is doing, as far as the unseen mark cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Mid-turn.
    Working,
    /// The turn is over and the prompt is yours.
    Stopped,
    /// Anything else — blocked on a question, sitting in an API error. Neither
    /// arms nor fires the mark: a question is already the loudest thing on the
    /// screen, and the turn it interrupted is not over.
    Other,
}

impl Phase {
    /// The phase of a row, read the way the status dot reads it.
    pub fn of(state: crate::session::ActivityState) -> Phase {
        use crate::session::ActivityState;
        match state {
            ActivityState::Working => Phase::Working,
            ActivityState::WaitingForInput => Phase::Stopped,
            ActivityState::Asking | ActivityState::ApiError => Phase::Other,
        }
    }
}

/// The sessions seen working since last looked at, and the ones that have
/// since stopped.
///
/// Keyed by [`Session::key`](crate::session::Session::key): the table's
/// identity for a row, which a tab reaches through its pane's pid.
#[derive(Debug, Default)]
pub struct Seen {
    /// Seen mid-turn, so a stop is a transition worth marking.
    working: HashSet<String>,
    /// Stopped while nobody was looking.
    done: HashSet<String>,
}

impl Seen {
    /// Fold in where every live session is now, and which one is being looked
    /// at. Returns whether the set of unseen sessions changed, which is a frame
    /// owed.
    ///
    /// `live` is every session still running. One that is missing has ended
    /// or was never live, and is forgotten: its dot is hollow grey whatever
    /// this says, and a pid reused by a later agent must not inherit it.
    pub fn observe<'a>(
        &mut self,
        live: impl IntoIterator<Item = (&'a str, Phase)>,
        viewed: Option<&str>,
    ) -> bool {
        let before = self.done.len();
        let mut changed = false;
        let mut present = HashSet::new();
        for (key, phase) in live {
            present.insert(key);
            let looking = viewed == Some(key);
            match phase {
                // Working again is a turn in progress, not one waiting to be
                // read: the mark goes, and comes back when this one ends.
                Phase::Working => {
                    self.working.insert(key.to_string());
                    changed |= self.done.remove(key);
                }
                // Only a stop this process watched happen. Watched while you
                // were looking at it, it is a turn you saw end — nothing to
                // tell you.
                Phase::Stopped => {
                    if self.working.remove(key) && !looking {
                        changed |= self.done.insert(key.to_string());
                    }
                }
                Phase::Other => {}
            }
            if looking {
                changed |= self.done.remove(key);
            }
        }
        self.working.retain(|key| present.contains(key.as_str()));
        self.done.retain(|key| present.contains(key.as_str()));
        changed || self.done.len() != before
    }

    /// Whether this session finished a turn you have not looked at.
    pub fn is_done(&self, key: &str) -> bool {
        self.done.contains(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole life of the mark: armed by working, fired by stopping
    /// unwatched, cleared by looking — and not re-fired by the same stop.
    #[test]
    fn a_turn_that_ends_unwatched_is_marked_until_looked_at() {
        let mut seen = Seen::default();
        assert!(!seen.observe([("a", Phase::Working)], None));
        assert!(!seen.is_done("a"), "working is not done");

        assert!(seen.observe([("a", Phase::Stopped)], None), "no frame owed");
        assert!(seen.is_done("a"), "an unwatched stop was not marked");

        // Still stopped, still unwatched: nothing new.
        assert!(!seen.observe([("a", Phase::Stopped)], None));
        assert!(seen.is_done("a"));

        assert!(seen.observe([("a", Phase::Stopped)], Some("a")));
        assert!(!seen.is_done("a"), "looking did not clear it");

        // Looked away again: the turn was read, and stays read.
        seen.observe([("a", Phase::Stopped)], None);
        assert!(!seen.is_done("a"), "the same stop was marked twice");
    }

    /// A session already sitting at its prompt when cctop starts has no turn
    /// this process watched end, so nothing is claimed about it.
    #[test]
    fn a_stop_nobody_saw_begin_is_not_marked() {
        let mut seen = Seen::default();
        seen.observe([("a", Phase::Stopped)], None);
        assert!(!seen.is_done("a"));
    }

    /// Watching a turn end is having seen it.
    #[test]
    fn a_turn_that_ends_while_watched_is_not_marked() {
        let mut seen = Seen::default();
        seen.observe([("a", Phase::Working)], Some("a"));
        seen.observe([("a", Phase::Stopped)], Some("a"));
        seen.observe([("a", Phase::Stopped)], None);
        assert!(!seen.is_done("a"));
    }

    /// Working again takes the mark down; the next stop puts it back.
    #[test]
    fn a_new_turn_clears_the_mark_until_it_ends() {
        let mut seen = Seen::default();
        seen.observe([("a", Phase::Working)], None);
        seen.observe([("a", Phase::Stopped)], None);
        assert!(seen.observe([("a", Phase::Working)], None));
        assert!(!seen.is_done("a"));
        seen.observe([("a", Phase::Stopped)], None);
        assert!(seen.is_done("a"));
    }

    /// A question mid-turn neither ends the turn nor forgets that it began:
    /// answered, the agent works on and the stop after it is still news.
    #[test]
    fn a_question_mid_turn_leaves_the_turn_armed() {
        let mut seen = Seen::default();
        seen.observe([("a", Phase::Working)], None);
        seen.observe([("a", Phase::Other)], None);
        assert!(!seen.is_done("a"), "a question is not a finished turn");
        seen.observe([("a", Phase::Stopped)], None);
        assert!(seen.is_done("a"));
    }

    /// A session that ends is forgotten, mark and all.
    #[test]
    fn an_ended_session_is_forgotten() {
        let mut seen = Seen::default();
        seen.observe([("a", Phase::Working), ("b", Phase::Working)], None);
        seen.observe([("a", Phase::Stopped), ("b", Phase::Working)], None);
        assert!(seen.is_done("a"));
        assert!(seen.observe([("b", Phase::Working)], None), "no frame owed");
        assert!(!seen.is_done("a"));
        // And coming back under the same key starts from nothing.
        seen.observe([("a", Phase::Stopped)], None);
        assert!(!seen.is_done("a"));
    }

    #[test]
    fn a_row_reads_its_phase_off_its_dot() {
        use crate::session::ActivityState;
        assert_eq!(Phase::of(ActivityState::Working), Phase::Working);
        assert_eq!(Phase::of(ActivityState::WaitingForInput), Phase::Stopped);
        assert_eq!(Phase::of(ActivityState::Asking), Phase::Other);
        assert_eq!(Phase::of(ActivityState::ApiError), Phase::Other);
    }
}
