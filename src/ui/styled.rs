//! Wrapping text that already carries its styles.
//!
//! [`panels::wrap`](super::panels::wrap) wraps a plain string, and the caller
//! styles each row it gets back. That stops working once the style changes
//! inside a line — a tool's red `error:` in front of plain text, a bold word in
//! a paragraph — because the row boundaries fall wherever the width says, not
//! where the styles do. So this wraps the styled cells themselves, by the same
//! rule: break at the last space that fits, hard-cut a token longer than the
//! row, and drop the spaces a break leaves at the start of the next row.
//!
//! It also carries links through the wrap. A link's text can land on two rows,
//! and OSC 8 is written per row over the columns it was drawn in (see
//! [`hyperlink::link`](super::hyperlink::link)), so each row reports the
//! columns every link ended up in.

use ratatui::buffer::CellWidth;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::ops::Range;

/// A run of text in one style, optionally the label of a link — an index into
/// whatever list of URLs the caller keeps.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Piece {
    pub text: String,
    pub style: Style,
    pub link: Option<usize>,
}

impl Piece {
    pub fn new(text: impl Into<String>, style: Style) -> Piece {
        Piece {
            text: text.into(),
            style,
            link: None,
        }
    }
}

/// One screen row, and the columns of it each link covers.
#[derive(Debug, Clone)]
pub(super) struct Row {
    pub line: Line<'static>,
    pub links: Vec<(Range<u16>, usize)>,
}

#[derive(Clone, Copy)]
struct Cell {
    ch: char,
    width: u16,
    style: Style,
    link: Option<usize>,
}

fn char_width(c: char) -> u16 {
    c.encode_utf8(&mut [0; 4]).cell_width()
}

fn spans_width(spans: &[Span<'static>]) -> usize {
    spans.iter().map(Span::width).sum()
}

/// Wrap `pieces` to `width` columns, `first` in front of the first row and
/// `rest` in front of every row after it — a list item's bullet, then the
/// hanging indent that lines its continuation up under the text.
///
/// Always at least one row, so an empty line still takes its place.
pub(super) fn wrap(
    pieces: &[Piece],
    width: usize,
    first: &[Span<'static>],
    rest: &[Span<'static>],
) -> Vec<Row> {
    // Control characters are dropped here rather than trusted to the buffer:
    // ratatui filters them out when it draws, but a width counted with them in
    // would put every column after them one out.
    let cells: Vec<Cell> = pieces
        .iter()
        .flat_map(|p| {
            p.text.chars().filter(|c| !c.is_control()).map(|ch| Cell {
                ch,
                width: char_width(ch),
                style: p.style,
                link: p.link,
            })
        })
        .collect();

    let mut out = Vec::new();
    let mut left: &[Cell] = &cells;
    let mut prefix = first;
    loop {
        let indent = spans_width(prefix);
        let room = width.saturating_sub(indent).max(1);
        let mut used = 0usize;
        let mut fit = 0;
        while fit < left.len() && used + left[fit].width as usize <= room {
            used += left[fit].width as usize;
            fit += 1;
        }
        let cut = match fit == left.len() {
            true => fit,
            // A space just past the edge is as good a break as one inside it:
            // the row is exactly full.
            false => match (1..=fit).rev().find(|&i| left[i].ch == ' ') {
                Some(i) => i,
                None => fit.max(1),
            },
        };
        out.push(row(prefix, &left[..cut], indent));
        left = &left[cut..];
        while left.first().is_some_and(|c| c.ch == ' ') {
            left = &left[1..];
        }
        if left.is_empty() {
            return out;
        }
        prefix = rest;
    }
}

/// `line` wrapped as it stands, for text with styles and no links — a tool's
/// coloured output.
pub(super) fn wrap_line(line: &Line<'static>, width: usize, indent: &str) -> Vec<Line<'static>> {
    let pieces: Vec<Piece> = line
        .spans
        .iter()
        .map(|s| Piece::new(s.content.to_string(), line.style.patch(s.style)))
        .collect();
    let prefix = [Span::raw(indent.to_string())];
    wrap(&pieces, width, &prefix, &prefix)
        .into_iter()
        .map(|r| r.line)
        .collect()
}

fn row(prefix: &[Span<'static>], cells: &[Cell], indent: usize) -> Row {
    let mut spans: Vec<Span<'static>> = prefix.to_vec();
    let mut links = Vec::new();
    let mut col = indent as u16;
    let mut i = 0;
    while i < cells.len() {
        let (style, link) = (cells[i].style, cells[i].link);
        let mut text = String::new();
        let start = col;
        while i < cells.len() && cells[i].style == style && cells[i].link == link {
            text.push(cells[i].ch);
            col += cells[i].width;
            i += 1;
        }
        if let Some(link) = link {
            links.push((start..col, link));
        }
        spans.push(Span::styled(text, style));
    }
    Row {
        line: Line::from(spans),
        links,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    fn text(row: &Row) -> String {
        row.line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Same rule as the plain wrap: the last space that fits, and no space
    /// carried to the front of the next row.
    #[test]
    fn breaks_at_the_last_space_and_keeps_each_style() {
        let red = Style::default().fg(Color::Red);
        let pieces = [
            Piece::new("error: ", red),
            Piece::new("expected one argument", Style::default()),
        ];
        let rows = wrap(&pieces, 16, &[], &[]);
        let texts: Vec<String> = rows.iter().map(text).collect();
        assert_eq!(texts, ["error: expected", "one argument"]);
        assert_eq!(rows[0].line.spans[0].style, red);
        assert_eq!(rows[0].line.spans[1].style, Style::default());
    }

    /// A token longer than the row is cut, not left overflowing it.
    #[test]
    fn a_long_token_is_hard_cut() {
        let rows = wrap(&[Piece::new("abcdefghij", Style::default())], 4, &[], &[]);
        let texts: Vec<String> = rows.iter().map(text).collect();
        assert_eq!(texts, ["abcd", "efgh", "ij"]);
    }

    /// A link wrapped over two rows is reported on both, at the columns it
    /// was drawn in, past the hanging indent.
    #[test]
    fn a_link_reports_its_columns_on_every_row() {
        let link = Piece {
            text: "the whole guide".into(),
            style: Style::default().add_modifier(Modifier::UNDERLINED),
            link: Some(0),
        };
        let first = [Span::raw("• ")];
        let rest = [Span::raw("  ")];
        let rows = wrap(
            &[Piece::new("see ", Style::default()), link],
            14,
            &first,
            &rest,
        );
        assert_eq!(text(&rows[0]), "• see the");
        assert_eq!(rows[0].links, vec![(6..9, 0)]);
        assert_eq!(text(&rows[1]), "  whole guide");
        assert_eq!(rows[1].links, vec![(2..13, 0)]);
    }

    /// Wide glyphs are two columns, so fewer of them fit.
    #[test]
    fn wide_glyphs_count_twice() {
        let rows = wrap(&[Piece::new("日本語です", Style::default())], 6, &[], &[]);
        let texts: Vec<String> = rows.iter().map(text).collect();
        assert_eq!(texts, ["日本語", "です"]);
    }
}
