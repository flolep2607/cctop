//! Mouse reports torn in half on their way in, caught before they reach an
//! agent as text.
//!
//! A terminal reports the mouse as an escape sequence — a hover is
//! `\x1b[<35;141;17M`, a release `\x1b[<0;97;19m`. When one arrives split across
//! two reads, with the lone `\x1b` at the end of the first, the key reader has
//! no way to tell it from a real Esc: it reports an Esc keypress, then `[`, `<`,
//! `3`, `5`… as typed characters. Inside a pane every key belongs to the agent,
//! so all of it went down the pty, and the agent printed `<35;141;17M` into the
//! prompt being written. Nothing about the split is rare: a lagging machine or
//! an ssh hop splits reads, and a pointer crossing the window produces a report
//! per cell.
//!
//! So an Esc bound for a pane is held for a moment. If what follows spells a
//! mouse report, the whole report is dropped; if anything else follows, or
//! nothing does, the Esc and whatever was held behind it go through exactly as
//! typed. An Esc pressed on purpose — the key that interrupts Claude — arrives
//! [`WINDOW`] later than it used to, which nobody can feel.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

/// How long an Esc waits for the rest of a report before it is taken to be an
/// Esc.
///
/// The second half of a split read comes from the same burst of input, so it
/// lands within a few milliseconds even over ssh; this is several times that,
/// and still well under anything a person pressing Esc could notice.
pub const WINDOW: Duration = Duration::from_millis(50);

/// Keys held back from a pane while they might still be a torn mouse report.
#[derive(Default)]
pub struct Torn {
    held: Vec<KeyEvent>,
    /// When the last held key arrived. Measured from the latest rather than the
    /// Esc, so a report whose pieces trickle in is still recognised whole.
    since: Option<Instant>,
}

impl Torn {
    /// Take one key bound for a pane, and give back the keys that should be
    /// delivered now — none while a report may be forming, or the held ones
    /// and this one once it clearly is not a report.
    pub fn feed(&mut self, key: KeyEvent, now: Instant) -> Vec<KeyEvent> {
        if self.held.is_empty() {
            if key.code == KeyCode::Esc && key.modifiers.is_empty() {
                self.held.push(key);
                self.since = Some(now);
                return Vec::new();
            }
            return vec![key];
        }
        self.held.push(key);
        self.since = Some(now);
        match shape(&self.held[1..]) {
            Shape::Report => {
                crate::elog::event("tui", "torn-mouse-report", serde_json::json!({}));
                self.held.clear();
                self.since = None;
                Vec::new()
            }
            Shape::Prefix => Vec::new(),
            // Everything before this key goes through as typed; this key is fed
            // again on its own, because it may be the Esc of the next report.
            Shape::Neither => {
                let last = self.held.pop().expect("just pushed");
                let mut out = std::mem::take(&mut self.held);
                self.since = None;
                out.extend(self.feed(last, now));
                out
            }
        }
    }

    /// The held keys, once [`WINDOW`] has passed without a report completing:
    /// they were typed, and belong to the agent.
    pub fn expire(&mut self, now: Instant) -> Vec<KeyEvent> {
        match self.since {
            Some(at) if now.duration_since(at) >= WINDOW => {
                self.since = None;
                std::mem::take(&mut self.held)
            }
            _ => Vec::new(),
        }
    }

    /// Whether anything is being held.
    #[cfg(test)]
    pub fn holding(&self) -> bool {
        !self.held.is_empty()
    }
}

enum Shape {
    /// `[<b;x;y` then `M` or `m`: an SGR mouse report, whole.
    Report,
    /// The start of one, so far.
    Prefix,
    Neither,
}

/// What the keys after an Esc spell, as far as an SGR mouse report goes.
fn shape(keys: &[KeyEvent]) -> Shape {
    let mut text = String::new();
    for key in keys {
        // The reader marks a capital as shifted, and `M` is one: shift is the
        // only modifier a character of a report can arrive with.
        let (KeyCode::Char(c), m) = (key.code, key.modifiers) else {
            return Shape::Neither;
        };
        if !(m - KeyModifiers::SHIFT).is_empty() {
            return Shape::Neither;
        }
        text.push(c);
    }
    if text.is_empty() || text == "[" {
        return Shape::Prefix;
    }
    let Some(body) = text.strip_prefix("[<") else {
        return Shape::Neither;
    };
    let (fields, end) = match body.char_indices().last() {
        Some((at, 'M' | 'm')) => (&body[..at], true),
        _ => (body, false),
    };
    let parts: Vec<&str> = fields.split(';').collect();
    let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    if parts.len() > 3 || !parts.iter().all(|p| digits(p)) {
        return Shape::Neither;
    }
    match end {
        true if parts.len() == 3 && parts.iter().all(|p| !p.is_empty()) => Shape::Report,
        true => Shape::Neither,
        false => Shape::Prefix,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(text: &str) -> Vec<KeyEvent> {
        text.chars()
            .map(|c| match c {
                '\x1b' => KeyEvent::from(KeyCode::Esc),
                c if c.is_ascii_uppercase() => KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT),
                c => KeyEvent::from(KeyCode::Char(c)),
            })
            .collect()
    }

    fn run(torn: &mut Torn, text: &str, now: Instant) -> Vec<KeyEvent> {
        keys(text)
            .into_iter()
            .flat_map(|k| torn.feed(k, now))
            .collect()
    }

    /// The reports from the bug, a hover and a release, torn after their
    /// `\x1b`: none of it reaches the agent.
    #[test]
    fn a_torn_mouse_report_never_reaches_the_pane() {
        let mut torn = Torn::default();
        let now = Instant::now();
        let out = run(&mut torn, "\x1b[<35;141;17M\x1b[<0;97;19m", now);
        assert!(out.is_empty(), "leaked: {out:?}");
        assert!(!torn.holding());
    }

    /// Text around a torn report keeps its order and loses nothing.
    #[test]
    fn typing_on_either_side_of_a_report_goes_through() {
        let mut torn = Torn::default();
        let now = Instant::now();
        assert_eq!(run(&mut torn, "hi\x1b[<35;1;1Mthere", now), keys("hithere"));
    }

    /// An Esc on purpose is held, then delivered: at once when something that
    /// cannot be a report follows, and after the window when nothing does.
    #[test]
    fn a_real_esc_still_arrives() {
        let mut torn = Torn::default();
        let now = Instant::now();
        assert!(run(&mut torn, "\x1b", now).is_empty());
        assert!(torn.expire(now).is_empty(), "released before the window");
        assert_eq!(torn.expire(now + WINDOW), keys("\x1b"));

        assert_eq!(run(&mut torn, "\x1bq", now), keys("\x1bq"));
        // A report's opening that turns out not to be one is handed back whole.
        assert_eq!(run(&mut torn, "\x1b[<3x", now), keys("\x1b[<3x"));
        // Esc Esc: the first is plainly typed, the second is held in its turn.
        assert_eq!(run(&mut torn, "\x1b\x1b", now), keys("\x1b"));
        assert_eq!(torn.expire(now + WINDOW), keys("\x1b"));
    }

    /// A report with a field missing or too many is not one, and is typed.
    #[test]
    fn only_a_whole_report_is_dropped() {
        let mut torn = Torn::default();
        let now = Instant::now();
        assert_eq!(run(&mut torn, "\x1b[<35;1M", now), keys("\x1b[<35;1M"));
        assert_eq!(
            run(&mut torn, "\x1b[<1;2;3;4M", now),
            keys("\x1b[<1;2;3;4M")
        );
    }
}
