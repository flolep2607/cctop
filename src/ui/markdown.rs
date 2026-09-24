//! An assistant's reply, drawn as the markdown it was written in.
//!
//! Headings, emphasis, inline code, fenced blocks, lists, quotes, rules and
//! tables — what a coding agent actually writes — laid out to a fixed width in
//! the palette's own colours, so a sixteen-colour or colourless terminal gets
//! the same fold as the rest of the screen. Links come back with the rows and
//! columns they were drawn in, for the caller to make OSC 8 hyperlinks of once
//! it knows where on screen the text landed ([`hyperlink::link`]).
//!
//! The parser is `pulldown-cmark`; the layout is here. See the note beside the
//! dependency in `Cargo.toml` for why not `tui-markdown`.
//!
//! ponytail: fenced code is one colour, not highlighted. The pure-Rust
//! highlighter is syntect, whose bundled syntaxes and themes are several
//! megabytes of binary for a view that shows a few snippets — and whose
//! colours would be 24-bit, to be folded down to cctop's palette anyway.
//!
//! [`hyperlink::link`]: super::hyperlink::link

use super::styled::{self, Piece};
use super::theme;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::buffer::CellWidth;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::ops::Range;

/// A reply laid out: its rows, and where on them each link was drawn.
#[derive(Debug, Default)]
pub(super) struct Rendered {
    pub lines: Vec<Line<'static>>,
    pub links: Vec<Link>,
}

/// One row's worth of one link — a link wrapped over two rows is two of these,
/// with the same `url`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Link {
    /// Index into [`Rendered::lines`].
    pub line: usize,
    pub columns: Range<u16>,
    pub url: String,
}

/// `text` as markdown, wrapped to `width`, with plain text in `base`.
pub(super) fn render(text: &str, width: usize, base: Style) -> Rendered {
    render_with(text, width, base, super::hyperlink::supported())
}

/// [`render`], told whether the terminal will make a link of the label. Where
/// it will not, the URL is printed after the label, because otherwise it is
/// nowhere on screen at all.
fn render_with(text: &str, width: usize, base: Style, osc8: bool) -> Rendered {
    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let mut r = Renderer {
        width: width.max(1),
        base,
        osc8,
        out: Rendered::default(),
        blank: true,
        inline: Vec::new(),
        styles: Vec::new(),
        containers: Vec::new(),
        lists: Vec::new(),
        urls: Vec::new(),
        link: None,
        code: None,
        table: None,
    };
    for event in Parser::new_ext(text, options) {
        r.event(event);
    }
    r.flush();
    // The last block's gap is a separation from nothing.
    while r.out.lines.last().is_some_and(|l| l.width() == 0) {
        r.out.lines.pop();
    }
    r.out
}

enum Container {
    Quote,
    /// A list item, with its marker until the item's first row has shown it.
    Item {
        marker: String,
        shown: bool,
    },
}

#[derive(Default)]
struct Table {
    rows: Vec<Vec<String>>,
    /// How many of `rows` are the header, which is drawn bold and ruled off.
    head: usize,
}

struct Renderer {
    width: usize,
    base: Style,
    osc8: bool,
    out: Rendered,
    /// The last row out is a gap (or there is none yet), so a block starting
    /// now needs no gap of its own.
    blank: bool,
    /// The paragraph, heading or item text being gathered, not yet wrapped.
    inline: Vec<Piece>,
    /// Emphasis, strong, link… each patched over `base` in turn.
    styles: Vec<Style>,
    containers: Vec<Container>,
    /// Each open list's next number, `None` for a bulleted one.
    lists: Vec<Option<u64>>,
    urls: Vec<String>,
    /// The link being drawn: its index in `urls`, and where in `inline` its
    /// label starts — to tell a bare URL from a labelled one at its end.
    link: Option<(usize, usize)>,
    code: Option<String>,
    table: Option<Table>,
}

impl Renderer {
    fn style(&self) -> Style {
        self.styles.iter().fold(self.base, |s, p| s.patch(*p))
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                let style = code_style();
                match &mut self.table {
                    Some(t) => cell(t).push_str(&code),
                    None => {
                        // Without colour the backticks are what still says
                        // "this is code".
                        let code = match theme::no_color() {
                            true => format!("`{code}`"),
                            false => code.to_string(),
                        };
                        self.push(code, style);
                    }
                }
            }
            Event::InlineMath(t) | Event::DisplayMath(t) => self.push(t.to_string(), code_style()),
            Event::Html(html) | Event::InlineHtml(html) => {
                for (i, part) in html.split('\n').enumerate() {
                    if i > 0 {
                        self.flush();
                    }
                    if !part.is_empty() {
                        self.push(part.to_string(), theme::dim());
                    }
                }
            }
            Event::FootnoteReference(name) => self.push(format!("[^{name}]"), theme::dim()),
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.flush();
                self.gap();
                let (_, rest) = self.prefixes();
                let room = self.width.saturating_sub(width_of(&rest));
                let mut spans = rest;
                spans.push(Span::styled("─".repeat(room), theme::dim()));
                self.emit(styled::Row {
                    line: Line::from(spans),
                    links: Vec::new(),
                });
            }
            Event::TaskListMarker(done) => {
                let mark = if done { "[x] " } else { "[ ] " };
                self.push(mark.to_string(), theme::dim());
            }
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.gap(),
            Tag::Heading { level, .. } => {
                self.gap();
                let mut style = theme::title();
                if level <= HeadingLevel::H2 {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                self.styles.push(style);
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.gap();
                self.containers.push(Container::Quote);
                self.styles
                    .push(Style::default().add_modifier(Modifier::ITALIC));
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                self.gap();
                // The fence's language, dim above the block, is the only
                // highlighting there is; see the module note.
                if let CodeBlockKind::Fenced(lang) = kind
                    && let Some(lang) = lang.split_whitespace().next()
                {
                    self.push(lang.to_string(), theme::dim());
                    self.flush();
                }
                self.code = Some(String::new());
            }
            Tag::List(first) => {
                self.flush();
                // A list nested in an item runs straight on from the item's
                // text; only a list standing on its own is set apart.
                if !self
                    .containers
                    .iter()
                    .any(|c| matches!(c, Container::Item { .. }))
                {
                    self.gap();
                }
                self.lists.push(first);
            }
            Tag::Item => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let marker = format!("{n}. ");
                        *n += 1;
                        marker
                    }
                    _ => "• ".to_string(),
                };
                self.containers.push(Container::Item {
                    marker,
                    shown: false,
                });
            }
            Tag::Table(_) => {
                self.flush();
                self.gap();
                self.table = Some(Table::default());
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.rows.push(Vec::new());
                }
            }
            Tag::TableCell => {
                if let Some(t) = &mut self.table {
                    if t.rows.is_empty() {
                        t.rows.push(Vec::new());
                    }
                    t.rows.last_mut().expect("a row").push(String::new());
                }
            }
            Tag::Emphasis => self
                .styles
                .push(Style::default().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self
                .styles
                .push(Style::default().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self
                .styles
                .push(Style::default().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                self.urls.push(dest_url.to_string());
                self.link = Some((self.urls.len() - 1, self.inline.len()));
                self.styles.push(link_style());
            }
            Tag::HtmlBlock => {
                self.flush();
                self.gap();
            }
            // Footnote bodies, definition lists, front matter, super- and
            // subscript: none of them are enabled, and a parser that emitted
            // one anyway gets its text drawn plainly.
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::HtmlBlock => self.flush(),
            TagEnd::Heading(_) => {
                self.flush();
                self.styles.pop();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.containers.pop();
                self.styles.pop();
                self.gap();
            }
            TagEnd::CodeBlock => {
                let code = self.code.take().unwrap_or_default();
                for line in code.trim_end_matches('\n').split('\n') {
                    self.push(line.replace('\t', "    "), code_style());
                    self.flush_with(Some(Span::styled("│ ", theme::dim())));
                }
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::Item => {
                self.flush();
                self.containers.pop();
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.table(t);
                }
            }
            TagEnd::TableHead => {
                if let Some(t) = &mut self.table {
                    t.head = t.rows.len();
                }
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.styles.pop();
            }
            TagEnd::Link | TagEnd::Image => {
                self.styles.pop();
                if let Some((index, from)) = self.link.take() {
                    let url = self.urls[index].clone();
                    let label: String = self.inline[from..]
                        .iter()
                        .map(|p| p.text.as_str())
                        .collect();
                    // A bare URL is its own label and needs saying only once.
                    if !self.osc8 && label != url && !url.is_empty() {
                        match &mut self.table {
                            Some(t) => cell(t).push_str(&format!(" <{url}>")),
                            None => self.push(format!(" <{url}>"), theme::dim()),
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if let Some(code) = &mut self.code {
            code.push_str(text);
            return;
        }
        if let Some(t) = &mut self.table {
            cell(t).push_str(text);
            return;
        }
        self.push(text.to_string(), self.style());
    }

    fn push(&mut self, text: String, style: Style) {
        self.inline.push(Piece {
            text,
            style,
            link: self.link.map(|(i, _)| i),
        });
    }

    /// Set the next block apart from the last with one empty row — carrying
    /// the bars of any quote it is inside, so the quote reads as unbroken.
    fn gap(&mut self) {
        if self.blank {
            return;
        }
        let bars: Vec<Span<'static>> = self
            .containers
            .iter()
            .filter(|c| matches!(c, Container::Quote))
            .map(|_| quote_bar())
            .collect();
        self.out.lines.push(Line::from(bars));
        self.blank = true;
    }

    /// What goes in front of the next row, and of the rows its text wraps on
    /// to: a quote's bar, and an item's marker once, then the marker's width
    /// in spaces so the text lines up under itself.
    fn prefixes(&self) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
        let mut first = Vec::new();
        let mut rest = Vec::new();
        for c in &self.containers {
            match c {
                Container::Quote => {
                    first.push(quote_bar());
                    rest.push(quote_bar());
                }
                Container::Item { marker, shown } => {
                    let pad = " ".repeat(marker.cell_width() as usize);
                    match shown {
                        false => first.push(Span::styled(marker.clone(), bullet_style())),
                        true => first.push(Span::raw(pad.clone())),
                    }
                    rest.push(Span::raw(pad));
                }
            }
        }
        (first, rest)
    }

    fn flush(&mut self) {
        self.flush_with(None);
    }

    /// Wrap the gathered text out as rows, `gutter` after the container
    /// prefixes — a code block's bar.
    fn flush_with(&mut self, gutter: Option<Span<'static>>) {
        if self.inline.is_empty() && gutter.is_none() {
            return;
        }
        let (mut first, mut rest) = self.prefixes();
        if let Some(g) = gutter {
            first.push(g.clone());
            rest.push(g);
        }
        let pieces = std::mem::take(&mut self.inline);
        for row in styled::wrap(&pieces, self.width, &first, &rest) {
            self.emit(row);
        }
        for c in &mut self.containers {
            if let Container::Item { shown, .. } = c {
                *shown = true;
            }
        }
    }

    fn emit(&mut self, row: styled::Row) {
        let line = self.out.lines.len();
        for (columns, index) in row.links {
            self.out.links.push(Link {
                line,
                columns,
                url: self.urls[index].clone(),
            });
        }
        self.out.lines.push(row.line);
        self.blank = false;
    }

    /// Columns padded to their widest cell, the header bold and ruled off.
    ///
    /// ponytail: a table wider than the view is wrapped like any other text,
    /// so its columns stop lining up. Squeezing columns to fit means choosing
    /// which cell's text to cut, and the transcript is there when it matters.
    fn table(&mut self, table: Table) {
        let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut widths = vec![0usize; columns];
        for row in &table.rows {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(cell.trim().cell_width() as usize);
            }
        }
        let sep = || Piece::new(" │ ", theme::dim());
        for (r, row) in table.rows.iter().enumerate() {
            let style = match r < table.head {
                true => self.base.add_modifier(Modifier::BOLD),
                false => self.base,
            };
            for (i, width) in widths.iter().enumerate() {
                if i > 0 {
                    self.inline.push(sep());
                }
                let text = row.get(i).map(|c| c.trim()).unwrap_or("");
                let pad = width.saturating_sub(text.cell_width() as usize);
                self.inline
                    .push(Piece::new(format!("{text}{}", " ".repeat(pad)), style));
            }
            self.flush();
            if r + 1 == table.head {
                let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
                self.inline.push(Piece::new(rule.join("─┼─"), theme::dim()));
                self.flush();
            }
        }
    }
}

/// The cell text is being gathered into.
fn cell(table: &mut Table) -> &mut String {
    if table.rows.is_empty() {
        table.rows.push(Vec::new());
    }
    let row = table.rows.last_mut().expect("a row");
    if row.is_empty() {
        row.push(String::new());
    }
    row.last_mut().expect("a cell")
}

fn width_of(spans: &[Span<'static>]) -> usize {
    spans.iter().map(Span::width).sum()
}

fn quote_bar() -> Span<'static> {
    Span::styled("│ ", theme::dim())
}

fn code_style() -> Style {
    Style::default().fg(theme::colors().name_hue)
}

fn link_style() -> Style {
    Style::default()
        .fg(theme::colors().label)
        .add_modifier(Modifier::UNDERLINED)
}

fn bullet_style() -> Style {
    Style::default().fg(theme::colors().accent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(text: &str, width: usize) -> Rendered {
        render_with(text, width, Style::default(), true)
    }

    fn rows(r: &Rendered) -> Vec<String> {
        r.lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    /// The span carrying `needle`, for asserting on how it was styled.
    fn span_of<'a>(r: &'a Rendered, needle: &str) -> &'a Span<'static> {
        r.lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} was not drawn"))
    }

    /// The markers are gone and what they meant is in the style instead.
    #[test]
    fn headings_and_emphasis_become_style() {
        let r = draw("# Plan\n\nThis is **bold**, *soft* and `code`.", 60);
        assert_eq!(rows(&r), ["Plan", "", "This is bold, soft and code."]);
        let heading = span_of(&r, "Plan").style;
        assert!(
            heading
                .add_modifier
                .contains(Modifier::BOLD | Modifier::UNDERLINED)
        );
        assert!(
            span_of(&r, "bold")
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            span_of(&r, "soft")
                .style
                .add_modifier
                .contains(Modifier::ITALIC)
        );
        assert_eq!(span_of(&r, "code").style, code_style());
    }

    /// A fenced block keeps its lines and indentation as written, behind a
    /// gutter, with the fence's language named above it.
    #[test]
    fn a_fenced_block_keeps_its_lines() {
        let r = draw("```rust\nfn main() {\n    run();\n}\n```", 40);
        assert_eq!(rows(&r), ["rust", "│ fn main() {", "│     run();", "│ }"]);
    }

    /// Bullets and numbers are drawn, and a wrapped item's continuation lines
    /// up under its text rather than under its marker.
    #[test]
    fn lists_hang_their_continuation_under_the_text() {
        let r = draw("- one two three four\n- five\n\n1. first\n2. second", 12);
        assert_eq!(
            rows(&r),
            [
                "• one two",
                "  three four",
                "• five",
                "",
                "1. first",
                "2. second"
            ]
        );
    }

    /// A nested list runs on from its parent item, indented under its text.
    #[test]
    fn a_nested_list_indents_under_its_parent() {
        let r = draw("- outer\n  - inner", 30);
        assert_eq!(rows(&r), ["• outer", "  • inner"]);
    }

    /// A link is reported at the row and columns its label was drawn in —
    /// on both rows when it wraps — so OSC 8 can be laid over exactly them.
    #[test]
    fn a_link_is_reported_where_it_was_drawn() {
        let r = draw("See [the guide](https://example.com/g) now.", 12);
        assert_eq!(rows(&r), ["See the", "guide now."]);
        assert_eq!(
            r.links,
            vec![
                Link {
                    line: 0,
                    columns: 4..7,
                    url: "https://example.com/g".into()
                },
                Link {
                    line: 1,
                    columns: 0..5,
                    url: "https://example.com/g".into()
                },
            ]
        );
        assert!(
            span_of(&r, "guide")
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    /// Where the terminal cannot make a link, the URL is printed after its
    /// label — but a bare URL is not printed twice.
    #[test]
    fn without_osc8_the_url_is_spelled_out() {
        let r = render_with(
            "[docs](https://a.io) and <https://b.io>",
            80,
            Style::default(),
            false,
        );
        assert_eq!(rows(&r), ["docs <https://a.io> and https://b.io"]);
    }

    /// Quotes carry their bar on every row, gaps included.
    #[test]
    fn a_quote_keeps_its_bar() {
        let r = draw("> one\n>\n> two", 20);
        assert_eq!(rows(&r), ["│ one", "│ ", "│ two"]);
    }

    /// A table's columns line up, the header ruled off from the body.
    #[test]
    fn a_table_lines_up() {
        let r = draw("| a | bee |\n|---|---|\n| cc | d |", 40);
        assert_eq!(rows(&r), ["a  │ bee", "───┼────", "cc │ d  "]);
    }

    /// A rule spans the width; task markers stay legible.
    #[test]
    fn rules_and_task_lists() {
        let r = draw("- [x] done\n- [ ] todo\n\n---", 12);
        assert_eq!(rows(&r), ["• [x] done", "• [ ] todo", "", &"─".repeat(12)]);
    }
}
