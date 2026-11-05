//! The column down the right of an agent's tab: its plan, the files it has
//! changed, and your notes on it.
//!
//! A tab is where you watch one agent work, and the three things you keep
//! scrolling back up its terminal for are what it said it would do, what it
//! has touched, and what you meant to tell it. The dashboard's panels have the
//! first two for the selected row, but a tab hides the dashboard — so the tab
//! carries its own short copy, about the agent in its focused pane.
//!
//! `Alt+i` folds it to a strip of counts and back; the choice is kept across
//! runs, since it is about how you like your screen and not about one tab.
//! `Alt+e` writes a note. A pane that is not an agent — a shell — has no
//! sidebar, and a terminal too narrow to spare the columns has none either.
//!
//! The plan and the files are read off the worker, as the Changes panel's are,
//! and asked again only when the session has done something since: a frame
//! draws what was last answered and never parses a transcript.
//!
//! ponytail: an agent on another machine has no sidebar. Its transcript and its
//! checkout are there; the panels on the dashboard already say what crossed.

use super::line_edit::{Edit, LineEdit};
use super::{App, Mode, changes, theme};
use cctop_core::notes::NOTE_MAX;
use cctop_core::plan::{Status, Step};
use cctop_core::session::{Session, SessionData};
use cctop_core::util;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};
use std::path::PathBuf;

/// The sidebar open, borders included: room for a step or a path to be read.
const WIDE: u16 = 32;
/// Never more than this share of the tab, in percent: the agent is the point.
const SHARE: u16 = 35;
/// Folded: a border and three columns, enough for `3/5`.
const STRIP: u16 = 5;
/// What an agent keeps at the least. Below it the sidebar folds, and below
/// this plus the strip it is not drawn at all.
const MIN_PANE: u16 = 60;

/// A pane as the sidebar tells one from another: its tab, its place in it,
/// its agent's pid, and the rmux names it was resumed and is running under.
type PaneId = (usize, usize, u32, Option<String>, Option<String>);

/// What the sidebar knows, about the session in the focused pane.
#[derive(Debug, Default)]
pub struct State {
    /// The focused pane, as last resolved: tab, pane, agent pid and the rmux
    /// names it answers to. Resolving it walks every session, so it is redone
    /// only when this changes, not on every turn of the loop.
    pane: Option<PaneId>,
    /// The session the pane holds, by provider and id; empty while it holds
    /// none cctop has found — a freshly started agent has no transcript yet.
    provider: String,
    id: String,
    /// Whether the focused pane is an agent here, which is what shows the
    /// sidebar at all — before its session is found, so the agent is not
    /// resized a second time when it is.
    shown: bool,
    /// The session's activity stamp at the last ask.
    asked: String,
    in_flight: bool,
    pub plan: Vec<Step>,
    /// `None` until the first answer; an `Err` is a sentence to show.
    pub files: Option<Result<Vec<File>, String>>,
    /// The note being written: its index, or one past the last for a new one.
    pub note_at: usize,
    pub note_input: LineEdit,
}

/// One changed file, as the sidebar lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// Relative to the checkout, or to the session's directory.
    pub path: String,
    /// `None` for a binary file, which has no lines to count.
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

/// What the worker reads for the sidebar.
#[derive(Debug)]
pub struct Answer {
    pub plan: Vec<Step>,
    pub files: Result<Vec<File>, String>,
}

impl State {
    /// The session key the sidebar is about, when it has one.
    fn key(&self) -> Option<String> {
        (!self.id.is_empty()).then(|| format!("{}:{}", self.provider, self.id))
    }
}

/// Read a session's plan and changed files, on the worker.
///
/// The checkout's own diff when there is one, as the Changes panel shows it:
/// that is what is actually on disk. Outside a repository — or where the
/// checkout cannot be read — the transcript's edits stand in, which at least
/// name the files.
pub(super) fn read(
    data: &SessionData,
    dir: Result<PathBuf, String>,
    started: Option<i64>,
    cwd: &str,
) -> Answer {
    let details = &data.metrics.tool_details;
    let plan = cctop_core::plan::latest(details);
    let edited = || -> Vec<File> {
        let root = format!("{}/", cwd.trim_end_matches('/'));
        cctop_core::handoff::touched_files(details)
            .into_iter()
            .map(|t| File {
                path: t.path.strip_prefix(&root).unwrap_or(&t.path).to_string(),
                added: Some(t.added),
                removed: Some(t.removed),
            })
            .collect()
    };
    let files = match dir.map(|dir| cctop_core::changes::read(&dir, started)) {
        Ok(Ok(Some(changes))) => Ok(changes
            .files
            .into_iter()
            .map(|f| File {
                path: f.path,
                added: f.added,
                removed: f.removed,
            })
            .collect()),
        Ok(Ok(None)) => Ok(edited()),
        Ok(Err(why)) | Err(why) => match edited() {
            files if files.is_empty() => Err(why),
            files => Ok(files),
        },
    };
    Answer { plan, files }
}

impl App {
    /// Resolve the focused pane's session, and ask for its plan and files once
    /// it has done something since they were last read. Called every turn of
    /// the loop; cheap unless something moved.
    pub(super) fn sync_sidebar(&mut self) {
        let focused = self
            .tab
            .checked_sub(1)
            .filter(|_| !self.on_settings())
            .and_then(|at| {
                let tab = self.tabs.get(at)?;
                let pane = tab.panes.get(tab.focus)?;
                Some((at, tab.focus, pane))
            });
        let Some((at, focus, pane)) = focused else {
            self.sidebar.shown = false;
            self.sidebar.pane = None;
            return;
        };
        self.sidebar.shown = pane.is_agent() && pane.transport.is_none() && pane.dead.is_none();
        let identity = (
            at,
            focus,
            pane.agent(),
            pane.resumed.clone(),
            pane.rmux.clone(),
        );
        if self.sidebar.pane.as_ref() != Some(&identity) || self.sidebar.id.is_empty() {
            let found = self
                .sessions
                .iter()
                .find(|s| holds(pane, s))
                .map(|s| (s.provider.as_str().to_string(), s.session_id.clone()));
            let (provider, id) = found.unwrap_or_default();
            if id != self.sidebar.id || provider != self.sidebar.provider {
                self.sidebar = State {
                    provider,
                    id,
                    shown: self.sidebar.shown,
                    ..State::default()
                };
                self.needs_redraw = true;
            }
            self.sidebar.pane = Some(identity);
        }
        if !self.sidebar.shown || self.sidebar.in_flight || self.sidebar.id.is_empty() {
            return;
        }
        let (provider, id) = (&self.sidebar.provider, &self.sidebar.id);
        let Some(session) = self
            .sessions
            .iter()
            .find(|s| s.remote.is_none() && s.session_id == *id && s.provider.as_str() == provider)
        else {
            return;
        };
        let stamp = session.touched();
        if self.sidebar.asked == stamp {
            return;
        }
        self.sidebar.asked = stamp.to_string();
        self.sidebar.in_flight = true;
        let started = chrono::DateTime::parse_from_rfc3339(&session.started_at)
            .ok()
            .map(|t| t.timestamp());
        let _ = self.tx.send(super::worker::Request::Sidebar {
            key: session.key(),
            dir: changes::checkout_of(session),
            started,
            session: Box::new(session.clone()),
        });
    }

    /// The worker's answer, kept only while the sidebar is still on that
    /// session.
    pub(super) fn got_sidebar(&mut self, key: String, answer: Answer) {
        if self.sidebar.key().as_deref() != Some(key.as_str()) {
            return;
        }
        self.sidebar.in_flight = false;
        self.sidebar.plan = answer.plan;
        self.sidebar.files = Some(answer.files);
        self.needs_redraw = true;
    }

    /// `Alt+i`: fold the sidebar to its strip, or open it out.
    pub(super) fn toggle_sidebar(&mut self) {
        if !self.sidebar.shown {
            return self.set_status("The sidebar is for an agent's tab on this machine");
        }
        self.sidebar_folded = !self.sidebar_folded;
        self.save_prefs();
        self.needs_redraw = true;
    }

    /// `Alt+e`: write a new note on the focused pane's session.
    pub(super) fn edit_note(&mut self) {
        let Some(key) = self.sidebar.key().filter(|_| self.sidebar.shown) else {
            return self.set_status(match self.sidebar.shown {
                true => "No session to note yet — the agent has not written its transcript",
                false => "Notes are for an agent's tab on this machine",
            });
        };
        self.sidebar_folded = false;
        self.sidebar.note_at = self.notes.of(&key).len();
        self.sidebar.note_input.clear();
        self.mode = Mode::Note;
        self.needs_redraw = true;
    }

    /// Typing a note. Enter keeps it, and an emptied note is removed; ↑ and ↓
    /// step to the note above or below to change it instead.
    pub(super) fn on_key_note(&mut self, key: KeyEvent) {
        let Some(session) = self.sidebar.key() else {
            self.mode = Mode::List;
            return;
        };
        let count = self.notes.of(&session).len();
        let pick = |app: &mut App, at: usize| {
            app.sidebar.note_at = at;
            let text = app.notes.of(&session).get(at).cloned().unwrap_or_default();
            app.sidebar.note_input.set(text);
        };
        match key.code {
            KeyCode::Esc => self.mode = Mode::List,
            KeyCode::Up if self.sidebar.note_at > 0 => pick(self, self.sidebar.note_at - 1),
            KeyCode::Down if self.sidebar.note_at < count => pick(self, self.sidebar.note_at + 1),
            KeyCode::Enter => {
                self.mode = Mode::List;
                let text = self.sidebar.note_input.to_string();
                if let Err(e) = self.notes.set(&session, self.sidebar.note_at, &text) {
                    self.set_status(format!("Could not save the note: {e}"));
                }
            }
            _ => {
                if self.sidebar.note_input.key(key, NOTE_MAX) == Edit::Ignored {
                    return;
                }
            }
        }
        self.needs_redraw = true;
    }
}

/// Whether `pane` is where `session`'s agent runs: by the agent's pid, or by
/// the rmux name a resume or a launch gave it.
fn holds(pane: &super::tabs::Pane, session: &Session) -> bool {
    if session.remote.is_some() {
        return false;
    }
    if session.root_pid() == Some(pane.agent()) {
        return true;
    }
    let names = [pane.resumed.as_deref(), pane.rmux.as_deref()];
    if names.iter().all(Option::is_none) {
        return false;
    }
    let name = cctop_core::rmux::name_for_session(session.provider.as_str(), &session.session_id);
    names.contains(&Some(name.as_str()))
}

/// Split a tab's pane area into the panes and the sidebar beside them.
pub(super) fn split(app: &App, area: Rect) -> (Rect, Option<Rect>) {
    let width = match (app.sidebar.shown, app.sidebar_folded) {
        (false, _) => 0,
        _ if area.width < MIN_PANE + STRIP => 0,
        (true, true) => STRIP,
        (true, false) => {
            let wide = WIDE.min(area.width * SHARE / 100);
            match wide >= 24 && area.width >= MIN_PANE + wide {
                true => wide,
                false => STRIP,
            }
        }
    };
    super::render::set_sidebar_width(width);
    if width == 0 {
        return (area, None);
    }
    let panes = Rect {
        width: area.width - width,
        ..area
    };
    let side = Rect {
        x: area.x + panes.width,
        width,
        ..area
    };
    (panes, Some(side))
}

/// Draw the sidebar into `area`, folded or open as it was split.
pub(super) fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let folded = area.width <= STRIP;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border))
        .style(theme::canvas());
    let block = match folded {
        true => block.title(Span::styled("◂", theme::dim())),
        false => block.title(Span::styled(" Alt+i ", theme::dim())),
    };
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let key = app.sidebar.key();
    let notes = key.as_deref().map_or(&[][..], |k| app.notes.of(k));
    let lines = match folded {
        true => strip(&app.sidebar, notes.len()),
        false => {
            let editing =
                (app.mode == Mode::Note).then_some((&app.sidebar.note_input, app.sidebar.note_at));
            open(
                &app.sidebar,
                key.is_some(),
                notes,
                editing,
                inner.width as usize,
                inner.height as usize,
            )
        }
    };
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The folded strip: a count under each section's sign.
fn strip(state: &State, notes: usize) -> Vec<Line<'static>> {
    let plan = match state.plan.len() {
        0 => "·".to_string(),
        n => {
            let both = format!("{}/{n}", cctop_core::plan::done(&state.plan));
            match both.chars().count() <= 3 {
                true => both,
                // Too wide for the strip: what is left is the number that
                // matters at a glance.
                false => format!("{}", n - cctop_core::plan::done(&state.plan)),
            }
        }
    };
    let files = match &state.files {
        Some(Ok(files)) => files.len().to_string(),
        _ => "·".into(),
    };
    let notes = match notes {
        0 => "·".into(),
        n => n.to_string(),
    };
    let mut out = Vec::new();
    for (sign, count) in [("≡", plan), ("±", files), ("✎", notes)] {
        out.push(Line::from(Span::styled(sign, theme::dim())));
        out.push(Line::from(Span::styled(count, theme::value())));
        out.push(Line::default());
    }
    out
}

/// The open sidebar's lines, fitted to `height`: the notes whole, since they
/// are short and yours, and the plan and the files sharing what is left.
fn open(
    state: &State,
    found: bool,
    notes: &[String],
    editing: Option<(&LineEdit, usize)>,
    width: usize,
    height: usize,
) -> Vec<Line<'static>> {
    if !found {
        return wrap(
            "Waiting for this agent's session to appear — its plan, files and notes show here then.",
            width,
        )
        .into_iter()
        .map(|l| Line::from(Span::styled(l, theme::dim())))
        .collect();
    }
    let notes = notes_lines(notes, editing, width);
    let room = height.saturating_sub(notes.len() + 2);
    let plan = plan_lines(&state.plan, width);
    let files = files_lines(state.files.as_ref(), width);
    // The files are usually the longer list and the plan the one being read
    // top to bottom, so the plan takes what it needs of half, and the files
    // the rest.
    let plan_room = plan
        .len()
        .min(room.saturating_sub(files.len().min(room / 2)));
    let mut out = fit(plan, plan_room);
    out.push(Line::default());
    out.extend(fit(files, room.saturating_sub(plan_room)));
    out.push(Line::default());
    out.extend(notes);
    out
}

/// A section cut to `room` lines, its header kept and a count of what was cut
/// on the last line kept.
fn fit(mut lines: Vec<Line<'static>>, room: usize) -> Vec<Line<'static>> {
    if lines.len() <= room {
        return lines;
    }
    if room <= 1 {
        lines.truncate(room);
        return lines;
    }
    let cut = lines.len() - room + 1;
    lines.truncate(room - 1);
    lines.push(Line::from(Span::styled(
        format!("  … {cut} more"),
        theme::dim(),
    )));
    lines
}

fn header(name: &str, rest: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::styled(name.to_string(), theme::title())];
    spans.extend(rest);
    Line::from(spans)
}

fn plan_lines(plan: &[Step], width: usize) -> Vec<Line<'static>> {
    let done = cctop_core::plan::done(plan);
    let mut out = vec![header(
        "Plan",
        match plan.len() {
            0 => vec![],
            n => vec![Span::styled(format!("  {done}/{n}"), theme::dim())],
        },
    )];
    if plan.is_empty() {
        out.push(Line::from(Span::styled("  none recorded", theme::dim())));
        return out;
    }
    // A long plan most of the way through is mostly ticks: they fold into one
    // line, so the steps still ahead are what fits.
    let active = plan
        .iter()
        .position(|s| s.status != Status::Done)
        .unwrap_or(plan.len());
    let folded = active.saturating_sub(1);
    if folded > 1 {
        out.push(Line::from(Span::styled(
            format!("✓ {folded} done"),
            theme::dim(),
        )));
    }
    let text_width = width.saturating_sub(2);
    for step in &plan[if folded > 1 { folded } else { 0 }..] {
        let (mark, mark_style, text_style) = match step.status {
            Status::Done => ("✓", changes::plus(), theme::dim()),
            Status::Active => (
                "▶",
                Style::default().fg(theme::colors().accent),
                theme::value().add_modifier(Modifier::BOLD),
            ),
            Status::Pending => ("·", theme::dim(), theme::value()),
        };
        out.push(Line::from(vec![
            Span::styled(format!("{mark} "), mark_style),
            Span::styled(util::clip(&step.text, text_width), text_style),
        ]));
    }
    out
}

fn files_lines(files: Option<&Result<Vec<File>, String>>, width: usize) -> Vec<Line<'static>> {
    let list = match files {
        None => {
            return vec![
                header("Files", vec![]),
                Line::from(Span::styled("  reading…", theme::dim())),
            ];
        }
        Some(Err(why)) => {
            let mut out = vec![header("Files", vec![])];
            out.extend(
                wrap(why, width.saturating_sub(2))
                    .into_iter()
                    .map(|l| Line::from(Span::styled(format!("  {l}"), theme::dim()))),
            );
            return out;
        }
        Some(Ok(list)) => list,
    };
    let added: u64 = list.iter().filter_map(|f| f.added).sum();
    let removed: u64 = list.iter().filter_map(|f| f.removed).sum();
    let mut out = vec![header(
        "Files",
        match list.len() {
            0 => vec![],
            n => vec![
                Span::styled(format!("  {n}  "), theme::dim()),
                Span::styled(format!("+{added}"), changes::plus()),
                Span::styled(format!(" −{removed}"), changes::minus()),
            ],
        },
    )];
    if list.is_empty() {
        out.push(Line::from(Span::styled("  nothing changed", theme::dim())));
        return out;
    }
    for file in list {
        let counts = match (file.added, file.removed) {
            (Some(a), Some(r)) => vec![
                Span::styled(format!(" +{a}"), changes::plus()),
                Span::styled(format!(" −{r}"), changes::minus()),
            ],
            _ => vec![Span::styled(" bin", theme::dim())],
        };
        let used: usize = counts.iter().map(|s| util::cells(&s.content)).sum();
        let path_width = width.saturating_sub(used).max(4);
        let mut spans = vec![Span::styled(
            changes::truncate_left(&file.path, path_width),
            theme::value(),
        )];
        spans.extend(counts);
        out.push(Line::from(spans));
    }
    out
}

fn notes_lines(
    notes: &[String],
    editing: Option<(&LineEdit, usize)>,
    width: usize,
) -> Vec<Line<'static>> {
    let mut out = vec![header(
        "Notes",
        vec![Span::styled(
            match editing {
                Some(_) => "  ↵ keep · ↑↓ pick · Esc",
                None => "  Alt+e",
            },
            theme::dim(),
        )],
    )];
    let text_width = width.saturating_sub(2);
    let input = |field: &LineEdit| {
        let mut spans = vec![Span::styled(
            "› ",
            Style::default().fg(theme::colors().accent),
        )];
        spans.extend(field.spans(text_width, theme::value(), theme::selected(), "█"));
        Line::from(spans)
    };
    for (i, note) in notes.iter().enumerate() {
        match editing {
            Some((field, at)) if at == i => out.push(input(field)),
            _ => {
                for (n, line) in wrap(note, text_width).into_iter().enumerate() {
                    let lead = if n == 0 { "• " } else { "  " };
                    out.push(Line::from(vec![
                        Span::styled(lead, theme::dim()),
                        Span::styled(line, theme::value()),
                    ]));
                }
            }
        }
    }
    match editing {
        Some((field, at)) if at >= notes.len() => out.push(input(field)),
        None if notes.is_empty() => {
            out.push(Line::from(Span::styled("  none yet", theme::dim())));
        }
        _ => {}
    }
    out
}

/// Words onto lines no wider than `width` cells; a word wider than a line is
/// clipped rather than split.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let fits = util::cells(&line) + usize::from(!line.is_empty()) + util::cells(word) <= width;
        if !line.is_empty() && !fits {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&util::clip(word, width));
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn step(text: &str, status: Status) -> Step {
        Step {
            text: text.into(),
            status,
        }
    }

    #[test]
    fn a_long_plan_folds_its_finished_steps() {
        let mut plan: Vec<Step> = (0..6)
            .map(|i| step(&format!("s{i}"), Status::Done))
            .collect();
        plan.push(step("now", Status::Active));
        plan.push(step("next", Status::Pending));
        let shown = text(&plan_lines(&plan, 30));
        assert_eq!(shown, ["Plan  6/8", "✓ 5 done", "✓ s5", "▶ now", "· next"]);
    }

    #[test]
    fn the_sections_share_the_height_and_say_what_was_cut() {
        let state = State {
            plan: (0..10)
                .map(|i| step(&format!("p{i}"), Status::Pending))
                .collect(),
            files: Some(Ok((0..10)
                .map(|i| File {
                    path: format!("f{i}.rs"),
                    added: Some(1),
                    removed: Some(0),
                })
                .collect())),
            ..State::default()
        };
        let lines = text(&open(&state, true, &["remember".into()], None, 30, 16));
        assert!(lines.len() <= 16, "{lines:#?}");
        assert!(lines.iter().any(|l| l.contains("more")), "{lines:#?}");
        assert!(lines.iter().any(|l| l == "• remember"), "{lines:#?}");
    }

    #[test]
    fn the_strip_counts_what_the_open_sidebar_lists() {
        let state = State {
            plan: vec![step("a", Status::Done), step("b", Status::Pending)],
            files: Some(Ok(vec![])),
            ..State::default()
        };
        assert_eq!(
            text(&strip(&state, 2)),
            ["≡", "1/2", "", "±", "0", "", "✎", "2", ""]
        );
    }

    #[test]
    fn words_wrap_to_the_width() {
        assert_eq!(
            wrap("ask Sam about the expiry", 9),
            ["ask Sam", "about the", "expiry"]
        );
    }
}
