//! The launcher's options for one harness: `→` on a row opens them.
//!
//! Everything a launch could be told beyond *which* agent used to be spread
//! over keys that each knew one thing — `p` for the account, `c` for the
//! directory, `F` on a dashboard row for a worktree — and the harness's own
//! flags (a model, a permission mode) were not reachable at all. This gathers
//! them per harness, offering only what that harness takes: a model and a
//! permission setting where [`cctop_core::launch_opts`] knows its flags, an
//! account where it has more than one — a sibling's own, for an agent started
//! on a sibling ([`cctop_core::peer::accounts`]) — and a worktree wherever the
//! launch is into a repository on this machine.
//!
//! Enter still starts at once, from the list or from here, with whatever the
//! options say — the defaults, until something is changed. What was changed
//! stays for the rest of the run, per harness, and the list row says so; the
//! worktree's branch is spent by the launch that creates it.

use super::*;
use cctop_core::launch_opts::{self, Options, Permissions};
use ratatui::crossterm::event::KeyEvent;

/// How long a branch or a model typed here may be.
const OPTION_MAX: usize = 100;

/// What the launcher starts one harness with, beyond the directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HarnessOptions {
    pub options: Options,
    /// A new worktree's branch, to start in. Spent by the launch that makes
    /// it: a second launch into the same branch would find it taken.
    pub branch: Option<String>,
}

impl HarnessOptions {
    /// One short line for the launcher's row: what differs from starting it
    /// bare. Empty when nothing does.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        let options = self.options.summary();
        if !options.is_empty() {
            parts.push(options);
        }
        if let Some(branch) = &self.branch {
            parts.push(format!("⑂ {branch}"));
        }
        parts.join(" · ")
    }
}

/// One line of the options panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptRow {
    Model,
    Account,
    Permissions,
    Worktree,
}

impl App {
    /// What the launcher would start `agent` with.
    pub fn options_for(&self, agent: &str) -> HarnessOptions {
        self.launch_options.get(agent).cloned().unwrap_or_default()
    }

    /// The agent the highlighted row starts fresh, by name — `None` for a
    /// row that reattaches or hands off, which take no options.
    pub fn launch_agent_name(&self) -> Option<String> {
        match self.launch_offer.get(self.launch_cursor) {
            Some(tabs::Choice::Start(argv)) => Some(location::agent_of(argv)),
            _ => None,
        }
    }

    /// The lines the highlighted harness's panel has, in order.
    pub fn option_rows(&self) -> Vec<OptRow> {
        let Some(agent) = self.launch_agent_name() else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        if launch_opts::takes_model(&agent) {
            rows.push(OptRow::Model);
        }
        // The accounts of whichever machine the agent runs on: a sibling's
        // as it named them, else this machine's.
        if self.launch_account_name().is_some() {
            rows.push(OptRow::Account);
        }
        if !launch_opts::permissions_for(&agent).is_empty() {
            rows.push(OptRow::Permissions);
        }
        rows.push(OptRow::Worktree);
        rows
    }

    /// The line under the cursor.
    pub fn option_row(&self) -> Option<OptRow> {
        self.option_rows().get(self.launch_opt_cursor).copied()
    }

    /// Open the highlighted harness's options.
    pub(super) fn open_launch_options(&mut self) {
        if self.launch_agent_name().is_none() {
            return;
        }
        self.launch_opt_cursor = 0;
        self.launch_opt_edit = None;
        self.mode = Mode::LaunchOptions;
        self.needs_redraw = true;
    }

    /// The models offered for `agent`, the harness's own default first — see
    /// [`launch_opts::models_offered`].
    pub fn model_choices(&self, agent: &str) -> Vec<Option<String>> {
        std::iter::once(None)
            .chain(
                launch_opts::models_offered(agent, &self.sessions)
                    .into_iter()
                    .map(Some),
            )
            .collect()
    }

    /// Where a worktree for this launch would fork from — the checkout the
    /// launch is headed into, and its repository — or why there is none.
    pub fn launch_worktree_point(
        &self,
    ) -> Result<(std::path::PathBuf, std::path::PathBuf), &'static str> {
        if self.launch_remote.is_some() {
            return Err("made on this machine only");
        }
        let dir = self
            .launch_cwd
            .clone()
            .or_else(|| self.launch_root.clone())
            .ok_or("no directory to fork")?;
        let (common, root) =
            tree::locate_cached(&dir.to_string_lossy()).ok_or("not in a git repository")?;
        Ok((root, tree::repo_dir(&common)))
    }

    /// ←/→ on the line under the cursor: the next or previous value.
    pub(super) fn step_option(&mut self, forward: bool) {
        let (Some(agent), Some(row)) = (self.launch_agent_name(), self.option_row()) else {
            return;
        };
        let mut entry = self.options_for(&agent);
        match row {
            OptRow::Model => {
                let choices = self.model_choices(&agent);
                let at = choices.iter().position(|m| *m == entry.options.model);
                let n = choices.len();
                let next = match (at, forward) {
                    (Some(i), true) => (i + 1) % n,
                    (Some(i), false) => (i + n - 1) % n,
                    // A model typed by hand is not in the list: either way
                    // out of it goes back to the default.
                    (None, _) => 0,
                };
                entry.options.model = choices[next].clone();
            }
            OptRow::Account => {
                // The same cycle `p` is, so the two never disagree; backwards
                // is all the way round less one.
                let n = self.launch_account_count();
                let steps = if forward { 1 } else { n.saturating_sub(1) };
                for _ in 0..steps {
                    self.cycle_launch_profile();
                }
                return;
            }
            OptRow::Permissions => {
                let all = launch_opts::permissions_for(&agent);
                let at = all
                    .iter()
                    .position(|p| *p == entry.options.permissions)
                    .unwrap_or(0);
                let n = all.len();
                entry.options.permissions = match forward {
                    true => all[(at + 1) % n],
                    false => all[(at + n - 1) % n],
                };
            }
            OptRow::Worktree => match forward {
                true => return self.edit_option(),
                false => entry.branch = None,
            },
        }
        self.launch_options.insert(agent, entry);
        self.needs_redraw = true;
    }

    /// Type the value of the line under the cursor: a model by name, or a
    /// worktree's branch.
    pub(super) fn edit_option(&mut self) {
        let (Some(agent), Some(row)) = (self.launch_agent_name(), self.option_row()) else {
            return;
        };
        let entry = self.options_for(&agent);
        let current = match row {
            OptRow::Model => entry.options.model,
            OptRow::Worktree => {
                if let Err(why) = self.launch_worktree_point() {
                    self.set_status(format!("No worktree here: {why}"));
                    return;
                }
                entry.branch
            }
            OptRow::Account | OptRow::Permissions => return,
        };
        self.launch_opt_input.set(current.unwrap_or_default());
        self.launch_opt_edit = Some(row);
        self.needs_redraw = true;
    }

    /// Take what was typed. Empty is the default: no model of cctop's
    /// choosing, no worktree.
    pub(super) fn accept_option_edit(&mut self) {
        let (Some(agent), Some(row)) = (self.launch_agent_name(), self.launch_opt_edit) else {
            return;
        };
        let typed = self.launch_opt_input.trim().to_string();
        let value = (!typed.is_empty()).then_some(typed);
        let mut entry = self.options_for(&agent);
        match row {
            OptRow::Model => {
                if let Some(model) = &value
                    && !launch_opts::model_ok(&agent, model)
                {
                    self.set_status(match agent.as_str() {
                        "opencode" => {
                            format!("{model:?} is not a model — opencode wants provider/model")
                        }
                        _ => format!("{model:?} is not a model name"),
                    });
                    return;
                }
                entry.options.model = value;
            }
            OptRow::Worktree => {
                // Asked of git now rather than at launch, so a bad name is
                // corrected here, where it was typed.
                if let Some(branch) = &value
                    && let Ok((base, _)) = self.launch_worktree_point()
                    && !branch_name_ok(&base, branch)
                {
                    self.set_status(format!("{branch:?} is not a valid branch name"));
                    return;
                }
                entry.branch = value;
            }
            OptRow::Account | OptRow::Permissions => {}
        }
        self.launch_options.insert(agent, entry);
        self.launch_opt_edit = None;
        self.needs_redraw = true;
    }

    /// The options panel's keys.
    pub(super) fn on_key_launch_options(&mut self, key: KeyEvent) {
        use ratatui::crossterm::event::KeyCode;
        if self.launch_opt_edit.is_some() {
            match key.code {
                KeyCode::Esc => self.launch_opt_edit = None,
                KeyCode::Enter => self.accept_option_edit(),
                _ => {
                    self.launch_opt_input.key(key, OPTION_MAX);
                }
            }
            self.needs_redraw = true;
            return;
        }
        let n = self.option_rows().len().max(1);
        match key.code {
            KeyCode::Esc => self.mode = Mode::Launch,
            KeyCode::Up | KeyCode::Char('k') => {
                self.launch_opt_cursor = (self.launch_opt_cursor + n - 1) % n
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.launch_opt_cursor = (self.launch_opt_cursor + 1) % n
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => self.step_option(true),
            KeyCode::Left | KeyCode::Char('h') => self.step_option(false),
            KeyCode::Char('e') => self.edit_option(),
            KeyCode::Char('p') => self.cycle_launch_profile(),
            KeyCode::Char('c') => self.edit_launch_cwd(),
            KeyCode::Enter => {
                if let Some(why) = self
                    .launch_offer
                    .get(self.launch_cursor)
                    .and_then(|c| self.launch_blocked(c))
                {
                    self.set_status(why);
                    return;
                }
                self.mode = Mode::List;
                self.launch_selected();
            }
            _ => {}
        }
        self.needs_redraw = true;
    }

    /// The argv for a fresh `argv` launch with its harness's options, or why
    /// those options cannot be given to it.
    pub(super) fn with_options(&self, argv: Vec<String>) -> Result<Vec<String>, String> {
        let agent = location::agent_of(&argv);
        let flags = self.options_for(&agent).options.flags(&agent)?;
        Ok(argv.into_iter().chain(flags).collect())
    }

    /// The worktree this launch was asked to start in, made now: its path,
    /// or `None` when none was asked for. The branch is spent either way it
    /// succeeds.
    pub(super) fn make_launch_worktree(
        &mut self,
        agent: &str,
    ) -> Result<Option<std::path::PathBuf>, String> {
        let Some(branch) = self.options_for(agent).branch else {
            return Ok(None);
        };
        let (base, repo) = self.launch_worktree_point().map_err(str::to_string)?;
        let path = launch::add_worktree(&base, &repo, &branch)?;
        if let Some(entry) = self.launch_options.get_mut(agent) {
            entry.branch = None;
        }
        Ok(Some(path))
    }
}

/// Whether git takes `branch` as a branch name, asked in `base`.
fn branch_name_ok(base: &std::path::Path, branch: &str) -> bool {
    launch::git_in(base)
        .args(["check-ref-format", "--branch", branch])
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Shown in the panel for a permission setting: the YOLO one in the colour
/// of a warning, since it is the one that cannot be taken back mid-turn.
pub(super) fn permission_is_loud(p: Permissions) -> bool {
    p == Permissions::Bypass
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{key, session, test_app};
    use ratatui::crossterm::event::KeyCode;

    fn on(app: &mut App, agents: &[&str]) {
        app.launch_offer = agents
            .iter()
            .map(|a| tabs::Choice::Start(vec![a.to_string()]))
            .collect();
        app.launch_cursor = 0;
        app.mode = Mode::Launch;
    }

    /// → opens the options of the harness under the cursor, offering only
    /// what it takes; ←/→ change them; Esc goes back to the list with them
    /// kept, and the row says what is set.
    #[test]
    fn a_harness_has_its_own_options_and_keeps_them() {
        let mut app = test_app();
        app.launch_root = None;
        on(&mut app, &["claude", "devin"]);
        app.on_key(key(KeyCode::Right));
        assert_eq!(app.mode, Mode::LaunchOptions);
        // The account line is there only on a machine with two accounts,
        // which the one running the tests may or may not be.
        let mut rows = app.option_rows();
        rows.retain(|r| *r != OptRow::Account);
        assert_eq!(rows, [OptRow::Model, OptRow::Permissions, OptRow::Worktree]);
        // Model: the default, then the names Claude resolves itself.
        app.on_key(key(KeyCode::Right));
        assert_eq!(
            app.options_for("claude").options.model.as_deref(),
            Some("opus")
        );
        app.on_key(key(KeyCode::Left));
        assert_eq!(app.options_for("claude").options.model, None);
        app.on_key(key(KeyCode::Right));
        // Permissions, both ways round.
        app.on_key(key(KeyCode::Down));
        if app.option_row() == Some(OptRow::Account) {
            app.on_key(key(KeyCode::Down));
        }
        assert_eq!(app.option_row(), Some(OptRow::Permissions));
        app.on_key(key(KeyCode::Left));
        assert_eq!(
            app.options_for("claude").options.permissions,
            Permissions::Bypass
        );
        app.on_key(key(KeyCode::Right));
        app.on_key(key(KeyCode::Right));
        app.on_key(key(KeyCode::Right));
        assert_eq!(
            app.options_for("claude").options.permissions,
            Permissions::Plan
        );
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Launch);
        assert_eq!(app.options_for("claude").summary(), "opus · plan");

        // A harness with no flags cctop knows has only the worktree.
        app.launch_cursor = 1;
        assert_eq!(app.option_rows(), [OptRow::Worktree]);
        assert_eq!(app.options_for("devin"), HarnessOptions::default());
    }

    /// On a sibling, the account line is the sibling's: its names, cycled by
    /// ←/→ and `p`, and gone where it named fewer than two — this machine's
    /// accounts never stand in for them.
    #[test]
    fn a_sibling_offers_its_own_accounts() {
        let mut app = test_app();
        on(&mut app, &["claude", "codex", "devin"]);
        app.launch_remote = Some(location::RemoteTarget {
            host: "acct-box".into(),
            path: "~".into(),
            resolved: None,
            facts: None,
            problem: None,
            unchecked: None,
            peer: true,
        });
        cctop_core::peer::set_accounts("acct-box", "claude", &["default", "work", "spare"]);
        cctop_core::peer::set_accounts("acct-box", "codex", &["default"]);
        app.open_launch_options();
        assert_eq!(
            app.option_rows(),
            [
                OptRow::Model,
                OptRow::Account,
                OptRow::Permissions,
                OptRow::Worktree
            ]
        );
        assert_eq!(app.launch_account_name().as_deref(), Some("default"));
        assert_eq!(
            app.peer_account_to_send("acct-box", Provider::Claude),
            None,
            "untouched, the sibling's own default"
        );
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Right));
        assert_eq!(app.launch_account_name().as_deref(), Some("work"));
        app.on_key(key(KeyCode::Left));
        app.on_key(key(KeyCode::Left));
        assert_eq!(app.launch_account_name().as_deref(), Some("spare"));
        app.on_key(key(KeyCode::Char('p')));
        assert_eq!(app.launch_account_name().as_deref(), Some("default"));
        app.on_key(key(KeyCode::Char('p')));
        assert_eq!(
            app.peer_account_to_send("acct-box", Provider::Claude)
                .as_deref(),
            Some("work"),
            "what the launch carries"
        );
        // An account the sibling no longer names falls back to its first,
        // and is not asked for by name.
        cctop_core::peer::set_accounts("acct-box", "claude", &["default", "spare"]);
        assert_eq!(app.launch_account_name().as_deref(), Some("default"));
        assert_eq!(app.peer_account_to_send("acct-box", Provider::Claude), None);

        // One account, or a harness with none: nothing to pick, nothing sent.
        app.launch_cursor = 1;
        assert!(!app.option_rows().contains(&OptRow::Account));
        assert_eq!(app.peer_account_to_send("acct-box", Provider::Codex), None);
        app.launch_cursor = 2;
        assert!(!app.option_rows().contains(&OptRow::Account));
        // A sibling that never said: none either.
        assert_eq!(
            app.peer_account_to_send("acct-silent", Provider::Claude),
            None
        );
    }

    /// The models offered are the harness's own names and what this
    /// machine's sessions of it ran — never another harness's, and never a
    /// string that is not a model.
    #[test]
    fn the_models_offered_are_the_ones_in_use_here() {
        let mut app = test_app();
        let mut a = session("a", false, "/w");
        a.model = "claude-opus-4-1-20250805".into();
        a.last_active = "2026-10-02T00:00:00Z".into();
        let mut b = session("b", false, "/w");
        b.model = "--dangerously-skip-permissions".into();
        let mut c = session("c", false, "/w");
        c.provider = Provider::Codex;
        c.model = "gpt-5-codex".into();
        app.sessions = vec![a, b, c];
        assert_eq!(
            app.model_choices("claude"),
            [
                None,
                Some("opus".into()),
                Some("sonnet".into()),
                Some("haiku".into()),
                Some("claude-opus-4-1-20250805".into()),
            ]
        );
        assert_eq!(
            app.model_choices("codex"),
            [None, Some("gpt-5-codex".into())]
        );
    }

    /// A model typed by hand is checked where it is typed; a good one is
    /// taken and reaches the argv, after the agent and before nothing else.
    #[test]
    fn a_typed_model_is_checked_and_reaches_the_command_line() {
        let mut app = test_app();
        on(&mut app, &["claude"]);
        app.open_launch_options();
        app.on_key(key(KeyCode::Char('e')));
        assert_eq!(app.launch_opt_edit, Some(OptRow::Model));
        for c in "-x".chars() {
            app.on_key(key(KeyCode::Char(c)));
        }
        app.on_key(key(KeyCode::Enter));
        assert_eq!(
            app.launch_opt_edit,
            Some(OptRow::Model),
            "refused, still editing"
        );
        app.launch_opt_input.set("sonnet[1m]".to_string());
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.launch_opt_edit, None);
        assert_eq!(
            app.with_options(vec!["claude".into()]),
            Ok(vec!["claude".into(), "--model".into(), "sonnet[1m]".into()])
        );
    }

    /// The worktree line forks the repository the launch is headed into, and
    /// is refused — with why — anywhere that is not one.
    #[test]
    fn a_worktree_is_offered_only_into_a_repository_here() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("mkdir");
        let git = |args: &[&str]| {
            let out = launch::git_in(&repo)
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .output()
                .expect("git");
            assert!(out.status.success(), "{args:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["commit", "-q", "--allow-empty", "-m", "root"]);

        let mut app = test_app();
        on(&mut app, &["devin"]);
        app.launch_cwd = Some(tmp.path().to_path_buf());
        app.open_launch_options();
        app.on_key(key(KeyCode::Right));
        assert_eq!(app.launch_opt_edit, None, "not a repository: no field");

        app.launch_cwd = Some(repo.clone());
        app.on_key(key(KeyCode::Right));
        assert_eq!(app.launch_opt_edit, Some(OptRow::Worktree));
        app.launch_opt_input.set("bad..name".to_string());
        app.on_key(key(KeyCode::Enter));
        assert_eq!(
            app.launch_opt_edit,
            Some(OptRow::Worktree),
            "git refused it"
        );
        app.launch_opt_input.set("try-it".to_string());
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.options_for("devin").branch.as_deref(), Some("try-it"));
        assert_eq!(app.options_for("devin").summary(), "⑂ try-it");

        let made = app
            .make_launch_worktree("devin")
            .expect("made")
            .expect("a path");
        assert!(made.ends_with(".claude/worktrees/try-it"), "{made:?}");
        assert_eq!(app.options_for("devin").branch, None, "spent");
        assert_eq!(app.make_launch_worktree("devin"), Ok(None));
    }
}
