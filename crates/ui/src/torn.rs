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
//!
//! The report's tail is caught on its own too, with no Esc in front of it. Over
//! ssh the two halves of a split read can land further apart than any window a
//! person would accept on their Esc key, and then the Esc has already gone
//! through by the time `[<35;141;17M` arrives as typing. A `[` is held until
//! the next key says whether a `<` follows it; nobody types `[<` then three
//! numbers and an `M`, and everything else is released the moment it cannot be
//! a report.
//!
//! Two more ways a report arrives torn. A read that ends after `\x1b[` is
//! decoded by crossterm as Alt+`[` — an Esc followed by a character is how a
//! terminal spells Alt — so the report opens with that instead of an Esc. And
//! when the `[` has gone through on its own, the tail starts at `<`. Both are
//! held the same way; a `<` typed on purpose waits only for the next key.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

/// How long an Esc waits for the rest of a report before it is taken to be an
/// Esc.
///
/// The second half of a split read usually lands within a few milliseconds; this
/// allows for an ssh hop that holds it for longer, and is still under what a
/// person pressing Esc would notice. A tail later than this is still caught,
/// without its Esc — see the module docs.
pub const WINDOW: Duration = Duration::from_millis(100);

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
            let opens = match (key.code, key.modifiers) {
                (KeyCode::Char('['), KeyModifiers::ALT) => true,
                (KeyCode::Esc | KeyCode::Char('[' | '<'), m) => m.is_empty(),
                _ => false,
            };
            if opens {
                self.held.push(key);
                self.since = Some(now);
                return Vec::new();
            }
            return vec![key];
        }
        self.held.push(key);
        self.since = Some(now);
        // Measured from after the Esc when there is one, so a report reads the
        // same whether or not its Esc was the half that arrived.
        let after = match self.held[0].code {
            KeyCode::Esc => &self.held[1..],
            _ => &self.held[..],
        };
        match shape(&spell(after)) {
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

/// The keys after an Esc as the text of a report, `[` included — or `None`
/// when one of them could not be part of a report at all.
///
/// An Alt+`[` in front is the Esc and `[` read as one key, and a lone `<` in
/// front is a tail whose `[` went through already; both spell the same text as
/// the report they came from.
fn spell(keys: &[KeyEvent]) -> Option<String> {
    let mut text = String::new();
    for (i, key) in keys.iter().enumerate() {
        match (key.code, key.modifiers) {
            (KeyCode::Char('['), KeyModifiers::ALT) if i == 0 => text.push('['),
            // The reader marks a capital as shifted, and `M` is one: shift is
            // the only modifier a character of a report can arrive with.
            (KeyCode::Char(c), m) if (m - KeyModifiers::SHIFT).is_empty() => text.push(c),
            _ => return None,
        }
    }
    if text.starts_with('<') {
        text.insert(0, '[');
    }
    Some(text)
}

/// What the keys after an Esc spell, as far as an SGR mouse report goes.
fn shape(text: &Option<String>) -> Shape {
    let Some(text) = text else {
        return Shape::Neither;
    };
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

    /// The Esc already gone through on its own — the halves of the read landed
    /// further apart than the window — and the tail arriving as typing: still
    /// caught, while a `[` that opens nothing is typed at once.
    #[test]
    fn a_report_tail_without_its_esc_is_dropped_too() {
        let mut torn = Torn::default();
        let now = Instant::now();
        assert!(run(&mut torn, "[<35;141;17M[<0;97;19m", now).is_empty());
        assert_eq!(run(&mut torn, "a[b]", now), keys("a[b]"));
        assert_eq!(run(&mut torn, "[", now), Vec::<KeyEvent>::new());
        assert_eq!(torn.expire(now + WINDOW), keys("["));
    }

    /// The two tears seen in a real pane: the read ending after `\x1b[`, which
    /// the reader hands over as Alt+`[`, and the tail arriving from its `<`.
    #[test]
    fn a_report_opened_by_alt_bracket_or_a_bare_angle_is_dropped() {
        let mut torn = Torn::default();
        let now = Instant::now();
        let alt = KeyEvent::new(KeyCode::Char('['), KeyModifiers::ALT);
        let mut out = torn.feed(alt, now);
        out.extend(run(&mut torn, "<35;148;3M", now));
        assert!(out.is_empty(), "leaked: {out:?}");
        assert!(!torn.holding());

        assert!(run(&mut torn, "<35;148;3M<0;97;19m", now).is_empty());

        // Typed on purpose, both still arrive.
        assert_eq!(run(&mut torn, "a < b", now), keys("a < b"));
        assert_eq!(run(&mut torn, "<", now), Vec::<KeyEvent>::new());
        assert_eq!(torn.expire(now + WINDOW), keys("<"));
        let mut out = torn.feed(alt, now);
        out.extend(run(&mut torn, "x", now));
        assert_eq!(out, vec![alt, KeyEvent::from(KeyCode::Char('x'))]);
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
