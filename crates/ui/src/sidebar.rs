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
use std::collections::HashSet;
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
/// The narrowest a dragged sidebar stays open; dragged narrower, it folds.
const OPEN_MIN: u16 = 24;
/// The most of the tab a dragged sidebar may take, in percent — more room
/// than the default gives, for a long plan or a note, at the person's choice.
const DRAG_SHARE: u16 = 60;
/// What a dragged sidebar always leaves the agent.
const AGENT_MIN: u16 = 40;
/// Two presses on the border this close together are a double-click.
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);

/// A dragged width as the sidebar can have it in a tab `tab` columns wide:
/// between the narrowest open one and [`DRAG_SHARE`] of the tab, leaving the
/// agent [`AGENT_MIN`]. The strip when even the narrowest does not fit.
pub(super) fn clamp_width(chosen: u16, tab: u16) -> u16 {
    let most = (tab * DRAG_SHARE / 100).min(tab.saturating_sub(AGENT_MIN));
    match most >= OPEN_MIN {
        true => chosen.clamp(OPEN_MIN, most),
        false => STRIP,
    }
}

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
    /// See [`Answer::checkout`].
    pub checkout: Option<(PathBuf, String)>,
    /// The note being written: its index, or one past the last for a new one.
    pub note_at: usize,
    pub note_input: LineEdit,
    /// How many rows of the Files tree the wheel has scrolled past.
    pub files_scroll: usize,
    /// The session the note being written is about, fixed when `Alt+e` was
    /// pressed. The sidebar follows the focus, and a note saved against
    /// whatever it shows at Enter could replace another session's.
    note_key: Option<String>,
}

/// One changed file, as the sidebar lists it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct File {
    /// Relative to the checkout, or to the session's directory.
    pub path: String,
    /// `None` for a binary file, which has no lines to count.
    pub added: Option<u64>,
    pub removed: Option<u64>,
    /// Never added to git: its diff is the whole file, read from disk.
    pub untracked: bool,
}

/// What the worker reads for the sidebar.
#[derive(Debug)]
pub struct Answer {
    pub plan: Vec<Step>,
    pub files: Result<Vec<File>, String>,
    /// The checkout the files were read from and the commit they are against,
    /// which is what a click on one diffs; `None` when they came from the
    /// transcript, which has no checkout to diff.
    pub checkout: Option<(PathBuf, String)>,
}

impl State {
    /// Whether the sidebar is drawn: the tab holds an agent here.
    pub(super) fn shown(&self) -> bool {
        self.shown
    }

    /// The session key the sidebar is about, when it has one.
    fn key(&self) -> Option<String> {
        (!self.id.is_empty()).then(|| format!("{}:{}", self.provider, self.id))
    }
}

/// A total at or under which every folder starts open: a short list reads
/// best whole.
const ALL_OPEN: usize = 15;
/// Past [`ALL_OPEN`], a top-level folder with more than this many files starts
/// folded, so the tree opens as an outline of where the work is.
const FEW: usize = 5;

/// A directory of changed files, as the sidebar's tree shows it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Dir {
    /// As shown: one name, or a chain of directories each holding only the
    /// next — `docs/explanation` — joined, so a deep path is one row.
    pub name: String,
    /// The path from the checkout, which is what its fold is kept by.
    pub path: String,
    pub dirs: Vec<Dir>,
    /// Its own files: name, lines added and removed (`None` when binary).
    pub files: Vec<(String, Option<u64>, Option<u64>)>,
    /// Every file under it, and their lines added and removed.
    pub count: usize,
    pub added: u64,
    pub removed: u64,
}

/// The changed files as a tree of directories, each sorted by name, with
/// chains of single-child directories joined into one.
pub(super) fn tree(files: &[File]) -> Dir {
    #[derive(Default)]
    struct Raw {
        dirs: std::collections::BTreeMap<String, Raw>,
        files: std::collections::BTreeMap<String, (Option<u64>, Option<u64>)>,
    }
    fn finish(mut raw: Raw, mut name: String, mut path: String) -> Dir {
        // The root is never joined to its only child: it has no row to join.
        while !path.is_empty() && raw.files.is_empty() && raw.dirs.len() == 1 {
            let Some((next, child)) = std::mem::take(&mut raw.dirs).into_iter().next() else {
                break;
            };
            name = format!("{name}/{next}");
            path = format!("{path}/{next}");
            raw = child;
        }
        let dirs: Vec<Dir> = std::mem::take(&mut raw.dirs)
            .into_iter()
            .map(|(child, sub)| {
                let at = match path.is_empty() {
                    true => child.clone(),
                    false => format!("{path}/{child}"),
                };
                finish(sub, child, at)
            })
            .collect();
        let files: Vec<_> = raw.files.into_iter().map(|(n, (a, r))| (n, a, r)).collect();
        let count = files.len() + dirs.iter().map(|d| d.count).sum::<usize>();
        let added = files.iter().filter_map(|f| f.1).sum::<u64>()
            + dirs.iter().map(|d| d.added).sum::<u64>();
        let removed = files.iter().filter_map(|f| f.2).sum::<u64>()
            + dirs.iter().map(|d| d.removed).sum::<u64>();
        Dir {
            name,
            path,
            dirs,
            files,
            count,
            added,
            removed,
        }
    }
    let mut root = Raw::default();
    for file in files {
        let mut parts: Vec<&str> = file.path.split('/').filter(|p| !p.is_empty()).collect();
        let Some(name) = parts.pop() else { continue };
        let mut at = &mut root;
        for part in parts {
            at = at.dirs.entry(part.to_string()).or_default();
        }
        at.files
            .insert(name.to_string(), (file.added, file.removed));
    }
    finish(root, String::new(), String::new())
}

/// Whether a folder is open before anyone clicks it — see [`ALL_OPEN`] and
/// [`FEW`] — and whether it is open now, a click having turned it over.
pub(super) fn is_open(dir: &Dir, depth: usize, total: usize, turned: &HashSet<String>) -> bool {
    let default = total <= ALL_OPEN || depth > 0 || dir.count <= FEW;
    default != turned.contains(&dir.path)
}

/// One row of the Files tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Row {
    Folder {
        depth: usize,
        name: String,
        path: String,
        open: bool,
        count: usize,
        added: u64,
        removed: u64,
    },
    File {
        depth: usize,
        name: String,
        /// From the checkout, which is what a click on it opens.
        path: String,
        added: Option<u64>,
        removed: Option<u64>,
    },
}

/// The tree as rows, top to bottom: folders before files at each level, and
/// nothing under a folded one.
pub(super) fn rows(root: &Dir, total: usize, turned: &HashSet<String>) -> Vec<Row> {
    fn walk(dir: &Dir, depth: usize, total: usize, turned: &HashSet<String>, out: &mut Vec<Row>) {
        for sub in &dir.dirs {
            let open = is_open(sub, depth, total, turned);
            out.push(Row::Folder {
                depth,
                name: sub.name.clone(),
                path: sub.path.clone(),
                open,
                count: sub.count,
                added: sub.added,
                removed: sub.removed,
            });
            if open {
                walk(sub, depth + 1, total, turned, out);
            }
        }
        for (name, added, removed) in &dir.files {
            let path = match dir.path.is_empty() {
                true => name.clone(),
                false => format!("{}/{name}", dir.path),
            };
            out.push(Row::File {
                depth,
                name: name.clone(),
                path,
                added: *added,
                removed: *removed,
            });
        }
    }
    let mut out = Vec::new();
    walk(root, 0, total, turned, &mut out);
    out
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
                untracked: false,
            })
            .collect()
    };
    let mut checkout = None;
    let files = match dir.map(|dir| cctop_core::changes::read(&dir, started)) {
        Ok(Ok(Some(changes))) => {
            checkout = Some((changes.root.clone(), changes.base.commit.clone()));
            Ok(changes
                .files
                .into_iter()
                .map(|f| File {
                    path: f.path,
                    added: f.added,
                    removed: f.removed,
                    untracked: f.untracked,
                })
                .collect())
        }
        Ok(Ok(None)) => Ok(edited()),
        Ok(Err(why)) | Err(why) => match edited() {
            files if files.is_empty() => Err(why),
            files => Ok(files),
        },
    };
    Answer {
        plan,
        files,
        checkout,
    }
}

impl App {
    /// Resolve the focused pane's session, and ask for its plan and files once
    /// it has done something since they were last read. Called every turn of
    /// the loop; cheap unless something moved.
    pub(super) fn sync_sidebar(&mut self) {
        self.diff_follows_tab();
        // About the focused pane when it is an agent here, else the tab's first
        // one: whether the sidebar is drawn is the tab's answer, not the
        // focus's, so moving between an agent and a shell beside it does not
        // resize every pane in the tab on each click.
        let ours = |pane: &super::tabs::Pane| {
            pane.is_agent() && pane.transport.is_none() && pane.dead.is_none()
        };
        let focused = self
            .tab
            .checked_sub(1)
            .filter(|_| !self.on_settings())
            .and_then(|at| {
                let tab = self.tabs.get(at)?;
                let focus = Some(tab.focus)
                    .filter(|&i| tab.panes.get(i).is_some_and(ours))
                    .or_else(|| tab.panes.iter().position(ours))?;
                Some((at, focus, tab.panes.get(focus)?))
            });
        let Some((at, focus, pane)) = focused else {
            self.sidebar.shown = false;
            self.sidebar.pane = None;
            return;
        };
        self.sidebar.shown = true;
        self.notes.refresh();
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
                // A note half written about the session the sidebar is
                // leaving goes with it, rather than being saved onto this one.
                if self.mode == Mode::Note {
                    self.mode = Mode::List;
                }
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
        let started = cctop_core::changes::since(session);
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
        self.sidebar.checkout = answer.checkout;
        self.needs_redraw = true;
    }

    /// The session the sidebar is about, for a test to compare.
    #[cfg(test)]
    pub(super) fn sidebar_key_for_test(&self) -> Option<String> {
        self.sidebar.key()
    }

    /// Whether the sidebar is drawn, for a test to ask.
    #[cfg(test)]
    pub(super) fn sidebar_shown_for_test(&self) -> bool {
        self.sidebar.shown
    }

    /// A press on the sidebar's left border: the start of a drag, or — the
    /// second press of a double-click — back to the default width.
    pub(super) fn press_sidebar_border(&mut self, column: u16) {
        let now = std::time::Instant::now();
        let double = self
            .sidebar_pressed
            .is_some_and(|at| now.duration_since(at) < DOUBLE_CLICK);
        self.sidebar_pressed = Some(now);
        if double {
            self.sidebar_pressed = None;
            self.sidebar_drag = None;
            self.sidebar_width = None;
            self.sidebar_folded = false;
            self.save_prefs();
        } else {
            self.sidebar_drag = Some((column, column));
        }
        self.needs_redraw = true;
    }

    /// The pointer moved while the border is held: only the guide moves. The
    /// panes keep their size until the drag ends, so the agent is resized
    /// once, not at every column the pointer crossed.
    pub(super) fn drag_sidebar_border(&mut self, column: u16) {
        if let Some((from, _)) = self.sidebar_drag {
            self.sidebar_drag = Some((from, column));
            self.needs_redraw = true;
        }
    }

    /// The border let go at `column`, in a tab whose pane area is `tab`. Not
    /// moved, it was a click, which folds or opens as a click on the sidebar
    /// does. Moved, it sets the width — and unfolds a folded strip dragged
    /// out; dragged narrower than the narrowest open sidebar, it folds.
    pub(super) fn release_sidebar_border(&mut self, column: u16, tab: Rect) {
        let Some((from, _)) = self.sidebar_drag.take() else {
            return;
        };
        self.needs_redraw = true;
        if column == from {
            if self.sidebar_folded {
                self.toggle_sidebar();
            }
            return;
        }
        let right = tab.x + tab.width;
        let wanted = right.saturating_sub(column);
        if wanted < OPEN_MIN {
            self.sidebar_folded = true;
        } else {
            self.sidebar_folded = false;
            self.sidebar_width = Some(clamp_width(wanted, tab.width));
        }
        self.save_prefs();
    }

    /// A click on a folder in the Files tree: fold it, or open it. Kept per
    /// session for as long as cctop runs, so coming back to a tab finds the
    /// tree as it was left.
    pub(super) fn turn_sidebar_folder(&mut self, path: &str) {
        let Some(key) = self.sidebar.key() else {
            return;
        };
        let turned = self.sidebar_folds.entry(key).or_default();
        if !turned.remove(path) {
            turned.insert(path.to_string());
        }
        self.needs_redraw = true;
    }

    /// The wheel over the Files tree.
    pub(super) fn scroll_sidebar_files(&mut self, down: bool) {
        self.sidebar.files_scroll = match down {
            true => self.sidebar.files_scroll + 3,
            false => self.sidebar.files_scroll.saturating_sub(3),
        };
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
        self.edit_note_at(None);
    }

    /// Write the note at `at` on the focused pane's session, or a new one: a
    /// click on a note, or on the Notes header.
    pub(super) fn edit_note_at(&mut self, at: Option<usize>) {
        let Some(key) = self.sidebar.key().filter(|_| self.sidebar.shown) else {
            return self.set_status(match self.sidebar.shown {
                true => "No session to note yet — the agent has not written its transcript",
                false => "Notes are for an agent's tab on this machine",
            });
        };
        self.sidebar_folded = false;
        let notes = self.notes.of(&key);
        match at.and_then(|i| notes.get(i).map(|text| (i, text.clone()))) {
            Some((i, text)) => {
                self.sidebar.note_at = i;
                self.sidebar.note_input.set(text);
            }
            None => {
                self.sidebar.note_at = notes.len();
                self.sidebar.note_input.clear();
            }
        }
        self.sidebar.note_key = Some(key);
        self.mode = Mode::Note;
        self.needs_redraw = true;
    }

    /// Typing a note. Enter keeps it, and an emptied note is removed; ↑ and ↓
    /// step to the note above or below to change it instead.
    pub(super) fn on_key_note(&mut self, key: KeyEvent) {
        let Some(session) = self.sidebar.note_key.clone() else {
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
        (true, false) => match app.sidebar_width {
            Some(chosen) => clamp_width(chosen, area.width),
            None => {
                let wide = WIDE.min(area.width * SHARE / 100);
                match wide >= OPEN_MIN && area.width >= MIN_PANE + wide {
                    true => wide,
                    false => STRIP,
                }
            }
        },
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

/// While the border is dragged, a line where it will land: the sidebar and
/// the panes are redrawn at the new width only when the drag ends.
pub(super) fn draw_drag_guide(frame: &mut Frame, tab: Rect, app: &App) {
    let Some((_, at)) = app.sidebar_drag else {
        return;
    };
    let x = at.clamp(tab.x, (tab.x + tab.width).saturating_sub(1));
    let style = Style::default()
        .fg(theme::colors().accent)
        .add_modifier(Modifier::BOLD);
    let buffer = frame.buffer_mut();
    for y in tab.y..tab.y + tab.height {
        if let Some(cell) = buffer.cell_mut((x, y)) {
            cell.set_symbol("┃").set_style(style);
        }
    }
}

/// Draw the sidebar into `area`, folded or open as it was split.
/// Where the open sidebar's Files tree landed on screen: its folder rows,
/// for a click to fold or open, and its lines, for the wheel.
#[derive(Debug, Default, Clone)]
pub(super) struct Hits {
    pub targets: Vec<(u16, Target)>,
    pub files: Option<(u16, u16)>,
}

pub(super) fn draw(frame: &mut Frame, area: Rect, app: &App) -> Hits {
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
    if folded {
        frame.render_widget(Paragraph::new(strip(&app.sidebar, notes.len())), inner);
        return Hits::default();
    }
    let editing =
        (app.mode == Mode::Note).then_some((&app.sidebar.note_input, app.sidebar.note_at));
    let none = HashSet::new();
    let turned = key
        .as_deref()
        .and_then(|k| app.sidebar_folds.get(k))
        .unwrap_or(&none);
    let drawn = open(
        &app.sidebar,
        key.is_some(),
        notes,
        editing,
        turned,
        inner.width as usize,
        inner.height as usize,
    );
    frame.render_widget(Paragraph::new(drawn.lines), inner);
    let at = |i: usize| inner.y + i as u16;
    Hits {
        targets: drawn
            .targets
            .into_iter()
            .map(|(i, target)| (at(i), target))
            .collect(),
        files: drawn.files.map(|(from, to)| (at(from), at(to))),
    }
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
/// The open sidebar's lines, and where in them the Files tree is: its folder
/// rows by line, for a click to fold, and its lines, for the wheel.
pub(super) struct Drawn {
    pub lines: Vec<Line<'static>>,
    pub targets: Vec<(usize, Target)>,
    pub files: Option<(usize, usize)>,
}

/// What a click on a line of the Files tree does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Target {
    /// Fold or open the folder at this path.
    Folder(String),
    /// Open this file's diff.
    File(String),
    /// Write a note: the one at this index, or a new one.
    Note(Option<usize>),
}

/// A line, and what a click on it does, if anything.
type Tagged = (Line<'static>, Option<Target>);

fn open(
    state: &State,
    found: bool,
    notes: &[String],
    editing: Option<(&LineEdit, usize)>,
    turned: &HashSet<String>,
    width: usize,
    height: usize,
) -> Drawn {
    if !found {
        let lines = wrap(
            "Waiting for this agent's session to appear — its plan, files and notes show here then.",
            width,
        )
        .into_iter()
        .map(|l| Line::from(Span::styled(l, theme::dim())))
        .collect();
        return Drawn {
            lines,
            targets: Vec::new(),
            files: None,
        };
    }
    let untagged = |lines: Vec<Line<'static>>| -> Vec<Tagged> {
        lines.into_iter().map(|l| (l, None)).collect()
    };
    let notes = notes_lines(notes, editing, width);
    let room = height.saturating_sub(notes.len() + 2);
    let plan = untagged(plan_lines(&state.plan, width));
    let files = files_lines(state.files.as_ref(), turned, state.files_scroll, width);
    // The files are usually the longer list and the plan the one being read
    // top to bottom, so the plan takes what it needs of half, and the files
    // the rest.
    let plan_room = plan
        .len()
        .min(room.saturating_sub(files.len().min(room / 2)));
    let mut out = fit(plan, plan_room);
    out.push((Line::default(), None));
    let files_at = out.len();
    out.extend(fit(files, room.saturating_sub(plan_room)));
    let files = (files_at, out.len());
    out.push((Line::default(), None));
    out.extend(notes);
    let targets = out
        .iter()
        .enumerate()
        .filter_map(|(i, (_, target))| target.clone().map(|t| (i, t)))
        .collect();
    Drawn {
        lines: out.into_iter().map(|(l, _)| l).collect(),
        targets,
        files: Some(files),
    }
}

/// A section cut to `room` lines, its header kept and a count of what was cut
/// on the last line kept.
fn fit(mut lines: Vec<Tagged>, room: usize) -> Vec<Tagged> {
    if lines.len() <= room {
        return lines;
    }
    if room <= 1 {
        lines.truncate(room);
        return lines;
    }
    let cut = lines.len() - room + 1;
    lines.truncate(room - 1);
    lines.push((
        Line::from(Span::styled(format!("  … {cut} more"), theme::dim())),
        None,
    ));
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

fn files_lines(
    files: Option<&Result<Vec<File>, String>>,
    turned: &HashSet<String>,
    scroll: usize,
    width: usize,
) -> Vec<Tagged> {
    let plain = |line: Line<'static>| (line, None);
    let list = match files {
        None => {
            return vec![
                plain(header("Files", vec![])),
                plain(Line::from(Span::styled("  reading…", theme::dim()))),
            ];
        }
        Some(Err(why)) => {
            let mut out = vec![plain(header("Files", vec![]))];
            out.extend(
                wrap(why, width.saturating_sub(2))
                    .into_iter()
                    .map(|l| plain(Line::from(Span::styled(format!("  {l}"), theme::dim())))),
            );
            return out;
        }
        Some(Ok(list)) => list,
    };
    let root = tree(list);
    let mut rest = match list.len() {
        0 => vec![],
        n => vec![
            Span::styled(format!("  {n}  "), theme::dim()),
            Span::styled(format!("+{}", root.added), changes::plus()),
            Span::styled(format!(" −{}", root.removed), changes::minus()),
        ],
    };
    // Where the git tool is, when there is room to say: a click on a file
    // opens it here, and Alt+g opens all of them there.
    let used = "Files".len() + rest.iter().map(|s| util::cells(&s.content)).sum::<usize>();
    if !list.is_empty() && used + 7 <= width {
        rest.push(Span::styled("  Alt+g", theme::dim()));
    }
    let mut out = vec![plain(header("Files", rest))];
    if list.is_empty() {
        out.push(plain(Line::from(Span::styled(
            "  nothing changed",
            theme::dim(),
        ))));
        return out;
    }
    let rows = rows(&root, list.len(), turned);
    let scroll = scroll.min(rows.len().saturating_sub(1));
    if scroll > 0 {
        out.push(plain(Line::from(Span::styled(
            format!("  ↑ {scroll} above"),
            theme::dim(),
        ))));
    }
    for row in rows.into_iter().skip(scroll) {
        out.push(row_line(row, width));
    }
    out
}

/// One row of the tree as a line: indented by depth, the name cut at its end
/// so the start of it — which says where it is — always shows, and its lines
/// added and removed on the right.
fn row_line(row: Row, width: usize) -> Tagged {
    let indent = |depth: usize| "  ".repeat(depth.min(3));
    let counts = |added: Option<u64>, removed: Option<u64>| match (added, removed) {
        (Some(a), Some(r)) => vec![
            Span::styled(format!(" +{a}"), changes::plus()),
            Span::styled(format!(" −{r}"), changes::minus()),
        ],
        _ => vec![Span::styled(" bin", theme::dim())],
    };
    let (lead, name, style, tail, folder) = match row {
        Row::Folder {
            depth,
            name,
            path,
            open,
            count,
            added,
            removed,
        } => {
            let mut tail = vec![Span::styled(format!(" {count}"), theme::dim())];
            tail.extend(counts(Some(added), Some(removed)));
            (
                format!("{}{} ", indent(depth), if open { "▾" } else { "▸" }),
                format!("{name}/"),
                theme::title(),
                tail,
                Some(Target::Folder(path)),
            )
        }
        Row::File {
            depth,
            name,
            path,
            added,
            removed,
        } => (
            format!("{}  ", indent(depth)),
            name,
            theme::value(),
            counts(added, removed),
            Some(Target::File(path)),
        ),
    };
    let used = util::cells(&lead) + tail.iter().map(|s| util::cells(&s.content)).sum::<usize>();
    let mut spans = vec![
        Span::styled(lead, theme::dim()),
        Span::styled(util::clip(&name, width.saturating_sub(used).max(4)), style),
    ];
    spans.extend(tail);
    (Line::from(spans), folder)
}

/// The notes, each line tagged with the note a click on it writes: the
/// header and the empty line start a new one, a note's own lines change it.
fn notes_lines(notes: &[String], editing: Option<(&LineEdit, usize)>, width: usize) -> Vec<Tagged> {
    let new = Some(Target::Note(None));
    let mut out = vec![(
        header(
            "Notes",
            vec![Span::styled(
                match editing {
                    Some(_) => "  ↵ keep · ↑↓ pick · Esc",
                    None => "  click or Alt+e",
                },
                theme::dim(),
            )],
        ),
        new.clone(),
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
        let this = Some(Target::Note(Some(i)));
        match editing {
            Some((field, at)) if at == i => out.push((input(field), this)),
            _ => {
                for (n, line) in wrap(note, text_width).into_iter().enumerate() {
                    let lead = if n == 0 { "• " } else { "  " };
                    out.push((
                        Line::from(vec![
                            Span::styled(lead, theme::dim()),
                            Span::styled(line, theme::value()),
                        ]),
                        this.clone(),
                    ));
                }
            }
        }
    }
    match editing {
        Some((field, at)) if at >= notes.len() => out.push((input(field), new)),
        None if notes.is_empty() => {
            out.push((Line::from(Span::styled("  none yet", theme::dim())), new));
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

    fn changed(path: &str, added: u64, removed: u64) -> File {
        File {
            path: path.into(),
            added: Some(added),
            removed: Some(removed),
            untracked: false,
        }
    }

    /// Directories each holding only the next are one row; every folder
    /// counts the files under it and adds up their lines; folders come before
    /// files, each in name order.
    #[test]
    fn the_tree_joins_chains_and_adds_up() {
        let root = tree(&[
            changed("docs/explanation/architecture.md", 10, 2),
            changed("docs/explanation/entity-resolution.md", 5, 1),
            changed("src/main.rs", 3, 0),
            changed("src/ui/table.rs", 7, 4),
            changed("README.md", 1, 1),
        ]);
        let names: Vec<&str> = root.dirs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["docs/explanation", "src"]);
        assert_eq!(root.dirs[0].path, "docs/explanation");
        assert_eq!(root.dirs[0].count, 2);
        assert_eq!((root.dirs[0].added, root.dirs[0].removed), (15, 3));
        assert_eq!(
            root.dirs[1].dirs[0].name, "ui",
            "a folder beside a file is kept"
        );
        assert_eq!(root.files, [("README.md".to_string(), Some(1), Some(1))]);
        assert_eq!((root.count, root.added, root.removed), (5, 26, 8));
        let all = rows(&root, 5, &HashSet::new());
        assert!(
            matches!(&all[0], Row::Folder { name, open: true, .. } if name == "docs/explanation")
        );
        assert!(matches!(all.last(), Some(Row::File { name, .. }) if name == "README.md"));
    }

    /// A long list starts as an outline, its big top-level folders folded,
    /// and a click turns one over.
    #[test]
    fn a_long_list_starts_folded_and_a_click_opens_a_folder() {
        let mut files: Vec<File> = (0..20)
            .map(|i| changed(&format!("docs/f{i:02}.md"), 1, 0))
            .collect();
        files.push(changed("src/a.rs", 1, 0));
        let root = tree(&files);
        let none = HashSet::new();
        let folded = rows(&root, files.len(), &none);
        assert_eq!(
            folded.len(),
            3,
            "docs folded, src (one file) open with its file: {folded:?}"
        );
        assert!(
            matches!(&folded[0], Row::Folder { name, open: false, count: 20, .. } if name == "docs")
        );
        let turned: HashSet<String> = ["docs".to_string()].into();
        assert_eq!(rows(&root, files.len(), &turned).len(), 23);
    }

    /// Every line of the notes is a click: the header and the empty line start
    /// a new note, and each of a note's lines — wrapped ones too — opens that
    /// note.
    #[test]
    fn a_click_on_a_note_writes_that_note() {
        let notes = vec!["ask Sam about expiry".to_string(), "rerun ci".to_string()];
        let lines = notes_lines(&notes, None, 14);
        let targets: Vec<_> = lines.iter().map(|(_, t)| t.clone()).collect();
        assert_eq!(targets[0], Some(Target::Note(None)), "the header adds");
        assert!(lines.len() > 3, "the first note wraps: {}", lines.len());
        assert!(
            targets[1..lines.len() - 1]
                .iter()
                .all(|t| *t == Some(Target::Note(Some(0))))
        );
        assert_eq!(targets.last().unwrap(), &Some(Target::Note(Some(1))));

        let empty = notes_lines(&[], None, 14);
        assert_eq!(empty.len(), 2);
        assert!(empty.iter().all(|(_, t)| *t == Some(Target::Note(None))));
    }

    /// A name too long for the sidebar is cut at its end, so where it is —
    /// the start — still shows.
    #[test]
    fn a_long_name_is_cut_at_its_end() {
        let (line, target) = row_line(
            Row::File {
                depth: 1,
                name: "entity-resolution-and-merging.md".into(),
                path: "docs/entity-resolution-and-merging.md".into(),
                added: Some(12),
                removed: Some(3),
            },
            26,
        );
        assert_eq!(
            target,
            Some(Target::File("docs/entity-resolution-and-merging.md".into()))
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.starts_with("    entity-res"), "{text}");
        assert!(text.contains('…'), "{text}");
        assert!(text.ends_with(" +12 −3"), "{text}");
    }

    /// A dragged width stays between the narrowest open sidebar and 60% of
    /// the tab, always leaves the agent 40 columns, and is the strip in a tab
    /// too narrow for even the narrowest.
    #[test]
    fn a_dragged_width_is_kept_within_bounds() {
        assert_eq!(clamp_width(50, 200), 50);
        assert_eq!(clamp_width(5, 200), OPEN_MIN);
        assert_eq!(clamp_width(190, 200), 120, "60% of the tab");
        assert_eq!(clamp_width(80, 90), 50, "40 columns left for the agent");
        assert_eq!(clamp_width(40, 60), STRIP, "no room to open at all");
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
                    untracked: false,
                })
                .collect())),
            ..State::default()
        };
        let lines = text(
            &open(
                &state,
                true,
                &["remember".into()],
                None,
                &HashSet::new(),
                30,
                16,
            )
            .lines,
        );
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
