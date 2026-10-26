//! The help screen: every key, as bound, with a filter to find one.

use super::*;

pub(crate) fn draw_help(frame: &mut Frame, area: Rect, app: &mut App) {
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
            "The settings tab: every setting, view choice and keybind",
        ),
        // Named here rather than only on the page, because it is the one
        // setting somebody looks for in the wrong place: it used to have an
        // environment variable, and the variable is gone, so "where do I set
        // the theme" is answered by the row and by this.
        item("theme", "auto / light / dark / mono, in config.toml"),
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
        item("{handoff}", "Hand its context to another agent or account"),
        item(
            "{conversation}",
            "Read its conversation (works on remote rows)",
        ),
        item(
            "  / n N  [ ]",
            "In it: search, next / previous hit, turn by turn",
        ),
        item("  ↵ t  m", "In it: this turn's tools / all tools, source"),
        item("  a", "In it: a subagent's own turns (Esc comes back)"),
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
        item("{machine}  M", "All machines / this one / each other one"),
        item("{tree}", "Tree view: group by repository and worktree"),
        item(
            "{worktree}",
            "Fork a git worktree and launch an agent in it",
        ),
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
            "New tab: an agent or a shell, here or on another machine, or one still running",
        ),
        item(
            "  →",
            "In it: the agent's model, account, permissions, worktree",
        ),
        item("Alt+v / Alt+s", "Split the tab right / down"),
        item("Alt+← / →", "Previous / next tab"),
        item("Alt+1 – 9", "Jump to a tab (1 is the dashboard)"),
        item("Alt+t", "Pick a tab from a list, typing to narrow it"),
        item("  Tab", "In it: all / needs you / working / idle"),
        item("Alt+b", "Jump to the next tab that needs you,"),
        item("", "then to one whose turn ended unseen (✓)"),
        item(
            "Alt+Shift+← / →",
            "Move this tab along the bar (or drag it with the mouse)",
        ),
        item("Right-click / Alt+r", "Rename or recolour a tab"),
        item("Alt+o", "Move focus to the next pane"),
        item("Alt+z", "Zoom the pane to fill the tab, or unzoom"),
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
        item("{wrapped}", "Your year with the agents, a card at a time"),
        item("{hooks}  F8", "Agent integration: what reports to cctop"),
        item(
            "{provider_status}",
            "Provider status: does an outage explain the errors",
        ),
        item("{refresh}  F5", "Refresh now"),
        gap(),
        section("Environment"),
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
pub(crate) fn build_label() -> String {
    let version = cctop_core::update::current_version();
    match cctop_core::update::current_commit() {
        "" => format!("cctop {version}"),
        commit => format!("cctop {version} ({commit})"),
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
fn bound_keys(template: &str, keymap: &cctop_core::settings::Keymap) -> String {
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
            .map(cctop_core::settings::key_label)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_help_key_longer_than_its_column_keeps_a_gap() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::tests::test_app();
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
        use crate::scrollbar::tests::thumb_cells;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::tests::test_app();
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
        use crate::tests::key;
        let mut app = crate::tests::test_app();
        app.mode = crate::Mode::Help;
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
        assert_eq!(app.mode, crate::Mode::Help, "j closed the help");
        assert_eq!(app.help_filter, "tab");
        // Esc: the filter, then the page.
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, crate::Mode::Help);
        assert!(app.help_filter.is_empty());
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, crate::Mode::List);

        // Esc while typing clears it and stays, too.
        app.mode = crate::Mode::Help;
        app.on_key(key(KeyCode::Char('/')));
        app.on_key(key(KeyCode::Char('x')));
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, crate::Mode::Help);
        assert!(!app.help_typing && app.help_filter.is_empty());
    }

    /// The help names the keys as `[keys]` has them, not as they ship.
    #[test]
    fn the_help_shows_the_keys_as_bound() {
        // A row as `item` lays it out: the key padded to its column.
        let row = |k: &str, d: &str| format!("  {k:<16} {d}");
        let mut app = crate::tests::test_app();
        let text = help_text(&mut app);
        // And which build is saying so, on its top border.
        let top = text.lines().find(|l| l.contains("╭ Help")).unwrap_or("");
        assert!(top.contains(&build_label()), "{top}");
        assert!(build_label().starts_with("cctop "), "{}", build_label());
        assert!(text.contains(&row("Ctrl+K", "Terminate it")), "{text}");
        assert!(text.contains("Shift+Home / End"), "{text}");
        assert!(text.contains("↑/k  ↓/j"), "{text}");

        let s = cctop_core::settings::Settings::parse(
            "[keys]\nterminate = \"ctrl+t\"\nhelp = \"H\"\nfollow = \"b\"\n",
        );
        app.keymap = cctop_core::settings::Keymap::build(&s).0;
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
}
