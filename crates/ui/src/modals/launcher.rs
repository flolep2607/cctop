//! The launcher: where a new agent starts, and on what.

use super::*;

/// One harness's options, over the launcher it was opened from: a line per
/// setting the harness takes, where it will start, and the command that
/// Enter would run — the last so that what a setting *means* is never a
/// guess.
fn draw_launch_options(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    layout: &mut super::render::Layout,
) {
    use super::harness_opts::OptRow;
    const WIDTH: u16 = 62;
    const NAME: usize = 13;
    let Some(agent) = app.launch_agent_name() else {
        return;
    };
    let entry = app.options_for(&agent);
    let editing = app.launch_opt_edit;
    let mut lines: Vec<Line> = Vec::new();
    for (i, row) in app.option_rows().into_iter().enumerate() {
        let selected = i == app.launch_opt_cursor;
        let (name, value, loud, why_not) = match row {
            OptRow::Model => (
                "model",
                entry
                    .options
                    .model
                    .clone()
                    .unwrap_or_else(|| "default".to_string()),
                false,
                None,
            ),
            OptRow::Account => (
                "account",
                app.launch_profile()
                    .map(|p| p.name.clone())
                    .unwrap_or_default(),
                false,
                None,
            ),
            OptRow::Permissions => (
                "permissions",
                entry.options.permissions.label().to_string(),
                super::harness_opts::permission_is_loud(entry.options.permissions),
                None,
            ),
            OptRow::Worktree => match (&entry.branch, app.launch_worktree_point()) {
                (_, Err(why)) => ("worktree", "—".to_string(), false, Some(why)),
                (Some(branch), Ok(_)) => ("worktree", format!("new branch {branch}"), false, None),
                (None, Ok(_)) => ("worktree", "off".to_string(), false, None),
            },
        };
        let mut spans = vec![
            Span::styled(
                match selected {
                    true => " › ",
                    false => "   ",
                },
                theme::dim(),
            ),
            Span::styled(
                format!("{name:<NAME$}"),
                Style::default().fg(theme::colors().label),
            ),
        ];
        if selected && editing == Some(row) {
            spans.extend(app.launch_opt_input.spans(
                WIDTH as usize - NAME - 8,
                theme::value(),
                theme::value(),
                "▏",
            ));
        } else {
            let style = match (selected, loud, why_not.is_some()) {
                (_, _, true) => Style::default().fg(theme::colors().dimmer),
                (_, true, _) => Style::default().fg(theme::colors().cost_high),
                (true, _, _) => theme::selected(),
                (false, _, _) => theme::value(),
            };
            let shown = match (selected, why_not.is_none()) {
                // The arrows only where they do something.
                (true, true) => format!("‹ {value} ›"),
                _ => value,
            };
            spans.push(Span::styled(shown, style));
            if let Some(why) = why_not {
                spans.push(Span::styled(format!("  {why}"), theme::dim()));
            }
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));

    // Where, as the launcher's own line says it.
    let place = match &app.launch_remote {
        Some(target) => target.spelled(),
        None => app
            .launch_cwd
            .as_ref()
            .map(|d| cctop_core::util::tildify(&d.to_string_lossy()))
            .unwrap_or_else(|| "this directory".to_string()),
    };
    lines.push(Line::from(vec![
        Span::styled(" in ", Style::default().fg(theme::colors().label)),
        Span::styled(
            cctop_core::util::truncate(&place, WIDTH as usize - 22),
            theme::value(),
        ),
        Span::styled("  (c to change)", theme::dim()),
    ]));
    // What Enter would run, flags and all, or why it would not.
    let command = match app.with_options(vec![agent.clone()]) {
        Ok(argv) => {
            let on = match &app.launch_remote {
                Some(target) if target.peer => format!("on {}: ", target.host),
                Some(target) => format!("sandboxed to {}: ", target.host),
                None => String::new(),
            };
            (format!(" {on}{}", argv.join(" ")), theme::dim())
        }
        Err(why) => (
            format!(" {why}"),
            Style::default().fg(theme::colors().cost_mid),
        ),
    };
    lines.push(Line::from(Span::styled(
        cctop_core::util::truncate(&command.0, WIDTH as usize - 4),
        command.1,
    )));
    let keys = match editing {
        Some(_) => " Enter keep  Esc cancel  (empty = default)",
        None => match app.option_row() {
            Some(OptRow::Model | OptRow::Worktree) => {
                " ↑/↓  ←/→ change  e type  Enter start  Esc back"
            }
            _ => " ↑/↓  ←/→ change  Enter start  Esc back",
        },
    };
    lines.push(Line::from(Span::styled(keys, theme::dim())));
    let title = format!("{agent} options");
    let (outer, _) = modal(frame, area, &title, lines, WIDTH);
    layout.modal_rect = Some(outer);
    layout.launch_rows = Vec::new();
    layout.launch_cwd_rows = Vec::new();
}

/// The launcher: what to put in the tab, and where it will run.
pub(crate) fn draw_launch(
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

    if app.mode == super::Mode::LaunchOptions {
        return draw_launch_options(frame, area, app, layout);
    }

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
    let mut short = cctop_core::util::abbreviate_paths(&dirs).into_iter();

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
            tabs::Choice::Start(_) | tabs::Choice::Handoff(_) => None,
        };
        // The dot carries whether anyone is already looking, the word carries
        // what the agent said about itself. Two facts that can both be true at
        // once, so neither is made to wait for the other's column.
        let dot = match choice {
            // Ringed: a client is on this session already. Attaching a second
            // works, but the two then fight over one window's size.
            tabs::Choice::Waiting(agent) if agent.attached => "◉",
            tabs::Choice::Waiting(_) => "●",
            tabs::Choice::Start(_) | tabs::Choice::Handoff(_) => " ",
        };
        let dot_color = match (choice, reported) {
            (tabs::Choice::Waiting(_), Some(signal)) => theme::signal_color(signal),
            // Running, but it has never reported: no hooks installed, or nothing
            // has happened since cctop started listening. Still a live agent, so
            // it keeps a dot — just one that claims nothing.
            (tabs::Choice::Waiting(_), None) => theme::colors().dim,
            (tabs::Choice::Start(_) | tabs::Choice::Handoff(_), _) => theme::colors().dimmer,
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
        // A handoff line has no directory of its own — it starts where the
        // footer says — so the column says whose account it starts under,
        // which is the half of the pair that tells two `claude` lines apart.
        // A row that cannot go where the launcher points is greyed and says
        // so in this column, which is what is read while the cursor is on some
        // other row; the full sentence is under the list when it is picked.
        let blocked = app.launch_blocked(choice);
        let at = match (choice, choice.cwd().is_some()) {
            // A sibling starts agents and nothing else; what it will not
            // start is what ssh is for.
            _ if blocked.is_some() && app.launch_remote.as_ref().is_some_and(|t| t.peer) => {
                "needs ssh".to_string()
            }
            _ if blocked.is_some() => {
                match app.launch_remote.as_ref().and_then(|t| t.problem.as_ref()) {
                    Some(_) if matches!(choice, tabs::Choice::Start(argv) if matches!(super::location::reach_of(argv), super::location::Reach::Sandbox(_))) => {
                        "can't work there".to_string()
                    }
                    _ => "local only".to_string(),
                }
            }
            (tabs::Choice::Handoff(target), _) => target
                .account
                .as_deref()
                .map(|account| format!("as {account}"))
                .unwrap_or_default(),
            // What its options panel set, when anything differs from
            // starting it bare — a model, a mode, a worktree — so a launch
            // in plan mode or YOLO is never one nobody could see coming.
            (tabs::Choice::Start(argv), _) => {
                app.options_for(&super::location::agent_of(argv)).summary()
            }
            (_, true) => short.next().unwrap_or_default(),
            (_, false) => String::new(),
        };

        // The session's own title where cctop knows it. A resumed session's rmux
        // name carries the whole session id, which for Codex is a timestamp and a
        // uuid — two of those are identical for far more characters than this
        // column is wide, so the names alone would draw two rows that read the
        // same and do different things.
        let name = match choice {
            tabs::Choice::Waiting(agent) => app.waiting_label(agent),
            tabs::Choice::Handoff(target) => Some(target.agent.clone()),
            tabs::Choice::Start(_) => None,
        }
        .unwrap_or_else(|| choice.label());
        let style = match blocked {
            Some(_) if selected => theme::selected().fg(theme::colors().dim),
            Some(_) => Style::default().fg(theme::colors().dimmer),
            None => style,
        };

        choice_lines.push(rows.len());
        rows.push(Line::from(vec![
            Span::styled(format!(" {dot} "), Style::default().fg(dot_color)),
            Span::styled(
                format!("{:<NAME$}", cctop_core::util::truncate(&name, NAME)),
                style,
            ),
            Span::styled(
                format!("{:<WHERE$}", cctop_core::util::truncate(&at, WHERE - 1)),
                theme::dim(),
            ),
            Span::styled(
                format!("{:<STATE$}", cctop_core::util::truncate(&state, STATE)),
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
    let hits: &[super::location::Hit] = match editing {
        true => &app.launch_cwd_hits,
        false => &[],
    };
    // One more line under the field while a host is being waited on, or under
    // the list when the picked row cannot go where the launcher points: both
    // are the answer to "why is nothing happening", and both stay on screen.
    let note: Option<(String, bool)> = match editing {
        true => app.location_note(),
        false => app
            .launch_offer
            .get(app.launch_cursor)
            .and_then(|c| app.launch_blocked(c))
            .map(|why| (why, true)),
    };
    let room = (area.height as usize)
        .saturating_sub(4 + FOOTER + hits.len() + usize::from(note.is_some()))
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
                match (app.launch_cwd_bad, &app.launch_cwd_why) {
                    (true, Some(why)) => format!("  {why}"),
                    (true, None) => "  no such directory".to_string(),
                    (false, _) => String::new(),
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
                // The host and the path on it, scp-style, as the field takes
                // them. "unchecked" when the host could not be asked: the tab
                // will ask, and may still refuse.
                _ if app.launch_remote.is_some() => {
                    let target = app.launch_remote.as_ref().expect("checked");
                    format!(
                        " in {}{}  (c to change)",
                        cctop_core::util::truncate(&target.spelled(), WIDTH as usize - 34),
                        // A sibling checks the directory itself, and is
                        // reached a different way — which is the half worth
                        // saying.
                        match (target.peer, &target.unchecked) {
                            (true, _) => " (via tunnel)",
                            (false, Some(_)) => " (unchecked)",
                            (false, None) => "",
                        }
                    )
                }
                (_, Some(dir)) => format!(
                    " in {}  (c to change)",
                    cctop_core::util::truncate(
                        &cctop_core::util::tildify(&dir.to_string_lossy()),
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
    for (i, hit) in hits.iter().enumerate() {
        let selected = app.launch_cwd_pick == Some(i);
        // The tail, like the field itself: the part that distinguishes two
        // long paths is their end, and the marker keeps the column of names
        // clear of the `in` line above it. What follows it is dim: an ssh
        // host's other names, or why a remote directory cannot be used —
        // marked rather than left out, since it is still the way into the
        // directories under it.
        let (text, aside) = match hit {
            super::location::Hit::Dir(_) => (hit.text(), String::new()),
            super::location::Hit::Host { aliases, .. } => (
                hit.text(),
                match aliases.is_empty() {
                    true => "  ssh host".to_string(),
                    false => format!("  ssh host · {}", aliases.join(", ")),
                },
            ),
            super::location::Hit::Peer { offline, .. } => (
                hit.text(),
                match offline {
                    None => "  cctop on this account".to_string(),
                    Some(_) => "  cctop on this account · offline".to_string(),
                },
            ),
            super::location::Hit::Remote { path, problem, .. } => (
                path.clone(),
                problem
                    .as_ref()
                    .map(|why| format!("  {why}"))
                    .unwrap_or_default(),
            ),
        };
        let room_for = (WIDTH as usize - 8).saturating_sub(aside.chars().count().min(34));
        let shown = tail(&text, room_for);
        let aside = cctop_core::util::truncate(&aside, WIDTH as usize - 8 - shown.chars().count());
        let unusable = matches!(
            hit,
            super::location::Hit::Remote {
                problem: Some(_),
                ..
            }
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
                match (selected, unusable) {
                    (true, _) => theme::selected(),
                    (false, true) => Style::default().fg(theme::colors().dimmer),
                    (false, false) => theme::dim(),
                },
            ),
            Span::styled(
                aside,
                Style::default().fg(match unusable {
                    true => theme::colors().cost_mid,
                    false => theme::colors().dimmer,
                }),
            ),
        ]));
    }
    if let Some((text, warn)) = &note {
        lines.push(Line::from(Span::styled(
            format!(" {}", cctop_core::util::truncate(text, WIDTH as usize - 4)),
            Style::default().fg(match warn {
                true => theme::colors().cost_mid,
                false => theme::colors().dim,
            }),
        )));
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
    let picked_blocked = picked.and_then(|c| app.launch_blocked(c));
    let keys = match (editing, picked_waiting) {
        (true, _) if !hits.is_empty() => " Enter accept  Tab fill in  ↑/↓ pick  Esc cancel",
        (true, _) => " Enter accept  Esc keep the old one",
        (false, true) => " ↑/↓  Enter reattach  Esc cancel",
        (false, false) if picked_blocked.is_some() => " ↑/↓  c change folder  Esc cancel",
        (false, false) if matches!(picked, Some(tabs::Choice::Start(_))) => {
            " ↑/↓  → options  Enter start  Esc cancel"
        }
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
