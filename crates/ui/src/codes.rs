//! A key sequence typed anywhere that switches something on and, typed again,
//! off — what [`super::rave`], [`super::drunk`] and [`super::high`] each listen
//! for.
//!
//! The three were one matcher written out three times, down to the comment on
//! the overlap rule: a counter of how far along the sequence the keyboard is,
//! the moment it was switched on, and the status line it says on the way in
//! and out. They differ in their keys and their words, and that is all
//! [`Code`] is given.
//!
//! Built to cost nothing while nobody is typing it: one comparison and one
//! counter per key.

use super::*;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use unicode_width::UnicodeWidthStr;

/// How far along its sequence the keyboard is, and since when it is on.
#[derive(Debug)]
pub struct Code<const N: usize> {
    keys: [KeyCode; N],
    heard: usize,
    since: Option<Instant>,
}

/// What a key was to a code.
#[derive(Debug, PartialEq, Eq)]
pub enum Heard {
    /// Nothing to do with it: the key does what it always does.
    Pass,
    /// Part of the code and not harmless alone — a letter, which would open
    /// something or be typed at the agent in a pane — so kept from whatever
    /// else would have taken it. The arrows pass through: they only move.
    Swallow,
    /// The code is complete: whatever it stands for starts, or ends.
    Toggle,
}

/// The status line a code says on the way in, on the way out, and — for one
/// that is nothing without colour — instead of starting on a terminal that has
/// none.
pub struct Says {
    pub on: &'static str,
    pub off: &'static str,
    pub colourless: Option<&'static str>,
}

impl<const N: usize> Code<N> {
    pub const fn new(keys: [KeyCode; N]) -> Self {
        Code {
            keys,
            heard: 0,
            since: None,
        }
    }

    /// Take one key, returning what it is to the code.
    ///
    /// A key held with a modifier is never part of one: Ctrl+↑ is something
    /// else, and has to break the run rather than count as the arrow.
    pub fn hear_key(&mut self, key: KeyEvent) -> Heard {
        self.hear(match key.modifiers.is_empty() {
            true => key.code,
            false => KeyCode::Null,
        })
    }

    /// Take one key code, returning what it is to the code.
    pub fn hear(&mut self, code: KeyCode) -> Heard {
        if code != self.keys[self.heard] {
            self.heard = self.overlap(code);
            return Heard::Pass;
        }
        self.heard += 1;
        if self.heard == N {
            self.heard = 0;
            self.since = match self.since {
                Some(_) => None,
                None => Some(Instant::now()),
            };
            return Heard::Toggle;
        }
        match code {
            KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => Heard::Pass,
            _ => Heard::Swallow,
        }
    }

    /// How far in the keyboard still is after `code` broke the run: the
    /// longest start of the sequence that the keys just typed end with.
    ///
    /// Every code here opens on the same key twice, so in practice this is the
    /// rule each of them used to spell out: a third of the opening key after
    /// two is still two in, any other opening key is one in, and anything else
    /// starts over. Worked out rather than written down, so a new code cannot
    /// get it wrong.
    fn overlap(&self, code: KeyCode) -> usize {
        let typed = || self.keys[..self.heard].iter().chain([&code]);
        (1..=self.heard)
            .rev()
            .find(|&k| typed().skip(self.heard + 1 - k).eq(&self.keys[..k]))
            .unwrap_or(0)
    }

    /// How long it has been on, while it is.
    pub fn elapsed(&self) -> Option<Duration> {
        self.since.map(|since| since.elapsed())
    }

    pub fn is_on(&self) -> bool {
        self.since.is_some()
    }

    /// Switch it on, keeping the start of a run already going.
    pub fn start(&mut self) {
        self.since.get_or_insert_with(Instant::now);
    }

    pub fn stop(&mut self) {
        self.since = None;
    }

    #[cfg(test)]
    pub fn heard(&self) -> usize {
        self.heard
    }
}

impl App {
    /// Feed `key` to the code `which` picks out, `true` when the key was the
    /// code's to keep; a completed code says what it did on the status line.
    pub(super) fn hear_code<const N: usize>(
        &mut self,
        key: KeyEvent,
        which: fn(&mut App) -> &mut Code<N>,
        says: &Says,
    ) -> bool {
        let code = which(self);
        let heard = code.hear_key(key);
        if heard == Heard::Toggle {
            let said = match (code.is_on(), says.colourless) {
                (true, Some(refusal)) if theme::no_color() => {
                    code.stop();
                    refusal
                }
                (true, _) => says.on,
                (false, _) => says.off,
            };
            self.set_status(said);
        }
        heard != Heard::Pass
    }
}

/// The way out, centred on the footer, where it is wide enough to hold it.
/// Returns the column it starts at, for anything drawn beside it.
pub fn sign(buf: &mut Buffer, footer: Rect, label: &str, style: Style) -> Option<u16> {
    let width = label.width() as u16;
    if footer.height == 0 || footer.width <= width {
        return None;
    }
    let x = footer.x + (footer.width - width) / 2;
    buf.set_string(x, footer.y, label, style);
    Some(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The overlap that is worked out is the one each code spelled out by
    /// hand: for every code, every key after every prefix lands where the old
    /// rule put it.
    #[test]
    fn the_overlap_is_the_rule_the_codes_spelled_out() {
        let keys = [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Char('a'),
            KeyCode::Char('b'),
            KeyCode::Char('h'),
            KeyCode::Char('i'),
            KeyCode::Char('x'),
        ];
        for sequence in [rave::CODE, drunk::CODE, high::CODE] {
            let opening = sequence[0];
            for heard in 0..sequence.len() {
                for &key in &keys {
                    if key == sequence[heard] {
                        continue;
                    }
                    let mut code = Code::new(sequence);
                    code.heard = heard;
                    code.hear(key);
                    let old = match key {
                        k if k == opening && heard == 2 => 2,
                        k if k == opening => 1,
                        _ => 0,
                    };
                    assert_eq!(code.heard, old, "{sequence:?} at {heard}, then {key:?}");
                }
            }
        }
    }

    /// A key with a modifier breaks the run instead of counting.
    #[test]
    fn a_modified_key_is_not_part_of_a_code() {
        let mut code = Code::new(rave::CODE);
        code.hear(KeyCode::Up);
        let ctrl = KeyEvent::new(KeyCode::Up, crossterm::event::KeyModifiers::CONTROL);
        assert_eq!(code.hear_key(ctrl), Heard::Pass);
        assert_eq!(code.heard, 0);
    }

    /// The sign sits in the middle, and stays off a footer too narrow for it.
    #[test]
    fn the_sign_is_centred_or_not_drawn() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
        let area = buf.area;
        assert_eq!(sign(&mut buf, area, " hi ", Style::default()), Some(8));
        assert_eq!(
            sign(&mut buf, Rect::new(0, 0, 4, 1), " hi ", Style::default()),
            None
        );
        assert_eq!(
            sign(&mut buf, Rect::new(0, 0, 20, 0), " hi ", Style::default()),
            None
        );
    }
}
