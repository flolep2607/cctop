//! → → ← ← ↑ ↓ ↑ ↓ h i, anywhere. The same again to come down.
//!
//! The third easter egg, after [`super::rave`] and [`super::drunk`], and built
//! the same way: one counter per key, nothing painted while it is off, and
//! every frame a pure function of how long the trip has been going.
//!
//! Where the rave is a beat, this is a slow melt, the kind of visuals people
//! put on to watch while high. Behind everything, a plasma of soft colour
//! breathes and turns, folded now and then into a spiral that winds in toward
//! the middle of the screen. Over it, on empty ground only, shapes drift
//! upward and sway as they go. The text keeps its own colours and its place,
//! so everything still reads and every click still lands. Only the ground
//! under it moves.
//!
//! Nothing here is fast. The quickest thing on screen takes several seconds to
//! change, with no flashes and no beat. It is meant to be stared at.

use super::effects::nearest_indexed;
use super::rave::hsv;
use super::*;
use crossterm::event::KeyCode;
use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use std::f32::consts::TAU;
use unicode_width::UnicodeWidthStr;

/// The code. Its last two keys spell what it is.
const CODE: [KeyCode; 10] = [
    KeyCode::Right,
    KeyCode::Right,
    KeyCode::Left,
    KeyCode::Left,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Char('h'),
    KeyCode::Char('i'),
];

/// Seconds for the ground to fold from plasma into the spiral and back.
const FOLD: f32 = 40.0;

/// How many shapes are drifting at once, at most.
const FLOATERS: u32 = 24;

/// What drifts.
const SHAPES: [&str; 8] = ["✿", "❀", "◌", "∞", "☮", "✺", "◯", "☁"];

/// How far along the code the keyboard is, and when the trip started.
#[derive(Debug, Default)]
pub struct High {
    heard: usize,
    since: Option<Instant>,
}

impl High {
    /// Take one key, returning what it is to the code, on the same contract
    /// as [`super::rave::Rave::hear`]. The arrows pass. `h` is kept, since it
    /// would open the hooks, and `i` completes the code.
    pub fn hear(&mut self, code: KeyCode) -> rave::Heard {
        if code != CODE[self.heard] {
            // The only overlap is the opening run: a third → after two is
            // still two rights in.
            self.heard = match code {
                KeyCode::Right if self.heard == 2 => 2,
                KeyCode::Right => 1,
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

    /// How long the trip has been going, while it is.
    pub fn elapsed(&self) -> Option<Duration> {
        self.since.map(|since| since.elapsed())
    }
}

impl App {
    /// Feed a key to the code, `true` when it was the code's to keep.
    pub(super) fn hear_high(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let code = match key.modifiers.is_empty() {
            true => key.code,
            false => KeyCode::Null,
        };
        match self.high.hear(code) {
            rave::Heard::Pass => false,
            rave::Heard::Swallow => true,
            rave::Heard::Toggle => {
                match (self.high.since, theme::no_color()) {
                    // The whole trip is colour, so without it there is none.
                    (Some(_), true) => {
                        self.high = High::default();
                        self.set_status("No colour, no trip");
                    }
                    (Some(_), false) => self
                        .set_status("☮ High mode. Stare at it. The same code again to come down"),
                    (None, _) => self.set_status("Back down. Drink some water"),
                }
                true
            }
        }
    }

    /// Whether the ground is melting, and wants a frame every
    /// [`effects::FRAME`] to do it.
    pub fn tripping(&self) -> bool {
        self.high.since.is_some()
    }
}

/// Repaint `buf` as the trip is `t` in, with the way down across `footer`
/// when `label`, which is when nothing louder is using the footer.
pub fn paint(buf: &mut Buffer, footer: Rect, t: Duration, truecolor: bool, label: bool) {
    let secs = t.as_secs_f32();
    // The first few seconds fade the ground in rather than dropping it on the
    // screen all at once.
    let fade = (secs / 3.0).min(1.0);
    let area = buf.area;
    let cx = f32::from(area.x) + f32::from(area.width) / 2.0;
    let cy = f32::from(area.y) + f32::from(area.height) / 2.0;
    // How much of the spiral is showing. It stays at zero for a stretch, eases
    // in, holds and eases out, so it arrives as a change rather than a
    // constant.
    let fold = (((secs / FOLD) * TAU - TAU / 4.0).sin() * 1.4).clamp(0.0, 1.0);

    // Snapped once per distinct colour, as the rave does. The hue and the
    // value are also stepped before they reach the palette, so a 256-colour
    // frame asks for a few hundred colours rather than one per cell.
    // Dim enough that text in any colour still stands off it. The 256-colour
    // palette has no dark colours, only dark greys: its cube starts at 95 of
    // 255, so anything much under a third of full value snaps to grey and
    // the trip is lost. There the ground sits higher, where the cube is.
    let floor = match truecolor {
        true => 0.12,
        false => 0.33,
    };
    let mut palette: std::collections::HashMap<(u16, u8), Color> = std::collections::HashMap::new();
    let mut ground = |hue: f32, value: f32| {
        let hue_step = (hue.rem_euclid(360.0) / 5.0) as u16;
        let value_step = (value.clamp(0.0, 1.0) * 40.0) as u8;
        *palette.entry((hue_step, value_step)).or_insert_with(|| {
            let rgb = hsv(
                f32::from(hue_step) * 5.0,
                0.65,
                f32::from(value_step) / 40.0,
            );
            match truecolor {
                true => Color::Rgb(rgb.0, rgb.1, rgb.2),
                false => Color::Indexed(nearest_indexed(rgb)),
            }
        })
    };

    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let Some(cell) = buf.cell_mut((x, y)) else {
                continue;
            };
            // Behind an image or a link's label, which would not show it.
            if cell.diff_option == CellDiffOption::Skip {
                continue;
            }
            // Columns halved, because a cell is twice as tall as it is wide
            // and a circle should look like one.
            let dx = (f32::from(x) - cx) / 2.0;
            let dy = f32::from(y) - cy;
            let (hue, glow) = pattern(dx, dy, secs, fold);
            let value = fade * (floor + 0.2 * glow);
            cell.bg = ground(hue, value);
        }
    }

    floaters(buf, secs, truecolor);
    if label {
        sign(buf, footer.intersection(area), secs, truecolor);
    }
}

/// The ground at a point: a hue in degrees and a glow in `0..=1`.
///
/// A plasma, which is four slow sines through the screen at different angles
/// and speeds, summed so that they interfere into blobs that swell, merge and
/// split. `fold` blends in a spiral that winds toward the middle.
fn pattern(dx: f32, dy: f32, secs: f32, fold: f32) -> (f32, f32) {
    let r = dx.hypot(dy);
    let plasma = (dx * 0.11 + secs * 0.31).sin()
        + (dy * 0.19 - secs * 0.23).sin()
        + ((dx + dy) * 0.07 + secs * 0.17).sin()
        + (r * 0.16 - secs * 0.41).sin();
    // `plasma` is in -4..4; the hue it gives drifts round the wheel on its own
    // clock too, so no colour settles in one place for long.
    let plasma_hue = plasma * 45.0 + secs * 6.0;
    let plasma_glow = (plasma / 4.0 + 1.0) / 2.0;

    let angle = dy.atan2(dx);
    // Three arms, twisting with distance, turning slowly.
    let arms = (angle * 3.0 + r * 0.35 - secs * 0.6).sin();
    let spiral_hue = angle.to_degrees() + r * 4.0 - secs * 12.0;
    let spiral_glow = (arms + 1.0) / 2.0;

    // Hues blended the short way round the wheel.
    let delta = (spiral_hue - plasma_hue + 180.0).rem_euclid(360.0) - 180.0;
    let hue = plasma_hue + delta * fold;
    let glow = plasma_glow + (spiral_glow - plasma_glow) * fold;
    (hue, glow)
}

/// Shapes drifting up the screen, on empty ground only.
///
/// Each one is a function of its index and the clock, not a particle kept
/// between frames: it rises at its own speed, sways as it rises, wraps from
/// the top back to the bottom, and is only drawn where it lands on a blank
/// cell with blanks either side, so it never sits among text.
fn floaters(buf: &mut Buffer, secs: f32, truecolor: bool) {
    let area = buf.area;
    if area.width < 3 || area.height == 0 {
        return;
    }
    let blank = |buf: &Buffer, x: u16, y: u16| {
        buf.cell((x, y))
            .is_some_and(|c| c.symbol() == " " && c.diff_option == CellDiffOption::None)
    };
    let (w, h) = (f32::from(area.width), f32::from(area.height));
    for k in 0..FLOATERS {
        let seed = scatter(k);
        let unit = |shift: u32| ((seed >> shift) & 0xFF) as f32 / 255.0;
        // A row every two to five seconds.
        let speed = 0.2 + 0.3 * unit(0);
        let rise = (unit(8) * h - secs * speed).rem_euclid(h);
        let sway = (secs * (0.2 + 0.3 * unit(16)) + unit(24) * TAU).sin() * 3.0;
        let x = area.x + ((unit(4) * w + sway).rem_euclid(w)) as u16;
        let y = area.y + rise as u16;
        let (Some(left), Some(right)) = (x.checked_sub(1), x.checked_add(1)) else {
            continue;
        };
        if !(blank(buf, left, y) && blank(buf, x, y) && blank(buf, right, y)) {
            continue;
        }
        let shape = SHAPES[(seed % SHAPES.len() as u32) as usize];
        let hue = unit(12) * 360.0 + secs * 10.0;
        let rgb = hsv(hue, 0.35, 0.95);
        let fg = match truecolor {
            true => Color::Rgb(rgb.0, rgb.1, rgb.2),
            false => Color::Indexed(nearest_indexed(rgb)),
        };
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_symbol(shape);
            cell.fg = fg;
        }
    }
}

/// A well-mixed number from a floater's index, so each has its own speed,
/// column and shape without any of them being chosen by hand.
fn scatter(k: u32) -> u32 {
    let mut h = k.wrapping_add(1).wrapping_mul(0x9E37_79B1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85EB_CA77);
    h ^= h >> 13;
    h
}

/// The way down, across the middle of the footer, its colour turning slowly.
fn sign(buf: &mut Buffer, footer: Rect, secs: f32, truecolor: bool) {
    let label = " ☮  →→←←↑↓↑↓hi to come down  ☮ ";
    let width = label.width() as u16;
    if footer.height == 0 || footer.width <= width {
        return;
    }
    let rgb = hsv(secs * 12.0, 0.45, 0.95);
    let bg = match truecolor {
        true => Color::Rgb(rgb.0, rgb.1, rgb.2),
        false => Color::Indexed(nearest_indexed(rgb)),
    };
    let style = Style::default()
        .fg(Color::Indexed(16))
        .bg(bg)
        .add_modifier(Modifier::BOLD);
    let x = footer.x + (footer.width - width) / 2;
    buf.set_string(x, footer.y, label, style);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_code(high: &mut High) -> Vec<rave::Heard> {
        CODE.iter().map(|&key| high.hear(key)).collect()
    }

    /// The code toggles the trip both ways and keeps only its letters.
    #[test]
    fn the_code_starts_the_trip_and_ends_it() {
        let mut high = High::default();
        let heard = type_code(&mut high);
        assert!(
            heard[..8].iter().all(|h| *h == rave::Heard::Pass),
            "{heard:?}"
        );
        assert_eq!(heard[8], rave::Heard::Swallow);
        assert_eq!(heard[9], rave::Heard::Toggle);
        assert!(high.elapsed().is_some());
        type_code(&mut high);
        assert!(high.elapsed().is_none());
    }

    /// Extra rights at the start are forgiven.
    #[test]
    fn a_third_right_is_forgiven() {
        let mut high = High::default();
        for _ in 0..5 {
            high.hear(KeyCode::Right);
        }
        assert_eq!(high.heard, 2);
    }

    /// All three codes share one keyboard, and each is heard through the
    /// others.
    #[test]
    fn all_three_codes_are_heard_side_by_side() {
        let mut app = crate::ui::tests::test_app();
        for code in CODE.into_iter().chain(rave::CODE).chain(drunk::CODE) {
            app.on_key(crate::ui::tests::key(code));
        }
        let colour = !theme::no_color();
        assert_eq!(app.tripping(), colour);
        assert_eq!(app.raving(), colour);
        assert!(app.reeling());
    }

    /// The ground moves under the text and never over it: every letter is
    /// where it was, a floater lands only on open ground, and a 256-colour
    /// terminal is never sent RGB.
    #[test]
    fn the_ground_melts_and_the_text_stays() {
        let area = Rect::new(0, 0, 60, 12);
        let footer = Rect::new(0, 11, 60, 1);
        for second in 0..90 {
            let mut buf = Buffer::empty(area);
            for y in 0..11 {
                buf.set_string(0, y, "$12.34 spent", Style::default());
            }
            paint(&mut buf, footer, Duration::from_secs(second), false, true);
            for y in 0..11 {
                let text: String = (0..13).map(|x| buf[(x, y)].symbol().to_string()).collect();
                assert_eq!(text, "$12.34 spent ", "second {second}, row {y}");
            }
            for cell in buf.content() {
                assert!(
                    !matches!(cell.fg, Color::Rgb(..)) && !matches!(cell.bg, Color::Rgb(..)),
                    "{cell:?}"
                );
            }
        }
    }

    /// The spiral comes and goes: some of every fold is pure plasma.
    #[test]
    fn the_spiral_folds_in_and_out() {
        let fold = |secs: f32| (((secs / FOLD) * TAU - TAU / 4.0).sin() * 1.4).clamp(0.0, 1.0);
        assert_eq!(fold(0.0), 0.0);
        assert_eq!(fold(FOLD / 2.0), 1.0);
        assert_eq!(fold(FOLD), 0.0);
    }
}
