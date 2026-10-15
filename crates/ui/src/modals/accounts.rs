//! Signing in: the Claude accounts agents run on.

use super::*;

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
pub(crate) fn draw_add_account(frame: &mut Frame, area: Rect, app: &mut App, layout: &mut Layout) {
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
        lines.push(Line::default());
        Confirm {
            title: "Add a Claude account",
            width: 72,
            hint: " [Enter] done",
            keys: &[("[Enter]", KeyEvent::from(KeyCode::Enter))],
        }
        .draw(frame, area, layout, lines);
        return;
    }

    if flow.named && flow.pane.is_none() {
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
        ];
        Confirm {
            title: &format!("Add Claude account {}", flow.name),
            width: 64,
            hint: " [f] full login   [t] token   [Esc] cancel",
            keys: &[
                ("[f]", KeyEvent::from(KeyCode::Char('f'))),
                ("[t]", KeyEvent::from(KeyCode::Char('t'))),
                ("[Esc]", esc),
            ],
        }
        .draw(frame, area, layout, lines);
        return;
    }

    let Some(pane) = flow.pane.as_mut() else {
        let lines = vec![
            Line::from(Span::styled(" What is it called?", theme::dim())),
            input_line(&flow.name),
            Line::default(),
        ];
        Confirm {
            title: "Add a Claude account",
            width: 52,
            hint: " [Enter] next   [Esc] cancel",
            keys: &[("[Enter]", KeyEvent::from(KeyCode::Enter)), ("[Esc]", esc)],
        }
        .draw(frame, area, layout, lines);
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
    // Before the link's hyperlink cells: those are cctop's, and the pass may
    // pin them like the rest of its drawing.
    layout
        .passthrough
        .extend(theme::passthrough(frame.buffer_mut(), screen));
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
            let mut app = crate::tests::test_app();
            let mut pane = crate::tabs::Pane::for_test("claude");
            pane.view.parser.process(
                format!(
                    "Browser didn't open? Use the url below:\r\n\r\n{link}\r\n\r\nPaste code > "
                )
                .as_bytes(),
            );
            app.add_account = crate::AddAccount {
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
}
