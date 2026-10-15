//! Frame layout, drawing, and mouse hit-testing.
//!
//! The frame and the workspace are drawn here; the overview, the bottom
//! panels, the limits and the footer are in the files beside it.

mod bottom;
mod footer;
mod limits;
mod overview;

use bottom::*;
use footer::*;
use limits::*;
use overview::*;

// What the sections name as `super::x`: they were written one level up,
// where `super` was the crate.
use super::{idle, outage, preview};

use super::columns::ColumnId;
use super::hyperlink;
use super::modals;
use super::spark;
use super::table;
use super::theme::{self, Gradient};
use super::{App, Mode, drunk, effects, high, panels, rave, scrollbar, tabs, toast};
use cctop_core::pricing::Provider;
use cctop_core::session::Surface;
use cctop_core::util;
use ratatui::Frame;
use ratatui::crossterm::event;
use ratatui::layout::{Constraint, Layout as RLayout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

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
    /// The `⧉ local` half of the share corner, while the corner is armed and
    /// nothing is being served. A click on it serves on this machine only;
    /// the rest of the corner is the internet.
    pub(super) share_local: Option<(u16, u16, u16)>,
    /// The rectangle a modal covers while one is up. A click inside it belongs
    /// to the modal, and a click outside it must not reach the dashboard the
    /// modal is sitting on top of.
    pub(super) modal_rect: Option<Rect>,
    /// `(row, item_index)` for each settings row drawn, while the settings
    /// page is what was drawn. The page is neither the table nor a modal over
    /// it: its rows are lines of one paragraph, headed by lines with no item
    /// behind them, so what a screen row means has to come from the draw.
    pub(super) settings_rows: Vec<(u16, usize)>,
    /// The rectangle the settings page's rows are drawn in — the block's
    /// inner area — while the page is drawn. The hit test needs it because
    /// the page takes the middle third of the frame, not the whole screen.
    pub(super) settings_inner: Option<Rect>,
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
    /// The session table's scrollbar track, while the table overflows.
    pub(super) table_track: Option<Rect>,
    /// The active bottom panel's scrollbar track, while it overflows.
    pub(super) panel_track: Option<Rect>,
    /// The Subagents panel's header cells while that header is on screen:
    /// `(row, start_col, end_col, sort)`. A click on one is the table header's
    /// click — it sorts the panel by that column, toggling on a repeat.
    pub(super) subagent_header: Vec<(u16, u16, u16, panels::SubagentSort)>,
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
    /// Every terminal drawn this frame — the panes, the preview, the login
    /// pane — as the terminal widget left it, so the colours an agent printed
    /// reach the screen as it printed them; see [`theme::pin_indexed`].
    pub(super) passthrough: Vec<theme::Passthrough>,
    /// `(row, start_col, end_col)` of an address a right-click renames: the
    /// share panel's `Address` line, or the serve panel's internet origin —
    /// recorded only while renaming it is on offer.
    pub(super) address_row: Option<(u16, u16, u16)>,
}

impl Layout {
    pub fn in_bottom_panel(&self, row: u16) -> bool {
        row >= self.bottom_start
    }

    pub fn row_at(&self, row: u16) -> Option<usize> {
        (row >= self.rows_start && row < self.rows_end).then(|| (row - self.rows_start) as usize)
    }

    /// The settings item drawn on the row a pointer event landed on, if a row
    /// is drawn there.
    ///
    /// The page records its own rows because they are not the table's: the
    /// file line, the section headings and the trailing hint are lines with
    /// no item behind them, and a click on those is a click on nothing.
    pub fn settings_row_at(&self, col: u16, row: u16) -> Option<usize> {
        self.settings_inner
            .is_some_and(|r| r.contains((col, row).into()))
            .then(|| {
                self.settings_rows
                    .iter()
                    .find(|(y, _)| *y == row)
                    .map(|(_, i)| *i)
            })
            .flatten()
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

    /// The Subagents panel's header column under the cursor, if any.
    pub fn subagent_header_at(&self, col: u16, row: u16) -> Option<panels::SubagentSort> {
        self.subagent_header
            .iter()
            .find(|(r, a, b, _)| *r == row && col >= *a && col < *b)
            .map(|(_, _, _, sort)| *sort)
    }

    /// The key written under the cursor, if a click there means pressing one.
    /// Whether a right-click here lands on an address that can be renamed.
    pub fn address_at(&self, col: u16, row: u16) -> bool {
        self.address_row
            .is_some_and(|(r, a, b)| r == row && col >= a && col < b)
    }

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

    /// Whether the cursor is on the armed corner's `⧉ local` half — the side
    /// that keeps the table on this machine rather than publishing it.
    pub fn share_local_at(&self, col: u16, row: u16) -> bool {
        matches!(self.share_local, Some((y, a, b)) if row == y && col >= a && col < b)
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
    panel_block_titled(title, theme::title())
}

/// A [`panel_block`] whose title is drawn in `title_style` rather than the
/// usual ink — how a painted tab colours the borders of its panes.
fn panel_block_titled(title: &str, title_style: Style) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border))
        .style(theme::canvas())
        .title(Span::styled(format!(" {title} "), title_style))
}

/// A tab's rows below the bar: the Overview, its terminals, and the footer.
/// Whether a terminal tab shows the Overview above its panes — `F4` in a tab.
///
/// Global rather than on `App`, because [`pane_size`] sizes an agent before
/// any `App` frame exists and has to agree with the layout it will be drawn in.
static TAB_OVERVIEW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_tab_overview(shown: bool) {
    TAB_OVERVIEW.store(shown, std::sync::atomic::Ordering::Relaxed);
}

pub fn tab_overview() -> bool {
    TAB_OVERVIEW.load(std::sync::atomic::Ordering::Relaxed)
}

/// Where a tab's toasts go: the Overview, or the top of the panes while `F4`
/// has handed the Overview's rows to them.
fn toast_area(chunks: &[Rect]) -> Rect {
    match chunks[0].height {
        0 => chunks[1],
        _ => chunks[0],
    }
}

fn tab_chunks(area: Rect) -> std::rc::Rc<[Rect]> {
    RLayout::vertical([
        Constraint::Length(if tab_overview() { 6 } else { 0 }),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area)
}

/// The size a one-pane tab draws its terminal at in this window, for starting an
/// agent at the size it will be shown rather than resizing it on the first frame.
///
/// That first-frame resize is not free. Claude outside fullscreen answers every
/// change of width by reprinting its whole conversation, and a reattach — which
/// is what switching to a rmux tab does — used to start the client at the
/// window's full size and then shrink it to the pane: two reprints of thousands
/// of lines, every switch.
pub fn pane_size() -> (u16, u16) {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    // Less the workspace bar, as `draw` takes it.
    let body = tab_chunks(Rect::new(0, 1, cols, rows.saturating_sub(1)))[1];
    let inner = Block::bordered().inner(body);
    (inner.width.max(1), inner.height.max(1))
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

    // The settings tab gets the same frame an agent tab does — the Overview
    // stays put, so the money and the alerts that several of its rows tune are
    // on screen while they are being tuned. Sharing `tab_chunks` rather than
    // taking the whole area is what keeps that true, and the page below it is
    // still the largest thing on the frame.
    if app.on_settings() {
        let chunks = tab_chunks(area);
        draw_overview(frame, chunks[0], app);
        modals::draw_settings(frame, chunks[1], app, &mut layout);
        modals::draw_settings_qr(frame, chunks[0].union(chunks[1]), app);
        draw_footer(frame, chunks[2], app, &mut layout);
        // A modal raised from the settings tab belongs to the tab, exactly as
        // one raised from the dashboard belongs to the dashboard.
        match app.mode {
            Mode::Launch | Mode::LaunchCwd | Mode::LaunchOptions => {
                modals::draw_launch(frame, area, app, &mut layout)
            }
            Mode::QuitConfirm => modals::draw_quit_confirm(frame, area, app, &mut layout),
            Mode::SwitchTab => modals::draw_switch_tab(frame, area, app, &mut layout),
            Mode::AddAccount => modals::draw_add_account(frame, area, app, &mut layout),
            Mode::Help => modals::draw_help(frame, area, app),
            _ => {}
        }
        draw_toasts(frame, toast_area(&chunks), app);
        return layout;
    }

    // A tab's terminals replace the table and panels, and the Overview stays put
    // so the money and the alerts never leave the frame. Agents are resized to
    // the space that leaves them rather than cropped to fit, so giving cctop
    // these rows costs nothing but the rows.
    if app.tab > 0 {
        let chunks = tab_chunks(area);
        draw_overview(frame, chunks[0], app);
        draw_panes(frame, chunks[1], app, &mut layout);
        draw_footer(frame, chunks[2], app, &mut layout);
        match app.mode {
            Mode::Launch | Mode::LaunchCwd | Mode::LaunchOptions => {
                modals::draw_launch(frame, area, app, &mut layout)
            }
            // F10 is cctop's inside a pane, and when there is an agent to warn
            // about it raises a question. Drawn here as well as on the
            // dashboard, because a question asked on a screen you are not
            // looking at is indistinguishable from a key that did nothing —
            // and the keyboard is waiting on the answer either way.
            Mode::QuitConfirm => modals::draw_quit_confirm(frame, area, app, &mut layout),
            // A tab is renamed by right-clicking it, and the bar is on screen
            // inside a tab as much as over the dashboard.
            Mode::RenameTab => modals::draw_rename_tab(frame, area, app, &mut layout),
            Mode::SwitchTab => modals::draw_switch_tab(frame, area, app, &mut layout),
            // A sign-in in progress stays on screen whichever tab is open.
            Mode::AddAccount => modals::draw_add_account(frame, area, app, &mut layout),
            // F1 inside a pane opens the sheet here rather than on the
            // dashboard; see `on_key_function`.
            Mode::Help => modals::draw_help(frame, area, app),
            _ => {}
        }
        // Under the rave, whose floor covers it, and over nothing else: the
        // trip is the ground, and the other two are drawn on top of it.
        if let Some(t) = app.high.elapsed() {
            high::paint(
                frame.buffer_mut(),
                chunks[2],
                t,
                theme::truecolor(),
                !app.raving(),
            );
        }
        if let Some(t) = app.rave.elapsed().filter(|_| app.raving()) {
            rave::paint(frame.buffer_mut(), chunks[2], t, theme::truecolor());
        }
        if let Some(t) = app.drunk.elapsed() {
            let colour = (!theme::no_color()).then(theme::truecolor);
            drunk::paint(
                frame.buffer_mut(),
                chunks[2],
                t,
                colour,
                !app.raving() && !app.tripping(),
            );
        }
        draw_toasts(frame, toast_area(&chunks), app);
        return layout;
    }

    // Overview and limits are fixed; the table and bottom panel split the rest,
    // with the bottom panel capped so the list never collapses to nothing.
    // On a short terminal the fixed panels go whole, limits first, so the table
    // and the footer keep their rows. Left to the solver, a 12-row screen drew
    // Limits as a lone top border, and a 5-row one lost the footer.
    // Overview, the table's floor, and the footer; then Limits on top of those.
    const WITH_OVERVIEW: u16 = 6 + 4 + 1;
    const WITH_LIMITS: u16 = WITH_OVERVIEW + 3;
    let limits_height = if area.height >= WITH_LIMITS { 3 } else { 0 };
    let overview_height = if area.height >= WITH_OVERVIEW { 6 } else { 0 };
    let body_height = area
        .height
        .saturating_sub(overview_height + limits_height + 1);
    let bottom_height = ((body_height as f32 * 0.45) as u16)
        .clamp(8, 24)
        .min(body_height.saturating_sub(4));

    let chunks = RLayout::vertical([
        // Four spend rows plus the border.
        Constraint::Length(overview_height),
        Constraint::Min(3),
        Constraint::Length(bottom_height),
        Constraint::Length(limits_height),
        Constraint::Length(1),
    ])
    .split(area);

    draw_overview(frame, chunks[0], app);
    table::draw_table(frame, chunks[1], app, &mut layout);
    draw_bottom(frame, chunks[2], app, &mut layout);
    draw_limits(frame, chunks[3], app, &mut layout);
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
        Mode::Serve => modals::draw_serve(frame, area, app, &mut layout),
        Mode::ShareQr => modals::draw_share_qr(frame, area, app, &mut layout),
        Mode::RenameAddress => modals::draw_address(frame, area, app, &mut layout),
        Mode::QuitConfirm => modals::draw_quit_confirm(frame, area, app, &mut layout),
        Mode::RemoteUpdateConfirm => {
            modals::draw_remote_update_confirm(frame, area, app, &mut layout)
        }
        Mode::KillBlocked => modals::draw_kill_blocked(frame, area, app, &mut layout),
        Mode::BatchConfirm => modals::draw_batch_confirm(frame, area, app, &mut layout),
        Mode::BatchDeleteBlocked => modals::draw_batch_blocked(frame, area, app, true, &mut layout),
        Mode::BatchKillBlocked => modals::draw_batch_blocked(frame, area, app, false, &mut layout),
        Mode::CostFilter => modals::draw_cost_filter(frame, area, app),
        Mode::SendKeys => modals::draw_send_keys(frame, area, app),
        Mode::NewWorktree => modals::draw_new_worktree(frame, area, app),
        Mode::RenameTab => modals::draw_rename_tab(frame, area, app, &mut layout),
        Mode::SwitchTab => modals::draw_switch_tab(frame, area, app, &mut layout),
        // The same modal: the directory field replaces one line of it, so the
        // list of agents stays on screen while the path is being typed.
        Mode::Launch | Mode::LaunchCwd | Mode::LaunchOptions => {
            modals::draw_launch(frame, area, app, &mut layout)
        }
        Mode::RowMenu => modals::draw_row_menu(frame, area, app, &mut layout),
        Mode::Hooks => modals::draw_hooks(frame, area, app),
        Mode::ProviderStatus => super::outage::draw(frame, area, app),
        Mode::Insight => modals::draw_insight(frame, area, app),
        Mode::Conversation => super::reader::draw(frame, area, app),
        Mode::AddAccount => modals::draw_add_account(frame, area, app, &mut layout),
        Mode::List => {}
    }
    // Over everything, modals included: a few seconds of confirmation for a
    // key that has already acted, in a corner nothing else owns.
    draw_paste_preview(frame, area, app);
    // Under the toasts, so what the code said when it was typed still reads.
    // Under the rave, whose floor covers it, and over nothing else: the
    // trip is the ground, and the other two are drawn on top of it.
    if let Some(t) = app.high.elapsed() {
        high::paint(
            frame.buffer_mut(),
            chunks[4],
            t,
            theme::truecolor(),
            !app.raving(),
        );
    }
    if let Some(t) = app.rave.elapsed().filter(|_| app.raving()) {
        rave::paint(frame.buffer_mut(), chunks[4], t, theme::truecolor());
    }
    if let Some(t) = app.drunk.elapsed() {
        let colour = (!theme::no_color()).then(theme::truecolor);
        drunk::paint(
            frame.buffer_mut(),
            chunks[4],
            t,
            colour,
            !app.raving() && !app.tripping(),
        );
    }
    // A terminal too short for the Overview still gets told things, on the
    // top row of what is there instead.
    let overview = match chunks[0].height {
        0 => Rect { height: 1, ..area },
        _ => chunks[0],
    };
    draw_toasts(frame, overview, app);
    layout
}

/// What cctop has said lately, over the Overview's right-hand side.
///
/// Over modals too, drawn last: a failure reported while a modal is up is the
/// one most likely to be about that modal, and the status line it replaces was
/// hidden behind every one of them.
fn draw_toasts(frame: &mut Frame, overview: Rect, app: &App) {
    // Inside the border, when there is one, so the panel still reads as a panel.
    let region = match overview.height > 2 {
        true => Block::bordered().inner(overview),
        false => overview,
    };
    toast::draw(frame.buffer_mut(), region, &app.toasts);
}

/// The image a paste just filed, held in the bottom-right corner briefly.
///
/// A toast rather than a modal — the paste has already happened and needs no
/// answering, it only needs seeing: the difference between "a path was typed"
/// and "*that* image was typed" is what this is for.
fn draw_paste_preview(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(preview) = &mut app.paste_preview else {
        return;
    };
    let width = 30u16.min(area.width.saturating_sub(4));
    let height = 10u16.min(area.height.saturating_sub(6));
    // Too small to say anything is too small to draw.
    if width < 8 || height < 4 {
        return;
    }
    let rect = Rect {
        x: area.right().saturating_sub(width + 1),
        y: area.bottom().saturating_sub(height + 2),
        width,
        height,
    };
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border_hi))
        .style(theme::canvas())
        .title(Span::styled(format!(" {} ", preview.name), theme::title()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_stateful_widget(
        ratatui_image::StatefulImage::default(),
        inner,
        &mut preview.image,
    );
}

/// What a tab being recorded carries after its label on the bar.
const REC: &str = "● REC ";

/// What a tab carries after its label while a turn in it ended unseen — see
/// [`seen`](super::seen). A glyph as well as the label's own colour, because
/// that colour is the idle green every finished tab already wears: the mark
/// has to say "this one is new" to someone who cannot tell two greens apart.
const DONE: &str = "✓ ";

/// What a tab carries after its label while one of its panes is zoomed, so a
/// split whose other panes have vanished says where they went.
const ZOOM: &str = "⤢ ";

/// The mark before a tab's title when its agent is on another machine, by how
/// the tab gets there: a sibling through its Cloudflare tunnel, or ssh — the
/// arrow the table's Host column puts after a sandboxed row's host.
///
/// One column each: the bar counts it as one, and a glyph a terminal draws two
/// wide would push every tab after it one column off its click target. The
/// cloud is an emoji-capable codepoint, so it carries U+FE0E: without it a
/// browser terminal on Windows draws Segoe UI Emoji's colour cloud, squeezed
/// into the cell or spilling over the next.
fn transport_mark(transport: cctop_core::reach::Transport) -> &'static str {
    match transport {
        cctop_core::reach::Transport::Peer => "☁\u{FE0E}",
        cctop_core::reach::Transport::Ssh => "⇄",
    }
}

/// The workspace tab bar: the dashboard first, then a tab per set of terminals.
fn draw_workspace_bar(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let titles: Vec<String> = (0..=app.tabs.len())
        .map(|i| {
            // The number is the Alt- key that jumps to the tab, not an ordinal
            // — past nine there is no such key, so the label stops pretending.
            match i + 1 {
                n @ 1..=9 => format!("{n}:{}", app.tab_title(i)),
                _ => app.tab_title(i),
            }
        })
        .collect();
    let mut spans = Vec::new();
    let mut pos = area.x;
    // The new-tab button is the one thing the bar exists for, so its room is
    // reserved before the labels get any: nothing else here is reachable by
    // mouse if it falls off the end. The settings tab is reserved beside it and
    // from the other end for the same reason — it is a tab like any other, and a
    // bar that pushed it off the right edge would be a bar that could not be
    // clicked its way to.
    let new_tab = match app.tab {
        // Which key to name depends on where the keyboard is: inside a pane it
        // belongs to the agent, so only the Alt- form gets through.
        0 => " + Tab (t) ",
        _ => " + Tab (Alt+n) ",
    };
    let settings = format!(" {} ", super::panes::SETTINGS_TITLE);
    let reserved = new_tab.chars().count() + settings.chars().count() + 2;
    let label_room = area.width.saturating_sub(reserved as u16) as usize;

    let hues: Vec<Option<theme::Hue>> = (0..titles.len()).map(|i| tab_hue(app, i)).collect();
    // A tab being recorded says so on the bar, where it is seen from every
    // other tab: a recording left running is a file growing for as long as the
    // agent draws, and the tab is the only place it could be noticed.
    let recording: Vec<bool> = std::iter::once(false)
        .chain(app.tabs.iter().map(tabs::Tab::recording))
        .collect();
    let markers: Vec<String> = (0..titles.len())
        .map(|i| {
            let tab = i.checked_sub(1).and_then(|t| app.tabs.get(t));
            let zoomed = tab.is_some_and(tabs::Tab::zoomed);
            let done = app.tab_attention(i) == Some(tabs::Attention::Done);
            format!(
                "{}{}",
                if done { DONE } else { "" },
                if zoomed { ZOOM } else { "" }
            )
        })
        .collect();
    // The mark and the space after it, before the title: part of the label's
    // budget, so a remote tab is no wider than a local one when the bar is
    // crowded.
    let remote: Vec<Option<&str>> = std::iter::once(None)
        .chain(
            app.tabs
                .iter()
                .map(|tab| tab.transport().map(transport_mark)),
        )
        .collect();

    // Only crowded bars pay for the crowding: while every label fits it is
    // drawn whole, and past that each tab gets an equal share. A clipped label
    // you can still count and click beats a bar that runs off the screen.
    // `titles.len()` columns go to the rules between labels — one between
    // every pair of tabs and one before the new-tab button.
    //
    // In cells, as the bar is drawn: a wide title counted in chars made the
    // bar look as if it fitted when it did not, and moved every click target
    // after it to the left of the tab it belongs to.
    let natural: usize = titles.iter().map(|t| util::cells(t) + 2).sum::<usize>()
        + titles.len()
        + recording.iter().filter(|&&r| r).count() * util::cells(REC)
        + markers.iter().map(|m| util::cells(m)).sum::<usize>()
        + remote.iter().flatten().count() * 2;
    let cap = match natural <= label_room {
        true => usize::MAX,
        false => ((label_room.saturating_sub(titles.len())) / titles.len())
            .saturating_sub(2)
            .max(3),
    };

    for (i, title) in titles.iter().enumerate() {
        if i > 0 {
            // A rule in the border's ink: the two spaces between labels were
            // never quite enough to tell where one tab ends and the next
            // begins.
            spans.push(Span::styled(
                "│",
                Style::default().fg(theme::colors().border),
            ));
            pos += 1;
        }
        let style = tab_style(app, i, hues[i]);
        spans.push(Span::styled(" ", style));
        let mut width = 1u16;
        let room = match remote[i] {
            // Dim and never bold: it says where, and the label's own colour is
            // already saying how the agent is. A painted tab keeps its fill's
            // ink, which dim on that fill would not be legible against.
            Some(mark) => {
                let quiet = match hues[i] {
                    Some(_) => style,
                    None => style.fg(theme::colors().dim),
                };
                spans.push(Span::styled(
                    format!("{mark} "),
                    quiet.remove_modifier(Modifier::BOLD),
                ));
                width += 2;
                cap.saturating_sub(2).max(1)
            }
            None => cap,
        };
        let text = format!("{} ", util::truncate(title, room));
        width += util::cells(&text) as u16;
        spans.push(Span::styled(text, style));
        // In the label's own style, so it reads as part of *that* tab rather
        // than a second one, and bold so it is not lost against the fill.
        if !markers[i].is_empty() {
            spans.push(Span::styled(
                markers[i].clone(),
                style.add_modifier(Modifier::BOLD),
            ));
            width += util::cells(&markers[i]) as u16;
        }
        // After the label rather than inside it, so it is never what the
        // elision cuts, and part of the tab's click target like the label is.
        if recording[i] {
            spans.push(Span::styled(
                REC,
                Style::default()
                    .fg(theme::colors().cost_high)
                    .add_modifier(Modifier::BOLD),
            ));
            width += util::cells(REC) as u16;
        }
        layout.workspace_spans.push((pos, pos + width, i));
        pos += width;
    }

    // The button that says the feature exists. It carries its key as well as
    // its click target, because the keyboard is how anyone will use it twice.
    let width = new_tab.chars().count() as u16;
    if pos + width < area.x + area.width {
        spans.push(Span::styled(
            "│",
            Style::default().fg(theme::colors().border),
        ));
        pos += 1;
        spans.push(Span::styled(
            new_tab,
            Style::default()
                .fg(theme::colors().accent)
                .add_modifier(Modifier::BOLD),
        ));
        layout.workspace_new = Some((pos, pos + width));
        pos += width;
    }

    // The settings tab, against the right edge rather than packed in with the
    // rest. It is the one thing on the bar that is not about the work in
    // front of you, and a fixed end is what makes it findable: someone who
    // does not know cctop has a settings tab will find it in the corner rather
    // than work out that `,` exists.
    //
    // Right-aligned rather than last-in-line, so that a bar crowded with agent
    // tabs cannot push the place everything is configured off the screen.
    let settings_w = settings.chars().count() as u16;
    let right = area.x + area.width;
    if pos + settings_w + 2 <= right {
        let at = right - settings_w;
        // Pad out to the edge with a rule, so the gap reads as "not a tab"
        // rather than as a tab whose label has been elided away.
        if at > pos {
            spans.push(Span::styled(
                "─".repeat((at - pos) as usize),
                Style::default().fg(theme::colors().border),
            ));
            pos = at;
        }
        let index = app.settings_tab();
        spans.push(Span::styled(settings, tab_style(app, index, None)));
        layout.workspace_spans.push((pos, pos + settings_w, index));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    // A restart is confirmed on the tab that took it, drawn over the label
    // once the bar is down so the sweep eases into what the tab really is.
    // Painted tabs sweep in from their own full hue, so the tab is still
    // *that* tab while it says so; a plain one from the accent.
    for &(left, right, i) in &layout.workspace_spans {
        let Some(since) = app.restart_flash(i) else {
            continue;
        };
        let wash = match hues[i] {
            Some(hue) => hue.fill(theme::Fill::Alert),
            None => theme::colors().accent,
        };
        let rect = Rect::new(left, area.y, right.saturating_sub(left), 1);
        effects::restart_sweep(
            frame.buffer_mut(),
            rect,
            since,
            wash,
            theme::ground_rgb(),
            theme::truecolor(),
        );
    }
}

/// Tab `i`'s painted hue, in [`App::tab`]'s numbering. A hue that resolves to
/// nothing — under `NO_COLOR` or the mono palette — leaves the tab drawn as an
/// unpainted one, or every painted tab would be a block of the terminal's
/// default.
fn tab_hue(app: &App, i: usize) -> Option<theme::Hue> {
    app.tab_color(i).filter(|hue| hue.color() != Color::Reset)
}

/// How tab `i` of the bar is drawn at this moment: its fill or its plain ink,
/// and whatever its agent is saying on top.
fn tab_style(app: &App, i: usize, hue: Option<theme::Hue>) -> Style {
    let attention = app.tab_attention(i);
    match hue {
        // A painted tab is filled with its colour the way a browser fills a
        // tab group: the whole cell between the rules, not the ink of the
        // label, so it reads as *that* tab from across the room. Attention
        // is said on top of the fill rather than instead of it — an idle
        // agent is the normal state of a tab, and a colour that vanished
        // every time its agent finished a turn would not be a mark at all.
        Some(hue) => {
            let watched = i == app.position();
            let strength = match watched {
                true => theme::Fill::Selected,
                false => theme::Fill::Rest,
            };
            let fill = hue.wash(strength);
            let fill = match watched {
                true => fill.add_modifier(Modifier::UNDERLINED),
                false => fill,
            };
            match attention {
                // The pulse is the tab's own colour swelling to full strength
                // and back: loud enough to be seen from another tab, and still
                // unmistakably *that* tab while it is.
                Some(tabs::Attention::NeedsInput) => needs_you(
                    app,
                    fill.add_modifier(Modifier::BOLD),
                    fill.patch(hue.wash(theme::Fill::Alert))
                        .add_modifier(Modifier::BOLD),
                ),
                // Bold rather than green ink, which on a green or cyan fill
                // would say nothing — and rather than a glyph, which made the
                // tab look like it carried a second label.
                Some(tabs::Attention::Idle | tabs::Attention::Done) => {
                    fill.add_modifier(Modifier::BOLD)
                }
                None => fill,
            }
        }
        // A tab wanting something outranks the plain selected/unselected
        // look: the whole point of the colour is to be seen while you are
        // reading a different tab.
        None => match attention {
            // Animated by hand rather than with `Modifier::SLOW_BLINK`, which
            // many terminals quietly drop — an attention cue that only works
            // on some emulators is worse than none, because you stop
            // trusting it.
            Some(tabs::Attention::NeedsInput) => needs_you(
                app,
                Style::default()
                    .fg(theme::colors().cost_mid)
                    .add_modifier(Modifier::BOLD),
                theme::attention_lit(theme::colors().cost_mid),
            ),
            // A quiet agent is useful context, not an alarm. Keep its green
            // label visible without repeatedly pulling attention from work.
            Some(tabs::Attention::Idle) => Style::default()
                .fg(theme::colors().cost_low)
                .add_modifier(Modifier::BOLD),
            // The accent, the ink of the table's own done mark, so the same
            // news is the same colour in both places. Still, unlike a
            // question: a finished turn waits for you without harm.
            Some(tabs::Attention::Done) => Style::default()
                .fg(theme::colors().accent)
                .add_modifier(Modifier::BOLD),
            None if i == app.position() => theme::selected(),
            None => Style::default().fg(theme::colors().dim),
        },
    }
}

/// A needs-you tab between its `rest` and `lit` looks: eased along the pulse
/// where the two have colours to ease between, and the hard blink where they
/// do not — under `NO_COLOR`, whose lit half is reverse video and has no
/// halfway, or on a colour the terminal's theme owns.
fn needs_you(app: &App, rest: Style, lit: Style) -> Style {
    let eased = match theme::no_color() {
        true => None,
        false => effects::pulse(
            rest,
            lit,
            theme::ground_rgb(),
            app.pulse_level(),
            theme::truecolor(),
        ),
    };
    eased.unwrap_or(match app.blink_on() {
        true => lit,
        false => rest,
    })
}

/// Every tab's look in the bar, as [`draw_workspace_bar`] would draw it now.
///
/// What the run loop compares frame to frame to decide whether the bar has
/// moved: a pulse snapped to 256 colours changes only a handful of times a
/// breath, and a frame between two identical ones is work for nothing.
pub(super) fn bar_styles(app: &App) -> Vec<Style> {
    // Past the last workspace tab, because the settings tab is drawn too and
    // the loop compares these to notice the bar moving.
    (0..=app.settings_tab())
        .map(|i| tab_style(app, i, tab_hue(app, i)))
        .collect()
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
    quota: &cctop_core::quota::Quota,
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
fn quota_suffix(
    status: &cctop_core::quota::ProviderStatus,
    now: i64,
    detail: QuotaDetail,
) -> String {
    let cctop_core::quota::ProviderStatus::Ok(quota) = status else {
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
    // The two conditions `App::active_tab` folds into its answer, asked here so
    // the tab can be reached through its own field. The settings page has none of
    // the tab's panes on screen, and `draw` returns before reaching this on it
    // anyway.
    if app.on_settings() {
        return;
    }
    let Some(index) = app.tab.checked_sub(1) else {
        return;
    };
    // The quota is borrowed, not copied. `App::active_tab` hands out a borrow of
    // the whole `App`, so reading the quota beside the tab meant reading it out
    // first — and every frame cloned every account's plan string and every one of
    // their rate-limit windows, sixty times a second inside a tab, to put two
    // percentages on a border. The fields are disjoint, so a reference into one of
    // them lives as long as a mutable borrow of the other.
    let quota = &app.quota;
    let Some(tab) = app.tabs.get_mut(index) else {
        return;
    };
    if tab.panes.is_empty() {
        return;
    }
    let focus = tab.focus;
    let zoomed = tab.zoomed();
    let share = Constraint::Ratio(1, tab.panes.len() as u32);
    let slots: Vec<Rect> = match zoomed {
        // The focused pane gets the whole tab; the rest get nothing and are
        // skipped below. Not resized to nothing, and not resized at all: see
        // [`hidden_by_zoom`].
        true => (0..tab.panes.len())
            .map(|i| if i == focus { area } else { Rect::default() })
            .collect(),
        false => match tab.stacked {
            true => RLayout::vertical(vec![share; tab.panes.len()]),
            false => RLayout::horizontal(vec![share; tab.panes.len()]),
        }
        .split(area)
        .to_vec(),
    };

    // The tab's colour, read before the panes are borrowed: a painted tab
    // carries its mark onto the title of every border it owns, which is where
    // it is visible while the bar is a row you are not looking at.
    //
    // The title wears the tab's own wash — the fill and ink it has in the bar,
    // at the watched strength, since a tab whose panes are on screen is the
    // watched one — so it reads as that tab continued down into the pane. The
    // saturated ink it used to be in was a different colour from the tab
    // above it, and said "related" where it should have said "the same".
    //
    // The border line itself is left alone. Its colour is already spoken for:
    // `border_hi` is how you tell which pane has the keyboard. And a fill is
    // made to sit behind text, not to be a line — the pale set drawn as a
    // one-cell rule on a white ground all but disappears. A hue that resolves
    // to nothing under `NO_COLOR` tints nothing either.
    let tint = tab
        .color
        .filter(|hue| hue.color() != Color::Reset)
        .map(|hue| hue.wash(theme::Fill::Selected).add_modifier(Modifier::BOLD));
    let now = chrono::Utc::now().timestamp();
    for (i, pane) in tab.panes.iter_mut().enumerate() {
        if hidden_by_zoom(zoomed, i, focus) {
            // A placeholder, so a pane's rectangle is still found at its own
            // index — the mouse maps a hit back to a pane by position — and
            // an empty one, so no click ever lands on a pane you cannot see.
            layout.pane_rects.push(Rect::default());
            continue;
        }
        let mut block = panel_block_titled(&pane.label, tint.unwrap_or_else(theme::title));
        // The border is long and empty, and the label has already told you which
        // agent this is; the quota is the other thing you want while it runs.
        // Right-aligned so it does not move when the label changes, and only if
        // it fits beside the label rather than pushing it off.
        let taken = util::cells(&pane.label) as u16 + 6;
        if let Some(text) = pane_quota(
            &pane.label,
            pane.profile.as_deref(),
            quota,
            now,
            slots[i].width.saturating_sub(taken).saturating_sub(2),
        ) {
            block = block.title_top(Line::from(Span::styled(text, theme::dim())).right_aligned());
        }
        if i == focus {
            block = block
                .border_style(Style::default().fg(theme::colors().border_hi))
                .title_bottom(Span::styled(
                    match (pane.dead.is_some(), zoomed) {
                        // Every key is "close" on a dead pane, so the hint
                        // naming one would undersell it.
                        (true, _) => " any key closes · F12 back ",
                        (false, true) => " F12 back · Alt+z unzoom ",
                        (false, false) => " F12 back · Alt+w close ",
                    },
                    theme::title(),
                ));
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
        // The drawn size, except for the instant of a nudge that takes the
        // rmux window back from another client; see `tabs::Fit`.
        let (ask_cols, ask_rows) = pane.fit_request(inner.width, inner.height);
        pane.view.resize(ask_cols, ask_rows);

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
            tui_term::widget::PseudoTerminal::new(pane.view.parser.screen()).cursor(
                // A cursor sitting in a dead pane reads as a prompt waiting
                // for input, which is the one thing it is not.
                tui_term::widget::Cursor::default().visibility(pane.dead.is_none()),
            ),
            screen,
        );
        // While another client holds a smaller rmux window, the agent fills
        // only its top-left corner and rmux redraws nothing around it — what
        // was there before stays on this client's screen, the agent's own
        // past frames at the old size. Blanked here, from the window size the
        // sweep and the keystroke checks keep.
        if pane.rmux.is_some()
            && let Some((cols, rows)) = pane.fit.window
        {
            let buf = frame.buffer_mut();
            for y in screen.y..screen.y + screen.height {
                for x in screen.x..screen.x + screen.width {
                    if x - screen.x >= cols || y - screen.y >= rows {
                        buf[(x, y)].reset();
                    }
                }
            }
        }
        // After the blanking, which is cctop's but only ever `Reset`, and
        // before the banner, which is cctop's and drawn in its palette.
        layout
            .passthrough
            .extend(theme::passthrough(frame.buffer_mut(), screen));
        if let Some(death) = &pane.dead {
            dead_banner(frame, inner, &death.banner(&pane.label));
        }
        layout.pane_rects.push(screen);
    }
}

/// The line across the bottom of a pane whose agent has died, saying how.
///
/// Inside the pane, over its last row, rather than on the border: the border
/// is already the label, the quota and the key hints, and this is the one
/// thing on the pane that has to be read. The agent's last screen stays above
/// it, which is the other half of the answer.
fn dead_banner(frame: &mut Frame, inner: Rect, text: &str) {
    if inner.height == 0 {
        return;
    }
    let row = Rect {
        y: inner.y + inner.height - 1,
        height: 1,
        ..inner
    };
    let style = theme::failed()
        .fg(theme::colors().value)
        .add_modifier(Modifier::BOLD);
    frame.render_widget(Clear, row);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(format!(" {text}"), style))).style(style),
        row,
    );
}

/// Whether pane `i` is off screen behind a zoomed one.
///
/// A hidden pane keeps running — its output is still pumped every tick, see
/// [`Tab::pump`](tabs::Tab::pump) — but it is not drawn and, deliberately, not
/// resized. Its agent goes on at the size it had in the split, so zooming in
/// and out costs the hidden agents nothing: no `SIGWINCH`, no reflow of a
/// screen nobody is reading, and no full repaint of a TUI that redraws its
/// whole frame on every resize. Only the zoomed agent is resized, once each
/// way, which is what tmux does too.
///
/// The one limit is rmux's, not this: an agent is drawn at the smallest size
/// any client watching it asked for, so a pane another cctop is also showing
/// at split size stays that size here, zoomed or not — the leftover is drawn
/// blank, as for any grant smaller than the request.
fn hidden_by_zoom(zoomed: bool, i: usize, focus: usize) -> bool {
    zoomed && i != focus
}

// The clipboard and the ssh test live in `clipboard`, below the UI: `attach`
// copies too, with no UI around it.
pub use cctop_core::clipboard::{copy_to_clipboard, over_ssh};

#[cfg(test)]
mod tests {
    use super::*;

    /// The bar books a mark and its space as two columns, so each mark has to
    /// be one column wherever it is drawn — not an emoji a terminal widens to
    /// two.
    #[test]
    fn a_transport_mark_is_one_column() {
        use cctop_core::reach::Transport;
        for transport in [Transport::Peer, Transport::Ssh] {
            let mark = transport_mark(transport);
            assert_eq!(unicode_width::UnicodeWidthStr::width(mark), 1, "{mark}");
            // Miscellaneous Symbols and Dingbats are where emoji presentation
            // lives: a glyph from there has to ask for text, or a later swap
            // brings the colour cloud back.
            let chars: Vec<char> = mark.chars().collect();
            for (i, c) in chars.iter().enumerate() {
                if ('\u{2600}'..='\u{27BF}').contains(c) {
                    assert_eq!(chars.get(i + 1), Some(&'\u{FE0E}'), "{mark}");
                }
            }
            assert_eq!(Transport::parse(transport.as_str()), Some(transport));
        }
    }

    fn window(label: &'static str, pct: u32, resets_at: i64) -> cctop_core::quota::Window {
        cctop_core::quota::Window {
            label,
            pct,
            duration: None,
            resets_at: Some(resets_at),
        }
    }

    /// A painted tab is filled with its colour, the whole cell between the
    /// rules the way a browser fills a tab group, in a pastel of its hue that
    /// strengthens for the watched tab; an unpainted one keeps the usual dim
    /// ink. Its agent going idle is said on top of the fill, not instead.
    #[test]
    fn a_painted_tab_wears_its_colour_in_the_bar() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let named = |name: &str, color: Option<&str>, activity: Option<u64>| {
            crate::tabs::Tab::for_agent(&cctop_core::rmux::Running {
                name: format!("cctop-{name}"),
                pid: None,
                cwd: None,
                attached: false,
                activity,
                label: Some(name.to_string()),
                profile: None,
                order: None,
                state: None,
                color: color.map(str::to_string),
                tab: None,
                pane: None,
                axis: None,
                transport: None,
                window: None,
            })
        };

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        app.tabs = vec![named("one", Some("violet"), None), named("two", None, None)];
        // Watching the dashboard, so no tab holds the selection wash.
        app.tab = 0;

        let mut terminal = Terminal::new(TestBackend::new(80, 1)).expect("backend");
        let mut layout = Layout::default();
        let row = |app: &App, layout: &mut Layout, terminal: &mut Terminal<TestBackend>| {
            layout.workspace_spans.clear();
            terminal
                .draw(|frame| draw_workspace_bar(frame, frame.area(), app, layout))
                .expect("draw");
            let buf = terminal.backend().buffer();
            (0..80)
                .map(|x| buf.cell((x, 0)).unwrap().symbol().to_string())
                .collect::<String>()
        };

        // `│` and `●` are multi-byte, so `find`'s byte offset is not the cell's
        // column: count the characters before the needle instead.
        let col = |text: &str, needle: &str| {
            text.find(needle)
                .map(|byte| text[..byte].chars().count() as u16)
        };

        let text = row(&app, &mut layout, &mut terminal);
        let buf = terminal.backend().buffer();
        let violet = theme::Hue::Violet.fill(theme::Fill::Rest);
        assert!(!text.contains('●'), "a busy painted tab grew a dot: {text}");
        let at = col(&text, "2:one").expect("the painted tab is not in the bar");
        let cell = buf.cell((at, 0)).unwrap();
        assert_eq!(cell.bg, violet, "the label is not on the tab's pastel");
        assert_eq!(cell.fg, theme::Fill::Rest.ink());
        // The padding is the tab too: filled up to the rule, not just behind
        // the letters.
        assert_eq!(buf.cell((at - 1, 0)).unwrap().bg, violet);
        let at = col(&text, "3:two").expect("the plain tab is not in the bar");
        assert_eq!(buf.cell((at, 0)).unwrap().fg, theme::colors().dim);
        assert_ne!(buf.cell((at, 0)).unwrap().bg, violet);

        // The same tab while it is the one being watched: a step stronger in
        // the same hue, and underlined.
        app.tab = 1;
        let text = row(&app, &mut layout, &mut terminal);
        let buf = terminal.backend().buffer();
        let at = col(&text, "2:one").expect("the painted tab is not in the bar");
        let cell = buf.cell((at, 0)).unwrap();
        assert_eq!(cell.bg, theme::Hue::Violet.fill(theme::Fill::Selected));
        // Its own strength's text, not the resting one's: on the light set the
        // two differ, and a mismatched pair is how a label vanishes into its
        // tab.
        assert_eq!(cell.fg, theme::Fill::Selected.ink());
        assert_ne!(
            cell.bg, violet,
            "the watched tab is no brighter than the rest"
        );
        assert!(cell.modifier.contains(Modifier::UNDERLINED));

        // And idle: the fill stays and the label goes bold — no glyph, so the
        // tab is no wider than it was.
        app.tab = 0;
        let width_busy = layout.workspace_spans[1];
        app.tabs[0] = named("one", Some("violet"), Some(0));
        let text = row(&app, &mut layout, &mut terminal);
        let buf = terminal.backend().buffer();
        assert!(
            !text.contains('●'),
            "an idle painted tab grew a dot: {text}"
        );
        let at = col(&text, "2:one").expect("the painted tab is not in the bar");
        let cell = buf.cell((at, 0)).unwrap();
        assert_eq!(cell.bg, violet);
        assert!(cell.modifier.contains(Modifier::BOLD));
        assert_eq!(
            layout.workspace_spans[1], width_busy,
            "the tab changed width"
        );
    }

    /// A click lands on the tab drawn under it when a title is wider than it
    /// is long: each tab's target starts where its label is drawn, so the
    /// tabs after a CJK or emoji title are not shifted onto their neighbours.
    #[test]
    fn a_wide_tab_title_keeps_every_click_target_on_its_tab() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let named = |name: &str| {
            crate::tabs::Tab::for_agent(&cctop_core::rmux::Running {
                name: format!("cctop-{name}"),
                pid: None,
                cwd: None,
                attached: false,
                activity: None,
                label: Some(name.to_string()),
                profile: None,
                order: None,
                state: None,
                color: None,
                tab: None,
                pane: None,
                axis: None,
                transport: None,
                window: None,
            })
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        app.tabs = vec![
            named("日本語プロジェクト"),
            named("⚠\u{FE0F} 🧑\u{200D}🌾 hotfix"),
            named("zsh"),
        ];
        app.tab = 0;

        let mut terminal = Terminal::new(TestBackend::new(100, 1)).expect("backend");
        let mut layout = Layout::default();
        terminal
            .draw(|frame| draw_workspace_bar(frame, frame.area(), &app, &mut layout))
            .expect("draw");
        let buf = terminal.backend().buffer();
        // The column where `n:` is drawn, by cell rather than by char, since a
        // wide character's second cell is a cell of its own.
        let label_at = |n: usize| {
            let digit = n.to_string();
            (0..99u16).find(|&x| {
                buf.cell((x, 0)).unwrap().symbol() == digit
                    && buf.cell((x + 1, 0)).unwrap().symbol() == ":"
            })
        };
        for i in 1..=3 {
            let at = label_at(i + 1).unwrap_or_else(|| panic!("tab {i} is not in the bar"));
            let (left, right, index) = layout.workspace_spans[i];
            assert_eq!(index, i);
            // One cell of padding before the label is the tab's too.
            assert_eq!(left + 1, at, "tab {i}'s click target starts elsewhere");
            assert!(at < right);
        }
    }

    /// A tab whose agent is asking pulses between its resting fill and its
    /// alert tone rather than switching: at rest it is exactly its resting
    /// self, at the peak exactly the alert the blink used to show, and in
    /// between a ground that is neither, under the ink of whichever end it is
    /// nearer. The unpainted tab beside it does the same toward amber.
    #[test]
    fn a_tab_that_needs_you_pulses_between_its_fill_and_its_alert() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use std::time::{Duration, Instant};

        let asking = |name: &str, color: Option<&str>| {
            crate::tabs::Tab::for_agent(&cctop_core::rmux::Running {
                name: format!("cctop-{name}"),
                pid: None,
                cwd: None,
                attached: false,
                activity: None,
                label: Some(name.to_string()),
                profile: None,
                order: None,
                state: Some(cctop_core::rmux::State {
                    signal: cctop_core::hook::Signal::NeedsInput,
                    at: cctop_core::util::now_secs(),
                }),
                color: color.map(str::to_string),
                tab: None,
                pane: None,
                axis: None,
                transport: None,
                window: None,
            })
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        app.tabs = vec![asking("one", Some("violet")), asking("two", None)];
        app.tab = 0;
        assert_eq!(
            app.tab_attention(1),
            Some(tabs::Attention::NeedsInput),
            "the fixture is not asking"
        );

        let mut terminal = Terminal::new(TestBackend::new(80, 1)).expect("backend");
        // The bar `ms` into the pulse: the first cell of each tab's label.
        let mut at = |app: &mut App, ms: u64| {
            app.started = Instant::now()
                .checked_sub(Duration::from_millis(ms))
                .expect("the clock is younger than the phase");
            let mut layout = Layout::default();
            terminal
                .draw(|frame| draw_workspace_bar(frame, frame.area(), app, &mut layout))
                .expect("draw");
            let buf = terminal.backend().buffer();
            let cell = |i: usize| {
                let (left, _, _) = layout.workspace_spans[i];
                buf.cell((left + 1, 0)).unwrap().clone()
            };
            (cell(1), cell(2))
        };
        let violet = theme::Hue::Violet;
        let amber = theme::colors().cost_mid;

        let (painted, plain) = at(&mut app, 0);
        assert_eq!(painted.bg, violet.fill(theme::Fill::Rest));
        assert_eq!(painted.fg, theme::Fill::Rest.ink());
        assert!(painted.modifier.contains(Modifier::BOLD));
        assert_eq!(plain.fg, amber);
        assert_ne!(plain.bg, amber, "the plain tab is lit at rest");

        let (painted, plain) = at(&mut app, effects::PULSE_PERIOD.as_millis() as u64 / 2);
        assert_eq!(painted.bg, violet.fill(theme::Fill::Alert));
        assert_eq!(painted.fg, theme::Fill::Alert.ink());
        assert_eq!(plain.bg, amber, "the plain tab never lights up");

        // A quarter period in, and the three-quarter mark on the way down:
        // halfway in level, so a ground that is neither end — on the ink of
        // one end or the other, never a third.
        for ms in [300, 900] {
            let (painted, _) = at(&mut app, ms);
            assert_ne!(painted.bg, violet.fill(theme::Fill::Rest), "{ms}ms");
            assert_ne!(painted.bg, violet.fill(theme::Fill::Alert), "{ms}ms");
            assert!(
                [theme::Fill::Rest.ink(), theme::Fill::Alert.ink()].contains(&painted.fg),
                "{ms}ms: {:?}",
                painted.fg
            );
        }

        // And the restart sweep: over the tab while it runs, gone after.
        app.tabs[0].restarted = Some(Instant::now());
        let (painted, _) = at(&mut app, 0);
        assert_ne!(painted.bg, violet.fill(theme::Fill::Rest), "no sweep");
        app.tabs[0].restarted = Instant::now().checked_sub(effects::FLASH * 2);
        let (painted, _) = at(&mut app, 0);
        assert_eq!(
            painted.bg,
            violet.fill(theme::Fill::Rest),
            "the sweep stayed"
        );
    }

    /// A painted tab's panes carry its colour as the bar does: the title on the
    /// border is filled with the watched tab's wash, padding and all, so it is
    /// the tab continued down rather than a label in a related ink. The border
    /// line keeps saying which pane has the keyboard, and an unpainted tab's
    /// title is the plain one it always was.
    #[test]
    fn a_painted_tab_wears_its_colour_on_its_panes() {
        use crate::tabs::{Pane, Tab};
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        let mut painted = Tab::new(Pane::for_test("claude"));
        painted.color = Some(theme::Hue::Violet);
        app.tabs = vec![painted, Tab::new(Pane::for_test("codex"))];

        let mut terminal = Terminal::new(TestBackend::new(40, 6)).expect("backend");
        // The top border of the tab at `index`: `╭`, then the title's leading
        // pad, then its first letter.
        let mut top = |app: &mut App, index: usize| {
            app.tab = index;
            terminal
                .draw(|frame| draw_panes(frame, frame.area(), app, &mut Layout::default()))
                .expect("draw");
            let buf = terminal.backend().buffer();
            (0..3)
                .map(|x| buf.cell((x, 0)).unwrap().clone())
                .collect::<Vec<_>>()
        };

        let cells = top(&mut app, 1);
        let wash = theme::Hue::Violet.wash(theme::Fill::Selected);
        assert_eq!(cells[2].symbol(), "c", "the title is not where it was");
        for cell in &cells[1..=2] {
            assert_eq!(
                cell.bg,
                wash.bg.unwrap(),
                "the title is not on the tab's fill"
            );
            assert_eq!(
                cell.fg,
                wash.fg.unwrap(),
                "the title's text is not the tab's ink"
            );
        }
        assert_eq!(
            cells[0].fg,
            theme::colors().border_hi,
            "the focused pane's border lost its focus colour"
        );

        // The unpainted tab: the plain title, on no fill at all.
        let cell = top(&mut app, 2).remove(2);
        assert_eq!(cell.symbol(), "c");
        assert_eq!(cell.fg, theme::title().fg.unwrap());
        assert_ne!(cell.bg, wash.bg.unwrap());
    }

    /// A figure the machine column cannot fit is left out, not cut to a
    /// different number at the panel edge.
    #[test]
    fn the_overview_never_shows_a_figure_without_its_unit() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::tests::test_app();
        app.stats.total_input = 73_200;
        app.stats.total_output = 2_200;
        for cols in [80u16, 200] {
            let mut terminal = Terminal::new(TestBackend::new(cols, 30)).expect("backend");
            terminal
                .draw(|frame| {
                    draw(frame, &mut app);
                })
                .expect("draw");
            let text = screen(&terminal, cols, 6).join("\n");
            assert_eq!(
                text.contains("73.2"),
                text.contains("73.2K in / 2.2K out"),
                "{text}"
            );
        }
    }

    /// Three expired accounts on 120 columns keep the whole login command, and
    /// on 80, where nothing fits, the cut is marked rather than mid-word.
    #[test]
    fn a_narrow_limits_column_shortens_its_words_before_its_command() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let expired = |profile: &str| cctop_core::quota::ProfileQuota {
            profile: profile.to_string(),
            status: cctop_core::quota::ProviderStatus::Expired,
            source: cctop_core::config::AccountSource::Directory,
        };
        let mut app = crate::tests::test_app();
        app.quota.claude = vec![expired("default")];
        app.quota.codex = vec![expired("default"), expired("work")];
        for (cols, wanted) in [(120u16, "Codex (work) expired — codex login"), (80, "…")] {
            let mut terminal = Terminal::new(TestBackend::new(cols, 40)).expect("backend");
            terminal
                .draw(|frame| {
                    draw(frame, &mut app);
                })
                .expect("draw");
            let limits = screen(&terminal, cols, 40)
                .into_iter()
                .find(|l| l.contains("Codex (work)"))
                .expect("no Limits line");
            assert!(limits.contains(wanted), "{limits:?}");
        }
    }

    /// A short terminal drops whole panels rather than drawing half of one,
    /// and never the footer.
    #[test]
    fn a_short_terminal_drops_whole_panels_and_keeps_the_footer() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::tests::test_app();
        for (cols, rows) in [(40u16, 12u16), (20, 5)] {
            let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
            terminal
                .draw(|frame| {
                    draw(frame, &mut app);
                })
                .expect("draw");
            let lines = screen(&terminal, cols, rows);
            let text = lines.join("\n");
            assert!(!text.contains("Limits"), "{text}");
            assert!(text.contains("Sessions"), "{text}");
            assert!(lines[rows as usize - 1].contains("Move"), "{text}");
        }
    }

    /// On the 80-column terminal most people open, with the share corner
    /// taking its columns, the help key is still on the footer.
    #[test]
    fn an_80_column_footer_still_says_how_to_get_help() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::tests::test_app();
        app.sessions = vec![crate::tests::session("a", false, "proj")];
        app.visible = vec![crate::Row::Session(0)];
        app.selected = 0;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("backend");
        terminal
            .draw(|frame| {
                draw(frame, &mut app);
            })
            .expect("draw");
        let footer = screen(&terminal, 80, 24).pop().unwrap_or_default();
        assert!(footer.contains("? Help"), "{footer:?}");
    }

    #[test]
    fn pane_quota_narrows_to_fit_and_only_for_a_provider() {
        let quota = cctop_core::quota::Quota {
            claude: vec![cctop_core::quota::ProfileQuota {
                profile: "default".into(),
                source: cctop_core::config::AccountSource::Directory,
                status: cctop_core::quota::ProviderStatus::Ok(cctop_core::quota::ProviderQuota {
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
        let separators = titles.len();
        let cap = ((label_room.saturating_sub(separators)) / titles.len())
            .saturating_sub(2)
            .max(3);
        let drawn: usize = titles
            .iter()
            .map(|t| util::cells(&util::truncate(t, cap)) + 2)
            .sum::<usize>()
            + separators;

        assert!(drawn <= label_room, "{drawn} columns in {label_room}");
        assert_eq!(util::truncate("1:claude-1", cap), "1:…");
        assert_eq!(util::truncate("1:cc", cap), "1:…");
    }

    #[test]
    fn quota_suffix_drops_detail_step_by_step() {
        let status = cctop_core::quota::ProviderStatus::Ok(cctop_core::quota::ProviderQuota {
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
                &cctop_core::quota::ProviderStatus::Pending,
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
            share_local: None,
            header_row: 6,
            rows_start: 7,
            rows_end: 12,
            tab_row: 20,
            bottom_start: 20,
            column_spans: vec![(0, 5, ColumnId::Status), (5, 12, ColumnId::Cost)],
            tab_spans: vec![(2, 6, 0), (8, 19, 1)],
            tool_sidebar: Some((18, 21, 0, 3)),
            tool_log: Some((19, 21, 4)),
            table_track: None,
            panel_track: None,
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
            passthrough: Vec::new(),
            address_row: Some((15, 11, 29)),
            subagent_header: vec![
                (19, 0, 9, panels::SubagentSort::Last),
                (19, 9, 22, panels::SubagentSort::Type),
            ],
            settings_rows: vec![(9, 0), (10, 3), (17, 4)],
            settings_inner: Some(Rect::new(0, 8, 100, 10)),
        };
        // An address answers a right-click on its own row and columns only.
        assert!(layout.address_at(11, 15));
        assert!(layout.address_at(28, 15));
        assert!(!layout.address_at(29, 15));
        assert!(!layout.address_at(11, 14));
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
        // The settings page's rows are hit-tested inside the block they were
        // drawn in: a heading shares a row with nothing, and the border is
        // outside the rectangle the rows live in.
        assert_eq!(layout.settings_row_at(20, 9), Some(0));
        assert_eq!(layout.settings_row_at(20, 10), Some(3));
        assert_eq!(layout.settings_row_at(20, 11), None);
        assert_eq!(layout.settings_row_at(20, 7), None);
        assert_eq!(layout.settings_row_at(20, 18), None);
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
        // The Subagents panel's header answers only its own row, and each
        // column only its own span of it.
        assert_eq!(
            layout.subagent_header_at(3, 19),
            Some(panels::SubagentSort::Last)
        );
        assert_eq!(
            layout.subagent_header_at(10, 19),
            Some(panels::SubagentSort::Type)
        );
        assert_eq!(layout.subagent_header_at(3, 20), None);
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

    /// A test agent that draws `text` once and then sits there, so anything on
    /// screen came from the replay and anything that moves came from a resize.
    fn test_pane(text: &str) -> (std::process::Child, u32, super::super::tabs::Pane) {
        let (child, pid) = cctop_core::shim::test_session(
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
    fn draw_until_sized(
        terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
        app: &mut App,
        want: &[(u16, u16)],
    ) -> bool {
        let sized = cctop_core::test_wait::waits_for(|| {
            terminal
                .draw(|frame| {
                    draw(frame, app);
                })
                .expect("draw");
            app.tabs.iter_mut().any(|tab| {
                tab.pump();
                tab.panes.len() == want.len()
                    && tab.panes.iter().zip(want).all(|(p, w)| p.view.size == *w)
            })
        });
        if sized {
            // One more frame, so what is asserted was drawn at the final size.
            terminal
                .draw(|frame| {
                    draw(frame, app);
                })
                .expect("draw");
        }
        sized
    }

    /// Draw until `text` appears on the screen, or the budget runs out.
    ///
    /// The resize above is one write from the shim and the agent's own output is
    /// another, and nothing orders them: a pane can be granted the right size
    /// while its content is still in flight. A test that asserted on the screen
    /// straight after the resize was therefore asserting on a race, and under
    /// load it lost.
    ///
    /// Bounded like the resize wait, and for the same reason — a failure has to
    /// be a failure rather than a hang, and the screen it gives up on is the
    /// evidence of what did arrive.
    fn draw_until_text(
        terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
        app: &mut App,
        text: &str,
    ) -> bool {
        cctop_core::test_wait::waits_for(|| {
            app.tabs.iter_mut().for_each(|tab| {
                tab.pump();
            });
            terminal
                .draw(|frame| {
                    draw(frame, app);
                })
                .expect("draw");
            let area = terminal.backend().buffer().area;
            screen(terminal, area.width, area.height)
                .iter()
                .any(|row| row.contains(text))
        })
    }

    /// The way in has to be on screen before anyone has used it: with no tabs
    /// open the bar still names the dashboard and offers the new-tab button, and
    /// clicking that button is what `t` does.
    #[test]
    fn the_bar_offers_a_new_tab_with_nothing_open() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
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
            top.starts_with(" 1:Dashboard │ + Tab (t) "),
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
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let expired = |profile: &str, source| cctop_core::quota::ProfileQuota {
            profile: profile.to_string(),
            status: cctop_core::quota::ProviderStatus::Expired,
            source,
        };
        use cctop_core::config::AccountSource::{Directory, Token};
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        app.quota.claude = vec![
            expired("default", Directory),
            expired("side", Token),
            cctop_core::quota::ProfileQuota {
                profile: "work2".into(),
                status: cctop_core::quota::ProviderStatus::NoLimits,
                source: Token,
            },
        ];
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

        // And an account that is only a token is never "expired", whether the
        // endpoint said 403 or 401 (or an older cctop cached the 401 as
        // expired): a `setup-token` token cannot read limits, so the refusal
        // is its normal state, and there is no command that would change it.
        for profile in ["Claude (side)", "Claude (work2)"] {
            let line = screen
                .lines()
                .find(|l| l.contains(profile))
                .expect("the token account was not drawn");
            let mine = &line[line.find(profile).expect("found above")..];
            let mine = mine.split("  ").next().unwrap_or(mine);
            assert!(
                mine.contains("no limits for a token")
                    && !mine.contains("expired")
                    && !mine.contains("--add-account")
                    && !mine.contains("login"),
                "a token account was told it is broken: {mine:?}"
            );
        }
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
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
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
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
        use cctop_core::session::Session;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        // A stopped session: nothing to type into and nothing to terminate, so
        // the menu carries two refusals and this actually tests the case.
        let mut session = Session::new(cctop_core::pricing::Provider::Claude, "abc".into());
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

    /// A paste is confirmed by seeing the image, not just by the status line
    /// naming its file: the corner box carries the name and the halfblocks.
    #[test]
    fn a_pasted_image_is_previewed_in_the_corner() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::tests::test_app();
        // A checkerboard, so the '▀' of a halfblock is earned in every cell —
        // a uniform picture renders as background alone and asserts nothing.
        let mut pixels = image::RgbaImage::new(16, 16);
        for (x, y, px) in pixels.enumerate_pixels_mut() {
            *px = match (x + y) % 2 == 0 {
                true => image::Rgba([255, 0, 0, 255]),
                false => image::Rgba([0, 0, 255, 255]),
            };
        }
        app.paste_preview = Some(crate::PastePreview {
            name: "paste-20260922-120000.png".into(),
            at: std::time::Instant::now(),
            image: ratatui_image::picker::Picker::halfblocks()
                .new_resize_protocol(image::DynamicImage::ImageRgba8(pixels)),
        });

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("backend");
        let mut layout = Layout::default();
        terminal
            .draw(|frame| layout = draw(frame, &mut app))
            .expect("draw");
        let screen = screen(&terminal, 80, 24).join("\n");

        assert!(
            screen.contains("paste-20260922-120000.png"),
            "the preview never named the file: {screen}"
        );
        assert!(
            screen.contains('▀'),
            "no halfblock was drawn for the image: {screen}"
        );
    }

    /// A message goes up over the Overview and the footer keeps its keys — the
    /// row the old status line used to take over for a few seconds.
    #[test]
    fn a_message_is_a_toast_over_the_overview_and_the_footer_keeps_its_keys() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::tests::test_app();
        app.set_status("Restarted 3 tabs, skipped 1 mid-turn");
        app.set_status("Tab renamed to api");
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("backend");
        let mut layout = Layout::default();
        terminal
            .draw(|frame| layout = draw(frame, &mut app))
            .expect("draw");
        let screen = screen(&terminal, 100, 30);

        // Row 0 is the tab bar and row 1 the Overview's top border.
        assert!(
            screen[2].contains("Tab renamed to api"),
            "the newest message is not at the top of the stack: {screen:#?}"
        );
        assert!(
            screen[3].contains("Restarted 3 tabs, skipped 1 mid-turn"),
            "the earlier message was not kept under it: {screen:#?}"
        );
        assert!(
            !screen[29].contains("Tab renamed"),
            "the footer still carries the message: {:?}",
            screen[29]
        );
        assert!(
            !layout.key_hits.is_empty(),
            "the footer lost its keys to the message"
        );
    }

    /// Inside a tab the bottom of the frame is the agent's, so a toast has to
    /// land on cctop's Overview and leave the agent's screen alone.
    #[test]
    fn a_toast_in_a_tab_stays_off_the_agents_screen() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (mut child, pid, pane) = test_pane("HELLO-FROM-AGENT");
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        app.tabs.push(super::super::tabs::Tab::new(pane));
        app.tab = 1;
        app.set_status("Pasted paste-20260922-120000.png");

        let (cols, rows) = (100u16, 24u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        terminal
            .draw(|frame| {
                draw(frame, &mut app);
            })
            .expect("draw");
        let screen = screen(&terminal, cols, rows);

        app.tabs.clear();
        let _ = child.kill();
        let _ = child.wait();
        let _ = cctop_core::shim::socket_path(pid).map(std::fs::remove_file);

        let at = screen
            .iter()
            .position(|row| row.contains("Pasted paste-20260922-120000.png"))
            .unwrap_or_else(|| panic!("the toast was not drawn: {screen:#?}"));
        assert_eq!(at, 2, "the toast is not on the Overview: {screen:#?}");
    }

    /// A tab keeps you inside cctop: the tab bar, the Overview and the footer
    /// stay, and the agent is resized into what is left rather than cropped.
    #[test]
    fn a_tab_resizes_its_agent_into_the_space_cctop_leaves_it() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
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
        // The pane being the right size is not the pane having been drawn into:
        // the shim's resize and the agent's own output are separate writes.
        let arrived = draw_until_text(&mut terminal, &mut app, "HELLO-FROM-AGENT");
        let screen = screen(&terminal, cols, rows);

        app.tabs.clear();
        let _ = child.kill();
        let _ = child.wait();
        let _ = cctop_core::shim::socket_path(pid).map(std::fs::remove_file);

        assert!(
            sized,
            "the pty was never resized to the pane; wanted {want:?}"
        );
        assert!(arrived, "the agent's output never arrived: {screen:#?}");
        assert!(
            screen[0].starts_with(" 1:Dashboard │ 2:HELLO-FROM"),
            "the tab bar is not the top row: {:?}",
            screen[0]
        );
        // The settings tab is on the right end of the bar even on a 60-column
        // terminal, which is the point of reserving its room before the labels
        // get any: at this width the agent tab's label is shortened to pay for
        // it, because a label that is elided is still a label you can click
        // and a settings tab that fell off the end would not be a tab at all.
        assert!(
            screen[0].ends_with(" Settings "),
            "the settings tab is not at the right end: {:?}",
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

    /// Zoomed, the focused pane is drawn over the whole tab and the other one
    /// not at all — not even as a click target — and the bar says so.
    #[test]
    fn a_zoomed_pane_fills_the_tab_and_hides_the_rest() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        let mut tab = tabs::Tab::new(tabs::Pane::for_test("left"));
        tab.split(tabs::Pane::for_test("right"), false);
        assert_eq!(tab.toggle_zoom(), Some(true));
        app.tabs.push(tab);
        app.tab = 1;

        let (cols, rows) = (80u16, 21u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        let mut layout = Layout::default();
        terminal
            .draw(|frame| layout = draw(frame, &mut app))
            .expect("draw");
        let screen = screen(&terminal, cols, rows);

        assert_eq!(layout.pane_rects.len(), 2, "a pane lost its index");
        assert_eq!(
            layout.pane_rects[0],
            Rect::default(),
            "the hidden pane is clickable"
        );
        assert!(
            layout.pane_rects[1].width > cols / 2,
            "the zoomed pane did not get the tab: {:?}",
            layout.pane_rects[1]
        );
        assert!(
            screen[0].contains(ZOOM.trim()),
            "the bar does not say the tab is zoomed: {:?}",
            screen[0]
        );
        // Below the bar, whose label is the tab's and may well name it.
        assert!(
            screen[1..].iter().all(|line| !line.contains("left")),
            "the hidden pane's border was drawn: {screen:#?}"
        );
    }

    /// A split gives each agent a real screen of its own, not two crops of one:
    /// both are resized to their half and both draw in it.
    #[test]
    fn a_split_sizes_both_agents_to_their_own_half() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
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
        // Both agents, for the reason the single-pane test gives: a granted
        // resize is a different write from the agent's own output.
        let left = draw_until_text(&mut terminal, &mut app, "LEFT-AGENT");
        let right = draw_until_text(&mut terminal, &mut app, "RIGHT-AGENT");
        let screen = screen(&terminal, cols, rows);

        app.tabs.clear();
        for child in [&mut left_child, &mut right_child] {
            let _ = child.kill();
            let _ = child.wait();
        }
        for pid in [left_pid, right_pid] {
            let _ = cctop_core::shim::socket_path(pid).map(std::fs::remove_file);
        }

        assert!(
            sized,
            "one of the split panes was never resized; wanted {want:?} each"
        );
        assert!(
            left && right,
            "an agent's output never arrived: {screen:#?}"
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

    /// The Subagents panel's header answers clicks while it is on screen, and
    /// stops the moment the panel is scrolled past it.
    #[test]
    fn the_subagents_header_is_clickable_only_while_it_shows() {
        use cctop_core::session::{SessionData, Subagent, SubagentStatus};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let draw = |scroll: u16| {
            let mut app = crate::tests::test_app();
            app.sessions = vec![crate::tests::session("a", false, "x")];
            app.loaded = true;
            app.refilter();
            app.bottom_tab = 4;
            app.subagent_scroll = scroll;
            // More agents than the pane has rows, so scrolling can actually
            // take the header off screen.
            let agent = |id: &str| Subagent {
                agent_id: id.into(),
                agent_type: "explore".into(),
                description: String::new(),
                model: "m".into(),
                started_at: None,
                last_active: None,
                duration_ms: 0,
                status: SubagentStatus::Done,
                cost: 0.0,
                tool_count: 0,
                turns: 0,
                tokens: 0,
                last_text: None,
                tool_use_id: None,
                context: None,
                ghost: false,
            };
            app.panel_data = Some(SessionData {
                subagents: vec![agent("s1"), agent("s2"), agent("s3")],
                ..Default::default()
            });
            let mut terminal = Terminal::new(TestBackend::new(120, 5)).expect("backend");
            let mut layout = Layout::default();
            terminal
                .draw(|frame| draw_bottom(frame, frame.area(), &mut app, &mut layout))
                .expect("draw");
            layout
        };
        let shown = draw(0);
        assert_eq!(
            shown.subagent_header_at(3, 1),
            Some(panels::SubagentSort::Last),
            "LAST is the first header cell, on the panel's first content row"
        );
        assert_eq!(
            shown.subagent_header_at(10, 2),
            None,
            "row two is the first agent's row, not the header"
        );
        let scrolled = draw(1);
        assert_eq!(
            scrolled.subagent_header_at(3, 1),
            None,
            "a scrolled panel's rows own every cell, header included"
        );
    }
}
