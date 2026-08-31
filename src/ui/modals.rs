//! Modal overlays: help, filters, and the confirmation dialogs.

use super::columns::COLUMNS;
use super::render::Layout;
use super::theme;
use super::{AGE_OPTIONS, App, BatchKind, LaunchInto, tabs};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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

/// Draws the overlay and returns `(outer, inner)`: the frame it covers, which is
/// what tells a click inside the modal from one meant for what it hides, and the
/// text area, whose rows are the lines that were passed in.
fn modal(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    width: u16,
) -> (Rect, Rect) {
    // Wrapped rows, not lines: the paragraph below wraps, and a box sized to
    // the line count is a box one line too short for every line that wrapped —
    // which is how content ends up drawn through the bottom border instead of
    // inside it.
    let inner_width = width.saturating_sub(2).max(1);
    let height = lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(inner_width as usize) as u16)
        .sum::<u16>()
        + 2;
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border_hi))
        .title(Span::styled(format!(" {title} "), theme::title()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    (rect, inner)
}

/// A modal whose content may be taller than the screen.
///
/// Returns the largest useful scroll offset so the key handler knows where the
/// bottom is; the caller stores it on the app. Without this the tail of a long
/// overlay is simply cut off by `centered`, with nothing on screen to say that
/// there is more — which is exactly how the Tabs section of the help went
/// missing on anything shorter than about 57 rows.
fn scrollable_modal(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    width: u16,
    scroll: u16,
) -> u16 {
    let total = lines.len() as u16;
    let rect = centered(area, width, total + 2);
    frame.render_widget(Clear, rect);

    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border_hi))
        .title(Span::styled(format!(" {title} "), theme::title()));

    let inner_height = block.inner(rect).height;
    let max_scroll = total.saturating_sub(inner_height);
    let scroll = scroll.min(max_scroll);
    if max_scroll > 0 {
        // On the border, so it costs no content line and can't scroll away.
        block = block.title_bottom(Span::styled(
            format!(
                " {}–{} of {total}   ↑↓ scroll ",
                scroll + 1,
                (scroll + inner_height).min(total)
            ),
            theme::dim(),
        ));
    }
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        inner,
    );
    max_scroll
}

pub(super) fn draw_help(frame: &mut Frame, area: Rect, app: &mut App) {
    let section = |t: &str| Line::from(Span::styled(t.to_string(), theme::title()));
    let item = |k: &str, d: &str| {
        Line::from(vec![
            Span::styled(
                format!("  {k:<16}"),
                Style::default().fg(theme::colors().accent),
            ),
            Span::raw(d.to_string()),
        ])
    };
    let lines = vec![
        // The order is what a reader needs first, not what the key handler
        // happens to match first: the row menu is the one entry that makes the
        // rest of this page optional, so it opens the help rather than sitting
        // under "Other" two screens down.
        section("Start here"),
        item("Enter", "Everything you can do to this row, in one menu"),
        item("?  F1", "This page"),
        item("q  F10", "Quit"),
        Line::default(),
        section("Navigation"),
        item("↑/k  ↓/j", "Move between sessions"),
        item("PgUp / PgDn", "Page through the list"),
        item("Ctrl+U / Ctrl+D", "Half a page up / down"),
        item("g / G", "Jump to first / last"),
        item("n / N", "Next / previous search match (wraps)"),
        item("b", "Jump to the session that rang last"),
        item("f", "Follow mode: keep the selection centered"),
        Line::default(),
        section("Acting on the selected session"),
        item("a", "Open its terminal in a tab"),
        item("R", "Resume it in a tab of its own"),
        item("s", "Type a line into its terminal"),
        item("O", "Hand its context off to a different agent"),
        item("y", "Copy resume command or transcript path"),
        item("e / E", "Show its subagents / all subagents"),
        item("d", "Delete it (only when it is not running)"),
        item("Ctrl+K", "Terminate it"),
        Line::default(),
        section("Several at once"),
        item("Space", "Mark / unmark the selected session"),
        item("U", "Clear all marks"),
        item("D", "Delete all marked sessions"),
        item("K", "Terminate all marked live sessions"),
        Line::default(),
        section("Filter and sort"),
        item("/  F3", "Filter by label, project, branch, model, id"),
        item("  Tab", "Search inside the transcripts as well"),
        item("  ↑ / ↓", "Bring back an earlier search"),
        item("S  F6  >  <", "Sort by any column"),
        item("F7", "Filter by age (1d / 1w / 1mo)"),
        item("#", "Cost floor: only sessions costing ≥ $X"),
        item("`", "Show only running sessions"),
        item("Esc", "Clear one filter layer per press"),
        Line::from(Span::styled(
            "  Clicking a column header sorts by it too.",
            theme::dim(),
        )),
        Line::default(),
        section("Bottom panels"),
        item("←  →", "Move between bottom panels"),
        item("Tab / Shift+Tab", "Same, either direction"),
        item("1 – 9", "Jump to a panel directly"),
        item("Shift+↑ / ↓", "Scroll inside the active panel"),
        item("Shift+Home / End", "Jump to the top / bottom of it"),
        item("[ / ]", "Move through the Tool Activity filter"),
        item("L", "Toggle the Tool Activity live filter"),
        item("v", "Toggle inline diffs for edits"),
        Line::default(),
        section("Tabs and splits"),
        item(
            "t or Alt+n",
            "New tab: an agent, a shell, or one still running",
        ),
        item("Alt+v / Alt+s", "Split the tab right / down"),
        item("Alt+← / →", "Previous / next tab"),
        item("Alt+1 – 9", "Jump to a tab (1 is the dashboard)"),
        item(
            "Alt+Shift+← / →",
            "Move this tab along the bar (or drag it with the mouse)",
        ),
        item("Right-click", "Rename the tab under the pointer"),
        item("Alt+o", "Move focus to the next pane"),
        item("Alt+w", "Close the pane and stop its agent"),
        item("Alt+Shift+W", "The same, by a name that says so"),
        item("F9", "Paste the clipboard's image as a file path"),
        item("F12", "Back to the dashboard, leaving it running"),
        Line::default(),
        section("Mouse"),
        item("Right-click a row", "Its menu, as Enter opens it"),
        item("Click a footer hint", "Presses that key (q asks twice)"),
        item("Click [y] / [n]", "Answers a confirmation"),
        Line::default(),
        section("Elsewhere"),
        item("A", "Open the agent this cctop launched"),
        item("w", "Bell + desktop alert when a session needs you"),
        item("W", "Share the agent's terminal to a browser"),
        item(
            "B",
            "Serve this table to a browser, with or without a tunnel",
        ),
        item("h  F8", "Agent integration: what reports to cctop"),
        item("r  F5", "Refresh now"),
        Line::default(),
        section("Environment"),
        item("CCTOP_THEME", "light / dark / auto (default: auto)"),
        item("NO_COLOR", "Drop colour; shape and weight carry the state"),
        item(
            "CCTOP_COLUMNS_HIDE",
            "Column keys to hide, e.g. tok_rate,mem",
        ),
        Line::from(Span::styled(
            "  Columns also drop by priority on their own as the window narrows.",
            theme::dim(),
        )),
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
        Line::from(Span::styled(
            "  ↑ / ↓ scroll   any other key returns",
            theme::dim(),
        )),
    ];
    app.help_max_scroll = scrollable_modal(frame, area, "Help", lines, 76, app.help_scroll);
}

pub(super) fn draw_search(frame: &mut Frame, area: Rect, app: &App) {
    const WIDTH: u16 = 62;
    let text_w = WIDTH as usize - 4;

    let mut lines = vec![
        Line::from(vec![
            Span::raw(" > "),
            Span::styled(
                app.search.clone(),
                Style::default()
                    .fg(theme::colors().value)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(theme::colors().accent)),
        ]),
        Line::from(Span::styled(
            format!(
                " {} of {} session{}",
                app.visible.len(),
                app.sessions.len(),
                if app.sessions.len() == 1 { "" } else { "s" }
            ),
            theme::dim(),
        )),
    ];

    // What is actually being searched, and whether the answer on screen is
    // final. A scan is the one filter here that takes long enough to see, so
    // saying nothing would read as "no transcript matches" while it runs.
    let (marker, style, text) = match (app.search_content, app.scanning) {
        (false, _) => (
            "○",
            theme::dim(),
            "Columns only — Tab also searches transcripts".to_string(),
        ),
        (true, true) => (
            "◌",
            Style::default().fg(theme::colors().accent),
            "Searching transcripts…".to_string(),
        ),
        (true, false) => (
            "●",
            Style::default().fg(theme::colors().cost_low),
            match app.scan_hits.len() {
                0 if app.search.chars().count() < 3 => {
                    "Transcripts: type three characters".to_string()
                }
                0 => "Transcripts: no matches".to_string(),
                n => format!("Transcripts: {n} matched"),
            },
        ),
    };
    lines.push(Line::from(vec![
        Span::styled(format!(" {marker} "), style),
        Span::styled(crate::util::truncate(&text, text_w), style),
    ]));

    // The matching line from the selected session's transcript. Without it a
    // content hit is a row that matches for reasons nothing on screen explains.
    if let Some(snippet) = app.selected_snippet() {
        lines.push(Line::from(Span::styled(
            // Less the three-space indent: the paragraph wraps, and a snippet
            // spilling onto a second line resizes the modal as you type.
            format!("   {}", crate::util::truncate(snippet, text_w - 3)),
            theme::value(),
        )));
    }

    lines.push(Line::from(Span::styled(
        if app.search_history.is_empty() {
            " Enter apply   Esc cancel"
        } else {
            " Enter apply   ↑/↓ past searches   Esc cancel"
        },
        theme::dim(),
    )));
    modal(frame, area, "Filter sessions", lines, WIDTH);
}

/// Resuming a session that is already running somewhere else.
pub(super) fn draw_resume_confirm(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let Some(session) = app.selected_session() else {
        return;
    };
    let command = session
        .resume_argv()
        .map(|argv| argv.join(" "))
        .unwrap_or_default();
    // `modal` sizes the box by line count and the paragraph wraps, so a line
    // long enough to wrap pushes the last one out of the border. Every line
    // here is kept inside the 60 columns the box has room for.
    let lines = vec![
        Line::from(Span::styled(
            format!(
                " {} is still running.",
                crate::util::truncate(session.display_label(), 40)
            ),
            Style::default().fg(theme::colors().cost_mid),
        )),
        Line::default(),
        Line::from(Span::raw(
            " Resuming starts a second agent on the same transcript,",
        )),
        Line::from(Span::raw(" which neither of them will know about.")),
        Line::default(),
        Line::from(Span::styled(
            format!("   {}", crate::util::truncate(&command, 56)),
            theme::value(),
        )),
        Line::default(),
        // The same two chips its siblings use, rather than the sentence this
        // line used to be: they are what a pointer can be aimed at, and one
        // dialog in the family phrasing it differently taught nobody anything.
        Line::from(Span::styled(RESUME_KEYS, theme::dim())),
    ];
    let last = lines.len() as u16 - 1;
    let (outer, inner) = modal(frame, area, "Resume a running session?", lines, 62);
    confirm_chips(
        layout,
        outer,
        inner,
        last,
        RESUME_KEYS,
        &[("[y]", ch('y')), ("[n / Esc]", dismiss())],
    );
}

const RESUME_KEYS: &str = " [y] resume anyway    [n / Esc] cancel";

/// Offering to install rmux, which is what would have made the agent about to
/// start outlive cctop.
pub(super) fn draw_rmux_install(frame: &mut Frame, area: Rect, app: &App) {
    let Some(install) = app.rmux_install.as_ref() else {
        return;
    };
    let command = install.shown();
    let mut lines = vec![
        Line::from(Span::styled(
            " rmux is not installed.",
            Style::default().fg(theme::colors().cost_mid),
        )),
        Line::default(),
        Line::from(Span::raw(
            " With it, agents run inside rmux and survive cctop",
        )),
        Line::from(Span::raw(
            " closing. Without it, quitting takes them with it.",
        )),
        Line::default(),
        Line::from(Span::styled(
            format!("   {}", crate::util::truncate(command, 56)),
            theme::value(),
        )),
    ];
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " y to install · any other key to start without it",
        theme::dim(),
    )));
    // The manager goes in the title rather than the footer: naming it inline
    // takes that line past the 60 columns the box has, and a footer that wraps
    // pushes itself out through the bottom border.
    modal(
        frame,
        area,
        &format!("Install rmux with {}?", install.manager),
        lines,
        62,
    );
}

/// Whether this cctop is serving its table to a browser, and on what.
///
/// The links are drawn as their origin and made clickable, rather than printed
/// in full. A served link carries the token that opens it, so the full text is
/// something to hand over deliberately — `y` puts it on the clipboard — and not
/// something to leave on screen. It also does not fit: a tunnel hostname plus a
/// token is most of a hundred columns.
pub(super) fn draw_serve(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();

    match &app.serving {
        None => {
            lines.push(Line::from(Span::raw(
                " Nothing is being served. The table can be a page,",
            )));
            lines.push(Line::from(Span::raw(" with cctop still running here.")));
        }
        Some(serving) => {
            // A free function rather than a closure: it appends to `lines`,
            // and a closure that captures it mutably shuts out every other push
            // in this arm.
            fn show(lines: &mut Vec<Line<'static>>, what: &str, url: &str) {
                lines.push(Line::from(Span::styled(format!(" {what}"), theme::dim())));
                lines.push(Line::from(Span::styled(
                    format!("  {}", origin_of(url)),
                    Style::default().fg(theme::colors().accent),
                )));
            }
            show(&mut lines, "This machine", &serving.local);
            match &serving.public {
                None => {
                    lines.push(Line::default());
                    lines.push(Line::from(Span::styled(
                        " No tunnel — nothing off this machine can reach it.",
                        theme::dim(),
                    )));
                }
                Some(public) => {
                    lines.push(Line::default());
                    show(&mut lines, "The internet", public);
                    lines.push(Line::default());
                    // The one thing to understand before sending this to
                    // anybody, said where the link is being looked at.
                    lines.push(Line::from(Span::styled(
                        " Anyone holding it reads every session here,",
                        Style::default().fg(theme::colors().cost_mid),
                    )));
                    lines.push(Line::from(Span::styled(
                        match serving.actions {
                            true => " and can type at your agents. Cloudflare carries it.",
                            false => " but cannot act. Cloudflare carries it.",
                        },
                        Style::default().fg(theme::colors().cost_mid),
                    )));
                }
            }
        }
    }
    // Said in the panel as well as in the corner: `t` is pressed here, and a
    // panel that answered a keypress with nothing would read as a dead key.
    if let Some(opening) = &app.share_opening {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!(" {} Opening a tunnel to trycloudflare…", opening.frame()),
            Style::default().fg(theme::colors().cost_mid),
        )));
    }
    if let Some(error) = &app.serve_error {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!(" {}", crate::util::truncate(error, 58)),
            Style::default().fg(theme::colors().cost_high),
        )));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        match app.serving.is_some() {
            true => " o open · y copy · l local · t + tunnel · x stop",
            false => " l this machine only · t also a public tunnel",
        },
        theme::dim(),
    )));

    // Sized to the longest line, so a tunnel hostname is one line and not two,
    // and capped to the screen, where it wraps instead — into a box now tall
    // enough for it.
    let widest = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = widest
        .saturating_add(3)
        .clamp(56, area.width.saturating_sub(4).max(24));
    modal(frame, area, "Serve this table to a browser", lines, width);
}

/// `http://127.0.0.1:7778/?t=abc` → `http://127.0.0.1:7778`.
///
/// What is worth reading on screen: where the link goes, without the token that
/// opens it.
fn origin_of(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let host = rest.split(['/', '?']).next().unwrap_or(rest);
    match scheme.is_empty() {
        true => host.to_string(),
        false => format!("{scheme}://{host}"),
    }
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
                theme::selected()
            } else if active {
                Style::default().fg(theme::colors().accent)
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
                theme::selected()
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(
                    format!(" {marker} "),
                    Style::default().fg(if active {
                        theme::colors().cost_low
                    } else {
                        theme::colors().dimmer
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

/// The last `width` characters of `s`, elided at the front.
///
/// For a field being typed into, where the end is the part that is moving.
/// [`truncate`](crate::util::truncate) keeps the head, which is the right
/// answer for a label and the wrong one for a cursor.
fn tail(s: &str, width: usize) -> String {
    let count = s.chars().count();
    if count <= width {
        return s.to_string();
    }
    let skip = count - width.saturating_sub(1);
    format!("…{}", s.chars().skip(skip).collect::<String>())
}

/// The most of a refusal the menu will show before eliding it.
///
/// Long enough for every reason in [`menu::items`](super::menu::items) and for
/// the useful half of a remote row's, which names the host first.
const MAX_REASON: usize = 34;

/// What sits at the right of an entry: the key, or why there is no point
/// pressing it.
fn trailing(item: &super::menu::Item) -> String {
    match &item.blocked {
        Some(why) => why.clone(),
        None => item.key.to_string(),
    }
}

/// The per-row action menu.
///
/// Sized to its widest entry rather than to a constant: the labels are fixed
/// strings, so the one width that always fits is the one measured from them.
pub(super) fn draw_row_menu(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    layout: &mut super::render::Layout,
) {
    let items = super::menu::items(app);
    if items.is_empty() {
        return;
    }

    // Two columns: what it does, and — flush right — either the key that does
    // it or the reason it cannot be done. One or the other, never both, because
    // the key of an entry that is refused is not information: pressing it only
    // repeats the refusal.
    //
    // On one line each. The reason used to sit on a line of its own underneath,
    // which doubled the height of exactly the entries the reader has already
    // decided not to use.
    let label_w = items.iter().map(|i| i.label.len()).max().unwrap_or(0);
    // Bounded so one long refusal cannot stretch the menu across the terminal;
    // the tail of `remote_refusal` is the part that repeats.
    let right_w = items
        .iter()
        .map(|i| trailing(i).chars().count().min(MAX_REASON))
        .max()
        .unwrap_or(0);
    let width = (label_w + right_w + 6).clamp(30, 76) as u16;

    let mut lines: Vec<Line<'static>> = Vec::new();
    // Which line each entry landed on, so a click lands on the entry rather
    // than on whatever the menu is covering.
    let mut rows: Vec<(usize, usize)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if item.rule {
            // Inset a cell each side, so the rule reads as a divider between
            // entries rather than as another border.
            lines.push(Line::from(Span::styled(
                format!(" {} ", "─".repeat(width.saturating_sub(4) as usize)),
                theme::dim(),
            )));
        }
        let selected = i == app.menu_cursor && item.enabled();
        let style = match (item.enabled(), selected) {
            (false, _) => theme::dim(),
            (true, true) => Style::default()
                .bg(theme::colors().selected_bg)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            (true, false) => Style::default(),
        };
        rows.push((lines.len(), i));
        lines.push(Line::from(vec![
            Span::styled(format!(" {:<label_w$}  ", item.label), style),
            Span::styled(
                format!(
                    "{:>right_w$} ",
                    crate::util::truncate(&trailing(item), MAX_REASON)
                ),
                theme::dim(),
            ),
        ]));
    }

    // The row this is about, on the border: it costs no content line, and a
    // menu floating over a table of seventy rows has to say which one it means.
    let subject = app
        .selected_session()
        .map(|s| crate::util::truncate(s.display_label(), width.saturating_sub(4) as usize))
        .unwrap_or_default();

    // Wrapped rows, not lines. The paragraph below wraps, so a box sized to the
    // line count is one row short for every line that wrapped — and the content
    // that does not fit is drawn through the bottom border rather than clipped.
    let inner_width = width.saturating_sub(2).max(1) as usize;
    let height = lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(inner_width) as u16)
        .sum::<u16>()
        + 2;
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border_hi))
        .title(Span::styled(format!(" {subject} "), theme::title()))
        .title_bottom(Span::styled(" ↑↓ Enter · Esc ", theme::dim()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(lines), inner);

    layout.modal_rect = Some(rect);
    layout.menu_rows = rows
        .into_iter()
        .map(|(line, i)| (inner.y + line as u16, i))
        .collect();
}

/// The launcher: what to put in the tab, and where it will run.
pub(super) fn draw_launch(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    layout: &mut super::render::Layout,
) {
    const WIDTH: u16 = 62;
    /// Room for the name, then where it is working, then what it is doing. The
    /// three together are what tells one `claude` from another `claude`.
    const NAME: usize = 24;
    const WHERE: usize = 20;
    const STATE: usize = 12;
    /// The lines held below the list: where it will run, and the keys.
    const FOOTER: usize = 2;

    let choices = app.launch_choices();
    let waiting = choices
        .iter()
        .filter(|c| matches!(c, tabs::Choice::Waiting(_)))
        .count();

    // Abbreviated together rather than each truncated, because the column exists
    // to tell two agents apart and the identifying part of a path is its tail.
    // This keeps whatever is needed to make each one unique and drops the rest:
    // one agent in ~/cctop reads `cctop`, and two in ~/a/api and ~/b/api read
    // `a/api` and `b/api` instead of the same elided prefix twice.
    let dirs: Vec<String> = choices
        .iter()
        .filter_map(|c| c.cwd())
        .map(|d| d.to_string_lossy().into_owned())
        .collect();
    let mut short = crate::util::abbreviate_paths(&dirs).into_iter();

    // The list on its own, kept apart from the two lines under it: it is the
    // part that scrolls, and they are the part that must stay on screen.
    let mut rows: Vec<Line> = Vec::new();
    // Which line each choice ends up on, so a click can be turned back into the
    // choice it landed on rather than into whatever the modal is covering.
    let mut choice_lines: Vec<usize> = Vec::new();
    for (i, choice) in choices.iter().enumerate() {
        // A heading above each group, so a still-running agent is never picked
        // in the belief that it starts a fresh one.
        if i == 0 && waiting > 0 {
            rows.push(Line::from(Span::styled(" Still running", theme::label())));
        }
        if i == waiting && waiting > 0 {
            rows.push(Line::from(Span::styled(" Start new", theme::label())));
        }
        let selected = i == app.launch_cursor;
        let style = if selected {
            Style::default()
                .bg(theme::colors().selected_bg)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        // What the agent last said about itself, which is the whole reason to
        // list it: an agent stuck on a question is the one to go back to, and it
        // looks exactly like an idle one from a list of names.
        let reported = match choice {
            tabs::Choice::Waiting(agent) => app.waiting_state(agent),
            tabs::Choice::Start(_) => None,
        };
        // The dot carries whether anyone is already looking, the word carries
        // what the agent said about itself. Two facts that can both be true at
        // once, so neither is made to wait for the other's column.
        let dot = match choice {
            // Ringed: a client is on this session already. Attaching a second
            // works, but the two then fight over one window's size.
            tabs::Choice::Waiting(agent) if agent.attached => "◉",
            tabs::Choice::Waiting(_) => "●",
            tabs::Choice::Start(_) => " ",
        };
        let dot_color = match (choice, reported) {
            (tabs::Choice::Waiting(_), Some(signal)) => theme::signal_color(signal),
            // Running, but it has never reported: no hooks installed, or nothing
            // has happened since cctop started listening. Still a live agent, so
            // it keeps a dot — just one that claims nothing.
            (tabs::Choice::Waiting(_), None) => theme::colors().dim,
            (tabs::Choice::Start(_), _) => theme::colors().dimmer,
        };

        let state = match reported {
            Some(signal) => signal.label().to_string(),
            None => String::new(),
        };
        let state_color = match reported {
            Some(signal) => theme::signal_color(signal),
            None => theme::colors().dim,
        };

        // Drawn from the same iterator the abbreviation was built from, so the
        // paths stay lined up with the choices that have one.
        let at = match choice.cwd().is_some() {
            true => short.next().unwrap_or_default(),
            false => String::new(),
        };

        // The session's own title where cctop knows it. A resumed session's rmux
        // name carries the whole session id, which for Codex is a timestamp and a
        // uuid — two of those are identical for far more characters than this
        // column is wide, so the names alone would draw two rows that read the
        // same and do different things.
        let name = match choice {
            tabs::Choice::Waiting(agent) => app.waiting_label(agent),
            tabs::Choice::Start(_) => None,
        }
        .unwrap_or_else(|| choice.label());

        choice_lines.push(rows.len());
        rows.push(Line::from(vec![
            Span::styled(format!(" {dot} "), Style::default().fg(dot_color)),
            Span::styled(
                format!("{:<NAME$}", crate::util::truncate(&name, NAME)),
                style,
            ),
            Span::styled(
                format!("{:<WHERE$}", crate::util::truncate(&at, WHERE - 1)),
                theme::dim(),
            ),
            Span::styled(
                format!("{:<STATE$}", crate::util::truncate(&state, STATE)),
                Style::default().fg(state_color),
            ),
        ]));
    }

    // The list scrolls rather than being cut off. `modal` sizes itself to its
    // lines and `centered` then clamps that to the screen, so on a short
    // terminal a long list loses its last rows silently — including, once the
    // cursor walks into them, the highlight itself: nothing on screen would say
    // what Enter is about to do.
    //
    // Two lines are held back for the footer below, which is where the working
    // directory is stated; scrolling that off would answer "in which project?"
    // with silence for exactly the launches that need asking about.
    // Two for the borders, two more for what `centered` will not give.
    // The suggestions live in the footer while the field is open: they are what
    // the field is being typed against, so scrolling them off would leave the
    // key that fills them in with nothing to explain it.
    let editing = app.mode == super::Mode::LaunchCwd;
    let hits: &[std::path::PathBuf] = match editing {
        true => &app.launch_cwd_hits,
        false => &[],
    };
    let room = (area.height as usize)
        .saturating_sub(4 + FOOTER + hits.len())
        .max(1);
    let total = rows.len();
    let scrolled = total > room;
    let cursor_line = choice_lines.get(app.launch_cursor).copied().unwrap_or(0);
    let offset = match scrolled {
        // Keep the cursor in view, and never scroll past the final row.
        true => cursor_line.saturating_sub(room - 1).min(total - room),
        false => 0,
    };
    let shown = room.min(total - offset);
    let mut lines: Vec<Line> = rows.into_iter().skip(offset).take(shown).collect();

    // Where it starts is not a detail: a claude opened on the wrong project
    // reads its way into the wrong repository before you notice. It says
    // nothing about reattaching, which lands wherever the agent already is.
    let picked = choices.get(app.launch_cursor);
    let picked_waiting = matches!(picked, Some(tabs::Choice::Waiting(_)));
    lines.push(if editing {
        // The field, in place of the line it replaces. Editing here rather than
        // in a modal of its own keeps the list of agents on screen: which agent
        // is picked is half of what the directory is being chosen for.
        Line::from(vec![
            Span::styled(" in ", Style::default().fg(theme::colors().label)),
            Span::styled(
                // The tail, when a long path outgrows the box. The end is the
                // part being typed, and a field that showed the start would
                // hide the cursor as soon as it mattered.
                tail(&app.launch_cwd_input, WIDTH as usize - 8),
                match app.launch_cwd_bad {
                    true => Style::default().fg(theme::colors().cost_high),
                    false => theme::value(),
                },
            ),
            Span::styled("▏", theme::value()),
            Span::styled(
                match app.launch_cwd_bad {
                    true => "  no such directory",
                    false => "",
                },
                Style::default().fg(theme::colors().cost_high),
            ),
        ])
    } else {
        Line::from(Span::styled(
            match (picked, &app.launch_cwd) {
                // The ring is the only unexplained mark on the row, and it is
                // the one worth explaining: it is the difference between coming
                // back to an agent and joining someone else on it.
                (Some(tabs::Choice::Waiting(agent)), _) if agent.attached => {
                    " ◉ already open elsewhere — both windows share one size".to_string()
                }
                (Some(tabs::Choice::Waiting(_)), _) => " where it already is".to_string(),
                (_, Some(dir)) => format!(
                    " in {}  (c to change)",
                    crate::util::truncate(
                        &crate::util::tildify(&dir.to_string_lossy()),
                        WIDTH as usize - 22
                    )
                ),
                (_, None) => " in this directory  (c to change)".to_string(),
            },
            Style::default().fg(theme::colors().label),
        ))
    });
    // What the field can see, under the field. Half a path plus this list is
    // how a directory gets reached without being recalled: a name matches the
    // projects agents have run in, and anything with a separator in it is read
    // off the disk.
    let mut hit_lines: Vec<usize> = Vec::new();
    for (i, dir) in hits.iter().enumerate() {
        let selected = app.launch_cwd_pick == Some(i);
        // The tail, like the field itself: the part that distinguishes two
        // long paths is their end, and the marker keeps the column of names
        // clear of the `in` line above it.
        let shown = tail(
            &crate::util::tildify(&dir.to_string_lossy()),
            WIDTH as usize - 8,
        );
        hit_lines.push(lines.len());
        lines.push(Line::from(vec![
            Span::styled(
                match selected {
                    true => " › ",
                    false => "   ",
                },
                theme::dim(),
            ),
            Span::styled(
                shown,
                match selected {
                    true => Style::default()
                        .bg(theme::colors().selected_bg)
                        .fg(Color::White),
                    false => theme::dim(),
                },
            ),
        ]));
    }

    // Only for an agent the profile reaches, and only where there is more than
    // one to be in: on a machine with a single account this says nothing, and a
    // line offering a key that changes nothing is worse than no line.
    let profile = match picked {
        // `launch_profile` already answers for the highlighted choice, so the
        // line appears exactly when the harness has an account to pick.
        Some(tabs::Choice::Start(_)) => app.launch_profile(),
        _ => None,
    };
    if let Some(profile) = profile {
        lines.push(Line::from(vec![
            Span::styled(" as ", Style::default().fg(theme::colors().label)),
            Span::styled(profile.name.clone(), theme::value()),
            Span::styled("  (p to change)", theme::dim()),
        ]));
    }
    let keys = match (editing, picked_waiting) {
        (true, _) if !hits.is_empty() => " Enter accept  Tab fill in  ↑/↓ pick  Esc cancel",
        (true, _) => " Enter accept  Esc keep the old one",
        (false, true) => " ↑/↓  Enter reattach  Esc cancel",
        (false, false) => " ↑/↓  Enter start  Esc cancel",
    };
    lines.push(Line::from(Span::styled(
        // A scrolled list has to say so, or the choices above and below the
        // window are simply missing as far as anyone can tell.
        match scrolled {
            true => format!("{keys}   {} of {}", app.launch_cursor + 1, choices.len()),
            false => keys.to_string(),
        },
        theme::dim(),
    )));
    // A handoff opens the same launcher for a different reason, and the picked
    // agent is about to be typed at rather than just started — which is worth
    // saying before Enter, not after.
    let title = match (app.pending_brief.is_some(), app.launch_into) {
        (true, _) => "Hand the context to",
        (false, LaunchInto::Tab) => "New tab",
        (false, LaunchInto::Split { stacked: false }) => "Split right",
        (false, LaunchInto::Split { stacked: true }) => "Split down",
    };
    let (outer, inner) = modal(frame, area, title, lines, WIDTH);
    layout.modal_rect = Some(outer);
    // Clickable for the same reason the choices are: the list is there to be
    // read, and a path you can see but not click reads as decoration.
    layout.launch_cwd_rows = hit_lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| (inner.y + line as u16, i))
        .collect();
    layout.launch_rows = choice_lines
        .into_iter()
        .enumerate()
        .filter_map(|(i, line)| {
            // Scrolled out above, or below the window: no row to click. The
            // indices stay attached to their choices either way, so a click
            // still resolves to what is drawn on that row and not to whatever
            // choice happens to sit that far down the list.
            let drawn = line.checked_sub(offset).filter(|d| *d < shown)?;
            let row = inner.y + drawn as u16;
            (row < inner.y + inner.height).then_some((row, i))
        })
        .collect();
}

/// What the agents have been asked to report, and whether they are doing it.
///
/// The panel exists because every part of this is invisible otherwise: a hook
/// that is not installed, or is installed at a path that has moved, looks
/// exactly like an agent that has nothing to say.
pub(super) fn draw_hooks(frame: &mut Frame, area: Rect, app: &App) {
    let Some(report) = &app.hooks else { return };
    const WIDTH: u16 = 78;
    // Two for the border, one for the marker and the space after it.
    let text_w = WIDTH as usize - 5;

    let mut lines: Vec<Line> = Vec::new();
    for (text, problem) in report.lines() {
        let (marker, style) = match problem {
            true => ("! ", Style::default().fg(theme::colors().cost_mid)),
            false => ("· ", theme::value()),
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker}"), style),
            Span::styled(crate::util::truncate(&text, text_w), style),
        ]));
    }

    // What has actually arrived, which is the only proof any of the above is
    // working. An install can be perfect and still deliver nothing, because
    // sessions started before it keep the hooks they were started with.
    lines.push(Line::default());
    let reporting = app.reporting();
    if reporting.is_empty() {
        lines.push(Line::from(Span::styled(
            " Nothing has reported in yet",
            theme::dim(),
        )));
    } else {
        lines.push(Line::from(Span::styled(" Reporting", theme::label())));
        for (project, state) in reporting.iter().take(6) {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("   {:<24}", crate::util::truncate(project, 24)),
                    theme::value(),
                ),
                Span::styled(*state, theme::dim()),
            ]));
        }
        if reporting.len() > 6 {
            lines.push(Line::from(Span::styled(
                format!("   … and {} more", reporting.len() - 6),
                theme::dim(),
            )));
        }
    }

    lines.push(Line::default());
    // The project keys name the directory they would write into: a settings
    // file committed to somebody's repository is not a thing to install by
    // accident.
    lines.push(Line::from(Span::styled(
        match app.hook_project() {
            Some(dir) => format!(
                " p / P  install / remove in {}",
                crate::util::truncate(&dir.display().to_string(), text_w.saturating_sub(30))
            ),
            None => " p / P  install / remove for the selected project".into(),
        },
        theme::dim(),
    )));
    lines.push(Line::from(Span::styled(
        " i / x  install / remove for this user (every agent above)",
        theme::dim(),
    )));
    lines.push(Line::from(Span::styled(
        " Esc    close   ·   sessions already running keep their old hooks",
        theme::dim(),
    )));

    modal(frame, area, "Agent integration", lines, WIDTH);
}

// ---------------------------------------------------------------------------
// Confirmations
// ---------------------------------------------------------------------------

/// The key a chip stands for: `[y]` is `y`.
fn ch(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

/// `[n / Esc]`, and `[any key]` on the dialogs that only have something to say.
fn dismiss() -> KeyEvent {
    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
}

/// Claim a confirmation's rectangle and make the `[k]` chips on its last line
/// clickable.
///
/// Both halves matter, and both were missing. Without the rectangle a click
/// anywhere on a dialog fell straight through to the table underneath, moving
/// the selection beneath a question about the row that was selected when it was
/// asked. And the chips are written to be read as buttons — `[y] delete` is not
/// prose — so a pointer aimed at one has to land somewhere, cctop holding the
/// terminal's mouse capture.
///
/// `hint` is the text of that line and `row` its index among the lines, which
/// is how the chips are located: they are found in the string that was drawn,
/// rather than by counting columns a caller would have to keep in step with the
/// wording.
fn confirm_chips(
    layout: &mut Layout,
    outer: Rect,
    inner: Rect,
    row: u16,
    hint: &str,
    keys: &[(&str, KeyEvent)],
) {
    layout.modal_rect = Some(outer);
    for (chip, key) in keys {
        let Some(at) = hint.find(chip) else {
            continue;
        };
        let x = inner.x + hint[..at].chars().count() as u16;
        let width = chip.chars().count() as u16;
        layout.key_hits.push((inner.y + row, x, x + width, *key));
    }
}

pub(super) fn draw_delete_confirm(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
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
            Style::default().fg(theme::colors().cost_mid),
        )),
        Line::default(),
        Line::from(Span::styled(DELETE_KEYS, theme::dim())),
    ];
    let last = lines.len() as u16 - 1;
    let (outer, inner) = modal(frame, area, "Delete session?", lines, 62);
    confirm_chips(
        layout,
        outer,
        inner,
        last,
        DELETE_KEYS,
        &[("[y]", ch('y')), ("[n / Esc]", dismiss())],
    );
}

const DELETE_KEYS: &str = "  [y] delete    [n / Esc] cancel";

pub(super) fn draw_delete_blocked(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
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
        Line::from(Span::styled(DISMISS_KEYS, theme::dim())),
    ];
    let last = lines.len() as u16 - 1;
    let (outer, inner) = modal(frame, area, "Cannot delete", lines, 62);
    confirm_chips(
        layout,
        outer,
        inner,
        last,
        DISMISS_KEYS,
        &[("[any key]", dismiss())],
    );
}

/// The one chip on a dialog that has nothing to decide, only something to say.
const DISMISS_KEYS: &str = "  [any key] dismiss";

pub(super) fn draw_kill_confirm(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
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
            Style::default().fg(theme::colors().cost_mid),
        )),
        Line::from(Span::raw("  Unsaved work in the agent may be interrupted.")),
        Line::default(),
        Line::from(Span::styled(KILL_KEYS, theme::dim())),
    ];
    let last = lines.len() as u16 - 1;
    let (outer, inner) = modal(frame, area, "Terminate session?", lines, 62);
    confirm_chips(
        layout,
        outer,
        inner,
        last,
        KILL_KEYS,
        &[("[y]", ch('y')), ("[n / Esc]", dismiss())],
    );
}

const KILL_KEYS: &str = "  [y] terminate    [n / Esc] cancel";

pub(super) fn draw_quit_confirm(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let label = match &app.hosted {
        Some((_, label)) => label.clone(),
        None => return,
    };
    let lines = vec![
        Line::from(Span::styled(format!("  {label}"), theme::value())),
        Line::default(),
        Line::from(Span::styled(
            "  This agent is running on a terminal cctop owns,",
            Style::default().fg(theme::colors().cost_mid),
        )),
        Line::from(Span::styled(
            "  and quitting ends it.",
            Style::default().fg(theme::colors().cost_mid),
        )),
        Line::from(Span::raw("  Exit the agent itself to leave it cleanly.")),
        Line::default(),
        Line::from(Span::styled(QUIT_KEYS, theme::dim())),
    ];
    let last = lines.len() as u16 - 1;
    let (outer, inner) = modal(frame, area, "Quit and stop the agent?", lines, 62);
    confirm_chips(
        layout,
        outer,
        inner,
        last,
        QUIT_KEYS,
        &[("[y]", ch('y')), ("[n / Esc]", dismiss()), ("[A]", ch('A'))],
    );
}

const QUIT_KEYS: &str = "  [y] quit anyway    [n / Esc] stay    [A] back to the agent";

pub(super) fn draw_kill_blocked(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
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
        Line::from(Span::styled(DISMISS_KEYS, theme::dim())),
    ];
    let last = lines.len() as u16 - 1;
    let (outer, inner) = modal(frame, area, "Cannot terminate", lines, 62);
    confirm_chips(
        layout,
        outer,
        inner,
        last,
        DISMISS_KEYS,
        &[("[any key]", dismiss())],
    );
}

pub(super) fn draw_batch_confirm(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
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
        Style::default().fg(theme::colors().cost_mid),
    )));
    lines.push(Line::default());
    let hint = format!("  [y] {verb} all    [n / Esc] cancel");
    lines.push(Line::from(Span::styled(hint.clone(), theme::dim())));
    let last = lines.len() as u16 - 1;
    let (outer, inner) = modal(frame, area, &format!("{verb} all?"), lines, 62);
    confirm_chips(
        layout,
        outer,
        inner,
        last,
        &hint,
        &[("[y]", ch('y')), ("[n / Esc]", dismiss())],
    );
}

pub(super) fn draw_batch_blocked(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    deleting: bool,
    layout: &mut Layout,
) {
    let ms = app.marked_sessions();
    // First one that can't be processed, for the explanation.
    let (explain, name) = match app.batch {
        BatchKind::Delete => (
            "running — stop the agent first",
            ms.iter().find(|s| s.is_running()),
        ),
        BatchKind::Kill => (
            "has no locally controllable process",
            ms.iter().find(|s| s.root_pid().is_none()),
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
        Line::from(Span::styled(DISMISS_KEYS, theme::dim())),
    ];
    let last = lines.len() as u16 - 1;
    let title = if deleting {
        "Cannot delete all"
    } else {
        "Cannot terminate all"
    };
    let (outer, inner) = modal(frame, area, title, lines, 62);
    confirm_chips(
        layout,
        outer,
        inner,
        last,
        DISMISS_KEYS,
        &[("[any key]", dismiss())],
    );
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
                    .fg(theme::colors().value)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(theme::colors().accent)),
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
            " Needs the agent under `cctop run`, rmux, or cctop as root.",
            theme::dim(),
        )),
        Line::default(),
        Line::from(vec![
            Span::raw(" > "),
            Span::styled(
                app.send_input.clone(),
                Style::default()
                    .fg(theme::colors().value)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(theme::colors().accent)),
        ]),
        Line::default(),
        Line::from(Span::styled(
            " Enter send   F9 paste an image   Esc cancel",
            theme::dim(),
        )),
    ];
    modal(frame, area, "Send to session", lines, 64);
}

/// Renaming a workspace tab, opened by right-clicking it in the bar.
///
/// The old name is shown rather than pre-filled into the field: the reason to
/// rename `3:claude-4` is that it says nothing, so starting from it would only
/// have to be deleted first. It is still on screen because the bar behind the
/// modal may have scrolled the tab out of view.
///
/// The rectangle is recorded, unlike the other one-line fields, because this
/// modal is reached by mouse: a click off it dismisses it, which is what
/// anything opened by a click should answer to.
pub(super) fn draw_rename_tab(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    layout: &mut super::render::Layout,
) {
    let lines = vec![
        Line::from(vec![
            Span::styled(" Now called ", theme::dim()),
            Span::styled(app.rename_was.clone(), theme::value()),
        ]),
        Line::from(Span::styled(
            " The name follows the tab into every cctop on this machine.",
            theme::dim(),
        )),
        Line::default(),
        Line::from(vec![
            Span::raw(" > "),
            Span::styled(
                app.rename_input.clone(),
                Style::default()
                    .fg(theme::colors().value)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(theme::colors().accent)),
        ]),
        Line::default(),
        Line::from(Span::styled(" Enter rename   Esc cancel", theme::dim())),
    ];
    let (outer, _) = modal(frame, area, "Rename tab", lines, 64);
    layout.modal_rect = Some(outer);
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
