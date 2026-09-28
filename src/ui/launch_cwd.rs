//! The launcher's `in` field: typing a directory and being offered one.
//!
//! [`dirs`](super::dirs) computes the suggestions; this owns the field they are
//! offered into — the editing state, the cursor, and the check that a typed path
//! is a directory that exists before a launch is allowed to take it. The split
//! is so that the matching logic can be tested without an `App` at all.

use super::*;

/// Directories the launcher's `in` field will remember, at most.
///
/// The list is a way of not having to recall a path, so it is worth being
/// generous — but it is filtered by what is typed, and a machine with hundreds
/// of transcripts would otherwise stat every project on it on every keystroke.
const MAX_KNOWN_DIRS: usize = 40;

impl App {
    /// Open the launcher's directory field, prefilled with where it would go.
    ///
    /// Prefilled with `~` spelling rather than the absolute path: that is how
    /// the line already reads, and a field that changed what it showed the
    /// moment it became editable would look like it had lost the setting.
    pub(super) fn edit_launch_cwd(&mut self) {
        let prefill = self
            .launch_cwd
            .as_ref()
            .map(|dir| crate::util::tildify(&dir.to_string_lossy()))
            .unwrap_or_default();
        self.launch_cwd_input.set(prefill);
        self.launch_cwd_bad = false;
        self.launch_cwd_known = self.known_dirs();
        self.launch_cwd_suggest();
        self.mode = Mode::LaunchCwd;
        self.needs_redraw = true;
    }

    /// Directories agents are known to have run in, newest first.
    ///
    /// Drawn from the dashboard's own rows, which is the list of projects cctop
    /// has any evidence of, plus where it was started and where this launch was
    /// already headed — those two are what "in this directory" and the line the
    /// field replaces already meant, and a suggestion list that omitted them
    /// would look like it had forgotten them.
    ///
    /// Only directories that still exist: a session's recorded project can have
    /// been moved or deleted since, and offering one leads to the refusal this
    /// list exists to avoid.
    pub(super) fn known_dirs(&self) -> Vec<std::path::PathBuf> {
        let mut recent: Vec<&Session> = self.sessions.iter().collect();
        // Newest first, so the projects worked on today are the ones on screen
        // before anything is typed.
        recent.sort_by(|a, b| b.last_active.cmp(&a.last_active));

        let mut seen = HashSet::new();
        self.launch_cwd
            .clone()
            .into_iter()
            .chain(self.launch_root.clone())
            .chain(
                self.launch_offer
                    .iter()
                    .filter_map(|c| c.cwd().map(std::path::Path::to_path_buf)),
            )
            .chain(
                recent
                    .iter()
                    .filter(|s| !s.label_source.is_empty())
                    .map(|s| std::path::PathBuf::from(&s.label_source)),
            )
            .filter(|dir| seen.insert(dir.clone()) && dir.is_dir())
            .take(MAX_KNOWN_DIRS)
            .collect()
    }

    /// Recompute what the field is offering, after anything that changed what
    /// is in it.
    ///
    /// The pick goes with it: a suggestion highlighted for the old text would
    /// otherwise still be what Enter took, which is a directory the field is no
    /// longer showing.
    pub(super) fn launch_cwd_suggest(&mut self) {
        self.launch_cwd_hits = dirs::suggest(&self.launch_cwd_input, &self.launch_cwd_known);
        self.launch_cwd_pick = None;
    }

    /// Move the cursor through the suggestions, or back into the text.
    ///
    /// Leaving the list at the top rather than wrapping to the bottom is what
    /// makes the text reachable again: the field is the thing being edited, and
    /// a list that cycled would trap the cursor in it.
    pub(super) fn step_launch_cwd(&mut self, down: bool) {
        let last = self.launch_cwd_hits.len().saturating_sub(1);
        self.launch_cwd_pick = match (self.launch_cwd_pick, down) {
            (_, _) if self.launch_cwd_hits.is_empty() => None,
            (None, true) => Some(0),
            (None, false) => Some(last),
            (Some(i), true) => Some((i + 1).min(last)),
            (Some(0), false) => None,
            (Some(i), false) => Some(i - 1),
        };
        self.needs_redraw = true;
    }

    /// Fill in as much of the path as the suggestions agree on.
    ///
    /// Completing re-suggests: `~/c` completing to `~/cctop/` is only useful if
    /// the list then shows what is inside it, which is how a deep path gets
    /// walked to without being remembered.
    pub(super) fn complete_launch_cwd(&mut self) {
        // The highlighted one, if the cursor is in the list — there Tab means
        // "that one", the same as Enter, minus the launching.
        let filled = match self
            .launch_cwd_pick
            .and_then(|i| self.launch_cwd_hits.get(i))
        {
            Some(dir) => format!("{}/", crate::util::tildify(&dir.to_string_lossy())),
            None => match dirs::complete(&self.launch_cwd_input, &self.launch_cwd_hits) {
                Some(filled) => filled,
                None => return,
            },
        };
        if filled.chars().count() > input::MAX_PATH_INPUT {
            return;
        }
        self.launch_cwd_input.set(filled);
        self.launch_cwd_bad = false;
        self.launch_cwd_suggest();
        self.needs_redraw = true;
    }

    /// Take the highlighted suggestion, or what is typed if there is none.
    ///
    /// A suggestion is a directory this code listed off the disk moments ago, so
    /// it is taken without the check the typed path gets — and it is taken by
    /// filling the field with it first, so that a suggestion which has since
    /// been deleted is refused in the field like anything else.
    pub(super) fn take_launch_cwd(&mut self) {
        if let Some(dir) = self
            .launch_cwd_pick
            .and_then(|i| self.launch_cwd_hits.get(i))
        {
            self.launch_cwd_input = crate::util::tildify(&dir.to_string_lossy()).into();
        }
        self.accept_launch_cwd();
    }

    /// Take the typed directory, if it names one.
    ///
    /// Checked here rather than at launch. A path that does not exist fails
    /// somewhere inside the shim with a message about spawning, by which point
    /// the launcher is gone and there is nothing left to correct.
    pub(super) fn accept_launch_cwd(&mut self) {
        let typed = self.launch_cwd_input.trim();
        // Empty means "wherever cctop was started", which is what the launcher
        // offers by default and what the footer calls "this directory".
        let taken = match typed.is_empty() {
            true => None,
            false => {
                let path = std::path::PathBuf::from(crate::util::untildify(typed));
                if !path.is_dir() {
                    self.launch_cwd_bad = true;
                    return;
                }
                Some(path)
            }
        };
        // Cleared on the way out, not only on the way in: a path corrected
        // after a refusal would otherwise carry the mark back to a field that
        // now holds something perfectly good.
        self.launch_cwd_bad = false;
        self.launch_cwd = taken;
        self.mode = Mode::Launch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{key, session, test_app};
    use ratatui::crossterm::event::KeyCode;
    use std::sync::mpsc::channel;
    /// The suggestions are part of the footer, not part of the list: a short
    /// terminal has to lose choices before it loses the paths being offered,
    /// because they are what the field is being typed against.
    #[test]
    fn the_directory_suggestions_stay_on_screen_under_a_long_list() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let root = tempfile::tempdir().expect("tempdir");
        let project = root.path().join("chosen-project");
        std::fs::create_dir(&project).expect("mkdir");

        let mut app = test_app();
        app.launch_offer = (0..20)
            .map(|i| tabs::Choice::Start(vec![format!("agent-{i}")]))
            .collect();
        app.launch_cwd_known = vec![project.clone()];
        app.launch_cwd_input = Default::default();
        app.launch_cwd_suggest();
        app.launch_cwd_pick = Some(0);
        app.mode = Mode::LaunchCwd;

        let (cols, rows) = (80u16, 14u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        let mut layout = render::Layout::default();
        terminal
            .draw(|frame| layout = render::draw(frame, &mut app))
            .expect("draw");

        let buffer = terminal.backend().buffer().clone();
        let text: String = (0..rows)
            .map(|y| {
                (0..cols)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        assert!(
            text.contains("chosen-project"),
            "the suggestion is not drawn"
        );
        assert!(text.contains("Tab fill in"), "the key that fills it in");

        // And it is clickable where it was drawn, rather than on a row the
        // choices above it also claim.
        let (row, i) = *layout
            .launch_cwd_rows
            .first()
            .expect("no clickable suggestion");
        assert_eq!(i, 0);
        assert!(row < rows, "the suggestion is drawn off screen");
        assert!(
            !layout.launch_rows.iter().any(|(y, _)| *y == row),
            "a choice and a suggestion share row {row}"
        );
    }

    /// The launcher's directory is a field, not a caption. A `claude` opened on
    /// the wrong project reads its way into the wrong repository before anyone
    /// notices, and until now the only way to change it was to restart cctop
    /// somewhere else.
    #[test]
    fn the_launchers_directory_can_be_typed_and_is_checked_before_it_is_taken() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::new(Plan::Retail, channel().0);
        app.launch_cwd = Some(dir.path().to_path_buf());

        // Opens prefilled with what it would have used, so nothing looks lost.
        app.edit_launch_cwd();
        assert_eq!(app.mode, Mode::LaunchCwd);
        assert_eq!(
            app.launch_cwd_input,
            crate::util::tildify(&dir.path().to_string_lossy())
        );

        // A directory that is not one is refused where it was typed, and the
        // field stays open. Failing at launch instead would report it from
        // inside the shim, after the launcher had gone.
        app.launch_cwd_input = dir
            .path()
            .join("nope")
            .to_string_lossy()
            .into_owned()
            .into();
        app.accept_launch_cwd();
        assert!(app.launch_cwd_bad);
        assert_eq!(app.mode, Mode::LaunchCwd, "the field stays open");
        assert_eq!(app.launch_cwd.as_deref(), Some(dir.path()), "unchanged");

        // A real one is taken.
        let sub = dir.path().join("work");
        std::fs::create_dir(&sub).expect("mkdir");
        app.launch_cwd_input = sub.to_string_lossy().into_owned().into();
        app.accept_launch_cwd();
        assert!(!app.launch_cwd_bad);
        assert_eq!(app.mode, Mode::Launch);
        assert_eq!(app.launch_cwd.as_deref(), Some(sub.as_path()));

        // Empty means where cctop was started, which is what the footer calls
        // "this directory" — not an error, and not the previous value.
        app.edit_launch_cwd();
        app.launch_cwd_input = "   ".into();
        app.accept_launch_cwd();
        assert_eq!(app.launch_cwd, None);
        assert_eq!(app.mode, Mode::Launch);
    }

    /// The field is the only place in cctop where a path has to be produced from
    /// memory, so it offers what it can see: the projects agents have run in
    /// before anything is typed, the directories under whatever is typed after.
    #[test]
    fn the_directory_field_offers_paths_instead_of_asking_you_to_recall_them() {
        let root = tempfile::tempdir().expect("tempdir");
        let project = root.path().join("api");
        let deep = project.join("service");
        std::fs::create_dir_all(&deep).expect("mkdir");
        let gone = root.path().join("deleted");

        let mut app = test_app();
        // One project that still exists and one that does not: a recorded
        // directory outlives the checkout it named.
        app.sessions = vec![
            session("a", true, &project.to_string_lossy()),
            session("b", false, &gone.to_string_lossy()),
        ];
        app.launch_root = None;
        app.edit_launch_cwd();

        // Nothing typed, and the project is already on screen. The one that has
        // since been deleted is not, because taking it could only fail.
        assert_eq!(app.launch_cwd_hits, vec![project.clone()]);
        assert!(!app.launch_cwd_hits.contains(&gone));

        // The arrows move into the list and back out of it. Out, because the
        // field is still what is being typed in.
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.launch_cwd_pick, Some(0));
        app.on_key(key(KeyCode::Up));
        assert_eq!(app.launch_cwd_pick, None);

        // Tab on the highlighted project fills it in and then shows what is
        // inside it, which is how a path nobody remembers gets walked to.
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Tab));
        assert_eq!(
            app.launch_cwd_input,
            format!("{}/", project.to_string_lossy())
        );
        assert_eq!(app.launch_cwd_hits, vec![deep.clone()]);
        assert_eq!(app.launch_cwd_pick, None, "a fresh list picks nothing");

        // Enter on a suggestion takes it without the refusal a mistyped path
        // gets — it was listed off the disk moments ago.
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::Launch);
        assert!(!app.launch_cwd_bad);
        assert_eq!(app.launch_cwd.as_deref(), Some(deep.as_path()));

        // Typing still decides on its own: a path with no match offers nothing
        // and is refused where it was typed, exactly as before.
        app.edit_launch_cwd();
        app.launch_cwd_input = root
            .path()
            .join("nowhere")
            .to_string_lossy()
            .into_owned()
            .into();
        app.launch_cwd_suggest();
        assert!(app.launch_cwd_hits.is_empty());
        app.on_key(key(KeyCode::Enter));
        assert!(app.launch_cwd_bad);
        assert_eq!(app.mode, Mode::LaunchCwd);
    }

    /// Esc has to leave the launch as it was found, or it becomes a way to lose
    /// the setting you opened the field to change.
    #[test]
    fn cancelling_the_directory_field_keeps_the_old_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::new(Plan::Retail, channel().0);
        app.launch_cwd = Some(dir.path().to_path_buf());
        app.edit_launch_cwd();
        app.launch_cwd_input = "/somewhere/else".into();
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Launch);
        assert_eq!(app.launch_cwd.as_deref(), Some(dir.path()));
    }
}
