//! The conversation reader: a session's transcript, full-screen, in the
//! terminal.
//!
//! It is the terminal half of what `cctop serve` shows a browser, over the same
//! [`chat::build`] — so every harness the web view reads, this reads, and a
//! remote row's conversation arrives over ssh as the same document. Somebody on
//! an ssh session with no browser to open gets the whole of it here.
//!
//! Laid out once per width, not once per frame. A long session paged back with
//! `u` is thousands of turns and tens of thousands of rows, and re-rendering
//! every reply's markdown on every frame — the dashboard redraws at ten a
//! second — would freeze the screen it is drawn on. So each turn is laid out
//! into a [`Block`] and kept; a frame copies only the rows it shows. Opening
//! one turn's tools, or a page of older turns arriving, lays out only the
//! turns that changed. Only a new width lays out everything again, which it
//! has to.
//!
//! [`chat::build`]: crate::serve::chat::build

use super::hyperlink;
use super::markdown::Link;
use super::share;
use super::theme;
use super::{App, ChatView, Mode};
use crate::serve::chat::{Conversation, ToolUse, Turn};
use crate::session::Session;
use chrono::{DateTime, Local, Utc};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block as Border, BorderType, Clear, Paragraph};
use std::collections::HashMap;

/// The widest the text runs, however wide the terminal. Prose past about a
/// hundred and twenty columns is read by moving the head, not the eyes; on a
/// wider screen the column is centred instead.
const MAX_TEXT: usize = 120;

/// Rows a wheel notch scrolls.
const WHEEL: usize = 3;

/// A conversation laid out to one width.
pub struct Laid {
    width: usize,
    raw: bool,
    blocks: Vec<Block>,
    /// The row each block starts on; `starts[i] + blocks[i].lines.len()` is the
    /// next one's.
    starts: Vec<usize>,
    total: usize,
}

/// One turn laid out — or the note above them all, which is the block with no
/// `seq`.
struct Block {
    seq: Option<usize>,
    /// Whether its tools were laid out in full, which is the other half of
    /// what makes a kept block still right.
    open: bool,
    has_tools: bool,
    /// When it was said, for the age drawn after the header at paint time —
    /// an age laid out once would go stale while the reader sat open.
    ts: Option<DateTime<Utc>>,
    lines: Vec<Line<'static>>,
    links: Vec<Link>,
    /// Each row's text, lower-cased a character at a time so a character
    /// index here is one in the row — what search matches against and
    /// highlights by.
    plain: Vec<Vec<char>>,
}

/// `/`, and what it found.
#[derive(Default)]
pub struct Search {
    pub query: String,
    /// The query is still being typed; keys go to it, not to the scroll.
    pub typing: bool,
    /// Rows the query is on, top to bottom.
    hits: Vec<usize>,
    /// Which of `hits` `n` and `N` last landed on.
    current: Option<usize>,
    /// Where the view stood when `/` was pressed: typing searches forward
    /// from here, and `Esc` puts the view back.
    origin: usize,
}

impl Laid {
    fn max_top(&self, visible: usize) -> usize {
        self.total.saturating_sub(visible)
    }

    /// The block row `row` falls in.
    fn block_at(&self, row: usize) -> usize {
        self.starts.partition_point(|&s| s <= row).saturating_sub(1)
    }

    fn row(&self, row: usize) -> Option<(&Block, usize)> {
        let b = self.block_at(row);
        let block = self.blocks.get(b)?;
        let at = row.checked_sub(self.starts[b])?;
        (at < block.lines.len()).then_some((block, at))
    }

    /// Every row the lower-cased `query` appears on.
    fn find(&self, query: &[char]) -> Vec<usize> {
        if query.is_empty() {
            return Vec::new();
        }
        let mut hits = Vec::new();
        for (block, start) in self.blocks.iter().zip(&self.starts) {
            for (i, row) in block.plain.iter().enumerate() {
                if !matches_in(row, query).is_empty() {
                    hits.push(start + i);
                }
            }
        }
        hits
    }
}

impl ChatView {
    fn laid_total(&self) -> usize {
        self.laid.as_ref().map_or(0, |l| l.total)
    }

    fn max_top(&self) -> usize {
        self.laid_total().saturating_sub(self.visible)
    }

    /// The row at the top of the view. `back` is kept from the end — see its
    /// field — so the top is worked out from it, not stored.
    fn top(&self) -> usize {
        self.max_top().saturating_sub(self.back)
    }

    fn set_top(&mut self, top: usize) {
        self.back = self.max_top().saturating_sub(top.min(self.max_top()));
    }

    /// Put `row` a third of the way down, where it reads with some of what led
    /// to it above.
    fn show_row(&mut self, row: usize) {
        self.set_top(row.saturating_sub(self.visible / 3));
    }

    fn scroll_up(&mut self, rows: usize) {
        self.back = (self.back + rows).min(self.max_top());
    }

    fn scroll_down(&mut self, rows: usize) {
        self.back = self.back.saturating_sub(rows);
    }

    /// Whether `seq`'s tool calls are drawn in full: `t` sets the default for
    /// all of them, and `Enter` flips one turn against it.
    fn is_open(&self, seq: usize) -> bool {
        self.tools_open != self.opened.contains(&seq)
    }

    /// The turn whose tools `Enter` opens: the first on screen, from the top,
    /// that has any — the turn being read, or the next one down when that one
    /// made no calls.
    fn turn_to_open(&self) -> Option<usize> {
        let laid = self.laid.as_ref()?;
        let top = self.top();
        let bottom = top + self.visible;
        (laid.block_at(top)..laid.blocks.len())
            .take_while(|&b| laid.starts[b] < bottom)
            .find(|&b| laid.blocks[b].has_tools)
            .and_then(|b| laid.blocks[b].seq)
    }

    /// The query's hits in the current layout, and the view moved to the first
    /// one at or below `from`.
    fn search_from(&mut self, from: usize) {
        let query: Vec<char> = lower(&self.search.query);
        self.search.hits = self
            .laid
            .as_ref()
            .map(|l| l.find(&query))
            .unwrap_or_default();
        self.search.current = self.search.hits.iter().position(|&h| h >= from).or((!self
            .search
            .hits
            .is_empty())
        .then_some(0));
        if let Some(i) = self.search.current {
            self.show_row(self.search.hits[i]);
        }
    }

    /// `n` and `N`: the next hit below the one last landed on, or above it,
    /// wrapping at the ends like the table's search does.
    fn step_hit(&mut self, forward: bool) {
        let n = self.search.hits.len();
        if n == 0 {
            return;
        }
        let i = match self.search.current {
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
            None => 0,
        };
        self.search.current = Some(i);
        self.show_row(self.search.hits[i]);
    }

    fn clear_search(&mut self) {
        self.search = Search::default();
    }
}

impl App {
    /// A key in the reader.
    ///
    /// There is nothing here that can touch the session: it is a transcript
    /// being read, not a terminal being driven.
    pub(super) fn on_key_conversation(&mut self, key: KeyEvent) {
        self.needs_redraw = true;
        let Some(view) = &mut self.chat else {
            self.mode = Mode::List;
            return;
        };
        if view.search.typing {
            return on_key_search(view, key);
        }
        let page = view.visible.saturating_sub(2).max(1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') => self.close_conversation(),
            // A search on screen goes first, as it does in the table: one Esc
            // to stop looking, another to leave.
            KeyCode::Esc if !view.search.query.is_empty() => view.clear_search(),
            KeyCode::Esc => self.close_conversation(),
            KeyCode::Char('d') if ctrl => view.scroll_down(page / 2),
            KeyCode::Char('u') if ctrl => view.scroll_up(page / 2),
            KeyCode::Down | KeyCode::Char('j') => view.scroll_down(1),
            KeyCode::Up | KeyCode::Char('k') => view.scroll_up(1),
            KeyCode::PageDown | KeyCode::Char(' ') => view.scroll_down(page),
            KeyCode::PageUp | KeyCode::Char('b') => view.scroll_up(page),
            // The start, and the live edge the reader opened on.
            KeyCode::Home | KeyCode::Char('g') => view.back = view.max_top(),
            KeyCode::End | KeyCode::Char('G') => view.back = 0,
            // A turn at a time: the reply you opened on is usually one `[`
            // away, however much tool output sits under it.
            KeyCode::Char('[') => {
                let top = view.top();
                if let Some(laid) = &view.laid {
                    let b = laid.block_at(top);
                    // The header of the turn being read, when it is above the
                    // top; the one before it when it is the top.
                    let row = match laid.starts[b] < top {
                        true => laid.starts[b],
                        false => laid.starts[b.saturating_sub(1)],
                    };
                    view.set_top(row);
                }
            }
            KeyCode::Char(']') => {
                let top = view.top();
                if let Some(laid) = &view.laid {
                    let next = laid.starts.iter().copied().find(|&s| s > top);
                    match next {
                        Some(row) => view.set_top(row),
                        None => view.back = 0,
                    }
                }
            }
            KeyCode::Char('/') => {
                view.search = Search {
                    typing: true,
                    origin: view.top(),
                    ..Search::default()
                };
            }
            KeyCode::Char('n') => view.step_hit(true),
            KeyCode::Char('N') => view.step_hit(false),
            // Every tool call at once, or back to one line each.
            KeyCode::Char('t') => {
                view.tools_open = !view.tools_open;
                view.opened.clear();
                view.relayout(true);
            }
            // The tools of the turn being read, and only those.
            KeyCode::Enter | KeyCode::Char('x') => {
                if let Some(seq) = view.turn_to_open() {
                    if !view.opened.remove(&seq) {
                        view.opened.insert(seq);
                    }
                    view.relayout(true);
                }
            }
            // The source, for when what matters is the exact characters.
            KeyCode::Char('m') => {
                view.raw = !view.raw;
                view.relayout(true);
            }
            // `u` for "earlier": the window grows at the top, which a
            // bottom-anchored scroll survives without moving a line.
            KeyCode::Char('u') => {
                let before = match view.fetching {
                    true => None,
                    false => view
                        .conversation
                        .as_ref()
                        .filter(|c| c.earlier > 0)
                        .and_then(|c| c.turns.first().map(|t| t.seq)),
                };
                if let Some(seq) = before {
                    self.fetch_chat(Some(seq));
                }
            }
            _ => {}
        }
    }

    /// The wheel in the reader, which covers the whole screen and so answers
    /// it wherever the pointer is.
    pub(super) fn on_wheel_conversation(&mut self, up: bool) {
        if let Some(view) = &mut self.chat {
            match up {
                true => view.scroll_up(WHEEL),
                false => view.scroll_down(WHEEL),
            }
        }
    }

    fn close_conversation(&mut self) {
        self.mode = Mode::List;
        self.chat = None;
    }
}

/// A key while the query is being typed.
///
/// Every keystroke searches again, from where the view stood when `/` was
/// pressed — the view follows the query as it narrows, the way `less` does.
fn on_key_search(view: &mut ChatView, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            let origin = view.search.origin;
            view.clear_search();
            view.set_top(origin);
        }
        // Kept, highlighted, for `n` and `N`; an empty query was nothing.
        KeyCode::Enter => match view.search.query.is_empty() {
            true => view.clear_search(),
            false => view.search.typing = false,
        },
        KeyCode::Backspace => {
            view.search.query.pop();
            view.search_from(view.search.origin);
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            view.search.query.push(c);
            view.search_from(view.search.origin);
        }
        _ => {}
    }
}

impl ChatView {
    /// Lay the conversation out again, before the next frame.
    ///
    /// `hold` keeps the row at the top where it is even when the view sits at
    /// the end. A key that opens or closes something is asking to see *that*
    /// change; the live edge's usual claim — stay with the newest line — is for
    /// turns arriving, not for a fold the reader just asked for.
    pub(super) fn relayout(&mut self, hold: bool) {
        self.dirty = true;
        self.hold |= hold;
    }
}

/// Bring `view.laid` up to date with `width`, reusing every block that is
/// still right, and keep the row that was at the top at the top.
fn lay_out(view: &mut ChatView, width: usize) {
    let fresh = view
        .laid
        .as_ref()
        .is_some_and(|l| l.width == width && l.raw == view.raw && !view.dirty);
    if fresh {
        return;
    }
    let Some(conv) = &view.conversation else {
        return;
    };

    // What is at the top now, as a turn and a row into it, which is what
    // survives a relayout — a row number does not.
    let anchor = match (&view.laid, view.back > 0 || view.hold) {
        (Some(old), true) => {
            let top = old.max_top(view.visible).saturating_sub(view.back);
            let b = old.block_at(top);
            old.blocks
                .get(b)
                .map(|block| (block.seq, top - old.starts[b]))
        }
        _ => None,
    };

    let reusable = view
        .laid
        .take()
        .filter(|l| l.width == width && l.raw == view.raw);
    let mut kept: HashMap<Option<usize>, Block> = reusable
        .map(|l| l.blocks.into_iter().map(|b| (b.seq, b)).collect())
        .unwrap_or_default();

    let who = view.session.surface.label(view.session.provider);
    let mut blocks = Vec::with_capacity(conv.turns.len() + 1);
    if let Some(note) = &conv.note {
        blocks.push(
            kept.remove(&None)
                .unwrap_or_else(|| note_block(note, width)),
        );
    }
    for turn in &conv.turns {
        let open = view.is_open(turn.seq);
        let block = match kept.remove(&Some(turn.seq)) {
            Some(b) if b.open == open => b,
            _ => turn_block(&view.session, who, turn, width, view.raw, open),
        };
        blocks.push(block);
    }
    let mut starts = Vec::with_capacity(blocks.len());
    let mut total = 0;
    for b in &blocks {
        starts.push(total);
        total += b.lines.len();
    }
    view.laid = Some(Laid {
        width,
        raw: view.raw,
        blocks,
        starts,
        total,
    });
    view.dirty = false;
    view.hold = false;

    let row = anchor.and_then(|(seq, offset)| {
        let laid = view.laid.as_ref()?;
        let b = laid.blocks.iter().position(|b| b.seq == seq)?;
        Some(laid.starts[b] + offset.min(laid.blocks[b].lines.len().saturating_sub(1)))
    });
    if let Some(row) = row {
        view.set_top(row);
    }
    if !view.search.query.is_empty() {
        let query = lower(&view.search.query);
        view.search.hits = view
            .laid
            .as_ref()
            .map(|l| l.find(&query))
            .unwrap_or_default();
        view.search.current = view.search.current.filter(|&i| i < view.search.hits.len());
    }
}

fn note_block(note: &str, width: usize) -> Block {
    let mut lines: Vec<Line<'static>> = super::panels::wrap(note, width)
        .into_iter()
        .map(|l| Line::styled(l, theme::dim()))
        .collect();
    lines.push(Line::default());
    finish(None, false, false, None, lines, Vec::new())
}

fn finish(
    seq: Option<usize>,
    open: bool,
    has_tools: bool,
    ts: Option<DateTime<Utc>>,
    lines: Vec<Line<'static>>,
    links: Vec<Link>,
) -> Block {
    let plain = lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .flat_map(|s| s.content.chars())
                .map(lower_char)
                .collect()
        })
        .collect();
    Block {
        seq,
        open,
        has_tools,
        ts,
        lines,
        links,
        plain,
    }
}

/// One turn, laid out to `width`.
///
/// A header names the speaker in its own colour — you in the accent, the agent
/// in its harness's hue — with the local time it was said. What you typed sits
/// behind a bar in that same accent, so a prompt is findable at a glance in a
/// page of replies. The agent's words are rendered as the markdown they were
/// written in, unless `raw` asks for the source; what the user typed is not,
/// since a prompt is rarely markdown on purpose and an asterisk in it should
/// stay one. Each tool call is one line under the text unless `open`, and a
/// tool's result keeps the colours its program printed it in (see
/// [`super::ansi`]).
fn turn_block(
    session: &Session,
    agent: &str,
    turn: &Turn,
    width: usize,
    raw: bool,
    open: bool,
) -> Block {
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut links = Vec::new();
    let ts = crate::util::parse_ts(&turn.ts);
    let has_tools = !turn.tools.is_empty();

    if turn.kind.as_ref() == "compaction" {
        // A seam, not something said — drawn as a rule so the eye reads it as
        // one rather than hunting for a speaker.
        let label = " compacted ";
        let side = width.saturating_sub(label.len()) / 2;
        out.push(Line::styled(
            format!("{}{label}{}", "─".repeat(side), "─".repeat(side)),
            theme::dim(),
        ));
        out.push(Line::default());
        return finish(Some(turn.seq), open, false, None, out, links);
    }

    let accent = Style::default()
        .fg(theme::colors().accent)
        .add_modifier(Modifier::BOLD);
    let reasoning = turn.kind.as_ref() == "reasoning";
    let (who, style) = match turn.role.as_ref() {
        "user" => ("you".to_string(), accent),
        "assistant" if reasoning => (
            format!("{agent} · thinking"),
            Style::default().fg(super::panels::provider_color(session)),
        ),
        "assistant" => (
            agent.to_string(),
            Style::default()
                .fg(super::panels::provider_color(session))
                .add_modifier(Modifier::BOLD),
        ),
        _ => ("system".to_string(), theme::dim()),
    };
    let mut header = vec![Span::styled(format!("● {who}"), style)];
    if let Some(ts) = ts {
        header.push(Span::styled(format!("  {}", clock(ts)), theme::dim()));
    }
    out.push(Line::from(header));

    let text_style = match reasoning {
        // The agent thinking out loud: kept dim, so the transcript's own
        // hierarchy survives the small screen.
        true => theme::dim().add_modifier(Modifier::ITALIC),
        false => theme::value(),
    };
    let body = width.saturating_sub(2).max(1);
    match turn.role.as_ref() {
        "user" => {
            let bar = Span::styled("▎ ", Style::default().fg(theme::colors().accent));
            for line in super::panels::wrap(&turn.text, body) {
                out.push(Line::from(vec![
                    bar.clone(),
                    Span::styled(line, text_style),
                ]));
            }
        }
        "assistant" if !raw => {
            // Rendered text sets its own weight, so the base is the ink
            // without the bold — or `**this**` would have nothing to stand
            // out from.
            let base = text_style.remove_modifier(Modifier::BOLD);
            let md = super::markdown::render(&turn.text, body, base);
            let offset = out.len();
            links.extend(md.links.into_iter().map(|mut link| {
                link.line += offset;
                link.columns = link.columns.start + 2..link.columns.end + 2;
                link
            }));
            out.extend(md.lines.into_iter().map(|mut line| {
                line.spans.insert(0, Span::raw("  "));
                line
            }));
        }
        _ => {
            for line in super::panels::wrap(&turn.text, body) {
                out.push(Line::styled(format!("  {line}"), text_style));
            }
        }
    }
    if turn.clipped {
        out.push(Line::styled(
            "  … cut here; the transcript has the rest",
            theme::dim(),
        ));
    }
    for tool in &turn.tools {
        tool_lines(tool, width, open, &mut out);
    }
    out.push(Line::default());
    finish(Some(turn.seq), open, has_tools, ts, out, links)
}

/// A tool call: one line when closed, with what opening it would show counted
/// at its end; its argument, result and diff under it when open.
fn tool_lines(tool: &ToolUse, width: usize, open: bool, out: &mut Vec<Line<'static>>) {
    let (mark, style) = match tool.failed {
        true => ("✗", theme::failed()),
        false => ("⚙", theme::dim()),
    };
    let more = tool.full.as_deref().map_or(0, |f| f.lines().count())
        + tool.result.as_deref().map_or(0, |r| r.lines().count())
        + tool.diff.len();
    let fold = match (more, open) {
        (0, _) => " ",
        (_, false) => "▸",
        (_, true) => "▾",
    };
    let counts = match (tool.added, tool.removed) {
        (0, 0) => String::new(),
        (a, r) => format!("  +{a} −{r}"),
    };
    let tail = match (more, open, &tool.result) {
        (_, _, None) => "  · running".to_string(),
        (0, _, _) | (_, true, _) => String::new(),
        (1, false, _) => "  · 1 line".to_string(),
        (n, false, _) => format!("  · {n} lines"),
    };
    let head = format!("  {fold} {mark} {}", tool.name);
    let room = width.saturating_sub(
        crate::util::cells(&head) + crate::util::cells(&counts) + crate::util::cells(&tail) + 2,
    );
    let detail = crate::util::truncate(
        super::ansi::strip(&tool.detail)
            .lines()
            .next()
            .unwrap_or(""),
        room,
    );
    out.push(Line::from(vec![
        Span::styled(head, style),
        Span::styled(format!("  {detail}"), theme::dim()),
        Span::styled(counts, theme::dim()),
        Span::styled(tail, theme::dim()),
    ]));
    if !open {
        return;
    }
    // The full argument only exists where it says more than the one-liner did
    // — a long command, a whole file body.
    if let Some(full) = &tool.full {
        out.extend(super::ansi::wrapped(full, theme::dim(), width, "      "));
    }
    if let Some(result) = &tool.result {
        out.extend(super::ansi::wrapped(result, theme::dim(), width, "      "));
    }
    for line in &tool.diff {
        let style = match line.starts_with('+') {
            true => theme::value(),
            false => theme::dim(),
        };
        out.push(Line::styled(
            format!("      {}", super::ansi::strip(line)),
            style,
        ));
    }
}

/// When a turn was said, in local time: the clock alone for today, the date
/// in front of it for anything older.
fn clock(ts: DateTime<Utc>) -> String {
    let local = ts.with_timezone(&Local);
    match local.date_naive() == Local::now().date_naive() {
        true => local.format("%H:%M").to_string(),
        false => local.format("%b %-d %H:%M").to_string(),
    }
}

/// One character lower-cased to one character, so a row's plain text and the
/// row itself stay the same length in characters. The rare character whose
/// lower case is two (`İ`) keeps the first, which is the letter a search for it
/// would type.
fn lower_char(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn lower(s: &str) -> Vec<char> {
    s.chars().map(lower_char).collect()
}

/// Where `query` occurs in `row`, as character ranges that do not overlap.
fn matches_in(row: &[char], query: &[char]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    if query.is_empty() || query.len() > row.len() {
        return out;
    }
    let mut i = 0;
    while i + query.len() <= row.len() {
        if row[i..i + query.len()] == *query {
            out.push(i..i + query.len());
            i += query.len();
        } else {
            i += 1;
        }
    }
    out
}

/// `line` with the characters in `ranges` drawn in `mark` over their own style
/// — spans split where a match starts or stops inside one.
fn highlight(
    line: &Line<'static>,
    ranges: &[std::ops::Range<usize>],
    mark: Style,
) -> Line<'static> {
    let mut spans = Vec::new();
    let mut at = 0usize;
    for span in &line.spans {
        let chars: Vec<char> = span.content.chars().collect();
        let mut run = String::new();
        let mut lit = None;
        for c in chars {
            let on = ranges.iter().any(|r| r.contains(&at));
            if lit != Some(on) && !run.is_empty() {
                let style = match lit == Some(true) {
                    true => span.style.patch(mark),
                    false => span.style,
                };
                spans.push(Span::styled(std::mem::take(&mut run), style));
            }
            lit = Some(on);
            run.push(c);
            at += 1;
        }
        if !run.is_empty() {
            let style = match lit == Some(true) {
                true => span.style.patch(mark),
                false => span.style,
            };
            spans.push(Span::styled(run, style));
        }
    }
    Line::from(spans).style(line.style)
}

/// The reader, over the whole screen.
///
/// Full-screen rather than a box over the table: it is read for minutes, not
/// glanced at, and every column it gives back to the dashboard is one a
/// wrapped reply is longer for.
pub(super) fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(view) = &mut app.chat else {
        return;
    };
    frame.render_widget(Clear, area);

    let title = match &view.session.remote {
        // The host is worth the title room: a remote conversation is otherwise
        // indistinguishable from a local one, and knowing which machine it was
        // read off is the whole difference.
        Some(remote) => format!(
            " {} — conversation · on {} ",
            view.session.display_label(),
            remote.host
        ),
        None => format!(" {} — conversation ", view.session.display_label()),
    };
    let inner = area.inner(Margin::new(1, 1));
    view.visible = inner.height as usize;
    // A space of padding either side, and no wider than prose reads.
    let width = (inner.width as usize).saturating_sub(2).clamp(1, MAX_TEXT);
    let column = Rect {
        x: inner.x + (inner.width.saturating_sub(width as u16)) / 2,
        width: width as u16,
        ..inner
    };

    let block = Border::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border_hi))
        .style(theme::canvas())
        .title(Span::styled(title, theme::title()))
        .title_bottom(footer(view));
    frame.render_widget(block, area);

    match (&view.conversation, &view.error) {
        (None, None) => {
            let msg = match view.host.is_some() {
                true => "  Reading it over ssh…",
                false => "  Reading the transcript…",
            };
            let line = Line::from(vec![
                Span::styled(format!("  {}", share::spinner_frame()), theme::title()),
                Span::styled(msg, theme::value()),
            ]);
            frame.render_widget(Paragraph::new(vec![Line::default(), line]), inner);
            return;
        }
        (None, Some(why)) => {
            let line = Line::styled(format!("  {why}"), theme::failed());
            frame.render_widget(Paragraph::new(vec![Line::default(), line]), inner);
            return;
        }
        (Some(_), _) => {}
    }

    lay_out(view, width);
    let Some(laid) = &view.laid else {
        return;
    };
    let top = view.top();
    let now = Utc::now();
    let query = lower(&view.search.query);
    let current = view
        .search
        .current
        .and_then(|i| view.search.hits.get(i))
        .copied();
    let mut rows = Vec::with_capacity(view.visible);
    let mut links = Vec::new();
    for (i, row) in (top..laid.total.min(top + view.visible)).enumerate() {
        let Some((block, at)) = laid.row(row) else {
            break;
        };
        let mut line = block.lines[at].clone();
        if at == 0
            && let Some(ts) = block.ts
        {
            line.spans.push(Span::styled(
                format!(" · {}", crate::util::relative_age(&ts.to_rfc3339(), &now)),
                theme::dim(),
            ));
        }
        if !query.is_empty() {
            let ranges = matches_in(&block.plain[at], &query);
            if !ranges.is_empty() {
                let mark = match Some(row) == current {
                    true => theme::key_cap(),
                    false => Style::default().add_modifier(Modifier::REVERSED),
                };
                line = highlight(&line, &ranges, mark);
            }
        }
        rows.push(line);
        links.extend(
            block
                .links
                .iter()
                .filter(|l| l.line == at)
                .map(|l| (i, l.columns.clone(), l.url.clone())),
        );
    }
    frame.render_widget(Paragraph::new(rows), column);
    super::scrollbar::on_border(frame, area, laid.total, view.visible, top);
    // Links are laid over the cells the paragraph has just drawn, so they have
    // to wait for it.
    for (row, columns, url) in links {
        let start = column.x + columns.start;
        let end = (column.x + columns.end).min(column.right());
        hyperlink::link(frame.buffer_mut(), column.y + row as u16, start..end, &url);
    }
}

/// The bottom border: the query while it is typed, and otherwise the keys.
fn footer(view: &ChatView) -> Line<'static> {
    let dim = theme::dim();
    let s = &view.search;
    if s.typing {
        let count = match (s.query.is_empty(), s.hits.len()) {
            (true, _) => String::new(),
            (false, 0) => "  no match".to_string(),
            (false, n) => format!("  {n} rows"),
        };
        return Line::from(vec![
            Span::styled(" /", theme::title()),
            Span::styled(format!("{}▏", s.query), theme::value()),
            Span::styled(format!("{count} · enter keep · esc cancel "), dim),
        ]);
    }
    let mut parts: Vec<String> = Vec::new();
    if !s.query.is_empty() {
        parts.push(match (s.current, s.hits.len()) {
            (_, 0) => format!("“{}” not found", s.query),
            (Some(i), n) => format!("“{}” {} of {n} · n N", s.query, i + 1),
            (None, n) => format!("“{}” {n} · n N", s.query),
        });
    }
    // What cannot be guessed, most useful first: the border cuts the rest off
    // on a narrow terminal, and scrolling keys are the ones nobody needs told.
    parts.push("/ search".into());
    parts.push(match view.tools_open {
        true => "t fold tools".into(),
        false => "↵ tools · t all".into(),
    });
    parts.push("[ ] turn".into());
    parts.push(match view.raw {
        true => "m rendered".into(),
        false => "m source".into(),
    });
    match &view.conversation {
        _ if view.fetching => parts.push(format!("{} loading earlier…", share::spinner_frame())),
        Some(Conversation { earlier, .. }) if *earlier > 0 => {
            parts.push(format!("u load {earlier} earlier"))
        }
        _ => {}
    }
    parts.push("q close".into());
    Line::styled(format!(" {} ", parts.join(" · ")), dim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve::chat::Conversation;

    fn turn(seq: usize, role: &'static str, text: &str) -> Turn {
        Turn {
            seq,
            role: role.into(),
            kind: "message".into(),
            ts: String::new(),
            text: text.into(),
            clipped: false,
            tools: Vec::new(),
        }
    }

    fn open_with(turns: Vec<Turn>) -> App {
        let mut app = crate::ui::tests::test_app();
        app.sessions = vec![crate::ui::tests::session("a", true, "/repo")];
        app.refilter();
        app.selected = 0;
        app.open_conversation();
        let key = app.chat.as_ref().expect("the view opened").session.key();
        let conv = Conversation {
            supported: true,
            turns,
            earlier: 0,
            note: None,
        };
        app.got_chat(key, None, Ok(Box::new(conv)));
        app
    }

    /// The view with one reply in markdown and one coloured tool result.
    fn chat_app(text: &str, result: &str) -> App {
        let mut reply = turn(1, "assistant", text);
        reply.tools.push(ToolUse {
            name: "Bash".into(),
            detail: "cargo test".into(),
            result: Some(result.into()),
            ..Default::default()
        });
        open_with(vec![turn(0, "user", "**keep** my stars"), reply])
    }

    fn draw_chat(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("backend");
        terminal
            .draw(|frame| draw(frame, frame.area(), app))
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    fn screen(app: &mut App, width: u16, height: u16) -> String {
        hyperlink::visible(&draw_chat(app, width, height))
    }

    fn press(app: &mut App, code: KeyCode) {
        app.on_key(crate::ui::tests::key(code));
    }

    fn typed(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    fn view(app: &App) -> &ChatView {
        app.chat.as_ref().expect("open")
    }

    /// The row just under the top border.
    fn first_row(screen: &str) -> String {
        screen.lines().nth(1).unwrap_or("").to_string()
    }

    /// A reply reads as rendered markdown with its link clickable, a user's
    /// prompt keeps its characters, and a coloured result, once its tool is
    /// opened, shows its colour and none of its escape codes.
    #[test]
    fn the_conversation_renders_replies_and_colours_results() {
        let mut app = chat_app(
            "## Done\n\nRan **all** of it, see [the log](https://example.com/log).",
            "\x1b[1m\x1b[32mtest result: ok\x1b[0m. 3 passed\x1b[2K",
        );
        let text = screen(&mut app, 100, 30);
        assert!(text.contains("Ran all of it, see the log."), "{text}");
        assert!(!text.contains("**all**") && !text.contains("## "), "{text}");
        assert!(
            text.contains("**keep** my stars"),
            "the prompt was rendered: {text}"
        );
        // Closed, the call is one line saying how much more there is.
        assert!(text.contains("▸ ⚙ Bash  cargo test  · 1 line"), "{text}");
        assert!(!text.contains("test result"), "{text}");

        press(&mut app, KeyCode::Char('t'));
        let buf = draw_chat(&mut app, 100, 30);
        let text = hyperlink::visible(&buf);
        assert!(text.contains("test result: ok. 3 passed"), "{text}");
        assert!(!text.contains("[32m") && !text.contains("[2K"), "{text}");
        let green = buf
            .content()
            .iter()
            .find(|c| c.symbol() == "t" && c.fg == ratatui::style::Color::Green);
        assert!(green.is_some(), "the result lost its colour");
        let targets: Vec<&str> = buf
            .content()
            .iter()
            .filter_map(|c| hyperlink::target_of(c.symbol()))
            .collect();
        assert_eq!(targets, ["https://example.com/log"]);

        // `m` shows the source instead, markers and all.
        press(&mut app, KeyCode::Char('m'));
        assert!(screen(&mut app, 100, 30).contains("Ran **all** of it"));
    }

    /// Enter opens the tools of the turn being read and no other, and the row
    /// that was at the top stays there while it does.
    #[test]
    fn enter_opens_one_turns_tools_in_place() {
        let tool = |detail: &str| ToolUse {
            name: "Read".into(),
            detail: detail.into(),
            result: Some("one\ntwo\nthree".into()),
            ..Default::default()
        };
        let mut first = turn(0, "assistant", "first");
        first.tools.push(tool("a.rs"));
        let mut second = turn(1, "assistant", "second");
        second.tools.push(tool("b.rs"));
        let filler = (0..30).map(|i| format!("line {i}\n\n")).collect::<String>();
        let mut app = open_with(vec![first, second, turn(2, "assistant", &filler)]);
        draw_chat(&mut app, 80, 20);
        press(&mut app, KeyCode::Char('g'));
        let before = first_row(&screen(&mut app, 80, 20));
        assert!(before.contains("Claude"), "{before}");

        press(&mut app, KeyCode::Enter);
        let text = screen(&mut app, 80, 20);
        assert_eq!(first_row(&text), before, "the view moved");
        assert!(text.contains("▾ ⚙ Read  a.rs"), "{text}");
        assert!(text.contains("▸ ⚙ Read  b.rs"), "{text}");
        assert!(text.contains("      three"), "{text}");

        press(&mut app, KeyCode::Enter);
        assert!(screen(&mut app, 80, 20).contains("▸ ⚙ Read  a.rs"));
    }

    /// `[` brings the previous turn's header to the top, `]` walks forward
    /// again, and past the last header is the end the reader opened on.
    #[test]
    fn brackets_step_through_the_turns() {
        let long = (0..40).map(|i| format!("line {i}\n\n")).collect::<String>();
        let mut app = open_with(vec![turn(0, "user", "go"), turn(1, "assistant", &long)]);
        draw_chat(&mut app, 80, 20);
        assert_eq!(view(&app).back, 0);

        press(&mut app, KeyCode::Char('['));
        let text = screen(&mut app, 80, 20);
        assert!(first_row(&text).contains("Claude"), "{text}");
        press(&mut app, KeyCode::Char('['));
        let text = screen(&mut app, 80, 20);
        assert!(first_row(&text).contains("you"), "{text}");
        assert_eq!(view(&app).top(), 0);

        press(&mut app, KeyCode::Char(']'));
        assert!(first_row(&screen(&mut app, 80, 20)).contains("Claude"));
        press(&mut app, KeyCode::Char(']'));
        assert_eq!(view(&app).back, 0);
    }

    /// `g` is the start and `G` the end; `k` from the end is one row back.
    #[test]
    fn g_and_shift_g_are_the_ends() {
        let long = (0..40).map(|i| format!("line {i}\n\n")).collect::<String>();
        let mut app = open_with(vec![turn(0, "user", "go"), turn(1, "assistant", &long)]);
        draw_chat(&mut app, 80, 20);
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(view(&app).top(), 0);
        press(&mut app, KeyCode::Char('G'));
        assert_eq!(view(&app).back, 0);
        // The turn's own gap is the last row, so two rows back is the first
        // that hides the reply's last line.
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(view(&app).back, 2);
        let text = screen(&mut app, 80, 20);
        assert!(!text.contains("line 39"), "{text}");
    }

    /// `/` finds a word in rendered text case-blind, highlights it, and `n`
    /// and `N` walk the rows it is on, wrapping at the ends.
    #[test]
    fn search_finds_walks_and_wraps() {
        let mut turns = vec![turn(0, "user", "where is the Needle?")];
        for seq in 1..30 {
            turns.push(turn(seq, "assistant", &format!("filler reply {seq}")));
        }
        turns.push(turn(30, "assistant", "Found the **needle** here."));
        let mut app = open_with(turns);
        draw_chat(&mut app, 80, 20);

        press(&mut app, KeyCode::Char('/'));
        typed(&mut app, "NEEDLE");
        let text = screen(&mut app, 80, 20);
        assert!(text.contains("/NEEDLE▏  2 rows"), "{text}");
        press(&mut app, KeyCode::Enter);
        assert!(!view(&app).search.typing);
        let hits = view(&app).search.hits.clone();
        assert_eq!(hits.len(), 2);

        // Typed from the end, so the first hit at or below it is none; the
        // search wraps to the top one.
        let first = view(&app).search.current;
        press(&mut app, KeyCode::Char('n'));
        let second = view(&app).search.current;
        assert_ne!(first, second);
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(view(&app).search.current, first, "n did not wrap");
        press(&mut app, KeyCode::Char('N'));
        assert_eq!(view(&app).search.current, second);

        // The hit landed on is on screen, and drawn in the key-cap style.
        let buf = draw_chat(&mut app, 80, 20);
        let text = hyperlink::visible(&buf);
        assert!(text.contains("the Needle?"), "{text}");
        let key_cap = theme::key_cap().bg.unwrap_or_default();
        let lit = buf.content().iter().filter(|c| c.bg == key_cap).count();
        assert_eq!(lit, "needle".len(), "the current hit is not highlighted");

        // Esc drops the search, then closes.
        press(&mut app, KeyCode::Esc);
        assert!(view(&app).search.query.is_empty());
        press(&mut app, KeyCode::Esc);
        assert!(app.chat.is_none());
    }

    /// Esc while typing puts the view back where `/` was pressed.
    #[test]
    fn cancelling_a_search_returns_to_where_it_started() {
        let long = (0..40).map(|i| format!("line {i}\n\n")).collect::<String>();
        let mut app = open_with(vec![
            turn(0, "user", "unique word"),
            turn(1, "assistant", &long),
        ]);
        draw_chat(&mut app, 80, 20);
        press(&mut app, KeyCode::Char('/'));
        typed(&mut app, "unique");
        assert_eq!(view(&app).top(), 0, "the search did not move to the hit");
        press(&mut app, KeyCode::Esc);
        assert_eq!(view(&app).back, 0);
        assert!(view(&app).search.query.is_empty());
    }

    /// Matching is by character, so a highlight splits spans at exactly the
    /// query's edges and keeps each piece's own style.
    #[test]
    fn a_highlight_splits_spans_at_the_match() {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let line = Line::from(vec![Span::raw("ab"), Span::styled("cdé", bold)]);
        let row = lower("abcdé");
        let ranges = matches_in(&row, &lower("BCD"));
        assert_eq!(ranges, vec![1..4]);
        let mark = Style::default().add_modifier(Modifier::REVERSED);
        let lit = highlight(&line, &ranges, mark);
        let pieces: Vec<(&str, bool)> = lit
            .spans
            .iter()
            .map(|s| {
                (
                    s.content.as_ref(),
                    s.style.add_modifier.contains(Modifier::REVERSED),
                )
            })
            .collect();
        assert_eq!(
            pieces,
            [("a", false), ("b", true), ("cd", true), ("é", false)]
        );
        assert!(lit.spans[2].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(matches_in(&lower("aaaa"), &lower("aa")), vec![0..2, 2..4]);
    }

    /// Frames at the same width reuse the layout rather than rendering every
    /// reply again, and a new width lays it out again with the same turn still
    /// at the top.
    #[test]
    fn the_layout_is_kept_until_the_width_changes() {
        let long = (0..40)
            .map(|i| format!("line {i} of a reply\n\n"))
            .collect::<String>();
        let mut app = open_with(vec![
            turn(0, "user", "go"),
            turn(1, "assistant", &long),
            turn(2, "assistant", &long),
        ]);
        draw_chat(&mut app, 80, 20);
        let ptr = |app: &App| {
            view(app).laid.as_ref().expect("laid").blocks[1]
                .lines
                .as_ptr()
        };
        let before = ptr(&app);
        press(&mut app, KeyCode::Char('j'));
        draw_chat(&mut app, 80, 20);
        assert_eq!(ptr(&app), before, "a scroll laid everything out again");

        // Onto the second reply's header, then narrower.
        press(&mut app, KeyCode::Char('['));
        let at = first_row(&screen(&mut app, 80, 20));
        let narrow = first_row(&screen(&mut app, 50, 20));
        assert!(
            at.contains("Claude") && narrow.contains("Claude"),
            "{narrow}"
        );
        assert_ne!(ptr(&app), before);
    }

    /// A few thousand turns lay out once and scroll without laying out again —
    /// the reader stays usable on the longest session a `u` can page in.
    #[test]
    fn a_huge_conversation_draws_only_what_is_shown() {
        let reply = "Some **markdown** with `code`, a [link](https://a.io) and\n\n- a list\n- of two\n\n```rs\nfn x() {}\n```";
        let turns = (0..3000)
            .map(|seq| turn(seq, ["user", "assistant"][seq % 2], reply))
            .collect();
        let mut app = open_with(turns);
        draw_chat(&mut app, 100, 40);
        let total = view(&app).laid_total();
        assert!(total > 3000 * 5, "{total}");
        let started = std::time::Instant::now();
        for _ in 0..50 {
            press(&mut app, KeyCode::PageUp);
            draw_chat(&mut app, 100, 40);
        }
        // Generous: an unoptimised test build on a loaded machine. Laying
        // everything out per frame is seconds here, not this.
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
    }
}
