//! Remote tabs' link health, read from the file each bridge keeps (see
//! [`cctop_core::link_health`]): the focused tab's in full for the footer's
//! corner, and every remote tab's state for the tab bar, which marks one whose
//! machine is not answering.
//!
//! Only on the loop's tick, at most once per
//! [`EVERY`](cctop_core::link_health::EVERY): the bridge writes no faster than
//! that. The bar's pass reads one small file per remote pane, which is what a
//! tab whose machine has died would otherwise only say once opened.

use std::collections::HashMap;
use std::time::Instant;

use cctop_core::link_health::{self, Stats};
use cctop_core::reach::Transport;

use crate::App;

/// What was last read for the focused remote pane.
#[derive(Debug, Clone)]
pub struct Seen {
    /// The rmux session it was read for: a focus that moves reads again at
    /// once rather than showing one tab's link under another.
    pub session: String,
    pub transport: Transport,
    /// `None` for a file that is missing or stale — a bridge that has not
    /// written yet, or one that is gone.
    pub stats: Option<Stats>,
    read: Instant,
}

impl Seen {
    /// A reading as of now, for the footer's tests and the snapshots.
    #[cfg(test)]
    pub fn fixed(session: &str, transport: Transport, stats: Option<Stats>) -> Seen {
        Seen {
            session: session.into(),
            transport,
            stats,
            read: Instant::now(),
        }
    }
}

/// Every remote pane whose link is not up, by its rmux session, as the bar
/// last read them.
#[derive(Debug, Default)]
pub struct Down {
    pub states: HashMap<String, link_health::State>,
    read: Option<Instant>,
}

impl App {
    /// The focused pane's rmux session and transport, when the focused tab is
    /// one on another machine.
    fn focused_remote(&self) -> Option<(&str, Transport)> {
        if self.on_settings() {
            return None;
        }
        let tab = self.tabs.get(self.tab.checked_sub(1)?)?;
        let pane = tab.panes.get(tab.focus)?;
        Some((pane.rmux.as_deref()?, pane.transport?))
    }

    /// Read the remote tabs' links again where due. Returns whether what the
    /// corner or the bar shows has changed.
    pub(super) fn tick_link(&mut self) -> bool {
        let bar = self.tick_down();
        self.tick_corner() || bar
    }

    /// Every sibling's pane, read again for the bar. A file that is missing
    /// or stale says nothing, as it does for the corner: a bridge that has
    /// not written yet, or one from a cctop before the bar marked tabs.
    fn tick_down(&mut self) -> bool {
        if self
            .down
            .read
            .is_some_and(|read| read.elapsed() < link_health::EVERY)
        {
            return false;
        }
        let states: HashMap<String, link_health::State> = self
            .tabs
            .iter()
            .flat_map(|tab| &tab.panes)
            .filter(|pane| pane.transport == Some(Transport::Peer))
            .filter_map(|pane| {
                let session = pane.rmux.as_deref()?;
                let state = link_health::read(session)?.state;
                // Only the states the bar has a mark for: one from a newer
                // bridge is not known to be down, and a mark would say it is.
                matches!(
                    state,
                    link_health::State::Reconnecting | link_health::State::Lost
                )
                .then(|| (session.to_string(), state))
            })
            .collect();
        let changed = states != self.down.states;
        self.down = Down {
            states,
            read: Some(Instant::now()),
        };
        changed
    }

    /// Whether tab `t` (counted from the first after the dashboard) has a
    /// link that is not up, and how: lost wins over reconnecting, since it is
    /// the one that will not mend itself.
    pub fn tab_down(&self, t: usize) -> Option<link_health::State> {
        self.tabs
            .get(t)?
            .panes
            .iter()
            .filter_map(|pane| self.down.states.get(pane.rmux.as_deref()?))
            .copied()
            .max_by_key(|state| *state == link_health::State::Lost)
    }

    /// Read the focused remote tab's link again if it is due. Returns whether
    /// what the corner shows has changed.
    fn tick_corner(&mut self) -> bool {
        let Some((session, transport)) = self.focused_remote() else {
            return self.link.take().is_some();
        };
        let due = match &self.link {
            Some(seen) if seen.session == session => seen.read.elapsed() >= link_health::EVERY,
            _ => true,
        };
        if !due {
            return false;
        }
        let seen = Seen {
            session: session.to_string(),
            transport,
            stats: link_health::read(session),
            read: Instant::now(),
        };
        // Measured on what is drawn: the file is rewritten every second with a
        // new timestamp and new byte counts, and a frame per write that looks
        // the same would be work for nothing.
        let changed = self
            .link
            .as_ref()
            .is_none_or(|old| label(old) != label(&seen));
        self.link = Some(seen);
        changed
    }
}

/// How the corner colours the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Under 100 ms: typing feels local.
    Good,
    /// Under 250 ms: noticeable, and the local echo is earning its keep.
    Slow,
    /// Slower than that, or the link is down and being dialled again.
    Bad,
    /// The bridge has said nothing recent, or has no round trip yet.
    Unknown,
}

/// Round trips under this are [`Tone::Good`].
const GOOD_MS: u32 = 100;
/// Round trips under this are [`Tone::Slow`]; anything above is [`Tone::Bad`].
const SLOW_MS: u32 = 250;

/// The cloud the tab bar marks a sibling's tab with, so the corner reads as
/// the same tab's. U+FE0E for the reason the bar gives — see `transport_mark`.
const CLOUD: &str = "☁\u{FE0E}";

/// The corner's text and tone for a reading: `☁ 118ms · 2↻ · ×4.1`.
///
/// Each part only when it says something: no reconnect count before the first
/// reconnect, no ratio before enough output to have one. `None` for an ssh tab,
/// which runs no bridge and so has no file to be missing — a `?` there would
/// be a question with no answer coming.
pub fn label(seen: &Seen) -> Option<(String, Tone)> {
    let Some(stats) = &seen.stats else {
        return match seen.transport {
            Transport::Peer => Some((format!("{CLOUD} ?"), Tone::Unknown)),
            Transport::Ssh => None,
        };
    };
    let mut parts = Vec::new();
    let tone = match stats.state {
        link_health::State::Reconnecting => {
            parts.push("reconnecting".to_string());
            Tone::Bad
        }
        link_health::State::Lost => {
            parts.push("unreachable".to_string());
            Tone::Bad
        }
        // A newer bridge's word for its link: the counts below still read,
        // and the state is a question rather than a guess.
        link_health::State::Unknown => {
            parts.push("?".to_string());
            Tone::Unknown
        }
        link_health::State::Connected => match stats.rtt_ms {
            Some(ms) => {
                parts.push(format!("{ms}ms"));
                match ms {
                    ms if ms < GOOD_MS => Tone::Good,
                    ms if ms < SLOW_MS => Tone::Slow,
                    _ => Tone::Bad,
                }
            }
            // Connected, and no keystroke has come back yet to time.
            None => {
                parts.push("—".to_string());
                Tone::Unknown
            }
        },
    };
    if stats.reconnects > 0 {
        parts.push(format!("{}↻", stats.reconnects));
    }
    if let Some(ratio) = stats.ratio() {
        parts.push(format!("×{ratio:.1}"));
    }
    Some((format!("{CLOUD} {}", parts.join(" · ")), tone))
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_health::State;

    fn stats(state: State, rtt: Option<u32>, reconnects: u32) -> Stats {
        Stats {
            state,
            rtt_ms: rtt,
            reconnects,
            raw_in: 41_000,
            wire_in: 10_000,
            raw_out: 0,
            wire_out: 0,
            since: 0,
            at: 0,
        }
    }

    fn peer(stats: Stats) -> Option<(String, Tone)> {
        label(&Seen::fixed("s", Transport::Peer, Some(stats)))
    }

    #[test]
    fn the_corner_says_rtt_reconnects_and_ratio() {
        let (text, tone) = peer(stats(State::Connected, Some(118), 2)).unwrap();
        assert_eq!(text, "☁\u{FE0E} 118ms · 2↻ · ×4.1");
        assert_eq!(tone, Tone::Slow);
    }

    #[test]
    fn the_colour_follows_the_round_trip() {
        let tone = |ms| peer(stats(State::Connected, Some(ms), 0)).unwrap().1;
        assert_eq!(tone(99), Tone::Good);
        assert_eq!(tone(100), Tone::Slow);
        assert_eq!(tone(249), Tone::Slow);
        assert_eq!(tone(250), Tone::Bad);
        // Reconnecting is red whatever the last round trip was.
        let (text, tone) = peer(stats(State::Reconnecting, Some(20), 3)).unwrap();
        assert_eq!(tone, Tone::Bad);
        assert_eq!(text, "☁\u{FE0E} reconnecting · 3↻ · ×4.1");
    }

    /// Given up on: the corner says the machine is unreachable, in red.
    #[test]
    fn a_lost_link_says_unreachable() {
        let (text, tone) = peer(stats(State::Lost, Some(20), 3)).unwrap();
        assert_eq!(text, "☁\u{FE0E} unreachable · 3↻ · ×4.1");
        assert_eq!(tone, Tone::Bad);
    }

    /// A state from a newer bridge is a question, with the counts it still
    /// carries.
    #[test]
    fn a_state_this_build_has_no_word_for_is_a_question() {
        let (text, tone) = peer(stats(State::Unknown, Some(20), 3)).unwrap();
        assert_eq!(text, "☁\u{FE0E} ? · 3↻ · ×4.1");
        assert_eq!(tone, Tone::Unknown);
    }

    #[test]
    fn nothing_said_is_a_question_only_where_an_answer_could_come() {
        let unknown = label(&Seen::fixed("s", Transport::Peer, None)).unwrap();
        assert_eq!(unknown, ("☁\u{FE0E} ?".to_string(), Tone::Unknown));
        assert_eq!(label(&Seen::fixed("s", Transport::Ssh, None)), None);
        // The quiet parts stay out until they have something to say.
        let mut fresh = stats(State::Connected, Some(40), 0);
        fresh.wire_in = 0;
        assert_eq!(peer(fresh).unwrap().0, "☁\u{FE0E} 40ms");
    }
}
