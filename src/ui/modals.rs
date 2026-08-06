//! Modal overlays: help, filters, and the confirmation dialogs.

use super::columns::COLUMNS;
use super::theme;
use super::{AGE_OPTIONS, App, BatchKind, session_root_pid};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};

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

pub(super) fn draw_help(frame: &mut Frame, area: Rect) {
    let section = |t: &str| Line::from(Span::styled(t.to_string(), theme::title()));
    let item = |k: &str, d: &str| {
        Line::from(vec![
            Span::styled(format!("  {k:<16}"), Style::default().fg(theme::ACCENT)),
            Span::raw(d.to_string()),
        ])
    };
    let lines = vec![
        section("Navigation"),
        item("↑  ↓/j", "Move between sessions"),
        item("PgUp / PgDn", "Page through the list"),
        item("Ctrl+U / Ctrl+D", "Half a page up / down"),
        item("b / PgUp", "Page up"),
        item("g / G", "Jump to first / last"),
        item("Home / End", "Jump to first / last"),
        item("n / N", "Next / previous search match (wraps)"),
        Line::default(),
        section("Panels"),
        item("←  →", "Move between bottom panels"),
        item("Tab / Shift+Tab", "Same, either direction"),
        item("1 – 7", "Jump to a panel directly"),
        item("Shift+↑ / ↓", "Scroll inside the active panel"),
        item("f", "Follow mode: keep the selection centered"),
        item("L", "Toggle the Tool Activity live filter"),
        item("v", "Toggle inline diffs for edits"),
        Line::default(),
        section("Filter and sort"),
        item("/ or F3", "Filter sessions by text"),
        item("F6  >  <", "Open the sort-by panel"),
        item("F7", "Filter by age (1d / 1w / 1mo)"),
        item("#", "Cost floor: only sessions costing ≥ $X"),
        item("`", "Show only running sessions"),
        item("P / M / T", "Sort by status / memory / cost"),
        item("H / X / S", "Sort by harness / context / tools"),
        item("+ / - / =", "Speed up / slow down / reset refresh"),
        item("Esc", "Clear the active filter"),
        Line::default(),
        section("Batch actions"),
        item("Space", "Mark / unmark the selected session"),
        item("D", "Delete all marked sessions"),
        item("K", "Terminate all marked live sessions"),
        item("U", "Clear all marks"),
        Line::default(),
        section("Other"),
        item("y", "Copy resume command or transcript path"),
        item("d", "Delete the selected session (not running)"),
        item("k", "Terminate the selected live session"),
        item("s", "Type a line into the session's terminal"),
        item("a", "Attach to the session's terminal (F12 detaches)"),
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

pub(super) fn draw_search(frame: &mut Frame, area: Rect, app: &App) {
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

pub(super) fn draw_sortby(frame: &mut Frame, area: Rect, app: &App) {
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

pub(super) fn draw_age_filter(frame: &mut Frame, area: Rect, app: &App) {
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

pub(super) fn draw_delete_confirm(frame: &mut Frame, area: Rect, app: &App) {
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

pub(super) fn draw_delete_blocked(frame: &mut Frame, area: Rect, app: &App) {
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

pub(super) fn draw_kill_confirm(frame: &mut Frame, area: Rect, app: &App) {
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
            "  Send a termination signal to this agent process?",
            Style::default().fg(theme::COST_MID),
        )),
        Line::from(Span::raw("  Unsaved work in the agent may be interrupted.")),
        Line::default(),
        Line::from(Span::styled(
            "  [y] terminate    [n / Esc] cancel",
            theme::dim(),
        )),
    ];
    modal(frame, area, "Terminate session?", lines, 62);
}

pub(super) fn draw_kill_blocked(frame: &mut Frame, area: Rect, app: &App) {
    let Some(s) = app.selected_session() else {
        return;
    };
    let lines = vec![
        Line::from(Span::styled(
            format!("  {}", s.display_label()),
            theme::value(),
        )),
        Line::default(),
        Line::from(Span::raw(
            "  This session has no locally controllable process.",
        )),
        Line::from(Span::raw("  It may be running in a remote or shared host.")),
        Line::default(),
        Line::from(Span::styled("  [any key] dismiss", theme::dim())),
    ];
    modal(frame, area, "Cannot terminate", lines, 62);
}

pub(super) fn draw_batch_confirm(frame: &mut Frame, area: Rect, app: &App) {
    let ms = app.marked_sessions();
    let (verb, noun) = match app.batch {
        BatchKind::Delete => ("delete", "sessions"),
        BatchKind::Kill => ("terminate", "live sessions"),
    };
    let mut lines = vec![
        Line::from(Span::styled(
            format!("  {} marked {noun}", ms.len()),
            theme::value(),
        )),
        Line::default(),
    ];
    for s in ms.iter().take(8) {
        lines.push(Line::from(Span::styled(
            format!("    · {}", s.display_label()),
            theme::dim(),
        )));
    }
    if ms.len() > 8 {
        lines.push(Line::from(Span::styled(
            format!("    … and {} more", ms.len() - 8),
            theme::dim(),
        )));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        match app.batch {
            BatchKind::Delete => "  This permanently removes their transcripts from disk.",
            BatchKind::Kill => "  Unsaved work in the agents may be interrupted.",
        },
        Style::default().fg(theme::COST_MID),
    )));
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        format!("  [y] {verb} all    [n / Esc] cancel"),
        theme::dim(),
    )));
    modal(frame, area, &format!("{verb} all?"), lines, 62);
}

pub(super) fn draw_batch_blocked(frame: &mut Frame, area: Rect, app: &App, deleting: bool) {
    let ms = app.marked_sessions();
    // First one that can't be processed, for the explanation.
    let (explain, name) = match app.batch {
        BatchKind::Delete => (
            "running — stop the agent first",
            ms.iter().find(|s| s.is_running()),
        ),
        BatchKind::Kill => (
            "has no locally controllable process",
            ms.iter().find(|s| session_root_pid(s).is_none()),
        ),
    };
    let lines = vec![
        Line::from(Span::styled(
            format!("  {} marked sessions", ms.len()),
            theme::value(),
        )),
        Line::default(),
        Line::from(Span::raw(format!(
            "  Not all can be {}.",
            if deleting { "deleted" } else { "killed" }
        ))),
        match name {
            Some(s) => Line::from(Span::styled(
                format!("  · {} {}", s.display_label(), explain),
                theme::dim(),
            )),
            None => Line::from(Span::raw("  At least one is not ready.")),
        },
        Line::default(),
        Line::from(Span::styled(
            "  e.g. unmark the running / remote sessions first.",
            theme::dim(),
        )),
        Line::default(),
        Line::from(Span::styled("  [any key] dismiss", theme::dim())),
    ];
    let title = if deleting {
        "Cannot delete all"
    } else {
        "Cannot terminate all"
    };
    modal(frame, area, title, lines, 62);
}

pub(super) fn draw_cost_filter(frame: &mut Frame, area: Rect, app: &App) {
    let mut input = app.cost_input.clone();
    if input.is_empty() {
        input = "0.00".to_string();
    }
    let lines = vec![
        Line::from(Span::styled(
            " Only show sessions whose total cost is at least:",
            theme::dim(),
        )),
        Line::from(vec![
            Span::raw(" $ "),
            Span::styled(
                input,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(theme::ACCENT)),
        ]),
        Line::default(),
        Line::from(Span::styled(
            " 0 clears the filter   Enter apply   Esc cancel",
            theme::dim(),
        )),
    ];
    modal(frame, area, "Cost floor", lines, 50);
}

pub(super) fn draw_send_keys(frame: &mut Frame, area: Rect, app: &App) {
    let Some(s) = app.selected_session() else {
        return;
    };
    let lines = vec![
        Line::from(Span::styled(
            format!(" {}", s.display_label()),
            theme::value(),
        )),
        Line::from(Span::styled(
            " Typed into the terminal running this agent, then submitted.",
            theme::dim(),
        )),
        Line::from(Span::styled(
            " Needs the agent under `cctop run`, tmux, or cctop as root.",
            theme::dim(),
        )),
        Line::default(),
        Line::from(vec![
            Span::raw(" > "),
            Span::styled(
                app.send_input.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(theme::ACCENT)),
        ]),
        Line::default(),
        Line::from(Span::styled(" Enter send   Esc cancel", theme::dim())),
    ];
    modal(frame, area, "Send to session", lines, 64);
}

// ---------------------------------------------------------------------------
// Clipboard
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
