//! Modal overlays: help, filters, and the confirmation dialogs.
//!
//! This file is the chrome every one of them is drawn in — the centred box,
//! the input line, the scrolled report, the QR code — and the dialogs are in
//! the files beside it, a family to a file.

mod accounts;
mod confirm;
mod help;
mod insight;
mod launcher;
mod pickers;
mod serve;
mod settings_page;

// Reached by the rest of the crate as `modals::draw_*`, as when this was one
// file, and by the dialogs as each other's helpers.
pub(super) use {
    accounts::*, confirm::*, help::*, insight::*, launcher::*, pickers::*, serve::*,
    settings_page::*,
};

// What the dialogs name as `super::x`: they were written one level up, where
// `super` was the crate.
use super::{Mode, address, connect, harness_opts, idle, location, menu, scrollbar, settings};

use super::columns::COLUMNS;
use super::hyperlink;
use super::line_edit::LineEdit;
use super::qr;
use super::render::{self, Layout};
use super::share;
use super::theme;
use super::{AGE_OPTIONS, AccountKind, App, BatchKind, LaunchInto, tabs};
use cctop_core::util;
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
///
/// The titles are more than a word — the help's carry its filter and the
/// build, which have to stay put on the border while the page under them
/// scrolls — so each line keeps its own alignment.
pub(super) fn scrollable_modal_titled(
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
