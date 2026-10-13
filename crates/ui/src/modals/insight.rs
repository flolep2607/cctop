//! `optimize` and `compare`, drawn over the table.

use super::*;

/// `optimize` or `compare`, drawn over the table.
///
/// The text arrives already laid out from [`cctop_core::insight`], so this only has
/// to frame and scroll it — the alternative was a second implementation of the
/// same tables that could drift from the one the command prints.
pub(crate) fn draw_insight(frame: &mut Frame, area: Rect, app: &App) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A report longer than its box gets a bar that follows the scroll: at the
    /// top of the border first, at the bottom once scrolled to the end.
    #[test]
    fn a_long_report_has_a_scrollbar_that_follows_the_scroll() {
        use crate::scrollbar::{THUMB, tests::thumb_cells};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::tests::test_app();
        app.mode = crate::Mode::Insight;
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

    /// While the worker is still reading transcripts the overlay keeps moving:
    /// the spinner frame is the difference between a wait and a hang.
    #[test]
    fn a_report_still_loading_turns_a_spinner() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::tests::test_app();
        app.mode = crate::Mode::Insight;
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
}
