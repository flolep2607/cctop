//! The session table: column sizing, row rendering, and per-cell colouring.

use super::App;
use super::columns::{self, ColumnId};
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
fn column_widths(cols: &[&'static columns::Column], total: u16) -> Vec<u16> {
    let fixed: u16 = cols.iter().filter_map(|c| c.width).sum::<u16>()
        + (cols.len().saturating_sub(1)) as u16; // single-space gutters
    let flex = total.saturating_sub(fixed).max(8);
    cols.iter().map(|c| c.width.unwrap_or(flex)).collect()
}

/// Providers cctop knows how to read, and where it looks for each. Shown when
/// the list is genuinely empty, since "found nothing" is only useful next to
/// "here is what I looked for".
fn provider_search_paths() -> Vec<(&'static str, String)> {
    use crate::config;
    vec![
        (
            "Claude Code",
            config::CLAUDE_PROJECTS_ROOT.display().to_string(),
        ),
        ("Codex", config::CODEX_SESSIONS_ROOT.display().to_string()),
        ("Cursor", config::CURSOR_PROJECTS_ROOT.display().to_string()),
        ("Gemini CLI", config::GEMINI_CHATS_ROOT.display().to_string()),
        ("OpenCode", config::OPENCODE_DATA_DIR.display().to_string()),
        ("Pi", config::PI_SESSIONS_ROOT.display().to_string()),
        ("Windsurf", config::WINDSURF_USER_DIR.display().to_string()),
    ]
}

#[cfg(test)]
/// Every provider must name itself here, or an empty screen quietly implies
/// cctop cannot see a tool it can in fact read.
fn provider_is_listed(p: crate::pricing::Provider) -> bool {
    let listed = provider_search_paths();
    listed
        .iter()
        .any(|(name, _)| name.to_ascii_lowercase().starts_with(p.as_str()))
}

/// What to say when there is nothing to draw.
fn empty_lines(app: &App) -> Vec<Line<'static>> {
    if !app.loaded {
        return vec![Line::from(Span::styled(
            "Scanning for sessions…",
            theme::dim(),
        ))];
    }
    if app.live_only {
        return vec![Line::from(Span::styled(
            "No running sessions. ` shows stopped ones too.",
            theme::dim(),
        ))];
    }
    if !app.search.is_empty() || app.age_filter.is_some() || app.cost_floor > 0.0 {
        return vec![Line::from(Span::styled(
            "No sessions match the current filters. Esc clears them, one at a time.",
            theme::dim(),
        ))];
    }
    let mut lines = vec![
        Line::from(Span::styled(
            "No agent sessions found. cctop looked in:",
            theme::dim(),
        )),
        Line::default(),
    ];
    for (name, path) in provider_search_paths() {
        lines.push(Line::from(vec![
            Span::styled(format!("  {name:<12}"), theme::dim()),
            Span::styled(path, theme::dim()),
        ]));
    }
    lines
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
    let cols = columns::visible_columns(inner.width, &app.hidden_columns);
    let widths = column_widths(&cols, inner.width);

    // Header, recording click spans as we go.
    let mut header_spans = Vec::new();
    let mut col_pos = inner.x;
    layout.column_spans.clear();
    for (c, w) in cols.iter().zip(&widths) {
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
        Paragraph::new(Line::from(header_spans)).style(theme::header()),
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
        frame.render_widget(Paragraph::new(empty_lines(app)), list_area);
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
            let key = s.key();
            let marked = app.marked.contains(&key);
            let deleting = app.deleting.contains(&key);
            let state = RowState {
                selected,
                marked,
                deleting,
                rang: app.notify.rang_recently(&key),
            };
            session_row(s, &cols, &widths, state, &now)
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), list_area);
}

/// What a row is doing beyond its data: whether it is under the cursor, marked
/// for a batch action, on its way out, or has just rung. Grouped because they
/// travel together and are all read by the same loop.
#[derive(Debug, Clone, Copy, Default)]
struct RowState {
    selected: bool,
    marked: bool,
    deleting: bool,
    rang: bool,
}

fn session_row(
    s: &crate::session::Session,
    cols: &[&'static columns::Column],
    widths: &[u16],
    state: RowState,
    now: &chrono::DateTime<chrono::Utc>,
) -> Line<'static> {
    let RowState {
        selected,
        marked,
        deleting,
        rang,
    } = state;
    let age_secs = util::parse_ts(&s.last_active).map(|d| (now.timestamp() - d.timestamp()).max(0));
    let base = if selected {
        theme::selected()
    } else if marked {
        theme::marked()
    } else {
        Style::default()
    };

    let mut spans = Vec::with_capacity(cols.len() * 2);
    for (c, w) in cols.iter().zip(widths) {
        // The session that just rang takes over its own status dot for a
        // moment. Without it a bell out of a dozen panes is a sound with no
        // row attached — and its ordinary dot would be the same hollow grey as
        // every other session that has stopped. A pending delete outranks it:
        // that row is on its way out.
        let bell = rang && !deleting && c.id == ColumnId::Status;
        let text = if deleting && c.id == ColumnId::Status {
            "…".to_string()
        } else if bell {
            "◉".to_string()
        } else {
            columns::render_cell(c.id, s, now)
        };
        // Selection keeps the row's highlight but the status dot must stay
        // colored, otherwise you can't tell a running session from a stopped
        // one on the selected line.
        let style = if bell {
            base.fg(theme::colors().accent).add_modifier(Modifier::BOLD)
        } else if selected {
            if c.id == ColumnId::Status {
                theme::selected().fg(cell_color(c.id, s, age_secs))
            } else {
                base
            }
        } else {
            base.fg(if deleting && c.id == ColumnId::Status {
                theme::colors().cost_mid
            } else {
                cell_color(c.id, s, age_secs)
            })
        };
        spans.push(Span::styled(pad(&text, *w, c.right_align), style));
        spans.push(Span::styled(" ", base));
    }
    Line::from(spans)
}

fn cell_color(id: ColumnId, s: &crate::session::Session, age_secs: Option<i64>) -> Color {
    match id {
        ColumnId::Status => match s.activity_state {
            crate::session::ActivityState::WaitingForInput => theme::colors().cost_mid,
            crate::session::ActivityState::ApiError => theme::colors().cost_high,
            crate::session::ActivityState::Working if s.is_running() => {
                theme::running_dot_color(age_secs)
            }
            crate::session::ActivityState::Working => theme::colors().dim,
        },
        ColumnId::Last => theme::age_color(age_secs, s.is_running()),
        ColumnId::Model => theme::model_color(&s.model),
        ColumnId::Project => match s.surface {
            Surface::DesktopCowork => theme::colors().desktop_cowork,
            Surface::DesktopCode => theme::colors().desktop_code,
            Surface::Editor => theme::colors().cursor,
            Surface::Cli if s.provider == Provider::Cursor => theme::colors().cursor,
            Surface::Cli => Color::Reset,
        },
        ColumnId::Cost => {
            if s.cost_is_free {
                theme::colors().dimmer
            } else {
                s.total_cost.map(theme::cost_color).unwrap_or(theme::colors().dim)
            }
        }
        ColumnId::CostHour => {
            if s.cost_is_free {
                theme::colors().dimmer
            } else if s.cost_hour > 0.0 {
                theme::cost_color(s.cost_hour)
            } else {
                theme::colors().dimmer
            }
        }
        ColumnId::CostToday => {
            if s.cost_is_free {
                theme::colors().dimmer
            } else if s.cost_today > 0.0 {
                theme::cost_color(s.cost_today)
            } else {
                theme::colors().dimmer
            }
        }
        ColumnId::Context => match &s.context {
            Some(c) if c.compacting => theme::colors().cost_high,
            Some(c) => theme::context_color(c.percent_to_compact()),
            None => theme::colors().dimmer,
        },
        ColumnId::Cpu => s
            .process
            .as_ref()
            .map(|p| theme::cpu_color(p.cpu))
            .unwrap_or(theme::colors().dimmer),
        ColumnId::TokenRate => {
            if s.tokens_per_min > 5000.0 {
                theme::colors().cost_high
            } else if s.tokens_per_min > 1000.0 {
                theme::colors().cost_mid
            } else if s.tokens_per_min > 0.0 {
                theme::colors().cost_low
            } else {
                theme::colors().dim
            }
        }
        _ => Color::Reset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::columns::COLUMNS;

    fn all_columns() -> Vec<&'static columns::Column> {
        COLUMNS.iter().collect()
    }

    #[test]
    fn column_widths_fill_the_available_space() {
        let cols = all_columns();
        let widths = column_widths(&cols, 200);
        let total: u16 = widths.iter().sum::<u16>() + (cols.len() - 1) as u16;
        assert_eq!(total, 200);
    }

    #[test]
    fn column_widths_stay_positive_when_cramped() {
        // A narrow terminal must not produce a zero or wrapped-around width.
        for w in [10u16, 40, 80] {
            let cols = columns::visible_columns(w, &[]);
            let widths = column_widths(&cols, w);
            assert!(
                widths.iter().all(|&x| x > 0),
                "width {w} produced {widths:?}"
            );
        }
    }

    /// The bell has to land on a row, not only in the footer — and a stopped
    /// session's ordinary hollow dot is exactly what it must not look like.
    #[test]
    fn the_session_that_rang_wears_its_own_marker() {
        let mut s = crate::session::Session::new(Provider::Claude, "a".into());
        s.last_active = chrono::Utc::now().to_rfc3339();
        let now = chrono::Utc::now();
        let cols = all_columns();
        let widths = column_widths(&cols, 200);

        let quiet = session_row(&s, &cols, &widths, RowState::default(), &now);
        let rang = session_row(
            &s,
            &cols,
            &widths,
            RowState {
                rang: true,
                ..RowState::default()
            },
            &now,
        );
        assert_eq!(quiet.spans[0].content, "○");
        assert_eq!(rang.spans[0].content, "◉");
        assert_eq!(rang.spans[0].style.fg, Some(theme::colors().accent));
    }

    /// The surviving columns must actually fit, or the drop was pointless.
    #[test]
    fn narrow_layouts_fit_inside_the_terminal() {
        for w in [40u16, 60, 80, 100, 132, 200] {
            let cols = columns::visible_columns(w, &[]);
            let widths = column_widths(&cols, w);
            let used: u16 = widths.iter().sum::<u16>() + (cols.len() - 1) as u16;
            assert!(used <= w, "width {w} used {used} cells");
        }
    }

    /// The empty screen names every provider cctop can read. A new one added
    /// to `Provider` without a line here would look unsupported.
    #[test]
    fn the_empty_state_accounts_for_every_provider() {
        for p in [
            Provider::Claude,
            Provider::Codex,
            Provider::Cursor,
            Provider::Gemini,
            Provider::OpenCode,
            Provider::Pi,
            Provider::Windsurf,
        ] {
            assert!(provider_is_listed(p), "{p:?} is missing from the empty state");
        }
        // And each names a real directory rather than an empty string.
        assert!(provider_search_paths().iter().all(|(_, path)| !path.is_empty()));
    }

    #[test]
    fn pad_truncates_and_aligns() {
        assert_eq!(pad("abc", 5, false), "abc  ");
        assert_eq!(pad("abc", 5, true), "  abc");
        assert_eq!(pad("abcdefgh", 4, false).chars().count(), 4);
    }
}
