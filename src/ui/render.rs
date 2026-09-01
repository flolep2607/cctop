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
use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::crossterm::event;
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
    /// `(row, start_col, end_col)` of the footer's share corner, while it is
    /// drawn. Recorded because cctop holds the terminal's mouse capture: the
    /// click that would follow the link has to be answered here or nowhere.
    pub(super) share_corner: Option<(u16, u16, u16)>,
    /// The rectangle a modal covers while one is up. A click inside it belongs
    /// to the modal, and a click outside it must not reach the dashboard the
    /// modal is sitting on top of.
    pub(super) modal_rect: Option<Rect>,
    /// `(row, choice_index)` for each row of the launcher's list.
    pub(super) launch_rows: Vec<(u16, usize)>,
    /// `(row, suggestion_index)` for each directory the `in` field is offering.
    pub(super) launch_cwd_rows: Vec<(u16, usize)>,
    /// `(row, item_index)` for each entry of the row menu.
    pub(super) menu_rows: Vec<(u16, usize)>,
    /// Tool Activity sidebar: `(x_end, y_start, first_index, row_count)`.
    pub(super) tool_sidebar: Option<(u16, u16, usize, usize)>,
    /// Tool Activity log area: `(x_start, y_start, height)`.
    pub(super) tool_log: Option<(u16, u16, u16)>,
    /// Every place a key is written on screen as its own label, and the key it
    /// stands for: `(row, start_col, end_col, key)`.
    ///
    /// The footer's hints and a confirmation's `[y]` are drawn to be read as
    /// buttons, and cctop holds the terminal's mouse capture — so a click on one
    /// is answered here or nowhere, exactly as it is for the share corner. One
    /// list rather than a field per surface, because the answer is always the
    /// same: press the key that is written there.
    pub(super) key_hits: Vec<(u16, u16, u16, event::KeyEvent)>,
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

    /// The key written under the cursor, if a click there means pressing one.
    pub fn key_at(&self, col: u16, row: u16) -> Option<event::KeyEvent> {
        self.key_hits
            .iter()
            .find(|(r, a, b, _)| *r == row && col >= *a && col < *b)
            .map(|(_, _, _, key)| *key)
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

    /// Whether the cursor is on the footer's share link.
    pub fn share_corner_at(&self, col: u16, row: u16) -> bool {
        matches!(self.share_corner, Some((y, a, b)) if row == y && col >= a && col < b)
    }

    /// Whether the cursor is inside the modal that is up, if one is.
    pub fn in_modal(&self, col: u16, row: u16) -> bool {
        self.modal_rect
            .is_some_and(|r| r.contains((col, row).into()))
    }

    /// Index of the directory suggestion under the cursor, if any.
    pub fn launch_cwd_row_at(&self, col: u16, row: u16) -> Option<usize> {
        self.in_modal(col, row)
            .then(|| self.launch_cwd_rows.iter().find(|(y, _)| *y == row))
            .flatten()
            .map(|(_, i)| *i)
    }

    /// Index of the launcher choice under the cursor, if any.
    /// The pane under `(col, row)`, and where in its agent's screen that is.
    pub fn pane_at(&self, col: u16, row: u16) -> Option<(usize, u16, u16)> {
        self.pane_rects.iter().enumerate().find_map(|(i, r)| {
            (col >= r.x && col < r.right() && row >= r.y && row < r.bottom())
                .then(|| (i, col - r.x, row - r.y))
        })
    }

    /// Index of the row-menu entry under the cursor, if any.
    pub fn menu_row_at(&self, col: u16, row: u16) -> Option<usize> {
        self.in_modal(col, row)
            .then(|| self.menu_rows.iter().find(|(y, _)| *y == row))
            .flatten()
            .map(|(_, i)| *i)
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
        .style(theme::canvas())
        .title(Span::styled(format!(" {title} "), theme::title()))
}

pub fn draw(frame: &mut Frame, app: &mut App) -> Layout {
    let mut area = frame.area();
    // Light paints the page first so empty cells and Reset spans stay on the
    // pale ground rather than punching through to a dark emulator. Dark is a
    // no-op: canvas() is Default, and the original look is the terminal's.
    frame.render_widget(Block::default().style(theme::canvas()), area);
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
        draw_footer(frame, chunks[2], app, &mut layout);
        match app.mode {
            Mode::Launch | Mode::LaunchCwd => modals::draw_launch(frame, area, app, &mut layout),
            // F10 is cctop's inside a pane, and when there is an agent to warn
            // about it raises a question. Drawn here as well as on the
            // dashboard, because a question asked on a screen you are not
            // looking at is indistinguishable from a key that did nothing —
            // and the keyboard is waiting on the answer either way.
            Mode::QuitConfirm => modals::draw_quit_confirm(frame, area, app, &mut layout),
            // A tab is renamed by right-clicking it, and the bar is on screen
            // inside a tab as much as over the dashboard.
            Mode::RenameTab => modals::draw_rename_tab(frame, area, app, &mut layout),
            _ => {}
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
    draw_footer(frame, chunks[4], app, &mut layout);

    match app.mode {
        Mode::Help => modals::draw_help(frame, area, app),
        Mode::Search => modals::draw_search(frame, area, app),
        Mode::SortBy => modals::draw_sortby(frame, area, app),
        Mode::AgeFilter => modals::draw_age_filter(frame, area, app),
        Mode::DeleteConfirm => modals::draw_delete_confirm(frame, area, app, &mut layout),
        Mode::DeleteBlocked => modals::draw_delete_blocked(frame, area, app, &mut layout),
        Mode::KillConfirm => modals::draw_kill_confirm(frame, area, app, &mut layout),
        Mode::ResumeConfirm => modals::draw_resume_confirm(frame, area, app, &mut layout),
        Mode::TmuxInstall => modals::draw_rmux_install(frame, area, app),
        Mode::Serve => modals::draw_serve(frame, area, app),
        Mode::QuitConfirm => modals::draw_quit_confirm(frame, area, app, &mut layout),
        Mode::KillBlocked => modals::draw_kill_blocked(frame, area, app, &mut layout),
        Mode::BatchConfirm => modals::draw_batch_confirm(frame, area, app, &mut layout),
        Mode::BatchDeleteBlocked => modals::draw_batch_blocked(frame, area, app, true, &mut layout),
        Mode::BatchKillBlocked => modals::draw_batch_blocked(frame, area, app, false, &mut layout),
        Mode::CostFilter => modals::draw_cost_filter(frame, area, app),
        Mode::SendKeys => modals::draw_send_keys(frame, area, app),
        Mode::RenameTab => modals::draw_rename_tab(frame, area, app, &mut layout),
        // The same modal: the directory field replaces one line of it, so the
        // list of agents stays on screen while the path is being typed.
        Mode::Launch | Mode::LaunchCwd => modals::draw_launch(frame, area, app, &mut layout),
        Mode::RowMenu => modals::draw_row_menu(frame, area, app, &mut layout),
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
fn pane_quota(
    label: &str,
    profile: Option<&str>,
    quota: &crate::quota::Quota,
    now: i64,
    budget: u16,
) -> Option<String> {
    let command = label
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    // A pane running as one account must not be shown another's remaining
    // budget: the figure is read to decide whether there is room to keep going.
    let status = if command.starts_with("claude") {
        quota.claude_for(profile)?
    } else if command.starts_with("codex") {
        quota.codex_for(profile)?
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
            pane.profile.as_deref(),
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
        // scrolled inside rmux is in rmux's copy-mode, which draws its own.
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

/// The longest series the Overview charts: one calendar month, one dot a day.
/// Every chart is drawn at this width or narrower and right-aligned into it, so
/// the newest sample of each row sits in the same column.
const SPARK_W: usize = 31;

/// Content rows the panel has, below its border.
const ROWS: usize = 4;

/// Scale floors, in the unit of each series.
///
/// Auto-scaling makes a chart's own peak full height, which is right for a busy
/// window and a lie for an idle one: a tenth of a cent an hour becomes a
/// mountain range, and the Overview reads as frantic while nothing is running.
/// Below these the chart scales to the floor instead, so an amount nobody would
/// call spending draws as the flat line it is.
/// Width of the right-hand machine column, and the least room the spend
/// breakdown is worth drawing in.
const MACHINE_W: usize = 42;
const MIN_GRID_W: usize = 30;
/// Target width of one project cell in the breakdown grid.
const CELL_W: usize = 27;

const RATE_FLOOR: f64 = 0.05; // $/min — $3 an hour
const HOUR_FLOOR: f64 = 1.00; // $/hour
const DAY_FLOOR: f64 = 10.00; // $/day

/// A sparkline right-aligned into `width`, scaled to its own peak or `floor`,
/// whichever is larger.
///
/// The lead is spaces rather than the baseline dots `sparkline` would pad with.
/// A dot is a reading, and the hours of today that have not happened yet have
/// not read zero — filling them made every quiet row an indistinguishable wall
/// of dots, which is the whole reason this panel was unreadable.
fn spark_spans(
    values: &[f64],
    width: usize,
    floor: f64,
    gradient: Gradient,
    now: Option<usize>,
) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let drawn = width.min(values.len().max(1));
    let peak = values.iter().cloned().fold(0.0f64, f64::max);
    let mut spans = Vec::with_capacity(drawn + 1);
    if width > drawn {
        spans.push(Span::raw(" ".repeat(width - drawn)));
    }
    spans.extend(spark::sparkline(values, drawn, peak.max(floor) * 1.1, gradient, now).spans);
    spans
}

/// `name  value` right-aligned into `width`, with the name truncated rather
/// than the amount — the amount is why the row is on the list.
fn ranked_row(name: &str, amount: f64, width: usize) -> Vec<Span<'static>> {
    let money = util::adaptive_usd(amount);
    let name_w = width.saturating_sub(money.chars().count() + 1);
    // A cut name that still looks like a whole one names the wrong project.
    let mut shown: String = match name.chars().count() > name_w && name_w > 0 {
        true => name
            .chars()
            .take(name_w - 1)
            .chain(std::iter::once('…'))
            .collect(),
        false => name.chars().take(name_w).collect(),
    };
    while shown.chars().count() < name_w {
        shown.push(' ');
    }
    vec![
        Span::styled(shown, theme::dim()),
        Span::raw(" "),
        Span::styled(money, Style::default().fg(theme::colors().cost_mid)),
    ]
}

fn draw_overview(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel_block("Overview");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let label_w = 11usize;
    let value_w = 12usize;
    let gutter = 2usize;

    // The charts shrink before anything else, and the two right-hand columns
    // drop off entirely rather than being squeezed into a width where a project
    // name is three letters. A narrow terminal gets the spend rows, which is
    // what the panel is for.
    let spark_w = SPARK_W.min(
        (inner.width as usize)
            .saturating_sub(label_w + value_w + 1 + gutter)
            .min(SPARK_W),
    );
    let left_w = (label_w + value_w + 1 + spark_w + gutter) as u16;
    let rest = inner.width.saturating_sub(left_w) as usize;
    // The machine's figures are a fixed width — they say the same thing on a
    // 100-column terminal as on a 300-column one. Every column beyond that goes
    // to the spend breakdown, which has more to say the more room it is given.
    let (mid_w, right_w) = match rest {
        r if r >= MACHINE_W + MIN_GRID_W => (r - MACHINE_W, MACHINE_W),
        r if r >= 20 => (0, r),
        _ => (0, 0),
    };

    let cols = RLayout::horizontal([
        Constraint::Length(left_w),
        Constraint::Length(mid_w as u16),
        Constraint::Length(right_w as u16),
    ])
    .split(inner);

    let now = chrono::Local::now();
    let hour_idx = Some(chrono::Timelike::hour(&now) as usize);
    let day_of_month = chrono::Datelike::day(&now) as usize;
    let day_idx = Some(day_of_month - 1);
    let rt_idx = app.global_spend.values().len().checked_sub(1);

    // --- Spend -------------------------------------------------------------
    let row = |name: &str, amount: f64, series: &[f64], floor: f64, now_idx: Option<usize>| {
        let mut spans = vec![
            Span::styled(format!("{name:<label_w$}"), theme::label()),
            Span::styled(
                format!("{:>value_w$} ", util::adaptive_usd(amount)),
                Style::default()
                    .fg(theme::colors().cost_mid)
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        spans.extend(spark_spans(
            series,
            spark_w,
            floor,
            Gradient::Spend,
            now_idx,
        ));
        Line::from(spans)
    };

    // A month-to-date average is the one number that says whether today was
    // ordinary, and it is a division of two figures already on the panel.
    let per_day = app.stats.spend_calendar_month / day_of_month as f64;

    let left_lines = vec![
        row(
            "Live rate",
            app.stats.spend_per_min,
            app.global_spend.values(),
            RATE_FLOOR,
            rt_idx,
        ),
        row(
            "Today",
            app.stats.spend_today,
            &app.stats.daily_hourly,
            HOUR_FLOOR,
            hour_idx,
        ),
        row(
            "This month",
            app.stats.spend_calendar_month,
            &app.stats.monthly_daily,
            DAY_FLOOR,
            day_idx,
        ),
        // Every session ever recorded, across every provider. Deliberately without
        // a sparkline: the others chart a window that scrolls, and a running total
        // only ever climbs, so a chart of it says nothing the number doesn't. The
        // space goes to the month's daily average instead, which does.
        Line::from(vec![
            Span::styled(format!("{:<label_w$}", "All time"), theme::label()),
            Span::styled(
                format!("{:>value_w$} ", util::adaptive_usd(app.stats.spend_total)),
                Style::default()
                    .fg(theme::colors().cost_mid)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}/day this month", util::adaptive_usd(per_day)),
                theme::dim(),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(left_lines), cols[0]);

    // --- Where today's money went ------------------------------------------
    //
    // The width that used to be filler is a ranking instead: today's spend by
    // project, read down each sub-column and then across. Four rows is what the
    // panel has, so a wider terminal buys more projects rather than more air.
    if mid_w > 0 {
        let head_w = 10usize;
        let grid_w = mid_w.saturating_sub(head_w);
        // Sub-columns sized near CELL_W rather than as many as will fit: two
        // columns of forty put the project at one end of the row and its amount
        // at the other, which is harder to read than three tighter ones.
        let sub_cols = ((grid_w + CELL_W / 2) / CELL_W).max(1);
        let cell_w = grid_w / sub_cols;
        let capacity = sub_cols * ROWS;

        let mut mid_lines: Vec<Line<'static>> = Vec::with_capacity(ROWS);
        let top = &app.stats.top_today;

        if top.is_empty() {
            mid_lines.push(Line::from(vec![
                Span::styled(format!("{:<head_w$}", "Top today"), theme::label()),
                Span::styled("nothing spent yet today", theme::dim()),
            ]));
        } else {
            let shown = capacity.min(top.len());
            // The tail is one row saying how much was left out, never a silent
            // truncation: a ranking you cannot tell is partial is a wrong total.
            let hidden = top.len() - shown;
            for r in 0..ROWS {
                let head = match r {
                    0 => format!("{:<head_w$}", "Top today"),
                    _ => " ".repeat(head_w),
                };
                let mut spans = vec![Span::styled(head, theme::label())];
                for c in 0..sub_cols {
                    let i = c * ROWS + r;
                    let last = i + 1 == shown;
                    if i >= shown {
                        break;
                    }
                    if hidden > 0 && last {
                        let rest: f64 = top[i..].iter().map(|e| e.1).sum();
                        spans.extend(ranked_row(
                            &format!("+{} more", hidden + 1),
                            rest,
                            cell_w.saturating_sub(2),
                        ));
                    } else {
                        spans.extend(ranked_row(&top[i].0, top[i].1, cell_w.saturating_sub(2)));
                    }
                    spans.push(Span::raw("  "));
                }
                mid_lines.push(Line::from(spans));
            }
        }
        frame.render_widget(Paragraph::new(mid_lines), cols[1]);
    }

    // --- The machine --------------------------------------------------------
    if right_w > 0 {
        let r_label_w = 10usize;
        let body_w = right_w.saturating_sub(r_label_w);
        let stats = &app.stats;

        let mut model_spans = vec![Span::styled(
            format!("{:<r_label_w$}", "Models"),
            theme::label(),
        )];
        let today: f64 = stats.models_today.iter().map(|m| m.1).sum();
        if today <= 0.0 {
            model_spans.push(Span::styled("idle today", theme::dim()));
        } else {
            // Percentages of today's spend, not a count of sessions ever seen:
            // the mix that is costing money is the one worth naming.
            for (i, (name, cost)) in stats.models_today.iter().take(3).enumerate() {
                if i > 0 {
                    model_spans.push(Span::styled(" · ", theme::dim()));
                }
                model_spans.push(Span::styled(name.clone(), theme::value()));
                model_spans.push(Span::styled(
                    format!(" {:.0}%", cost / today * 100.0),
                    theme::dim(),
                ));
            }
        }

        let mem_mb = stats.total_memory as f64 / (1024.0 * 1024.0);
        let cpu_text = format!("{:.1}%", stats.total_cpu);
        let cpu_spark_w = body_w.saturating_sub(cpu_text.chars().count() + 2).min(20);

        let mut cpu_spans = vec![
            Span::styled(format!("{:<r_label_w$}", "Agent CPU"), theme::label()),
            Span::styled(format!("{cpu_text:>6} "), theme::value()),
        ];
        cpu_spans.extend(spark_spans(
            app.global_cpu.values(),
            cpu_spark_w,
            100.0,
            Gradient::Cpu,
            app.global_cpu.values().len().checked_sub(1),
        ));

        let right_lines = vec![
            Line::from(model_spans),
            Line::from(vec![
                Span::styled(format!("{:<r_label_w$}", "Sessions"), theme::label()),
                Span::styled(stats.total.to_string(), theme::value()),
                Span::styled(" · ", theme::dim()),
                Span::styled(
                    format!("{} live", stats.running),
                    Style::default().fg(theme::colors().cost_low),
                ),
                Span::styled(format!(" · {} in 24h", stats.active_24h), theme::dim()),
            ]),
            Line::from(cpu_spans),
            Line::from(vec![
                Span::styled(format!("{:<r_label_w$}", "Agent mem"), theme::label()),
                Span::styled(format!("{mem_mb:.0} MB"), theme::value()),
                Span::styled(
                    format!(
                        " · {} in / {} out",
                        util::compact_tokens(stats.total_input),
                        util::compact_tokens(stats.total_output)
                    ),
                    theme::dim(),
                ),
            ]),
        ];
        frame.render_widget(Paragraph::new(right_lines), cols[2]);
    }
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
        .border_style(Style::default().fg(theme::colors().border))
        .style(theme::canvas());
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

    // A remote row carries what the table shows and nothing behind it: the
    // transcript that every other panel is a reading of is a file on the other
    // machine, and cctop never fetches it. Info is the exception, being built
    // from the row itself.
    //
    // Said outright rather than left to draw as zeroes. A Cost panel reporting
    // $0.00 next to a `$` column reporting $12 is not an empty panel, it is a
    // wrong one, and nothing on screen would say which to believe.
    if app.bottom_tab != 0
        && let Some(host) = app
            .selected_session()
            .and_then(|s| s.remote.as_ref())
            .map(|r| r.host.clone())
    {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!("This session is on {host}."),
                    theme::dim(),
                )),
                Line::default(),
                Line::from(Span::styled(
                    "cctop reads that machine's summary over ssh; the transcript this panel \
                     would break down stays there. Info has everything that crossed.",
                    theme::dim(),
                )),
            ])
            .wrap(ratatui::widgets::Wrap { trim: true }),
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
    app.panel_max_scroll = max_scroll;
    // Tool Activity follows its tail unless the user has scrolled away.
    let scroll = if app.bottom_tab == 3 {
        app.tool_owners = std::mem::take(&mut tool_owners);
        layout.tool_log = Some((target.x, target.y, target.height));
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

    // One column per account rather than a fixed two: a machine with a personal
    // and a work login has three things to report, and the panel exists to say
    // how much room is left in each.
    // The first of a harness's accounts is the one it would use unasked, and
    // naming it costs width the figures need — so only the others are
    // qualified. On the overwhelming majority of machines there are no others.
    // The harness travels alongside the label rather than being read back out
    // of it: `Codex (work)` is still Codex, and a hint chosen by comparing the
    // label told a Codex account to run `claude login`.
    let mut accounts: Vec<(String, &'static str, &crate::quota::ProfileQuota)> = Vec::new();
    for (harness, qs) in [("Claude", &app.quota.claude), ("Codex", &app.quota.codex)] {
        for (i, q) in qs.iter().enumerate() {
            let name = match i {
                0 => harness.to_string(),
                _ => format!("{harness} ({})", q.profile),
            };
            accounts.push((name, harness, q));
        }
    }

    let share = 100 / accounts.len().max(1) as u16;
    let cols = RLayout::horizontal(
        accounts
            .iter()
            .map(|_| Constraint::Percentage(share))
            .collect::<Vec<_>>(),
    )
    // A gap, or a column that fills its width runs straight into the next one
    // and the two read as one sentence.
    .spacing(2)
    .split(inner);

    for (i, (name, harness, account)) in accounts.iter().enumerate() {
        let status = &account.status;
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
                // A token account has no login to renew: its credentials are
                // the line the user pasted into cctop's own config, and
                // `claude login` would refresh a directory it does not use.
                // The shorter wording goes with the longer command, because an
                // account's whole message lives in one column of a shared line
                // and a hint clipped mid-flag is not a hint.
                let (said, cmd) = match (account.source, *harness) {
                    (crate::config::AccountSource::Token, _) => {
                        ("expired — ", "cctop --add-account")
                    }
                    (_, "Codex") => ("sign-in expired — ", "codex login"),
                    _ => ("sign-in expired — ", "claude login"),
                };
                spans.push(Span::styled(
                    said,
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

/// One key hint, and how hard it fights for a place on the footer.
///
/// The footer used to be a fixed list of twelve, which meant a narrow terminal
/// lost whichever three happened to be last and a wide one still never showed
/// `Enter`. Ordering the hints by what they earn, and fitting them to the
/// width that is actually left, fixes both: the row menu is always there and
/// the speed control is there only when nothing better wants the space.
struct Hint {
    key: &'static str,
    name: &'static str,
}

impl Hint {
    /// `key` + a space + `name` + a trailing space, matching how it renders.
    fn width(&self) -> usize {
        self.key.chars().count() + self.name.chars().count() + 2
    }

    /// The keystroke this hint stands for, so a click on it can be answered by
    /// pressing that key — one dispatch, and a hint cannot drift from what
    /// clicking it does.
    ///
    /// `None` for the hints that name more than one key: `↑↓`, `Alt+←→` and
    /// `Alt+v/s` are maps of a pair, and a click cannot say which half was
    /// meant. They stay labels, which costs nothing — a mouse already moves the
    /// selection by clicking a row and switches tab by clicking the bar.
    fn event(&self) -> Option<event::KeyEvent> {
        use event::{KeyCode, KeyModifiers as Mods};
        let (code, mods) = match self.key {
            "↑↓" | "Alt+←→" | "Alt+v/s" => return None,
            "↵" => (KeyCode::Enter, Mods::NONE),
            "Esc" => (KeyCode::Esc, Mods::NONE),
            "Tab" => (KeyCode::Tab, Mods::NONE),
            "Space" => (KeyCode::Char(' '), Mods::NONE),
            "F1" => (KeyCode::F(1), Mods::NONE),
            "F10" => (KeyCode::F(10), Mods::NONE),
            "F12" => (KeyCode::F(12), Mods::NONE),
            // `Alt+n` and friends. The letter is the last character, and the
            // handler for these reads the modifier as well as the code.
            alt if alt.starts_with("Alt+") => match alt.chars().next_back() {
                Some(c) => (KeyCode::Char(c), Mods::ALT),
                None => return None,
            },
            // Everything else is the single character it prints: `/`, `?`, `a`,
            // `D`. Uppercase as written, which is what the list handler matches.
            one => match (one.chars().next(), one.chars().count()) {
                (Some(c), 1) => (KeyCode::Char(c), Mods::NONE),
                _ => return None,
            },
        };
        Some(event::KeyEvent::new(code, mods))
    }
}

const fn hint(key: &'static str, name: &'static str) -> Hint {
    Hint { key, name }
}

/// Lay hints out left to right until the next one would not fit, then stop.
///
/// Dropping from the tail rather than truncating mid-word is what makes the
/// priority order mean anything: a half-drawn `Qu` teaches nobody that `q`
/// quits, so the hint that cannot fit whole is simply not shown.
fn fit_hints(hints: &[Hint], room: usize) -> Vec<Span<'static>> {
    fit_hints_at(hints, room, None, 0, 0)
}

/// The hints as spans, recording where each clickable one landed.
///
/// `x` and `row` are where the first span will be drawn, so a hit region is in
/// screen coordinates rather than in offsets a caller would have to add up
/// again. Passing no `hits` draws without recording, which is what the tests
/// and the width arithmetic want.
fn fit_hints_at(
    hints: &[Hint],
    mut room: usize,
    mut hits: Option<&mut Vec<(u16, u16, u16, event::KeyEvent)>>,
    x: u16,
    row: u16,
) -> Vec<Span<'static>> {
    let key_style = theme::key_cap();
    let label_style = Style::default().fg(theme::colors().dim);
    let mut spans = Vec::new();
    let mut at = x;
    for h in hints {
        let w = h.width();
        if w > room {
            break;
        }
        room -= w;
        // The whole chip, key and label together: `↵ Actions` reads as one
        // button, and a region that covered only the `↵` would miss most of
        // what a pointer is aimed at.
        if let Some(hits) = hits.as_deref_mut()
            && let Some(key) = h.event()
        {
            hits.push((row, at, at + w as u16, key));
        }
        at += w as u16;
        spans.push(Span::styled(h.key, key_style));
        spans.push(Span::styled(format!(" {} ", h.name), label_style));
    }
    spans
}

/// The keys a terminal tab keeps while the rest of the keyboard belongs to the
/// focused agent, most-used first.
fn tab_hints() -> Vec<Hint> {
    vec![
        hint("F12", "Dashboard"),
        hint("Alt+←→", "Tabs"),
        hint("Alt+n", "New"),
        hint("Alt+w", "Close"),
        hint("Alt+o", "Focus"),
        hint("F1", "Help"),
        hint("F9", "Image"),
        hint("Alt+v/s", "Split"),
        hint("F10", "Quit"),
    ]
}

/// The dashboard's hints for the state it is actually in.
///
/// Context, not a fixed list: `Enter` is worth nothing with an empty table,
/// `Esc` is worth nothing with no filter to clear, and once rows are marked the
/// batch actions matter more than the mark key that is already doing its job.
fn list_hints(app: &App) -> Vec<Hint> {
    let mut hints = vec![hint("↑↓", "Move")];
    if app.selected_session().is_some() {
        // First, and by some distance. Every per-row action is behind it, and
        // it is the one key that teaches the others their letters.
        hints.push(hint("↵", "Actions"));
    }
    hints.push(hint("/", "Filter"));
    if app.marked.is_empty() {
        hints.push(hint("Space", "Mark"));
    } else {
        // Marking is done; what is unobvious now is how to act on the marks.
        hints.push(hint("D", "Delete marked"));
        hints.push(hint("K", "Kill marked"));
        hints.push(hint("U", "Unmark"));
    }
    if app.has_filter() {
        hints.push(hint("Esc", "Clear filter"));
    }
    if app.selected_session().is_some() {
        hints.push(hint("a", "Attach"));
        hints.push(hint("R", "Resume"));
    }
    hints.push(hint("t", "New tab"));
    hints.push(hint("Tab", "Panel"));
    hints.push(hint("S", "Sort"));
    hints.push(hint("?", "Help"));
    hints.push(hint("q", "Quit"));
    hints
}

/// The badges: what cctop is doing to the table right now, and anything that
/// wants answering. These take their width before the hints do, because a hint
/// is a thing you could learn from the help and a badge is a fact about this
/// screen that is on show nowhere else.
fn footer_badges(app: &App) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    // First, and in the colour the costly things use: it is a question waiting
    // for an answer, and the answer is a second click on the hint beside it.
    if app.quit_arm {
        spans.push(Span::styled(
            " click q again to quit ",
            Style::default().fg(theme::colors().cost_high),
        ));
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
    // A machine that has dropped out has to say so. Its rows are still on
    // screen, holding their last reading, and the totals still look complete.
    if let Some(down) = app.remote_footer() {
        spans.push(Span::styled(
            format!(" ⚠ {down} "),
            Style::default().fg(theme::colors().cost_mid),
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
    if let Some(version) = &app.update_available {
        spans.push(Span::styled(
            format!(" v{version} available — cctop --update "),
            Style::default()
                .fg(theme::colors().cost_mid)
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans
}

/// What the footer's right-hand corner is showing.
///
/// The corner is one thing in two states, not two things: a tunnel that exists
/// is a link, and a tunnel that does not is the button that would make one.
/// Sharing this machine has never been reachable except through `B` and a
/// second key, which is a feature nobody finds by using cctop.
enum Corner {
    /// A tunnel is up. The label is its host; the link behind it carries the
    /// token, which is what opens the page.
    Link(String, String),
    /// No tunnel, and a click would open one — after a second click, because
    /// the first only arms it. The label says which of the two it is asking
    /// for.
    Button(&'static str),
    /// A tunnel is being registered. Cloudflare's edge takes a second or so to
    /// answer, and a spinner is the difference between a wait and a hang.
    Working(String),
}

impl Corner {
    fn label(&self) -> &str {
        match self {
            Self::Link(label, _) => label,
            Self::Button(label) => label,
            Self::Working(label) => label,
        }
    }
}

/// The corner for the state this cctop is in.
///
/// The link is the tunnel's and not the loopback one: `127.0.0.1:7777` is
/// something anyone can retype, while a quick tunnel's hostname is words
/// Cloudflare picked and changes every run. That is the one worth a click.
///
/// The button is armed by its first click and fires on the second — the same
/// two-step the launcher uses, and for a stronger reason: this one puts every
/// session on this machine behind a URL on the internet, and a stray click is
/// not consent to that. The armed label is where that is said, because it is
/// the sentence being clicked through.
fn share_corner(app: &App) -> Corner {
    // Before the states below, because it outranks all of them: a tunnel being
    // dialled is what the corner is doing, whatever it was showing before.
    if let Some(opening) = &app.share_opening {
        return Corner::Working(format!("{} opening a tunnel…", opening.frame()));
    }
    match app.serving.as_ref().and_then(|s| s.public.as_deref()) {
        Some(url) => Corner::Link(link_label(url), url.to_string()),
        None if app.share_arm => Corner::Button("⧉ publish to the internet?"),
        // Serving already, just not off this machine: the click adds the tunnel
        // to the server that is up rather than starting one.
        None if app.serving.is_some() => Corner::Button("⧉ + tunnel"),
        None => Corner::Button("⧉ share"),
    }
}

/// Columns a share label may take before it is shortened.
///
/// A quick tunnel's hostname is four words and a domain —
/// `tribute-resistance-resolved-moscow.trycloudflare.com` is a real one — which
/// at full length took a quarter of a wide footer and half a narrow one, and
/// pushed the key hints off the row it was borrowing.
const LINK_MAX: usize = 30;

/// `https://abc-def.trycloudflare.com/?t=secret` → `⧉ abc-def.trycloudflare.com`.
///
/// The host and not the query. The token is what opens the page, and the footer
/// is on screen for the whole run — in front of whoever walks past it and in
/// every screenshot that happens to have cctop in it. The link behind the label
/// still carries the token, because a link without it opens nothing.
///
/// Shortened from the middle when the host is long, which keeps both ends that
/// say something: the first words, which are what distinguishes one tunnel from
/// another, and `trycloudflare.com`, which is what says where it goes.
fn link_label(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?']).next().unwrap_or(rest);
    format!("⧉ {}", shorten_host(host))
}

fn shorten_host(host: &str) -> String {
    let chars: Vec<char> = host.chars().collect();
    if chars.len() <= LINK_MAX {
        return host.to_string();
    }
    // The tail is the domain, which is fixed and worth keeping whole; the head
    // takes whatever the ellipsis leaves.
    let tail = "trycloudflare.com";
    let keep = match host.ends_with(tail) {
        true => tail.chars().count(),
        false => LINK_MAX / 2,
    };
    let head = LINK_MAX.saturating_sub(keep + 1);
    let front: String = chars[..head].iter().collect();
    let back: String = chars[chars.len() - keep..].iter().collect();
    format!("{front}…{back}")
}

/// Columns the hints keep for themselves before the share link is offered any.
///
/// Enough for the first three, which is the same floor the badges respect: a
/// footer that has given its width away to a link and cannot say how to quit
/// has the priorities backwards.
const LINK_MIN_HINTS: usize = 26;

/// Draw `label` at `(x, y)` as an OSC 8 hyperlink to `url`.
///
/// A terminal that understands OSC 8 — Ghostty, kitty, WezTerm, iTerm2, VTE,
/// Windows Terminal — makes the label itself clickable and shows the target on
/// hover. One that does not swallows the sequence and prints the label. Either
/// way the columns spent are the label's.
///
/// The whole link goes in **one** cell — opening sequence, label and closing
/// sequence together — with [`CellDiffOption::ForcedWidth`] telling the diff how
/// many columns that cell actually paints. That option is ratatui/ratatui#1605,
/// and it is what lets an escape sequence live in a symbol at all: without it
/// the diff measures the sequence as text and skips the columns after it.
///
/// One cell rather than two, because the pair has to be atomic. Split across the
/// first and last column, a redraw that emitted a changed opening cell and left
/// the unchanged closing one alone would leave the hyperlink *open*, and every
/// cell written after it, anywhere on screen, would join the link.
///
/// The columns the label covers are then filled with its own characters, which
/// the diff will never draw — `ForcedWidth` skips them for as long as the link
/// is there. They are for the frame after it goes: the diff erases what the
/// previous buffer said was on screen, and columns it believed were blank are
/// columns it does not bother to erase.
fn draw_hyperlink(buf: &mut Buffer, x: u16, y: u16, label: &str, url: &str, style: Style) {
    // An ESC or a BEL inside the URL would end the sequence early and hand the
    // remainder to the terminal as commands. Nothing here builds such a URL —
    // it is Cloudflare's hostname and cctop's own token — which is exactly the
    // kind of assumption that stops being true without anyone noticing.
    let url: String = url.chars().filter(|c| !c.is_control()).collect();
    // Every character of the label is one column wide (a host name and one
    // glyph), so counting them is counting columns.
    let Some(width) = std::num::NonZeroU16::new(label.chars().count() as u16) else {
        return;
    };
    let Some(cell) = buf.cell_mut((x, y)) else {
        return;
    };
    cell.set_symbol(&format!("\x1b]8;;{url}\x1b\\{label}\x1b]8;;\x1b\\"))
        .set_style(style)
        .set_diff_option(CellDiffOption::ForcedWidth(width));
    for (i, ch) in label.chars().enumerate().skip(1) {
        let mut utf8 = [0u8; 4];
        if let Some(cell) = buf.cell_mut((x + i as u16, y)) {
            cell.set_symbol(ch.encode_utf8(&mut utf8)).set_style(style);
        }
    }
}

/// The footer, with the share link in its right-hand corner when one exists.
///
/// The link takes its columns before the hints and badges do, and is drawn after
/// them, so nothing lands on top of a cell holding an escape sequence.
fn draw_footer(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let corner = share_corner(app);
    let width = corner.label().chars().count();
    // Shown only with a footer's worth of keys still beside it. A column of gap
    // as well, so the corner never abuts the last badge.
    // Two columns of gap: one before it, so the corner never abuts the last
    // badge, and one after, which is the margin below.
    let reserved = match area.width as usize >= width + 2 + LINK_MIN_HINTS {
        true => width as u16 + 2,
        false => 0,
    };
    draw_footer_keys(
        frame,
        Rect {
            width: area.width - reserved,
            ..area
        },
        app,
        layout,
    );
    if reserved == 0 {
        return;
    }
    // One column short of the edge. The bottom-right cell is the one a terminal
    // is least willing to paint — writing it can scroll the screen — and a
    // label that ends there also reads as if it had been cut off.
    let x = area.right() - width as u16 - 1;
    match &corner {
        Corner::Link(label, url) => draw_hyperlink(
            frame.buffer_mut(),
            x,
            area.y,
            label,
            url,
            Style::default().fg(theme::colors().accent),
        ),
        // Amber while armed: the next click is the one that publishes, and the
        // colour cctop uses for "this wants reading" is the honest one for it.
        Corner::Button(label) => {
            let style = match app.share_arm {
                true => Style::default().fg(theme::colors().cost_mid),
                false => theme::dim(),
            };
            frame.buffer_mut().set_string(x, area.y, label, style);
        }
        Corner::Working(label) => {
            frame.buffer_mut().set_string(
                x,
                area.y,
                label,
                Style::default().fg(theme::colors().cost_mid),
            );
        }
    }
    layout.share_corner = Some((area.y, x, x + width as u16));
}

fn draw_footer_keys(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
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

    let total = area.width as usize;
    let (hints, badges) = if app.tab > 0 {
        // A terminal tab has no dashboard selection, filters or panels to act
        // on, so it has no badges either — just the compact map of the keys
        // cctop keeps.
        (tab_hints(), Vec::new())
    } else {
        (list_hints(app), footer_badges(app))
    };

    let badge_w: usize = badges.iter().map(|s| s.content.chars().count()).sum();
    // The badges never squeeze the hints below the first few: a footer that is
    // all state and no keys is what the terminal is for. Past that the badges
    // win, and the tail hints are the ones that go.
    let floor = hints.iter().take(3).map(Hint::width).sum::<usize>();
    let room = total.saturating_sub(badge_w).max(floor.min(total));

    // Only while the dashboard is what the keys act on. A hint clicked with a
    // modal up would be dispatched into that modal's handler — the `?` under a
    // search box types a question mark into the query — and the footer is drawn
    // beneath every modal, where it is a legend rather than a row of buttons.
    let mut spans = match app.mode {
        Mode::List => fit_hints_at(&hints, room, Some(&mut layout.key_hits), area.x, area.y),
        _ => fit_hints(&hints, room),
    };
    spans.extend(badges);
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

    /// The label is what is on screen and the token is not, but the link a
    /// click follows still carries it.
    #[test]
    fn the_share_label_shows_the_host_and_the_link_keeps_the_token() {
        let url = "https://few-words-here.trycloudflare.com/?t=secret";
        assert_eq!(link_label(url), "⧉ few-words-he…trycloudflare.com");
        // A short host is left alone; a long one keeps its first words and its
        // domain, and never grows past the width the footer set aside.
        assert_eq!(link_label("http://127.0.0.1:7777/?t=x"), "⧉ 127.0.0.1:7777");
        let long = link_label("https://tribute-resistance-resolved-moscow.trycloudflare.com/?t=x");
        assert!(long.starts_with("⧉ tribute"), "{long:?}");
        assert!(long.ends_with("trycloudflare.com"), "{long:?}");
        assert!(long.chars().count() <= LINK_MAX + 2, "{long:?}");

        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 1));
        draw_hyperlink(&mut buf, 2, 0, &link_label(url), url, Style::default());
        let opening = buf.cell((2, 0)).unwrap();
        assert!(opening.symbol().contains(url), "{:?}", opening.symbol());
        assert!(opening.symbol().starts_with("\x1b]8;;"));
        assert!(opening.symbol().ends_with("\x1b]8;;\x1b\\"));
    }

    /// One cell carries the escape sequences, and the columns it paints over
    /// hold the plain label — so the diff spends nothing on them while the link
    /// is up, and knows to erase them once it is gone.
    #[test]
    fn the_share_link_costs_one_cell_and_still_erases_cleanly() {
        let url = "https://few-words-here.trycloudflare.com/?t=secret";
        let label = link_label(url);

        let mut linked = Buffer::empty(Rect::new(0, 0, 40, 1));
        draw_hyperlink(&mut linked, 2, 0, &label, url, Style::default());
        // The columns the label covers hold its characters, not its escapes.
        assert_eq!(linked.cell((3, 0)).unwrap().symbol(), " ");
        assert_eq!(linked.cell((4, 0)).unwrap().symbol(), "f");

        // Drawing it twice writes nothing: the forced width keeps the covered
        // columns out of the diff entirely.
        let mut again = Buffer::empty(Rect::new(0, 0, 40, 1));
        draw_hyperlink(&mut again, 2, 0, &label, url, Style::default());
        assert!(linked.diff(&again).is_empty());

        // And when the tunnel goes, every column it painted is erased rather
        // than left holding half a hostname. The label's own blank column is
        // the exception, and only because a blank is what would be drawn there.
        let blank = Buffer::empty(Rect::new(0, 0, 40, 1));
        let erased: Vec<u16> = linked.diff(&blank).iter().map(|(x, _, _)| *x).collect();
        for (i, ch) in label.chars().enumerate().filter(|(_, c)| *c != ' ') {
            let x = 2 + i as u16;
            assert!(
                erased.contains(&x),
                "{ch} at column {x} survived: {erased:?}"
            );
        }
    }

    /// The corner is drawn in the corner: last row, hard against the right
    /// edge, and hit-testable where it was drawn.
    #[test]
    fn the_share_button_sits_in_the_bottom_right_corner() {
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
        let footer: String = (0..cols).map(|x| buffer[(x, rows - 1)].symbol()).collect();
        assert!(
            footer.ends_with("⧉ share "),
            "the share button is not in the corner: {footer:?}"
        );
        // The keys it took the columns from are still there beside it.
        assert!(footer.contains("Quit"), "{footer:?}");
        let (row, a, b) = layout.share_corner.expect("no share hit region");
        // One column short of the edge, and the hit region covers the label
        // rather than the margin beside it.
        assert_eq!((row, b), (rows - 1, cols - 1));
        assert!(layout.share_corner_at(a, row));
        assert!(layout.share_corner_at(b - 1, row));
        assert!(!layout.share_corner_at(b, row));
        assert!(!layout.share_corner_at(a - 1, row));
    }

    /// With no tunnel the corner is the button that would open one, and it
    /// says so twice: the first click only arms it.
    #[test]
    fn the_share_corner_asks_before_it_publishes() {
        use crate::cache::UiPrefs;
        use crate::pricing::Plan;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());

        assert!(app.serving.is_none());
        assert_eq!(share_corner(&app).label(), "⧉ share");

        app.share_arm = true;
        let armed = share_corner(&app);
        assert_eq!(armed.label(), "⧉ publish to the internet?");
        // Nothing to open yet, so the corner is a button and not a link: a
        // click on it starts a tunnel rather than a browser.
        assert!(matches!(armed, Corner::Button(_)));
    }

    /// While the edge is being dialled the corner spins, because the second
    /// that takes used to be a second of a frozen dashboard.
    #[test]
    fn the_share_corner_spins_while_the_tunnel_is_being_opened() {
        use crate::cache::UiPrefs;
        use crate::pricing::Plan;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        let (_done_tx, done_rx) = std::sync::mpsc::channel();
        app.share_opening = Some(super::super::Opening {
            rx: done_rx,
            since: std::time::Instant::now(),
        });

        let label = share_corner(&app).label().to_string();
        assert!(label.ends_with(" opening a tunnel…"), "{label:?}");
        // A frame of the spinner leads it, and it is one column like the rest.
        assert_eq!(
            label.chars().count(),
            " opening a tunnel…".chars().count() + 1
        );

        // And a click at it while it spins is impatience, not a second tunnel.
        app.on_share_corner(true);
        assert!(app.share_opening.is_some());
        assert!(app.serving.is_none());
    }

    /// A hint that does not fit whole is dropped, not cut: half a key cap
    /// teaches the wrong key.
    #[test]
    fn a_narrow_footer_drops_whole_hints_from_the_tail() {
        let hints = [hint("↑↓", "Move"), hint("↵", "Actions"), hint("q", "Quit")];
        // Two spans per hint that fits, so the count says how many made it.
        assert_eq!(fit_hints(&hints, 100).len(), 6);
        // "↑↓ Move " is 8, "↵ Actions " is 10: room for the first, and one
        // column short of the second.
        assert_eq!(fit_hints(&hints, 17).len(), 2);
        assert_eq!(fit_hints(&hints, 18).len(), 4);
        assert!(fit_hints(&hints, 3).is_empty());
    }

    /// The footer is a map of the state you are in, not a fixed list. Marking
    /// rows swaps the mark key for the things you can now do with the marks,
    /// and a filter that is on brings `Esc` forward.
    #[test]
    fn the_footer_hints_follow_what_the_dashboard_is_doing() {
        use crate::cache::UiPrefs;
        use crate::pricing::Plan;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        let names =
            |app: &App| -> Vec<&'static str> { list_hints(app).iter().map(|h| h.name).collect() };

        // Nothing selected: no per-row key is offered, because none would work.
        assert!(app.selected_session().is_none());
        let empty = names(&app);
        assert!(!empty.contains(&"Actions"), "{empty:?}");
        assert!(!empty.contains(&"Attach"), "{empty:?}");
        assert!(!empty.contains(&"Clear filter"), "{empty:?}");
        assert!(
            empty.contains(&"Help") && empty.contains(&"Quit"),
            "{empty:?}"
        );

        app.marked.insert("session-key".into());
        let marked = names(&app);
        assert!(!marked.contains(&"Mark"), "{marked:?}");
        assert!(marked.contains(&"Delete marked"), "{marked:?}");
        assert!(marked.contains(&"Unmark"), "{marked:?}");

        app.search = "web".into();
        assert!(names(&app).contains(&"Clear filter"));
    }

    #[test]
    fn pane_quota_narrows_to_fit_and_only_for_a_provider() {
        let quota = crate::quota::Quota {
            claude: vec![crate::quota::ProfileQuota {
                profile: "default".into(),
                source: crate::config::AccountSource::Directory,
                status: crate::quota::ProviderStatus::Ok(crate::quota::ProviderQuota {
                    plan: None,
                    windows: vec![window("5h", 37, 10_000), window("7d", 21, 500_000)],
                    limit_reached: false,
                }),
            }],
            ..Default::default()
        };

        assert_eq!(
            pane_quota("claude-6", None, &quota, 4_500, 40).as_deref(),
            Some(" 5h 37% 1h31m · 7d 21% 137h38m ")
        );
        assert_eq!(
            pane_quota("claude-6", None, &quota, 4_500, 20).as_deref(),
            Some(" 5h 37% · 7d 21% ")
        );
        assert_eq!(
            pane_quota("claude-6", None, &quota, 4_500, 10).as_deref(),
            Some(" 37%/21% ")
        );
        // Too narrow for even the percentages, and a shell has no provider.
        assert!(pane_quota("claude-6", None, &quota, 4_500, 5).is_none());
        assert!(pane_quota("zsh", None, &quota, 4_500, 40).is_none());
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
            share_corner: Some((24, 50, 78)),
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
            launch_cwd_rows: vec![(12, 0)],
            menu_rows: vec![(9, 0), (10, 1)],
            key_hits: vec![(
                24,
                0,
                10,
                event::KeyEvent::new(event::KeyCode::Enter, event::KeyModifiers::NONE),
            )],
            pane_rects: vec![Rect::new(1, 7, 40, 10)],
        };
        // A key written on screen answers its own columns and nothing else.
        assert_eq!(
            layout.key_at(3, 24).map(|k| k.code),
            Some(event::KeyCode::Enter)
        );
        assert!(layout.key_at(10, 24).is_none());
        assert!(layout.key_at(3, 23).is_none());
        // A modal takes the clicks that land on it, and its rows resolve to the
        // choice on them — not to the table underneath, which shares those rows.
        // The share link answers only its own columns on its own row: the
        // footer is one row of a screen that is otherwise all clickable.
        assert!(layout.share_corner_at(50, 24));
        assert!(layout.share_corner_at(77, 24));
        assert!(!layout.share_corner_at(78, 24));
        assert!(!layout.share_corner_at(50, 23));
        assert_eq!(layout.launch_row_at(15, 11), Some(1));
        assert_eq!(layout.launch_row_at(15, 12), None);
        // Same row, but outside the modal: still not a choice.
        assert_eq!(layout.launch_row_at(5, 11), None);

        // The directory suggestions are their own targets, on rows the choices
        // above them do not claim.
        assert_eq!(layout.launch_cwd_row_at(15, 12), Some(0));
        assert_eq!(layout.launch_cwd_row_at(15, 11), None);
        assert_eq!(layout.launch_cwd_row_at(5, 12), None);

        // The row menu is hit-tested the same way, and by the same rule: a
        // click outside the modal rectangle is not the menu's, whatever row it
        // happens to share with one of its entries.
        assert_eq!(layout.menu_row_at(15, 9), Some(0));
        assert_eq!(layout.menu_row_at(15, 10), Some(1));
        assert_eq!(layout.menu_row_at(15, 13), None);
        assert_eq!(layout.menu_row_at(5, 9), None);
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

    /// A named account is still its own harness's account.
    ///
    /// The bug this closes was on screen the first time the limits panel drew
    /// two Codex logins: the expired-sign-in hint was chosen by comparing the
    /// *label*, and `Codex (work)` is not `Codex`, so it told a Codex account
    /// to run `claude login`.
    #[test]
    fn an_expired_account_is_told_to_log_into_its_own_harness() {
        use crate::cache::UiPrefs;
        use crate::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let expired = |profile: &str, source| crate::quota::ProfileQuota {
            profile: profile.to_string(),
            status: crate::quota::ProviderStatus::Expired,
            source,
        };
        use crate::config::AccountSource::{Directory, Token};
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        app.quota.claude = vec![expired("default", Directory), expired("side", Token)];
        app.quota.codex = vec![expired("default", Directory), expired("work", Directory)];

        let (cols, rows) = (200u16, 50u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        terminal
            .draw(|frame| {
                draw(frame, &mut app);
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let screen: String = (0..rows)
            .map(|y| {
                (0..cols)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Every account shares one line, so the assertion has to look at this
        // account's own segment of it — the column to its left is also a Codex
        // one, and would satisfy a whole-line match on its behalf.
        let line = screen
            .lines()
            .find(|l| l.contains("Codex (work)"))
            .expect("the named account was not drawn");
        let mine = &line[line.find("Codex (work)").expect("found above")..];
        assert!(
            mine.contains("codex login") && !mine.contains("claude login"),
            "a Codex account was pointed at another harness's login: {mine:?}"
        );

        // And an account that is only a token is pointed at the command that
        // replaces one: it has no directory for `claude login` to write to, so
        // the ordinary hint would be a repair that changes nothing.
        let line = screen
            .lines()
            .find(|l| l.contains("Claude (side)"))
            .expect("the token account was not drawn");
        let mine = &line[line.find("Claude (side)").expect("found above")..];
        assert!(
            mine.contains("cctop --add-account") && !mine.contains("claude login"),
            "a token account was told to log in: {mine:?}"
        );
    }

    /// The Overview earns its width or gives it up.
    ///
    /// It used to spend a wide terminal on a hundred columns of baseline dots
    /// per row — a chart of nothing, drawn at the same size whether or not
    /// there was anything to chart. The width now carries today's spend broken
    /// down by project and by model, and on a terminal too narrow to hold that
    /// the breakdown leaves rather than being squeezed into initials.
    #[test]
    fn the_overview_spends_its_width_on_a_breakdown_and_gives_it_back_when_narrow() {
        use crate::cache::UiPrefs;
        use crate::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let overview = |cols: u16| -> Vec<String> {
            let (tx, _rx) = std::sync::mpsc::channel();
            let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
            app.stats.spend_today = 60.0;
            app.stats.spend_calendar_month = 300.0;
            app.stats.spend_total = 900.0;
            app.stats.top_today = vec![
                ("orchard".into(), 30.0),
                ("beehive".into(), 20.0),
                ("cellar".into(), 6.0),
                ("attic".into(), 3.0),
                ("shed".into(), 1.0),
            ];
            app.stats.models_today = vec![("opus-5".into(), 45.0), ("haiku-4-5".into(), 15.0)];

            let rows = 30u16;
            let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
            terminal
                .draw(|frame| {
                    draw(frame, &mut app);
                })
                .expect("draw");
            let buffer = terminal.backend().buffer().clone();
            (0..6)
                .map(|y| (0..cols).map(|x| buffer[(x, y)].symbol()).collect())
                .collect()
        };

        let wide = overview(190).join("\n");
        assert!(
            wide.contains("Top today") && wide.contains("orchard") && wide.contains("shed"),
            "the breakdown is missing from a wide Overview: {wide}"
        );
        // A ranking you cannot tell is partial is a wrong total, so the tail is
        // named even when every entry happens to fit.
        assert!(
            wide.contains("opus-5") && wide.contains("75%"),
            "today's model mix is not shown as a share of today: {wide}"
        );
        // The month's daily average is a division of two figures already on the
        // panel, and the row it sits on has no chart to draw.
        assert!(
            wide.contains("/day this month"),
            "the daily average is missing: {wide}"
        );

        let narrow = overview(72).join("\n");
        assert!(
            narrow.contains("Live rate") && narrow.contains("All time"),
            "a narrow Overview lost the spend rows it exists for: {narrow}"
        );
        assert!(
            !narrow.contains("Top today"),
            "the breakdown was squeezed into a narrow Overview: {narrow}"
        );
    }

    /// One line per entry, whether or not it can run.
    ///
    /// The reason a blocked entry carries used to sit on a line of its own,
    /// which doubled the height of exactly the entries the reader has already
    /// decided against — a menu mostly made of grey.
    #[test]
    fn the_row_menu_gives_every_entry_exactly_one_line() {
        use crate::cache::UiPrefs;
        use crate::pricing::Plan;
        use crate::session::Session;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        // A stopped session: nothing to type into and nothing to terminate, so
        // the menu carries two refusals and this actually tests the case.
        let mut session = Session::new(crate::pricing::Provider::Claude, "abc".into());
        session.started_at = "2026-01-01T00:00:00Z".into();
        session.last_active = session.started_at.clone();
        session.label_source = "/repo".into();
        app.sessions = vec![session];
        app.refilter();
        app.selected = 0;
        app.open_row_menu();
        assert_eq!(app.mode, super::super::Mode::RowMenu);

        let (cols, rows) = (120u16, 40u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        let mut layout = Layout::default();
        terminal
            .draw(|frame| layout = draw(frame, &mut app))
            .expect("draw");

        let items = super::super::menu::items(&app);
        let blocked: Vec<&str> = items.iter().filter_map(|i| i.blocked.as_deref()).collect();
        assert!(!blocked.is_empty(), "a stopped session refuses something");

        // Every entry got a hit region, and no two share a line — which is the
        // same thing as saying nothing spilled onto a second one.
        assert_eq!(layout.menu_rows.len(), items.len());
        let mut lines: Vec<u16> = layout.menu_rows.iter().map(|(y, _)| *y).collect();
        lines.sort_unstable();
        lines.dedup();
        assert_eq!(lines.len(), items.len(), "entries share or straddle lines");

        // And the refusal is on the same line as the label it explains.
        let buffer = terminal.backend().buffer().clone();
        let text_at = |y: u16| -> String { (0..cols).map(|x| buffer[(x, y)].symbol()).collect() };
        for (y, i) in &layout.menu_rows {
            let line = text_at(*y);
            assert!(
                line.contains(items[*i].label),
                "entry {i} is not on its own line: {line:?}"
            );
            if let Some(why) = &items[*i].blocked {
                let head: String = why.chars().take(12).collect();
                assert!(
                    line.contains(&head),
                    "the refusal for {:?} is not beside it: {line:?}",
                    items[*i].label
                );
            }
        }
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
