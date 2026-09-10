//! Which account a launch runs as.
//!
//! An account is a directory the harness is pointed at by an environment
//! variable, and only harnesses that read such a variable can have one — so the
//! question "is there a profile for this command" has to be answered from the
//! command itself, before anything is spawned. That lookup is shared by the
//! launcher's cycling, the tab border's label, and the argv actually run.

use super::*;

impl App {
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
        let profiles = crate::config::profiles_for(provider);
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
        let n = crate::config::profiles_for(provider).len();
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
