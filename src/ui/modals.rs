//! Modal overlays: help, filters, and the confirmation dialogs.

use super::columns::COLUMNS;
use super::hyperlink;
use super::line_edit::LineEdit;
use super::qr;
use super::render::Layout;
use super::share;
use super::theme;
use super::{AGE_OPTIONS, AccountKind, App, BatchKind, LaunchInto, tabs};
use crate::session::Session;
use crate::util;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
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

/// The typing style every one-line box shares, and the cursor's.
fn input_styles() -> (Style, Style) {
    (
        Style::default()
            .fg(theme::colors().value)
            .add_modifier(Modifier::BOLD),
        Style::default().fg(theme::colors().accent),
    )
}

/// A box's ` > ` row, with the cursor drawn where it is in the text.
///
/// Not windowed: the modals these sit in wrap, so a line longer than the box
/// goes onto a second row rather than off its edge, and all of it stays
/// readable — which the send box, at up to five hundred characters, needs.
fn input_line(field: &LineEdit) -> Line<'static> {
    let (text, cursor) = input_styles();
    let mut spans = vec![Span::raw(" > ")];
    spans.extend(field.spans(usize::MAX, text, cursor, "█"));
    Line::from(spans)
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
    let height = wrapped_rows(&lines, width.saturating_sub(2)) + 2;
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border_hi))
        .style(theme::canvas())
        .title(Span::styled(format!(" {title} "), theme::title()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .style(theme::canvas()),
        inner,
    );
    (rect, inner)
}

/// The rows `lines` take once wrapped to `width` columns — an empty line still
/// being one.
fn wrapped_rows(lines: &[Line], width: u16) -> u16 {
    let width = width.max(1) as usize;
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width) as u16)
        .sum()
}

/// Make room for `qr` at line `at` of a modal `width` wide, if it fits.
///
/// Room is blank lines, so the code sits inside the same paragraph as the text
/// around it and the box grows by exactly its height; the row it starts on is
/// returned, for drawing over those lines once the box is up. `None`, and the
/// lines untouched, when the box would then be taller than the screen or too
/// narrow for the code and a column either side of it: the panel is drawn as it
/// would have been without one, rather than with a code `centered` has cut.
fn make_room_for_qr(
    area: Rect,
    width: u16,
    lines: &mut Vec<Line<'static>>,
    at: usize,
    qr: &qr::Qr,
) -> Option<u16> {
    let inner = width.min(area.width.saturating_sub(2)).saturating_sub(2);
    // The box with the code in it: the text, the code and the borders. It has
    // to fit in the rows `centered` allows, which keeps one off each edge of
    // the screen.
    let rows = wrapped_rows(lines, inner) + qr.height + 2;
    if !qr.fits(inner.saturating_sub(2), u16::MAX) || rows > area.height.saturating_sub(2) {
        return None;
    }
    let row = wrapped_rows(&lines[..at], inner);
    lines.splice(at..at, (0..qr.height).map(|_| Line::default()));
    Some(row)
}

/// Draw `qr` over the room [`make_room_for_qr`] left at `row` of `inner`.
fn draw_qr(frame: &mut Frame, inner: Rect, row: u16, qr: &qr::Qr) {
    let area = Rect {
        y: inner.y + row,
        height: qr.height,
        ..inner
    };
    qr.draw(area.intersection(inner), frame.buffer_mut());
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
    let title = Line::from(Span::styled(format!(" {title} "), theme::title()));
    scrollable_modal_titled(frame, area, vec![title], lines, width, scroll)
}

/// [`scrollable_modal`] with titles that are more than a word: the help's
/// carry its filter and the build, which have to stay put on the border while
/// the page under them scrolls. Each line keeps its own alignment.
fn scrollable_modal_titled(
    frame: &mut Frame,
    area: Rect,
    titles: Vec<Line<'static>>,
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
        .style(theme::canvas());
    for title in titles {
        block = block.title(title);
    }

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
            .scroll((scroll, 0))
            .style(theme::canvas()),
        inner,
    );
    super::scrollbar::on_border(
        frame,
        rect,
        total as usize,
        inner.height as usize,
        scroll as usize,
    );
    max_scroll
}

pub(super) fn draw_help(frame: &mut Frame, area: Rect, app: &mut App) {
    let section = |t: &str| {
        (
            HelpRow::Section,
            Line::from(Span::styled(t.to_string(), theme::title())),
        )
    };
    let gap = || (HelpRow::Gap, Line::default());
    let note = |line: Line<'static>| (HelpRow::More, line);
    // The keys as they are bound now, not as they ship: `{action}` in a key
    // column is whatever `[keys]` has put that action on.
    let keymap = &app.keymap;
    let item = |k: &str, d: &str| {
        let k = bound_keys(k, keymap);
        let row = match k.is_empty() {
            true => HelpRow::More,
            false => HelpRow::Entry,
        };
        let line = Line::from(vec![
            // The space after the padding is the floor: a key longer than the
            // column (`g / G  Home / End`) otherwise runs into its description
            // and reads as `EndJump`.
            Span::styled(
                format!("  {k:<16} "),
                Style::default().fg(theme::colors().accent),
            ),
            Span::raw(d.to_string()),
        ]);
        (row, line)
    };
    let rows = vec![
        // The order is what a reader needs first, not what the key handler
        // happens to match first: the row menu is the one entry that makes the
        // rest of this page optional, so it opens the help rather than sitting
        // under "Other" two screens down.
        section("Start here"),
        item("Enter", "Everything you can do to this row, in one menu"),
        item("{help}  F1", "This page"),
        item(
            "{settings}",
            "Settings and keybinds, and the file that holds them",
        ),
        item("{quit}  F10", "Quit"),
        item("+", "Add a Claude account (runs claude setup-token)"),
        gap(),
        section("Navigation"),
        item("↑/{up}  ↓/{down}", "Move between sessions"),
        item("PgUp / PgDn", "Page through the list"),
        item("Ctrl+U / Ctrl+D", "Half a page up / down"),
        item("{top} / {bottom}  Home / End", "Jump to first / last"),
        item(
            "{next_match} / {prev_match}",
            "Next / previous search match (wraps)",
        ),
        item("{bell}", "Jump to the session that rang last"),
        item("{follow}", "Follow mode: keep the selection centered"),
        gap(),
        section("Acting on the selected session"),
        item("{attach}", "Open its terminal in a tab"),
        item("{resume}", "Resume it in a tab of its own"),
        item("{send}", "Type a line into its terminal"),
        item("{handoff}", "Hand its context off to a different agent"),
        item(
            "{conversation}",
            "Read its conversation (works on remote rows)",
        ),
        item("  [ / ]  m", "In it: previous / next turn, markdown source"),
        item("{copy}", "Copy resume command or transcript path"),
        item(
            "{expand} / {expand_all}",
            "Show its subagents / all subagents",
        ),
        item("{delete}", "Delete it (only when it is not running)"),
        item("{terminate}", "Terminate it"),
        gap(),
        section("Several at once"),
        item("{mark}", "Mark / unmark the selected session"),
        item("{unmark_all}", "Clear all marks"),
        item("{delete_marked}", "Delete all marked sessions"),
        item("{kill_marked}", "Terminate all marked live sessions"),
        item("{idle}", "Idle view: live sessions quiet for idle_after"),
        item("", "(6h by default), the biggest memory first"),
        item(
            "  {kill_marked}",
            "In it: stop the marked ones, or all of them,",
        ),
        item("", "skipping any that are working or asking"),
        gap(),
        section("Filter and sort"),
        item(
            "{search}  F3",
            "Filter by label, project, branch, model, id",
        ),
        item("  Tab", "Search inside the transcripts as well"),
        item("  ↑ / ↓", "Bring back an earlier search"),
        item("{sort}  F6  >  <", "Sort by any column"),
        item("F7", "Filter by age (1d / 1w / 1mo)"),
        item("{cost_floor}", "Cost floor: only sessions costing ≥ $X"),
        item("{running_only}", "Show only running sessions"),
        item("{tree}", "Tree view: group by repository and worktree"),
        item(
            "  Enter / Space",
            "Fold or unfold the group under the cursor",
        ),
        item("Esc", "Clear one filter layer per press"),
        note(Line::from(Span::styled(
            "  Clicking a column header sorts by it too.",
            theme::dim(),
        ))),
        gap(),
        // Its own section rather than a line under search: every box on the
        // dashboard takes these, and a reader looking for how to cut a word
        // out of the send box would not look under the filter for it.
        section("Typing in a box"),
        item("←→  Ctrl+B / F", "Move a character"),
        item("Alt+B / F", "Move a word (Ctrl+← / → too)"),
        item("Ctrl+A / E", "To the start / end (Home / End too)"),
        item("Ctrl+H / D", "Delete before / after (Backspace, Delete)"),
        item("Ctrl+W  Alt+D", "Cut the word before / after the cursor"),
        item("Ctrl+U / K", "Cut to the start / end"),
        item("Ctrl+Y", "Put the last cut back"),
        item("/", "In this page: filter it (Esc clears)"),
        gap(),
        section("Bottom panels"),
        item("←  →", "Move between bottom panels"),
        item("Tab / Shift+Tab", "Same, either direction"),
        item("1 – 9", "Jump to a panel directly"),
        item(
            "{panel_up} / {panel_down}",
            "Scroll inside the active panel",
        ),
        item(
            "{panel_top} / {panel_bottom}",
            "Jump to the top / bottom of it",
        ),
        item(
            "{tool_filter_prev} / {tool_filter_next}",
            "Move through the Tool Activity filter",
        ),
        item("{live_filter}", "Toggle the Tool Activity live filter"),
        item("{diffs}", "Toggle inline diffs for edits"),
        gap(),
        section("Tabs and splits"),
        item(
            "{new_tab} or Alt+n",
            "New tab: an agent, a shell, or one still running",
        ),
        item("Alt+v / Alt+s", "Split the tab right / down"),
        item("Alt+← / →", "Previous / next tab"),
        item("Alt+1 – 9", "Jump to a tab (1 is the dashboard)"),
        item("Alt+t", "Pick a tab from a list, typing to narrow it"),
        item("  Tab", "In it: all / needs you / working / idle"),
        item("Alt+b", "Jump to the next tab that needs you"),
        item(
            "Alt+Shift+← / →",
            "Move this tab along the bar (or drag it with the mouse)",
        ),
        item("Right-click / Alt+r", "Rename or recolour a tab"),
        item("Alt+o", "Move focus to the next pane"),
        item("Alt+w", "Close the pane and stop its agent"),
        item("Alt+Shift+W", "The same, by a name that says so"),
        item(
            "Alt+Shift+R",
            "Restart its agent on the same session (after an update)",
        ),
        item("", "On the dashboard: the selected row's tab"),
        item(
            "{restart_all}",
            "On the dashboard: restart every agent tab,",
        ),
        item("", "leaving the ones mid-turn alone"),
        item(
            "Alt+Shift+C",
            "Record the pane to an asciinema .cast, or stop",
        ),
        item("", "and say where it went (~/.local/share/cctop/casts)"),
        item("F9", "Paste the clipboard's image as a file path"),
        item("Ctrl+V", "The same, in terminals that send it"),
        item("Home / End", "In a Claude pane: top / bottom of the chat"),
        item("F12", "Back to the dashboard, leaving it running"),
        gap(),
        section("Pasting an image"),
        item("", "A terminal carries text, never a picture — so cctop"),
        item("", "writes the image to a file and types its path, which"),
        item("", "is how every one of these agents reads one."),
        item("F9", "Does that with the clipboard of this machine"),
        item(
            "over ssh",
            "That clipboard is on the machine you sshed from,",
        ),
        item("", "so F9 asks the terminal instead — kitty answers"),
        item("", "when its clipboard_control allows reads. Windows"),
        item("", "Terminal never does: tools/clipboard-bridge.ps1"),
        item("", "on it plus ssh -R 8377:127.0.0.1:8377 gives F9"),
        item("", "and Ctrl+V the clipboard anyway. Otherwise the"),
        item("", "page — `cctop serve` here, open it there — takes"),
        item("", "a real paste, and base64 text is filed too:"),
        item("", "  wl-paste -t image/png | base64 -w0 | wl-copy"),
        item("", "  pngpaste - | base64 | pbcopy"),
        item("", "  PowerShell: see docs/the-table.md"),
        gap(),
        section("Mouse"),
        item("Right-click a row", "Its menu, as Enter opens it"),
        item("Click a footer hint", "Presses that key (q asks twice)"),
        item("Click [y] / [n]", "Answers a confirmation"),
        gap(),
        section("Elsewhere"),
        item("{open_hosted}", "Open the agent this cctop launched"),
        item("{notify}", "Bell + desktop alert when a session needs you"),
        item("{share}", "Share the agent's terminal to a browser"),
        item(
            "{serve}",
            "Serve this table to a browser, with or without a tunnel",
        ),
        item("{optimize}", "What was spent and not got back"),
        item("{compare}", "How each model did on the work you gave it"),
        item("{hooks}  F8", "Agent integration: what reports to cctop"),
        item("{refresh}  F5", "Refresh now"),
        gap(),
        section("Environment"),
        item("CCTOP_THEME", "light / dark / auto (default: auto)"),
        item("NO_COLOR", "Drop colour; shape and weight carry the state"),
        item(
            "CCTOP_COLUMNS_HIDE",
            "Column keys to hide, e.g. tok_rate,mem",
        ),
        note(Line::from(Span::styled(
            "  Columns also drop by priority on their own as the window narrows.",
            theme::dim(),
        ))),
        gap(),
        note(Line::from(Span::styled(
            "  Costs are estimates from published per-token rates. Flat-rate plans",
            theme::dim(),
        ))),
        note(Line::from(Span::styled(
            "  (Max, Pro, Team) bill differently, so these may not match your invoice.",
            theme::dim(),
        ))),
    ];

    let query = app.help_filter.trim();
    let mut lines = filter_help(rows, query);
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  Nothing in the help mentions “{query}”"),
            theme::dim(),
        )));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        match (app.help_typing, query.is_empty()) {
            (true, _) => "  Enter keeps the filter   Esc clears it",
            (false, true) => "  ↑ / ↓ scroll   / filter   any other key returns",
            (false, false) => "  ↑ / ↓ scroll   / edit the filter   Esc clears it",
        },
        theme::dim(),
    )));

    // On the border rather than as the page's first row, so it stays where it
    // is typed while the page scrolls under it.
    let mut title = vec![Span::styled(" Help ", theme::title())];
    if app.help_typing || !app.help_filter.is_empty() {
        let (text, cursor) = input_styles();
        title.push(Span::styled("/ ", theme::dim()));
        match app.help_typing {
            true => title.extend(app.help_filter.spans(40, text, cursor, "█")),
            false => title.push(Span::styled(app.help_filter.to_string(), text)),
        }
        title.push(Span::raw(" "));
    }
    // Which build this is, on the right of the same border: the page is where
    // someone asking "is this the new one?" already is.
    let build =
        Line::from(Span::styled(format!(" {} ", build_label()), theme::dim())).right_aligned();
    app.help_max_scroll = scrollable_modal_titled(
        frame,
        area,
        vec![Line::from(title), build],
        lines,
        76,
        app.help_scroll,
    );
}

/// `cctop 0.19.2 (1a2b3c4d5)`: the version, and the commit it was built from
/// when the build knew one (see `commit` in `build.rs`).
///
/// The commit rather than the version alone because the version is only
/// bumped at a release: every build between two of them says the same number,
/// and "which one is this" is the question the number cannot answer.
pub(super) fn build_label() -> String {
    match env!("CCTOP_COMMIT") {
        "" => format!("cctop {}", env!("CARGO_PKG_VERSION")),
        commit => format!("cctop {} ({commit})", env!("CARGO_PKG_VERSION")),
    }
}

/// A help key column with each `{action}` in it replaced by the key that
/// action is on now, so a key moved in `[keys]` is where the help says it is.
///
/// Two keys joined by ` / ` that share their modifiers say them once —
/// `Shift+Home / End` rather than `Shift+Home / Shift+End` — which is how the
/// page was written by hand, and what keeps a pair inside the key column. An
/// action nothing is bound to reads `unbound`, rather than the page quietly
/// naming a key that now does something else or nothing at all.
fn bound_keys(template: &str, keymap: &crate::settings::Keymap) -> String {
    let mut out = String::new();
    let mut rest = template;
    // The modifiers of the key just written, while nothing but ` / ` has
    // followed it.
    let mut prefix: Option<String> = None;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}').map(|c| open + c) else {
            break;
        };
        let between = &rest[..open];
        let label = keymap
            .key_of(&rest[open + 1..close])
            .map(crate::settings::key_label)
            .unwrap_or_else(|| "unbound".to_string());
        let mods = label
            .rfind('+')
            .filter(|&i| i + 1 < label.len())
            .map(|i| label[..=i].to_string());
        out.push_str(between);
        match (&prefix, &mods) {
            (Some(p), Some(m)) if between == " / " && p == m => out.push_str(&label[m.len()..]),
            _ => out.push_str(&label),
        }
        prefix = mods;
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// What a row of the help is, for the filter to know what to keep together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelpRow {
    /// A heading, kept whenever anything under it is.
    Section,
    /// A key and what it does: the unit the filter keeps or drops.
    Entry,
    /// The rest of the entry above it — a description wrapped by hand, or a
    /// note under it. Kept or dropped with it, so a match on its second line
    /// shows the key it is the second line of.
    More,
    /// Between sections.
    Gap,
}

/// The help narrowed to the entries that mention `query`, each under its
/// heading; the whole page when there is no query.
///
/// A plain substring, case-insensitive, over an entry's lines joined — the
/// key column included, so `ctrl+u` finds the keys as well as `half a page`
/// finds what they do. A heading that matches keeps its whole section: typing
/// `tabs` is asking for the section called that, not for the entries that
/// happen to say the word.
///
/// ponytail: the match is not highlighted. The page left is a few lines long,
/// and the word typed is on the border above it.
fn filter_help(rows: Vec<(HelpRow, Line<'static>)>, query: &str) -> Vec<Line<'static>> {
    if query.is_empty() {
        return rows.into_iter().map(|(_, line)| line).collect();
    }
    let needle = query.to_lowercase();
    let text = |line: &Line| -> String {
        line.spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>()
            .to_lowercase()
    };

    // (heading, entries) in page order.
    let mut sections: Vec<(Option<Line<'static>>, Vec<Vec<Line<'static>>>)> = Vec::new();
    let mut open = false;
    for (row, line) in rows {
        match row {
            HelpRow::Section => {
                sections.push((Some(line), Vec::new()));
                open = false;
            }
            HelpRow::Gap => open = false,
            HelpRow::Entry | HelpRow::More => {
                if sections.is_empty() {
                    sections.push((None, Vec::new()));
                }
                let entries = &mut sections.last_mut().expect("pushed above").1;
                match (row, open) {
                    (HelpRow::More, true) => entries.last_mut().expect("open").push(line),
                    _ => entries.push(vec![line]),
                }
                open = true;
            }
        }
    }

    let mut out = Vec::new();
    for (heading, entries) in sections {
        let whole = heading.as_ref().is_some_and(|h| text(h).contains(&needle));
        let kept: Vec<_> = entries
            .into_iter()
            .filter(|entry| {
                whole
                    || entry
                        .iter()
                        .map(text)
                        .collect::<Vec<_>>()
                        .join(" ")
                        .contains(&needle)
            })
            .collect();
        if kept.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(Line::default());
        }
        out.extend(heading);
        out.extend(kept.into_iter().flatten());
    }
    out
}

/// Every setting and dashboard key at the value it has now, marked where
/// `config.toml` is what set it — so the panel answers both "what can I tune"
/// and "what did my file actually do" — with a cursor to change them in place.
pub(super) fn draw_settings(frame: &mut Frame, area: Rect, app: &mut App) {
    use crate::settings::{BINDINGS, SETTINGS};
    let section = |t: &str| Line::from(Span::styled(t.to_string(), theme::title()));
    let path = crate::util::tildify(&crate::config::CONFIG_FILE.to_string_lossy());
    let mut lines = vec![
        Line::from(vec![Span::styled("  File ", theme::dim()), Span::raw(path)]),
        Line::from(Span::styled(
            "  * set in the file, rather than left at the default",
            theme::dim(),
        )),
    ];
    for problem in &app.settings.problems {
        lines.push(Line::from(Span::styled(
            format!("  ! {problem}"),
            theme::failed(),
        )));
    }

    // One row per setting, then per keybind; `row` is the cursor's index.
    let mut cursor_line = 0;
    let mut push_row = |lines: &mut Vec<Line<'static>>,
                        row: usize,
                        name: &str,
                        value: String,
                        set: bool,
                        what: &str| {
        let here = row == app.settings_cursor;
        let accent = Style::default().fg(theme::colors().accent);
        let style = if set {
            accent.add_modifier(Modifier::BOLD)
        } else {
            accent
        };
        let value = match (here, &app.settings_input) {
            (true, _) if app.settings_capture => {
                vec![Span::styled(
                    format!("{:<14}", "press a key… (Esc cancels)"),
                    style,
                )]
            }
            (true, Some(input)) => {
                let mut spans = input.spans(usize::MAX, style, accent, "█");
                // Padded to the column the other rows fill, so the `*` and
                // the description do not shift as the value is typed.
                let used: usize = spans.iter().map(Span::width).sum();
                spans.push(Span::raw(" ".repeat(14usize.saturating_sub(used))));
                spans
            }
            _ => vec![Span::styled(format!("{value:<14}"), style)],
        };
        let mut line = Line::from_iter(
            std::iter::once(Span::raw(format!("  {name:<18}")))
                .chain(value)
                .chain([
                    Span::styled(if set { " * " } else { "   " }, theme::dim()),
                    Span::styled(what.to_string(), theme::dim()),
                ]),
        );
        if here {
            cursor_line = lines.len();
            line = line.style(theme::selected());
        }
        lines.push(line);
    };

    lines.push(Line::default());
    lines.push(section("[settings]"));
    for (row, (name, _, what)) in SETTINGS.iter().enumerate() {
        let (value, set) = app.settings.value_of(name);
        push_row(&mut lines, row, name, value, set, what);
    }
    lines.push(Line::default());
    lines.push(section("[keys]  on the session table"));
    for (i, (action, default, what)) in BINDINGS.iter().enumerate() {
        let key = app.settings.key_for(action).to_string();
        let set = key != *default;
        push_row(&mut lines, SETTINGS.len() + i, action, key, set, what);
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "  ↵ change   ⌫ reset   e open in your editor   Esc close",
        theme::dim(),
    )));

    // Keep the cursor on screen: the same height `scrollable_modal` will give
    // the box, less its border.
    let inner = (lines.len() as u16 + 2)
        .min(area.height.saturating_sub(2))
        .saturating_sub(2)
        .max(1);
    let at = cursor_line as u16;
    // The first row brings the file's name and any problems back into view.
    if app.settings_cursor == 0 {
        app.settings_scroll = 0;
    } else if at < app.settings_scroll {
        app.settings_scroll = at;
    } else if at >= app.settings_scroll + inner {
        app.settings_scroll = at + 1 - inner;
    }
    scrollable_modal(frame, area, "Settings", lines, 96, app.settings_scroll);
}

pub(super) fn draw_search(frame: &mut Frame, area: Rect, app: &App) {
    const WIDTH: u16 = 62;
    let text_w = WIDTH as usize - 4;

    let mut lines = vec![
        input_line(&app.search),
        Line::from(Span::styled(
            format!(
                " {} of {} session{}",
                app.matched,
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
    let (_, inner) = modal(
        frame,
        area,
        &format!("Install rmux with {}?", install.manager),
        lines,
        62,
    );
    // The install script's URL, when that is the route, opens from where it is
    // printed: reading a script before piping it into `sh` is the one check
    // this prompt can make easy, and it is a click rather than a retype.
    let url = command
        .split_whitespace()
        .find(|word| word.starts_with("https://"));
    if let Some(url) = url {
        hyperlink::link_shown(frame.buffer_mut(), inner, inner.y, url, url);
    }
}

/// Whether this cctop is serving its table to a browser, and on what.
///
/// The links are drawn as their origin and made clickable — an OSC 8 hyperlink
/// to the whole URL, token and all, laid over the origin — rather than printed
/// in full. A served link carries the token that opens it, so the full text is
/// something to hand over deliberately — `y` puts it on the clipboard — and not
/// something to leave on screen. It also does not fit: a tunnel hostname plus a
/// token is most of a hundred columns.
///
/// The tunnel link can also be drawn as a QR code, with `c`, for a phone to
/// open it. Only the tunnel's: the loopback link is the one thing a phone
/// cannot reach, and it is the only other link this panel has — the dashboard
/// binds nothing but `127.0.0.1`, so there is no LAN address to offer either.
/// The code encodes what `y` copies, token and all, which is why it waits to be
/// asked for rather than appearing with the link; see [`qr`].
pub(super) fn draw_serve(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();
    // Where the code goes if there is one to draw: under the warning that says
    // what holding it grants, so the two are read together.
    let mut qr_at = None;
    // `(what is drawn, where it goes)`, top to bottom, for the hyperlinks laid
    // over the drawn origins once the paragraph has placed them.
    let mut links: Vec<(String, String)> = Vec::new();

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
            fn show(
                lines: &mut Vec<Line<'static>>,
                links: &mut Vec<(String, String)>,
                what: &str,
                url: &str,
            ) {
                lines.push(Line::from(Span::styled(format!(" {what}"), theme::dim())));
                lines.push(Line::from(Span::styled(
                    format!("  {}", origin_of(url)),
                    Style::default().fg(theme::colors().accent),
                )));
                links.push((origin_of(url), url.to_string()));
            }
            show(&mut lines, &mut links, "This machine", &serving.local);
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
                    show(&mut lines, &mut links, "The internet", public);
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
                    if app.serve_qr {
                        lines.push(Line::default());
                        qr_at = Some((lines.len(), qr::encode(public)));
                    }
                }
            }
            // The same origin as whichever link is handed out, but a different
            // credential behind it — the label has to say that, because the
            // drawn origin cannot.
            if !serving.readonly.is_empty() {
                lines.push(Line::default());
                show(
                    &mut lines,
                    &mut links,
                    "Read-only — watches, never acts",
                    &serving.readonly,
                );
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
    let tunnelled = app.serving.as_ref().is_some_and(|s| s.public.is_some());
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        match (app.serving.is_some(), tunnelled, app.serve_qr) {
            (true, true, false) => " o open · y copy · c QR code · l local · t + tunnel · x stop",
            (true, true, true) => " o open · y copy · c hide QR · l local · t + tunnel · x stop",
            (true, false, _) => " o open · y copy · l local · t + tunnel · x stop",
            (false, ..) => " l this machine only · t also a public tunnel",
        },
        theme::dim(),
    )));

    // Sized to the longest line, so a tunnel hostname is one line and not two,
    // and capped to the screen, where it wraps instead — into a box now tall
    // enough for it. A code asked for widens it to the code, when the screen
    // has the columns. `max` then `min` and not `clamp`: on a screen narrower
    // than the floor the cap is below it, and `clamp` panics on that.
    let widest = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let code_width = match &qr_at {
        Some((_, Some(qr))) => qr.width + 4,
        _ => 0,
    };
    let width = widest
        .saturating_add(3)
        .max(code_width)
        .max(56)
        .min(area.width.saturating_sub(4).max(24));
    let room = match &qr_at {
        Some((at, Some(qr))) => make_room_for_qr(area, width, &mut lines, *at, qr),
        _ => None,
    };
    // `c` was pressed and there is no code to show for it. Said, because a key
    // that visibly does nothing reads as a dead one; and said as the thing to
    // do about it, which is nearly always a taller terminal.
    if let (Some((at, _)), None) = (&qr_at, room) {
        lines.insert(
            *at,
            Line::from(Span::styled(
                " No room here for a QR code — try a taller terminal.",
                theme::dim(),
            )),
        );
    }
    let (_, inner) = modal(frame, area, "Serve this table to a browser", lines, width);
    if let (Some((_, Some(qr))), Some(row)) = (&qr_at, room) {
        draw_qr(frame, inner, row, qr);
    }
    // Searched for in order, each below the last: the read-only link has the
    // same origin as the local one, and only its place says which token it is.
    let mut from = inner.y;
    for (shown, url) in &links {
        if let Some(row) = hyperlink::link_shown(frame.buffer_mut(), inner, from, shown, url) {
            from = row + 1;
        }
    }
}

/// A terminal `W` has just shared off this machine, as a code a phone can scan.
///
/// The operator link is a credential for a live agent — whoever opens it types
/// into it — and it is otherwise kept off the screen entirely; see
/// [`App::share_selected`](super::App) for the one reason it is drawn here.
/// The warning sits directly under the code for that reason, and the PIN beside
/// it because the browser asks for it once the link has opened.
///
/// Without the rows or columns for the code the panel still opens, saying so:
/// `W` was pressed to hand the terminal over, and the clipboard has done that
/// whether or not the code could be drawn.
pub(super) fn draw_share_qr(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let Some(share) = &app.share_qr else {
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            format!(" {}", crate::util::truncate(&share.label, 50)),
            theme::value(),
        )),
        Line::default(),
    ];
    let at = lines.len();
    lines.push(Line::default());
    if let Some(pin) = &share.pin {
        lines.push(Line::from(vec![
            Span::raw(" Pairing code "),
            Span::styled(pin.clone(), theme::value()),
            Span::styled(" — asked for once the link opens.", theme::dim()),
        ]));
    }
    lines.push(Line::from(Span::styled(
        " Whoever holds this link can type at the agent.",
        Style::default().fg(theme::colors().cost_mid),
    )));
    lines.push(Line::from(Span::styled(
        " It is on your clipboard as well.",
        theme::dim(),
    )));
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(DISMISS_KEYS, theme::dim())));

    let code = qr::encode(&share.link);
    let width = code
        .as_ref()
        .map_or(0, |qr| qr.width + 4)
        .max(56)
        .min(area.width.saturating_sub(4).max(24));
    let room = code
        .as_ref()
        .and_then(|qr| make_room_for_qr(area, width, &mut lines, at, qr));
    if room.is_none() {
        lines[at] = Line::from(Span::styled(
            " No room here for a QR code — try a taller terminal.",
            theme::dim(),
        ));
    }
    let last = lines.len() as u16 - 1;
    let (outer, inner) = modal(frame, area, "Open this terminal elsewhere", lines, width);
    if let (Some(qr), Some(row)) = (&code, room) {
        draw_qr(frame, inner, row, qr);
    }
    confirm_chips(
        layout,
        outer,
        inner,
        last,
        DISMISS_KEYS,
        &[("[any key]", dismiss())],
    );
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
            (true, true) => theme::selected(),
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
        .style(theme::canvas())
        .title(Span::styled(format!(" {subject} "), theme::title()))
        .title_bottom(Span::styled(" ↑↓ Enter · Esc ", theme::dim()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), inner);

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
            theme::selected()
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
        Line::from_iter(
            std::iter::once(Span::styled(
                " in ",
                Style::default().fg(theme::colors().label),
            ))
            // Windowed around the cursor when a long path outgrows the box,
            // which at the end is its tail: the end is the part being typed,
            // and a field that showed the start would hide the cursor as soon
            // as it mattered.
            .chain(app.launch_cwd_input.spans(
                // The cursor's cell is in this, where it used to follow it.
                WIDTH as usize - 7,
                match app.launch_cwd_bad {
                    true => Style::default().fg(theme::colors().cost_high),
                    false => theme::value(),
                },
                theme::value(),
                "▏",
            ))
            .chain(std::iter::once(Span::styled(
                match app.launch_cwd_bad {
                    true => "  no such directory",
                    false => "",
                },
                Style::default().fg(theme::colors().cost_high),
            ))),
        )
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
                    true => theme::selected(),
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
    // Beside the list rows only: the footer below them never scrolls, and a
    // track running past the list would make it look as if it did.
    super::scrollbar::draw(
        frame,
        super::scrollbar::right_border(outer, inner.y, shown as u16),
        total,
        shown,
        offset,
    );
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

/// Ask before running `cctop --update` on another machine.
///
/// The command is shown as it will run, because this is the one key in cctop
/// that changes something that is not on this computer: someone saying yes to
/// it should be able to read, in full, what they are saying yes to.
pub(super) fn draw_remote_update_confirm(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    layout: &mut Layout,
) {
    let Some(host) = app
        .remote_update
        .as_deref()
        .and_then(|t| app.remote_host(t))
    else {
        return;
    };
    let theirs = match app.remote_versions.get(&host.target) {
        Some(crate::fleet::Probe::Version(v)) => v.clone(),
        _ => "an unknown version".to_string(),
    };
    let lines = vec![
        Line::from(Span::styled(format!("  {}", host.target), theme::value())),
        Line::from(Span::styled(
            format!(
                "  runs cctop {theirs}; this one is {}",
                crate::update::current_version()
            ),
            theme::dim(),
        )),
        Line::default(),
        Line::from(Span::raw("  Run this, to fetch the newest release there:")),
        Line::from(Span::styled(
            format!("    {}", host.shown_command(&["--update"])),
            Style::default().fg(theme::colors().cost_mid),
        )),
        Line::default(),
        Line::from(Span::raw(
            "  It replaces the binary on that machine. Nothing is asked there: if it",
        )),
        Line::from(Span::raw(
            "  needs root, cctop says so and shows the sudo command to run yourself.",
        )),
        Line::default(),
        Line::from(Span::styled(REMOTE_UPDATE_KEYS, theme::dim())),
    ];
    let last = lines.len() as u16 - 1;
    let (outer, inner) = modal(frame, area, "Update cctop over there?", lines, 78);
    confirm_chips(
        layout,
        outer,
        inner,
        last,
        REMOTE_UPDATE_KEYS,
        &[("[y]", ch('y')), ("[n / Esc]", dismiss())],
    );
}

const REMOTE_UPDATE_KEYS: &str = "  [y] update    [n / Esc] cancel";

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
    let (verb, noun) = match app.batch {
        BatchKind::Delete => ("delete", "sessions"),
        BatchKind::Kill => ("terminate", "live sessions"),
        BatchKind::Reclaim => return draw_reclaim_confirm(frame, area, app, layout),
    };
    let ms = app.marked_sessions();
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
            BatchKind::Kill | BatchKind::Reclaim => {
                "  Unsaved work in the agents may be interrupted."
            }
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

/// The idle view's `K`: what will be stopped, how much memory that gives
/// back, and — named, not counted — what is being left running and why.
///
/// The skipped ones are listed because the count alone would read as cctop
/// having failed at them; a reason per row says it chose not to.
fn draw_reclaim_confirm(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    const SHOWN: usize = 8;
    const KEPT_SHOWN: usize = 4;
    let plan = app.reclaim_plan();
    let now = chrono::Utc::now();
    let n = plan.stop.len();
    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "  Stop {n} idle session{}, freeing {}",
                if n == 1 { "" } else { "s" },
                util::compact_bytes(plan.bytes)
            ),
            theme::value(),
        )),
        Line::default(),
    ];
    // Label, then how long it has been quiet and what it holds, in the table's
    // own spellings so the two can be read against each other.
    for s in plan.stop.iter().take(SHOWN) {
        lines.push(Line::from(vec![
            Span::styled(
                format!("    · {:<36}", util::truncate(s.display_label(), 36)),
                theme::dim(),
            ),
            Span::styled(
                format!(
                    "{:>5}",
                    util::relative_age(super::idle::quiet_since(s), &now)
                ),
                theme::dim(),
            ),
            Span::styled(
                format!("{:>8}", util::compact_bytes(super::idle::tree_memory(s))),
                theme::value(),
            ),
        ]));
    }
    if n > SHOWN {
        let rest: u64 = plan.stop[SHOWN..]
            .iter()
            .map(|s| super::idle::tree_memory(s))
            .sum();
        lines.push(Line::from(Span::styled(
            format!(
                "    … and {} more, {}",
                n - SHOWN,
                util::compact_bytes(rest)
            ),
            theme::dim(),
        )));
    }
    if !plan.keep.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::raw(format!(
            "  Left running ({}):",
            plan.keep.len()
        ))));
        for (s, why) in plan.keep.iter().take(KEPT_SHOWN) {
            lines.push(Line::from(Span::styled(
                format!(
                    "    · {} — {}",
                    util::truncate(s.display_label(), 30),
                    why.reason()
                ),
                theme::dim(),
            )));
        }
        if plan.keep.len() > KEPT_SHOWN {
            lines.push(Line::from(Span::styled(
                format!("    … and {} more", plan.keep.len() - KEPT_SHOWN),
                theme::dim(),
            )));
        }
    }
    lines.push(Line::default());
    let warn = Style::default().fg(theme::colors().cost_mid);
    lines.push(Line::from(Span::styled(
        "  Each is sent SIGTERM and exits cleanly; R resumes it.",
        warn,
    )));
    lines.push(Line::from(Span::styled(
        "  A prompt typed into one and not sent is lost.",
        warn,
    )));
    lines.push(Line::default());
    let hint = format!("  [y] stop {n}    [n / Esc] cancel");
    lines.push(Line::from(Span::styled(hint.clone(), theme::dim())));
    let last = lines.len() as u16 - 1;
    let (outer, inner) = modal(frame, area, "Stop idle sessions?", lines, 62);
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
        BatchKind::Kill | BatchKind::Reclaim => (
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
    let (text, cursor) = input_styles();
    // An empty floor shows the zero it means, with the cursor after it as it
    // always was — it is a hint, not text the cursor can move through.
    let mut input = vec![Span::raw(" $ ")];
    match app.cost_input.is_empty() {
        true => input.extend([Span::styled("0.00", text), Span::styled("█", cursor)]),
        false => input.extend(app.cost_input.spans(usize::MAX, text, cursor, "█")),
    }
    let lines = vec![
        Line::from(Span::styled(
            " Only show sessions whose total cost is at least:",
            theme::dim(),
        )),
        Line::from(input),
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
        input_line(&app.send_input),
        Line::default(),
        Line::from(Span::styled(
            " Enter send   F9 paste an image   Esc cancel",
            theme::dim(),
        )),
    ];
    modal(frame, area, "Send to session", lines, 64);
}

/// Naming and painting a workspace tab, opened by right-clicking it in the bar.
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
    // A swatch per hue with "none" leading, the current pick bracketed. The
    // name is spelled beside them because under NO_COLOR every swatch is the
    // same ink and the word is all there is to go on.
    let mut swatches = vec![Span::styled(" Colour ", theme::dim())];
    let mut named = "none";
    for option in std::iter::once(None).chain(theme::Hue::ALL.into_iter().map(Some)) {
        let picked = option == app.rename_color;
        if picked {
            named = option.map_or("none", theme::Hue::name);
        }
        let (glyph, style) = match option {
            Some(hue) => ("●", Style::default().fg(hue.color())),
            None => ("○", theme::dim()),
        };
        swatches.push(Span::raw(" "));
        if picked {
            swatches.push(Span::styled("[", theme::dim()));
            swatches.push(Span::styled(glyph, style.add_modifier(Modifier::BOLD)));
            swatches.push(Span::styled("]", theme::dim()));
        } else {
            swatches.push(Span::styled(format!(" {glyph} "), style));
        }
    }
    swatches.push(Span::styled(format!("  {named}"), theme::value()));

    let lines = vec![
        Line::from(vec![
            Span::styled(" Now called ", theme::dim()),
            Span::styled(app.rename_was.clone(), theme::value()),
        ]),
        Line::from(Span::styled(
            " The name and colour follow the tab into every cctop on this machine.",
            theme::dim(),
        )),
        Line::default(),
        input_line(&app.rename_input),
        Line::default(),
        Line::from(swatches),
        Line::default(),
        Line::from(Span::styled(
            " Enter apply   ← → colour   Esc cancel",
            theme::dim(),
        )),
    ];
    let (outer, _) = modal(frame, area, "Rename tab", lines, 64);
    layout.modal_rect = Some(outer);
}

/// The tab switcher, opened by `Alt+t`.
///
/// The list is the whole bar — the dashboard first, then the tabs in bar
/// order — narrowed by whatever has been typed. Rows longer than the box
/// are windowed around the cursor rather than scrolled: the pick is what
/// moves, not the frame around it.
pub(super) fn draw_switch_tab(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    layout: &mut super::render::Layout,
) {
    const WIDTH: u16 = 60;
    /// Rows of list under the query line. Ten is a screenful of tab names;
    /// past it the window slides under the cursor instead of growing.
    const ROWS: usize = 10;
    /// The column the title is fixed to before the state word.
    const TITLE_W: usize = 38;

    let matches = app.switch_matches();
    let cursor = app.switch_cursor.min(matches.len().saturating_sub(1));
    let start = cursor
        .saturating_sub(ROWS / 2)
        .min(matches.len().saturating_sub(ROWS));
    let shown = &matches[start..matches.len().min(start + ROWS)];

    let mut lines = vec![input_line(&app.switch_filter), Line::default()];
    if start > 0 {
        lines.push(Line::from(Span::styled(
            format!("    … {start} above"),
            theme::dim(),
        )));
    }
    for (row, &i) in shown.iter().enumerate() {
        let tab = i.checked_sub(1).and_then(|t| app.tabs.get(t));
        let title = tab.map_or_else(|| "Dashboard".to_string(), tabs::Tab::title);
        // The bar's own honesty about digits: 1 is the dashboard, and a
        // position past 9 has no key to be labelled with.
        let number = if i < 9 {
            (i + 1).to_string()
        } else {
            String::new()
        };
        let state = match app.switch_attention(i) {
            Some(tabs::Attention::NeedsInput) => Some(("needs you", theme::colors().cost_mid)),
            Some(tabs::Attention::Idle) => Some(("idle", theme::colors().cost_low)),
            // Something to say about every row that asks nothing: the tab
            // you would land on by doing nothing at all.
            None if i == app.tab => Some(("current", theme::colors().dim)),
            None => None,
        };

        let mut spans = vec![
            Span::styled(
                if start + row == cursor {
                    " › "
                } else {
                    "   "
                },
                Style::default().fg(theme::colors().accent),
            ),
            Span::styled(format!("{number:>2} "), theme::dim()),
        ];
        match tab.and_then(|tab| tab.color) {
            Some(hue) => spans.push(Span::styled("● ", Style::default().fg(hue.color()))),
            None => spans.push(Span::raw("  ")),
        }
        spans.push(Span::styled(
            format!(
                "{:<width$}",
                super::render::elide(&title, TITLE_W),
                width = TITLE_W
            ),
            theme::value(),
        ));
        if let Some((word, color)) = state {
            spans.push(Span::styled(
                format!(" {word:>9}"),
                Style::default().fg(color),
            ));
        }
        let mut line = Line::from(spans);
        if start + row == cursor {
            line = line.patch_style(theme::selected());
        }
        lines.push(line);
    }
    if start + shown.len() < matches.len() {
        lines.push(Line::from(Span::styled(
            format!("    … {} below", matches.len() - start - shown.len()),
            theme::dim(),
        )));
    }
    if matches.is_empty() {
        // Say which filter emptied it: a name that matches a tab that is
        // merely not in this state is not a name that is wrong.
        let empty = match (
            app.switch_state.predicate(),
            app.switch_filter.trim().is_empty(),
        ) {
            (None, _) => "    No tab by that name".to_string(),
            (Some(state), true) => format!("    No tab {state}"),
            (Some(state), false) => format!("    No tab by that name {state}"),
        };
        lines.push(Line::from(Span::styled(empty, theme::dim())));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " Enter go   ↑↓ choose   Tab state   Esc cancel",
        theme::dim(),
    )));

    let title = match app.switch_state.word() {
        Some(state) => format!("Go to tab · {state}"),
        None => "Go to tab".to_string(),
    };
    let (outer, inner) = modal(frame, area, &title, lines, WIDTH);
    layout.modal_rect = Some(outer);
    // Down the rows the window covers, "… above" and "… below" included, and
    // not the query line or the keys: those stay put while the list slides.
    let list_rows =
        usize::from(start > 0) + shown.len() + usize::from(start + shown.len() < matches.len());
    super::scrollbar::draw(
        frame,
        super::scrollbar::right_border(outer, inner.y + 2, list_rows as u16),
        matches.len(),
        ROWS,
        start,
    );
}

/// Where on `screen` `link` is drawn: `(row, columns)` for the row it starts on
/// and each row after that carries a piece of it.
///
/// Columns and not only rows, because the rows are shared: the first has
/// whatever the command printed before the URL, and a hyperlink laid over that
/// would make the prompt text open the sign-in page too.
fn link_cells(screen: &vt100::Screen, link: &str) -> Vec<(u16, std::ops::Range<u16>)> {
    let (rows, width) = screen.size();
    // One char per column, so an index into the text is a column. The URL is
    // ASCII, so nothing it is matched against is lost by keeping only the
    // first char of a cell; a wide glyph's second column reads as a blank.
    let text = |row: u16| -> Vec<char> {
        (0..width)
            .map(|col| {
                screen
                    .cell(row, col)
                    .and_then(|cell| cell.contents().chars().next())
                    .unwrap_or(' ')
            })
            .collect()
    };
    // The run of non-blank columns from `start`.
    let run = |chars: &[char], start: usize| -> std::ops::Range<u16> {
        let len = chars[start..].iter().take_while(|c| **c != ' ').count();
        start as u16..(start + len) as u16
    };
    let Some((first, start)) = (0..rows).find_map(|row| {
        let chars = text(row);
        let at = chars
            .windows(8)
            .position(|w| w.iter().copied().eq("https://".chars()))?;
        Some((row, run(&chars, at)))
    }) else {
        return Vec::new();
    };
    let mut out = vec![(first, start)];
    for row in first + 1..rows {
        let chars = text(row);
        let Some(at) = chars.iter().position(|c| *c != ' ') else {
            break;
        };
        let piece = run(&chars, at);
        let shown: String = chars[piece.start as usize..piece.end as usize]
            .iter()
            .collect();
        // Blanks after the piece that are not the end of the row mean more text
        // on it, which the URL does not have.
        let rest_blank = chars[piece.end as usize..].iter().all(|c| *c == ' ');
        if !rest_blank || !link.contains(&shown) {
            break;
        }
        out.push((row, piece));
    }
    out
}

/// The add-account popup: a name field, which kind of account, then the command
/// that makes it running in a terminal inside the popup, then what came of it.
pub(super) fn draw_add_account(frame: &mut Frame, area: Rect, app: &mut App, layout: &mut Layout) {
    let esc = KeyEvent::from(KeyCode::Esc);
    let flow = &mut app.add_account;

    if let Some(outcome) = &flow.outcome
        && flow.pane.is_none()
    {
        let mut lines = match outcome {
            Ok(name) => vec![
                Line::from(vec![
                    Span::styled(" ✓ Saved ", Style::default().fg(theme::colors().cost_low)),
                    Span::styled(name.clone(), theme::value()),
                    Span::styled(". Its limits appear in the panel below.", theme::dim()),
                ]),
                Line::from(Span::styled(
                    " Start an agent on it with p in the launcher, or from a shell:",
                    theme::dim(),
                )),
                Line::from(Span::styled(
                    format!("   cctop as {name} claude"),
                    theme::value(),
                )),
            ],
            Err(why) => vec![Line::from(Span::styled(
                format!(" {why}"),
                Style::default().fg(theme::colors().cost_mid),
            ))],
        };
        let hint = " [Enter] done";
        lines.extend([
            Line::default(),
            Line::from(Span::styled(hint, theme::dim())),
        ]);
        let row = lines.len() as u16 - 1;
        let (outer, inner) = modal(frame, area, "Add a Claude account", lines, 72);
        confirm_chips(
            layout,
            outer,
            inner,
            row,
            hint,
            &[("[Enter]", KeyEvent::from(KeyCode::Enter))],
        );
        return;
    }

    if flow.named && flow.pane.is_none() {
        let hint = " [f] full login   [t] token   [Esc] cancel";
        let key = |label: &'static str| Span::styled(label, theme::value());
        let lines = vec![
            Line::from(vec![
                key(" [f] full login  "),
                Span::styled(
                    format!("its own ~/.claude-{}, everything works", flow.name),
                    theme::dim(),
                ),
            ]),
            Line::from(vec![
                key(" [t] token       "),
                Span::styled("shares ~/.claude history, but no Remote", theme::dim()),
            ]),
            Line::from(Span::styled(
                "                  Control or claude.ai connectors",
                theme::dim(),
            )),
            Line::default(),
            Line::from(Span::styled(hint, theme::dim())),
        ];
        let row = lines.len() as u16 - 1;
        let (outer, inner) = modal(
            frame,
            area,
            &format!("Add Claude account {}", flow.name),
            lines,
            64,
        );
        confirm_chips(
            layout,
            outer,
            inner,
            row,
            hint,
            &[
                ("[f]", KeyEvent::from(KeyCode::Char('f'))),
                ("[t]", KeyEvent::from(KeyCode::Char('t'))),
                ("[Esc]", esc),
            ],
        );
        return;
    }

    let Some(pane) = flow.pane.as_mut() else {
        let hint = " [Enter] next   [Esc] cancel";
        let lines = vec![
            Line::from(Span::styled(" What is it called?", theme::dim())),
            input_line(&flow.name),
            Line::default(),
            Line::from(Span::styled(hint, theme::dim())),
        ];
        let row = lines.len() as u16 - 1;
        let (outer, inner) = modal(frame, area, "Add a Claude account", lines, 52);
        confirm_chips(
            layout,
            outer,
            inner,
            row,
            hint,
            &[("[Enter]", KeyEvent::from(KeyCode::Enter)), ("[Esc]", esc)],
        );
        return;
    };

    // Wide enough for the token on one line where the screen allows: it is
    // read back off this terminal, and unwrapped is the easy case.
    let rect = centered(area, 116, 30);
    frame.render_widget(Clear, rect);
    let hint = match flow.link {
        Some(_) => " [Ctrl+O] or click the link: copy it   [Esc] cancel ",
        None => " [Esc] cancel ",
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border_hi))
        .style(theme::canvas())
        .title(Span::styled(
            format!(
                " Add Claude account {} — {} ",
                flow.name,
                match flow.kind {
                    Some(AccountKind::Login) => "claude auth login",
                    _ => "claude setup-token",
                }
            ),
            theme::title(),
        ))
        .title_bottom(Span::styled(hint, theme::title()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let [note, term] = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(2),
        ratatui::layout::Constraint::Min(1),
    ])
    .areas(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    " Approve it in the browser signed in as {}. {}",
                    flow.name,
                    match flow.kind {
                        Some(AccountKind::Login) => "The account is ready",
                        _ => "The token is saved",
                    }
                ),
                theme::dim(),
            )),
            Line::from(Span::styled(
                match flow.kind {
                    Some(AccountKind::Login) => " the moment the login finishes.",
                    _ => " the moment it is printed — nothing to copy.",
                },
                theme::dim(),
            )),
        ]),
        note,
    );
    pane.view.resize(term.width, term.height);
    let (cols, rows) = pane.view.size;
    let screen = Rect {
        width: cols.min(term.width),
        height: rows.min(term.height),
        ..term
    };
    frame.render_widget(
        tui_term::widget::PseudoTerminal::new(pane.view.parser.screen()),
        screen,
    );
    // Every row the link is wrapped over is one target. The terminal's own
    // link detection sees only the row under the pointer, so a click there
    // opened the first line of the URL — a sign-in page that cannot work.
    //
    // Two targets, in fact, for two kinds of click. cctop holds the mouse, so a
    // plain click is cctop's and copies the link. The cells are also an OSC 8
    // hyperlink to the whole URL, for the click a terminal keeps for itself —
    // Ctrl or Shift and a click, depending on the terminal — which now opens
    // the page it means from any row of it.
    if let Some(link) = &flow.link {
        for (y, columns) in link_cells(pane.view.parser.screen(), link) {
            if y < screen.height {
                hyperlink::link(
                    frame.buffer_mut(),
                    screen.y + y,
                    screen.x + columns.start..screen.x + columns.end.min(screen.width),
                    link,
                );
                layout.key_hits.push((
                    screen.y + y,
                    screen.x,
                    screen.x + screen.width,
                    KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
                ));
            }
        }
    }
    // The chips are on the bottom border, one row below the terminal.
    layout.modal_rect = Some(rect);
    let bottom = rect.y + rect.height - 1;
    for (chip, key) in [
        (
            "[Ctrl+O] or click the link: copy it",
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        ),
        ("[Esc] cancel", esc),
    ] {
        if let Some(at) = hint.find(chip) {
            let x = rect.x + 1 + hint[..at].chars().count() as u16;
            layout
                .key_hits
                .push((bottom, x, x + chip.chars().count() as u16, key));
        }
    }
}

// ---------------------------------------------------------------------------
// Clipboard
// ---------------------------------------------------------------------------

/// `optimize` or `compare`, drawn over the table.
///
/// The text arrives already laid out from [`crate::insight`], so this only has
/// to frame and scroll it — the alternative was a second implementation of the
/// same tables that could drift from the one the command prints.
pub(super) fn draw_insight(frame: &mut Frame, area: Rect, app: &App) {
    let title = match app.insight_kind {
        "optimize" => " optimize — what was spent and not got back ",
        _ => " compare — models on the work you gave them ",
    };

    let body: Vec<Line> = match &app.insight {
        // The spinner is the difference between this wait and a hang: the
        // report re-parses every transcript, and how long that takes depends
        // on the machine.
        None => vec![
            Line::default(),
            Line::from(vec![
                Span::styled(format!("  {}", share::spinner_frame()), theme::title()),
                Span::styled(
                    "  Reading every transcript. This is the slow one.",
                    theme::value(),
                ),
            ]),
        ],
        Some(text) => text
            .lines()
            .map(|l| Line::from(Span::styled(l.to_string(), theme::value())))
            .collect(),
    };

    let width = area.width.saturating_sub(4).min(96);
    let height = area.height.saturating_sub(4);
    let box_area = centered(area, width, height);
    frame.render_widget(Clear, box_area);

    let footer = match app.insight.is_some() {
        true => " ↑↓ scroll · o optimize · c compare · esc close ",
        false => " esc close ",
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border_hi))
        .style(theme::canvas())
        .title(Span::styled(title, theme::title()))
        .title_bottom(Span::styled(footer, theme::dim()));

    // Clamped so scrolling cannot run off the end and leave an empty frame with
    // no way to tell it is still open.
    let visible = box_area.height.saturating_sub(2);
    let max_scroll = (body.len() as u16).saturating_sub(visible);
    let scroll = app.insight_scroll.min(max_scroll);
    let total = body.len();
    frame.render_widget(
        Paragraph::new(body).block(block).scroll((scroll, 0)),
        box_area,
    );
    super::scrollbar::on_border(frame, box_area, total, visible as usize, scroll as usize);
}

/// The selected session's conversation, read-only — the terminal half of what
/// the report page shows a browser, over the same `chat::build`.
///
/// `back` is scrolled from the end rather than the top: the document the user
/// is reading can grow while they read it, and a position counted from the
/// start would shift under them every time it does.
pub(super) fn draw_conversation(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(view) = &mut app.chat else {
        return;
    };

    let title = match &view.session.remote {
        // The host is worth the title room: a remote conversation is otherwise
        // indistinguishable from a local one, and knowing which machine it was
        // read off is the whole difference.
        Some(remote) => format!(
            " {} — conversation · on {} ",
            view.session.display_label(),
            remote.host
        ),
        None => format!(" {} — conversation ", view.session.display_label()),
    };

    let width = area.width.saturating_sub(4).min(110);
    let height = area.height.saturating_sub(4);
    let box_area = centered(area, width, height);
    frame.render_widget(Clear, box_area);

    // Borders, plus a space of padding either side, is what the wrap below has
    // to agree with.
    let text_width = (box_area.width as usize).saturating_sub(4).max(1);
    let mut laid = Chat::default();
    let body: Vec<Line> = match (&view.conversation, &view.error) {
        (None, None) => vec![
            Line::default(),
            Line::from(vec![
                Span::styled(format!("  {}", share::spinner_frame()), theme::title()),
                match view.host.is_some() {
                    true => Span::styled("  Reading it over ssh…", theme::value()),
                    false => Span::styled("  Reading the transcript…", theme::value()),
                },
            ]),
        ],
        (None, Some(why)) => vec![
            Line::default(),
            Line::from(Span::styled(format!("  {why}"), theme::failed())),
        ],
        (Some(conv), _) => {
            laid = chat_lines(&view.session, conv, text_width, view.raw);
            std::mem::take(&mut laid.lines)
        }
    };

    let shown = match view.raw {
        true => "m rendered",
        false => "m source",
    };
    let footer = match &view.conversation {
        _ if view.fetching => format!(
            " ↑↓ scroll · [ ] turn · {shown} · {} loading earlier… · esc close ",
            share::spinner_frame()
        ),
        Some(c) if c.earlier > 0 => format!(
            " ↑↓ scroll · [ ] turn · {shown} · u load {} earlier · esc close ",
            c.earlier
        ),
        _ => format!(" ↑↓ scroll · [ ] turn · {shown} · esc close "),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border_hi))
        .style(theme::canvas())
        .title(Span::styled(title, theme::title()))
        .title_bottom(Span::styled(footer, theme::dim()));

    let visible = box_area.height.saturating_sub(2) as usize;
    // The only place the wrapped height is known, so the furthest-back offset
    // is written here for the key handler to clamp against.
    view.max_back = (body.len().saturating_sub(visible)).min(u16::MAX as usize) as u16;
    let total = body.len();
    let top = total.saturating_sub(visible + view.back as usize);
    // Each turn's header, as the `back` that brings it to the top — clamped,
    // so the last few turns, which cannot reach the top, all read as the end.
    view.turn_backs = laid
        .turns
        .iter()
        .map(|&start| {
            (total.saturating_sub(visible).saturating_sub(start) as u16).min(view.max_back)
        })
        .collect();
    frame.render_widget(
        Paragraph::new(body).block(block).scroll((top as u16, 0)),
        box_area,
    );
    super::scrollbar::on_border(frame, box_area, total, visible, top);
    // Links are laid over the cells the paragraph has just drawn, so they have
    // to wait for it; one scrolled out of the box is simply not on screen.
    let inner = box_area.inner(ratatui::layout::Margin::new(1, 1));
    for link in &laid.links {
        let Some(row) = link.line.checked_sub(top).filter(|r| *r < visible) else {
            continue;
        };
        let start = inner.x + link.columns.start;
        let end = (inner.x + link.columns.end).min(inner.right());
        hyperlink::link(
            frame.buffer_mut(),
            inner.y + row as u16,
            start..end,
            &link.url,
        );
    }
}

/// A conversation laid out: its rows, where its links landed, and the row each
/// turn starts on.
#[derive(Default)]
struct Chat {
    lines: Vec<Line<'static>>,
    links: Vec<super::markdown::Link>,
    turns: Vec<usize>,
}

/// A conversation laid out as styled lines, wrapped to `width`.
///
/// Turns read like the report page's: a small header naming the speaker, the
/// text at full width, and each tool call underneath it dimmed — a tool's work
/// is context for the text, not the text itself.
///
/// The agent's own words are rendered as the markdown they were written in,
/// unless `raw` asks for the source. What the user typed is not: a prompt is
/// rarely markdown on purpose, and an asterisk in it should stay one. A tool's
/// result keeps the colours its program printed it in (see [`super::ansi`]).
fn chat_lines(
    session: &Session,
    conv: &crate::serve::chat::Conversation,
    width: usize,
    raw: bool,
) -> Chat {
    let assistant = session.surface.label(session.provider).to_string();
    let now = chrono::Utc::now();
    let mut chat = Chat::default();
    let out = &mut chat.lines;

    if let Some(note) = &conv.note {
        for line in super::panels::wrap(note, width) {
            out.push(Line::styled(line, theme::dim()));
        }
        out.push(Line::default());
    }
    for turn in &conv.turns {
        chat.turns.push(out.len());
        if turn.kind.as_ref() == "compaction" {
            // A seam, not something said — drawn as a rule so the eye reads it
            // as one rather than hunting for a speaker.
            out.push(Line::styled("  ── compacted ──", theme::dim()));
            out.push(Line::default());
            continue;
        }
        let (who, style) = match turn.role.as_ref() {
            "user" => ("you".to_string(), theme::title()),
            "assistant" => (assistant.clone(), theme::title()),
            _ => ("system".to_string(), theme::dim()),
        };
        let when = crate::util::relative_age(&turn.ts, &now);
        let text_style = match turn.kind.as_ref() {
            // Reasoning is the agent thinking out loud: kept dim so the
            // transcript's own hierarchy survives the small screen.
            "reasoning" => theme::dim(),
            _ => theme::value(),
        };
        out.push(Line::from(vec![
            Span::styled(format!("  {who}"), style),
            Span::styled(format!("  {when}"), theme::dim()),
        ]));
        match turn.role.as_ref() == "assistant" && !raw {
            true => {
                // Rendered text sets its own weight, so the base is the ink
                // without the bold — or `**this**` would have nothing to stand
                // out from.
                let base = text_style.remove_modifier(Modifier::BOLD);
                let md = super::markdown::render(&turn.text, width, base);
                let offset = out.len();
                chat.links.extend(md.links.into_iter().map(|mut link| {
                    link.line += offset;
                    link.columns = link.columns.start + 2..link.columns.end + 2;
                    link
                }));
                out.extend(md.lines.into_iter().map(|mut line| {
                    line.spans.insert(0, Span::raw("  "));
                    line
                }));
            }
            false => {
                for line in super::panels::wrap(&turn.text, width) {
                    out.push(Line::styled(format!("  {line}"), text_style));
                }
            }
        }
        for tool in &turn.tools {
            let (mark, style) = match tool.failed {
                true => ("✗", theme::failed()),
                false => ("⚙", theme::dim()),
            };
            let counts = match (tool.added, tool.removed) {
                (0, 0) => String::new(),
                (a, r) => format!("  +{a} −{r}"),
            };
            out.push(Line::from(vec![
                Span::styled(format!("    {mark} {}", tool.name), style),
                Span::styled(
                    format!("  {}", super::ansi::strip(&tool.detail)),
                    theme::dim(),
                ),
                Span::styled(counts, theme::dim()),
            ]));
            // The full argument only exists where it says more than the
            // one-liner did — a long command, a whole file body.
            if let Some(full) = &tool.full {
                out.extend(super::ansi::wrapped(full, theme::dim(), width, "      "));
            }
            if let Some(result) = &tool.result {
                out.extend(super::ansi::wrapped(result, theme::dim(), width, "      "));
            }
            for line in &tool.diff {
                let style = match line.starts_with('+') {
                    true => theme::value(),
                    false => theme::dim(),
                };
                out.push(Line::styled(
                    format!("      {}", super::ansi::strip(line)),
                    style,
                ));
            }
        }
        out.push(Line::default());
    }
    chat
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sign-in link wrapped over three rows is three click targets, and the
    /// text after it is none — a click there is not a click on the link.
    #[test]
    fn every_row_of_a_wrapped_link_is_clickable() {
        let mut parser = vt100::Parser::new(6, 20, 0);
        let link = "https://claude.com/cai/oauth/authorize?code=true";
        parser.process(format!("Sign in:\r\n{link}\r\n\r\nPaste code:").as_bytes());
        assert_eq!(
            link_cells(parser.screen(), link),
            vec![(1, 0..20), (2, 0..20), (3, 0..8)]
        );
    }

    /// The prompt in front of the URL is not part of the link, and neither is
    /// a row that only starts like a piece of it.
    #[test]
    fn the_link_is_its_own_columns_and_not_the_row() {
        let mut parser = vt100::Parser::new(6, 20, 0);
        let link = "https://a.io/abc";
        // "abc later" starts with a piece of it, but carries on past one.
        parser.process(format!("Go: {link}\r\nabc later").as_bytes());
        assert_eq!(link_cells(parser.screen(), link), vec![(0, 4..20)]);
    }

    /// The popup's sign-in link, wrapped over rows by the terminal that printed
    /// it, is one hyperlink to the whole URL on every one of them — and the
    /// screen reads, cell for cell, as it did before the link was laid over it.
    #[test]
    fn a_wrapped_sign_in_link_opens_the_whole_url_from_every_row() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let link = format!(
            "https://claude.com/cai/oauth/authorize?code=true&client_id={}&state={}",
            "c".repeat(60),
            "s".repeat(70)
        );
        let draw = |with_link: bool| {
            let mut app = crate::ui::tests::test_app();
            let mut pane = crate::ui::tabs::Pane::for_test("claude");
            pane.view.parser.process(
                format!(
                    "Browser didn't open? Use the url below:\r\n\r\n{link}\r\n\r\nPaste code > "
                )
                .as_bytes(),
            );
            app.add_account = crate::ui::AddAccount {
                name: "work".into(),
                kind: Some(AccountKind::Token),
                named: true,
                pane: Some(pane),
                link: with_link.then(|| link.clone()),
                outcome: None,
            };
            let mut layout = Layout::default();
            let mut terminal = Terminal::new(TestBackend::new(140, 40)).expect("backend");
            terminal
                .draw(|frame| draw_add_account(frame, frame.area(), &mut app, &mut layout))
                .expect("draw");
            (terminal.backend().buffer().clone(), layout)
        };
        let (linked, layout) = draw(true);
        let (plain, _) = draw(false);

        // Every cell that opens a link opens this one, and read in order the
        // linked cells spell it exactly: nothing of it left plain, nothing
        // around it swept in.
        let mut rows = Vec::new();
        let mut spelled = String::new();
        let mut skip = 0u16;
        for (i, cell) in linked.content().iter().enumerate() {
            let (x, y) = linked.pos_of(i);
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if let Some(target) = hyperlink::target_of(cell.symbol()) {
                assert_eq!(target, link, "a cell links somewhere else");
                let ratatui::buffer::CellDiffOption::ForcedWidth(width) = cell.diff_option else {
                    panic!("a link cell at ({x}, {y}) with no forced width");
                };
                let label = cell
                    .symbol()
                    .split_once("\x1b\\")
                    .and_then(|(_, rest)| rest.strip_suffix("\x1b]8;;\x1b\\"))
                    .expect("a closed link");
                assert_eq!(label.chars().count(), width.get() as usize);
                spelled.push_str(label);
                skip = width.get() - 1;
                if rows.last() != Some(&y) {
                    rows.push(y);
                }
            }
        }
        assert_eq!(spelled, link);
        assert!(rows.len() >= 3, "the URL was meant to wrap: {rows:?}");

        // What is on screen has not moved a column: the text is the same, row
        // for row, as the frame drawn without the link — the bottom border
        // aside, whose hint only offers the copy when there is a link.
        let bottom = rows[rows.len() - 1] + 4;
        let text = |buf: &ratatui::buffer::Buffer| -> Vec<String> {
            hyperlink::visible(buf)
                .lines()
                .take(bottom as usize)
                .map(str::to_owned)
                .collect()
        };
        assert_eq!(text(&linked), text(&plain));

        // And a plain click on any of those rows is still cctop's, and copies.
        let copy = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL);
        for y in rows {
            assert!(
                layout
                    .key_hits
                    .iter()
                    .any(|(row, _, _, key)| *row == y && *key == copy),
                "row {y} is not a click target"
            );
        }
    }

    #[test]
    fn a_help_key_longer_than_its_column_keeps_a_gap() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::ui::tests::test_app();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("backend");
        terminal
            .draw(|frame| draw_help(frame, frame.area(), &mut app))
            .expect("draw");
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Home / End Jump"), "{text}");
    }

    /// The help is taller than a small terminal, and the bar on its border is
    /// what says so; on a screen it fits, there is no bar to misread.
    #[test]
    fn the_help_has_a_scrollbar_only_when_it_overflows() {
        use crate::ui::scrollbar::tests::thumb_cells;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::ui::tests::test_app();
        let mut short = Terminal::new(TestBackend::new(100, 20)).expect("backend");
        short
            .draw(|frame| draw_help(frame, frame.area(), &mut app))
            .expect("draw");
        assert!(app.help_max_scroll > 0, "the help fits in 20 rows now");
        assert!(thumb_cells(short.backend().buffer()) > 0, "no scrollbar");

        let mut tall = Terminal::new(TestBackend::new(100, 200)).expect("backend");
        tall.draw(|frame| draw_help(frame, frame.area(), &mut app))
            .expect("draw");
        assert_eq!(app.help_max_scroll, 0);
        assert_eq!(thumb_cells(tall.backend().buffer()), 0);
    }

    fn help_text(app: &mut App) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(100, 200)).expect("backend");
        terminal
            .draw(|frame| draw_help(frame, frame.area(), app))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `/` in the help narrows it to the entries that mention what is typed,
    /// under their headings, with the query on the border; Enter keeps it,
    /// and Esc takes the filter off before it takes the page away.
    #[test]
    fn slash_filters_the_help() {
        use crate::ui::tests::key;
        let mut app = crate::ui::tests::test_app();
        app.mode = crate::ui::Mode::Help;
        app.on_key(key(KeyCode::Char('/')));
        assert!(app.help_typing);
        // `j` and `k` are letters of the query now, not the scroll.
        for c in "ctrl+k".chars() {
            app.on_key(key(KeyCode::Char(c)));
        }
        assert_eq!(app.help_filter, "ctrl+k");
        let text = help_text(&mut app);
        assert!(text.contains("Terminate it"), "{text}");
        assert!(text.contains("Acting on the selected session"), "{text}");
        assert!(text.contains("/ ctrl+k"), "the query is not shown: {text}");
        assert!(
            !text.contains("Move between sessions"),
            "not narrowed: {text}"
        );

        // A match on an entry's second line keeps its first, key and all.
        app.help_filter = "leaving the ones mid-turn".into();
        let text = help_text(&mut app);
        assert!(text.contains("Ctrl+R"), "{text}");
        // A heading keeps its whole section.
        app.help_filter = "several at once".into();
        let text = help_text(&mut app);
        assert!(text.contains("Clear all marks"), "{text}");
        assert!(text.contains("skipping any that are working"), "{text}");
        // And nothing matching says so.
        app.help_filter = "zzzz".into();
        let text = help_text(&mut app);
        assert!(
            text.contains("Nothing in the help mentions “zzzz”"),
            "{text}"
        );

        // Enter keeps the filter and gives the letters back to the page.
        app.help_filter = "tab".into();
        app.on_key(key(KeyCode::Enter));
        assert!(!app.help_typing);
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.mode, crate::ui::Mode::Help, "j closed the help");
        assert_eq!(app.help_filter, "tab");
        // Esc: the filter, then the page.
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, crate::ui::Mode::Help);
        assert!(app.help_filter.is_empty());
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, crate::ui::Mode::List);

        // Esc while typing clears it and stays, too.
        app.mode = crate::ui::Mode::Help;
        app.on_key(key(KeyCode::Char('/')));
        app.on_key(key(KeyCode::Char('x')));
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, crate::ui::Mode::Help);
        assert!(!app.help_typing && app.help_filter.is_empty());
    }

    /// The help names the keys as `[keys]` has them, not as they ship.
    #[test]
    fn the_help_shows_the_keys_as_bound() {
        // A row as `item` lays it out: the key padded to its column.
        let row = |k: &str, d: &str| format!("  {k:<16} {d}");
        let mut app = crate::ui::tests::test_app();
        let text = help_text(&mut app);
        // And which build is saying so, on its top border.
        let top = text.lines().find(|l| l.contains("╭ Help")).unwrap_or("");
        assert!(top.contains(&build_label()), "{top}");
        assert!(build_label().starts_with("cctop "), "{}", build_label());
        assert!(text.contains(&row("Ctrl+K", "Terminate it")), "{text}");
        assert!(text.contains("Shift+Home / End"), "{text}");
        assert!(text.contains("↑/k  ↓/j"), "{text}");

        let s = crate::settings::Settings::parse(
            "[keys]\nterminate = \"ctrl+t\"\nhelp = \"H\"\nfollow = \"b\"\n",
        );
        app.keymap = crate::settings::Keymap::build(&s).0;
        let text = help_text(&mut app);
        assert!(text.contains(&row("Ctrl+T", "Terminate it")), "{text}");
        assert!(text.contains("H  F1"), "{text}");
        assert!(text.contains(&row("b", "Follow mode")), "{text}");
        // `b` was the bell's, and nothing gave the bell another key.
        assert!(
            text.contains(&row("unbound", "Jump to the session that rang")),
            "{text}"
        );
    }

    /// The whole page with no query, and an unmatched heading hides nothing
    /// that matched under it.
    #[test]
    fn filter_help_keeps_entries_under_their_headings() {
        let line = |t: &str| Line::from(t.to_string());
        let rows = vec![
            (HelpRow::Section, line("Alpha")),
            (HelpRow::Entry, line("a  first")),
            (HelpRow::More, line("   and more of it")),
            (HelpRow::Entry, line("b  second")),
            (HelpRow::Gap, Line::default()),
            (HelpRow::Section, line("Beta")),
            (HelpRow::More, line("   a note with no key")),
            (HelpRow::Entry, line("c  third, more")),
        ];
        let texts = |lines: Vec<Line<'static>>| -> Vec<String> {
            lines.iter().map(|l| l.to_string()).collect()
        };
        assert_eq!(filter_help(rows.clone(), "").len(), rows.len());
        assert_eq!(
            texts(filter_help(rows.clone(), "MORE")),
            vec![
                "Alpha",
                "a  first",
                "   and more of it",
                "",
                "Beta",
                "c  third, more"
            ]
        );
        assert_eq!(
            texts(filter_help(rows.clone(), "beta")),
            vec!["Beta", "   a note with no key", "c  third, more"]
        );
        assert!(filter_help(rows, "nowhere").is_empty());
    }

    /// A report longer than its box gets a bar that follows the scroll: at the
    /// top of the border first, at the bottom once scrolled to the end.
    #[test]
    fn a_long_report_has_a_scrollbar_that_follows_the_scroll() {
        use crate::ui::scrollbar::{THUMB, tests::thumb_cells};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::ui::tests::test_app();
        app.mode = crate::ui::Mode::Insight;
        let draw = |app: &App| {
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("backend");
            terminal
                .draw(|frame| draw_insight(frame, frame.area(), app))
                .expect("draw");
            terminal.backend().buffer().clone()
        };

        app.insight = Some("a short report\n".repeat(5));
        assert_eq!(thumb_cells(&draw(&app)), 0, "a report that fits has a bar");

        app.insight = Some("line\n".repeat(200));
        let buf = draw(&app);
        assert!(
            thumb_cells(&buf) > 0,
            "no scrollbar on an overflowing report"
        );
        // The box is centred with two columns of margin: its right border is
        // column 77, and its first inner row is 3.
        assert_eq!(buf[(77, 3)].symbol(), THUMB);

        app.insight_scroll = u16::MAX;
        let buf = draw(&app);
        assert_eq!(buf[(77, 3)].symbol(), "│");
        assert_eq!(buf[(77, 20)].symbol(), THUMB);
    }

    /// The picker row draws every swatch, brackets the pick, and spells its
    /// name — under NO_COLOR the name is the only thing there is to read.
    #[test]
    fn the_colour_row_brackets_the_pick_and_names_it() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::ui::tests::test_app();
        app.rename_was = "claude-4".to_string();
        app.rename_color = Some(theme::Hue::Cyan);

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("backend");
        let mut layout = crate::ui::render::Layout::default();
        terminal
            .draw(|frame| draw_rename_tab(frame, frame.area(), &app, &mut layout))
            .expect("draw");
        let buf = terminal.backend().buffer();
        let text = buf
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(
            text.contains("cyan"),
            "the pick's name is not spelled: {text}"
        );
        assert!(text.contains("[●]"), "the pick is not bracketed: {text}");
        assert!(text.contains('○'), "the none stop is missing: {text}");
        // The picked swatch carries the hue's colour; under the default
        // palette that is an indexed colour, not the terminal's default ink.
        let cyan = buf
            .content()
            .iter()
            .find(|cell| cell.symbol() == "●" && cell.fg == theme::Hue::Cyan.color());
        assert!(cyan.is_some(), "no swatch wears the picked hue");
    }

    /// While the worker is still reading transcripts the overlay keeps moving:
    /// the spinner frame is the difference between a wait and a hang.
    #[test]
    fn a_report_still_loading_turns_a_spinner() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::ui::tests::test_app();
        app.mode = crate::ui::Mode::Insight;
        app.insight = None;

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("backend");
        terminal
            .draw(|frame| draw_insight(frame, frame.area(), &app))
            .expect("draw");
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(
            text.contains("Reading every transcript"),
            "the wait is not described: {text}"
        );
        assert!(
            text.chars().any(|c| "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏".contains(c)),
            "no spinner frame on screen: {text}"
        );
    }

    /// The switcher draws the whole bar — dashboard first — names the pick
    /// with `›`, and says beside each tab what it wants, so the row you land
    /// on is the row you read.
    #[test]
    fn the_switcher_lists_the_bar_with_its_states() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let shared = |name: &str, signal: Option<crate::hook::Signal>| {
            tabs::Tab::shared(&crate::rmux::Running {
                name: format!("cctop-{name}"),
                pid: None,
                cwd: None,
                attached: false,
                activity: None,
                label: Some(name.to_string()),
                profile: None,
                order: None,
                state: signal.map(|signal| crate::rmux::State {
                    signal,
                    at: crate::rmux::now_secs(),
                }),
                color: None,
            })
        };

        let mut app = crate::ui::tests::test_app();
        app.tabs = vec![
            shared("claude", None),
            shared("blocked", Some(crate::hook::Signal::NeedsInput)),
        ];
        app.mode = crate::ui::Mode::SwitchTab;

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("backend");
        let mut layout = crate::ui::render::Layout::default();
        terminal
            .draw(|frame| draw_switch_tab(frame, frame.area(), &app, &mut layout))
            .expect("draw");
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(text.contains("Go to tab"), "no title: {text}");
        assert!(
            text.contains("Dashboard"),
            "the dashboard is missing: {text}"
        );
        assert!(text.contains("claude"), "a tab is missing: {text}");
        assert!(text.contains("needs you"), "the ask is not marked: {text}");
        assert!(text.contains("›"), "no cursor on the pick: {text}");

        // A filter that matches nothing says so rather than listing air.
        app.switch_filter = "zzz".into();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("backend");
        let mut layout = crate::ui::render::Layout::default();
        terminal
            .draw(|frame| draw_switch_tab(frame, frame.area(), &app, &mut layout))
            .expect("draw");
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("No tab by that name"), "silence: {text}");

        // Under a state filter, the title and the empty line both say which.
        app.switch_state = crate::ui::panes::SwitchState::NeedsYou;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("backend");
        terminal
            .draw(|frame| draw_switch_tab(frame, frame.area(), &app, &mut layout))
            .expect("draw");
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Go to tab · needs you"), "{text}");
        assert!(text.contains("No tab by that name needs you"), "{text}");
    }

    /// The conversation view with one reply in markdown and one coloured tool
    /// result, open and read in by the worker.
    fn chat_app(text: &str, result: &str) -> App {
        let mut app = crate::ui::tests::test_app();
        app.sessions = vec![crate::ui::tests::session("a", true, "/repo")];
        app.refilter();
        app.selected = 0;
        app.open_conversation();
        let key = app.chat.as_ref().expect("the view opened").session.key();
        let turn = |seq: usize, role: &'static str, text: &str| crate::serve::chat::Turn {
            seq,
            role: role.into(),
            kind: "message".into(),
            ts: String::new(),
            text: text.into(),
            clipped: false,
            tools: Vec::new(),
        };
        let mut reply = turn(1, "assistant", text);
        reply.tools.push(crate::serve::chat::ToolUse {
            name: "Bash".into(),
            detail: "cargo test".into(),
            result: Some(result.into()),
            ..Default::default()
        });
        let conv = crate::serve::chat::Conversation {
            supported: true,
            turns: vec![turn(0, "user", "**keep** my stars"), reply],
            earlier: 0,
            note: None,
        };
        app.got_chat(key, None, Ok(Box::new(conv)));
        app
    }

    fn draw_chat(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("backend");
        terminal
            .draw(|frame| draw_conversation(frame, frame.area(), app))
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    /// A reply reads as rendered markdown with its link clickable, a user's
    /// prompt keeps its characters, and a coloured result shows its colour and
    /// none of its escape codes.
    #[test]
    fn the_conversation_renders_replies_and_colours_results() {
        let mut app = chat_app(
            "## Done\n\nRan **all** of it, see [the log](https://example.com/log).",
            "\x1b[1m\x1b[32mtest result: ok\x1b[0m. 3 passed\x1b[2K",
        );
        let buf = draw_chat(&mut app, 100, 30);
        let screen = hyperlink::visible(&buf);
        assert!(screen.contains("Ran all of it, see the log."), "{screen}");
        assert!(
            !screen.contains("**all**") && !screen.contains("## "),
            "{screen}"
        );
        assert!(
            screen.contains("**keep** my stars"),
            "the prompt was rendered: {screen}"
        );
        assert!(screen.contains("test result: ok. 3 passed"), "{screen}");
        assert!(
            !screen.contains("[32m") && !screen.contains("[2K"),
            "{screen}"
        );

        let green = buf
            .content()
            .iter()
            .find(|c| c.symbol() == "t" && c.fg == ratatui::style::Color::Green);
        assert!(green.is_some(), "the result lost its colour");
        let targets: Vec<&str> = buf
            .content()
            .iter()
            .filter_map(|c| hyperlink::target_of(c.symbol()))
            .collect();
        assert_eq!(targets, ["https://example.com/log"]);

        // `m` shows the source instead, markers and all.
        app.on_key(crate::ui::tests::key(KeyCode::Char('m')));
        let screen = hyperlink::visible(&draw_chat(&mut app, 100, 30));
        assert!(screen.contains("Ran **all** of it"), "{screen}");
    }

    /// `[` brings the previous turn's header to the top and `]` walks back
    /// towards the end, which is where the view opened.
    #[test]
    fn brackets_step_through_the_turns() {
        let long = (0..40).map(|i| format!("line {i}\n\n")).collect::<String>();
        let mut app = chat_app(&long, "ok");
        draw_chat(&mut app, 80, 20);
        let view = app.chat.as_ref().expect("open");
        assert_eq!(view.back, 0);
        let backs = view.turn_backs.clone();
        assert_eq!(backs.len(), 2);

        app.on_key(crate::ui::tests::key(KeyCode::Char('[')));
        assert_eq!(app.chat.as_ref().map(|v| v.back), Some(backs[1]));
        let top = hyperlink::visible(&draw_chat(&mut app, 80, 20));
        // The row under the box's top border is the reply's own header.
        let under_border = top.lines().skip_while(|l| !l.contains('╭')).nth(1);
        assert!(under_border.is_some_and(|l| l.contains("Claude")), "{top}");

        app.on_key(crate::ui::tests::key(KeyCode::Char('[')));
        assert_eq!(app.chat.as_ref().map(|v| v.back), Some(backs[0]));
        app.on_key(crate::ui::tests::key(KeyCode::Char(']')));
        assert_eq!(app.chat.as_ref().map(|v| v.back), Some(backs[1]));
        app.on_key(crate::ui::tests::key(KeyCode::Char(']')));
        assert_eq!(app.chat.as_ref().map(|v| v.back), Some(0));
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
