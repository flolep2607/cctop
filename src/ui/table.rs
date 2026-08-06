//! The session table: column sizing, row rendering, and per-cell colouring.

use super::App;
use super::columns::{self, COLUMNS, ColumnId};
use super::render::{Layout, panel_block};
use super::theme;
use crate::pricing::Provider;
use crate::session::Surface;
use crate::util;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

// ---------------------------------------------------------------------------
// Session table
// ---------------------------------------------------------------------------

/// Resolve each column's width, giving the flexible column whatever is left.
fn column_widths(total: u16) -> Vec<u16> {
    let fixed: u16 = COLUMNS.iter().filter_map(|c| c.width).sum::<u16>()
        + (COLUMNS.len().saturating_sub(1)) as u16; // single-space gutters
    let flex = total.saturating_sub(fixed).max(8);
    COLUMNS.iter().map(|c| c.width.unwrap_or(flex)).collect()
}

fn pad(text: &str, width: u16, right: bool) -> String {
    let w = width as usize;
    let t = util::truncate(text, w);
    let len = t.chars().count();
    if right {
        format!("{}{}", " ".repeat(w.saturating_sub(len)), t)
    } else {
        format!("{}{}", t, " ".repeat(w.saturating_sub(len)))
    }
}

pub(super) fn draw_table(frame: &mut Frame, area: Rect, app: &mut App, layout: &mut Layout) {
    let title = if app.live_only {
        format!(
            "Sessions ({}/{}) — live",
            app.visible.len(),
            app.sessions.len()
        )
    } else {
        format!("Sessions ({})", app.sessions.len())
    };
    let block = panel_block(&title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 {
        return;
    }
    let widths = column_widths(inner.width);

    // Header, recording click spans as we go.
    let mut header_spans = Vec::new();
    let mut col_pos = inner.x;
    layout.column_spans.clear();
    for (c, w) in COLUMNS.iter().zip(&widths) {
        let arrow = if c.id == app.sort_col {
            if app.sort_asc { "▲" } else { "▼" }
        } else {
            ""
        };
        let text = format!("{}{}", c.label, arrow);
        header_spans.push(Span::raw(pad(&text, *w, c.right_align)));
        header_spans.push(Span::raw(" "));
        layout.column_spans.push((col_pos, col_pos + w + 1, c.id));
        col_pos += w + 1;
    }
    layout.header_row = inner.y;
    frame.render_widget(
        Paragraph::new(Line::from(header_spans)).style(
            Style::default()
                .fg(Color::White)
                .bg(theme::HEADER_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Rect { height: 1, ..inner },
    );

    let list_area = Rect {
        y: inner.y + 1,
        height: inner.height.saturating_sub(1),
        ..inner
    };
    layout.rows_start = list_area.y;
    layout.rows_end = list_area.y + list_area.height;

    if app.visible.is_empty() {
        let msg = if app.live_only {
            "No running sessions."
        } else if !app.search.is_empty() || app.age_filter.is_some() {
            "No sessions match the current filters. Esc clears them."
        } else {
            "No Claude, Codex, Cursor, OpenCode, or Pi sessions found."
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, theme::dim()))),
            list_area,
        );
        return;
    }

    // Keep the cursor on screen; in follow mode it's held centered.
    let height = list_area.height as usize;
    app.list_height = height as u16;
    app.scroll = if app.follow {
        app.selected.saturating_sub(height / 2)
    } else {
        if app.selected < app.scroll {
            app.selected
        } else if app.selected >= app.scroll + height {
            app.selected + 1 - height
        } else {
            app.scroll
        }
    };
    app.scroll = app
        .scroll
        .min(app.visible.len().saturating_sub(height.max(1)));

    let now = chrono::Utc::now();
    let lines: Vec<Line> = app
        .visible
        .iter()
        .skip(app.scroll)
        .take(height)
        .enumerate()
        .map(|(i, &idx)| {
            let s = &app.sessions[idx];
            let selected = app.scroll + i == app.selected;
            let marked = app.marked.contains(&s.key());
            session_row(s, &widths, selected, marked, &now)
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), list_area);
}

fn session_row(
    s: &crate::session::Session,
    widths: &[u16],
    selected: bool,
    marked: bool,
    now: &chrono::DateTime<chrono::Utc>,
) -> Line<'static> {
    let age_secs = util::parse_ts(&s.last_active).map(|d| (now.timestamp() - d.timestamp()).max(0));
    let base = if selected {
        Style::default()
            .bg(theme::SELECTED_BG)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else if marked {
        Style::default().bg(theme::MARKED_BG)
    } else {
        Style::default()
    };

    let mut spans = Vec::with_capacity(COLUMNS.len() * 2);
    for (c, w) in COLUMNS.iter().zip(widths) {
        let text = columns::render_cell(c.id, s, now);
        // Selection keeps the row's highlight but the status dot must stay
        // colored, otherwise you can't tell a running session from a stopped
        // one on the selected line.
        let style = if selected {
            if c.id == ColumnId::Status {
                Style::default()
                    .bg(theme::SELECTED_BG)
                    .fg(cell_color(c.id, s, age_secs))
                    .add_modifier(Modifier::BOLD)
            } else {
                base
            }
        } else {
            base.fg(cell_color(c.id, s, age_secs))
        };
        spans.push(Span::styled(pad(&text, *w, c.right_align), style));
        spans.push(Span::styled(" ", base));
    }
    Line::from(spans)
}

fn cell_color(id: ColumnId, s: &crate::session::Session, age_secs: Option<i64>) -> Color {
    match id {
        ColumnId::Status => match s.activity_state {
            crate::session::ActivityState::WaitingForInput => theme::COST_MID,
            crate::session::ActivityState::ApiError => theme::COST_HIGH,
            crate::session::ActivityState::Working if s.is_running() => {
                theme::running_dot_color(age_secs)
            }
            crate::session::ActivityState::Working => theme::DIM,
        },
        ColumnId::Last => theme::age_color(age_secs, s.is_running()),
        ColumnId::Model => theme::model_color(&s.model),
        ColumnId::Project => match s.surface {
            Surface::DesktopCowork => theme::DESKTOP_COWORK,
            Surface::DesktopCode => theme::DESKTOP_CODE,
            Surface::Editor => theme::CURSOR,
            Surface::Cli if s.provider == Provider::Cursor => theme::CURSOR,
            Surface::Cli => Color::Reset,
        },
        ColumnId::Cost => {
            if s.cost_is_free {
                theme::DIMMER
            } else {
                s.total_cost.map(theme::cost_color).unwrap_or(theme::DIM)
            }
        }
        ColumnId::CostHour => {
            if s.cost_is_free {
                theme::DIMMER
            } else if s.cost_hour > 0.0 {
                theme::cost_color(s.cost_hour)
            } else {
                theme::DIMMER
            }
        }
        ColumnId::CostToday => {
            if s.cost_is_free {
                theme::DIMMER
            } else if s.cost_today > 0.0 {
                theme::cost_color(s.cost_today)
            } else {
                theme::DIMMER
            }
        }
        ColumnId::Context => match &s.context {
            Some(c) if c.compacting => theme::COST_HIGH,
            Some(c) => theme::context_color(c.percent_to_compact()),
            None => theme::DIMMER,
        },
        ColumnId::Cpu => s
            .process
            .as_ref()
            .map(|p| theme::cpu_color(p.cpu))
            .unwrap_or(theme::DIMMER),
        ColumnId::TokenRate => {
            if s.tokens_per_min > 5000.0 {
                theme::COST_HIGH
            } else if s.tokens_per_min > 1000.0 {
                theme::COST_MID
            } else if s.tokens_per_min > 0.0 {
                theme::COST_LOW
            } else {
                theme::DIM
            }
        }
        _ => Color::Reset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_widths_fill_the_available_space() {
        let widths = column_widths(200);
        let total: u16 = widths.iter().sum::<u16>() + (COLUMNS.len() - 1) as u16;
        assert_eq!(total, 200);
    }

    #[test]
    fn column_widths_stay_positive_when_cramped() {
        // A narrow terminal must not produce a zero or wrapped-around width.
        for w in [10u16, 40, 80] {
            let widths = column_widths(w);
            assert!(
                widths.iter().all(|&x| x > 0),
                "width {w} produced {widths:?}"
            );
        }
    }

    #[test]
    fn pad_truncates_and_aligns() {
        assert_eq!(pad("abc", 5, false), "abc  ");
        assert_eq!(pad("abc", 5, true), "  abc");
        assert_eq!(pad("abcdefgh", 4, false).chars().count(), 4);
    }
}
