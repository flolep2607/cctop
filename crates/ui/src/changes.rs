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

use super::{App, theme};
#[cfg(test)]
use cctop_core::changes::{Base, BaseKind};
use cctop_core::changes::{Changes, FileChange};
use cctop_core::util;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

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
    /// Which file each drawn line belongs to, so a click maps to one.
    pub owners: Vec<Option<usize>>,
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
        let dir = session.label_source.clone();
        // As `branch_of` reasons: a stopped sandbox's directory was the host's,
        // mounted for as long as the agent ran, and the same path here now is
        // something else.
        let unmounted = session.sandbox.is_some() && !session.is_running();
        let work_dir = session.work_dir();
        let started = chrono::DateTime::parse_from_rfc3339(&session.started_at)
            .ok()
            .map(|t| t.timestamp());

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
        let dir = match work_dir {
            _ if unmounted => {
                state.result = Some(Err(
                    "this session ran in a sandbox that has stopped, so its checkout is no \
                     longer mounted here"
                        .into(),
                ));
                return;
            }
            Some(dir) => dir,
            None if dir.is_empty() => {
                state.result = Some(Err("this session did not record a directory".into()));
                return;
            }
            None => {
                state.result = Some(Err(format!(
                    "{} no longer exists on this machine",
                    util::tildify(&dir)
                )));
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
        let Some(Some(file)) = self.changes.owners.get(line).copied() else {
            return;
        };
        self.changes.cursor = file;
        self.toggle_change();
        // The click was on what is already in view; moving it would slide the
        // row out from under the pointer.
        self.changes.reveal = false;
    }
}

/// The panel's lines for `state`, the file each line belongs to, and the line
/// the cursor's file starts on.
pub fn lines(state: &State, width: usize) -> (Vec<Line<'static>>, Vec<Option<usize>>, usize) {
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
            "   [ ] pick a file, v opens its diff",
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

fn plus() -> Style {
    Style::default().fg(theme::colors().cost_low)
}

fn minus() -> Style {
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
fn truncate_left(path: &str, width: usize) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

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
