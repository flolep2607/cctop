//! Frame layout, drawing, and mouse hit-testing.

use super::columns::{self, COLUMNS, ColumnId};
use super::spark;
use super::theme::{self, Gradient};
use super::{AGE_OPTIONS, App, Mode, panels};
use crate::session::Surface;
use crate::util;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout as RLayout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};

/// Screen regions recorded during a draw, so mouse events can be mapped back to
/// what was actually rendered rather than to a guessed layout.
#[derive(Debug, Default, Clone)]
pub struct Layout {
    header_row: u16,
    rows_start: u16,
    rows_end: u16,
    tab_row: u16,
    bottom_start: u16,
    /// `(start_col, end_col, column)` spans in the table header.
    column_spans: Vec<(u16, u16, ColumnId)>,
    /// `(start_col, end_col, tab_index)` spans in the bottom tab bar.
    tab_spans: Vec<(u16, u16, usize)>,
    /// Tool Activity sidebar: `(x_end, y_start, first_index, row_count)`.
    tool_sidebar: Option<(u16, u16, usize, usize)>,
    /// Tool Activity log area: `(x_start, y_start, height)`.
    tool_log: Option<(u16, u16, u16)>,
}

impl Layout {
    pub fn in_bottom_panel(&self, row: u16) -> bool {
        row >= self.bottom_start
    }

    pub fn row_at(&self, row: u16) -> Option<usize> {
        (row >= self.rows_start && row < self.rows_end).then(|| (row - self.rows_start) as usize)
    }

    pub fn header_column_at(&self, col: u16, row: u16) -> Option<ColumnId> {
        if row != self.header_row {
            return None;
        }
        self.column_spans
            .iter()
            .find(|(a, b, _)| col >= *a && col < *b)
            .map(|(_, _, id)| *id)
    }

    /// Index of the tool-filter row under the cursor, if any.
    pub fn tool_sidebar_at(&self, col: u16, row: u16) -> Option<usize> {
        let (x_end, y_start, first, count) = self.tool_sidebar?;
        if col >= x_end || row < y_start {
            return None;
        }
        let offset = (row - y_start) as usize;
        (offset < count).then_some(first + offset)
    }

    /// Line offset within the tool log under the cursor, before scrolling.
    pub fn tool_log_row_at(&self, col: u16, row: u16) -> Option<usize> {
        let (x_start, y_start, height) = self.tool_log?;
        if col < x_start || row < y_start || row >= y_start + height {
            return None;
        }
        Some((row - y_start) as usize)
    }

    pub fn tab_at(&self, col: u16, row: u16) -> Option<usize> {
        if row != self.tab_row {
            return None;
        }
        self.tab_spans
            .iter()
            .find(|(a, b, _)| col >= *a && col < *b)
            .map(|(_, _, i)| *i)
    }
}

fn panel_block(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .title(Span::styled(format!(" {title} "), theme::title()))
}

pub fn draw(frame: &mut Frame, app: &mut App) -> Layout {
    let area = frame.area();
    let mut layout = Layout::default();

    // Overview and limits are fixed; the table and bottom panel split the rest,
    // with the bottom panel capped so the list never collapses to nothing.
    let body_height = area.height.saturating_sub(5 + 3 + 1);
    let bottom_height = ((body_height as f32 * 0.45) as u16)
        .clamp(8, 24)
        .min(body_height.saturating_sub(4));

    let chunks = RLayout::vertical([
        Constraint::Length(5),
        Constraint::Min(4),
        Constraint::Length(bottom_height),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(area);

    draw_overview(frame, chunks[0], app);
    draw_table(frame, chunks[1], app, &mut layout);
    draw_bottom(frame, chunks[2], app, &mut layout);
    draw_limits(frame, chunks[3], app);
    draw_footer(frame, chunks[4], app);

    match app.mode {
        Mode::Help => draw_help(frame, area),
        Mode::Search => draw_search(frame, area, app),
        Mode::SortBy => draw_sortby(frame, area, app),
        Mode::AgeFilter => draw_age_filter(frame, area, app),
        Mode::DeleteConfirm => draw_delete_confirm(frame, area, app),
        Mode::DeleteBlocked => draw_delete_blocked(frame, area, app),
        Mode::List => {}
    }
    layout
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

fn draw_overview(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel_block("Overview");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols =
        RLayout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(inner);
    let (left, right) = (cols[0], cols[1]);

    // The newest sample of the spend series is the spend in the last tick.
    let realtime = app.global_spend.values().last().copied().unwrap_or(0.0);
    let label_w = 15usize;
    let value_w = 10usize;
    let chart_w = left.width.saturating_sub((label_w + value_w + 2) as u16) as usize;

    let row = |name: &str, amount: f64, series: &[f64], now_idx: Option<usize>| -> Line<'static> {
        let mut spans = vec![
            Span::styled(format!("{name:<label_w$}"), theme::label()),
            Span::styled(
                format!("{:>value_w$} ", util::adaptive_usd(amount)),
                Style::default()
                    .fg(Color::Indexed(221))
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        spans.extend(spark::sparkline(series, chart_w, 0.0, Gradient::Spend, now_idx).spans);
        Line::from(spans)
    };

    let now = chrono::Local::now();
    let hour_idx = Some(chrono::Timelike::hour(&now) as usize);
    let day_idx = Some(chrono::Datelike::day(&now) as usize - 1);
    let rt_idx = app.global_spend.values().len().checked_sub(1);

    let left_lines = vec![
        row(
            "Real-time Spend",
            realtime,
            app.global_spend.values(),
            rt_idx,
        ),
        row(
            "Daily Spend",
            app.stats.spend_today,
            &app.stats.daily_hourly,
            hour_idx,
        ),
        row(
            "Monthly Spend",
            app.stats.spend_calendar_month,
            &app.stats.monthly_daily,
            day_idx,
        ),
    ];
    frame.render_widget(Paragraph::new(left_lines), left);

    let mem_mb = app.stats.total_memory as f64 / (1024.0 * 1024.0);
    let r_label_w = 12usize;
    let r_value_w = 9usize;
    let r_chart_w = right
        .width
        .saturating_sub((r_label_w + r_value_w + 2) as u16) as usize;

    let mut cpu_spans = vec![
        Span::styled(format!("{:<r_label_w$}", "Agents CPU"), theme::label()),
        Span::styled(
            format!("{:>r_value_w$} ", format!("{:.1}%", app.stats.total_cpu)),
            theme::value(),
        ),
    ];
    cpu_spans.extend(
        spark::sparkline(
            app.global_cpu.values(),
            r_chart_w,
            100.0,
            Gradient::Cpu,
            app.global_cpu.values().len().checked_sub(1),
        )
        .spans,
    );

    let right_lines = vec![
        Line::from(cpu_spans),
        Line::from(vec![
            Span::styled(format!("{:<r_label_w$}", "Agents Mem"), theme::label()),
            Span::styled(
                format!("{:>r_value_w$}", format!("{mem_mb:.0} MB")),
                theme::value(),
            ),
        ]),
        Line::from(vec![
            Span::styled(format!("{:<r_label_w$}", "Sessions"), theme::label()),
            Span::styled(
                format!("{:>r_value_w$}", app.stats.total.to_string()),
                theme::value(),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{} active", app.stats.active_1h),
                Style::default().fg(theme::COST_LOW),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(right_lines), right);
}

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

fn draw_table(frame: &mut Frame, area: Rect, app: &mut App, layout: &mut Layout) {
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
            "No sessions found in ~/.claude/projects or ~/.codex/sessions."
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, theme::dim()))),
            list_area,
        );
        return;
    }

    // Keep the cursor on screen.
    let height = list_area.height as usize;
    if app.selected < app.scroll {
        app.scroll = app.selected;
    } else if app.selected >= app.scroll + height {
        app.scroll = app.selected + 1 - height;
    }
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
            session_row(s, &widths, selected, &now)
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), list_area);
}

fn session_row(
    s: &crate::session::Session,
    widths: &[u16],
    selected: bool,
    now: &chrono::DateTime<chrono::Utc>,
) -> Line<'static> {
    let age_secs = util::parse_ts(&s.last_active).map(|d| (now.timestamp() - d.timestamp()).max(0));
    let base = if selected {
        Style::default()
            .bg(theme::SELECTED_BG)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let mut spans = Vec::with_capacity(COLUMNS.len() * 2);
    for (c, w) in COLUMNS.iter().zip(widths) {
        let text = columns::render_cell(c.id, s, now);
        // Selection styling wins so the highlighted row stays readable.
        let style = if selected {
            base
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
        ColumnId::Status => {
            if s.is_running() {
                theme::running_dot_color(age_secs)
            } else {
                theme::DIM
            }
        }
        ColumnId::Last => theme::age_color(age_secs, s.is_running()),
        ColumnId::Model => theme::model_color(&s.model),
        ColumnId::Project => match s.surface {
            Surface::DesktopCowork => theme::DESKTOP_COWORK,
            Surface::DesktopCode => theme::DESKTOP_CODE,
            Surface::Cli => Color::Reset,
        },
        ColumnId::Cost => s.total_cost.map(theme::cost_color).unwrap_or(theme::DIM),
        ColumnId::CostHour => {
            if s.cost_hour > 0.0 {
                theme::cost_color(s.cost_hour)
            } else {
                theme::DIMMER
            }
        }
        ColumnId::CostToday => {
            if s.cost_today > 0.0 {
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

// ---------------------------------------------------------------------------
// Bottom panels
// ---------------------------------------------------------------------------

fn draw_bottom(frame: &mut Frame, area: Rect, app: &mut App, layout: &mut Layout) {
    layout.bottom_start = area.y;
    layout.tab_row = area.y;
    layout.tool_sidebar = None;
    layout.tool_log = None;

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Tab bar drawn on the top border line, htop-style.
    let mut spans = vec![Span::raw(" ")];
    let mut pos = area.x + 2;
    layout.tab_spans.clear();
    for (i, name) in panels::TABS.iter().enumerate() {
        let style = if i == app.bottom_tab {
            theme::title().add_modifier(Modifier::UNDERLINED)
        } else {
            theme::dim()
        };
        spans.push(Span::styled((*name).to_string(), style));
        spans.push(Span::raw("  "));
        let w = name.chars().count() as u16;
        layout.tab_spans.push((pos, pos + w, i));
        pos += w + 2;
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect {
            x: area.x + 1,
            y: area.y,
            width: area.width.saturating_sub(2),
            height: 1,
        },
    );

    if app.selected_session().is_none() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No session selected",
                theme::dim(),
            ))),
            inner,
        );
        return;
    }

    // The Performance tab draws charts rather than text lines.
    if app.bottom_tab == 1 {
        if let Some(session) = app.selected_session() {
            draw_performance(frame, inner, session, &app.cpu_history, &app.mem_history);
        }
        return;
    }

    let mut tool_owners: Vec<Option<String>> = Vec::new();

    // Build the panel's lines under an immutable borrow, then release it before
    // touching the scroll state below. `Line<'static>` owns its text, so nothing
    // here keeps `app` borrowed — no per-frame clone of the session data needed.
    let (lines, scroll) = {
        let width = inner.width as usize;
        let Some(session) = app.selected_session() else {
            return;
        };
        let data = app.panel_data.as_ref();
        match app.bottom_tab {
            0 => (panels::info(session, data, app.plan), app.info_scroll),
            2 => (panels::processes(session, width), app.proc_scroll),
            3 => {
                let live = app.tool_live_only.then_some(app.started_at.as_str());
                match data {
                    Some(d) => {
                        let (lines, owners) =
                            draw_tool_sidebar(frame, inner, app, d, live, width, layout);
                        tool_owners = owners;
                        (lines, app.tool_scroll)
                    }
                    None => (vec![Line::from(Span::styled("Loading…", theme::dim()))], 0),
                }
            }
            4 => (
                panels::subagents(data, app.subagent_sort.0, app.subagent_sort.1, width),
                app.subagent_scroll,
            ),
            5 => (panels::cost(session, data, app.plan), app.cost_scroll),
            _ => (panels::config(session), app.config_scroll),
        }
    };

    // The tool tab renders its own split; everything else fills the panel.
    let target = if app.bottom_tab == 3 {
        Rect {
            x: inner.x + TOOL_SIDEBAR_W + 1,
            width: inner.width.saturating_sub(TOOL_SIDEBAR_W + 1),
            ..inner
        }
    } else {
        inner
    };

    let max_scroll = (lines.len() as u16).saturating_sub(target.height);
    // Tool Activity follows its tail unless the user has scrolled away.
    let scroll = if app.bottom_tab == 3 {
        app.tool_owners = std::mem::take(&mut tool_owners);
        layout.tool_log = Some((target.x, target.y, target.height));
        app.tool_max_scroll = max_scroll;
        if app.tool_follow {
            app.tool_scroll = max_scroll;
        }
        app.tool_scroll.min(max_scroll)
    } else {
        scroll.min(max_scroll)
    };
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), target);
}

const TOOL_SIDEBAR_W: u16 = 18;

/// Draw the per-tool sidebar and return the invocation lines for the main area.
#[allow(clippy::too_many_arguments)]
fn draw_tool_sidebar(
    frame: &mut Frame,
    inner: Rect,
    app: &App,
    data: &crate::session::SessionData,
    live: Option<&str>,
    width: usize,
    layout: &mut Layout,
) -> (Vec<Line<'static>>, Vec<Option<String>>) {
    let tabs = panels::tool_tabs(data);
    // Keep the selected filter on screen when the list is longer than the panel.
    let first = app.tool_tab.saturating_sub(inner.height as usize / 2);
    let visible = tabs.len().saturating_sub(first).min(inner.height as usize);
    layout.tool_sidebar = Some((inner.x + TOOL_SIDEBAR_W, inner.y, first, visible));
    let lines: Vec<Line> = tabs
        .iter()
        .enumerate()
        .skip(first)
        .take(inner.height as usize)
        .map(|(i, (name, count))| {
            let selected = i == app.tool_tab;
            let display = util::pretty_mcp_name(name);
            let count_str = count.to_string();
            let name_w = (TOOL_SIDEBAR_W as usize).saturating_sub(count_str.len() + 2);
            Line::from(vec![
                Span::styled(
                    format!("{:<name_w$}", util::truncate(&display, name_w)),
                    if selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        theme::dim()
                    },
                ),
                Span::raw(" "),
                Span::styled(count_str, theme::dim()),
            ])
        })
        .collect();

    frame.render_widget(
        Paragraph::new(lines),
        Rect {
            width: TOOL_SIDEBAR_W,
            ..inner
        },
    );
    frame.render_widget(
        Paragraph::new(
            (0..inner.height)
                .map(|_| Line::from(Span::styled("│", Style::default().fg(theme::DIMMER))))
                .collect::<Vec<_>>(),
        ),
        Rect {
            x: inner.x + TOOL_SIDEBAR_W,
            width: 1,
            ..inner
        },
    );

    panels::tool_activity(
        data,
        app.tool_tab,
        live,
        app.tool_show_diff,
        app.tool_expanded.as_deref(),
        width.saturating_sub(TOOL_SIDEBAR_W as usize + 1),
    )
}

fn draw_performance(
    frame: &mut Frame,
    inner: Rect,
    session: &crate::session::Session,
    cpu_history: &std::collections::HashMap<String, spark::History>,
    mem_history: &std::collections::HashMap<String, spark::History>,
) {
    if session.surface == Surface::DesktopCowork {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "Cowork sessions run in a cloud VM.",
                    theme::dim(),
                )),
                Line::from(Span::styled(
                    "No local CPU or memory metrics are available.",
                    theme::dim(),
                )),
            ]),
            inner,
        );
        return;
    }
    let Some(pm) = &session.process else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Performance data is only available for running sessions.",
                theme::dim(),
            ))),
            inner,
        );
        return;
    };

    let key = session.key();
    let empty = spark::History::default();
    let cpu = cpu_history.get(&key).unwrap_or(&empty);
    let mem = mem_history.get(&key).unwrap_or(&empty);
    let mem_mb = pm.memory as f64 / (1024.0 * 1024.0);
    let mem_max = util::nice_max(mem.values().iter().cloned().fold(1.0, f64::max));

    // A gutter keeps the CPU plot from butting against the memory axis labels.
    let cols = RLayout::horizontal([
        Constraint::Percentage(50),
        Constraint::Length(2),
        Constraint::Percentage(50),
    ])
    .split(inner);
    let (cpu_area, mem_area) = (cols[0], cols[2]);
    let rows = inner.height.saturating_sub(2).max(2) as usize;
    // A shared gutter keeps the two plots' data columns aligned.
    let axis_w = format!("{}", mem_max.ceil() as i64).len().max(3) + 2;

    let mut left = vec![Line::from(vec![
        Span::styled("CPU ", theme::label()),
        Span::styled(
            format!("{:>6.1}%", pm.cpu),
            Style::default().fg(theme::cpu_color(pm.cpu)),
        ),
        Span::raw("   "),
        Span::styled("PIDs ", theme::label()),
        Span::styled(pm.pids.to_string(), theme::value()),
    ])];
    left.extend(spark::line_chart(
        cpu.values(),
        cpu_area.width as usize,
        rows,
        100.0,
        Gradient::Cpu,
        Some(axis_w),
    ));

    let mut right = vec![Line::from(vec![
        Span::styled("Mem ", theme::label()),
        Span::styled(format!("{mem_mb:>8.0} MB"), theme::value()),
    ])];
    right.extend(spark::line_chart(
        mem.values(),
        mem_area.width as usize,
        rows,
        mem_max,
        Gradient::Accent,
        Some(axis_w),
    ));

    frame.render_widget(Paragraph::new(left), cpu_area);
    frame.render_widget(Paragraph::new(right), mem_area);
}

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

fn draw_limits(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel_block("Limits");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols =
        RLayout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(inner);

    for (i, (name, status)) in [("Claude", &app.quota.claude), ("Codex", &app.quota.codex)]
        .iter()
        .enumerate()
    {
        let mut spans = vec![Span::styled(format!("{name} "), theme::label())];
        // Each failure mode gets its own message: an expired sign-in needs the
        // user to act, a rate-limit clears on its own, and "not signed in" is
        // neither. One catch-all string would hide all of that.
        match status {
            crate::quota::ProviderStatus::Pending => {
                spans.push(Span::styled("checking…", theme::dim()));
            }
            crate::quota::ProviderStatus::NotSignedIn => {
                spans.push(Span::styled("not signed in", theme::dim()));
            }
            crate::quota::ProviderStatus::ApiBilling => {
                spans.push(Span::styled("API billing, no limits", theme::dim()));
            }
            crate::quota::ProviderStatus::Expired => {
                let cmd = if *name == "Codex" {
                    "codex login"
                } else {
                    "claude login"
                };
                spans.push(Span::styled(
                    "sign-in expired — ",
                    Style::default().fg(theme::COST_MID),
                ));
                spans.push(Span::styled(cmd.to_string(), theme::value()));
            }
            crate::quota::ProviderStatus::RateLimited { retry_at } => {
                spans.push(Span::styled(
                    "rate limited",
                    Style::default().fg(theme::COST_MID),
                ));
                if let Some(at) = retry_at {
                    let remaining = at - chrono::Utc::now().timestamp();
                    if remaining > 0 {
                        spans.push(Span::styled(
                            format!(" — retry in {}m{:02}s", remaining / 60, remaining % 60),
                            theme::dim(),
                        ));
                    }
                }
            }
            crate::quota::ProviderStatus::Unavailable(reason) => {
                spans.push(Span::styled(
                    format!("unavailable ({reason})"),
                    theme::dim(),
                ));
            }
            crate::quota::ProviderStatus::Ok(q) => {
                if let Some(plan) = &q.plan {
                    spans.push(Span::styled(format!("({plan}) "), theme::dim()));
                }
                for w in &q.windows {
                    let color = if w.pct >= 90 {
                        theme::COST_HIGH
                    } else if w.pct >= 70 {
                        theme::COST_MID
                    } else {
                        theme::COST_LOW
                    };
                    spans.push(Span::styled(format!("{} ", w.label), theme::label()));
                    spans.push(Span::styled(
                        format!("{:>3}% ", w.pct),
                        Style::default().fg(color),
                    ));
                    let filled = (w.pct as usize * 8 / 100).min(8);
                    spans.push(Span::styled(
                        "\u{2501}".repeat(filled),
                        Style::default().fg(color),
                    ));
                    spans.push(Span::styled(
                        "\u{2500}".repeat(8 - filled),
                        Style::default().fg(Color::Indexed(244)),
                    ));
                    if let Some(reset) = w.resets_at {
                        let remaining = reset - chrono::Utc::now().timestamp();
                        if remaining > 0 {
                            spans.push(Span::styled(
                                format!(" {}h{:02}m", remaining / 3600, (remaining % 3600) / 60),
                                theme::dim(),
                            ));
                        }
                    }
                    spans.push(Span::raw("  "));
                }
                if q.limit_reached {
                    spans.push(Span::styled(
                        "\u{26a0} limit",
                        Style::default().fg(theme::COST_HIGH),
                    ));
                }
            }
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), cols[i]);
    }
}

// ---------------------------------------------------------------------------
// Footer
// ---------------------------------------------------------------------------

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    if let Some((msg, _)) = &app.status {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                msg.clone(),
                Style::default().fg(theme::COST_LOW),
            ))),
            area,
        );
        return;
    }

    let key_style = Style::default()
        .fg(Color::Black)
        .bg(theme::ACCENT)
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(theme::DIM);

    let mut spans = Vec::new();
    for (key, name) in [
        ("F1", "Help"),
        ("F3", "Filter"),
        ("F5", "Refresh"),
        ("F6", "Sort"),
        ("F7", "Age"),
        ("←→", "Panel"),
        ("`", "Live"),
        ("y", "Copy"),
        ("d", "Delete"),
        ("F10", "Quit"),
    ] {
        spans.push(Span::styled(key, key_style));
        spans.push(Span::styled(format!("{name} "), label_style));
    }
    if let Some(age) = app.age_filter {
        spans.push(Span::styled(
            format!(" Age<{} ", age.short()),
            Style::default().fg(theme::PANEL_TITLE),
        ));
    }
    if !app.search.is_empty() {
        spans.push(Span::styled(
            format!(" Filter: {} ", app.search),
            Style::default().fg(Color::Cyan),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ---------------------------------------------------------------------------
// Modals
// ---------------------------------------------------------------------------

/// A centred rectangle of the given size, clamped to the screen.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

fn modal(frame: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>, width: u16) {
    let height = lines.len() as u16 + 2;
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER_HI))
        .title(Span::styled(format!(" {title} "), theme::title()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let section = |t: &str| Line::from(Span::styled(t.to_string(), theme::title()));
    let item = |k: &str, d: &str| {
        Line::from(vec![
            Span::styled(format!("  {k:<16}"), Style::default().fg(theme::ACCENT)),
            Span::raw(d.to_string()),
        ])
    };
    let lines = vec![
        section("Navigation"),
        item("↑/k  ↓/j", "Move between sessions"),
        item("PgUp / PgDn", "Page through the list"),
        item("Home / End", "Jump to first / last"),
        Line::default(),
        section("Panels"),
        item("←  →", "Move between bottom panels"),
        item("Tab / Shift+Tab", "Same, either direction"),
        item("1 – 7", "Jump to a panel directly"),
        item("Shift+↑ / ↓", "Scroll inside the active panel"),
        item("L", "Toggle the Tool Activity live filter"),
        item("v", "Toggle inline diffs for edits"),
        Line::default(),
        section("Filter and sort"),
        item("/ or F3", "Filter sessions by text"),
        item("F6  >  <", "Open the sort-by panel"),
        item("F7", "Filter by age (1d / 1w / 1mo)"),
        item("`", "Show only running sessions"),
        item("P / M / T", "Sort by status / memory / cost"),
        item("Esc", "Clear the active filter"),
        Line::default(),
        section("Other"),
        item("y", "Copy resume command or transcript path"),
        item("d", "Delete the selected session (not running)"),
        item("r or F5", "Refresh now"),
        item("q or F10", "Quit"),
        Line::default(),
        Line::from(Span::styled(
            "  Costs are estimates from published per-token rates. Flat-rate plans",
            theme::dim(),
        )),
        Line::from(Span::styled(
            "  (Max, Pro, Team) bill differently, so these may not match your invoice.",
            theme::dim(),
        )),
        Line::default(),
        Line::from(Span::styled("  Press any key to return", theme::dim())),
    ];
    modal(frame, area, "Help", lines, 76);
}

fn draw_search(frame: &mut Frame, area: Rect, app: &App) {
    let lines = vec![
        Line::from(vec![
            Span::raw(" > "),
            Span::styled(
                app.search.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(theme::ACCENT)),
        ]),
        Line::from(Span::styled(
            format!(
                " {} match{}   Enter/Esc to close",
                app.visible.len(),
                if app.visible.len() == 1 { "" } else { "es" }
            ),
            theme::dim(),
        )),
    ];
    modal(frame, area, "Filter sessions", lines, 54);
}

fn draw_sortby(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = COLUMNS
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let active = c.id == app.sort_col;
            let arrow = if active {
                if app.sort_asc { " ▲" } else { " ▼" }
            } else {
                ""
            };
            let text = format!(" {}{}", c.label.trim(), arrow);
            let style = if i == app.sortby_cursor {
                Style::default()
                    .bg(theme::SELECTED_BG)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if active {
                Style::default().fg(theme::ACCENT)
            } else {
                Style::default()
            };
            Line::from(Span::styled(format!("{text:<26}"), style))
        })
        .collect();
    // Explain the highlighted column, since several are non-obvious ($/1H, CTX%).
    lines.push(Line::default());
    for part in COLUMNS[app.sortby_cursor].desc.lines() {
        lines.push(Line::from(Span::styled(format!(" {part}"), theme::dim())));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " Enter select   Esc cancel",
        theme::dim(),
    )));
    modal(frame, area, "Sort by", lines, 54);
}

fn draw_age_filter(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = AGE_OPTIONS
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let active = *opt == app.age_filter;
            let marker = if active { "●" } else { "○" };
            let text = opt.map(|o| o.label()).unwrap_or("No filter");
            let style = if i == app.age_cursor {
                Style::default()
                    .bg(theme::SELECTED_BG)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(
                    format!(" {marker} "),
                    Style::default().fg(if active {
                        theme::COST_LOW
                    } else {
                        theme::DIMMER
                    }),
                ),
                Span::styled(format!("{text:<22}"), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        " ↑/↓  Enter apply  Esc cancel",
        theme::dim(),
    )));
    modal(frame, area, "Show sessions active within", lines, 34);
}

fn draw_delete_confirm(frame: &mut Frame, area: Rect, app: &App) {
    let Some(s) = app.selected_session() else {
        return;
    };
    let lines = vec![
        Line::from(Span::styled(
            format!("  {}", s.display_label()),
            theme::value(),
        )),
        Line::from(Span::styled(format!("  {}", s.session_id), theme::dim())),
        Line::default(),
        Line::from(Span::styled(
            "  This permanently removes the transcript from disk.",
            Style::default().fg(theme::COST_MID),
        )),
        Line::default(),
        Line::from(Span::styled(
            "  [y] delete    [n / Esc] cancel",
            theme::dim(),
        )),
    ];
    modal(frame, area, "Delete session?", lines, 62);
}

fn draw_delete_blocked(frame: &mut Frame, area: Rect, app: &App) {
    let Some(s) = app.selected_session() else {
        return;
    };
    let lines = vec![
        Line::from(Span::styled(
            format!("  {}", s.display_label()),
            theme::value(),
        )),
        Line::default(),
        Line::from(Span::raw("  This session is still running.")),
        Line::from(Span::raw("  Stop the agent first, then delete it.")),
        Line::default(),
        Line::from(Span::styled("  [any key] dismiss", theme::dim())),
    ];
    modal(frame, area, "Cannot delete", lines, 62);
}

// ---------------------------------------------------------------------------
// Clipboard
// ---------------------------------------------------------------------------

/// Copy text via a platform helper, falling back to the OSC 52 escape sequence.
///
/// OSC 52 works over SSH and inside multiplexers where no local clipboard tool
/// exists, so it's the last resort rather than the first choice.
pub fn copy_to_clipboard(text: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    const HELPERS: &[(&str, &[&str])] = &[
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
        ("pbcopy", &[]),
        ("clip.exe", &[]),
    ];

    for (cmd, args) in HELPERS {
        let Ok(mut child) = Command::new(cmd)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(stdin) = child.stdin.as_mut()
            && stdin.write_all(text.as_bytes()).is_ok()
        {
            drop(child.stdin.take());
            if child.wait().map(|s| s.success()).unwrap_or(false) {
                return;
            }
        }
    }

    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", util::b64_encode(text.as_bytes()));
    let _ = out.flush();
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

    #[test]
    fn hit_testing_maps_regions() {
        let layout = Layout {
            header_row: 6,
            rows_start: 7,
            rows_end: 12,
            tab_row: 20,
            bottom_start: 20,
            column_spans: vec![(0, 5, ColumnId::Status), (5, 12, ColumnId::Cost)],
            tab_spans: vec![(2, 6, 0), (8, 19, 1)],
            tool_sidebar: Some((18, 21, 0, 3)),
            tool_log: Some((19, 21, 4)),
        };
        assert_eq!(layout.row_at(7), Some(0));
        assert_eq!(layout.row_at(11), Some(4));
        assert_eq!(layout.row_at(12), None);
        assert_eq!(layout.header_column_at(6, 6), Some(ColumnId::Cost));
        assert_eq!(layout.header_column_at(6, 7), None);
        assert_eq!(layout.tab_at(9, 20), Some(1));
        assert_eq!(layout.tab_at(9, 21), None);
        assert!(layout.in_bottom_panel(20));
        assert!(!layout.in_bottom_panel(19));
        // Sidebar clicks map to a tool filter; clicks past its right edge don't.
        assert_eq!(layout.tool_sidebar_at(4, 22), Some(1));
        assert_eq!(layout.tool_sidebar_at(4, 24), None);
        assert_eq!(layout.tool_sidebar_at(40, 22), None);
        // Log clicks resolve to a line offset; the sidebar column is excluded.
        assert_eq!(layout.tool_log_row_at(30, 21), Some(0));
        assert_eq!(layout.tool_log_row_at(30, 24), Some(3));
        assert_eq!(layout.tool_log_row_at(30, 25), None);
        assert_eq!(layout.tool_log_row_at(5, 22), None);
    }

    #[test]
    fn centered_rect_never_exceeds_screen() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 10,
        };
        let r = centered(area, 100, 100);
        assert!(r.width <= area.width && r.height <= area.height);
        assert!(r.x + r.width <= area.width);
    }
}
