//! The small dialogs that pick or type one thing: the filter, the sort,
//! the row menu, the hooks, keys to send, a worktree, a tab's name and the
//! tab switcher.

use super::*;

pub(crate) fn draw_search(frame: &mut Frame, area: Rect, app: &App) {
    const WIDTH: u16 = 62;
    let text_w = WIDTH as usize - 4;

    let mut lines = vec![
        input_line(&app.search),
        Line::from(Span::styled(
            format!(
                " {} of {}",
                app.matched,
                util::plural(app.sessions.len(), "session")
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
        Span::styled(cctop_core::util::truncate(&text, text_w), style),
    ]));

    // The matching line from the selected session's transcript. Without it a
    // content hit is a row that matches for reasons nothing on screen explains.
    if let Some(snippet) = app.selected_snippet() {
        lines.push(Line::from(Span::styled(
            // Less the three-space indent: the paragraph wraps, and a snippet
            // spilling onto a second line resizes the modal as you type.
            format!("   {}", cctop_core::util::truncate(snippet, text_w - 3)),
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

pub(crate) fn draw_sortby(frame: &mut Frame, area: Rect, app: &App) {
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

pub(crate) fn draw_age_filter(frame: &mut Frame, area: Rect, app: &App) {
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
/// [`truncate`](cctop_core::util::truncate) keeps the head, which is the right
/// answer for a label and the wrong one for a cursor.
pub(super) fn tail(s: &str, width: usize) -> String {
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
pub(crate) fn draw_row_menu(
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
                    cctop_core::util::truncate(&trailing(item), MAX_REASON)
                ),
                theme::dim(),
            ),
        ]));
    }

    // The row this is about, on the border: it costs no content line, and a
    // menu floating over a table of seventy rows has to say which one it means.
    let subject = app
        .selected_session()
        .map(|s| cctop_core::util::truncate(s.display_label(), width.saturating_sub(4) as usize))
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

/// What the agents have been asked to report, and whether they are doing it.
///
/// The panel exists because every part of this is invisible otherwise: a hook
/// that is not installed, or is installed at a path that has moved, looks
/// exactly like an agent that has nothing to say.
pub(crate) fn draw_hooks(frame: &mut Frame, area: Rect, app: &App) {
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
            Span::styled(cctop_core::util::truncate(&text, text_w), style),
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
                    format!("   {:<24}", cctop_core::util::truncate(project, 24)),
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
                cctop_core::util::truncate(&dir.display().to_string(), text_w.saturating_sub(30))
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

pub(crate) fn draw_cost_filter(frame: &mut Frame, area: Rect, app: &App) {
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

pub(crate) fn draw_send_keys(frame: &mut Frame, area: Rect, app: &App) {
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

/// Naming the branch for `F`. The directory it will land in is shown as it is
/// typed, since that path is the thing the launcher opens next.
pub(crate) fn draw_new_worktree(frame: &mut Frame, area: Rect, app: &App) {
    let branch = app.worktree_input.trim();
    let into = app
        .worktree_repo
        .join(".claude/worktrees")
        .join(match branch {
            "" => "…",
            b => b,
        });
    let lines = vec![
        Line::from(Span::styled(
            format!(" From {}", app.worktree_base.display()),
            theme::value(),
        )),
        Line::from(Span::styled(
            " A new branch forks from here; an existing one is checked out.",
            theme::dim(),
        )),
        Line::default(),
        input_line(&app.worktree_input),
        Line::from(Span::styled(format!(" → {}", into.display()), theme::dim())),
        Line::default(),
        Line::from(Span::styled(
            " Enter create and pick an agent   Esc cancel",
            theme::dim(),
        )),
    ];
    modal(frame, area, "New worktree", lines, 64);
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
pub(crate) fn draw_rename_tab(
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
pub(crate) fn draw_switch_tab(
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
            Some(tabs::Attention::Done) => Some(("done ✓", theme::colors().accent)),
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
                util::truncate(&title, TITLE_W),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The switcher draws the whole bar — dashboard first — names the pick
    /// with `›`, and says beside each tab what it wants, so the row you land
    /// on is the row you read.
    #[test]
    fn the_switcher_lists_the_bar_with_its_states() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let shared = |name: &str, signal: Option<cctop_core::hook::Signal>| {
            tabs::Tab::for_agent(&cctop_core::rmux::Running {
                name: format!("cctop-{name}"),
                pid: None,
                cwd: None,
                attached: false,
                activity: None,
                label: Some(name.to_string()),
                profile: None,
                order: None,
                state: signal.map(|signal| cctop_core::rmux::State {
                    signal,
                    at: cctop_core::util::now_secs(),
                }),
                color: None,
                tab: None,
                pane: None,
                axis: None,
                window: None,
            })
        };

        let mut app = crate::tests::test_app();
        app.tabs = vec![
            shared("claude", None),
            shared("blocked", Some(cctop_core::hook::Signal::NeedsInput)),
        ];
        app.mode = crate::Mode::SwitchTab;

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("backend");
        let mut layout = crate::render::Layout::default();
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
        let mut layout = crate::render::Layout::default();
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
        app.switch_state = crate::panes::SwitchState::NeedsYou;
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
}
