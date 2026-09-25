//! ↓ ↓ ↑ ↑ → ← → ← a b, anywhere: the Konami code as it looks after a few.
//! The same again to sober up.
//!
//! The other easter egg, and built like [`super::rave`]: one counter advanced
//! per key, a paint pass that returns before touching the buffer, and every
//! frame a pure function of how long the night has been going.
//!
//! It is the one effect that moves text. Each row sways sideways on its own
//! slow swell, the whole screen hiccups now and then, blank cells beside a
//! word see it a second time, faintly, and the ground drifts through a haze of
//! colour. Nothing is ever written over a letter — the double vision lands
//! only in blanks — so every number still reads, a couple of cells from where
//! it was.
//!
//! ponytail: a click lands where the row was drawn before it swayed, not where
//! it is shown. At most three cells out, and it is the mode you asked for.
//!
//! No flashing, and nothing faster than a hiccup: the sway takes seconds to
//! swing, which keeps it on the right side of motion that makes people ill.

use super::rave::{hsv, snap};
use super::*;
use crossterm::event::KeyCode;
use ratatui::buffer::{Buffer, Cell, CellDiffOption};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthStr;

/// The code: the Konami code upside down and back to front, as it would be
/// remembered at two in the morning.
pub(super) const CODE: [KeyCode; 10] = [
    KeyCode::Down,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::Up,
    KeyCode::Right,
    KeyCode::Left,
    KeyCode::Right,
    KeyCode::Left,
    KeyCode::Char('a'),
    KeyCode::Char('b'),
];

/// Seconds between hiccups.
const HICCUP: f32 = 6.5;

/// How long a hiccup jolts the screen for.
const JOLT: f32 = 0.18;

/// How far along the code the keyboard is, and when the drinking started.
#[derive(Debug, Default)]
pub struct Drunk {
    heard: usize,
    since: Option<Instant>,
}

impl Drunk {
    /// Take one key, returning what it is to the code — the same contract as
    /// [`super::rave::Rave::hear`]: the arrows pass, `a` is kept (it would
    /// attach), and `b` completes it.
    pub fn hear(&mut self, code: KeyCode) -> rave::Heard {
        if code != CODE[self.heard] {
            // As with the rave, the only overlap is the opening run: a third ↓
            // after two is still two downs in.
            self.heard = match code {
                KeyCode::Down if self.heard == 2 => 2,
                KeyCode::Down => 1,
                _ => 0,
            };
            return rave::Heard::Pass;
        }
        self.heard += 1;
        match self.heard {
            n if n == CODE.len() => {
                self.heard = 0;
                self.since = match self.since {
                    Some(_) => None,
                    None => Some(Instant::now()),
                };
                rave::Heard::Toggle
            }
            n if n > 8 => rave::Heard::Swallow,
            _ => rave::Heard::Pass,
        }
    }

    /// How long the night has been going, while it is.
    pub fn elapsed(&self) -> Option<Duration> {
        self.since.map(|since| since.elapsed())
    }
}

impl App {
    /// Feed a key to the code, `true` when it was the code's to keep.
    pub(super) fn hear_drunk(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let code = match key.modifiers.is_empty() {
            true => key.code,
            false => KeyCode::Null,
        };
        match self.drunk.hear(code) {
            rave::Heard::Pass => false,
            rave::Heard::Swallow => true,
            rave::Heard::Toggle => {
                match self.drunk.since {
                    Some(_) => self.set_status("🍺 Drunk mode. The same code again to sober up"),
                    None => self.set_status("Water, and bed. Back to work"),
                }
                true
            }
        }
    }

    /// Whether the screen is swaying, and wants a frame every
    /// [`effects::FRAME`] to do it.
    pub fn reeling(&self) -> bool {
        self.drunk.since.is_some()
    }
}

/// Repaint `buf` as the night is `t` in, with the way out across `footer`
/// when `label` — which is when the rave is not already using the footer.
///
/// `colour` is `None` on a terminal asked for no colour, where the double
/// vision is dimmed rather than greyed and there is no haze; otherwise it is
/// whether the terminal takes truecolor.
pub fn paint(buf: &mut Buffer, footer: Rect, t: Duration, colour: Option<bool>, label: bool) {
    let secs = t.as_secs_f32();
    let hiccup = hiccuping(secs);
    let area = buf.area;
    for y in area.top()..area.bottom() {
        sway(buf, y, lean(secs, y) + i32::from(hiccup) * 2);
    }
    // Seeing double comes and goes, rather than being a fixed feature of the
    // screen that the eye stops noticing.
    if (secs * 0.5).sin() > -0.3 {
        ghost(buf, 2, colour);
    }
    if let Some(truecolor) = colour {
        haze(buf, secs, truecolor);
    }
    if label {
        sign(buf, footer.intersection(area), hiccup, colour);
    }
}

/// How far row `y` leans at `secs`, in columns: two swells at unrelated rates,
/// so the rows never settle into a wave the eye can follow, and eased in over
/// the first two seconds — the first drink takes a moment.
fn lean(secs: f32, y: u16) -> i32 {
    let fy = f32::from(y);
    let tipsy = (secs / 2.0).min(1.0);
    let lean = 2.2 * (secs * 0.9 + fy * 0.35).sin() + 0.8 * (secs * 0.37 - fy * 0.21).sin();
    (tipsy * lean).round() as i32
}

/// Whether the screen is mid-hiccup. Never in the first stretch, so the code
/// itself is not met with one.
fn hiccuping(secs: f32) -> bool {
    secs >= HICCUP && secs.rem_euclid(HICCUP) < JOLT
}

/// Shift row `y` right by `dx` columns (left when negative).
///
/// Not a row holding an image or a link, whose cells are drawn from somewhere
/// other than where they sit and would come apart if moved. The row moves as
/// a block, so a wide glyph keeps the blank cell it covers; only one pushed
/// into the last column, with nowhere for its second half, is dropped.
fn sway(buf: &mut Buffer, y: u16, dx: i32) {
    let area = buf.area;
    if dx == 0 {
        return;
    }
    let row: Vec<Cell> = (area.left()..area.right())
        .map(|x| buf[(x, y)].clone())
        .collect();
    if row.iter().any(|c| c.diff_option != CellDiffOption::None) {
        return;
    }
    let width = row.len() as i32;
    for (i, was) in row.iter().enumerate() {
        let from = i as i32 - dx;
        let mut cell = match (0..width).contains(&from) {
            true => row[from as usize].clone(),
            // What slid in from off the edge: nothing, on the ground that
            // was there.
            false => {
                let mut blank = Cell::default();
                blank.bg = was.bg;
                blank
            }
        };
        if i as i32 == width - 1 && cell.symbol().width() > 1 {
            cell.set_symbol(" ");
        }
        buf[(area.left() + i as u16, y)] = cell;
    }
}

/// Echo every glyph `offset` columns to its right, faintly, where that lands
/// on open ground: a blank with at least `offset` more blanks after it. So an
/// echo never covers anything, never fills the space between two words, and
/// never sits against the next word to be read as part of it.
fn ghost(buf: &mut Buffer, offset: u16, colour: Option<bool>) {
    let area = buf.area;
    let faint = match colour {
        Some(truecolor) => Style::default().fg(snap((95, 95, 125), truecolor)),
        None => Style::default().add_modifier(Modifier::DIM),
    };
    for y in area.top()..area.bottom() {
        // Read before anything is written, so an echo is always of the
        // original and one echo never makes the ground beside it look taken.
        let row: Vec<(String, bool)> = (area.left()..area.right())
            .map(|x| {
                let cell = &buf[(x, y)];
                let plain = cell.diff_option == CellDiffOption::None;
                (cell.symbol().to_string(), plain)
            })
            .collect();
        let blank = |i: usize| row.get(i).is_none_or(|(s, plain)| s == " " && *plain);
        let off = usize::from(offset);
        for i in off..row.len() {
            let (glyph, plain) = &row[i - off];
            // The blank half of a wide glyph is not open ground: the glyph
            // covers it on screen.
            let covered = row[i - 1].0.width() > 1;
            let open = (i..=i + off).all(blank) && !covered;
            if open && *plain && glyph != " " && glyph.width() == 1 {
                let cell = &mut buf[(area.left() + i as u16, y)];
                cell.set_symbol(glyph);
                cell.set_style(faint);
            }
        }
    }
}

/// Tint the bare ground — cells with no background of their own — through a
/// slow drift of colour, a row at a time. Anything with a background of its
/// own, the selected row or the rave's floor, is left alone.
fn haze(buf: &mut Buffer, secs: f32, truecolor: bool) {
    let area = buf.area;
    for y in area.top()..area.bottom() {
        // One colour a row, so a 256-colour terminal is searched once a row
        // rather than once a cell.
        let hue = (secs * 20.0 + f32::from(y) * 6.0).rem_euclid(360.0);
        let tint = snap(hsv(hue, 0.55, 0.14), truecolor);
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            if cell.bg == Color::Reset && cell.diff_option == CellDiffOption::None {
                cell.bg = tint;
            }
        }
    }
}

/// The way home, across the middle of the footer.
fn sign(buf: &mut Buffer, footer: Rect, hiccup: bool, colour: Option<bool>) {
    let label = match hiccup {
        true => " ~  *hic*  ~ ",
        false => " ~  ↓↓↑↑→←→←ab to sober up  ~ ",
    };
    let width = label.width() as u16;
    if footer.height == 0 || footer.width <= width {
        return;
    }
    let style = match colour {
        Some(truecolor) => Style::default()
            .fg(Color::Indexed(16))
            .bg(snap((230, 170, 60), truecolor)),
        None => Style::default().add_modifier(Modifier::REVERSED),
    };
    let x = footer.x + (footer.width - width) / 2;
    buf.set_string(x, footer.y, label, style.add_modifier(Modifier::BOLD));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_code(drunk: &mut Drunk) -> Vec<rave::Heard> {
        CODE.iter().map(|&key| drunk.hear(key)).collect()
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (buf.area.left()..buf.area.right())
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    }

    /// The code toggles the night both ways, and keeps only its letters.
    #[test]
    fn the_code_starts_the_night_and_ends_it() {
        let mut drunk = Drunk::default();
        let heard = type_code(&mut drunk);
        assert!(
            heard[..8].iter().all(|h| *h == rave::Heard::Pass),
            "{heard:?}"
        );
        assert_eq!(heard[8], rave::Heard::Swallow);
        assert_eq!(heard[9], rave::Heard::Toggle);
        assert!(drunk.elapsed().is_some());
        type_code(&mut drunk);
        assert!(drunk.elapsed().is_none());
    }

    /// Extra downs at the start are forgiven, and a slip starts over.
    #[test]
    fn a_third_down_is_forgiven_and_a_slip_is_not() {
        let mut drunk = Drunk::default();
        for _ in 0..4 {
            drunk.hear(KeyCode::Down);
        }
        assert_eq!(drunk.heard, 2);
        drunk.hear(KeyCode::Char('x'));
        assert_eq!(drunk.heard, 0);
    }

    /// The Konami code and this one are both heard from the same keyboard
    /// without either getting in the other's way.
    #[test]
    fn both_codes_are_heard_side_by_side() {
        let mut app = crate::ui::tests::test_app();
        for code in CODE {
            app.on_key(crate::ui::tests::key(code));
        }
        assert!(app.reeling() && !app.raving());
        for code in rave::CODE {
            app.on_key(crate::ui::tests::key(code));
        }
        assert!(app.reeling() && app.raving());
    }

    /// A row sways as a whole: its text is all still there and in order,
    /// only somewhere else, and a row with a link in it does not move.
    #[test]
    fn a_row_sways_without_losing_a_letter() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 2));
        buf.set_string(10, 0, "$12.34 spent", Style::default());
        buf.set_string(10, 1, "linked", Style::default());
        buf[(10, 1)].set_diff_option(CellDiffOption::Skip);
        for dx in [-3, -1, 2, 3] {
            let mut moved = buf.clone();
            sway(&mut moved, 0, dx);
            sway(&mut moved, 1, dx);
            let text = row(&moved, 0);
            let at = text.find("$12.34 spent").expect(&text);
            assert_eq!(at as i32, 10 + dx);
            assert_eq!(row(&moved, 1), row(&buf, 1));
        }
    }

    /// Double vision writes only into blanks, never over a letter.
    #[test]
    fn the_echo_lands_only_on_blanks() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
        buf.set_string(0, 0, "ab cd", Style::default());
        ghost(&mut buf, 2, None);
        assert_eq!(row(&buf, 0), "ab cdcd     ");
        assert!(buf[(5, 0)].modifier.contains(Modifier::DIM));
        assert!(!buf[(3, 0)].modifier.contains(Modifier::DIM));
    }

    /// Every frame over a minute keeps the text of a full-width row, and a
    /// 256-colour terminal is never sent RGB.
    #[test]
    fn a_night_out_keeps_the_numbers_and_the_palette() {
        let area = Rect::new(0, 0, 40, 4);
        let footer = Rect::new(0, 3, 40, 1);
        for tenth in 0..600 {
            let mut buf = Buffer::empty(area);
            buf.set_string(8, 0, "$12.34 spent", Style::default());
            let t = Duration::from_millis(tenth * 100);
            paint(&mut buf, footer, t, Some(false), true);
            assert!(
                row(&buf, 0).contains("$12.34 spent"),
                "{t:?}: {}",
                row(&buf, 0)
            );
            for cell in buf.content() {
                assert!(
                    !matches!(cell.fg, Color::Rgb(..)) && !matches!(cell.bg, Color::Rgb(..)),
                    "{cell:?}"
                );
            }
        }
    }

    /// The first hiccup waits a while, then they come on a schedule.
    #[test]
    fn hiccups_come_later_and_briefly() {
        assert!(!hiccuping(0.05));
        assert!(hiccuping(HICCUP + 0.05));
        assert!(!hiccuping(HICCUP + JOLT + 0.01));
        assert!(hiccuping(2.0 * HICCUP));
    }
}
