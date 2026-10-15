//! Sparklines and line charts drawn in sub-cell glyphs.
//!
//! A cell is a small grid of dots — 2×4 for braille and octants, 2×3 for
//! sextants — which gives charts several times the vertical resolution of a
//! row of text. Which glyphs draw the grid is the user's choice
//! (`graph_glyphs`): braille dots work in every font cctop has met, octants and
//! sextants look solid but need a font that has the Legacy Computing blocks.
//!
//! Sparklines draw one sample per cell. In braille they use the left dot column
//! only, a thin bar; the block sets fill both columns, which is the solid look
//! they were chosen for. The line charts use both columns in every set.

use super::theme::{self, Gradient};
use ratatui::style::Style;
use ratatui::symbols::pixel::{OCTANTS, SEXTANTS};
use ratatui::text::{Line, Span};
use std::cell::Cell;

/// Braille bit for dot (x, y) within a cell, `[x % 2][y % 4]`.
const DOT_BITS: [[u16; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

const BRAILLE_BASE: u16 = 0x2800;

/// Which characters draw the dot grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Glyphs {
    #[default]
    Braille,
    /// 2×4 blocks, U+1CD00 and the block elements it reuses (Unicode 16).
    Octant,
    /// 2×3 blocks, U+1FB00 and the half and full blocks (Unicode 13).
    Sextant,
}

impl Glyphs {
    /// In the order the setting cycles through, default first.
    pub const ALL: [Glyphs; 3] = [Glyphs::Braille, Glyphs::Octant, Glyphs::Sextant];

    /// The name `config.toml` spells it with.
    pub fn name(self) -> &'static str {
        match self {
            Glyphs::Braille => "braille",
            Glyphs::Octant => "octant",
            Glyphs::Sextant => "sextant",
        }
    }

    /// Anything unknown is braille, the set every font draws: a setting the
    /// parser already flagged must not leave the charts blank.
    pub fn from_name(name: Option<&str>) -> Glyphs {
        Glyphs::ALL
            .into_iter()
            .find(|g| Some(g.name()) == name)
            .unwrap_or_default()
    }

    /// Dot rows in one cell.
    fn rows(self) -> usize {
        match self {
            Glyphs::Braille | Glyphs::Octant => 4,
            Glyphs::Sextant => 3,
        }
    }

    /// The character for a cell whose dots are `bits`, row-major from the top
    /// left: bit `row * 2 + col`. The order ratatui's pixel tables are indexed
    /// in, so the block sets are a lookup; braille's own bit order is not
    /// row-major and is translated.
    fn glyph(self, bits: u8) -> char {
        match self {
            Glyphs::Braille => {
                let dots = (0..8)
                    .filter(|i| bits & (1 << i) != 0)
                    .fold(0u16, |a, i| a | DOT_BITS[i % 2][i / 2]);
                char::from_u32((BRAILLE_BASE | dots) as u32).unwrap_or(' ')
            }
            Glyphs::Octant => OCTANTS[bits as usize],
            Glyphs::Sextant => SEXTANTS[(bits & 0x3f) as usize],
        }
    }

    /// A sparkline cell standing `filled` dot rows tall.
    fn bar(self, filled: usize) -> char {
        let rows = self.rows();
        let per_row = match self {
            Glyphs::Braille => 0b01,
            Glyphs::Octant | Glyphs::Sextant => 0b11,
        };
        let bits = (rows - filled.min(rows)..rows).fold(0u8, |a, r| a | per_row << (r * 2));
        self.glyph(bits)
    }
}

// Per-thread, for the palette's reason (see `theme::PALETTE`): a test that
// draws in octants must not leave the next test on the same machine drawing
// in them too. The draw thread sets it from the settings file.
thread_local! {
    static GLYPHS: Cell<Glyphs> = const { Cell::new(Glyphs::Braille) };
}

/// The set charts on this thread are drawn in.
pub fn glyphs() -> Glyphs {
    GLYPHS.with(Cell::get)
}

pub fn set_glyphs(g: Glyphs) {
    GLYPHS.with(|c| c.set(g));
}

/// A one-row sparkline, newest sample at the right.
///
/// `max` of 0 auto-scales with 10% headroom. `now_index` highlights the sample
/// representing the current hour/day so a partial bucket reads as "in progress"
/// rather than as a dip.
pub fn sparkline(
    values: &[f64],
    width: usize,
    max: f64,
    gradient: Gradient,
    now_index: Option<usize>,
) -> Line<'static> {
    sparkline_in(glyphs(), values, width, max, gradient, now_index)
}

fn sparkline_in(
    set: Glyphs,
    values: &[f64],
    width: usize,
    max: f64,
    gradient: Gradient,
    now_index: Option<usize>,
) -> Line<'static> {
    let baseline = set.bar(1);
    let base_style = Style::default().fg(gradient.baseline());
    let now_style = Style::default()
        .fg(theme::colors().value)
        .add_modifier(ratatui::style::Modifier::BOLD);

    if width == 0 {
        return Line::default();
    }
    if values.is_empty() {
        return Line::from(vec![Span::styled(
            baseline.to_string().repeat(width),
            base_style,
        )]);
    }

    let start = values.len().saturating_sub(width);
    let visible = &values[start..];

    let hi = if max > 0.0 {
        max
    } else {
        let peak = visible.iter().cloned().fold(0.0f64, f64::max);
        if peak <= 0.0 { 1.0 } else { peak * 1.1 }
    };

    let lead = width - visible.len();
    let now_col = now_index
        .and_then(|n| n.checked_sub(start))
        .map(|n| lead + n);

    // Rows above the baseline: three in a 2×4 cell, two in a sextant. Scaled
    // to the set's own height, so a full-scale sample is a full cell in each.
    let levels = set.rows() - 1;
    let mut spans = Vec::with_capacity(width);
    if lead > 0 {
        spans.push(Span::styled(baseline.to_string().repeat(lead), base_style));
    }
    for (i, &v) in visible.iter().enumerate() {
        let is_now = now_col == Some(lead + i);
        if v <= 0.0 {
            // Still draw the baseline dot; highlight it if this is "now".
            spans.push(Span::styled(
                baseline.to_string(),
                if is_now { now_style } else { base_style },
            ));
            continue;
        }
        // The bottom dot is reserved for "nothing happened here", so a sample
        // that is merely small still stands up out of the baseline. With four
        // levels there is no room for both a faithful scale and a distinct
        // empty state, and a chart where an idle hour and a cheap one draw the
        // same glyph is a chart nobody can read.
        let ratio = (v / hi).clamp(0.0, 1.0);
        let filled = ((ratio * levels as f64).ceil() as usize).clamp(1, levels) + 1;
        let style = if is_now {
            now_style
        } else {
            Style::default().fg(gradient.color(ratio))
        };
        spans.push(Span::styled(set.bar(filled).to_string(), style));
    }
    Line::from(spans)
}

/// `values` plotted as a joined line into `rows` × `cols` cells of `set`,
/// newest at the right: each cell's dot bits, and the tallest sample in each
/// column for its colour.
fn plot(
    set: Glyphs,
    values: &[f64],
    cols: usize,
    rows: usize,
    hi: f64,
) -> (Vec<Vec<u8>>, Vec<f64>) {
    let cell_h = set.rows();
    let dot_w = cols * 2;
    let dot_h = rows * cell_h;

    let mut grid = vec![vec![0u8; cols]; rows];
    let mut col_ratio = vec![0.0f64; cols];

    let start = values.len().saturating_sub(dot_w);
    let visible = &values[start..];

    let set_dot = |grid: &mut Vec<Vec<u8>>, x: usize, y: usize| {
        if x < dot_w && y < dot_h {
            grid[y / cell_h][x / 2] |= 1 << ((y % cell_h) * 2 + x % 2);
        }
    };

    let mut prev_y: Option<usize> = None;
    for (x, &v) in visible.iter().enumerate() {
        let ratio = (v / hi).clamp(0.0, 1.0);
        let y = ((1.0 - ratio) * (dot_h.saturating_sub(1)) as f64).round() as usize;
        set_dot(&mut grid, x, y);

        let tc = x / 2;
        if tc < cols && ratio > col_ratio[tc] {
            col_ratio[tc] = ratio;
        }

        // Join consecutive samples so the series reads as a line, not dots.
        if let Some(py) = prev_y
            && py != y
        {
            let (lo, hi_y) = (py.min(y), py.max(y));
            for yy in (lo + 1)..hi_y {
                let frac = (yy as f64 - py as f64) / (y as f64 - py as f64);
                let xx = (x as f64 - 1.0 + frac).round().max(0.0) as usize;
                set_dot(&mut grid, xx, yy);
            }
        }
        prev_y = Some(y);
    }
    (grid, col_ratio)
}

/// A multi-row line chart with a labelled Y axis.
///
/// Returns `rows + 1` lines: the plot, then the axis rule. `axis_width` forces a
/// shared gutter so two charts side by side line up their plot areas.
pub fn line_chart(
    values: &[f64],
    total_width: usize,
    rows: usize,
    max: f64,
    gradient: Gradient,
    axis_width: Option<usize>,
) -> Vec<Line<'static>> {
    line_chart_in(
        glyphs(),
        values,
        total_width,
        rows,
        max,
        gradient,
        axis_width,
    )
}

fn line_chart_in(
    set: Glyphs,
    values: &[f64],
    total_width: usize,
    rows: usize,
    max: f64,
    gradient: Gradient,
    axis_width: Option<usize>,
) -> Vec<Line<'static>> {
    let hi = if max > 0.0 {
        max
    } else {
        cctop_core::util::nice_max(values.iter().cloned().fold(1.0f64, f64::max))
    };
    let label_len = format!("{}", hi.ceil() as i64).len().max(1);
    let axis_w = axis_width.unwrap_or(label_len + 2);
    let cols = total_width.saturating_sub(axis_w).max(4);
    let (grid, col_ratio) = plot(set, values, cols, rows, hi);

    let axis_style = Style::default().fg(theme::gray(238));
    let label_style = Style::default().fg(theme::gray(239));
    let empty_style = Style::default().fg(theme::gray(236));

    let mut out = Vec::with_capacity(rows + 1);
    for (r, row) in grid.iter().enumerate() {
        let tick = (hi * (rows - r) as f64 / rows as f64).round() as i64;
        let mut spans = vec![
            Span::styled(format!("{tick:>label_len$}"), label_style),
            Span::styled(" ┤".to_string(), axis_style),
        ];
        for (c, &bits) in row.iter().enumerate() {
            let style = if bits == 0 {
                empty_style
            } else {
                Style::default().fg(gradient.color(col_ratio[c]))
            };
            spans.push(Span::styled(set.glyph(bits).to_string(), style));
        }
        out.push(Line::from(spans));
    }
    out.push(Line::from(vec![
        Span::styled(format!("{:>label_len$}", 0), label_style),
        Span::styled(format!(" └{}", "─".repeat(cols)), axis_style),
    ]));
    out
}

/// A few cells of a fixed wave drawn in `set`, for the settings row: a chart
/// is where the set's rarer glyphs turn up, so it is what shows whether the
/// font has them before the choice is made. Four cells, so all three sets fit
/// beside each other at 80 columns.
pub fn preview(set: Glyphs) -> String {
    const WAVE: [f64; 8] = [0.1, 0.6, 1.0, 0.7, 0.2, 0.0, 0.5, 0.9];
    let (grid, _) = plot(set, &WAVE, WAVE.len() / 2, 1, 1.0);
    grid[0].iter().map(|&b| set.glyph(b)).collect()
}

/// Fixed-size ring of recent samples, for the CPU/memory history charts.
#[derive(Debug, Clone)]
pub struct History {
    values: Vec<f64>,
    capacity: usize,
}

impl History {
    pub fn new(capacity: usize) -> Self {
        History {
            values: Vec::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, v: f64) {
        if self.values.len() == self.capacity {
            self.values.remove(0);
        }
        self.values.push(v);
    }

    pub fn values(&self) -> &[f64] {
        &self.values
    }
}

impl Default for History {
    fn default() -> Self {
        History::new(300)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::symbols::pixel::QUADRANTS;
    use std::collections::HashSet;

    /// The braille the sparklines drew before there was a choice, spelled out
    /// here rather than borrowed from the code under test, so the default
    /// staying put is something these tests check rather than assume.
    const LEFT_COL_BITS: [u16; 4] = [0x40, 0x04, 0x02, 0x01];

    fn braille(bits: u16) -> char {
        char::from_u32(0x2800 | bits as u32).unwrap()
    }

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Asserted by glyph rather than by width, because a sparkline that ignored its
    /// input and drew one repeated character would be exactly the width asked
    /// for: the count says only that `width` characters came out.
    #[test]
    fn sparkline_pads_left_when_underfilled() {
        let l = sparkline(&[1.0, 2.0], 6, 0.0, Gradient::Cpu, None);
        let baseline = braille(LEFT_COL_BITS[0]).to_string();
        let height = |filled: usize| {
            braille(LEFT_COL_BITS[..filled].iter().fold(0u16, |a, b| a | b)).to_string()
        };
        assert_eq!(
            text(&l),
            // Four columns of "nothing yet", then the two samples — and the
            // taller of the two last, because 2.0 is the larger of them.
            format!(
                "{baseline}{baseline}{baseline}{baseline}{}{}",
                height(3),
                height(4)
            )
        );
    }

    /// Clipping keeps the *newest* samples, which is the whole point of a
    /// sparkline: the current hour is at the right, so keeping the oldest would
    /// draw a chart about yesterday.
    #[test]
    fn sparkline_clips_to_newest_samples() {
        let values: Vec<f64> = (1..=20).map(|v| v as f64).collect();
        let clipped = sparkline(&values, 5, 0.0, Gradient::Cpu, None);
        let last_five: Vec<f64> = values[15..].to_vec();
        assert_eq!(
            text(&clipped),
            text(&sparkline(&last_five, 5, 0.0, Gradient::Cpu, None))
        );
        // …and not the oldest five, which is what a chart about the wrong window
        // would look like.
        let first_five: Vec<f64> = values[..5].to_vec();
        assert_ne!(
            text(&clipped),
            text(&sparkline(&first_five, 5, 0.0, Gradient::Cpu, None))
        );
    }

    /// A rising series and a falling one are the same numbers the other way
    /// round, so anything that drew one glyph for both — or for either — would
    /// pass a test that only counted characters.
    #[test]
    fn a_rising_series_is_not_a_falling_one() {
        let rising = sparkline(&[1.0, 2.0, 3.0, 4.0], 4, 10.0, Gradient::Cpu, None);
        let falling = sparkline(&[4.0, 3.0, 2.0, 1.0], 4, 10.0, Gradient::Cpu, None);
        assert_ne!(text(&rising), text(&falling));
        // Newest at the right: the tall end is the last column of the rising
        // one and the first column of the falling one.
        let glyphs: Vec<String> = rising
            .spans
            .iter()
            .map(|s| s.content.as_ref().to_string())
            .collect();
        assert_ne!(glyphs[0], glyphs[3]);
    }

    #[test]
    fn sparkline_height_tracks_magnitude() {
        // A full-scale sample fills all four dots; a small one does not.
        let full = sparkline(&[100.0], 1, 100.0, Gradient::Cpu, None);
        let low = sparkline(&[10.0], 1, 100.0, Gradient::Cpu, None);
        assert_eq!(text(&full), braille(0x40 | 0x04 | 0x02 | 0x01).to_string());
        assert_eq!(text(&low), braille(0x40 | 0x04).to_string());
    }

    /// The whole point of the reserved baseline: a bucket with a trace of spend
    /// in it must not draw the same glyph as one with none.
    #[test]
    fn a_tiny_sample_is_still_distinguishable_from_nothing() {
        let empty = sparkline(&[0.0], 1, 100.0, Gradient::Spend, None);
        let trace = sparkline(&[0.000_1], 1, 100.0, Gradient::Spend, None);
        assert_ne!(text(&empty), text(&trace));
        assert_eq!(text(&empty), braille(0x40).to_string());
    }

    #[test]
    fn zero_samples_still_draw_a_baseline() {
        let l = sparkline(&[0.0, 0.0], 2, 10.0, Gradient::Spend, None);
        assert_eq!(text(&l), braille(0x40).to_string().repeat(2));
    }

    #[test]
    fn empty_and_zero_width_are_safe() {
        assert_eq!(
            text(&sparkline(&[], 4, 0.0, Gradient::Cpu, None))
                .chars()
                .count(),
            4
        );
        assert_eq!(text(&sparkline(&[1.0], 0, 0.0, Gradient::Cpu, None)), "");
    }

    #[test]
    fn line_chart_returns_rows_plus_axis() {
        let values: Vec<f64> = (0..40).map(|v| v as f64).collect();
        let lines = line_chart(&values, 30, 4, 100.0, Gradient::Cpu, None);
        assert_eq!(lines.len(), 5);
        // Every row must be the same display width or the panel will tear.
        let widths: Vec<usize> = lines.iter().map(|l| text(l).chars().count()).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "ragged chart rows: {widths:?}"
        );
    }

    #[test]
    fn line_chart_survives_empty_input() {
        let lines = line_chart(&[], 20, 3, 0.0, Gradient::Accent, None);
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn history_keeps_newest_within_capacity() {
        let mut h = History::new(3);
        for i in 1..=5 {
            h.push(i as f64);
        }
        assert_eq!(h.values(), &[3.0, 4.0, 5.0]);
    }

    /// Every pattern a cell can hold draws a different character, empty is
    /// blank and full is the full block — in each set, over its whole range.
    #[test]
    fn every_pattern_has_its_own_glyph() {
        for set in Glyphs::ALL {
            let patterns = 1usize << (set.rows() * 2);
            let drawn: HashSet<char> = (0..patterns).map(|b| set.glyph(b as u8)).collect();
            assert_eq!(drawn.len(), patterns, "{set:?} reuses a glyph");
        }
        assert_eq!(Glyphs::Braille.glyph(0), '\u{2800}');
        assert_eq!(Glyphs::Braille.glyph(0xff), '\u{28ff}');
        for set in [Glyphs::Octant, Glyphs::Sextant] {
            assert_eq!(set.glyph(0), ' ', "{set:?}");
            assert_eq!(
                set.glyph(((1u16 << (set.rows() * 2)) - 1) as u8),
                '█',
                "{set:?}"
            );
        }
    }

    /// Unicode 16 left out of U+1CD00 every octant pattern that already had a
    /// character — quadrants, halves, eighth blocks — and numbered the rest in
    /// pattern order. So the 230 that remain must be that block, in order, and
    /// a pattern a quadrant can draw must be drawn by that quadrant.
    #[test]
    fn octants_are_the_unicode_block_plus_the_old_blocks() {
        let fresh: Vec<u32> = (0..=255u8)
            .map(|b| Glyphs::Octant.glyph(b) as u32)
            .filter(|c| (0x1CD00..=0x1CDE5).contains(c))
            .collect();
        assert_eq!(fresh, (0x1CD00..=0x1CDE5).collect::<Vec<_>>());
        for (q, &quadrant) in QUADRANTS.iter().enumerate() {
            // A quadrant is two dot rows of one column.
            let bits = [
                (0, 0b0101u8),
                (1, 0b1010),
                (2, 0b0101_0000),
                (3, 0b1010_0000),
            ]
            .iter()
            .filter(|(bit, _)| q & (1 << bit) != 0)
            .fold(0u8, |a, (_, dots)| a | dots);
            assert_eq!(Glyphs::Octant.glyph(bits), quadrant, "quadrant {q:04b}");
        }
        // The eighth blocks the sparkline bars are made of.
        assert_eq!(Glyphs::Octant.glyph(0b1100_0000), '▂');
        assert_eq!(Glyphs::Octant.glyph(0b1111_0000), '▄');
        assert_eq!(Glyphs::Octant.glyph(0b1111_1100), '▆');
    }

    /// The same rule one block over: sextants skip only the two half blocks
    /// and the full one, besides empty.
    #[test]
    fn sextants_are_the_unicode_block_plus_the_halves() {
        let fresh: Vec<u32> = (0..64u8)
            .map(|b| Glyphs::Sextant.glyph(b) as u32)
            .filter(|c| (0x1FB00..=0x1FB3B).contains(c))
            .collect();
        assert_eq!(fresh, (0x1FB00..=0x1FB3B).collect::<Vec<_>>());
        assert_eq!(Glyphs::Sextant.glyph(0b01_0101), '▌');
        assert_eq!(Glyphs::Sextant.glyph(0b10_1010), '▐');
    }

    /// One series, each set: braille keeps its thin left-column bars, the
    /// block sets fill the cell, and the sextant's three rows still put a
    /// full-scale sample at full height.
    #[test]
    fn a_known_series_in_each_set() {
        let series = [0.0, 1.0, 2.0, 3.0, 4.0];
        let drawn = |set| text(&sparkline_in(set, &series, 5, 4.0, Gradient::Cpu, None));
        assert_eq!(drawn(Glyphs::Braille), "⡀⡄⡆⡇⡇");
        assert_eq!(drawn(Glyphs::Octant), "▂▄▆██");
        assert_eq!(drawn(Glyphs::Sextant), "🬭🬹🬹██");
    }

    /// The thread's choice is what the public functions draw in, and a new
    /// thread starts on braille whatever another chose.
    #[test]
    fn the_chosen_set_is_per_thread() {
        let before = glyphs();
        set_glyphs(Glyphs::Octant);
        let drawn = text(&sparkline(&[1.0], 1, 1.0, Gradient::Cpu, None));
        let elsewhere = std::thread::spawn(glyphs).join().unwrap();
        set_glyphs(before);
        assert_eq!(drawn, "█");
        assert_eq!(elsewhere, Glyphs::Braille);
    }

    /// A slope through a chart in octants lands on U+1CD00 glyphs, which is
    /// the font check the settings preview exists for, and every set keeps
    /// the rows one width.
    #[test]
    fn line_charts_in_each_set() {
        let values: Vec<f64> = (0..40).map(|v| v as f64).collect();
        let chart = |set| line_chart_in(set, &values, 30, 3, 40.0, Gradient::Cpu, None);
        for set in Glyphs::ALL {
            let widths: Vec<usize> = chart(set).iter().map(Line::width).collect();
            assert!(
                widths.windows(2).all(|w| w[0] == w[1]),
                "{set:?}: {widths:?}"
            );
        }
        let octant: String = chart(Glyphs::Octant).iter().map(text).collect();
        assert!(
            octant
                .chars()
                .any(|c| (0x1CD00..=0x1CDE5).contains(&(c as u32)))
        );
        for set in Glyphs::ALL {
            assert_eq!(preview(set).chars().count(), 4, "{set:?}");
        }
        assert_ne!(preview(Glyphs::Octant), preview(Glyphs::Sextant));
    }

    #[test]
    fn names_round_trip_and_unknown_is_braille() {
        for set in Glyphs::ALL {
            assert_eq!(Glyphs::from_name(Some(set.name())), set);
        }
        assert_eq!(Glyphs::from_name(Some("quadrant")), Glyphs::Braille);
        assert_eq!(Glyphs::from_name(None), Glyphs::Braille);
    }
}
