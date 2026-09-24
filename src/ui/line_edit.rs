//! One line of editable text: what every input box on the dashboard types into.
//!
//! The filter, the send box, the launcher's directory, the tab name and the
//! rest each used to be a `String` that only ever grew at the end and shrank
//! from it — `push` on a letter, `pop` on Backspace, and nothing else. This is
//! the one editor they share, so a box that takes text takes it the way a shell
//! prompt does: a cursor that moves, words that can be jumped and deleted, and
//! a line that can be killed either side of the cursor.
//!
//! Written here rather than taken from `tui-input` or `ratatui-textarea`. The
//! second is a multi-line editor with its own widget, which is more than a
//! one-line box wants and a second opinion on how a modal is drawn. The first
//! is close to this, but it brings its own crossterm feature and its own
//! keymap, and several fields here have already given some of those keys to
//! something else — the rename box's arrows move its colour, the directory
//! box's Tab completes — so the keymap would have been filtered at every call
//! site anyway. What is left is small enough to own, and `unicode-width` and
//! `unicode-segmentation` are already in the tree under ratatui.
//!
//! The cursor moves by grapheme, not by `char`: an accented letter typed as a
//! letter plus a combining mark, or a flag, is one thing on screen, and a
//! cursor that could land inside it would be somewhere nobody can see. Columns
//! are measured with `unicode-width`, so a wide character takes the two cells
//! the terminal gives it.
//!
//! ponytail: one line only. Every box is drawn in one strip, and a paste has
//! its line breaks flattened before it gets here (see `input::flatten`) —
//! including the send box's, whose Enter is what submits.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The text of a box and where its cursor is.
///
/// The cap is not stored. Each box's is a constant beside the key handler that
/// enforces it, and keeping it out of here is what lets a test, or the code
/// that recalls history, assign a field with `"text".into()` without having to
/// know it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineEdit {
    text: String,
    /// A byte offset into `text`, always on a grapheme boundary.
    cursor: usize,
}

/// What a key did to the field, for the callers that react to a change — the
/// filter re-runs on one, the directory box re-suggests — and not to a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// The text is different.
    Changed,
    /// An editing key, but the text is the same: the cursor moved, or there
    /// was nothing to delete.
    Unchanged,
    /// Not a key the editor has a meaning for.
    Ignored,
}

impl Edit {
    pub(super) fn changed(self) -> bool {
        self == Edit::Changed
    }
}

/// Whether a grapheme belongs to a word, for the word jumps and deletes.
///
/// Letters, digits and the underscore, as readline's Alt+B and Alt+F have it.
/// So a word jump in `~/src/cctop` stops at each path component, which is the
/// unit anyone correcting a path wants to delete.
fn is_word(g: &str) -> bool {
    g.chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
}

impl From<String> for LineEdit {
    /// The cursor at the end, where it is after typing the text.
    fn from(text: String) -> Self {
        let cursor = text.len();
        LineEdit { text, cursor }
    }
}

impl From<&str> for LineEdit {
    fn from(text: &str) -> Self {
        text.to_string().into()
    }
}

impl std::ops::Deref for LineEdit {
    type Target = str;
    fn deref(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Display for LineEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartialEq<str> for LineEdit {
    fn eq(&self, other: &str) -> bool {
        self.text == other
    }
}

impl PartialEq<String> for LineEdit {
    fn eq(&self, other: &String) -> bool {
        self.text == *other
    }
}

impl PartialEq<&str> for LineEdit {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

impl LineEdit {
    /// The byte offset of the cursor.
    #[cfg(test)]
    pub(super) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(super) fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// How many more characters fit under `cap`.
    ///
    /// Characters rather than bytes, which is what the caps used to count in
    /// some boxes and not others: a cap in bytes gives someone typing in
    /// Japanese a third of the box everybody else gets.
    pub(super) fn room(&self, cap: usize) -> usize {
        cap.saturating_sub(self.text.chars().count())
    }

    /// Put `s` in at the cursor, or as much of it as fits under `cap`, and
    /// leave the cursor after it. True when anything went in.
    ///
    /// The text is taken as given: a paste is flattened to one line by the
    /// caller, which knows what the field it is aiming at can hold.
    pub(super) fn insert_str(&mut self, s: &str, cap: usize) -> bool {
        let room = self.room(cap);
        let end = s.char_indices().nth(room).map_or(s.len(), |(i, _)| i);
        if end == 0 {
            return false;
        }
        self.text.insert_str(self.cursor, &s[..end]);
        self.cursor += end;
        // A combining mark typed after a letter joins the letter, which leaves
        // the cursor where it was; one typed before a mark already there
        // would leave it inside the cluster, so it goes to the far side.
        self.cursor = self.boundary_at_or_after(self.cursor);
        true
    }

    /// Apply one key under `cap`.
    ///
    /// Callers match the keys their own mode has given a meaning to first and
    /// hand the rest here, so a key that is already something else in a box —
    /// the rename box's arrows, the directory box's Tab — never reaches this.
    pub(super) fn key(&mut self, key: KeyEvent, cap: usize) -> Edit {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let before = self.text.len();
        let moved = |to: usize, this: &mut Self| {
            this.cursor = to;
            Edit::Unchanged
        };
        match key.code {
            KeyCode::Home => moved(0, self),
            KeyCode::End => moved(self.text.len(), self),
            KeyCode::Char('a') if ctrl => moved(0, self),
            KeyCode::Char('e') if ctrl => moved(self.text.len(), self),
            KeyCode::Left if ctrl || alt => moved(self.word_left(), self),
            KeyCode::Right if ctrl || alt => moved(self.word_right(), self),
            KeyCode::Char('b') if alt => moved(self.word_left(), self),
            KeyCode::Char('f') if alt => moved(self.word_right(), self),
            KeyCode::Left => moved(self.prev_boundary(), self),
            KeyCode::Right => moved(self.next_boundary(), self),
            // Ctrl+Backspace too: it is the word delete in every editor that
            // is not a terminal, and terminals that can tell it apart send it.
            KeyCode::Backspace if ctrl || alt => self.delete_to(self.word_left()),
            KeyCode::Char('w') if ctrl => self.delete_to(self.word_left()),
            KeyCode::Backspace => self.delete_to(self.prev_boundary()),
            KeyCode::Delete if ctrl || alt => self.delete_to(self.word_right()),
            KeyCode::Char('d') if alt => self.delete_to(self.word_right()),
            KeyCode::Delete => self.delete_to(self.next_boundary()),
            KeyCode::Char('u') if ctrl => self.delete_to(0),
            KeyCode::Char('k') if ctrl => self.delete_to(self.text.len()),
            // A letter with Ctrl or Alt held is a command this editor does not
            // have, not a letter: typing `w` into the box on a Ctrl+W that
            // meant something elsewhere is how a field fills with junk.
            KeyCode::Char(_) if ctrl || alt => Edit::Ignored,
            KeyCode::Char(c) => {
                let mut buf = [0; 4];
                self.insert_str(c.encode_utf8(&mut buf), cap);
                if self.text.len() == before {
                    Edit::Unchanged
                } else {
                    Edit::Changed
                }
            }
            _ => Edit::Ignored,
        }
    }

    /// Remove the text between the cursor and `to`, either side of it.
    fn delete_to(&mut self, to: usize) -> Edit {
        let (from, to) = (self.cursor.min(to), self.cursor.max(to));
        if from == to {
            return Edit::Unchanged;
        }
        self.text.replace_range(from..to, "");
        self.cursor = from;
        Edit::Changed
    }

    fn prev_boundary(&self) -> usize {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self) -> usize {
        self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map_or(self.cursor, |g| self.cursor + g.len())
    }

    /// The first grapheme boundary at or past `at`.
    fn boundary_at_or_after(&self, at: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|&i| i >= at)
            .unwrap_or(self.text.len())
    }

    /// The start of the word the cursor is in or after: back over whatever
    /// separates it, then over the word.
    fn word_left(&self) -> usize {
        let mut at = self.cursor;
        let mut in_word = false;
        for (i, g) in self.text[..self.cursor].grapheme_indices(true).rev() {
            if is_word(g) {
                in_word = true;
            } else if in_word {
                break;
            }
            at = i;
        }
        at
    }

    /// The end of the word the cursor is in or before, mirroring `word_left`.
    fn word_right(&self) -> usize {
        let mut at = self.cursor;
        let mut in_word = false;
        for g in self.text[self.cursor..].graphemes(true) {
            if is_word(g) {
                in_word = true;
            } else if in_word {
                break;
            }
            at += g.len();
        }
        at
    }

    /// The field drawn into `width` columns, the cursor included: the cell
    /// under it reversed, or `end` after the text when it is at the end.
    ///
    /// When the text is wider than the box, the part shown is the part the
    /// cursor is in, with `…` where text runs off either edge. Worked out from
    /// the cursor alone rather than remembered between frames: the start is
    /// shown while the cursor is near it, and otherwise the cursor sits at the
    /// right edge — so typing at the end of a long path shows its tail, which
    /// is what the directory box always did. A box that wraps instead passes
    /// `usize::MAX`.
    pub(super) fn spans(
        &self,
        width: usize,
        text: Style,
        cursor: Style,
        end: &'static str,
    ) -> Vec<Span<'static>> {
        let mut cells: Vec<(usize, &str, usize)> = self
            .text
            .grapheme_indices(true)
            .map(|(i, g)| (i, g, g.width()))
            .collect();
        let n = cells.len();
        let at = cells
            .iter()
            .position(|&(i, _, _)| i == self.cursor)
            .unwrap_or(n);
        if at == n {
            cells.push((self.text.len(), end, end.width()));
        }
        let m = cells.len();
        let used = |lo: usize, hi: usize| -> usize { cells[lo..hi].iter().map(|c| c.2).sum() };
        let fits = |lo: usize, hi: usize| {
            used(lo, hi)
                .saturating_add(usize::from(lo > 0))
                .saturating_add(usize::from(hi < m))
                <= width
        };
        let (lo, hi) = if fits(0, m) {
            (0, m)
        } else if fits(0, at + 1) {
            let mut hi = at + 1;
            while hi < m && fits(0, hi + 1) {
                hi += 1;
            }
            (0, hi)
        } else {
            let mut lo = at;
            while lo > 0 && fits(lo - 1, at + 1) {
                lo -= 1;
            }
            (lo, at + 1)
        };

        let join = |cells: &[(usize, &str, usize)]| cells.iter().map(|c| c.1).collect::<String>();
        let mut spans = Vec::with_capacity(5);
        if lo > 0 {
            spans.push(Span::styled("…", text));
        }
        spans.push(Span::styled(join(&cells[lo..at]), text));
        spans.push(match at < n {
            // Reversed rather than recoloured, so the letter under the cursor
            // stays legible under NO_COLOR too, where every style is the same
            // ink and reversal is the only mark left.
            true => Span::styled(
                cells[at].1.to_string(),
                cursor.add_modifier(Modifier::REVERSED),
            ),
            false => Span::styled(end, cursor),
        });
        spans.push(Span::styled(join(&cells[(at + 1).min(n)..hi.min(n)]), text));
        if hi < m {
            spans.push(Span::styled("…", text));
        }
        spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }
    fn alt(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::ALT)
    }
    fn typed(field: &mut LineEdit, s: &str) {
        for c in s.chars() {
            field.key(press(KeyCode::Char(c)), usize::MAX);
        }
    }
    /// The text with a `|` where the cursor is.
    fn shown(field: &LineEdit) -> String {
        let mut s = field.to_string();
        s.insert(field.cursor(), '|');
        s
    }

    #[test]
    fn typing_goes_in_at_the_cursor() {
        let mut f = LineEdit::default();
        typed(&mut f, "helo");
        f.key(press(KeyCode::Left), 64);
        typed(&mut f, "l");
        assert_eq!(shown(&f), "hell|o");
        f.key(press(KeyCode::Home), 64);
        typed(&mut f, ">");
        assert_eq!(shown(&f), ">|hello");
        f.key(ctrl(KeyCode::Char('e')), 64);
        assert_eq!(shown(&f), ">hello|");
        f.key(ctrl(KeyCode::Char('a')), 64);
        assert_eq!(shown(&f), "|>hello");
        f.key(press(KeyCode::End), 64);
        assert_eq!(shown(&f), ">hello|");
    }

    #[test]
    fn backspace_and_delete_take_one_grapheme_either_side() {
        let mut f = LineEdit::from("abc");
        f.key(press(KeyCode::Left), 64);
        assert!(f.key(press(KeyCode::Backspace), 64).changed());
        assert_eq!(shown(&f), "a|c");
        assert!(f.key(press(KeyCode::Delete), 64).changed());
        assert_eq!(shown(&f), "a|");
        // Nothing past the end, and nothing before the start: an editing key
        // that did nothing, not a change.
        assert_eq!(f.key(press(KeyCode::Delete), 64), Edit::Unchanged);
        f.key(press(KeyCode::Home), 64);
        assert_eq!(f.key(press(KeyCode::Backspace), 64), Edit::Unchanged);
    }

    #[test]
    fn word_jumps_stop_at_each_word_and_path_component() {
        let mut f = LineEdit::from("~/src/cctop  fix it");
        f.key(ctrl(KeyCode::Left), 64);
        assert_eq!(shown(&f), "~/src/cctop  fix |it");
        f.key(alt(KeyCode::Char('b')), 64);
        assert_eq!(shown(&f), "~/src/cctop  |fix it");
        f.key(ctrl(KeyCode::Left), 64);
        assert_eq!(shown(&f), "~/src/|cctop  fix it");
        f.key(ctrl(KeyCode::Left), 64);
        f.key(ctrl(KeyCode::Left), 64);
        assert_eq!(shown(&f), "|~/src/cctop  fix it");
        f.key(ctrl(KeyCode::Right), 64);
        assert_eq!(shown(&f), "~/src|/cctop  fix it");
        f.key(alt(KeyCode::Char('f')), 64);
        assert_eq!(shown(&f), "~/src/cctop|  fix it");
    }

    #[test]
    fn word_and_line_deletes() {
        let mut f = LineEdit::from("send the patch");
        f.key(ctrl(KeyCode::Char('w')), 64);
        assert_eq!(shown(&f), "send the |");
        f.key(alt(KeyCode::Backspace), 64);
        assert_eq!(shown(&f), "send |");
        f.key(ctrl(KeyCode::Backspace), 64);
        assert_eq!(shown(&f), "|");

        let mut f = LineEdit::from("one two three");
        f.key(ctrl(KeyCode::Left), 64);
        f.key(alt(KeyCode::Char('d')), 64);
        assert_eq!(shown(&f), "one two |");
        let mut f = LineEdit::from("one two three");
        f.key(ctrl(KeyCode::Left), 64);
        f.key(ctrl(KeyCode::Char('k')), 64);
        assert_eq!(shown(&f), "one two |");
        f.key(press(KeyCode::Left), 64);
        f.key(ctrl(KeyCode::Char('u')), 64);
        assert_eq!(shown(&f), "| ");
    }

    #[test]
    fn multibyte_and_combined_characters_are_stepped_over_whole() {
        // é as e + a combining acute, a flag of two regional indicators, and a
        // family joined by zero-width joiners: each one thing on screen.
        let mut f = LineEdit::from("ae\u{301}🇫🇷👨‍👩‍👧z");
        f.key(press(KeyCode::Left), 64);
        f.key(press(KeyCode::Left), 64);
        assert_eq!(shown(&f), "ae\u{301}🇫🇷|👨‍👩‍👧z");
        f.key(press(KeyCode::Backspace), 64);
        assert_eq!(shown(&f), "ae\u{301}|👨‍👩‍👧z");
        f.key(press(KeyCode::Backspace), 64);
        assert_eq!(shown(&f), "a|👨‍👩‍👧z");
        f.key(press(KeyCode::Delete), 64);
        assert_eq!(shown(&f), "a|z");
        // A combining mark typed after a letter joins it.
        typed(&mut f, "e\u{301}");
        assert_eq!(shown(&f), "ae\u{301}|z");
        // And a letter is a word whatever script it is in.
        let mut f = LineEdit::from("日本語 テキスト");
        f.key(ctrl(KeyCode::Char('w')), 64);
        assert_eq!(shown(&f), "日本語 |");
    }

    #[test]
    fn the_cap_counts_characters_and_a_paste_is_cut_to_it() {
        let mut f = LineEdit::from("日本");
        assert_eq!(f.room(4), 2);
        assert!(f.insert_str("語テキスト", 4));
        assert_eq!(f, "日本語テ");
        assert_eq!(f.key(press(KeyCode::Char('x')), 4), Edit::Unchanged);
        assert!(!f.insert_str("x", 4));
        // In the middle, where the cursor is, not appended.
        let mut f = LineEdit::from("ad");
        f.key(press(KeyCode::Left), 64);
        f.insert_str("bc", 64);
        assert_eq!(shown(&f), "abc|d");
    }

    #[test]
    fn a_letter_with_ctrl_or_alt_is_not_typed() {
        let mut f = LineEdit::from("x");
        assert_eq!(f.key(ctrl(KeyCode::Char('v')), 64), Edit::Ignored);
        assert_eq!(f.key(alt(KeyCode::Char('z')), 64), Edit::Ignored);
        assert_eq!(f.key(press(KeyCode::Tab), 64), Edit::Ignored);
        // Shift is how a capital arrives, and it is typed.
        f.key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT), 64);
        assert_eq!(f, "xY");
    }

    fn drawn(f: &LineEdit, width: usize) -> String {
        f.spans(width, Style::default(), Style::default(), "█")
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    #[test]
    fn the_cursor_is_drawn_where_it_is_and_the_box_scrolls_to_it() {
        let mut f = LineEdit::from("abcdef");
        assert_eq!(drawn(&f, 80), "abcdef█");
        f.key(press(KeyCode::Home), 64);
        let spans = f.spans(80, Style::default(), Style::default(), "█");
        assert_eq!(spans[1].content, "a");
        assert!(spans[1].style.add_modifier.contains(Modifier::REVERSED));

        // Too long for the box: the tail while typing at the end...
        let mut f = LineEdit::from("0123456789");
        assert_eq!(drawn(&f, 6), "…6789█");
        // ...the head with the cursor near it...
        f.key(press(KeyCode::Home), 64);
        assert_eq!(drawn(&f, 6), "01234…");
        // ...and the cursor at the right edge in between.
        for _ in 0..7 {
            f.key(press(KeyCode::Right), 64);
        }
        assert_eq!(drawn(&f, 6), "…4567…");
    }

    #[test]
    fn wide_characters_take_two_columns_in_the_window() {
        let f = LineEdit::from("日本語テキスト");
        // Five columns: the ellipsis, two wide characters, and the cursor.
        assert_eq!(drawn(&f, 6), "…スト█");
        assert!(
            f.spans(6, Style::default(), Style::default(), "█")
                .iter()
                .map(|s| s.content.width())
                .sum::<usize>()
                <= 6
        );
    }
}
