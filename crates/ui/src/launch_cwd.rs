//! The launcher's `in` field: typing a directory and being offered one.
//!
//! [`dirs`](super::dirs) computes the local suggestions and
//! [`location`](super::location) the remote ones; this owns the field they are
//! offered into — the editing state, the cursor, and the check that a typed path
//! is a directory that exists before a launch is allowed to take it. The split
//! is so that the matching logic can be tested without an `App` at all.

use super::location::{Hit, Typed};
use super::*;

/// The hosts in `~/.ssh/config`. Not read under test, where the person
/// running the tests has a config of their own that no test should depend on.
fn ssh_hosts() -> Vec<crate::ssh_config::Host> {
    match cfg!(test) {
        true => Vec::new(),
        false => crate::ssh_config::hosts(),
    }
}

/// Directories the launcher's `in` field will remember, at most.
///
/// The list is a way of not having to recall a path, so it is worth being
/// generous — but it is filtered by what is typed, and a machine with hundreds
/// of transcripts would otherwise stat every project on it on every keystroke.
const MAX_KNOWN_DIRS: usize = 40;

/// How many of those each source may fill: the projects agents ran in, and the
/// repositories the scan found.
///
/// Half each, so neither crowds the other out. With one shared cap the
/// sessions came first and took every slot on any machine with a few dozen
/// projects behind it, and the repositories — the answer to "the one I just
/// pulled" — were never offered at all. Each half is also a bound on the
/// `is_dir` checks spent filling it.
const KNOWN_SHARE: usize = MAX_KNOWN_DIRS / 2;

impl App {
    /// Open the launcher's directory field, prefilled with where it would go.
    ///
    /// Prefilled with `~` spelling rather than the absolute path: that is how
    /// the line already reads, and a field that changed what it showed the
    /// moment it became editable would look like it had lost the setting.
    ///
    /// A remote location is prefilled as `host:path` and read as one at once:
    /// what is under it on the host is what the field is for then, and asking
    /// for it reconnects a host that went offline earlier.
    pub(super) fn edit_launch_cwd(&mut self) {
        let remote = self.launch_remote.as_ref().map(|t| t.spelled());
        let prefill = remote.clone().unwrap_or_else(|| {
            self.launch_cwd
                .as_ref()
                .map(|dir| crate::util::tildify(&dir.to_string_lossy()))
                .unwrap_or_default()
        });
        self.launch_cwd_input.set(prefill);
        self.launch_cwd_bad = false;
        self.launch_cwd_why = None;
        self.launch_cwd_checking = None;
        self.launch_cwd_pristine = remote.is_none();
        self.launch_cwd_known = self.known_dirs();
        self.ssh_hosts = ssh_hosts();
        // An offline host is asked again each time the field opens: a key
        // added to the agent, a VPN brought up, is what the person went away
        // to fix.
        let offline: Vec<String> = self
            .ssh_states
            .iter()
            .filter(|(_, s)| matches!(s.conn, location::Conn::Offline(_)))
            .map(|(h, _)| h.clone())
            .collect();
        for host in offline {
            self.preconnect(&host, true);
        }
        self.launch_cwd_suggest();
        // Asked for as the field opens, and answered a moment later by a worker
        // walking the home directory. Sent every time rather than once per
        // session: a repository pulled while cctop is running is the case this
        // exists for, and the walk is tens of milliseconds on a thread of its
        // own. The list is rebuilt when the answer lands only while nothing in
        // it is highlighted — see [`App::got_repos`].
        let _ = self.tx.send(worker::Request::Repos);
        self.mode = Mode::LaunchCwd;
        self.needs_redraw = true;
    }

    /// Repositories the worker found: kept for the next opening, and shown now
    /// if the field is open with nothing highlighted.
    ///
    /// The list is a snapshot so that Enter takes the directory that is
    /// highlighted, and a walk landing a moment after the field opened must not
    /// move a highlight under the cursor. But with nothing highlighted there is
    /// nothing to move, and keeping the answer for "next time" meant the first
    /// `c` after cctop started — every scan's first answer lands after the
    /// field has opened — never offered a repository at all.
    pub(super) fn got_repos(&mut self, repos: Vec<std::path::PathBuf>) {
        self.launch_cwd_repos = repos;
        if self.mode == Mode::LaunchCwd && self.launch_cwd_pick.is_none() {
            self.launch_cwd_known = self.known_dirs();
            self.launch_cwd_suggest();
            self.needs_redraw = true;
        }
    }

    /// The field was typed in, pasted into or filled: from here on what it
    /// holds is what is being asked for.
    pub(super) fn launch_cwd_edited(&mut self) {
        self.launch_cwd_pristine = false;
        self.launch_cwd_bad = false;
        self.launch_cwd_why = None;
        self.launch_cwd_checking = None;
        self.launch_cwd_suggest();
    }

    /// The local directories among the suggestions.
    pub fn launch_cwd_dirs(&self) -> Vec<std::path::PathBuf> {
        self.launch_cwd_hits
            .iter()
            .filter_map(|h| match h {
                Hit::Dir(dir) => Some(dir.clone()),
                _ => None,
            })
            .collect()
    }

    /// Directories agents are known to have run in, last used first.
    ///
    /// Drawn from the dashboard's own rows, which is the list of projects cctop
    /// has any evidence of, plus where it was started and where this launch was
    /// already headed — those two are what "in this directory" and the line the
    /// field replaces already meant, and a suggestion list that omitted them
    /// would look like it had forgotten them.
    ///
    /// A session that ran inside a repository offers the repository, not the
    /// directory it was sitting in. `label_source` is a working directory, and
    /// an agent that started in `cctop/src/ui` records exactly that — which
    /// answers a question nobody asked, and with the deeper of the two paths to
    /// remember. The checkout is found through [`tree::locate_cached`], the same
    /// resolution the tree view groups by, so a linked worktree is understood
    /// here exactly as it is there. Two checkouts of one repository stay two
    /// answers: they are two directories to start an agent in.
    ///
    /// A directory that is in no repository is offered as itself. Work outside a
    /// checkout is real work, and this list exists to avoid the refusal that a
    /// directory cctop would itself accept would only invite.
    ///
    /// Only directories that still exist: a session's recorded project can have
    /// been moved or deleted since, and offering one leads to the refusal this
    /// list exists to avoid.
    pub(super) fn known_dirs(&self) -> Vec<std::path::PathBuf> {
        let mut recent: Vec<&Session> = self.sessions.iter().collect();
        // Last used first, so the projects worked on today are the ones on
        // screen before anything is typed. Ties go to the session that began
        // more recently, which is the only thing left that separates them: two
        // sessions touched in the same refresh would otherwise come out in
        // whichever order the rows arrived, and that order changes.
        recent.sort_by(|a, b| {
            b.last_active
                .cmp(&a.last_active)
                .then_with(|| b.started_at.cmp(&a.started_at))
        });

        let mut seen = HashSet::new();
        // Where this launch was already headed, first and whatever else is on
        // the list: these are what the line the field replaces already meant.
        let mut out: Vec<std::path::PathBuf> = self
            .launch_cwd
            .clone()
            .into_iter()
            .chain(self.launch_root.clone())
            .chain(
                self.launch_offer
                    .iter()
                    .filter_map(|c| c.cwd().map(std::path::Path::to_path_buf)),
            )
            .filter(|dir| seen.insert(dir.clone()) && dir.is_dir())
            .collect();

        // Each source filtered and then capped, not capped and then filtered:
        // a share filled with directories since deleted, or with ones already
        // listed, is a share that offers nothing.
        let sessions: Vec<std::path::PathBuf> = recent
            .iter()
            .filter(|s| !s.label_source.is_empty())
            // `locate` answers (common git dir, checkout root); the root is
            // the directory, and `None` — a relative path, or a directory in no
            // repository — leaves it as it was.
            .map(|s| match tree::locate_cached(&s.label_source) {
                Some((_, root)) => root,
                None => std::path::PathBuf::from(&s.label_source),
            })
            .filter(|dir| seen.insert(dir.clone()) && dir.is_dir())
            .take(KNOWN_SHARE)
            .collect();
        // The `seen` set does the deduplication: a repository a session has
        // already contributed is not offered twice.
        let repos: Vec<std::path::PathBuf> = self
            .launch_cwd_repos
            .iter()
            .filter(|dir| seen.insert((*dir).clone()) && dir.is_dir())
            .take(KNOWN_SHARE)
            .cloned()
            .collect();

        // Taken in turns, a project an agent ran in ahead of each repository
        // found on disk: a project worked in today is the likelier answer, so
        // it leads, but only by one — the field shows a handful of lines, and
        // a strict "sessions, then repositories" filled every one of them
        // before the first repository came up.
        let mut sessions = sessions.into_iter();
        let mut repos = repos.into_iter();
        loop {
            let (s, r) = (sessions.next(), repos.next());
            if s.is_none() && r.is_none() {
                break;
            }
            out.extend(s);
            out.extend(r);
        }
        out.truncate(MAX_KNOWN_DIRS);
        out
    }

    /// Recompute what the field is offering, after anything that changed what
    /// is in it.
    ///
    /// The pick goes with it: a suggestion highlighted for the old text would
    /// otherwise still be what Enter took, which is a directory the field is no
    /// longer showing.
    ///
    /// An untouched field asks nothing — see `launch_cwd_pristine` — so it is
    /// offered the known list rather than the filesystem under its prefill.
    ///
    /// `host:path` is completed on the host. Anything else is local, and a
    /// bare word — or nothing — is matched against the ssh hosts as well, so a
    /// host can be found by the name it is remembered by.
    pub(super) fn launch_cwd_suggest(&mut self) {
        let text: String = match self.launch_cwd_pristine {
            true => String::new(),
            false => self.launch_cwd_input.to_string(),
        };
        self.launch_cwd_hits = match location::parse(&text) {
            Typed::Remote { host, path } => self.remote_hits(host, path),
            Typed::Local(typed) => {
                let local: Vec<Hit> = dirs::suggest(typed, &self.launch_cwd_known)
                    .into_iter()
                    .map(Hit::Dir)
                    .collect();
                match typed.contains('/') || typed.starts_with(['~', '.']) {
                    true => local,
                    false => self.with_hosts(local, typed),
                }
            }
        };
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
            Some(hit) => location::fill_of(hit),
            None => {
                // An untouched field is offering the known list, which has
                // nothing to do with the prefill; completing means completing
                // what is written, so it is read as a path from here.
                if self.launch_cwd_pristine {
                    self.launch_cwd_edited();
                }
                let dirs = self.launch_cwd_dirs();
                let only_dirs = dirs.len() == self.launch_cwd_hits.len();
                let filled = match only_dirs {
                    true => dirs::complete(&self.launch_cwd_input, &dirs),
                    false => {
                        location::complete_mixed(&self.launch_cwd_input, &self.launch_cwd_hits)
                    }
                };
                match filled {
                    Some(filled) => filled,
                    None => return,
                }
            }
        };
        if filled.chars().count() > input::MAX_PATH_INPUT {
            return;
        }
        self.launch_cwd_input.set(filled);
        self.launch_cwd_edited();
        self.needs_redraw = true;
    }

    /// Take the highlighted suggestion, or what is typed if there is none.
    ///
    /// A suggestion is a directory this code listed off the disk moments ago, so
    /// it is taken without the check the typed path gets — and it is taken by
    /// filling the field with it first, so that a suggestion which has since
    /// been deleted is refused in the field like anything else.
    ///
    /// A host is not a place to start yet, so taking one puts `host:` in the
    /// field and leaves it open, connecting, on the host's directories.
    pub(super) fn take_launch_cwd(&mut self) {
        if let Some(hit) = self
            .launch_cwd_pick
            .and_then(|i| self.launch_cwd_hits.get(i))
            .cloned()
        {
            self.launch_cwd_input.set(hit.text());
            if let Hit::Host { .. } = hit {
                self.launch_cwd_edited();
                self.needs_redraw = true;
                return;
            }
        }
        self.accept_launch_cwd();
    }

    /// Take the typed directory, if it names one.
    ///
    /// Checked here rather than at launch. A path that does not exist fails
    /// somewhere inside the shim with a message about spawning, by which point
    /// the launcher is gone and there is nothing left to correct.
    pub(super) fn accept_launch_cwd(&mut self) {
        if let Typed::Remote { host, path } = location::parse(&self.launch_cwd_input) {
            let (host, path) = (host.to_string(), path.to_string());
            self.accept_remote(&host, &path);
            return;
        }
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
        self.launch_remote = None;
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
        assert_eq!(app.launch_cwd_dirs(), vec![project.clone()]);
        assert!(!app.launch_cwd_dirs().contains(&gone));

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
        assert_eq!(app.launch_cwd_dirs(), vec![deep.clone()]);
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
        app.launch_cwd_edited();
        assert!(app.launch_cwd_hits.is_empty());
        app.on_key(key(KeyCode::Enter));
        assert!(app.launch_cwd_bad);
        assert_eq!(app.mode, Mode::LaunchCwd);
    }

    /// The field offers the repository, not the directory inside it that an
    /// agent happened to be sitting in.
    ///
    /// A session's `cwd` is whatever it was launched in, and an agent that
    /// started in `cctop/src/ui` records exactly that. Offering it back answers
    /// a question nobody asked: nobody means "the ui directory of cctop" when
    /// they want to work on cctop, and it is the deeper of the two paths to
    /// have to remember.
    #[test]
    fn the_directory_field_offers_the_repository_and_not_the_directory_inside_it() {
        let root = tempfile::tempdir().expect("tempdir");
        let repo = root.path().join("cctop");
        let deep = repo.join("src/ui");
        // The `.git` sits at the checkout root, and the agent ran two levels
        // below it — which is the whole point.
        std::fs::create_dir_all(&deep).expect("deep");
        std::fs::create_dir_all(repo.join(".git")).expect("repo");
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").expect("HEAD");
        // A second checkout of the same repository, so the two are told apart.
        let other = root.path().join("cctop-worktree");
        std::fs::create_dir_all(other.join(".git")).expect("worktree");
        std::fs::write(other.join(".git/HEAD"), "ref: refs/heads/wip\n").expect("HEAD");
        let plain = root.path().join("not-a-repo");
        std::fs::create_dir_all(&plain).expect("plain");

        let mut app = test_app();
        app.sessions = vec![
            session("a", true, &deep.to_string_lossy()),
            session("b", false, &other.to_string_lossy()),
            session("c", false, &plain.to_string_lossy()),
        ];
        app.launch_root = None;
        app.launch_cwd = None;

        let known = app.known_dirs();
        assert!(
            known.contains(&repo),
            "the repository is not offered: {known:?}"
        );
        assert!(
            !known.contains(&deep),
            "the subdirectory is offered instead of the repository: {known:?}"
        );
        // A second checkout is its own answer: it is a different directory to
        // start an agent in, even though it is the same repository.
        assert!(known.contains(&other), "{known:?}");
        // And work outside any repository is still offered — it is real work,
        // and refusing it here would refuse a directory the launch itself takes.
        assert!(known.contains(&plain), "{known:?}");
    }

    /// A linked worktree is offered as itself, because it is a directory to
    /// start an agent in — and it is where this gets subtle, since a worktree's
    /// `.git` is a *file* naming a private git dir rather than a directory.
    #[test]
    fn the_directory_field_understands_a_linked_worktree() {
        let root = tempfile::tempdir().expect("tempdir");
        let repo = root.path().join("cctop");
        std::fs::create_dir_all(repo.join(".git/worktrees/wip")).expect("gitdir");
        std::fs::write(repo.join(".git/worktrees/wip/commondir"), "../..\n").expect("commondir");
        std::fs::write(
            repo.join(".git/worktrees/wip/HEAD"),
            "ref: refs/heads/wip\n",
        )
        .expect("HEAD");
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").expect("HEAD");
        // What `git worktree add` leaves behind: a `.git` file, not a directory.
        let tree = root.path().join("cctop-wip");
        std::fs::create_dir_all(&tree).expect("worktree");
        std::fs::write(
            tree.join(".git"),
            format!("gitdir: {}\n", repo.join(".git/worktrees/wip").display()),
        )
        .expect(".git file");
        // And an agent one level inside it.
        let deep = tree.join("src");

        let mut app = test_app();
        app.sessions = vec![session("a", false, &deep.to_string_lossy())];
        app.launch_root = None;
        app.launch_cwd = None;

        let known = app.known_dirs();
        assert_eq!(
            known,
            vec![tree],
            "the worktree, not the main checkout and not the directory inside it"
        );
    }

    /// A repository nobody has launched an agent in is offered anyway.
    ///
    /// This is the case the scan exists for: cctop learns about projects from
    /// the agents that ran in them, so a repository you have just pulled is
    /// exactly the one it has no evidence for. And it is offered after the ones
    /// an agent has run in — a project you worked in today is the likelier
    /// answer, and an untried repository is a fallback.
    #[test]
    fn a_repository_with_no_agent_history_is_offered_after_the_ones_that_have_one() {
        let root = tempfile::tempdir().expect("tempdir");
        let worked = root.path().join("worked");
        let fresh = root.path().join("fresh");
        std::fs::create_dir_all(worked.join(".git")).expect("worked repo");
        std::fs::create_dir_all(fresh.join(".git")).expect("fresh repo");

        let mut app = test_app();
        app.sessions = vec![session("a", false, &worked.to_string_lossy())];
        app.launch_root = None;
        app.launch_cwd = None;
        app.launch_cwd_repos = vec![fresh.clone(), worked.clone()];

        assert_eq!(
            app.known_dirs(),
            vec![worked, fresh],
            "the one with history first, and the repository nobody has tried is offered at all"
        );
    }

    /// A repository already offered because an agent ran in it is not offered
    /// again as a discovery — the list would otherwise show the same path twice.
    #[test]
    fn a_repository_known_from_a_session_is_not_offered_again_by_the_scan() {
        let root = tempfile::tempdir().expect("tempdir");
        let repo = root.path().join("project");
        std::fs::create_dir_all(repo.join(".git")).expect("repo");

        let mut app = test_app();
        app.sessions = vec![session("a", false, &repo.to_string_lossy())];
        app.launch_root = None;
        app.launch_cwd = None;
        app.launch_cwd_repos = vec![repo.clone()];

        assert_eq!(app.known_dirs(), vec![repo]);
    }

    /// The order is last used, then when the session began.
    ///
    /// `last_active` is the useful half and it was already there; `started_at`
    /// is what makes two sessions that were last touched at the same instant
    /// come out in a fixed order rather than whatever the rows happened to be in
    /// — which changes under a refresh, and a list that reshuffles while it is
    /// being read is a list nobody can point at.
    #[test]
    fn the_directory_field_orders_by_last_used_then_by_when_the_session_began() {
        let root = tempfile::tempdir().expect("tempdir");
        let a = root.path().join("a");
        let b = root.path().join("b");
        let c = root.path().join("c");
        for d in [&a, &b, &c] {
            std::fs::create_dir(d).expect("dir");
        }

        let mut app = test_app();
        // `a` and `b` were last touched at the same instant, and began an hour
        // apart, so only `started_at` can say which was first.
        let mut older = session("a", false, &a.to_string_lossy());
        older.last_active = "2026-10-06T12:00:00Z".into();
        older.started_at = "2026-10-06T09:00:00Z".into();
        let mut newer = session("b", false, &b.to_string_lossy());
        newer.last_active = "2026-10-06T12:00:00Z".into();
        newer.started_at = "2026-10-06T11:00:00Z".into();
        // Touched most recently of all, so it leads regardless of when it began.
        let mut touched = session("c", false, &c.to_string_lossy());
        touched.last_active = "2026-10-06T13:00:00Z".into();
        touched.started_at = "2026-10-05T08:00:00Z".into();
        app.sessions = vec![older, newer, touched];
        app.launch_root = None;
        app.launch_cwd = None;

        assert_eq!(
            app.known_dirs(),
            vec![c, b, a],
            "last used first, and the newer of two equally-touched sessions ahead"
        );
    }

    /// Opened on a directory, the field still offers the projects and the
    /// repositories: the prefill is where the launch was headed, not a path
    /// being asked about. The first edit is what turns it into one.
    #[test]
    fn a_prefilled_field_offers_the_known_list_until_it_is_edited() {
        let root = tempfile::tempdir().expect("tempdir");
        let here = root.path().join("here");
        let repo = root.path().join("pulled");
        std::fs::create_dir_all(here.join("inside")).expect("here");
        std::fs::create_dir_all(repo.join(".git")).expect("repo");

        let mut app = test_app();
        app.launch_root = None;
        app.launch_cwd = Some(here.clone());
        app.launch_cwd_repos = vec![repo.clone()];
        app.edit_launch_cwd();
        assert_eq!(
            app.launch_cwd_input,
            crate::util::tildify(&here.to_string_lossy())
        );
        assert_eq!(app.launch_cwd_dirs(), vec![here.clone(), repo.clone()]);

        // Typed into, it is a path again, and completes against the disk.
        app.on_key(key(KeyCode::Char('/')));
        assert_eq!(app.launch_cwd_dirs(), vec![here.join("inside")]);
    }

    /// The scan's first answer lands after the field has opened, so it is shown
    /// at once — unless something is highlighted, which must not move.
    #[test]
    fn repositories_that_land_while_the_field_is_open_are_offered_at_once() {
        let root = tempfile::tempdir().expect("tempdir");
        let repo = root.path().join("pulled");
        let worked = root.path().join("worked");
        std::fs::create_dir_all(repo.join(".git")).expect("repo");
        std::fs::create_dir_all(&worked).expect("worked");

        let mut app = test_app();
        app.sessions = vec![session("a", false, &worked.to_string_lossy())];
        app.launch_root = None;
        app.launch_cwd = None;
        app.edit_launch_cwd();
        assert_eq!(app.launch_cwd_dirs(), vec![worked.clone()]);

        app.got_repos(vec![repo.clone()]);
        assert_eq!(app.launch_cwd_dirs(), vec![worked.clone(), repo.clone()]);

        // With a pick on screen the list stays as it is, and the answer waits
        // for the next opening.
        app.edit_launch_cwd();
        app.on_key(key(KeyCode::Down));
        let before = app.launch_cwd_hits.clone();
        app.got_repos(vec![]);
        assert_eq!(app.launch_cwd_hits, before);
        assert_eq!(app.launch_cwd_pick, Some(0));
    }

    /// A long history of projects does not push every repository off the list.
    #[test]
    fn many_session_directories_do_not_crowd_out_the_repositories() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut sessions = Vec::new();
        for i in 0..(MAX_KNOWN_DIRS + 10) {
            let dir = root.path().join(format!("project-{i:02}"));
            std::fs::create_dir(&dir).expect("dir");
            sessions.push(session(&format!("s{i}"), false, &dir.to_string_lossy()));
        }
        let repo = root.path().join("pulled");
        std::fs::create_dir_all(repo.join(".git")).expect("repo");

        let mut app = test_app();
        app.sessions = sessions;
        app.launch_root = None;
        app.launch_cwd = None;
        app.launch_cwd_repos = vec![repo.clone()];

        let known = app.known_dirs();
        assert!(known.len() <= MAX_KNOWN_DIRS, "{}", known.len());
        // Second, behind the most recent project and not behind all of them,
        // so it is among the handful the field shows before anything is typed.
        assert_eq!(known.iter().position(|d| *d == repo), Some(1), "{known:?}");
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
