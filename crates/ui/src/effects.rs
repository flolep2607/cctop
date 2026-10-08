//! Motion in the tab bar: the pulse on a tab whose agent needs you, and the
//! sweep that confirms a restart.
//!
//! Both are drawn with [`tachyonfx`] — its easing curves, its colour
//! interpolation and, for the restart, one of its effects — but neither keeps
//! any state between frames. The pulse is a pure function of the clock, and the
//! restart sweep is rebuilt every frame and run forward to how long ago the
//! restart was. The obvious design was the other one, an effect manager holding
//! live effects across frames, and it fits this bar badly: a tab's cells move
//! whenever a title changes or a tab opens or closes, and the tabs themselves
//! are rebuilt by the rmux sweep, so an effect bound to where a tab *was* would
//! paint the wrong one. Asked afresh from `(tab, elapsed)`, every frame is
//! right by construction, the same instant always draws the same bar, and a
//! test can pin a phase instead of sleeping to reach it.
//!
//! cctop's colours are 256-colour indices and tachyonfx interpolates between
//! RGB values, so everything here converts through the xterm-256 palette — and only the part
//! of it with a fixed value. The cube (16–231) and the grey ramp (232–255) are
//! the same on every terminal; 0–15 are whatever the user's theme says, so a
//! colour there is never interpolated, and a tab that would need one falls back
//! to the hard blink. What an interpolation produces is written as RGB only on
//! a terminal that says it takes truecolor, and snapped back to the nearest
//! index everywhere else.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use std::time::Duration;
use tachyonfx::{ColorSpace, Interpolation, Motion, fx};

/// One full breath of the needs-you pulse, rest to alert and back. The same
/// period as the blink it replaces, which was tuned to be slow enough to read
/// the title through and fast enough to catch the eye.
pub const PULSE_PERIOD: Duration = Duration::from_millis(1200);

/// How long the restart sweep runs. Long enough to be seen from the pane you
/// pressed the key in, short enough to be over before you look for it again.
pub const FLASH: Duration = Duration::from_millis(450);

/// The frame interval while anything here is moving. Thirty a second is past
/// where a colour change reads as motion rather than steps; more would be
/// frames nobody sees, drawn for a bar that is one row of the screen.
pub const FRAME: Duration = Duration::from_millis(33);

/// The same, for a pulse seen from the dashboard.
///
/// There a redraw is the whole table, not a pane's worth of cells, and a tab
/// left waiting on you can wait for an hour — thirty full redraws a second for
/// that long is a fan spinning for one row of the screen. Ten a second still
/// reads as a pulse over its 1.2s breath. A restart sweep is short enough to
/// keep [`FRAME`] wherever it runs.
pub const DASHBOARD_FRAME: Duration = Duration::from_millis(100);

/// How often the moving bar is sampled from where the view is now.
pub fn frame_for(tab: usize) -> Duration {
    match tab {
        0 => DASHBOARD_FRAME,
        _ => FRAME,
    }
}

/// How far through the pulse the bar is at `elapsed`: `0.0` is the tab's
/// resting look, `1.0` its alert tone.
///
/// A triangle wave eased with a sine at both ends, so the swing slows into each
/// extreme and rushes through the middle — which is also where the ink swaps,
/// and a label spends the least time on the colour it reads worst against.
pub fn pulse_level(elapsed: Duration) -> f32 {
    let period = PULSE_PERIOD.as_millis();
    let t = (elapsed.as_millis() % period) as f32 / period as f32;
    let triangle = 1.0 - (2.0 * t - 1.0).abs();
    Interpolation::SineInOut.alpha(triangle)
}

/// The RGB a colour is on every terminal, or `None` for one whose value is the
/// user's theme's to decide — the sixteen named colours and their indices, and
/// `Reset`, which is whatever the terminal's own ground or ink is.
///
/// tachyonfx has a conversion of its own, but it answers for those too, with
/// xterm's defaults; a pulse that eased toward xterm's idea of "white" on a
/// terminal themed otherwise would land on a colour the tab never wears.
pub fn to_rgb(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(i @ 16..=231) => {
            let i = i - 16;
            Some((level(i / 36), level((i / 6) % 6), level(i % 6)))
        }
        Color::Indexed(i @ 232..=255) => {
            let grey = 8 + (i - 232) * 10;
            Some((grey, grey, grey))
        }
        _ => None,
    }
}

/// A cube coordinate's channel value, per xterm's 256-colour table.
fn level(step: u8) -> u8 {
    match step {
        0 => 0,
        n => 55 + 40 * n,
    }
}

/// The index, cube or grey ramp, nearest `rgb` — the closest a 256-colour
/// terminal can come.
///
/// Nearest by squared distance over both candidates rather than by
/// thresholding each channel, which is what tachyonfx's own quantiser does: a
/// channel threshold picks a cube cell for a colour a grey step sits closer to,
/// and a pulse snapped that way lurches sideways between hues on its way.
pub fn nearest_indexed((r, g, b): (u8, u8, u8)) -> u8 {
    let distance = |(x, y, z): (u8, u8, u8)| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(x, r) + d(y, g) + d(z, b)
    };
    let step = |v: u8| (0..6u8).min_by_key(|&n| (i32::from(level(n)) - i32::from(v)).abs());
    let (sr, sg, sb) = (
        step(r).unwrap_or(0),
        step(g).unwrap_or(0),
        step(b).unwrap_or(0),
    );
    let cube = 16 + 36 * sr + 6 * sg + sb;
    let grey = (0..24u8)
        .min_by_key(|&n| distance((8 + n * 10, 8 + n * 10, 8 + n * 10)))
        .map_or(232, |n| 232 + n);
    let rgb = |i: u8| to_rgb(Color::Indexed(i)).unwrap_or((0, 0, 0));
    match distance(rgb(grey)) < distance(rgb(cube)) {
        true => grey,
        false => cube,
    }
}

/// `from` eased `level` of the way to `to`, in the colours this terminal can
/// show; `None` when either end has no fixed value to ease from.
///
/// In RGB rather than tachyonfx's default HSL: the two ends of a tab's pulse
/// are one hue at two strengths, and HSL, going the short way round the wheel
/// between a dusty pastel and its saturated self, can drift through a
/// neighbouring hue that neither end is.
pub fn mix(from: Color, to: Color, level: f32, truecolor: bool) -> Option<Color> {
    let ((r0, g0, b0), (r1, g1, b1)) = (to_rgb(from)?, to_rgb(to)?);
    let mixed = ColorSpace::Rgb.lerp(
        &Color::Rgb(r0, g0, b0),
        &Color::Rgb(r1, g1, b1),
        level.clamp(0.0, 1.0),
    );
    Some(match (truecolor, mixed) {
        (true, rgb) => rgb,
        (false, Color::Rgb(r, g, b)) => Color::Indexed(nearest_indexed((r, g, b))),
        (false, other) => other,
    })
}

/// The needs-you look at `level` of the way from `rest` to `lit`, or `None`
/// where the two cannot be eased between and the caller should blink instead.
///
/// Only the ground moves. The ink is one end's or the other's, swapped at the
/// midpoint: ink that eased too would pass through a colour matched to neither
/// ground, and a label is least legible exactly when its ink and its ground are
/// both halfway. Near either end the style *is* that end, so the resting tab is
/// exactly its resting self between pulses — not a snapped approximation of
/// it — and the peak is exactly the alert tone the blink used to show.
///
/// `ground` stands in for a `rest` with no background of its own, which is an
/// unpainted tab sitting on the terminal's ground.
pub fn pulse(rest: Style, lit: Style, ground: Color, level: f32, truecolor: bool) -> Option<Style> {
    let from = rest.bg.filter(|bg| *bg != Color::Reset).unwrap_or(ground);
    let to = lit.bg?;
    let mixed = mix(from, to, level, truecolor)?;
    Some(match level {
        l if l < 1.0 / 16.0 => rest,
        l if l > 15.0 / 16.0 => lit,
        l if l < 0.5 => rest.bg(mixed),
        _ => lit.bg(mixed),
    })
}

/// Sweep the tab at `area` in from `color`, as it is `since` into the restart
/// it confirms. A fresh effect each frame, run forward by `since` — see the
/// module docs for why nothing is kept.
///
/// `ground` is written under cells with no ground of their own before the
/// sweep reads them: tachyonfx eases a `Reset` cell from black, which on a
/// light terminal is a dark smear across a pale tab.
pub fn restart_sweep(
    buf: &mut Buffer,
    area: Rect,
    since: Duration,
    color: Color,
    ground: Color,
    truecolor: bool,
) {
    if since >= FLASH {
        return;
    }
    let area = area.intersection(buf.area);
    each(buf, area, |cell| {
        if cell.bg == Color::Reset {
            cell.bg = ground;
        }
    });
    // A gradient as wide as the tab, so the leading edge is a wash rather than
    // a line: over a label a dozen cells wide, a hard edge reads as the text
    // being redrawn, not as the tab being renewed.
    let mut sweep = fx::sweep_in(
        Motion::LeftToRight,
        area.width.max(4),
        0,
        color,
        (FLASH.as_millis() as u32, Interpolation::QuadOut),
    )
    .with_color_space(ColorSpace::Rgb);
    sweep.process(since.into(), buf, area);
    if !truecolor {
        each(buf, area, |cell| {
            if let Color::Rgb(r, g, b) = cell.fg {
                cell.fg = Color::Indexed(nearest_indexed((r, g, b)));
            }
            if let Color::Rgb(r, g, b) = cell.bg {
                cell.bg = Color::Indexed(nearest_indexed((r, g, b)));
            }
        });
    }
}

/// Apply `f` to every cell of `area`, which the caller has clipped to `buf`.
fn each(buf: &mut Buffer, area: Rect, mut f: impl FnMut(&mut ratatui::buffer::Cell)) {
    for position in area.positions() {
        if let Some(cell) = buf.cell_mut(position) {
            f(cell);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    /// The fixed part of the palette survives the trip both ways exactly; the
    /// cube's own greys may come back as the ramp's, which is the same colour
    /// or a closer one.
    #[test]
    fn a_fixed_index_round_trips_through_rgb() {
        for i in 16..=255u8 {
            let rgb = to_rgb(Color::Indexed(i)).expect("a fixed index has an RGB");
            let back = nearest_indexed(rgb);
            if back != i {
                let (r, g, b) = rgb;
                assert!(r == g && g == b, "{i} came back as {back}");
                let there = to_rgb(Color::Indexed(back)).unwrap();
                let d = |(x, y, z): (u8, u8, u8)| {
                    (i32::from(x) - i32::from(r)).abs()
                        + (i32::from(y) - i32::from(g)).abs()
                        + (i32::from(z) - i32::from(b)).abs()
                };
                assert!(d(there) <= d(rgb), "{i} came back as the farther {back}");
            }
        }
        assert_eq!(to_rgb(Color::Indexed(203)), Some((255, 95, 95)));
        assert_eq!(to_rgb(Color::Indexed(232)), Some((8, 8, 8)));
        assert_eq!(to_rgb(Color::Indexed(255)), Some((238, 238, 238)));
    }

    /// The sixteen the user's theme owns are not guessed at, and so never
    /// eased toward: the tab blinks instead.
    #[test]
    fn a_themed_colour_has_no_rgb() {
        for color in [Color::Reset, Color::Black, Color::White, Color::Indexed(7)] {
            assert_eq!(to_rgb(color), None, "{color:?}");
        }
        assert_eq!(mix(Color::White, Color::Indexed(203), 0.5, true), None);
        let lit = Style::default().bg(Color::Indexed(7));
        assert_eq!(
            pulse(Style::default(), lit, Color::Indexed(234), 0.5, true),
            None
        );
    }

    /// Rest, climb, peak and back: the pulse starts at the resting look, is at
    /// the alert tone half a period in, and is back where it began a period on.
    #[test]
    fn the_pulse_breathes_rest_to_alert_and_back() {
        let at = |ms: u64| pulse_level(Duration::from_millis(ms));
        assert!(at(0) < 0.01);
        assert!((at(300) - 0.5).abs() < 0.01, "{}", at(300));
        assert!(at(600) > 0.99);
        assert!((at(900) - 0.5).abs() < 0.01);
        assert!(at(1200) < 0.01, "a period on is not back at rest");
        // Slow at the ends, quick through the middle.
        assert!(at(100) - at(0) < at(350) - at(250));
    }

    /// A painted tab's pulse, at a few phases, on both kinds of terminal.
    #[test]
    fn a_painted_pulse_eases_its_ground_and_swaps_its_ink_halfway() {
        // Dark violet: rest 97 (#875faf) under light ink, alert 135 (#af5fff)
        // under dark.
        let rest = Style::default()
            .bg(Color::Indexed(97))
            .fg(Color::Indexed(254))
            .add_modifier(Modifier::BOLD);
        let lit = Style::default()
            .bg(Color::Indexed(135))
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD);
        let ground = Color::Indexed(234);

        assert_eq!(pulse(rest, lit, ground, 0.0, true), Some(rest));
        assert_eq!(pulse(rest, lit, ground, 1.0, true), Some(lit));

        let quarter = pulse(rest, lit, ground, 0.25, true).unwrap();
        assert_eq!(quarter.fg, rest.fg, "the ink swapped before halfway");
        let Some(Color::Rgb(r, g, b)) = quarter.bg else {
            panic!("truecolor was not used: {quarter:?}");
        };
        assert!((135..175).contains(&r) && g == 95 && (175..255).contains(&b));
        assert!(quarter.add_modifier.contains(Modifier::BOLD));

        let three = pulse(rest, lit, ground, 0.75, true).unwrap();
        assert_eq!(three.fg, lit.fg, "the ink had not swapped past halfway");

        // Without truecolor the same moment is the nearest index — somewhere
        // on the road between the two ends, not off it.
        let snapped = pulse(rest, lit, ground, 0.5, false).unwrap();
        let Some(Color::Indexed(i)) = snapped.bg else {
            panic!("an RGB colour reached a 256-colour terminal: {snapped:?}");
        };
        assert!(
            [97, 98, 134, 135].contains(&i),
            "the midpoint snapped to {i}"
        );
    }

    /// An unpainted tab has no ground of its own, so it eases from the one it
    /// sits on toward the amber it lights up in.
    #[test]
    fn an_unpainted_pulse_eases_from_the_ground() {
        let rest = Style::default().fg(Color::Indexed(221));
        let lit = Style::default().bg(Color::Indexed(221)).fg(Color::Black);
        let mid = pulse(rest, lit, Color::Indexed(234), 0.5, false).unwrap();
        let Some(Color::Indexed(i)) = mid.bg else {
            panic!("{mid:?}");
        };
        assert_ne!(i, 221, "the midpoint is already the full amber");
        assert_ne!(i, 234, "the midpoint is still the ground");
        assert_eq!(
            pulse(rest, lit, Color::Indexed(234), 0.0, false),
            Some(rest)
        );
    }

    /// The restart sweep starts as a wash of its colour and leaves the tab
    /// exactly as it was drawn, touching nothing outside it; a 256-colour
    /// terminal gets indices throughout.
    #[test]
    fn the_restart_sweep_washes_in_and_leaves_the_tab_as_drawn() {
        let area = Rect::new(0, 0, 20, 1);
        let tab = Rect::new(4, 0, 8, 1);
        let drawn = || {
            let mut buf = Buffer::empty(area);
            buf.set_string(4, 0, " 2:one  ", Style::default().bg(Color::Indexed(97)));
            buf
        };
        let wash = Color::Indexed(75);

        let mut start = drawn();
        restart_sweep(
            &mut start,
            tab,
            Duration::ZERO,
            wash,
            Color::Indexed(234),
            false,
        );
        assert_eq!(start.cell((4, 0)).unwrap().bg, wash);
        assert_eq!(start.cell((3, 0)).unwrap(), drawn().cell((3, 0)).unwrap());

        let mut mid = drawn();
        restart_sweep(&mut mid, tab, FLASH / 2, wash, Color::Indexed(234), false);
        for x in 4..12 {
            let cell = mid.cell((x, 0)).unwrap();
            assert!(
                !matches!(cell.bg, Color::Rgb(..)) && !matches!(cell.fg, Color::Rgb(..)),
                "column {x} carries RGB to a 256-colour terminal: {cell:?}"
            );
        }

        let mut done = drawn();
        restart_sweep(&mut done, tab, FLASH, wash, Color::Indexed(234), true);
        assert_eq!(done, drawn());
    }
}
