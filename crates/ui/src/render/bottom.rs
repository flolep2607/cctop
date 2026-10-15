//! The bottom panels: the selected session's details, its tools and its
//! performance.

use super::*;

pub(super) fn draw_bottom(frame: &mut Frame, area: Rect, app: &mut App, layout: &mut Layout) {
    app.ensure_available_tab();
    layout.bottom_start = area.y;
    layout.tab_row = area.y;
    layout.tool_sidebar = None;
    layout.tool_log = None;
    layout.subagent_header.clear();

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
            format!("↳ {}", cctop_core::util::truncate(&what, 48)),
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
        // A heading is a place, not a session: say which and how to open it
        // rather than implying the cursor is on nothing.
        let said = match app.selected_group() {
            Some(g) => format!(
                "{} — {}. Enter folds or unfolds it.",
                g.label,
                util::plural(g.sessions, "session")
            ),
            None => "No session selected".to_string(),
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(said, theme::dim()))),
            inner,
        );
        return;
    }

    // Ahead of the remote note below, which speaks of a transcript: what the
    // Preview panel lacks for a remote row is a tab, and it says so itself.
    if app.bottom_tab == super::preview::TAB {
        app.panel_max_scroll = 0;
        super::preview::draw(frame, area, inner, app, layout);
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
        let via = app.remote_via(&host);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!("This session is on {host}."),
                    theme::dim(),
                )),
                Line::default(),
                Line::from(Span::styled(
                    format!(
                        "cctop reads that machine's summary {via}; the transcript this panel \
                         would break down stays there. Info has everything that crossed."
                    ),
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
                panels::info(
                    session,
                    data,
                    app.plan,
                    app.clash_of(session).as_ref(),
                    session
                        .remote
                        .as_ref()
                        .map_or("over ssh", |r| app.remote_via(&r.host)),
                ),
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
            _ => (
                panels::context(session, data, width, inner.height as usize),
                app.context_scroll,
            ),
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
    // The subagents header is the panel's first line, and clickable only
    // while it is on screen: once the panel is scrolled, its rows own every
    // cell and the header is not among them.
    layout.subagent_header = if app.bottom_tab == 4 && scroll == 0 {
        app.panel_data
            .as_ref()
            .filter(|d| !d.subagents.is_empty())
            .map(|_| {
                panels::subagent_header_spans(target.width as usize)
                    .into_iter()
                    .map(|(a, b, sort)| (target.y, target.x + a as u16, target.x + b as u16, sort))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let total = lines.len();
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), target);
    // Shift+↑/↓ scrolls every one of these, and without the bar a panel cut
    // off at its last visible row looks exactly like one that ended there.
    layout.panel_track =
        scrollbar::on_border(frame, area, total, target.height as usize, scroll as usize);
}

const TOOL_SIDEBAR_W: u16 = 18;

/// Draw the per-tool sidebar and return the invocation lines for the main area.
#[allow(clippy::too_many_arguments)]
fn draw_tool_sidebar(
    frame: &mut Frame,
    inner: Rect,
    app: &App,
    data: &cctop_core::session::SessionData,
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
                    util::pad(&display, name_w, false),
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
    // The divider is the sidebar's own right border, so its bar goes there
    // rather than on the panel's, which belongs to the log beside it.
    scrollbar::draw(
        frame,
        Rect {
            x: inner.x + TOOL_SIDEBAR_W,
            width: 1,
            ..inner
        },
        tabs.len(),
        inner.height as usize,
        first,
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
    session: &cctop_core::session::Session,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The Info panel cut off by a short bottom pane carries a bar on its right
    /// border, and the same panel given room to finish carries none.
    #[test]
    fn a_bottom_panel_has_a_scrollbar_only_when_it_overflows() {
        use crate::scrollbar::tests::thumb_cells;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let draw = |rows: u16| {
            let mut app = crate::tests::test_app();
            app.sessions = vec![crate::tests::session("a", false, "x")];
            app.loaded = true;
            app.refilter();
            app.bottom_tab = 0;
            // An empty extraction is still one: without it Info is "Loading…".
            app.panel_data = Some(Default::default());
            let mut terminal = Terminal::new(TestBackend::new(120, rows)).expect("backend");
            let mut layout = Layout::default();
            terminal
                .draw(|frame| draw_bottom(frame, frame.area(), &mut app, &mut layout))
                .expect("draw");
            (terminal.backend().buffer().clone(), app.panel_max_scroll)
        };
        let (buf, max) = draw(5);
        assert!(max > 0, "Info fits in three rows now");
        assert!(thumb_cells(&buf) > 0, "no scrollbar on a clipped panel");

        let (buf, max) = draw(100);
        assert_eq!(max, 0);
        assert_eq!(thumb_cells(&buf), 0);
    }
}
