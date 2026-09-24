//! Clickable links: OSC 8 hyperlinks written into ratatui's cells.
//!
//! Adapted from the `hyperrat` crate, version 0.1.3 —
//! <https://crates.io/crates/hyperrat>, source at
//! <https://github.com/JayanAXHF/gitv/tree/main/crates/hyperrat> — which is
//! dual-licensed `Unlicense OR MIT`. cctop takes it under MIT, the licence cctop
//! itself is under, and that licence asks for its notice to travel with the code:
//!
//! > Copyright (c) 2015 Jayan Sunil
//! >
//! > Permission is hereby granted, free of charge, to any person obtaining a copy
//! > of this software and associated documentation files (the "Software"), to
//! > deal in the Software without restriction, including without limitation the
//! > rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
//! > sell copies of the Software, and to permit persons to whom the Software is
//! > furnished to do so, subject to the following conditions:
//! >
//! > The above copyright notice and this permission notice shall be included in
//! > all copies or substantial portions of the Software.
//! >
//! > THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! > IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! > FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! > AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! > LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
//! > FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
//! > IN THE SOFTWARE.
//!
//! Copied rather than depended on because what cctop needs from it is its one
//! idea, not its widget. The widget draws a label it is given; the link cctop
//! most needs is the sign-in URL in the add-account popup, which is drawn by
//! the embedded terminal and never exists as a label cctop holds. So the part
//! kept is the cell technique, turned around to work on cells already painted,
//! and hyperrat's truncation, hover style and fallback suffix are left behind
//! because nothing here would call them.
//!
//! # Why a link at all
//!
//! A terminal finds URLs in plain text by looking at the row under the pointer.
//! A URL wrapped over three rows is three fragments to it, and clicking the
//! first opened the first fragment — a sign-in page that cannot work. OSC 8
//! (`ESC ] 8 ; params ; URI ST label ESC ] 8 ; ; ST`) says outright which cells
//! are a link and where it goes, so every cell of every row it is wrapped over
//! opens the whole thing.
//!
//! # How it fits in a cell grid
//!
//! ratatui has no notion of an escape sequence: a cell holds a symbol and the
//! diff prints it. The trick, which is hyperrat's, is to put a whole run — open
//! sequence, label, close sequence — into the run's **first** cell, so the
//! terminal receives it in one write, and to keep the diff away from the cells
//! that write has already painted. Two things make that correct:
//!
//! - [`CellDiffOption::ForcedWidth`] on the first cell, set to the columns the
//!   label paints. Without it the diff measures the symbol as text — the URL
//!   included — and steps over that many columns, leaving whatever was drawn
//!   after the link undrawn. hyperrat 0.1.3 leaves this cell at `None`, which
//!   is that bug on ratatui 0.30; `ForcedWidth` is ratatui/ratatui#1605 and is
//!   what the option is for. The backend then moves the cursor before the next
//!   cell it prints, because that cell is not the one after the last it wrote.
//! - [`CellDiffOption::Skip`] on the cells the label covers, which is
//!   hyperrat's. They keep the characters they already held, so the buffer
//!   still reads as the screen does, but the diff never prints them while the
//!   link is up — and on the frame after it goes, a plain cell compares unequal
//!   to a skipped one whatever it holds, so every column is rewritten and none
//!   is left carrying a link to a page that is no longer on screen.
//!
//! The pair is atomic, which is the reason for one cell rather than an opening
//! cell and a closing one: a redraw that re-sent a changed opening and left the
//! unchanged closing alone would leave the link open, and every cell written
//! after it, anywhere on screen, would join it.
//!
//! A terminal without OSC 8 consumes the sequence, as ECMA-48 has every
//! terminal do with an operating-system command it does not know, and prints
//! the label — the same characters in the same columns either way.

use ratatui::buffer::{Buffer, CellDiffOption, CellWidth};
use ratatui::layout::Rect;
use ratatui::style::Style;
use std::num::NonZeroU16;
use std::ops::Range;

/// Draw `label` at `(x, y)` as a link to `url`, clipped to the buffer.
pub(super) fn draw(buf: &mut Buffer, x: u16, y: u16, label: &str, url: &str, style: Style) {
    let room = buf.area.right().saturating_sub(x) as usize;
    let (end, _) = buf.set_stringn(x, y, label, room, style);
    link(buf, y, x..end, url);
}

/// Make the cells already painted at `columns` on row `y` a link to `url`.
///
/// What is in them stays exactly as it was drawn. That is the point: the text
/// is the embedded terminal's or a paragraph's, laid out by them, and all this
/// adds is where a click on it goes.
///
/// One link per run of identically styled cells rather than one per row,
/// because the first cell's style is the style the whole run is printed in. A
/// row that underlines half the URL would otherwise come out underlined
/// throughout. The runs share an `id`, which is what tells a terminal that
/// they — and the runs on the rows below — are one link, to be lit up together
/// on hover.
pub(super) fn link(buf: &mut Buffer, y: u16, columns: Range<u16>, url: &str) {
    // Not under test: the runner's own TERM — `dumb` on plenty of CI — is not
    // the terminal these tests are about.
    if !cfg!(test) && !supported() {
        return;
    }
    let url = sanitise(url);
    if url.is_empty() {
        return;
    }
    let id = id_for(&url);
    let area = buf.area;
    if y < area.top() || y >= area.bottom() {
        return;
    }
    let end = columns.end.min(area.right());
    let mut x = columns.start.max(area.left());
    while x < end {
        let start = x;
        let style = buf[(x, y)].style();
        let mut label = String::new();
        // A cell some earlier writer already took out of the diff — an image, a
        // link drawn twice — is its to paint, and a run stops short of it.
        while x < end
            && buf[(x, y)].diff_option == CellDiffOption::None
            && buf[(x, y)].style() == style
        {
            let symbol = buf[(x, y)].symbol();
            // An empty symbol prints nothing and would leave the terminal's
            // cursor a column short of where the forced width says it is.
            let symbol = if symbol.is_empty() { " " } else { symbol };
            label.extend(symbol.chars().filter(|c| !c.is_control()));
            // A wide glyph's second column belongs to it, and is not the next
            // character of the label.
            x = x.saturating_add(symbol.cell_width().max(1));
        }
        let Some(width) = NonZeroU16::new(x - start) else {
            x += 1;
            continue;
        };
        for covered in start + 1..x.min(area.right()) {
            buf[(covered, y)].set_diff_option(CellDiffOption::Skip);
        }
        buf[(start, y)]
            .set_symbol(&format!("\x1b]8;id={id};{url}\x1b\\{label}\x1b]8;;\x1b\\"))
            .set_diff_option(CellDiffOption::ForcedWidth(width));
    }
}

/// Find `shown` on the rows of `area`, and make it a link to `url`.
///
/// For text cctop lays out through a paragraph, where the columns it lands in
/// are the paragraph's to decide: the text is looked for where it ended up
/// rather than predicted. Rows before `from` are passed over, so a caller with
/// two links that read the same — two tokens behind one origin — can take them
/// in order. Returns the row the text was found on.
///
/// ponytail: a `shown` the paragraph wrapped over two rows is not found, and
/// stays the plain text it was. Everything that calls this is a short origin or
/// command in a box sized to hold it on one line.
pub(super) fn link_shown(
    buf: &mut Buffer,
    area: Rect,
    from: u16,
    shown: &str,
    url: &str,
) -> Option<u16> {
    let area = area.intersection(buf.area);
    for y in from.max(area.top())..area.bottom() {
        let mut text = String::new();
        // `(byte offset into text, column)` at every cell boundary, so a match
        // in the text maps back to the columns that drew it.
        let mut bounds = Vec::new();
        let mut x = area.left();
        while x < area.right() {
            let symbol = buf[(x, y)].symbol();
            bounds.push((text.len(), x));
            text.push_str(symbol);
            x = x.saturating_add(symbol.cell_width().max(1));
        }
        bounds.push((text.len(), x));
        let Some(at) = text.find(shown) else {
            continue;
        };
        let column = |offset: usize| bounds.iter().find(|(o, _)| *o == offset).map(|b| b.1);
        if let (Some(start), Some(end)) = (column(at), column(at + shown.len())) {
            link(buf, y, start..end, url);
            return Some(y);
        }
    }
    None
}

/// Whether to write OSC 8 at all.
///
/// Every terminal is assumed to cope, as hyperrat assumes, bar the two that
/// demonstrably do not. `dumb` is hyperrat's. `linux` is cctop's: the kernel
/// console knows only the palette commands under `ESC ]`, and anything else
/// drops it back to printing — so the URL would land on screen as text, over
/// whatever was drawn after the link.
pub(super) fn supported() -> bool {
    static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORTED.get_or_init(|| supported_by(std::env::var("TERM").ok().as_deref()))
}

fn supported_by(term: Option<&str>) -> bool {
    !matches!(term, Some("dumb" | "linux"))
}

/// `url` with anything that could end the sequence taken out.
///
/// An ESC or a BEL inside it would close the sequence early and hand the rest
/// to the terminal as commands. None of the URLs cctop links is built that way
/// — they are Claude's, Cloudflare's and cctop's own — which is exactly the
/// kind of assumption that stops being true without anyone noticing.
fn sanitise(url: &str) -> String {
    url.chars().filter(|c| !c.is_control()).collect()
}

/// A link id for `url`: the same URL, the same id.
///
/// The spec allows any printable text but `:` and `;`, and a terminal compares
/// ids only against each other, so a hash is enough and needs no bookkeeping
/// to keep one wrapped link's rows together.
fn id_for(url: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    format!("cctop-{:x}", hasher.finish())
}

/// What one cell shows: the first character of a link's label for the cell that
/// carries the link, and the cell's own symbol otherwise — never the URL.
#[cfg(test)]
pub(crate) fn shown_in(symbol: &str) -> String {
    match label_of(symbol) {
        Some(label) => label.chars().next().map(String::from).unwrap_or_default(),
        None => symbol.to_string(),
    }
}

/// What `buf` reads as on a screen: each link's label in place of its escape
/// sequences, and the columns it covers not read twice.
///
/// For tests that look at what was drawn — which is the label, and never the
/// URL behind it — rather than at every byte a cell holds.
#[cfg(test)]
pub(crate) fn visible(buf: &Buffer) -> String {
    let mut out = String::new();
    let mut skip = 0u16;
    for (i, cell) in buf.content().iter().enumerate() {
        if i > 0 && i % buf.area.width as usize == 0 {
            out.push('\n');
            skip = 0;
        }
        if skip > 0 {
            skip -= 1;
            continue;
        }
        match (label_of(cell.symbol()), cell.diff_option) {
            (Some(label), CellDiffOption::ForcedWidth(width)) => {
                out.push_str(label);
                skip = width.get() - 1;
            }
            _ => out.push_str(cell.symbol()),
        }
    }
    out
}

/// The URL a cell's symbol links to, when it opens a link.
#[cfg(test)]
pub(crate) fn target_of(symbol: &str) -> Option<&str> {
    let rest = symbol.strip_prefix("\x1b]8;")?;
    let (_, rest) = rest.split_once(';')?;
    rest.split_once("\x1b\\").map(|(url, _)| url)
}

#[cfg(test)]
fn label_of(symbol: &str) -> Option<&str> {
    let rest = symbol.strip_prefix("\x1b]8;")?;
    let (_, rest) = rest.split_once("\x1b\\")?;
    rest.strip_suffix("\x1b]8;;\x1b\\")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    const URL: &str = "https://github.com/ratatui/";

    /// hyperrat's first test, with the forced width it was missing: one cell
    /// carries the sequence and the label, the rest of the label's columns are
    /// out of the diff, and the buffer still reads as the label.
    #[test]
    fn one_cell_carries_the_link_and_the_rest_are_skipped() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
        draw(&mut buf, 0, 0, "ratatui", URL, Style::default());

        let first = &buf[(0, 0)];
        assert_eq!(target_of(first.symbol()), Some(URL));
        assert_eq!(label_of(first.symbol()), Some("ratatui"));
        assert_eq!(
            first.diff_option,
            CellDiffOption::ForcedWidth(NonZeroU16::new(7).unwrap())
        );
        for x in 1..7 {
            assert_eq!(buf[(x, 0)].diff_option, CellDiffOption::Skip, "column {x}");
        }
        assert_eq!(buf[(7, 0)].diff_option, CellDiffOption::None);
        assert_eq!(visible(&buf).trim_end(), "ratatui");
    }

    /// The width a link takes is its label's and not its sequence's: what the
    /// diff writes after it lands in the column after the label, and the
    /// backend is made to move there rather than trust the cursor.
    #[test]
    fn what_follows_a_link_is_still_drawn_where_it_belongs() {
        let before = Buffer::empty(Rect::new(0, 0, 20, 1));
        let mut after = Buffer::empty(Rect::new(0, 0, 20, 1));
        draw(&mut after, 0, 0, "ratatui", URL, Style::default());
        after.set_string(8, 0, "next", Style::default());

        let drawn: Vec<(u16, &str)> = before
            .diff(&after)
            .into_iter()
            .map(|(x, _, cell)| (x, cell.symbol()))
            .collect();
        assert_eq!(drawn[0].0, 0);
        assert_eq!(
            drawn[1..].iter().map(|(x, s)| (*x, *s)).collect::<Vec<_>>(),
            vec![(8, "n"), (9, "e"), (10, "x"), (11, "t")]
        );
    }

    /// Drawn again unchanged, a link costs nothing; gone, it costs every
    /// column it covered — blanks included, since a blank the terminal printed
    /// inside a link is a blank that still opens it.
    #[test]
    fn a_link_is_free_to_keep_and_erased_whole_when_it_goes() {
        let mut linked = Buffer::empty(Rect::new(0, 0, 20, 1));
        draw(&mut linked, 2, 0, "a b", URL, Style::default());
        let mut again = Buffer::empty(Rect::new(0, 0, 20, 1));
        draw(&mut again, 2, 0, "a b", URL, Style::default());
        assert!(linked.diff(&again).is_empty());

        let blank = Buffer::empty(Rect::new(0, 0, 20, 1));
        let erased: Vec<u16> = linked.diff(&blank).iter().map(|(x, _, _)| *x).collect();
        assert_eq!(erased, vec![2, 3, 4]);
    }

    /// Two styles in one row are two runs, each printed in its own, and both
    /// the same link.
    #[test]
    fn a_change_of_style_splits_the_run_but_not_the_link() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        buf.set_string(0, 0, "abc", Style::default());
        let underlined = Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::UNDERLINED);
        buf.set_string(3, 0, "def", underlined);
        link(&mut buf, 0, 0..6, URL);

        assert_eq!(label_of(buf[(0, 0)].symbol()), Some("abc"));
        assert_eq!(label_of(buf[(3, 0)].symbol()), Some("def"));
        assert_eq!(buf[(3, 0)].style().fg, Some(Color::Blue));
        let id = |x: u16| buf[(x, 0)].symbol().split(';').nth(1).map(str::to_owned);
        assert_eq!(id(0), id(3));
        assert_eq!(visible(&buf).trim_end(), "abcdef");
    }

    /// A wide glyph is one character of the label and two columns of its width.
    #[test]
    fn a_wide_glyph_counts_its_columns_once() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        draw(&mut buf, 0, 0, "コx", URL, Style::default());
        assert_eq!(label_of(buf[(0, 0)].symbol()), Some("コx"));
        assert_eq!(
            buf[(0, 0)].diff_option,
            CellDiffOption::ForcedWidth(NonZeroU16::new(3).unwrap())
        );
    }

    /// Clipped by the buffer's edge, as hyperrat clips to its area, rather
    /// than claiming columns there are not.
    #[test]
    fn a_label_past_the_edge_is_cut_there() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        draw(&mut buf, 1, 0, "ratatui", URL, Style::default());
        assert_eq!(label_of(buf[(1, 0)].symbol()), Some("rat"));
        assert_eq!(visible(&buf), " rat");
    }

    /// A URL cannot smuggle a command out of the sequence.
    #[test]
    fn a_control_character_in_the_url_goes_nowhere() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        draw(
            &mut buf,
            0,
            0,
            "x",
            "https://a\x1b]0;pwned\x07/",
            Style::default(),
        );
        assert_eq!(target_of(buf[(0, 0)].symbol()), Some("https://a]0;pwned/"));
    }

    #[test]
    fn only_the_terminals_known_to_print_it_go_without() {
        assert!(!supported_by(Some("dumb")));
        assert!(!supported_by(Some("linux")));
        assert!(supported_by(Some("xterm-256color")));
        assert!(supported_by(Some("tmux-256color")));
        assert!(supported_by(None));
    }

    /// Text is found where the paragraph put it, and a second copy of it is
    /// found by starting below the first.
    #[test]
    fn shown_text_is_linked_where_it_landed() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 3));
        buf.set_string(1, 0, "go http://h:1 now", Style::default());
        buf.set_string(4, 2, "http://h:1", Style::default());
        let area = buf.area;

        let row = link_shown(&mut buf, area, 0, "http://h:1", "http://h:1/?t=a");
        assert_eq!(row, Some(0));
        assert_eq!(target_of(buf[(4, 0)].symbol()), Some("http://h:1/?t=a"));
        let row = link_shown(&mut buf, area, 1, "http://h:1", "http://h:1/?t=b");
        assert_eq!(row, Some(2));
        assert_eq!(target_of(buf[(4, 2)].symbol()), Some("http://h:1/?t=b"));
        assert_eq!(link_shown(&mut buf, area, 0, "absent", URL), None);
    }
}
