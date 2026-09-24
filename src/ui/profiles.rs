//! Which account a launch runs as.
//!
//! An account is a directory the harness is pointed at by an environment
//! variable, and only harnesses that read such a variable can have one — so the
//! question "is there a profile for this command" has to be answered from the
//! command itself, before anything is spawned. That lookup is shared by the
//! launcher's cycling, the tab border's label, and the argv actually run.

use super::*;

impl App {
    /// Open the add-account popup on its first step, the name.
    pub(super) fn open_add_account(&mut self) {
        self.add_account = AddAccount::default();
        self.mode = Mode::AddAccount;
        self.needs_redraw = true;
    }

    /// The name is in: check it, and ask which kind of account it is.
    pub(super) fn accept_account_name(&mut self) {
        let name = self.add_account.name.trim().to_string();
        if !crate::quota::valid_account_name(&name) {
            self.set_status("An account name is letters, digits, - _ and . only");
            return;
        }
        // `~/.claude-default` would be found as a second `default` beside
        // `~/.claude` — the same refusal `cctop --add-account` makes.
        if name == "default" {
            self.set_status("`default` is ~/.claude itself — name this account something else");
            return;
        }
        self.add_account.name = name.into();
        self.add_account.named = true;
        self.needs_redraw = true;
    }

    /// Start the kind of account that was picked, in the popup's terminal.
    pub(super) fn start_add_account(&mut self, kind: AccountKind) {
        let flow = &mut self.add_account;
        let (argv, cwd, what) = match kind {
            AccountKind::Token => (
                vec!["claude".to_string(), "setup-token".to_string()],
                None,
                "claude setup-token",
            ),
            AccountKind::Login => {
                let dir = crate::config::claude_login_dir(&flow.name);
                if crate::quota::login_landed(&dir) {
                    flow.outcome = Some(Err(format!(
                        "{} is already logged in — it is in the launcher as {}.",
                        crate::util::tildify(&dir.to_string_lossy()),
                        flow.name
                    )));
                    self.needs_redraw = true;
                    return;
                }
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    flow.outcome = Some(Err(format!("Could not create {}: {e}", dir.display())));
                    self.needs_redraw = true;
                    return;
                }
                // `env` in the argv rather than on the child, the way every
                // other account launch is spelled: see `argv_under_profile`.
                (
                    vec![
                        "env".to_string(),
                        format!("CLAUDE_CONFIG_DIR={}", dir.display()),
                        "claude".to_string(),
                        "auth".to_string(),
                        "login".to_string(),
                        "--claudeai".to_string(),
                    ],
                    Some(dir),
                    "claude auth login",
                )
            }
        };
        flow.kind = Some(kind);
        match tabs::Pane::launch(&argv, cwd.as_deref(), tabs::Own::Cctop) {
            Ok(pane) => flow.pane = Some(pane),
            // A `claude` that cannot be started here: the command-line
            // walkthrough does the token half of the same job.
            Err(e) => {
                flow.outcome = Some(Err(format!(
                    "Could not run `{what}` here ({e}). In a terminal, \
                     `cctop --add-account {}` adds it as a token.",
                    flow.name
                )))
            }
        }
        self.needs_redraw = true;
    }

    /// Feed the popup's terminal, and notice the moment the account exists: a
    /// token when it is printed, a login when its process ends having written
    /// its credentials.
    pub(super) fn pump_add_account(&mut self) {
        let flow = &mut self.add_account;
        let Some(pane) = flow.pane.as_mut() else {
            return;
        };
        if pane.view.pump() {
            self.needs_redraw = true;
            let screen = pane.view.parser.screen();
            // Read again on every draw rather than kept from the first: the
            // link is printed over several rows, and the first sighting can be
            // before the last of them has arrived — which kept half a URL.
            if let Some(link) = crate::quota::link_on_screen(screen) {
                flow.link = Some(link);
            }
            if flow.kind == Some(AccountKind::Token)
                && let Some(token) = crate::quota::token_on_screen(screen)
            {
                // Its job is done, and a process holding a fresh token has no
                // reason to outlive the popup that asked for it.
                flow.pane = None;
                flow.outcome = Some(
                    crate::quota::save_token(&flow.name, &token)
                        .map(|()| flow.name.to_string())
                        .map_err(|e| format!("Could not save the token: {e}")),
                );
                return;
            }
        }
        if flow.outcome.is_some() || !pane.view.closed() {
            return;
        }
        self.needs_redraw = true;
        flow.outcome = Some(match flow.kind {
            Some(AccountKind::Login)
                if crate::quota::login_landed(&crate::config::claude_login_dir(&flow.name)) =>
            {
                // Gone, not left on screen: it succeeded, and the popup's
                // outcome says so better than its last frame.
                flow.pane = None;
                Ok(flow.name.to_string())
            }
            // Left on screen rather than dropped: whatever it said before it
            // went is the explanation.
            Some(AccountKind::Login) => {
                Err("`claude auth login` ended without logging in.".to_string())
            }
            _ => Err("`claude setup-token` ended without printing a token.".to_string()),
        });
    }

    /// The profile a launch would use, or `None` when the highlighted command
    /// takes none — or takes one but has only a single account, so there is
    /// nothing to choose between.
    pub fn launch_profile(&self) -> Option<&'static crate::config::Profile> {
        self.chosen_profile(self.launch_provider()?)
    }

    /// Which harness the highlighted choice would start, when it is one whose
    /// account cctop can pick.
    pub(super) fn launch_provider(&self) -> Option<Provider> {
        match self.launch_offer.get(self.launch_cursor) {
            Some(tabs::Choice::Start(argv)) => Self::profile_provider(argv),
            _ => None,
        }
    }

    /// The profile `provider` would be started under, or `None` when it has
    /// only the one and so nothing to choose between.
    pub fn chosen_profile(&self, provider: Provider) -> Option<&'static crate::config::Profile> {
        let profiles = crate::config::launchable_for(provider);
        if profiles.len() <= 1 {
            return None;
        }
        let at = self.launch_profile.get(&provider).copied().unwrap_or(0);
        profiles.get(at).copied()
    }

    /// Move to the next profile of the highlighted harness. Wraps, because with
    /// two — which is the case this exists for — a key that toggles is the whole
    /// interaction.
    pub(super) fn cycle_launch_profile(&mut self) {
        let Some(provider) = self.launch_provider() else {
            return;
        };
        let n = crate::config::launchable_for(provider).len();
        if n > 1 {
            let at = self.launch_profile.entry(provider).or_insert(0);
            *at = (*at + 1) % n;
            self.save_prefs();
            self.needs_redraw = true;
        }
    }

    /// Which harness `argv` starts, when it is one whose account is selected by
    /// an environment variable — and so one a profile means something to.
    ///
    /// `$CLAUDE_CONFIG_DIR` is Claude's and `$CODEX_HOME` is Codex's; putting
    /// either in front of anything else would be a promise the env var cannot
    /// keep.
    pub(super) fn profile_provider(argv: &[String]) -> Option<Provider> {
        let command = argv
            .first()
            .map(|c| c.rsplit(['/', '\\']).next().unwrap_or(c))?;
        match command.strip_suffix(".exe").unwrap_or(command) {
            "claude" => Some(Provider::Claude),
            "codex" => Some(Provider::Codex),
            _ => None,
        }
    }

    /// Put the chosen profile in front of the command that will read it.
    ///
    /// `env VAR=value cmd` rather than plumbing an environment through every
    /// spawn path: the same argv is handed to rmux, to a pty cctop owns, and to
    /// `rmux new-session`, and `env` is understood identically by all three.
    /// [`tabs::label_of`] drops the prefix again so the tab is named after the
    /// agent rather than after how it was started.
    pub(super) fn with_profile(&self, argv: Vec<String>) -> Vec<String> {
        let profile = Self::profile_provider(&argv).and_then(|p| self.chosen_profile(p));
        let Some(profile) = profile else {
            return argv;
        };
        crate::config::argv_under_profile(argv, profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent};

    /// The name, then the question: Enter on a good name asks which kind of
    /// account rather than starting anything, Backspace goes back to the name,
    /// and a bad name is refused where it was typed.
    #[test]
    fn the_add_account_popup_asks_which_kind_after_the_name() {
        let mut app = crate::ui::tests::test_app();
        app.open_add_account();
        for c in "work 2".chars() {
            app.on_key(KeyEvent::from(KeyCode::Char(c)));
        }
        app.on_key(KeyEvent::from(KeyCode::Enter));
        assert!(!app.add_account.named, "a name with a space was accepted");

        app.add_account.name = "work2".into();
        app.on_key(KeyEvent::from(KeyCode::Enter));
        assert!(app.add_account.named);
        assert!(
            app.add_account.pane.is_none(),
            "something started before the choice"
        );
        assert_eq!(app.add_account.kind, None);

        app.on_key(KeyEvent::from(KeyCode::Backspace));
        assert!(
            !app.add_account.named,
            "Backspace did not go back to the name"
        );
        assert_eq!(app.add_account.name, "work2", "going back lost the name");
        assert_eq!(app.mode, Mode::AddAccount);
    }
    /// Which harness an argv names, and so which variable may be put in front
    /// of it. Getting this wrong is not a cosmetic error: `CODEX_HOME` in front
    /// of `claude` is ignored, and the agent then runs as an account the pane
    /// says it is not.
    #[test]
    fn only_a_harness_with_a_config_variable_takes_a_profile() {
        let of = |c: &str| App::profile_provider(&[c.to_string()]);
        assert_eq!(of("claude"), Some(Provider::Claude));
        assert_eq!(of("codex"), Some(Provider::Codex));
        // Found by basename, however it was spelled on the way in.
        assert_eq!(of("/usr/local/bin/codex"), Some(Provider::Codex));
        assert_eq!(of("codex.exe"), Some(Provider::Codex));
        assert_eq!(of(r"C:\tools\claude.exe"), Some(Provider::Claude));
        // No such variable: a profile would be a setting that did nothing.
        assert_eq!(of("cursor-agent"), None);
        assert_eq!(of("gemini"), None);
        // Not a prefix match — `codex-something` is not codex.
        assert_eq!(of("codexa"), None);
        assert_eq!(App::profile_provider(&[]), None);
    }

    /// The prefix a launch actually carries, per harness.
    #[test]
    fn a_profile_reaches_the_agent_as_its_own_variable() {
        let under = |dir: &str, provider, command: &str| {
            let profile = crate::config::Profile {
                provider,
                name: "work".to_string(),
                dir: std::path::PathBuf::from(dir),
                source: crate::config::AccountSource::Directory,
            };
            crate::config::argv_under_profile(vec![command.to_string()], &profile)
        };
        assert_eq!(
            under("/home/x/.codex-work", Provider::Codex, "codex"),
            ["env", "CODEX_HOME=/home/x/.codex-work", "codex"]
        );
        assert_eq!(
            under("/home/x/.claude-work", Provider::Claude, "claude"),
            ["env", "CLAUDE_CONFIG_DIR=/home/x/.claude-work", "claude"]
        );
        // A harness with no variable is handed its command untouched rather
        // than an `env` prefix that promises something.
        assert_eq!(
            under("/home/x/.cursor", Provider::Cursor, "cursor-agent"),
            ["cursor-agent"]
        );
    }
}
