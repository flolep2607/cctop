//! ↑ ↑ ↓ ↓ ← → ← → b a, anywhere. The same again to go home.
//!
//! Or type `ultracode` into a Claude Code or Codex session cctop is watching:
//! the party starts itself, and goes home the same way. It is read from the
//! transcript, so it needs no hook, and only a prompt newer than the last one
//! heard — never one from before cctop started — starts it, so a session that
//! said it last week does not throw a party every time the dashboard opens.
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

mod sound;

use super::effects::nearest_indexed;
use super::*;
use crossterm::event::KeyCode;
use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::Rect;
use ratatui::style::Color;

/// The code, as the keys it is typed with.
pub(super) const CODE: [KeyCode; 10] = [
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
    /// The music, while there is a party and something to play it with.
    sound: Option<sound::Sound>,
    /// The newest `ultracode` prompt already answered, so one prompt starts
    /// one party: going home is not undone by the next refresh reading the
    /// same transcript again.
    ultracode_heard: Option<chrono::DateTime<chrono::Utc>>,
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
                match self.rave.since {
                    Some(_) => self.start_rave("↑↑↓↓←→←→ba"),
                    None => {
                        // Dropping it is what stops the player.
                        self.rave.sound = None;
                        self.set_status("Lights up. Back to work")
                    }
                }
                true
            }
        }
    }

    /// Start the music and say so, the lights having just come on for `why`.
    fn start_rave(&mut self, why: &str) {
        if theme::no_color() {
            self.rave.since = None;
            self.set_status("No colour, no party");
            return;
        }
        self.rave.since.get_or_insert_with(Instant::now);
        self.rave.sound = sound::Sound::start();
        self.set_status(format!(
            "♫ {why}: rave mode, with {} if you are online. ↑↑↓↓←→←→ba to go home",
            sound::STATION
        ));
    }

    /// Start the party when a Claude Code or Codex session has been asked for
    /// `ultracode` since the last time one was — see the module docs.
    ///
    /// Called whenever the rows change. A party already going is left alone:
    /// the word asks for the lights, it does not switch them off.
    pub(super) fn hear_ultracode(&mut self) {
        let parse = |ts: &str| {
            chrono::DateTime::parse_from_rfc3339(ts)
                .ok()
                .map(|t| t.with_timezone(&chrono::Utc))
        };
        let Some(newest) = self
            .sessions
            .iter()
            .filter(|s| matches!(s.provider, Provider::Claude | Provider::Codex))
            .filter_map(|s| s.ultracode_at.as_deref().and_then(parse))
            .max()
        else {
            return;
        };
        let since = self
            .rave
            .ultracode_heard
            .or_else(|| parse(&self.started_at));
        if since.is_some_and(|since| newest <= since) {
            return;
        }
        self.rave.ultracode_heard = Some(newest);
        if self.rave.since.is_none() {
            self.start_rave("ultracode");
        }
    }

    /// Whether the screen is dancing, and wants a frame every
    /// [`effects::FRAME`] to do it.
    pub fn raving(&self) -> bool {
        self.rave.since.is_some()
    }
}

/// Beats to a phrase: eight bars of four, the unit a DJ builds and drops on.
const PHRASE: f32 = 32.0;

/// The last this many beats of every phrase are the build.
const BUILD: f32 = 8.0;

/// How far into the build toward the next drop the party is at `beats`, `0.0`
/// outside one and rising to `1.0` on the beat before the drop.
///
/// The first phrase has no build: the party starts on a drop, which is what
/// typing the code is.
fn build(beats: f32) -> f32 {
    let into = beats.rem_euclid(PHRASE) - (PHRASE - BUILD);
    (into / BUILD).clamp(0.0, 1.0)
}

/// Whether the drop is still landing: the first two beats of every phrase but
/// the first, which never had a build to drop from.
fn dropping(beats: f32) -> bool {
    beats >= PHRASE && beats.rem_euclid(PHRASE) < 2.0
}

/// Repaint `buf` as the party is `t` in, with the equaliser across `footer`.
///
/// In layers, back to front: a ground that thumps on every beat and rises
/// through each build, a ring spreading out from the middle on the beat, two
/// pairs of lasers turning against each other, glints of a mirror ball on
/// empty ground, and over all of it the rainbow the text is written in. Every
/// layer moves the colours of a cell; only the glints write a symbol, and only
/// where there was nothing to read.
pub fn paint(buf: &mut Buffer, footer: Rect, t: Duration, truecolor: bool) {
    let secs = t.as_secs_f32();
    let beats = secs * BPM / 60.0;
    let beat = beats.floor();
    let phase = beats - beat;
    // A sharp attack at each beat, decaying well before the next.
    let thump = (-phase * 5.0).exp();
    let build = build(beats);
    // The ground takes a new colour every beat, a fifth of the wheel on so
    // that consecutive beats never sit beside each other; through a build it
    // climbs, so the drop is felt as the release of something.
    let ground_hue = (beat * 72.0) % 360.0;
    let ground_value = 0.08 + 0.2 * thump + 0.12 * build;
    // The rainbow rolls faster as the build climbs. Its extra travel is a
    // whole number of turns by the drop, so where it resets the colours are
    // exactly where they would have been.
    let roll = secs * 180.0 + 720.0 * build * build * build;

    let area = buf.area;
    let cx = f32::from(area.x) + f32::from(area.width) / 2.0;
    let cy = f32::from(area.y) + f32::from(area.height) / 2.0;
    // A cell is about twice as tall as it is wide, so distances are measured
    // in rows with columns halved — or the ring would be an ellipse, and a
    // laser at 45° would lie flatter than it should.
    let reach = (f32::from(area.width) / 4.0).hypot(f32::from(area.height) / 2.0);
    let ring_at = phase * reach * 1.2;
    let ring_fade = 1.0 - phase;
    let beams = [
        (secs * 0.5, 120.0),
        (secs * 0.5 + std::f32::consts::FRAC_PI_2, 120.0),
        (-secs * 0.35 + 0.4, 300.0),
        (-secs * 0.35 + 0.4 + std::f32::consts::FRAC_PI_2, 300.0),
    ];

    let glints = glints(buf, beats);
    // Snapping to the 256-colour palette is a search, and a screen is ten
    // thousand cells asking it twice each: in a debug build that alone was
    // twice the frame. A frame has far fewer distinct colours than cells, so
    // each is searched for once.
    let mut palette: std::collections::HashMap<(u8, u8, u8), Color> =
        std::collections::HashMap::new();
    // In truecolor there is no search to save, and the lookup would cost more
    // than the conversion it stands in for.
    let mut snap = |rgb| match truecolor {
        true => snap(rgb, true),
        false => *palette.entry(rgb).or_insert_with(|| snap(rgb, false)),
    };
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
            let dx = (f32::from(x) - cx) / 2.0;
            let dy = f32::from(y) - cy;

            let ring = (-((dx.hypot(dy) - ring_at) / 1.5).powi(2)).exp() * ring_fade;
            let (mut hue, mut value) = match ring > 0.05 {
                // The complement of the ground, so the ring reads as a wave
                // passing over it rather than the ground getting brighter.
                true => ((ground_hue + 180.0) % 360.0, ground_value + 0.25 * ring),
                false => (ground_hue, ground_value),
            };
            for (angle, beam_hue) in beams {
                // Distance from the line through the middle at `angle`.
                let off = (dx * angle.sin() - dy * angle.cos()).abs();
                if off < 0.6 {
                    hue = beam_hue;
                    value = value.max(0.45 * (1.0 - off / 0.6) + 0.1);
                }
            }
            cell.bg = snap(hsv(hue, 0.9, value.min(0.55)));

            let hue = (f32::from(x) * 6.0 + f32::from(y) * 12.0 - roll).rem_euclid(360.0);
            cell.fg = snap(hsv(hue, 0.85, 1.0));
        }
    }
    for (x, y, glint) in glints {
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_symbol(glint);
            cell.fg = snap(hsv(0.0, 0.0, 1.0));
        }
    }
    equaliser(
        buf,
        footer.intersection(area),
        beats,
        thump,
        build,
        truecolor,
    );
}

/// Where the mirror ball throws its light this half-beat, and with what.
///
/// Only on open ground: a blank cell with blanks either side, so a glint never
/// lands between two words and reads as punctuation. Chosen before anything is
/// written, because a glint is not blank and would close the ground beside it.
fn glints(buf: &Buffer, beats: f32) -> Vec<(u16, u16, &'static str)> {
    const GLINTS: [&str; 4] = ["✦", "✧", "·", "*"];
    let tick = (beats * 2.0).floor() as u32;
    let area = buf.area;
    let blank = |x: u16, y: u16| {
        buf.cell((x, y))
            .is_some_and(|c| c.symbol() == " " && c.diff_option == CellDiffOption::None)
    };
    let mut out = Vec::new();
    for y in area.top()..area.bottom() {
        for x in area.left() + 1..area.right().saturating_sub(1) {
            let roll = scatter(u32::from(x), u32::from(y), tick);
            if roll % 1000 < 6 && blank(x - 1, y) && blank(x, y) && blank(x + 1, y) {
                out.push((x, y, GLINTS[(roll / 1000) as usize % GLINTS.len()]));
            }
        }
    }
    out
}

/// A well-mixed number from a cell and a moment, so the glints land somewhere
/// new every half-beat without anything being remembered between frames.
fn scatter(x: u32, y: u32, tick: u32) -> u32 {
    let mut h =
        x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ tick.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h
}

/// A bar per column, bouncing to the beat, with the way out across the middle
/// and a crowd either side of it.
fn equaliser(buf: &mut Buffer, footer: Rect, beats: f32, thump: f32, build: f32, truecolor: bool) {
    const BARS: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    const MOVES: [&str; 4] = ["\\o/", "\\o_", "_o/", "|o|"];
    let secs = beats * 60.0 / BPM;
    let y = footer.y;
    for x in footer.left()..footer.right() {
        let fx = f32::from(x);
        // Three sines at unrelated rates, so the bars never fall into a
        // pattern the eye can follow, all lifted together by the kick — and
        // pushed toward the top as a build climbs.
        let level = 0.5
            + 0.2 * (fx * 0.9 + secs * 7.0).sin()
            + 0.15 * (fx * 0.37 - secs * 3.1).sin()
            + 0.15 * (fx * 1.7 + secs * 11.3).sin();
        let level = (level * (0.55 + 0.45 * thump) + 0.5 * build).clamp(0.0, 0.999);
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

    let label = match (dropping(beats), build > 0.0) {
        (true, _) => " ♫  D R O P  ♫ ",
        (false, true) => " ♫  here it comes…  ♫ ",
        (false, false) => " ♫  ↑↑↓↓←→←→ba to go home  ♫ ",
    };
    let width = label.chars().count() as u16;
    if footer.width <= width {
        return;
    }
    let x = footer.x + (footer.width - width) / 2;
    let style = ratatui::style::Style::default()
        .fg(Color::Indexed(16))
        .bg(snap(
            hsv((secs * 90.0).rem_euclid(360.0), 0.7, 1.0),
            truecolor,
        ))
        .add_modifier(ratatui::style::Modifier::BOLD);
    buf.set_string(x, y, label, style);

    // Two dancers a side, each a beat out of step with the next, where the
    // footer has room for them past the label.
    const DANCER: u16 = 4;
    if footer.width < width + 4 * DANCER + 2 {
        return;
    }
    let dancer = ratatui::style::Style::default()
        .fg(snap(hsv(0.0, 0.0, 1.0), truecolor))
        .bg(Color::Indexed(16))
        .add_modifier(ratatui::style::Modifier::BOLD);
    let beat = beats.floor() as usize;
    let spots = [x - 2 * DANCER, x - DANCER, x + width, x + width + DANCER];
    for (i, at) in spots.into_iter().enumerate() {
        let step = MOVES[(beat + i) % MOVES.len()];
        buf.set_string(at, y, format!(" {step}"), dancer);
    }
}

/// `rgb` as this terminal can show it.
pub(super) fn snap(rgb: (u8, u8, u8), truecolor: bool) -> Color {
    match truecolor {
        true => Color::Rgb(rgb.0, rgb.1, rgb.2),
        false => Color::Indexed(nearest_indexed(rgb)),
    }
}

/// Hue in degrees, saturation and value in `0..=1`, to RGB.
pub(super) fn hsv(hue: f32, saturation: f32, value: f32) -> (u8, u8, u8) {
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

    /// The party opens on a drop, builds through the last eight beats of
    /// every phrase, and drops on the first beat of the next.
    #[test]
    fn a_phrase_builds_for_eight_beats_and_drops() {
        assert_eq!(build(0.0), 0.0);
        assert!(!dropping(0.5), "the first phrase has no build to drop from");
        assert_eq!(build(23.9), 0.0);
        assert!((build(28.0) - 0.5).abs() < 1e-4);
        assert!(build(31.99) > 0.99);
        assert_eq!(build(32.0), 0.0);
        assert!(dropping(32.0) && dropping(33.9) && !dropping(34.0));
    }

    /// A glint lands only on open ground, never in the space between words,
    /// and never over a letter.
    #[test]
    fn the_mirror_ball_keeps_off_the_text() {
        let area = Rect::new(0, 0, 60, 20);
        let mut buf = Buffer::empty(area);
        for y in 0..20 {
            buf.set_string(0, y, "a b c d e f g", ratatui::style::Style::default());
        }
        // Over enough half-beats that the ball has thrown light everywhere.
        let mut seen = 0;
        for tick in 0..64 {
            for (x, y, _) in glints(&buf, tick as f32 / 2.0) {
                assert!(x > 13, "a glint at ({x}, {y}) is inside the text");
                seen += 1;
            }
        }
        assert!(seen > 0, "the ball never threw any light");
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

    /// A session asked for `ultracode` after cctop started starts the party
    /// once; one from before it started never does, and going home is not
    /// undone by the same prompt being read again.
    #[test]
    fn ultracode_starts_the_party_once() {
        let mut app = crate::ui::tests::test_app();
        let mut before = crate::session::Session::new(Provider::Claude, "old".into());
        before.ultracode_at = Some("2020-01-01T00:00:00.000Z".into());
        app.sessions.push(before);
        app.hear_ultracode();
        assert!(!app.raving(), "a prompt from before cctop started");

        let mut now = crate::session::Session::new(Provider::Codex, "new".into());
        now.ultracode_at = Some(chrono::Utc::now().to_rfc3339());
        app.sessions.push(now);
        app.hear_ultracode();
        assert!(app.raving() || theme::no_color());

        for code in CODE {
            app.on_key(crate::ui::tests::key(code));
        }
        assert!(!app.raving());
        app.hear_ultracode();
        assert!(!app.raving(), "the same prompt, read again");
    }
}
