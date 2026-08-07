//! Braille sparklines and line charts.
//!
//! Braille cells pack a 2×4 dot grid into one character, giving 8× the vertical
//! resolution of block glyphs. Sparklines use only the left dot column so each
//! character maps to exactly one sample; the larger charts use both.

use super::theme::{self, Gradient};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// Left-column braille bits, bottom row first.
const LEFT_COL_BITS: [u16; 4] = [0x40, 0x04, 0x02, 0x01];

/// Braille bit for dot (x, y) within a cell, `[x % 2][y % 4]`.
const DOT_BITS: [[u16; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

const BRAILLE_BASE: u16 = 0x2800;

fn braille(bits: u16) -> char {
    char::from_u32((BRAILLE_BASE | bits) as u32).unwrap_or(' ')
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
    let baseline = braille(LEFT_COL_BITS[0]);
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
        let ratio = (v / hi).clamp(0.0, 1.0);
        let filled = ((ratio * 4.0).round() as usize).clamp(1, 4);
        let bits = LEFT_COL_BITS[..filled].iter().fold(0u16, |a, b| a | b);
        let style = if is_now {
            now_style
        } else {
            Style::default().fg(gradient.color(ratio))
        };
        spans.push(Span::styled(braille(bits).to_string(), style));
    }
    Line::from(spans)
}

/// A multi-row braille line chart with a labelled Y axis.
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
    let hi = if max > 0.0 {
        max
    } else {
        crate::util::nice_max(values.iter().cloned().fold(1.0f64, f64::max))
    };
    let label_len = format!("{}", hi.ceil() as i64).len().max(1);
    let axis_w = axis_width.unwrap_or(label_len + 2);
    let cols = total_width.saturating_sub(axis_w).max(4);
    let dot_w = cols * 2;
    let dot_h = rows * 4;

    let mut grid = vec![vec![0u16; cols]; rows];
    let mut col_ratio = vec![0.0f64; cols];

    let start = values.len().saturating_sub(dot_w);
    let visible = &values[start..];

    let set_dot = |grid: &mut Vec<Vec<u16>>, x: usize, y: usize| {
        if x < dot_w && y < dot_h {
            grid[y / 4][x / 2] |= DOT_BITS[x % 2][y % 4];
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
            spans.push(Span::styled(braille(bits).to_string(), style));
        }
        out.push(Line::from(spans));
    }
    out.push(Line::from(vec![
        Span::styled(format!("{:>label_len$}", 0), label_style),
        Span::styled(format!(" └{}", "─".repeat(cols)), axis_style),
    ]));
    out
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

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn sparkline_pads_left_when_underfilled() {
        let l = sparkline(&[1.0, 2.0], 6, 0.0, Gradient::Cpu, None);
        assert_eq!(text(&l).chars().count(), 6);
    }

    #[test]
    fn sparkline_clips_to_newest_samples() {
        let values: Vec<f64> = (1..=20).map(|v| v as f64).collect();
        let l = sparkline(&values, 5, 0.0, Gradient::Cpu, None);
        assert_eq!(text(&l).chars().count(), 5);
    }

    #[test]
    fn sparkline_height_tracks_magnitude() {
        // A full-scale sample fills all four dots; a small one does not.
        let full = sparkline(&[100.0], 1, 100.0, Gradient::Cpu, None);
        let low = sparkline(&[10.0], 1, 100.0, Gradient::Cpu, None);
        assert_eq!(text(&full), braille(0x40 | 0x04 | 0x02 | 0x01).to_string());
        assert_eq!(text(&low), braille(0x40).to_string());
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
}
