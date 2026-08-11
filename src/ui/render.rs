//! Frame layout, drawing, and mouse hit-testing.

use super::columns::ColumnId;
use super::modals;
use super::spark;
use super::table;
use super::theme::{self, Gradient};
use super::{App, Mode, panels, tabs};
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
    /// `(start_col, end_col, workspace_index)` spans in the top tab bar.
    pub(super) workspace_spans: Vec<(u16, u16, usize)>,
    /// `(start_col, end_col)` of the bar's new-tab button.
    pub(super) workspace_new: Option<(u16, u16)>,
    /// The rectangle a modal covers while one is up. A click inside it belongs
    /// to the modal, and a click outside it must not reach the dashboard the
    /// modal is sitting on top of.
    pub(super) modal_rect: Option<Rect>,
    /// `(row, choice_index)` for each row of the launcher's list.
    pub(super) launch_rows: Vec<(u16, usize)>,
    /// Tool Activity sidebar: `(x_end, y_start, first_index, row_count)`.
    pub(super) tool_sidebar: Option<(u16, u16, usize, usize)>,
    /// Tool Activity log area: `(x_start, y_start, height)`.
    pub(super) tool_log: Option<(u16, u16, u16)>,
    /// Where each pane of the open tab has its agent's screen, in pane order.
    ///
    /// The agent's screen, not the pane: the border is cctop's and the shim may
    /// have granted less room than the pane has, so this is the rectangle a
    /// mouse position can be turned into a cell of.
    pub(super) pane_rects: Vec<Rect>,
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

    /// Index of the workspace tab under the cursor. The bar is always the top
    /// row when it is drawn at all.
    pub fn workspace_at(&self, col: u16, row: u16) -> Option<usize> {
        if row != 0 {
            return None;
        }
        self.workspace_spans
            .iter()
            .find(|(a, b, _)| col >= *a && col < *b)
            .map(|(_, _, i)| *i)
    }

    /// Whether the cursor is on the bar's new-tab button.
    pub fn workspace_new_at(&self, col: u16, row: u16) -> bool {
        matches!(self.workspace_new, Some((a, b)) if row == 0 && col >= a && col < b)
    }

    /// Whether the cursor is inside the modal that is up, if one is.
    pub fn in_modal(&self, col: u16, row: u16) -> bool {
        self.modal_rect
            .is_some_and(|r| r.contains((col, row).into()))
    }

    /// Index of the launcher choice under the cursor, if any.
    /// The pane under `(col, row)`, and where in its agent's screen that is.
    pub fn pane_at(&self, col: u16, row: u16) -> Option<(usize, u16, u16)> {
        self.pane_rects.iter().enumerate().find_map(|(i, r)| {
            (col >= r.x && col < r.right() && row >= r.y && row < r.bottom())
                .then(|| (i, col - r.x, row - r.y))
        })
    }

    pub fn launch_row_at(&self, col: u16, row: u16) -> Option<usize> {
        self.in_modal(col, row)
            .then(|| self.launch_rows.iter().find(|(y, _)| *y == row))
            .flatten()
            .map(|(_, i)| *i)
    }
}

pub(super) fn panel_block(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border))
        .title(Span::styled(format!(" {title} "), theme::title()))
}

pub fn draw(frame: &mut Frame, app: &mut App) -> Layout {
    let mut area = frame.area();
    let mut layout = Layout::default();

    // The bar is always there, even with only the dashboard in it: the way to
    // open an agent has to be visible before you have opened one, or nobody
    // finds it. One row is a cheap price for that.
    {
        let bar = Rect { height: 1, ..area };
        draw_workspace_bar(frame, bar, app, &mut layout);
        area = Rect {
            y: area.y + 1,
            height: area.height.saturating_sub(1),
            ..area
        };
    }

    // A tab's terminals replace the table and panels, and the Overview stays put
    // so the money and the alerts never leave the frame. Agents are resized to
    // the space that leaves them rather than cropped to fit, so giving cctop
    // these rows costs nothing but the rows.
    if app.tab > 0 {
        let chunks = RLayout::vertical([
            Constraint::Length(6),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);
        draw_overview(frame, chunks[0], app);
        draw_panes(frame, chunks[1], app, &mut layout);
        draw_footer(frame, chunks[2], app);
        if app.mode == Mode::Launch {
            modals::draw_launch(frame, area, app, &mut layout);
        }
        return layout;
    }

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
        Mode::Help => modals::draw_help(frame, area, app),
        Mode::Search => modals::draw_search(frame, area, app),
        Mode::SortBy => modals::draw_sortby(frame, area, app),
        Mode::AgeFilter => modals::draw_age_filter(frame, area, app),
        Mode::DeleteConfirm => modals::draw_delete_confirm(frame, area, app),
        Mode::DeleteBlocked => modals::draw_delete_blocked(frame, area, app),
        Mode::KillConfirm => modals::draw_kill_confirm(frame, area, app),
        Mode::ResumeConfirm => modals::draw_resume_confirm(frame, area, app),
        Mode::TmuxInstall => modals::draw_tmux_install(frame, area, app),
        Mode::QuitConfirm => modals::draw_quit_confirm(frame, area, app),
        Mode::KillBlocked => modals::draw_kill_blocked(frame, area, app),
        Mode::BatchConfirm => modals::draw_batch_confirm(frame, area, app),
        Mode::BatchDeleteBlocked => modals::draw_batch_blocked(frame, area, app, true),
        Mode::BatchKillBlocked => modals::draw_batch_blocked(frame, area, app, false),
        Mode::CostFilter => modals::draw_cost_filter(frame, area, app),
        Mode::SendKeys => modals::draw_send_keys(frame, area, app),
        Mode::Launch => modals::draw_launch(frame, area, app, &mut layout),
        Mode::Hooks => modals::draw_hooks(frame, area, app),
        Mode::List => {}
    }
    layout
}

/// The workspace tab bar: the dashboard first, then a tab per set of terminals.
fn draw_workspace_bar(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let titles: Vec<String> = std::iter::once("Dashboard".to_string())
        .chain(app.tabs.iter().map(|tab| tab.title()))
        .enumerate()
        .map(|(i, title)| format!("{}:{}", i + 1, title))
        .collect();
    let on = app.blink_on();
    let mut spans = Vec::new();
    let mut pos = area.x;
    // The new-tab button is the one thing the bar exists for, so its room is
    // reserved before the labels get any: nothing else here is reachable by
    // mouse if it falls off the end.
    let new_tab = match app.tab {
        // Which key to name depends on where the keyboard is: inside a pane it
        // belongs to the agent, so only the Alt- form gets through.
        0 => " + Tab (t) ",
        _ => " + Tab (Alt+n) ",
    };
    let label_room = area.width.saturating_sub(new_tab.chars().count() as u16) as usize;

    // Only crowded bars pay for the crowding: while every label fits it is
    // drawn whole, and past that each tab gets an equal share. A clipped label
    // you can still count and click beats a bar that runs off the screen.
    let natural: usize = titles.iter().map(|t| t.chars().count() + 2).sum();
    let cap = match natural <= label_room {
        true => usize::MAX,
        false => (label_room / titles.len()).saturating_sub(2).max(3),
    };

    for (i, title) in titles.iter().enumerate() {
        let text = format!(" {} ", elide(title, cap));
        let width = text.chars().count() as u16;
        // A tab wanting something outranks the plain selected/unselected look:
        // the whole point of the colour is to be seen while you are reading a
        // different tab.
        let style = match app.tab_attention(i) {
            // Blinking by hand rather than with `Modifier::SLOW_BLINK`, which
            // many terminals quietly drop — an attention cue that only works on
            // some emulators is worse than none, because you stop trusting it.
            Some(tabs::Attention::NeedsInput) => match on {
                true => theme::attention_lit(theme::colors().cost_mid),
                false => Style::default()
                    .fg(theme::colors().cost_mid)
                    .add_modifier(Modifier::BOLD),
            },
            // A quiet agent is useful context, not an alarm. Keep its green
            // label visible without repeatedly pulling attention from work.
            Some(tabs::Attention::Idle) => Style::default()
                .fg(theme::colors().cost_low)
                .add_modifier(Modifier::BOLD),
            None if i == app.tab => theme::selected(),
            None => Style::default().fg(theme::colors().dim),
        };
        spans.push(Span::styled(text, style));
        layout.workspace_spans.push((pos, pos + width, i));
        pos += width;
    }

    // The button that says the feature exists. It carries its key as well as
    // its click target, because the keyboard is how anyone will use it twice.
    let width = new_tab.chars().count() as u16;
    if pos + width <= area.x + area.width {
        spans.push(Span::styled(
            new_tab,
            Style::default()
                .fg(theme::colors().accent)
                .add_modifier(Modifier::BOLD),
        ));
        layout.workspace_new = Some((pos, pos + width));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// `text`, clipped to `max` columns with an ellipsis when it does not fit.
fn elide(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
}

/// How much of the quota fits. Rather than truncate mid-number, the pane border
/// drops whole pieces: the reset countdowns first, then the window labels,
/// keeping the percentages — the part that answers "can I keep working" — down
/// to the narrowest split.
#[derive(Clone, Copy)]
enum QuotaDetail {
    Full,
    NoResets,
    PctOnly,
}

/// The quota to put on a pane's border, at the widest detail fitting `budget`.
///
/// Quota belongs to the provider, not the pane, but a pane is where you are
/// looking when it matters and its border already names the command. Repeating
/// it in every tab label — the previous home — made a wall of identical
/// percentages that buried the titles; on the border, each pane shows the
/// figures for its own agent and nothing shows them twice on one line.
///
/// The provider comes from the pane's command label rather than a session: a
/// fresh pane has no transcript, let alone a known provider session, when its
/// border is drawn.
fn pane_quota(label: &str, quota: &crate::quota::Quota, now: i64, budget: u16) -> Option<String> {
    let command = label
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let status = if command.starts_with("claude") {
        &quota.claude
    } else if command.starts_with("codex") {
        &quota.codex
    } else {
        return None;
    };

    [
        QuotaDetail::Full,
        QuotaDetail::NoResets,
        QuotaDetail::PctOnly,
    ]
    .into_iter()
    .map(|detail| quota_suffix(status, now, detail))
    .take_while(|text| !text.is_empty())
    .find(|text| text.chars().count() as u16 + 2 <= budget)
    .map(|text| format!(" {text} "))
}

/// One provider's windows, compacted to `detail`.
fn quota_suffix(status: &crate::quota::ProviderStatus, now: i64, detail: QuotaDetail) -> String {
    let crate::quota::ProviderStatus::Ok(quota) = status else {
        return String::new();
    };
    let windows = quota.windows.iter().map(|window| match detail {
        QuotaDetail::PctOnly => format!("{}%", window.pct),
        QuotaDetail::NoResets => format!("{} {}%", window.label, window.pct),
        QuotaDetail::Full => {
            let reset = window
                .resets_at
                .map(|at| at - now)
                .filter(|remaining| *remaining > 0)
                .map(|remaining| format!(" {}h{:02}m", remaining / 3600, (remaining % 3600) / 60))
                .unwrap_or_default();
            format!("{} {}%{}", window.label, window.pct, reset)
        }
    });
    let sep = match detail {
        QuotaDetail::PctOnly => "/",
        _ => " · ",
    };
    windows.collect::<Vec<_>>().join(sep)
}

/// Every terminal in the active tab, sharing the space evenly.
///
/// Sizing is the part that already worked: each pane asks the shim for exactly
/// the rectangle it was given, so a split is two agents each drawing a real
/// screen rather than two crops of one.
fn draw_panes(frame: &mut Frame, area: Rect, app: &mut App, layout: &mut Layout) {
    // Read out before the tab is borrowed mutably; the panes' borders want it.
    let quota = app.quota.clone();
    let Some(tab) = app.active_tab() else {
        return;
    };
    if tab.panes.is_empty() {
        return;
    }
    let share = Constraint::Ratio(1, tab.panes.len() as u32);
    let slots = match tab.stacked {
        true => RLayout::vertical(vec![share; tab.panes.len()]),
        false => RLayout::horizontal(vec![share; tab.panes.len()]),
    }
    .split(area);

    let focus = tab.focus;
    let now = chrono::Utc::now().timestamp();
    for (i, pane) in tab.panes.iter_mut().enumerate() {
        let mut block = panel_block(&pane.label);
        // The border is long and empty, and the label has already told you which
        // agent this is; the quota is the other thing you want while it runs.
        // Right-aligned so it does not move when the label changes, and only if
        // it fits beside the label rather than pushing it off.
        let taken = pane.label.chars().count() as u16 + 6;
        if let Some(text) = pane_quota(
            &pane.label,
            &quota,
            now,
            slots[i].width.saturating_sub(taken).saturating_sub(2),
        ) {
            block = block.title_top(Line::from(Span::styled(text, theme::dim())).right_aligned());
        }
        if i == focus {
            block = block
                .border_style(Style::default().fg(theme::colors().border_hi))
                .title_bottom(Span::styled(" F12 back · Alt+w close ", theme::title()));
        }
        // Scrolled back, this pane is showing history rather than the agent, and
        // there is nothing on a still screen to say so — the agent may well be
        // working below it. Only cctop's own history says anything here: a pane
        // scrolled inside tmux is in tmux's copy-mode, which draws its own.
        let behind = pane.view.parser.screen().scrollback();
        if behind > 0 {
            block = block.title_bottom(
                Line::from(Span::styled(
                    format!(" ↑ {behind} — type to catch up "),
                    theme::title(),
                ))
                .right_aligned(),
            );
        }
        let inner = block.inner(slots[i]);
        pane.view.resize(inner.width, inner.height);

        // The shim may grant less than was asked for — it has to satisfy every
        // watcher at once — so the answer, not the request, is what gets drawn,
        // and the leftover is left blank rather than stretched into.
        let (cols, rows) = pane.view.size;
        let screen = Rect {
            width: cols.min(inner.width),
            height: rows.min(inner.height),
            ..inner
        };
        frame.render_widget(block, slots[i]);
        frame.render_widget(
            tui_term::widget::PseudoTerminal::new(pane.view.parser.screen()),
            screen,
        );
        layout.pane_rects.push(screen);
    }
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

fn draw_overview(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel_block("Overview");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Spend has three time-series rows, while the agent figures are naturally
    // compact. Giving the charts the extra room makes a trend readable instead
    // of a decorative strip, especially on laptop-width terminals.
    let cols =
        RLayout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).split(inner);
    let (left, right) = (cols[0], cols[1]);

    let realtime = app.stats.spend_per_min;
    let label_w = 11usize;
    let value_w = 12usize;
    let chart_w = left.width.saturating_sub((label_w + value_w + 2) as u16) as usize;

    let row = |name: &str, amount: f64, series: &[f64], now_idx: Option<usize>| -> Line<'static> {
        let mut spans = vec![
            Span::styled(format!("{name:<label_w$}"), theme::label()),
            Span::styled(
                format!("{:>value_w$} ", util::adaptive_usd(amount)),
                Style::default()
                    .fg(theme::colors().cost_mid)
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
        row("Live rate", realtime, app.global_spend.values(), rt_idx),
        row(
            "Today",
            app.stats.spend_today,
            &app.stats.daily_hourly,
            hour_idx,
        ),
        row(
            "This month",
            app.stats.spend_calendar_month,
            &app.stats.monthly_daily,
            day_idx,
        ),
        // Every session ever recorded, across every provider. Deliberately without
        // a sparkline: the others chart a window that scrolls, and a running total
        // only ever climbs, so a chart of it says nothing the number doesn't.
        Line::from(vec![
            Span::styled(format!("{:<label_w$}", "All time"), theme::label()),
            Span::styled(
                format!("{:>value_w$}", util::adaptive_usd(app.stats.spend_total)),
                Style::default()
                    .fg(theme::colors().cost_mid)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(left_lines), left);

    let mem_mb = app.stats.total_memory as f64 / (1024.0 * 1024.0);
    let r_label_w = 10usize;
    let r_value_w = 9usize;
    let r_chart_w = right
        .width
        .saturating_sub((r_label_w + r_value_w + 2) as u16) as usize;

    let mut cpu_spans = vec![
        Span::styled(format!("{:<r_label_w$}", "Agent CPU"), theme::label()),
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
            Span::styled(format!("{:<r_label_w$}", "Agent mem"), theme::label()),
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
                Style::default().fg(theme::colors().cost_low),
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
        .border_style(Style::default().fg(theme::colors().border));
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
    // These panels describe the subagent, not the session it ran under, and the
    // two are a keystroke apart in the table. Naming it on the border is what
    // stops a subagent's small numbers from being read as its parent's.
    if let Some(sub) = app.selected_subagent() {
        let what = if sub.description.is_empty() {
            sub.agent_type.clone()
        } else {
            format!("{}: {}", sub.agent_type, sub.description)
        };
        spans.push(Span::styled(
            format!("↳ {}", crate::util::truncate(&what, 48)),
            Style::default().fg(theme::colors().accent),
        ));
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
            0 => (
                panels::info(session, data, app.plan, app.clash_of(session).as_ref()),
                app.info_scroll,
            ),
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
            6 => (panels::config(session), app.config_scroll),
            _ => (panels::context(session, data, width), app.context_scroll),
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
                            .fg(theme::colors().value)
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
                .map(|_| {
                    Line::from(Span::styled(
                        "│",
                        Style::default().fg(theme::colors().dimmer),
                    ))
                })
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
            return theme::colors().cost_high;
        }
        if pace_ratio >= 1.1 {
            return theme::colors().cost_mid;
        }
        return theme::colors().cost_low;
    }

    if window.pct >= 90 {
        theme::colors().cost_high
    } else if window.pct >= 70 {
        theme::colors().cost_mid
    } else {
        theme::colors().cost_low
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
                    Style::default().fg(theme::colors().cost_mid),
                ));
                spans.push(Span::styled(cmd.to_string(), theme::value()));
            }
            crate::quota::ProviderStatus::RateLimited { retry_at } => {
                spans.push(Span::styled(
                    "rate limited",
                    Style::default().fg(theme::colors().cost_mid),
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
                        Style::default().fg(theme::gray(244)),
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
                        Style::default().fg(theme::colors().cost_high),
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
                Style::default().fg(theme::colors().cost_low),
            ))),
            area,
        );
        return;
    }

    let key_style = theme::key_cap();
    let label_style = Style::default().fg(theme::colors().dim);

    // A terminal tab has no dashboard selection, filters, or panels to act on.
    // Its footer is the compact map of the keys cctop keeps while the rest of
    // the keyboard belongs to the focused agent.
    if app.tab > 0 {
        let mut spans = Vec::new();
        for (key, name) in [
            ("F12", "Dashboard"),
            ("F10", "Quit"),
            ("F1", "Help"),
            ("Alt+←→", "Tabs"),
            ("Alt+n", "New"),
            ("Alt+v/s", "Split"),
            ("Alt+o", "Focus"),
            ("Alt+w", "Close"),
            ("Alt+Shift+W", "Stop"),
        ] {
            spans.push(Span::styled(key, key_style));
            spans.push(Span::styled(format!(" {name} "), label_style));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }

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
        ("^K", "Kill"),
        ("+/-", "Speed"),
        ("F10", "Quit"),
    ] {
        spans.push(Span::styled(key, key_style));
        spans.push(Span::styled(format!("{name} "), label_style));
    }
    if let Some(age) = app.age_filter {
        spans.push(Span::styled(
            format!(" Age<{} ", age.short()),
            Style::default().fg(theme::colors().panel_title),
        ));
    }
    if !app.search.is_empty() {
        // The filter stays on the footer once the modal closes, so it has to
        // say whether transcripts are in scope — the two searches can return
        // very different tables for the same word.
        let scope = match (app.search_content, app.scanning) {
            (false, _) => String::new(),
            (true, true) => " +transcripts…".to_string(),
            (true, false) => format!(" +transcripts({})", app.scan_hits.len()),
        };
        spans.push(Span::styled(
            format!(" Filter: {}{scope} ", app.search),
            Style::default().fg(theme::colors().filter_badge),
        ));
    }
    if app.cost_floor > 0.0 {
        spans.push(Span::styled(
            format!(" ≥${:.2} ", app.cost_floor),
            Style::default().fg(theme::colors().cost_high),
        ));
    }
    if !app.marked.is_empty() {
        spans.push(Span::styled(
            format!(" [{} marked] ", app.marked.len()),
            Style::default()
                .fg(theme::colors().accent)
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(
        format!(" {}s ", app.refresh_secs),
        Style::default().fg(theme::colors().dimmer),
    ));
    if app.follow {
        spans.push(Span::styled(
            " FOLLOW ",
            Style::default().fg(theme::colors().cost_mid),
        ));
    }
    // Who rang, kept there until you are looking at them. A bell you heard from
    // the next room has to still be answerable when you come back.
    if let Some(bell) = app
        .notify
        .footer(app.selected_session().map(|s| s.key()).as_deref())
    {
        spans.push(Span::styled(
            format!(" {bell} "),
            Style::default()
                .fg(theme::colors().accent)
                .add_modifier(Modifier::BOLD),
        ));
    }
    // Two agents writing one file is the only thing here that is a fault rather
    // than a state, so it sits with the bell rather than among the badges.
    if let Some(clash) = app.conflict_footer() {
        spans.push(Span::styled(
            format!(" {clash} "),
            Style::default()
                .fg(theme::colors().cost_high)
                .add_modifier(Modifier::BOLD),
        ));
    }
    // Last, so it never pushes a key hint off the end of a narrow footer.
    if let Some(version) = &app.update_available {
        spans.push(Span::styled(
            format!(" v{version} available — cctop --update "),
            Style::default()
                .fg(theme::colors().cost_mid)
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

    fn window(label: &'static str, pct: u32, resets_at: i64) -> crate::quota::Window {
        crate::quota::Window {
            label,
            pct,
            duration: None,
            resets_at: Some(resets_at),
        }
    }

    #[test]
    fn pane_quota_narrows_to_fit_and_only_for_a_provider() {
        let quota = crate::quota::Quota {
            claude: crate::quota::ProviderStatus::Ok(crate::quota::ProviderQuota {
                plan: None,
                windows: vec![window("5h", 37, 10_000), window("7d", 21, 500_000)],
                limit_reached: false,
            }),
            ..Default::default()
        };

        assert_eq!(
            pane_quota("claude-6", &quota, 4_500, 40).as_deref(),
            Some(" 5h 37% 1h31m · 7d 21% 137h38m ")
        );
        assert_eq!(
            pane_quota("claude-6", &quota, 4_500, 20).as_deref(),
            Some(" 5h 37% · 7d 21% ")
        );
        assert_eq!(
            pane_quota("claude-6", &quota, 4_500, 10).as_deref(),
            Some(" 37%/21% ")
        );
        // Too narrow for even the percentages, and a shell has no provider.
        assert!(pane_quota("claude-6", &quota, 4_500, 5).is_none());
        assert!(pane_quota("zsh", &quota, 4_500, 40).is_none());
    }

    #[test]
    fn a_crowded_tab_bar_elides_instead_of_overflowing() {
        let titles: Vec<String> = (1..=6).map(|i| format!("{i}:claude-{i}")).collect();
        let label_room = 40usize;
        let cap = (label_room / titles.len()).saturating_sub(2).max(3);
        let drawn: usize = titles
            .iter()
            .map(|t| elide(t, cap).chars().count() + 2)
            .sum();

        assert!(drawn <= label_room, "{drawn} columns in {label_room}");
        assert_eq!(elide("1:claude-1", cap), "1:c…");
        assert_eq!(elide("1:cc", cap), "1:cc");
    }

    #[test]
    fn quota_suffix_drops_detail_step_by_step() {
        let status = crate::quota::ProviderStatus::Ok(crate::quota::ProviderQuota {
            plan: None,
            windows: vec![window("5h", 37, 10_000), window("7d", 21, 500_000)],
            limit_reached: false,
        });

        assert_eq!(
            quota_suffix(&status, 4_500, QuotaDetail::Full),
            "5h 37% 1h31m · 7d 21% 137h38m"
        );
        assert_eq!(
            quota_suffix(&status, 4_500, QuotaDetail::NoResets),
            "5h 37% · 7d 21%"
        );
        assert_eq!(
            quota_suffix(&status, 4_500, QuotaDetail::PctOnly),
            "37%/21%"
        );
        assert!(
            quota_suffix(
                &crate::quota::ProviderStatus::Pending,
                4_500,
                QuotaDetail::Full
            )
            .is_empty()
        );
    }

    #[test]
    fn hit_testing_maps_regions() {
        let layout = Layout {
            workspace_spans: vec![(0, 12, 0)],
            workspace_new: Some((12, 23)),
            header_row: 6,
            rows_start: 7,
            rows_end: 12,
            tab_row: 20,
            bottom_start: 20,
            column_spans: vec![(0, 5, ColumnId::Status), (5, 12, ColumnId::Cost)],
            tab_spans: vec![(2, 6, 0), (8, 19, 1)],
            tool_sidebar: Some((18, 21, 0, 3)),
            tool_log: Some((19, 21, 4)),
            modal_rect: Some(Rect::new(10, 8, 20, 6)),
            launch_rows: vec![(10, 0), (11, 1)],
            pane_rects: vec![Rect::new(1, 7, 40, 10)],
        };
        // A modal takes the clicks that land on it, and its rows resolve to the
        // choice on them — not to the table underneath, which shares those rows.
        assert_eq!(layout.launch_row_at(15, 11), Some(1));
        assert_eq!(layout.launch_row_at(15, 12), None);
        // Same row, but outside the modal: still not a choice.
        assert_eq!(layout.launch_row_at(5, 11), None);
        assert!(layout.in_modal(10, 8));
        assert!(!layout.in_modal(30, 8));
        // The bar's tabs and its new-tab button are separate targets, and both
        // only exist on the top row.
        assert_eq!(layout.workspace_at(3, 0), Some(0));
        assert_eq!(layout.workspace_at(15, 0), None);
        assert!(layout.workspace_new_at(15, 0));
        assert!(!layout.workspace_new_at(3, 0));
        assert!(!layout.workspace_new_at(15, 1));
        assert_eq!(layout.row_at(7), Some(0));
        assert_eq!(layout.row_at(11), Some(4));
        assert_eq!(layout.row_at(12), None);
        assert_eq!(layout.header_column_at(6, 6), Some(ColumnId::Cost));
        assert_eq!(layout.header_column_at(6, 7), None);
        assert_eq!(layout.tab_at(9, 20), Some(1));
        assert_eq!(layout.tab_at(9, 21), None);
        assert!(layout.in_bottom_panel(20));
        assert!(!layout.in_bottom_panel(19));
        // A pane resolves to the cell of the agent's own screen under the
        // pointer, so the agent is told where the wheel is, not where cctop
        // happens to have drawn it.
        assert_eq!(layout.pane_at(1, 7), Some((0, 0, 0)));
        assert_eq!(layout.pane_at(10, 9), Some((0, 9, 2)));
        assert_eq!(layout.pane_at(41, 9), None);
        assert_eq!(layout.pane_at(10, 17), None);
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
        assert_eq!(quota_color(&window, reset), theme::colors().cost_high);

        let sustainable = crate::quota::Window {
            pct: 50,
            resets_at: Some(reset + duration.as_secs() as i64 / 2),
            ..window
        };
        assert_eq!(quota_color(&sustainable, reset), theme::colors().cost_low);
    }

    /// A test agent that draws `text` once and then sits there, so anything on
    /// screen came from the replay and anything that moves came from a resize.
    ///
    /// Linux-only for the same reason as the shim's own tests — it needs a pty
    /// child, which hangs on the macOS runner.
    #[cfg(target_os = "linux")]
    fn test_pane(text: &str) -> (std::process::Child, u32, super::super::tabs::Pane) {
        let (child, pid) = crate::shim::test_session(
            &["sh", "-c", &format!("printf '{text}'; sleep 30")],
            // Wider and taller than any window below, so a crop would show.
            (200, 60),
        );
        let pane =
            super::super::tabs::Pane::view_of(pid, text.into()).expect("no attach connection");
        (child, pid, pane)
    }

    /// Draw until every pane has been granted the size it asked for. The resize
    /// is requested while drawing and answered a round trip later, so drawing
    /// once is never enough.
    #[cfg(target_os = "linux")]
    fn draw_until_sized(
        terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
        app: &mut App,
        want: &[(u16, u16)],
    ) -> bool {
        for _ in 0..50 {
            terminal
                .draw(|frame| {
                    draw(frame, app);
                })
                .expect("draw");
            let sized = app.tabs.iter_mut().any(|tab| {
                tab.pump();
                tab.panes.len() == want.len()
                    && tab.panes.iter().zip(want).all(|(p, w)| p.view.size == *w)
            });
            if sized {
                // One more frame, so what is asserted was drawn at the final size.
                terminal
                    .draw(|frame| {
                        draw(frame, app);
                    })
                    .expect("draw");
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        false
    }

    /// The way in has to be on screen before anyone has used it: with no tabs
    /// open the bar still names the dashboard and offers the new-tab button, and
    /// clicking that button is what `t` does.
    #[test]
    fn the_bar_offers_a_new_tab_with_nothing_open() {
        use crate::cache::UiPrefs;
        use crate::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        let (cols, rows) = (80u16, 24u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        let mut layout = Layout::default();
        terminal
            .draw(|frame| layout = draw(frame, &mut app))
            .expect("draw");

        let buffer = terminal.backend().buffer().clone();
        let top: String = (0..cols).map(|x| buffer[(x, 0)].symbol()).collect();
        assert!(
            top.starts_with(" 1:Dashboard  + Tab (t) "),
            "the new-tab button is not on the bar: {top:?}"
        );
        let (a, _) = layout.workspace_new.expect("no new-tab hit region");
        assert!(layout.workspace_new_at(a, 0));
        assert!(!layout.workspace_new_at(a, 1));
    }

    #[cfg(target_os = "linux")]
    fn screen(
        terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
        cols: u16,
        rows: u16,
    ) -> Vec<String> {
        let buffer = terminal.backend().buffer().clone();
        (0..rows)
            .map(|y| {
                (0..cols)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    /// A tab keeps you inside cctop: the tab bar, the Overview and the footer
    /// stay, and the agent is resized into what is left rather than cropped.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_tab_resizes_its_agent_into_the_space_cctop_leaves_it() {
        use crate::cache::UiPrefs;
        use crate::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (mut child, pid, pane) = test_pane("HELLO-FROM-AGENT");
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        app.tabs.push(super::super::tabs::Tab::new(pane));
        app.tab = 1;

        let (cols, rows) = (60u16, 21u16);
        // One row of tab bar, six of Overview, one of footer, and a border.
        let want = (cols - 2, rows - 1 - 6 - 1 - 2);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        let sized = draw_until_sized(&mut terminal, &mut app, &[want]);
        let screen = screen(&terminal, cols, rows);

        app.tabs.clear();
        let _ = child.kill();
        let _ = child.wait();
        let _ = crate::shim::socket_path(pid).map(std::fs::remove_file);

        assert!(
            sized,
            "the pty was never resized to the pane; wanted {want:?}"
        );
        assert!(
            screen[0].starts_with(" 1:Dashboard  2:HELLO-FROM-AGENT"),
            "the tab bar is not the top row: {:?}",
            screen[0]
        );
        assert!(
            screen[1].contains("Overview"),
            "the Overview is gone: {:?}",
            screen[1]
        );
        assert!(
            screen[8].starts_with("│HELLO-FROM-AGENT"),
            "the agent's screen is not inside the pane: {:?}",
            &screen[7..10]
        );
        assert!(
            screen[rows as usize - 2].contains("F12 back"),
            "the focused pane's hint is missing: {:?}",
            screen[rows as usize - 2]
        );
        assert!(
            screen[rows as usize - 1].contains("Dashboard"),
            "the agent footer is not showing workspace controls: {:?}",
            screen[rows as usize - 1]
        );
        assert!(
            !screen[rows as usize - 1].contains("Filter"),
            "the dashboard footer leaked into an agent tab: {:?}",
            screen[rows as usize - 1]
        );
    }

    /// A split gives each agent a real screen of its own, not two crops of one:
    /// both are resized to their half and both draw in it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_split_sizes_both_agents_to_their_own_half() {
        use crate::cache::UiPrefs;
        use crate::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (mut left_child, left_pid, left) = test_pane("LEFT-AGENT");
        let (mut right_child, right_pid, right) = test_pane("RIGHT-AGENT");
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        let mut tab = super::super::tabs::Tab::new(left);
        tab.panes.push(right);
        app.tabs.push(tab);
        app.tab = 1;

        let (cols, rows) = (80u16, 21u16);
        // Half the width each, minus each pane's own left and right border.
        let want = (cols / 2 - 2, rows - 1 - 6 - 1 - 2);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        let sized = draw_until_sized(&mut terminal, &mut app, &[want, want]);
        let screen = screen(&terminal, cols, rows);

        app.tabs.clear();
        for child in [&mut left_child, &mut right_child] {
            let _ = child.kill();
            let _ = child.wait();
        }
        for pid in [left_pid, right_pid] {
            let _ = crate::shim::socket_path(pid).map(std::fs::remove_file);
        }

        assert!(
            sized,
            "one of the split panes was never resized; wanted {want:?} each"
        );
        // Both agents on one row, each starting just inside its own border.
        let split_row = &screen[8];
        assert!(
            split_row.starts_with("│LEFT-AGENT"),
            "the left agent is not in the left half: {split_row:?}"
        );
        // By character, not by byte: the borders are multi-byte.
        let right_half: String = split_row.chars().skip((cols / 2) as usize).collect();
        assert!(
            right_half.starts_with("│RIGHT-AGENT"),
            "the right agent is not in the right half: {split_row:?}"
        );
    }
}
