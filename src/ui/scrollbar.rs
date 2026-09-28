//! The scrollbar every scrolled view draws on its own right border.
//!
//! On the border rather than in a column of its own: a column would take a
//! cell from every line of text for something that is only there when the text
//! overflows, and the reflow between "fits" and "does not fit" would move every
//! line the moment the box got one row shorter. The border is already there,
//! and a thumb laid over it costs nothing it was showing.
//!
//! Drawn only when the content overflows. A bar with a thumb the length of its
//! track says nothing, and on every box that fits it would read as a hint that
//! there is more.

use super::theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};

/// The thumb, heavier than the border it slides along.
///
/// No track symbol: the border line underneath *is* the track, in whatever
/// colour its box already gave it, so the bar matches the box it is on
/// instead of carrying a colour of its own down every panel. The thumb is in
/// the accent, which the palette has already folded for a sixteen-colour
/// terminal; without colour it is told apart by weight alone, `┃` over `│`.
pub(super) const THUMB: &str = "┃";

/// Draw a scrollbar over `track` — one column, one cell per visible row — for
/// `total` rows of which `visible` are shown from `offset`.
///
/// Nothing is drawn when everything fits. Returns the track when a bar was
/// drawn, for the caller to record as somewhere a click means "go here": a
/// bar that answered clicks while invisible would be a trap on every border.
pub(super) fn draw(
    frame: &mut Frame,
    track: Rect,
    total: usize,
    visible: usize,
    offset: usize,
) -> Option<Rect> {
    if total <= visible || visible == 0 || track.height == 0 || track.width == 0 {
        return None;
    }
    let max_offset = total - visible;
    // Positions, not rows: ratatui sizes the thumb as `viewport` over
    // `positions - 1 + viewport`, which is `visible / total` only when the
    // positions are the offsets the view can actually take.
    let mut state = ScrollbarState::new(max_offset + 1)
        .viewport_content_length(visible)
        .position(offset.min(max_offset));
    let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(None)
        .thumb_symbol(THUMB)
        .thumb_style(Style::default().fg(theme::colors().accent));
    frame.render_stateful_widget(bar, track, &mut state);
    Some(track)
}

/// The position a click on `track` at screen row `row` asks for, out of
/// `0..=last`: the top cell is the first, the bottom cell the last, and the
/// cells between share the rest evenly.
///
/// A jump to the spot rather than a page towards it, because the thumb is the
/// share shown: pointing at where it should be is the one gesture the bar can
/// answer exactly, and a page is already `PgUp`/`PgDn`. Rounded to the nearest
/// so the middle cell of an odd track lands on the middle of the content.
pub(super) fn position_at(track: Rect, row: u16, last: usize) -> usize {
    let span = track.height.saturating_sub(1) as usize;
    let at = row
        .saturating_sub(track.y)
        .min(track.height.saturating_sub(1)) as usize;
    if span == 0 {
        return 0;
    }
    (at * last + span / 2) / span
}

/// Whether the screen cell `(col, row)` is on `track`.
pub(super) fn hit(track: Option<Rect>, col: u16, row: u16) -> Option<Rect> {
    track.filter(|t| t.contains(ratatui::layout::Position::new(col, row)))
}

/// The right border of the bordered box `outer`, between its corners, from
/// row `from` for `rows` rows — clamped so a track can never reach a corner.
pub(super) fn right_border(outer: Rect, from: u16, rows: u16) -> Rect {
    let top = from.max(outer.y + 1);
    let bottom = (from + rows).min(outer.bottom().saturating_sub(1));
    Rect {
        x: outer.right().saturating_sub(1),
        y: top,
        width: 1,
        height: bottom.saturating_sub(top),
    }
}

/// [`draw`] down the whole right border of `outer`, for a box whose every
/// inner row is the scrolled text.
pub(super) fn on_border(
    frame: &mut Frame,
    outer: Rect,
    total: usize,
    visible: usize,
    offset: usize,
) -> Option<Rect> {
    let track = right_border(outer, outer.y + 1, outer.height.saturating_sub(2));
    draw(frame, track, total, visible, offset)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::widgets::Block;

    /// How many cells of `buf` are a thumb: what every overflow test asks.
    pub(crate) fn thumb_cells(buf: &Buffer) -> usize {
        buf.content().iter().filter(|c| c.symbol() == THUMB).count()
    }

    fn drawn(total: usize, offset: usize) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(20, 12)).expect("backend");
        terminal
            .draw(|frame| {
                let outer = frame.area();
                frame.render_widget(Block::bordered(), outer);
                let _ = on_border(frame, outer, total, 10, offset);
            })
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    /// The ends of the track are the ends of the content, whatever its length,
    /// and a click off the track is clamped to it rather than past it.
    #[test]
    fn a_click_on_the_track_asks_for_its_share_of_the_content() {
        let track = Rect::new(19, 1, 1, 10);
        assert_eq!(position_at(track, 1, 99), 0);
        assert_eq!(position_at(track, 10, 99), 99);
        assert_eq!(position_at(track, 30, 99), 99);
        assert_eq!(position_at(track, 0, 99), 0);
        let mid = position_at(track, 5, 99);
        assert!((40..=50).contains(&mid), "{mid}");
        assert_eq!(position_at(Rect::new(0, 4, 1, 1), 4, 7), 0);
        assert!(hit(Some(track), 19, 3).is_some());
        assert!(hit(Some(track), 18, 3).is_none());
        assert!(hit(None, 19, 3).is_none());
    }

    #[test]
    fn nothing_is_drawn_when_it_fits() {
        assert_eq!(thumb_cells(&drawn(10, 0)), 0);
        assert_eq!(thumb_cells(&drawn(3, 0)), 0);
    }

    /// Twice the rows is half the track, and the thumb stays on the border
    /// between the corners wherever the view is.
    #[test]
    fn the_thumb_is_the_share_shown_and_moves_with_the_view() {
        let top = drawn(20, 0);
        assert_eq!(thumb_cells(&top), 5);
        assert_eq!(top[(19, 1)].symbol(), THUMB);
        assert_eq!(top[(19, 0)].symbol(), "┐");

        let bottom = drawn(20, 10);
        assert_eq!(thumb_cells(&bottom), 5);
        assert_eq!(bottom[(19, 10)].symbol(), THUMB);
        assert_eq!(bottom[(19, 11)].symbol(), "┘");
        assert_eq!(bottom[(19, 1)].symbol(), "│");
    }
}
