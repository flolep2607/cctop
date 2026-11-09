//! The recall view: what your agents said about something, as passages, from
//! the dashboard.
//!
//! The filter (`/`, with Tab) already searches inside transcripts — by words,
//! and by meaning once a sentence finds nothing literal — but what it answers
//! is *which rows*: the table narrows, and the selected row shows one matching
//! line. This answers the other question, the one `recall` answers for an
//! agent over MCP: *what was said*. Passages, ranked, from every session at
//! once, each with enough around it to tell whether it is the one — and Enter
//! opens the conversation at that passage rather than at its end.
//!
//! The ranking is not this file's. It is [`cctop_core::recall::recall_with`],
//! the function behind the MCP tool, run on the worker thread over the rows
//! the last walk found; this is the box, the list, and the two ways out of it.
//!
//! ponytail: this machine's sessions only. A remote row's transcript is on its
//! own machine, and asking each one over ssh per pause in the typing is a
//! different feature; `cctop recall` run there answers for that machine.

use super::line_edit::LineEdit;
use super::reader;
use super::theme;
use super::{App, ChatView, Mode, Row};
use cctop_core::recall::{Passage, Recall, words_of};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use std::ops::Range;
use std::time::{Duration, Instant};

/// Passages asked for. More than the agent's eight: a person scrolls a list
/// rather than paying for every passage in tokens, and the one they are after
/// is often the fifteenth mention of a word rather than the first.
pub(super) const LIMIT: usize = 20;

/// How long the typing has to pause before the query is asked. The same as
/// the filter's transcript scan, for the same reason: every ask reads
/// transcripts, and one per keystroke would spend the worker on prefixes.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// Shorter than this is not asked: two letters start a word in nearly every
/// passage on the machine.
const MIN_CHARS: usize = 3;

/// The longest query the box takes.
pub(super) const QUERY_MAX: usize = 200;

/// Rows of snippet under each passage's heading.
const SNIPPET_ROWS: usize = 2;

/// The widest the box gets, however wide the screen: past this the snippets
/// are long lines read by moving the head.
const MAX_WIDTH: u16 = 110;

/// The view's state, kept while the reader is open over it so `Esc` there
/// comes back to the same list.
#[derive(Default)]
pub struct RecallView {
    pub query: LineEdit,
    /// When the query last changed, for the debounce.
    typed_at: Option<Instant>,
    /// The query out with the worker, not answered yet.
    pending: Option<String>,
    /// The last answer — for `answer.query`, which may be behind the box
    /// while a newer one is out.
    pub answer: Option<Recall>,
    pub selected: usize,
}

impl RecallView {
    /// The query as it is asked: what is in the box, less the spaces at its
    /// ends, which change nothing about the answer.
    fn asking(&self) -> &str {
        self.query.trim()
    }

    /// The query to send now, if one is due — and, as it is sent, noted as
    /// out.
    ///
    /// Asked every pass of the loop rather than on each key, so a burst of
    /// typing costs one search, and one already out holds the next back
    /// until it lands: the worker answers in order, and a queue of stale
    /// queries is time the current one waits behind.
    pub(super) fn due(&mut self, now: Instant) -> Option<String> {
        if self.pending.is_some() {
            return None;
        }
        let q = self.asking().to_string();
        if q.chars().count() < MIN_CHARS {
            // Deleting back below the minimum drops the list with it, rather
            // than leaving passages on screen for a query no longer asked.
            self.answer = None;
            self.typed_at = None;
            return None;
        }
        if self.answer.as_ref().is_some_and(|a| a.query == q) {
            self.typed_at = None;
            return None;
        }
        if self
            .typed_at
            .is_some_and(|at| now.duration_since(at) < DEBOUNCE)
        {
            return None;
        }
        self.typed_at = None;
        self.pending = Some(q.clone());
        Some(q)
    }

    /// Fold in an answer. One for a query since edited is still shown — it is
    /// closer to what is wanted than an empty list — and [`Self::due`] sends
    /// the current one on the next pass.
    pub(super) fn answered(&mut self, answer: Recall) {
        self.pending = None;
        let same = self
            .answer
            .as_ref()
            .is_some_and(|a| a.query == answer.query);
        if !same {
            self.selected = 0;
        }
        self.selected = self.selected.min(answer.passages.len().saturating_sub(1));
        self.answer = Some(answer);
    }

    pub(super) fn searching(&self) -> bool {
        self.pending.is_some()
    }

    fn passages(&self) -> &[Passage] {
        self.answer.as_ref().map_or(&[], |a| a.passages.as_slice())
    }

    fn selected_passage(&self) -> Option<&Passage> {
        self.passages().get(self.selected)
    }

    fn step(&mut self, by: isize) {
        let n = self.passages().len();
        if n == 0 {
            return;
        }
        self.selected = self.selected.saturating_add_signed(by).min(n - 1);
    }

    pub(super) fn typed(&mut self) {
        self.typed_at = Some(Instant::now());
    }
}

/// The session a passage is from, as [`cctop_core::session::Session::key`]
/// spells it.
fn key_of(p: &Passage) -> String {
    format!("{}:{}", p.provider, p.session_id)
}

/// Where the reader should land for a passage: the query word to search for,
/// and a phrase from the passage that pins which turn it is in.
///
/// Two, because neither does it alone. The word is what the reader highlights
/// and what `n` steps through, but it may be said a hundred times before the
/// passage. The phrase is a stretch of the passage's own text around that
/// word — the start of the passage when no word matched, as for a passage
/// found by meaning — and finds the turn; the reader lands on the word's first
/// hit inside that turn, or on the turn itself when there is none.
///
/// The phrase is cut from one line, so it never spans the join between two
/// turns, and whitespace-folded and lower-cased as the reader folds a turn's
/// text to look for it.
pub(super) fn landing(query: &str, text: &str) -> (String, String) {
    const ANCHOR: usize = 48;
    let words = words_of(query);
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim_matches(|c: char| c == '…' || c.is_whitespace()))
        .filter(|l| !l.is_empty())
        .collect();
    let found = lines.iter().find_map(|line| {
        let folded = fold(line);
        let chars: Vec<char> = folded.chars().collect();
        let hit = words.iter().find_map(|w| {
            let w: Vec<char> = w.chars().collect();
            marks(&chars, std::slice::from_ref(&w))
                .first()
                .map(|r| (w.iter().collect::<String>(), r.start))
        })?;
        Some((hit, chars))
    });
    match found {
        Some(((word, at), chars)) => {
            let from = at.saturating_sub(ANCHOR / 3);
            let anchor: String = chars.iter().skip(from).take(ANCHOR).collect();
            (word, anchor.trim().to_string())
        }
        None => {
            let first = lines.first().map(|l| fold(l)).unwrap_or_default();
            let anchor: String = first.chars().take(ANCHOR).collect();
            (String::new(), anchor.trim().to_string())
        }
    }
}

/// Lower-cased, every run of whitespace one space — the form a passage's
/// phrase and a turn's text are compared in, since a passage is the turn's
/// text re-joined and an excerpt of it re-trimmed.
pub(super) fn fold(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Where any of `words` starts a word in `row`: the same rule recall ranks by
/// — at the start of a word, so `rent` is marked in `rented` but not in
/// `current`.
fn marks(row: &[char], words: &[Vec<char>]) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    for i in 0..row.len() {
        if i > 0 && row[i - 1].is_alphanumeric() {
            continue;
        }
        if out.last().is_some_and(|r| r.end > i) {
            continue;
        }
        if let Some(w) = words
            .iter()
            .filter(|w| !w.is_empty() && row[i..].starts_with(w))
            .max_by_key(|w| w.len())
        {
            out.push(i..i + w.len());
        }
    }
    out
}

/// A passage's text as up to `rows` lines of `width`, starting a little before
/// its first marked word so the reason it came back is on screen.
fn snippet(text: &str, words: &[Vec<char>], width: usize, rows: usize) -> Vec<String> {
    let flat: Vec<char> = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches('…')
        .trim()
        .chars()
        .collect();
    let lower: Vec<char> = flat.iter().flat_map(|c| c.to_lowercase()).collect();
    // Lower-casing can lengthen a character (`İ`), and then positions in the
    // two disagree; such text simply starts from its beginning.
    let first = match lower.len() == flat.len() {
        true => marks(&lower, words).first().map_or(0, |r| r.start),
        false => 0,
    };
    let mut start = first.saturating_sub(width / 4);
    // Back to the start of the word the window would otherwise cut.
    while start > 0 && start < first && flat[start - 1] != ' ' {
        start -= 1;
        if first - start > width / 2 {
            break;
        }
    }
    let mut body: String = flat[start..].iter().collect();
    // Recall cut the passage out of a longer chunk, and said so with `…`.
    if start > 0 || text.trim_start().starts_with('…') {
        body.insert(0, '…');
    }
    let mut lines = cctop_core::util::wrap(&body, width);
    if lines.len() > rows {
        lines.truncate(rows);
        if let Some(last) = lines.last_mut() {
            *last = cctop_core::util::truncate(&format!("{last} …"), width);
        }
    }
    lines
}

/// `line` with each query word in it marked.
fn marked(text: &str, base: Style, words: &[Vec<char>]) -> Vec<Span<'static>> {
    let lower: Vec<char> = text.chars().flat_map(|c| c.to_lowercase()).collect();
    let line = Line::from(Span::styled(text.to_string(), base));
    if lower.len() != text.chars().count() {
        return line.spans;
    }
    let ranges = marks(&lower, words);
    let mark = Style::default()
        .fg(theme::colors().accent)
        .add_modifier(Modifier::BOLD);
    reader::highlight(&line, &ranges, mark).spans
}

impl App {
    /// `H`: open the view, with the last query still in it if there was one
    /// this run — a second look usually refines the first.
    pub(super) fn open_recall(&mut self) {
        if self.recall.is_none() {
            self.recall = Some(RecallView::default());
        }
        self.mode = Mode::Recall;
        self.needs_redraw = true;
    }

    fn close_recall(&mut self) {
        self.recall = None;
        self.mode = Mode::List;
    }

    /// Send the query once the typing has settled. Called every pass of the
    /// loop, like the filter's [`App::tick_scan`].
    pub(super) fn tick_recall(&mut self) {
        if self.mode != Mode::Recall {
            return;
        }
        let Some(view) = self.recall.as_mut() else {
            return;
        };
        let had = view.answer.is_some();
        match view.due(Instant::now()) {
            Some(query) => {
                let _ = self.tx.send(super::worker::Request::Recall { query });
                self.needs_redraw = true;
            }
            None if had && view.answer.is_none() => self.needs_redraw = true,
            None => {}
        }
    }

    /// A worker's answer, for the view if it is still open.
    pub(super) fn recalled(&mut self, answer: Recall) {
        if let Some(view) = self.recall.as_mut() {
            view.answered(answer);
            self.needs_redraw = true;
        }
    }

    /// Whether the view is waiting on the worker — the loop keeps waking for
    /// the spinner while it is.
    pub(crate) fn recall_loading(&self) -> bool {
        self.mode == Mode::Recall && self.recall.as_ref().is_some_and(RecallView::searching)
    }

    pub(super) fn on_key_recall(&mut self, key: KeyEvent) {
        self.needs_redraw = true;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(view) = self.recall.as_mut() else {
            self.mode = Mode::List;
            return;
        };
        match key.code {
            KeyCode::Esc => self.close_recall(),
            KeyCode::Enter => self.recall_read(),
            // `R` on a row resumes; here letters are the query's, so the same
            // letter under Ctrl.
            KeyCode::Char('r') if ctrl => self.recall_resume(),
            KeyCode::Up => view.step(-1),
            KeyCode::Down => view.step(1),
            KeyCode::Char('p') if ctrl => view.step(-1),
            KeyCode::Char('n') if ctrl => view.step(1),
            KeyCode::PageUp => view.step(-5),
            KeyCode::PageDown => view.step(5),
            _ => {
                if view.query.key(key, QUERY_MAX).changed() {
                    view.typed();
                }
            }
        }
    }

    /// The wheel moves through the list; nothing under the view hears it.
    pub(super) fn on_wheel_recall(&mut self, up: bool) {
        if let Some(view) = self.recall.as_mut() {
            view.step(if up { -1 } else { 1 });
            self.needs_redraw = true;
        }
    }

    /// Enter: the passage's conversation in the reader, at the passage.
    ///
    /// By the session itself, not the table's selection: a passage can come
    /// from a row the filter is hiding, and reading it should not need the
    /// filter cleared. The view stays behind it, for `Esc` to come back to.
    fn recall_read(&mut self) {
        let Some(view) = self.recall.as_ref() else {
            return;
        };
        let Some(p) = view.selected_passage() else {
            return;
        };
        let key = key_of(p);
        let query = view.answer.as_ref().map_or("", |a| a.query.as_str());
        let (word, anchor) = landing(query, &p.text);
        let Some(session) = self.sessions.iter().find(|s| s.key() == key).cloned() else {
            self.set_status("That session is no longer on this machine's table");
            return;
        };
        let mut chat = ChatView::new(session, None);
        chat.search = reader::Search::for_passage(&word, &anchor);
        self.chat = Some(chat);
        self.mode = Mode::Conversation;
        self.fetch_chat(None);
    }

    /// Ctrl+R: resume the passage's session in a tab, as `R` does on its row
    /// — which is what this does, having put the cursor on that row, so every
    /// refusal and confirmation `R` has applies here too.
    fn recall_resume(&mut self) {
        let Some(key) = self
            .recall
            .as_ref()
            .and_then(RecallView::selected_passage)
            .map(key_of)
        else {
            return;
        };
        self.reveal(&key);
        let row = self
            .visible
            .iter()
            .position(|&r| matches!(r, Row::Session(i) if self.sessions[i].key() == key));
        match row {
            Some(row) => {
                self.selected = row;
                self.ensure_available_tab();
                self.close_recall();
                self.resume_selected();
            }
            None => self.set_status(
                "That session is hidden by the table's filter — Enter reads it, or clear the filter to resume it",
            ),
        }
    }
}

/// The view: the query box, a line on what was searched, and the passages.
pub(super) fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(view) = app.recall.as_ref() else {
        return;
    };
    let width = area.width.saturating_sub(4).min(MAX_WIDTH);
    let height = area.height.saturating_sub(2);
    let rect = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border_hi))
        .style(theme::canvas())
        .title(Span::styled(
            " Recall — what your agents said ",
            theme::title(),
        ));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let text_w = (inner.width as usize).saturating_sub(4).max(10);

    let mut head: Vec<Line<'static>> = Vec::new();
    let (text, cursor) = (
        Style::default()
            .fg(theme::colors().value)
            .add_modifier(Modifier::BOLD),
        Style::default().fg(theme::colors().accent),
    );
    let mut input = vec![Span::raw(" > ")];
    input.extend(
        view.query
            .spans(inner.width.saturating_sub(4) as usize, text, cursor, "█"),
    );
    head.push(Line::from(input));
    head.extend(status_lines(view, text_w));
    head.push(Line::default());

    let foot = Line::from(Span::styled(
        " Enter read it there   Ctrl+R resume in a tab   ↑↓ pick   Esc close",
        theme::dim(),
    ));

    let list_rows = (inner.height as usize).saturating_sub(head.len() + 1);
    let words: Vec<Vec<char>> = view
        .answer
        .as_ref()
        .map(|a| words_of(&a.query))
        .unwrap_or_default()
        .iter()
        .map(|w| w.chars().collect())
        .collect();
    // Each passage is a heading and its snippet; the list scrolls by whole
    // passages, far enough that the selected one is on screen.
    let per = 1 + SNIPPET_ROWS;
    let fit = (list_rows / per).max(1);
    let first = view.selected.saturating_sub(fit - 1);
    let mut list: Vec<Line<'static>> = Vec::new();
    for (i, p) in view.passages().iter().enumerate().skip(first).take(fit) {
        let on = i == view.selected;
        list.extend(passage_lines(p, on, &words, text_w));
    }
    list.truncate(list_rows);

    let mut lines = head;
    lines.extend(list);
    let used = lines.len();
    lines.extend(std::iter::repeat_n(
        Line::default(),
        (inner.height as usize).saturating_sub(used + 1),
    ));
    lines.push(foot);
    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), inner);
}

/// What was searched and how: the count, which tiers took part, and — when
/// the meaning half could not — why, with the command that fixes it.
fn status_lines(view: &RecallView, width: usize) -> Vec<Line<'static>> {
    let accent = Style::default().fg(theme::colors().accent);
    let (marker, style, text) = match (&view.answer, view.searching()) {
        (_, true) => ("◌", accent, "Searching every transcript…".to_string()),
        (None, false) if view.asking().chars().count() < MIN_CHARS => (
            "○",
            theme::dim(),
            "Ask anything — a name, an error, or what you were trying to decide".to_string(),
        ),
        (None, false) => ("◌", accent, "Searching every transcript…".to_string()),
        (Some(a), false) => {
            let found = match a.passages.len() {
                0 => "Nothing matches".to_string(),
                n => format!(
                    "{} from {}",
                    cctop_core::util::plural(n, "passage"),
                    cctop_core::util::plural(a.sessions_matched, "session")
                ),
            };
            match a.topical {
                true => (
                    "●",
                    Style::default().fg(theme::colors().cost_low),
                    format!("{found} · by words and meaning"),
                ),
                false => (
                    "●",
                    Style::default().fg(theme::colors().cost_mid),
                    format!("{found} · by words only"),
                ),
            }
        }
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(format!(" {marker} "), style),
        Span::styled(cctop_core::util::truncate(&text, width), style),
    ])];
    // The fallback says so, and says the way out of it, on a line of its own
    // so a narrow screen does not cut the command off: a person who sees only
    // literal matches should know there is more to have.
    if view.answer.as_ref().is_some_and(|a| !a.topical) {
        lines.push(Line::from(Span::styled(
            cctop_core::util::truncate(
                "   Matching by meaning needs the search model: cctop --fetch-search-model",
                width + 3,
            ),
            theme::dim(),
        )));
    }
    lines
}

/// One passage: agent, directory, day and title on a heading, then the
/// snippet with the query's words marked.
fn passage_lines(p: &Passage, on: bool, words: &[Vec<char>], width: usize) -> Vec<Line<'static>> {
    let day = p
        .last_active
        .get(..10)
        .unwrap_or(&p.last_active)
        .to_string();
    let title = p.title.as_deref().unwrap_or("untitled");
    let heading = format!(
        "{} · {} · {} · {}",
        p.harness,
        cctop_core::util::tildify(&p.directory),
        day,
        title
    );
    let (pointer, style) = match on {
        true => ("▸ ", theme::selected()),
        false => ("  ", Style::default().fg(theme::colors().value)),
    };
    let mut out = vec![Line::from(vec![
        Span::styled(format!(" {pointer}"), style),
        Span::styled(cctop_core::util::truncate(&heading, width), style),
    ])];
    let body = snippet(&p.text, words, width.saturating_sub(2), SNIPPET_ROWS);
    for i in 0..SNIPPET_ROWS {
        let mut spans = vec![Span::raw("     ")];
        if let Some(line) = body.get(i) {
            spans.extend(marked(line, theme::dim(), words));
        }
        out.push(Line::from(spans));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passage(id: &str, text: &str) -> Passage {
        Passage {
            session_id: id.into(),
            harness: "claude".into(),
            provider: "claude".into(),
            directory: "/nonexistent/web".into(),
            title: Some("Cache layout".into()),
            last_active: "2026-10-01T10:00:00Z".into(),
            passage: Some(0),
            text: text.into(),
            matched: "words",
        }
    }

    fn answer(query: &str, passages: Vec<Passage>) -> Recall {
        Recall {
            query: query.into(),
            sessions_matched: passages.len(),
            passages,
            topical: false,
        }
    }

    /// Typing is asked once it pauses, not per key; a query already answered
    /// is not asked again; one out holds the next back.
    #[test]
    fn a_query_is_asked_once_the_typing_settles() {
        let mut view = RecallView::default();
        let t0 = Instant::now();
        view.query = "sharded cache".into();
        view.typed_at = Some(t0);
        assert_eq!(view.due(t0), None, "still typing");
        assert_eq!(view.due(t0 + DEBOUNCE), Some("sharded cache".into()));
        assert!(view.searching());
        view.query = "sharded cache layout".into();
        view.typed_at = Some(t0);
        assert_eq!(
            view.due(t0 + DEBOUNCE * 2),
            None,
            "one is already out with the worker"
        );
        view.answered(answer("sharded cache", vec![passage("a", "x")]));
        assert!(!view.searching());
        assert_eq!(
            view.due(t0 + DEBOUNCE * 2),
            Some("sharded cache layout".into()),
            "the stale answer is shown and the current query sent"
        );
        view.answered(answer("sharded cache layout", vec![]));
        assert_eq!(view.due(t0 + DEBOUNCE * 3), None, "answered already");
    }

    #[test]
    fn a_short_query_is_not_asked_and_drops_the_list() {
        let mut view = RecallView {
            query: "ab".into(),
            ..Default::default()
        };
        assert_eq!(view.due(Instant::now()), None);
        view.answer = Some(answer("abc", vec![passage("a", "abc")]));
        assert_eq!(view.due(Instant::now()), None);
        assert!(view.answer.is_none());
        view.query = "  ab  ".into();
        assert_eq!(view.due(Instant::now()), None, "spaces are not letters");
    }

    #[test]
    fn the_selection_stays_on_the_list() {
        let mut view = RecallView::default();
        view.step(1);
        assert_eq!(view.selected, 0, "nothing to move through");
        view.answered(answer(
            "q",
            vec![passage("a", "1"), passage("b", "2"), passage("c", "3")],
        ));
        view.step(5);
        assert_eq!(view.selected, 2);
        view.step(-1);
        assert_eq!(view.selected, 1);
        // A refreshed answer to the same query keeps the place; a new query
        // starts at the top, clamped to what came back.
        view.answered(answer("q", vec![passage("a", "1"), passage("b", "2")]));
        assert_eq!(view.selected, 1);
        view.answered(answer("other", vec![passage("a", "1"), passage("b", "2")]));
        assert_eq!(view.selected, 0);
        assert_eq!(
            view.selected_passage().map(key_of).as_deref(),
            Some("claude:a")
        );
    }

    /// The reader is pointed at the word and at the turn the passage is in.
    #[test]
    fn landing_takes_the_query_word_and_a_phrase_around_it() {
        let text = "…opening of the chunk\nWe chose a Sharded   cache because the single file grew past 120 MB.\nlater";
        let (word, anchor) = landing("why is the cache sharded", text);
        assert_eq!(word, "cache", "the first query word the passage has");
        assert!(anchor.contains("sharded cache because"), "{anchor:?}");
        assert_eq!(anchor, fold(&anchor), "folded as turns are");
        assert!(
            fold("We chose a Sharded cache because the single file grew past 120 MB.")
                .contains(&anchor)
        );

        // A passage found by meaning has none of the words: it lands on its
        // own opening, with nothing to highlight.
        let (word, anchor) = landing("renting gpus", "…The H100 hourly price on vast…");
        assert_eq!(word, "");
        assert_eq!(anchor, "the h100 hourly price on vast");
    }

    #[test]
    fn words_are_marked_where_they_start_a_word() {
        let row: Vec<char> = "the current rent, rented".chars().collect();
        let words = vec!["rent".chars().collect::<Vec<char>>()];
        assert_eq!(marks(&row, &words), vec![12..16, 18..22]);
    }

    #[test]
    fn the_snippet_starts_near_the_first_match() {
        let text = format!("{} the sharded cache was chosen", "filler ".repeat(40));
        let words = vec!["sharded".chars().collect::<Vec<char>>()];
        let lines = snippet(&text, &words, 40, 2);
        assert!(lines[0].starts_with('…'), "{lines:?}");
        assert!(lines.join(" ").contains("sharded cache"), "{lines:?}");
        assert!(lines.len() <= 2);
    }
}
