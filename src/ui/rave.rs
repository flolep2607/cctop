//! ↑ ↑ ↓ ↓ ← → ← → b a, anywhere. The same again to go home.
//!
//! Heard in every mode and on every tab, ahead of everything else that reads
//! the keyboard: the arrows are what move you between panels and tabs, so a
//! code only listened for in one place is carried out of it by its own keys.
//!
//! An easter egg, and so built to cost nothing when it is not running: one
//! counter advanced per key, and a paint pass that returns before touching the
//! buffer. While it runs, the screen is repainted after it has been drawn —
//! colours only, never a symbol, so every number and every agent's output
//! still reads and every click still lands — plus an equaliser where the
//! footer was.
//!
//! Like the rest of [`super::effects`], nothing is kept between frames: every
//! frame is a pure function of how long the party has been going, so the same
//! instant always draws the same floor.
//!
//! The beat is a soft thump rather than a strobe. At 128 BPM it lands a little
//! over twice a second, under the three-a-second line past which flashing is a
//! photosensitivity hazard, and its peak lifts the ground to at most a third of
//! full brightness rather than flashing it white.

use super::effects::nearest_indexed;
use super::*;
use crossterm::event::KeyCode;
use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::Rect;
use ratatui::style::Color;

/// The code, as the keys it is typed with.
const CODE: [KeyCode; 10] = [
    KeyCode::Up,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Down,
    KeyCode::Left,
    KeyCode::Right,
    KeyCode::Left,
    KeyCode::Right,
    KeyCode::Char('b'),
    KeyCode::Char('a'),
];

/// House tempo.
const BPM: f32 = 128.0;

/// How far along the code the keyboard is, and when the party started.
#[derive(Debug, Default)]
pub struct Rave {
    heard: usize,
    since: Option<Instant>,
}

/// What a key was to the code.
#[derive(Debug, PartialEq, Eq)]
pub enum Heard {
    /// Nothing to do with it: the key does what it always does.
    Pass,
    /// Part of the code and not harmless alone — `b` jumps to a bell, and in a
    /// pane both letters would be typed at the agent — so kept from whatever
    /// else would have taken it. The arrows pass through: they only move.
    Swallow,
    /// The code is complete: the party starts, or ends.
    Toggle,
}

impl Rave {
    /// Take one key, returning what it is to the code.
    pub fn hear(&mut self, code: KeyCode) -> Heard {
        if code != CODE[self.heard] {
            // The only overlap the code has with itself is its opening run of
            // ups: a third ↑ after two is still two ups in, not one.
            self.heard = match code {
                KeyCode::Up if self.heard == 2 => 2,
                KeyCode::Up => 1,
                _ => 0,
            };
            return Heard::Pass;
        }
        self.heard += 1;
        match self.heard {
            n if n == CODE.len() => {
                self.heard = 0;
                self.since = match self.since {
                    Some(_) => None,
                    None => Some(Instant::now()),
                };
                Heard::Toggle
            }
            n if n > 8 => Heard::Swallow,
            _ => Heard::Pass,
        }
    }

    /// How long the party has been going, while it is.
    pub fn elapsed(&self) -> Option<Duration> {
        self.since.map(|since| since.elapsed())
    }
}

impl App {
    /// Feed a key to the code, `true` when it was the code's to keep.
    pub(super) fn hear_rave(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let code = match key.modifiers.is_empty() {
            true => key.code,
            false => KeyCode::Null,
        };
        match self.rave.hear(code) {
            Heard::Pass => false,
            Heard::Swallow => true,
            Heard::Toggle => {
                match (self.rave.elapsed(), theme::no_color()) {
                    (Some(_), true) => {
                        self.rave = Rave::default();
                        self.set_status("No colour, no party");
                    }
                    (Some(_), false) => {
                        self.set_status("♫ Rave mode. The same code again to go home")
                    }
                    (None, _) => self.set_status("Lights up. Back to work"),
                }
                true
            }
        }
    }

    /// Whether the screen is dancing, and wants a frame every
    /// [`effects::FRAME`] to do it.
    pub fn raving(&self) -> bool {
        self.rave.since.is_some()
    }
}

/// Repaint `buf` as the party is `t` in, with the equaliser across `footer`.
pub fn paint(buf: &mut Buffer, footer: Rect, t: Duration, truecolor: bool) {
    let secs = t.as_secs_f32();
    let beats = secs * BPM / 60.0;
    let beat = beats.floor();
    // A sharp attack at each beat, decaying well before the next.
    let thump = (-(beats - beat) * 5.0).exp();
    // The ground takes a new colour every beat, a fifth of the wheel on so
    // that consecutive beats never sit beside each other.
    let ground_hue = (beat * 72.0) % 360.0;
    let ground = snap(hsv(ground_hue, 0.9, 0.08 + 0.24 * thump), truecolor);

    let area = buf.area;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let Some(cell) = buf.cell_mut((x, y)) else {
                continue;
            };
            // Behind an image or a link's label, which is drawn from another
            // cell and would not show a colour set here anyway.
            if cell.diff_option == CellDiffOption::Skip {
                continue;
            }
            // A diagonal rainbow rolling across the screen, half a turn a
            // second: two columns to a row, because a cell is twice as tall as
            // it is wide and a 45° wave should look like one.
            let hue = (f32::from(x) * 6.0 + f32::from(y) * 12.0 - secs * 180.0).rem_euclid(360.0);
            cell.fg = snap(hsv(hue, 0.85, 1.0), truecolor);
            cell.bg = ground;
        }
    }
    equaliser(buf, footer.intersection(area), secs, thump, truecolor);
}

/// A bar per column, bouncing to the beat, with the way out across the middle.
fn equaliser(buf: &mut Buffer, footer: Rect, secs: f32, thump: f32, truecolor: bool) {
    const BARS: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    let y = footer.y;
    for x in footer.left()..footer.right() {
        let fx = f32::from(x);
        // Three sines at unrelated rates, so the bars never fall into a
        // pattern the eye can follow, all lifted together by the kick.
        let level = 0.5
            + 0.2 * (fx * 0.9 + secs * 7.0).sin()
            + 0.15 * (fx * 0.37 - secs * 3.1).sin()
            + 0.15 * (fx * 1.7 + secs * 11.3).sin();
        let level = (level * (0.55 + 0.45 * thump)).clamp(0.0, 0.999);
        let hue = (fx * 8.0 + secs * 90.0).rem_euclid(360.0);
        if let Some(cell) = buf.cell_mut((x, y)) {
            // A link in the footer was one wide cell standing for its whole
            // label; a bar is one cell standing for itself.
            cell.set_diff_option(CellDiffOption::None);
            cell.set_symbol(BARS[(level * BARS.len() as f32) as usize]);
            cell.fg = snap(hsv(hue, 1.0, 1.0), truecolor);
            cell.bg = Color::Indexed(16);
        }
    }
    let label = " ♫  ↑↑↓↓←→←→ba to go home  ♫ ";
    let width = label.chars().count() as u16;
    if footer.width > width {
        let x = footer.x + (footer.width - width) / 2;
        let style = ratatui::style::Style::default()
            .fg(Color::Indexed(16))
            .bg(snap(
                hsv((secs * 90.0).rem_euclid(360.0), 0.7, 1.0),
                truecolor,
            ))
            .add_modifier(ratatui::style::Modifier::BOLD);
        buf.set_string(x, y, label, style);
    }
}

/// `rgb` as this terminal can show it.
fn snap(rgb: (u8, u8, u8), truecolor: bool) -> Color {
    match truecolor {
        true => Color::Rgb(rgb.0, rgb.1, rgb.2),
        false => Color::Indexed(nearest_indexed(rgb)),
    }
}

/// Hue in degrees, saturation and value in `0..=1`, to RGB.
fn hsv(hue: f32, saturation: f32, value: f32) -> (u8, u8, u8) {
    let c = value * saturation;
    let h = hue.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u8 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = value - c;
    let byte = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (byte(r), byte(g), byte(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_code(rave: &mut Rave) -> Vec<Heard> {
        CODE.iter().map(|&key| rave.hear(key)).collect()
    }

    /// The code toggles the party both ways, and only its `b` and `a` are kept
    /// from the table: the arrows still move the cursor.
    #[test]
    fn the_code_starts_the_party_and_ends_it() {
        let mut rave = Rave::default();
        let heard = type_code(&mut rave);
        assert!(heard[..8].iter().all(|h| *h == Heard::Pass), "{heard:?}");
        assert_eq!(heard[8], Heard::Swallow);
        assert_eq!(heard[9], Heard::Toggle);
        assert!(rave.elapsed().is_some());
        type_code(&mut rave);
        assert!(rave.elapsed().is_none());
    }

    /// A slip starts the code over, and extra ups at the start are forgiven.
    #[test]
    fn a_wrong_key_starts_over_but_a_third_up_does_not() {
        let mut rave = Rave::default();
        rave.hear(KeyCode::Up);
        rave.hear(KeyCode::Up);
        rave.hear(KeyCode::Down);
        assert_eq!(rave.hear(KeyCode::Char('x')), Heard::Pass);
        assert_eq!(rave.heard, 0);
        // A plain `b` or `a` outside the code is the table's.
        assert_eq!(rave.hear(KeyCode::Char('b')), Heard::Pass);
        for _ in 0..5 {
            rave.hear(KeyCode::Up);
        }
        assert_eq!(rave.heard, 2);
        let rest: Vec<_> = CODE[2..].iter().map(|&key| rave.hear(key)).collect();
        assert_eq!(rest.last(), Some(&Heard::Toggle));
    }

    /// Started over a modal the code is still heard, even though its own ←
    /// closes help on the way: a key that moves you elsewhere halfway through
    /// does not lose the rest of the code.
    #[test]
    fn the_code_is_heard_away_from_the_table() {
        let mut app = crate::ui::tests::test_app();
        app.mode = Mode::Help;
        for code in CODE {
            app.on_key(crate::ui::tests::key(code));
        }
        assert!(app.raving());
    }

    #[test]
    fn hsv_lands_on_the_primaries() {
        assert_eq!(hsv(0.0, 1.0, 1.0), (255, 0, 0));
        assert_eq!(hsv(120.0, 1.0, 1.0), (0, 255, 0));
        assert_eq!(hsv(240.0, 1.0, 1.0), (0, 0, 255));
        assert_eq!(hsv(360.0, 1.0, 1.0), (255, 0, 0));
        assert_eq!(hsv(42.0, 0.0, 0.0), (0, 0, 0));
    }

    /// Colours move, symbols do not — except the footer, which is the
    /// equaliser — and a 256-colour terminal is never sent RGB.
    #[test]
    fn the_party_repaints_colours_and_keeps_the_text() {
        let area = Rect::new(0, 0, 40, 3);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "$12.34 spent", ratatui::style::Style::default());
        let footer = Rect::new(0, 2, 40, 1);
        paint(&mut buf, footer, Duration::from_millis(1234), false);
        let text: String = (0..12).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert_eq!(text, "$12.34 spent");
        for cell in buf.content() {
            assert!(
                !matches!(cell.fg, Color::Rgb(..)) && !matches!(cell.bg, Color::Rgb(..)),
                "{cell:?}"
            );
        }
        assert!(buf.content()[80..].iter().any(|c| c.symbol() != " "));
    }
}
