//! Frame layout, drawing, and mouse hit-testing.

use super::columns::ColumnId;
use super::modals;
use super::spark;
use super::table;
use super::theme::{self, Gradient};
use super::{App, Mode, panels};
use crate::pricing::Provider;
use crate::session::Surface;
use crate::util;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout as RLayout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};

/// Screen regions recorded during a draw, so mouse events can be mapped back to
/// what was actually rendered rather than to a guessed layout.
#[derive(Debug, Default, Clone)]
pub struct Layout {
    pub(super) header_row: u16,
    pub(super) rows_start: u16,
    pub(super) rows_end: u16,
    pub(super) tab_row: u16,
    pub(super) bottom_start: u16,
    /// `(start_col, end_col, column)` spans in the table header.
    pub(super) column_spans: Vec<(u16, u16, ColumnId)>,
    /// `(start_col, end_col, tab_index)` spans in the bottom tab bar.
    pub(super) tab_spans: Vec<(u16, u16, usize)>,
    /// Tool Activity sidebar: `(x_end, y_start, first_index, row_count)`.
    pub(super) tool_sidebar: Option<(u16, u16, usize, usize)>,
    /// Tool Activity log area: `(x_start, y_start, height)`.
    pub(super) tool_log: Option<(u16, u16, u16)>,
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

pub(super) fn panel_block(title: &str) -> Block<'static> {
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
        // Four spend rows plus the border.
        Constraint::Length(6),
        Constraint::Min(4),
        Constraint::Length(bottom_height),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(area);

    draw_overview(frame, chunks[0], app);
    table::draw_table(frame, chunks[1], app, &mut layout);
    draw_bottom(frame, chunks[2], app, &mut layout);
    draw_limits(frame, chunks[3], app);
    draw_footer(frame, chunks[4], app);

    match app.mode {
        Mode::Help => modals::draw_help(frame, area),
        Mode::Search => modals::draw_search(frame, area, app),
        Mode::SortBy => modals::draw_sortby(frame, area, app),
        Mode::AgeFilter => modals::draw_age_filter(frame, area, app),
        Mode::DeleteConfirm => modals::draw_delete_confirm(frame, area, app),
        Mode::DeleteBlocked => modals::draw_delete_blocked(frame, area, app),
        Mode::KillConfirm => modals::draw_kill_confirm(frame, area, app),
        Mode::KillBlocked => modals::draw_kill_blocked(frame, area, app),
        Mode::BatchConfirm => modals::draw_batch_confirm(frame, area, app),
        Mode::BatchDeleteBlocked => modals::draw_batch_blocked(frame, area, app, true),
        Mode::BatchKillBlocked => modals::draw_batch_blocked(frame, area, app, false),
        Mode::CostFilter => modals::draw_cost_filter(frame, area, app),
        Mode::SendKeys => modals::draw_send_keys(frame, area, app),
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

    let realtime = app.stats.spend_per_min;
    let label_w = 15usize;
    let value_w = 12usize;
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
            "Live Spend/min",
            realtime,
            app.global_spend.values(),
            rt_idx,
        ),
        row(
            "Today Spend",
            app.stats.spend_today,
            &app.stats.daily_hourly,
            hour_idx,
        ),
        row(
            "Month-to-date",
            app.stats.spend_calendar_month,
            &app.stats.monthly_daily,
            day_idx,
        ),
        // Every session ever recorded, across every provider. Deliberately without
        // a sparkline: the others chart a window that scrolls, and a running total
        // only ever climbs, so a chart of it says nothing the number doesn't.
        Line::from(vec![
            Span::styled(format!("{:<label_w$}", "Total Spend"), theme::label()),
            Span::styled(
                format!("{:>value_w$}", util::adaptive_usd(app.stats.spend_total)),
                Style::default()
                    .fg(Color::Indexed(221))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
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
                format!("{} active", app.stats.running),
                Style::default().fg(theme::COST_LOW),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(right_lines), right);
}

// ---------------------------------------------------------------------------
// Bottom panels
// ---------------------------------------------------------------------------

fn draw_bottom(frame: &mut Frame, area: Rect, app: &mut App, layout: &mut Layout) {
    app.ensure_available_tab();
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
        if !app.tab_available(i) {
            continue;
        }
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
    if session.surface == Surface::Editor && session.provider == Provider::Cursor {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "Cursor uses a shared editor process.",
                    theme::dim(),
                )),
                Line::from(Span::styled(
                    "No per-session CPU or memory metrics are available.",
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

/// Colour quota usage by whether it is being spent faster than an even budget
/// across its reset window. Falling back to absolute pressure keeps windows
/// useful when a provider omits either its reset time or duration.
fn quota_color(window: &crate::quota::Window, now: i64) -> Color {
    if let (Some(duration), Some(reset)) = (window.duration, window.resets_at) {
        let duration_secs = duration.as_secs() as i64;
        let elapsed_secs = (now - (reset - duration_secs)).clamp(1, duration_secs);
        let pace_ratio = window.pct as f64 * duration_secs as f64 / (100.0 * elapsed_secs as f64);

        // A small overspend is worth noticing, while 50% above the sustainable
        // rate should be unmistakable. For a 7d window the sustainable rate is
        // 100 / (7 * 24), or roughly 0.6 percentage points per hour.
        if pace_ratio >= 1.5 {
            return theme::COST_HIGH;
        }
        if pace_ratio >= 1.1 {
            return theme::COST_MID;
        }
        return theme::COST_LOW;
    }

    if window.pct >= 90 {
        theme::COST_HIGH
    } else if window.pct >= 70 {
        theme::COST_MID
    } else {
        theme::COST_LOW
    }
}

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
                let now = chrono::Utc::now().timestamp();
                if let Some(plan) = &q.plan {
                    spans.push(Span::styled(format!("({plan}) "), theme::dim()));
                }
                for w in &q.windows {
                    let color = quota_color(w, now);
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
                        let remaining = reset - now;
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
        ("F7", "Age"),
        ("←→", "Panel"),
        ("Space", "Mark"),
        ("D", "Batch"),
        ("y", "Copy"),
        ("d", "Delete"),
        ("k", "Kill"),
        ("+/-", "Speed"),
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
    if app.cost_floor > 0.0 {
        spans.push(Span::styled(
            format!(" ≥${:.2} ", app.cost_floor),
            Style::default().fg(theme::COST_HIGH),
        ));
    }
    if !app.marked.is_empty() {
        spans.push(Span::styled(
            format!(" [{} marked] ", app.marked.len()),
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(
        format!(" {}s ", app.refresh_secs),
        Style::default().fg(theme::DIMMER),
    ));
    if app.follow {
        spans.push(Span::styled(
            " FOLLOW ",
            Style::default().fg(theme::COST_MID),
        ));
    }
    // Last, so it never pushes a key hint off the end of a narrow footer.
    if let Some(version) = &app.update_available {
        spans.push(Span::styled(
            format!(" v{version} available — cctop --update "),
            Style::default()
                .fg(theme::COST_MID)
                .add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

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
    fn quota_colour_tracks_spending_pace() {
        let duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);
        let reset = 1_000_000;
        let window = crate::quota::Window {
            label: "7d",
            pct: 80,
            duration: Some(duration),
            // 80% used halfway through a seven-day window is well ahead of
            // the even 100/(7*24) percentage-points-per-hour pace.
            resets_at: Some(reset + duration.as_secs() as i64 / 2),
        };
        assert_eq!(quota_color(&window, reset), theme::COST_HIGH);

        let sustainable = crate::quota::Window {
            pct: 50,
            resets_at: Some(reset + duration.as_secs() as i64 / 2),
            ..window
        };
        assert_eq!(quota_color(&sustainable, reset), theme::COST_LOW);
    }
}
