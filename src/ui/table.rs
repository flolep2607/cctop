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
    let fixed: u16 =
        cols.iter().filter_map(|c| c.width).sum::<u16>() + (cols.len().saturating_sub(1)) as u16; // single-space gutters
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
        (
            "Gemini CLI",
            config::GEMINI_CHATS_ROOT.display().to_string(),
        ),
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
    // Once anything is filtered, the count that matters is how much of the
    // table is being hidden — "Sessions (54)" over six rows reads as a bug.
    let title = match (app.live_only, app.visible.len() != app.sessions.len()) {
        (true, _) => format!(
            "Sessions ({}/{}) — live",
            app.visible.len(),
            app.sessions.len()
        ),
        (false, true) => format!("Sessions ({}/{})", app.visible.len(), app.sessions.len()),
        (false, false) => format!("Sessions ({})", app.sessions.len()),
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
    let query = app.search.to_ascii_lowercase();
    let lines: Vec<Line> = app
        .visible
        .iter()
        .skip(app.scroll)
        .take(height)
        .enumerate()
        .map(|(i, &row)| {
            let s = &app.sessions[row.session()];
            let selected = app.scroll + i == app.selected;
            let key = s.key();
            match row {
                crate::ui::Row::Session(_) => session_row(
                    s,
                    &cols,
                    &widths,
                    &RowState {
                        selected,
                        marked: app.marked.contains(&key),
                        deleting: app.deleting.contains(&key),
                        rang: app.notify.rang_recently(&key),
                        query: &query,
                        // Only sessions that have subagents get a marker, so the
                        // glyph is an offer rather than decoration on every row.
                        expand: match (s.subagents.is_empty(), app.is_expanded(s)) {
                            (true, _) => None,
                            (false, true) => Some('▾'),
                            (false, false) => Some('▸'),
                        },
                    },
                    &now,
                ),
                crate::ui::Row::Subagent { index, .. } => match s.subagents.get(index) {
                    Some(sub) => subagent_row(
                        sub,
                        &cols,
                        &widths,
                        selected,
                        index + 1 == s.subagents.len(),
                        &now,
                    ),
                    None => Line::default(),
                },
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), list_area);
}

/// Split a cell around the active query, so the matching run can be picked out.
///
/// A filtered table shows the rows that matched but not *why* — with a query
/// over five columns, the reason is often a cell nobody was looking at. This
/// makes the cause visible without a column of its own.
///
/// `query` is lowercase; matching folds case on the cell's side. Anything that
/// is not a match keeps `base` exactly, padding included, so highlighting never
/// changes a row's width or colour.
fn highlight(text: String, query: &str, base: Style) -> Vec<Span<'static>> {
    if query.is_empty() {
        return vec![Span::styled(text, base)];
    }
    let lower = text.to_ascii_lowercase();
    let Some(at) = lower.find(query) else {
        return vec![Span::styled(text, base)];
    };
    // ASCII-lowercasing preserves byte length, so offsets carry across — but
    // only if the query itself is ASCII. A non-ASCII query can fold to a
    // different length, and slicing `text` at the wrong offset would panic.
    if !query.is_ascii() || !text.is_char_boundary(at) || !text.is_char_boundary(at + query.len()) {
        return vec![Span::styled(text, base)];
    }
    let hit = base
        .fg(theme::colors().accent)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    vec![
        Span::styled(text[..at].to_string(), base),
        Span::styled(text[at..at + query.len()].to_string(), hit),
        Span::styled(text[at + query.len()..].to_string(), base),
    ]
}

/// What the table knows about a row beyond the session on it.
struct RowState<'a> {
    selected: bool,
    marked: bool,
    /// This session rang the bell a moment ago.
    rang: bool,
    /// Its deletion has been accepted but not yet confirmed.
    deleting: bool,
    /// The active filter, lowercased, for marking the cells that matched.
    query: &'a str,
    /// The expansion marker, for a session that has subagents.
    expand: Option<char>,
}

fn session_row(
    s: &crate::session::Session,
    cols: &[&'static columns::Column],
    widths: &[u16],
    row: &RowState,
    now: &chrono::DateTime<chrono::Utc>,
) -> Line<'static> {
    let &RowState {
        selected,
        marked,
        rang,
        deleting,
        query,
        expand,
    } = row;
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
        } else if c.id == ColumnId::Project {
            // Prefixed on the label rather than given a column of its own: one
            // more column costs every row two cells of width to serve the few
            // rows that have children.
            match expand {
                Some(glyph) => format!("{glyph} {}", columns::render_cell(c.id, s, now)),
                None => columns::render_cell(c.id, s, now),
            }
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
        // The status dot is a glyph, not text, and the numeric columns match a
        // query only by coincidence — highlighting "42" inside a cost because
        // the query was "42" is noise. The columns worth marking are the ones
        // the filter is actually aimed at.
        let searchable = matches!(
            c.id,
            ColumnId::Project
                | ColumnId::Model
                | ColumnId::Harness
                | ColumnId::Host
                | ColumnId::Branch
        );
        let padded = pad(&text, *w, c.right_align);
        if searchable && !deleting {
            spans.extend(highlight(padded, query, style));
        } else {
            spans.push(Span::styled(padded, style));
        }
        spans.push(Span::styled(" ", base));
    }
    Line::from(spans)
}

/// A subagent's line, indented under the session that spawned it.
///
/// Dimmer than a session row throughout, so an expanded session still reads as
/// one block with a heading rather than as several peers: the child rows are
/// detail about the row above them, not more sessions.
fn subagent_row(
    sub: &crate::session::Subagent,
    cols: &[&'static columns::Column],
    widths: &[u16],
    selected: bool,
    last: bool,
    now: &chrono::DateTime<chrono::Utc>,
) -> Line<'static> {
    let running = matches!(sub.status, crate::session::SubagentStatus::Running);
    let base = if selected {
        Style::default()
            .bg(theme::colors().selected_bg)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::colors().dim)
    };

    let mut spans = Vec::with_capacity(cols.len() * 2);
    for (c, w) in cols.iter().zip(widths) {
        let text = columns::render_subagent_cell(c.id, sub, last, now);
        // The status dot keeps its colour on the selected row for the same
        // reason a session's does: it is the one cell whose colour *is* the
        // information. A ghost's transcript is gone, so its dot is hollow.
        let style = match c.id {
            ColumnId::Status => {
                let fg = if sub.ghost {
                    theme::colors().dimmer
                } else if running {
                    theme::colors().cost_low
                } else {
                    theme::colors().dim
                };
                if selected {
                    Style::default()
                        .bg(theme::colors().selected_bg)
                        .fg(fg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(fg)
                }
            }
            _ => base,
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
                s.total_cost
                    .map(theme::cost_color)
                    .unwrap_or(theme::colors().dim)
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
            _ if s.is_compacting() => theme::colors().cost_high,
            // Dim once compacted and stopped: the percentage is real but it
            // measures a window the session has already thrown away.
            Some(c) if c.compacted => theme::colors().dimmer,
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
        // The only cell here whose colour is a warning rather than a
        // measurement. An agent that asks about nothing is the thing worth
        // catching across a screen of rows, so it gets the hot end of the
        // scale; the modes that do ask stay quiet, because they are the norm
        // and colouring the norm is how a warning stops being read.
        ColumnId::Permission => match s.permission {
            Some(p) if p.is_unrestricted() => theme::colors().cost_high,
            Some(_) => theme::colors().dim,
            None => theme::colors().dimmer,
        },
        // A few failed calls is ordinary — a grep that found nothing, a build
        // that caught a mistake. A quarter of them is a session that has stopped
        // making progress and is still being billed for the attempt.
        ColumnId::Errors => match s.error_rate() {
            Some(r) if r >= 0.25 => theme::colors().cost_high,
            Some(r) if r >= 0.10 => theme::colors().cost_mid,
            Some(r) if r > 0.0 => theme::colors().dim,
            _ => theme::colors().dimmer,
        },
        // A remote row is dimmer throughout its identifying cells, so a table
        // spanning machines still reads as "here, plus elsewhere" at a glance.
        ColumnId::Host => match s.remote {
            Some(_) => theme::colors().accent,
            None => theme::colors().dimmer,
        },
        // The other warning colour, and the only one about a second session:
        // hot once two agents have written the same file, amber while they are
        // merely in the same repository and have not met yet.
        ColumnId::Conflict => match s.conflict {
            Some(crate::collide::Overlap::File) => theme::colors().cost_high,
            Some(crate::collide::Overlap::Directory) => theme::colors().cost_mid,
            None => Color::Reset,
        },
        _ => Color::Reset,
    }
}

#[cfg(test)]
mod tests {
    use super::columns::COLUMNS;
    use super::*;

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

        let row = |rang| RowState {
            selected: false,
            marked: false,
            rang,
            deleting: false,
            query: "",
            expand: None,
        };
        let quiet = session_row(&s, &cols, &widths, &row(false), &now);
        let rang = session_row(&s, &cols, &widths, &row(true), &now);
        assert_eq!(quiet.spans[0].content, "○ ");
        assert_eq!(rang.spans[0].content, "◉ ");
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
            assert!(
                provider_is_listed(p),
                "{p:?} is missing from the empty state"
            );
        }
        // And each names a real directory rather than an empty string.
        assert!(
            provider_search_paths()
                .iter()
                .all(|(_, path)| !path.is_empty())
        );
    }

    /// Which rows are children has to be readable at the left edge. The tree
    /// glyph saying so lives out in the Project column, and a subagent's dot
    /// used to sit in the same cell as its parent's — so the only way to tell
    /// what a row was meant reading across the whole width and back.
    #[test]
    fn a_subagents_dot_is_indented_under_its_parents() {
        let now = chrono::Utc::now();
        let cols = all_columns();
        let widths = column_widths(&cols, 200);
        let mut s = crate::session::Session::new(Provider::Claude, "a".into());
        s.last_active = now.to_rfc3339();
        let parent = session_row(
            &s,
            &cols,
            &widths,
            &RowState {
                selected: false,
                marked: false,
                rang: false,
                deleting: false,
                query: "",
                expand: Some('▾'),
            },
            &now,
        );

        let sub = crate::session::Subagent {
            agent_id: "sub-1".into(),
            agent_type: "general-purpose".into(),
            description: "Review performance".into(),
            model: "claude-opus-5".into(),
            started_at: None,
            last_active: None,
            duration_ms: 0,
            status: crate::session::SubagentStatus::Done,
            cost: 0.0,
            tool_count: 0,
            tool_use_id: None,
            context: None,
            ghost: false,
        };
        let child = subagent_row(&sub, &cols, &widths, false, true, &now);

        assert_eq!(
            parent.spans[0].content, "○ ",
            "a session starts at column 0"
        );
        assert_eq!(child.spans[0].content, " ○", "a subagent starts one in");
        // Indenting must not cost the row its alignment: every other column has
        // to stay under the same header as the parent's.
        let width = |line: &Line| -> usize {
            line.spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>()
        };
        assert_eq!(width(&parent), width(&child));
    }

    /// Highlighting marks the match and changes nothing else — a row that
    /// shifted by a character under the cursor would be worse than no
    /// highlight at all.
    #[test]
    fn highlighting_marks_the_match_without_moving_the_cell() {
        let base = Style::default();
        let cell = pad("Billing API", 20, false);

        let spans = highlight(cell.clone(), "api", base);
        let rebuilt: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rebuilt, cell, "the cell must survive intact");
        assert_eq!(spans[1].content, "API", "case folds on the cell's side");
        assert_eq!(spans[1].style.fg, Some(theme::colors().accent));

        // No match, no query, and a query that is not in the cell all leave one
        // plain span behind.
        for query in ["", "kingfisher"] {
            let spans = highlight(cell.clone(), query, base);
            assert_eq!(spans.len(), 1);
            assert_eq!(spans[0].style.fg, None);
        }
    }

    /// Regression guard: byte offsets from an ASCII-lowercased copy only line
    /// up while the query is ASCII, and slicing on a bad boundary panics.
    #[test]
    fn highlighting_a_multibyte_cell_does_not_slice_a_character() {
        let base = Style::default();
        let spans = highlight("café ▸ api".to_string(), "api", base);
        let rebuilt: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rebuilt, "café ▸ api");
        assert_eq!(spans[1].content, "api");

        // A non-ASCII query is left unmarked rather than risked.
        let spans = highlight("café".to_string(), "café", base);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn pad_truncates_and_aligns() {
        assert_eq!(pad("abc", 5, false), "abc  ");
        assert_eq!(pad("abc", 5, true), "  abc");
        assert_eq!(pad("abcdefgh", 4, false).chars().count(), 4);
    }
}
