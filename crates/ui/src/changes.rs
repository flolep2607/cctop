//! The Changes panel: what the selected session did to its repository.
//!
//! Which base the diff is against, and why, is [`cctop_core::changes`]'s to
//! say; this is the panel — when to ask the worker, which file the cursor is
//! on and whether its diff is open, and the lines it draws.
//!
//! Asked only while the panel is on screen. A `git diff` is a few processes and
//! a read of every changed file, which is nothing once and too much to repeat
//! for every row the cursor passes on its way down the table with another
//! panel open.
//!
//! # Checkpoints
//!
//! `u` turns the panel to the session's checkpoints — the snapshots the hook
//! has [`cctop_core::checkpoint`] take at each turn — and from there to a
//! confirmation and a rewind. In this panel because a rewind is an answer to
//! what the panel shows: the diff is how somebody finds the turn they want
//! undone. Three faces of one panel rather than a modal, so the table stays in
//! view and the row being rewound is the one selected above it.

use super::{App, theme};
#[cfg(test)]
use cctop_core::changes::{Base, BaseKind};
use cctop_core::changes::{Changes, FileChange};
use cctop_core::checkpoint::{Effect, Point, Rewound, Survey};
use cctop_core::session::Session;
use cctop_core::util;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::path::PathBuf;

/// This panel's place in [`panels::TABS`](super::panels::TABS): after Preview,
/// so the panels that were there keep their numbers and a saved choice of
/// panel still means the same one. Its key is `0`, the digit after `9`.
pub const TAB: usize = 9;

/// What the panel knows about the session it is showing.
#[derive(Default)]
pub struct State {
    /// The session the panel is about, by [`Session::key`].
    ///
    /// [`Session::key`]: cctop_core::session::Session::key
    key: String,
    /// The session's activity stamp when its changes were last asked for. Empty
    /// while the panel is hidden, so showing it again asks afresh: the checkout
    /// may have moved without the session doing anything.
    asked: String,
    /// A read is out; another waits for it rather than piling up behind it.
    in_flight: bool,
    /// The last answer: `Ok(None)` is a directory outside any repository, and
    /// an `Err` is a sentence for the panel.
    pub result: Option<Result<Option<Changes>, String>>,
    /// The file the cursor is on, by index into the listed files.
    pub cursor: usize,
    /// The file whose diff is open, by path, so a refresh that inserts a file
    /// above it keeps the same one open.
    pub open: Option<String>,
    pub scroll: u16,
    /// Bring the cursor's row into view at the next draw.
    pub reveal: bool,
    /// Which file — or checkpoint — each drawn line belongs to, so a click
    /// maps to one.
    pub owners: Vec<Option<usize>>,
    pub view: View,
    /// The session's checkpoints against the checkout now; `None` while they
    /// are being read.
    pub survey: Option<Result<Option<Survey>, String>>,
    /// The checkpoint the cursor is on, by index into the survey's, newest
    /// first.
    pub point: usize,
    /// A rewind is out; a second `y` waits for it.
    rewinding: bool,
}

/// Which face of the panel is up.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The diff since the session started.
    #[default]
    Files,
    /// The session's checkpoints, to pick one to rewind to.
    Checkpoints,
    /// What a rewind to the picked one would do, waiting for `y`.
    Confirm,
}

/// The checkout a session's changes are read from, or the sentence saying why
/// there is none here.
pub(super) fn checkout_of(session: &Session) -> Result<PathBuf, String> {
    cctop_core::changes::checkout_of(session)
}

impl App {
    /// Ask the worker for the selected session's changes when the panel is on
    /// show and the session has done something since they were last read.
    pub(super) fn sync_changes(&mut self) {
        if self.tab != 0 || self.bottom_tab != TAB {
            self.changes.asked.clear();
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        // A remote row's checkout is on its machine; the panel says so.
        if session.remote.is_some() {
            return;
        }
        let key = session.key();
        let stamp = session.touched().to_string();
        let checkout = checkout_of(session);
        let started = cctop_core::changes::since(session);

        let state = &mut self.changes;
        if key != state.key {
            *state = State {
                key: key.clone(),
                ..State::default()
            };
        }
        if state.in_flight || state.asked == stamp {
            return;
        }
        state.asked = stamp;
        let dir = match checkout {
            Ok(dir) => dir,
            Err(why) => {
                state.result = Some(Err(why));
                return;
            }
        };
        state.in_flight = true;
        let _ = self
            .tx
            .send(super::worker::Request::Changes { key, dir, started });
    }

    /// The worker's answer, kept only if the panel is still on that session.
    pub(super) fn got_changes(&mut self, key: String, result: Result<Option<Changes>, String>) {
        let state = &mut self.changes;
        if key != state.key {
            return;
        }
        state.in_flight = false;
        let files = match &result {
            Ok(Some(c)) => c.files.len(),
            _ => 0,
        };
        state.cursor = state.cursor.min(files.saturating_sub(1));
        state.result = Some(result);
        self.needs_redraw = true;
    }

    fn change_files(&self) -> &[FileChange] {
        match &self.changes.result {
            Some(Ok(Some(c))) => &c.files,
            _ => &[],
        }
    }

    /// Move the cursor through the files, wrapping like the Tool Activity
    /// filter the same keys move.
    pub(super) fn step_change(&mut self, delta: isize) {
        if self.changes.view == View::Checkpoints {
            return self.step_checkpoint(delta);
        }
        let n = self.change_files().len() as isize;
        if n == 0 {
            return;
        }
        let state = &mut self.changes;
        state.cursor = (((state.cursor as isize + delta) % n + n) % n) as usize;
        state.reveal = true;
        self.needs_redraw = true;
    }

    /// Open the cursor's file's diff, or close it if it is the open one.
    pub(super) fn toggle_change(&mut self) {
        if self.changes.view == View::Checkpoints {
            return self.choose_checkpoint();
        }
        let Some(path) = self
            .change_files()
            .get(self.changes.cursor)
            .map(|f| f.path.clone())
        else {
            return;
        };
        let state = &mut self.changes;
        state.open = (state.open.as_deref() != Some(path.as_str())).then_some(path);
        state.reveal = true;
        self.needs_redraw = true;
    }

    /// A click on a drawn line: the file it belongs to takes the cursor and
    /// opens or closes.
    pub(super) fn click_change(&mut self, row_offset: usize) {
        let line = self.changes.scroll as usize + row_offset;
        let Some(Some(item)) = self.changes.owners.get(line).copied() else {
            return;
        };
        match self.changes.view {
            View::Files => self.changes.cursor = item,
            View::Checkpoints => self.changes.point = item,
            View::Confirm => return,
        }
        self.toggle_change();
        // The click was on what is already in view; moving it would slide the
        // row out from under the pointer.
        self.changes.reveal = false;
    }

    /// The panel's face, when it is the one on show: what decides whether
    /// Enter, `y` and Esc are the checkpoints' or the table's.
    pub(super) fn changes_view(&self) -> Option<View> {
        (self.tab == 0 && self.bottom_tab == TAB).then_some(self.changes.view)
    }

    /// `u`: the selected session's checkpoints, or back to its diff.
    ///
    /// Refused for a remote row, whose checkout is on its machine, and for
    /// another user's session: even the listing writes — comparing a
    /// checkpoint with now snapshots the checkout, which adds objects to its
    /// repository — and cctop never writes into another user's files.
    pub(super) fn toggle_checkpoints(&mut self) {
        if self.changes.view != View::Files {
            self.changes.view = View::Files;
            self.changes.scroll = 0;
            self.needs_redraw = true;
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        if let Some(why) = App::remote_refusal(session) {
            return self.set_status(why);
        }
        if let Some(owner) = &session.owner {
            return self.set_status(format!(
                "{owner}'s session: cctop never changes another user's files, so it has no \
                 checkpoints to rewind"
            ));
        }
        let key = session.key();
        let id = session.session_id.clone();
        let dir = match checkout_of(session) {
            Ok(dir) => dir,
            Err(why) => return self.set_status(format!("No checkpoints: {why}")),
        };
        self.sync_changes();
        let state = &mut self.changes;
        state.view = View::Checkpoints;
        state.survey = None;
        state.point = 0;
        state.scroll = 0;
        self.needs_redraw = true;
        let _ = self
            .tx
            .send(super::worker::Request::Checkpoints { key, dir, id });
    }

    /// The worker's survey, kept only if the panel is still on that session.
    pub(super) fn got_checkpoints(&mut self, key: String, result: Result<Option<Survey>, String>) {
        let state = &mut self.changes;
        if key != state.key {
            return;
        }
        let n = match &result {
            Ok(Some(s)) => s.points.len(),
            _ => 0,
        };
        state.point = state.point.min(n.saturating_sub(1));
        state.survey = Some(result);
        self.needs_redraw = true;
    }

    fn points(&self) -> &[Point] {
        match &self.changes.survey {
            Some(Ok(Some(s))) => &s.points,
            _ => &[],
        }
    }

    fn step_checkpoint(&mut self, delta: isize) {
        let n = self.points().len() as isize;
        if n == 0 {
            return;
        }
        let state = &mut self.changes;
        state.point = (((state.point as isize + delta) % n + n) % n) as usize;
        state.reveal = true;
        self.needs_redraw = true;
    }

    /// `v` or Enter on a checkpoint: what a rewind to it would do.
    fn choose_checkpoint(&mut self) {
        let Some(point) = self.points().get(self.changes.point) else {
            return;
        };
        if point.files() == 0 {
            let n = point.checkpoint.n;
            return self.set_status(format!(
                "Checkpoint #{n} is how the checkout is now; there is nothing to rewind"
            ));
        }
        self.changes.view = View::Confirm;
        self.changes.scroll = 0;
        self.needs_redraw = true;
    }

    /// Esc: back one face — from the confirmation to the list, from the list
    /// to the diff.
    pub(super) fn back_out_of_checkpoints(&mut self) {
        let state = &mut self.changes;
        state.view = match state.view {
            View::Confirm => View::Checkpoints,
            View::Checkpoints | View::Files => View::Files,
        };
        state.scroll = 0;
        state.reveal = true;
        self.needs_redraw = true;
    }

    /// `y` on the confirmation: rewind.
    pub(super) fn confirm_rewind(&mut self) {
        if self.changes.rewinding {
            return;
        }
        let Some(n) = self
            .points()
            .get(self.changes.point)
            .map(|p| p.checkpoint.n)
        else {
            return;
        };
        let Some(session) = self.selected_session() else {
            return;
        };
        if session.key() != self.changes.key {
            return;
        }
        let key = session.key();
        let id = session.session_id.clone();
        let owner = session.owner.clone();
        let dir = match checkout_of(session) {
            Ok(dir) => dir,
            Err(why) => return self.set_status(format!("Cannot rewind: {why}")),
        };
        self.changes.rewinding = true;
        self.set_status(format!("Rewinding to checkpoint #{n}…"));
        let _ = self.tx.send(super::worker::Request::Rewind {
            key,
            dir,
            id,
            n,
            owner,
        });
    }

    /// The rewind's outcome: said on the status line, and the diff read again,
    /// since the checkout under it has just changed.
    pub(super) fn got_rewound(&mut self, key: String, result: Result<Rewound, String>) {
        if key == self.changes.key {
            let state = &mut self.changes;
            state.rewinding = false;
            state.view = View::Files;
            state.survey = None;
            state.asked.clear();
            state.scroll = 0;
        }
        let said = match result {
            Ok(r) => {
                let moved = match r.head_moved {
                    true => "; HEAD stays where it is",
                    false => "",
                };
                format!(
                    "Rewound to checkpoint #{}{moved}. What was there before is #{}: u lists it",
                    r.to, r.undo
                )
            }
            Err(e) => format!("The rewind failed: {e}"),
        };
        self.set_status(said);
        self.needs_redraw = true;
    }
}

/// The panel's lines for `state`, the file each line belongs to, and the line
/// the cursor's file starts on.
pub fn lines(state: &State, width: usize) -> (Vec<Line<'static>>, Vec<Option<usize>>, usize) {
    match state.view {
        View::Files => {}
        View::Checkpoints => return checkpoint_lines(state, width),
        View::Confirm => {
            let lines = confirm_lines(state, width);
            let owners = vec![None; lines.len()];
            return (lines, owners, 0);
        }
    }
    let dim = |text: String| Line::from(Span::styled(text, theme::dim()));
    let changes = match &state.result {
        None => return (vec![dim("Reading the changes…".into())], vec![None], 0),
        Some(Err(why)) => {
            let mut why = why.clone();
            if let Some(first) = why.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            return (vec![dim(format!("{why}."))], vec![None], 0);
        }
        Some(Ok(None)) => {
            return (
                vec![dim(
                    "Not in a git repository, so there is nothing to diff.".into()
                )],
                vec![None],
                0,
            );
        }
        Some(Ok(Some(c))) => c,
    };

    let mut out = Vec::new();
    let mut owners = Vec::new();
    let mut cursor_line = 0;
    let short: String = changes.base.commit.chars().take(7).collect();
    let (added, removed) = changes.totals();
    let listed = changes.files.len() + changes.more_files;
    out.push(Line::from(vec![
        Span::styled("Since ", theme::label()),
        Span::styled(short, theme::value()),
        Span::styled(format!(", {}", changes.base.kind.describe()), theme::dim()),
    ]));
    owners.push(None);
    if listed == 0 {
        out.push(Line::from(Span::styled(
            "Nothing has changed since then.",
            theme::dim(),
        )));
        owners.push(None);
        return (out, owners, 0);
    }
    out.push(Line::from(vec![
        Span::styled(util::plural(listed, "file"), theme::value()),
        Span::raw("  "),
        Span::styled(format!("+{added}"), plus()),
        Span::raw(" "),
        Span::styled(format!("−{removed}"), minus()),
        Span::styled(
            "   [ ] pick, v diff, p pull request, u checkpoints",
            Style::default().fg(theme::colors().dimmer),
        ),
    ]));
    owners.push(None);

    // The counts are right-aligned in a column wide enough for the largest.
    let count = |n: Option<u64>, sign: char| n.map_or(String::new(), |n| format!("{sign}{n}"));
    let add_w = changes
        .files
        .iter()
        .map(|f| count(f.added, '+').chars().count())
        .max()
        .unwrap_or(0);
    let del_w = changes
        .files
        .iter()
        .map(|f| count(f.removed, '−').chars().count())
        .max()
        .unwrap_or(0);
    // Two for the marker, then the tag column ("new" / "binary").
    let tag_w = 6;
    // As wide as the longest path and no wider, the way `git diff --stat`
    // lays it out: counts pushed to the far edge of a wide panel are a long
    // way from the name they belong to.
    let longest = changes
        .files
        .iter()
        .map(|f| f.path.chars().count())
        .max()
        .unwrap_or(0);
    let path_w = width
        .saturating_sub(2 + tag_w + 1 + add_w + 1 + del_w + 1)
        .min(longest);

    for (i, file) in changes.files.iter().enumerate() {
        let open = state.open.as_deref() == Some(file.path.as_str());
        let here = i == state.cursor;
        if here {
            cursor_line = out.len();
        }
        // No count is a binary file — or an untracked one that could not be
        // read, which has no diff either and is only new.
        let tag = match (file.untracked, file.added, file.diff.is_empty()) {
            (_, None, false) => "binary",
            (true, _, _) => "new",
            (false, _, _) => "",
        };
        let row = if here {
            theme::selected()
        } else {
            Style::default()
        };
        out.push(
            Line::from(vec![
                Span::styled(if open { "▾ " } else { "▸ " }, theme::dim()),
                Span::styled(
                    util::pad(&truncate_left(&file.path, path_w), path_w, false),
                    theme::value(),
                ),
                Span::raw(" "),
                Span::styled(format!("{tag:>tag_w$}"), theme::dim()),
                Span::raw(" "),
                Span::styled(format!("{:>add_w$}", count(file.added, '+')), plus()),
                Span::raw(" "),
                Span::styled(format!("{:>del_w$}", count(file.removed, '−')), minus()),
            ])
            .style(row),
        );
        owners.push(Some(i));
        if open {
            for line in &file.diff {
                out.push(diff_line(line, width));
                owners.push(Some(i));
            }
            if file.diff.is_empty() {
                out.push(dim(
                    "    (no diff: the patch was too long to read this far)".into(),
                ));
                owners.push(Some(i));
            } else if file.cut {
                out.push(dim(format!(
                    "    … cut at {} lines; git diff in the checkout has the rest",
                    file.diff.len()
                )));
                owners.push(Some(i));
            }
        }
    }
    if changes.more_files > 0 {
        out.push(dim(format!(
            "… and {} not listed",
            util::plural(changes.more_files, "more file")
        )));
        owners.push(None);
    }
    if changes.patch_cut {
        out.push(dim(
            "The patch is over 1 MiB; files past the cut have counts but no diff.".into(),
        ));
        owners.push(None);
    }
    (out, owners, cursor_line)
}

/// How long ago a checkpoint was taken, in the table's own units.
fn age(at: i64) -> String {
    let now = chrono::Utc::now();
    match chrono::DateTime::from_timestamp(at, 0) {
        Some(t) => match util::relative_age(&t.to_rfc3339(), &now).as_str() {
            "now" => "just now".into(),
            ago => format!("{ago} ago"),
        },
        None => "n/a".into(),
    }
}

/// The checkpoint list: one row each, newest first, with how far each is from
/// the checkout now — the number a person picks by.
fn checkpoint_lines(
    state: &State,
    width: usize,
) -> (Vec<Line<'static>>, Vec<Option<usize>>, usize) {
    let dim = |text: String| Line::from(Span::styled(text, theme::dim()));
    let survey = match &state.survey {
        None => return (vec![dim("Reading the checkpoints…".into())], vec![None], 0),
        Some(Err(why)) => return (vec![dim(format!("No checkpoints: {why}."))], vec![None], 0),
        Some(Ok(None)) => {
            return (
                vec![dim(
                    "Not in a git repository, so there are no checkpoints.".into()
                )],
                vec![None],
                0,
            );
        }
        Some(Ok(Some(s))) => s,
    };
    let mut out = vec![Line::from(vec![
        Span::styled("Checkpoints", theme::label()),
        Span::styled(", newest first", theme::dim()),
        Span::styled(
            "   [ ] pick one, v rewinds to it, u back to the diff",
            Style::default().fg(theme::colors().dimmer),
        ),
    ])];
    let mut owners = vec![None];
    if survey.points.is_empty() {
        for line in [
            "None for this session in this checkout.",
            "With checkpoints = true in Settings (,), the hook takes one at each prompt",
            "and at the end of each turn, for any agent, to rewind a bad turn from here.",
        ] {
            out.push(dim(line.into()));
            owners.push(None);
        }
        return (out, owners, 0);
    }
    let mut cursor_line = 0;
    let why_w = survey
        .points
        .iter()
        .map(|p| p.checkpoint.why.chars().count())
        .max()
        .unwrap_or(0);
    let age_w = survey
        .points
        .iter()
        .map(|p| age(p.checkpoint.at).chars().count())
        .max()
        .unwrap_or(0);
    let n_w = survey
        .points
        .iter()
        .map(|p| p.checkpoint.n.to_string().len())
        .max()
        .unwrap_or(1);
    for (i, point) in survey.points.iter().enumerate() {
        let here = i == state.point;
        if here {
            cursor_line = out.len();
        }
        let c = &point.checkpoint;
        let differ = match point.files() {
            0 => Span::styled("same as now", theme::dim()),
            n => Span::styled(
                format!("{} differ", util::plural(n, "file")),
                theme::value(),
            ),
        };
        // Taken before the agent committed: a rewind restores the files and
        // leaves the commits, which the confirmation spells out.
        let moved = match c.head != survey.head {
            true => Span::styled("  HEAD moved since", theme::dim()),
            false => Span::raw(""),
        };
        let row = Line::from(vec![
            Span::styled(if here { "▸ " } else { "  " }, theme::dim()),
            Span::styled(format!("#{:<n_w$}", c.n), theme::value()),
            Span::raw("  "),
            Span::styled(format!("{:>age_w$}", age(c.at)), theme::dim()),
            Span::raw("  "),
            Span::styled(util::pad(&c.why, why_w, false), theme::label()),
            Span::raw("  "),
            differ,
            moved,
        ]);
        let row = match here {
            true => row.style(theme::selected()),
            false => row,
        };
        out.push(truncate_line(row, width));
        owners.push(Some(i));
    }
    (out, owners, cursor_line)
}

/// What a rewind to the picked checkpoint would do, before it is done: the
/// files it changes, what it leaves alone, and that it can be undone.
fn confirm_lines(state: &State, width: usize) -> Vec<Line<'static>> {
    let Some(Ok(Some(survey))) = &state.survey else {
        return Vec::new();
    };
    let Some(point) = survey.points.get(state.point) else {
        return Vec::new();
    };
    let c = &point.checkpoint;
    let dim = |text: String| Line::from(Span::styled(text, theme::dim()));
    let mut out = vec![Line::from(vec![
        Span::styled(format!("Rewind to checkpoint #{}", c.n), theme::label()),
        Span::styled(format!(", {} ({})?", age(c.at), c.why), theme::dim()),
        // On the first line, so a short panel still shows how to answer.
        Span::styled("   y rewinds, Esc goes back", theme::value()),
    ])];
    out.push(Line::from(Span::styled(
        format!("{} change:", util::plural(point.files(), "file")),
        theme::value(),
    )));
    // Room for a handful of files and the sentences below them; the rest are
    // counted, since the list is for recognising the turn, not auditing it.
    const SHOWN: usize = 8;
    // As wide as the longest path shown, leaving the description room.
    let path_w = point
        .differs
        .iter()
        .take(SHOWN)
        .map(|d| d.path.chars().count())
        .max()
        .unwrap_or(0)
        .min(width.saturating_sub(4 + 31).max(12));
    for d in point.differs.iter().take(SHOWN) {
        let (mark, style, what) = match d.effect {
            Effect::Revert => (
                "~",
                Style::default().fg(theme::colors().accent),
                "back to how it was",
            ),
            Effect::Remove => ("−", minus(), "removed: not in the checkpoint"),
            Effect::Restore => ("+", plus(), "restored"),
        };
        out.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(mark, style),
            Span::raw(" "),
            Span::styled(
                util::pad(&truncate_left(&d.path, path_w), path_w, false),
                theme::value(),
            ),
            Span::raw(" "),
            Span::styled(what, theme::dim()),
        ]));
    }
    let more = point.files().saturating_sub(SHOWN);
    if more > 0 {
        out.push(dim(format!("  … and {}", util::plural(more, "more file"))));
    }
    if c.head != survey.head {
        out.push(dim(format!(
            "HEAD has moved since #{} (commits were made). Only files are restored; HEAD,",
            c.n
        )));
        out.push(dim("branches and commits stay as they are.".into()));
    }
    out.push(dim(
        "Ignored files are left alone, and the index is set to match the checkpoint.".into(),
    ));
    out.push(dim(
        "How it is now is saved first as a checkpoint of its own, so this can be undone.".into(),
    ));
    out.into_iter().map(|l| truncate_line(l, width)).collect()
}

/// A line cut to `width` columns, span by span, keeping each span's style.
fn truncate_line(line: Line<'static>, width: usize) -> Line<'static> {
    let mut left = width;
    let style = line.style;
    let spans: Vec<Span<'static>> = line
        .spans
        .into_iter()
        .filter_map(|span| {
            if left == 0 {
                return None;
            }
            let len = span.content.chars().count();
            let text = match len <= left {
                true => span.content.into_owned(),
                false => util::truncate(&span.content, left),
            };
            left = left.saturating_sub(len);
            Some(Span::styled(text, span.style))
        })
        .collect();
    Line::from(spans).style(style)
}

pub(super) fn plus() -> Style {
    Style::default().fg(theme::colors().cost_low)
}

pub(super) fn minus() -> Style {
    Style::default().fg(theme::colors().cost_high)
}

/// One line of a diff, coloured the way Tool Activity colours an edit's: added
/// and removed in the theme's good and bad, a hunk header in the accent, and
/// the context dim.
fn diff_line(line: &str, width: usize) -> Line<'static> {
    let style = match line.chars().next() {
        Some('+') => plus(),
        Some('-') => minus(),
        Some('@') => Style::default().fg(theme::colors().accent),
        _ => Style::default().fg(theme::colors().dimmer),
    };
    // A tab is a column of its own to the terminal and none to ratatui.
    let line = line.replace('\t', "    ");
    Line::from(vec![
        Span::raw("    "),
        Span::styled(util::truncate(&line, width.saturating_sub(4)), style),
    ])
}

/// `path` cut from the left, keeping the file name: in a deep tree the
/// directories are what repeat and the name is what differs.
pub(super) fn truncate_left(path: &str, width: usize) -> String {
    let len = path.chars().count();
    if len <= width {
        return path.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let tail: String = path.chars().skip(len - (width - 1)).collect();
    format!("…{tail}")
}

/// Three files that between them show each kind of row: an edit, a binary and
/// a new file. The snapshot draws the panel from it.
#[cfg(test)]
pub(super) fn sample() -> Changes {
    Changes {
        root: "/nonexistent/web".into(),
        base: Base {
            commit: "3f2a91c0d4e5f60718293a4b5c6d7e8f90a1b2c3".into(),
            kind: BaseKind::AtStart,
        },
        files: vec![
            FileChange {
                path: "src/app.rs".into(),
                added: Some(12),
                removed: Some(3),
                diff: vec![
                    "@@ -1,3 +1,4 @@".into(),
                    " fn main() {".into(),
                    "-    run();".into(),
                    "+    run_with(args());".into(),
                    "+    log();".into(),
                ],
                ..Default::default()
            },
            FileChange {
                path: "assets/logo.png".into(),
                diff: vec!["Binary files a/assets/logo.png and b/assets/logo.png differ".into()],
                ..Default::default()
            },
            FileChange {
                path: "notes.md".into(),
                added: Some(2),
                removed: Some(0),
                untracked: true,
                diff: vec!["+# Notes".into(), "+todo".into()],
                ..Default::default()
            },
        ],
        more_files: 0,
        patch_cut: false,
    }
}

/// Three checkpoints: the newest the same as now, one a turn back, and one
/// from before the agent committed. Ages sit mid-bucket, as the snapshot
/// fixture's do, so a slow draw still reads the same.
#[cfg(test)]
pub(super) fn sample_survey() -> Survey {
    use cctop_core::checkpoint::{Checkpoint, Differ};
    let now = chrono::Utc::now().timestamp();
    let (old_head, head) = ("a".repeat(40), "b".repeat(40));
    let point = |n: u64, ago: i64, why: &str, head: &str, differs: Vec<(Effect, &str)>| Point {
        checkpoint: Checkpoint {
            n,
            commit: format!("{n:040}"),
            tree: String::new(),
            head: Some(head.into()),
            at: now - ago,
            why: why.into(),
        },
        differs: differs
            .into_iter()
            .map(|(effect, path)| Differ {
                path: path.into(),
                effect,
            })
            .collect(),
        more: 0,
    };
    Survey {
        head: Some(head.clone()),
        points: vec![
            point(3, 7 * 60 + 30, "turn ended", &head, vec![]),
            point(
                2,
                22 * 60 + 30,
                "prompt submitted",
                &head,
                vec![(Effect::Revert, "src/app.rs"), (Effect::Remove, "notes.md")],
            ),
            point(
                1,
                2 * 3600 + 1800,
                "prompt submitted",
                &old_head,
                vec![
                    (Effect::Revert, "src/app.rs"),
                    (Effect::Remove, "notes.md"),
                    (Effect::Restore, "src/legacy.rs"),
                ],
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    #[test]
    fn the_checkpoint_list_says_how_far_each_is_from_now() {
        let state = State {
            view: View::Checkpoints,
            survey: Some(Ok(Some(sample_survey()))),
            point: 1,
            ..State::default()
        };
        let (lines, owners, cursor) = lines(&state, 100);
        let text = text(&lines);
        assert!(text[0].starts_with("Checkpoints, newest first"));
        assert!(text[1].contains("#3") && text[1].contains("same as now"));
        assert!(text[2].starts_with("▸ #2"), "{}", text[2]);
        assert!(text[2].contains("2 files differ"));
        assert!(text[3].contains("HEAD moved since"), "{}", text[3]);
        assert!(!text[2].contains("HEAD moved"));
        assert_eq!(cursor, 2);
        assert_eq!(owners, [None, Some(0), Some(1), Some(2)]);
    }

    #[test]
    fn the_confirmation_says_what_changes_and_that_it_can_be_undone() {
        let mut state = State {
            view: View::Confirm,
            survey: Some(Ok(Some(sample_survey()))),
            point: 2,
            ..State::default()
        };
        let (lines, _, _) = lines(&state, 100);
        let all = text(&lines).join("\n");
        assert!(all.contains("Rewind to checkpoint #1"), "{all}");
        assert!(all.contains("3 files change"));
        assert!(all.contains("+ src/legacy.rs"));
        assert!(all.contains("HEAD has moved since #1"));
        assert!(all.contains("can be undone"));
        assert!(all.contains("Ignored files are left alone"));

        state.point = 1;
        let all = text(&lines_of(&state)).join("\n");
        assert!(!all.contains("HEAD has moved"), "{all}");
    }

    fn lines_of(state: &State) -> Vec<Line<'static>> {
        lines(state, 100).0
    }

    #[test]
    fn no_checkpoints_says_how_to_get_them() {
        let state = State {
            view: View::Checkpoints,
            survey: Some(Ok(Some(Survey {
                head: None,
                points: vec![],
            }))),
            ..State::default()
        };
        let all = text(&lines_of(&state)).join("\n");
        assert!(all.contains("checkpoints = true"), "{all}");
    }

    /// Another user's session and a remote row get a sentence, not a list:
    /// even reading the list writes objects into the repository.
    #[test]
    fn checkpoints_are_refused_for_another_users_session_and_a_remote_row() {
        let mut app = crate::tests::test_app();
        let mut theirs = crate::tests::session("a", true, "/tmp");
        theirs.owner = Some("bob".into());
        app.sessions = vec![theirs];
        app.refilter();
        app.bottom_tab = TAB;
        app.toggle_checkpoints();
        assert_eq!(app.changes.view, View::Files);
        assert!(
            app.status().unwrap_or_default().contains("bob"),
            "{}",
            app.status().unwrap_or_default()
        );

        let mut far = crate::tests::session("b", true, "/tmp");
        far.remote = Some(cctop_core::session::Remote {
            host: "procdb".into(),
            ..Default::default()
        });
        app.sessions = vec![far];
        app.refilter();
        app.toggle_checkpoints();
        assert_eq!(app.changes.view, View::Files);
        assert!(
            app.status().unwrap_or_default().contains("procdb"),
            "{}",
            app.status().unwrap_or_default()
        );
    }

    /// `u` opens the list and asks for it; Esc walks back a face at a time.
    #[test]
    fn u_lists_and_esc_walks_back() {
        let mut app = crate::tests::test_app();
        let dir = tempfile::tempdir().unwrap();
        app.sessions = vec![crate::tests::session(
            "a",
            true,
            &dir.path().to_string_lossy(),
        )];
        app.refilter();
        app.bottom_tab = TAB;
        app.toggle_checkpoints();
        assert_eq!(app.changes.view, View::Checkpoints);
        assert!(app.changes.survey.is_none(), "asked, not yet answered");
        let key = app.changes.key.clone();
        app.got_checkpoints(key, Ok(Some(sample_survey())));
        app.step_change(1);
        assert_eq!(app.changes.point, 1);
        app.toggle_change();
        assert_eq!(app.changes.view, View::Confirm);
        app.back_out_of_checkpoints();
        assert_eq!(app.changes.view, View::Checkpoints);
        // The newest is the same as now: nothing to confirm.
        app.step_change(-1);
        app.toggle_change();
        assert_eq!(app.changes.view, View::Checkpoints);
        app.back_out_of_checkpoints();
        assert_eq!(app.changes.view, View::Files);
    }

    fn text(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn the_first_line_names_the_base() {
        let state = State {
            result: Some(Ok(Some(sample()))),
            ..State::default()
        };
        let (lines, owners, cursor) = lines(&state, 80);
        let text = text(&lines);
        assert!(text[0].starts_with("Since 3f2a91c, HEAD when the session started"));
        assert!(text[1].starts_with("3 files  +14 −3"), "{}", text[1]);
        assert_eq!(cursor, 2);
        assert_eq!(owners.len(), lines.len());
        assert!(lines[2].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn opening_a_file_puts_its_diff_under_it_and_clicks_find_it() {
        let state = State {
            result: Some(Ok(Some(sample()))),
            cursor: 1,
            open: Some("src/app.rs".into()),
            ..State::default()
        };
        let (lines, owners, cursor) = lines(&state, 80);
        let text = text(&lines);
        assert!(text[2].starts_with("▾ src/app.rs"));
        assert_eq!(text[3].trim(), "@@ -1,3 +1,4 @@");
        assert_eq!(owners[3], Some(0), "a diff line belongs to its file");
        assert_eq!(cursor, 8, "the cursor's file is below the open diff");
        assert!(text[8].contains("binary"));
    }

    #[test]
    fn a_long_path_keeps_its_file_name() {
        assert_eq!(truncate_left("a/very/deep/tree/file.rs", 10), "…e/file.rs");
        assert_eq!(truncate_left("short.rs", 10), "short.rs");
    }

    #[test]
    fn not_a_repository_says_so() {
        let state = State {
            result: Some(Ok(None)),
            ..State::default()
        };
        let (lines, _, _) = lines(&state, 80);
        assert!(text(&lines)[0].contains("Not in a git repository"));
    }

    /// Nothing is asked while the panel is hidden, and a directory that is
    /// gone is said without a trip to the worker.
    #[test]
    fn the_panel_asks_only_while_it_is_shown() {
        let mut app = crate::tests::test_app();
        app.sessions = vec![crate::tests::session("a", true, "/nonexistent/gone")];
        app.refilter();
        app.bottom_tab = 0;
        app.sync_changes();
        assert!(app.changes.result.is_none(), "asked from another panel");

        app.bottom_tab = TAB;
        app.sync_changes();
        let Some(Err(why)) = &app.changes.result else {
            panic!("expected a sentence, got {:?}", app.changes.result);
        };
        assert!(why.contains("no longer exists"), "{why}");
        assert!(!app.changes.in_flight);
    }
}
