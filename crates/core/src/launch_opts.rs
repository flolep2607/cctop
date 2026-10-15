//! What a new agent is started with beyond its name: which model, and how much
//! it may do without asking.
//!
//! Both are flags on the harness's own command line, and each harness spells
//! them its own way — Claude Code's `--permission-mode plan` is Codex's
//! `--sandbox read-only`, and a flag one of them does not know ends the launch
//! with a usage error. So the launcher never builds a flag itself: it asks this
//! table which settings a harness takes, and asks it again for the words. A
//! harness that is not in the table takes none, and is offered none.
//!
//! The same table serves the terminal's launcher, the sandbox an ssh launch
//! runs in, and the far side of a launch on a sibling ([`crate::remote_open`]),
//! which is handed the settings as words and builds the flags itself — never
//! as flags, which would be an argv chosen by another machine.

/// How much a new agent may do before it asks.
///
/// One list across harnesses, named for what it means rather than for any one
/// harness's flag, so that the launcher can say the same thing for each. Only
/// the ones a harness can actually be started in are offered for it — see
/// [`permissions_for`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Permissions {
    /// The harness's own default: it asks.
    #[default]
    Ask,
    /// Edits go through; commands still ask. Claude Code's `acceptEdits`.
    AcceptEdits,
    /// It may read and plan, and changes nothing. Claude Code's `plan`.
    Plan,
    /// It may not write at all. Codex's read-only sandbox.
    ReadOnly,
    /// It works in its workspace without asking, sandboxed. Codex's
    /// `--full-auto`.
    Auto,
    /// Nothing is asked and nothing is sandboxed: YOLO.
    Bypass,
}

impl Permissions {
    /// The word that names it on a command line or in a URL — stable, because
    /// another machine's cctop parses it.
    pub fn word(self) -> &'static str {
        match self {
            Permissions::Ask => "ask",
            Permissions::AcceptEdits => "accept-edits",
            Permissions::Plan => "plan",
            Permissions::ReadOnly => "read-only",
            Permissions::Auto => "auto",
            Permissions::Bypass => "bypass",
        }
    }

    pub fn parse(word: &str) -> Option<Permissions> {
        [
            Permissions::Ask,
            Permissions::AcceptEdits,
            Permissions::Plan,
            Permissions::ReadOnly,
            Permissions::Auto,
            Permissions::Bypass,
        ]
        .into_iter()
        .find(|p| p.word() == word)
    }

    /// What the launcher calls it.
    pub fn label(self) -> &'static str {
        match self {
            Permissions::Ask => "ask first",
            Permissions::AcceptEdits => "accept edits",
            Permissions::Plan => "plan only",
            Permissions::ReadOnly => "read-only",
            Permissions::Auto => "auto, sandboxed",
            Permissions::Bypass => "bypass all (YOLO)",
        }
    }
}

/// The permission settings `agent` can be started in, its default first —
/// empty when it takes none cctop knows how to give it.
pub fn permissions_for(agent: &str) -> &'static [Permissions] {
    use Permissions::*;
    match agent {
        "claude" => &[Ask, AcceptEdits, Plan, Bypass],
        "codex" => &[Ask, ReadOnly, Auto, Bypass],
        _ => &[],
    }
}

/// The flags that start `agent` in `permissions`. Only ever called with a
/// pair [`permissions_for`] offers; anything else is the default's none.
fn permission_flags(agent: &str, permissions: Permissions) -> &'static [&'static str] {
    use Permissions::*;
    match (agent, permissions) {
        ("claude", AcceptEdits) => &["--permission-mode", "acceptEdits"],
        ("claude", Plan) => &["--permission-mode", "plan"],
        ("claude", Bypass) => &["--dangerously-skip-permissions"],
        ("codex", ReadOnly) => &["--sandbox", "read-only"],
        ("codex", Auto) => &["--full-auto"],
        ("codex", Bypass) => &["--dangerously-bypass-approvals-and-sandbox"],
        _ => &[],
    }
}

/// Whether `agent` takes `--model`.
pub fn takes_model(agent: &str) -> bool {
    matches!(agent, "claude" | "codex" | "opencode")
}

/// The names `agent` resolves to its current models itself, offered before
/// anything a transcript recorded: they do not go stale.
pub fn model_aliases(agent: &str) -> &'static [&'static str] {
    match agent {
        "claude" => &["opus", "sonnet", "haiku"],
        _ => &[],
    }
}

/// Whether `model` is one `agent` could be handed as its model.
///
/// What a transcript records is offered back, so this is what stands between
/// whatever a file said and an argv. Model ids are letters, digits and
/// `-._:/@[]` (`claude-opus-4-1`, `gpt-5-codex`, `anthropic/claude-sonnet-4`,
/// `sonnet[1m]`); a leading dash would read as a flag. OpenCode wants its
/// models as `provider/model` and refuses a bare one, so a bare one is not
/// offered to it.
pub fn model_ok(agent: &str, model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 100
        && !model.starts_with(['-', '.', '/'])
        && model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._:/@[]".contains(&b))
        && (agent != "opencode" || model.contains('/'))
}

/// The models a launcher offers `agent`, after its own default: the names it
/// resolves itself, then every model this machine's sessions of it have run,
/// most recently used first — the ones actually in use here.
///
/// One list for the terminal's launcher and the web's, so the two offer the
/// same eight.
pub fn models_offered(agent: &str, sessions: &[crate::session::Session]) -> Vec<String> {
    let provider = crate::pricing::Provider::parse(agent);
    let mut recent: Vec<&crate::session::Session> = sessions
        .iter()
        .filter(|s| s.remote.is_none() && Some(s.provider) == provider)
        .collect();
    recent.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    let mut seen = std::collections::HashSet::new();
    model_aliases(agent)
        .iter()
        .map(|m| m.to_string())
        .chain(recent.into_iter().map(|s| s.model.clone()))
        .filter(|m| model_ok(agent, m))
        .filter(|m| seen.insert(m.clone()))
        .take(8)
        .collect()
}

/// A launch's settings beyond the agent and the directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// `None` is the harness's own default.
    pub model: Option<String>,
    pub permissions: Permissions,
}

impl Options {
    pub fn is_default(&self) -> bool {
        self.model.is_none() && self.permissions == Permissions::Ask
    }

    /// The flags that start `agent` with these settings, or why it cannot be.
    ///
    /// Refused rather than quietly dropped: a launch asked for in plan mode
    /// and started with every permission would be the worst way for a setting
    /// to go missing.
    pub fn flags(&self, agent: &str) -> Result<Vec<String>, String> {
        let mut flags = Vec::new();
        if let Some(model) = &self.model {
            if !takes_model(agent) {
                return Err(format!(
                    "{agent} is not started with a model of cctop's choosing"
                ));
            }
            if !model_ok(agent, model) {
                return Err(format!("{model:?} is not a model {agent} can be handed"));
            }
            flags.extend(["--model".to_string(), model.clone()]);
        }
        if self.permissions != Permissions::Ask {
            if !permissions_for(agent).contains(&self.permissions) {
                return Err(format!(
                    "{agent} has no {} mode cctop can start it in",
                    self.permissions.label()
                ));
            }
            flags.extend(
                permission_flags(agent, self.permissions)
                    .iter()
                    .map(|f| f.to_string()),
            );
        }
        Ok(flags)
    }

    /// One short line for a launcher row: what differs from the default.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(model) = &self.model {
            parts.push(crate::util::short_model(model));
        }
        if self.permissions != Permissions::Ask {
            parts.push(match self.permissions {
                Permissions::Bypass => "YOLO".to_string(),
                p => p.word().to_string(),
            });
        }
        parts.join(" · ")
    }

    /// As `--model M --permissions P`, for a cctop subcommand to read back
    /// with [`Options::from_args`]. Only what differs from the default.
    pub fn to_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(model) = &self.model {
            args.extend(["--model".to_string(), model.clone()]);
        }
        if self.permissions != Permissions::Ask {
            args.extend([
                "--permissions".to_string(),
                self.permissions.word().to_string(),
            ]);
        }
        args
    }

    /// Read back what [`Options::to_args`] wrote. Anything else is refused,
    /// not skipped: the far side of a launch reads these.
    pub fn from_args(args: &[String]) -> Result<Options, String> {
        let mut options = Options::default();
        let mut args = args.iter();
        while let Some(flag) = args.next() {
            let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
            match flag.as_str() {
                "--model" if options.model.is_none() => options.model = Some(value.clone()),
                "--permissions" => {
                    options.permissions = Permissions::parse(value)
                        .ok_or_else(|| format!("{value:?} is not a permission setting"))?;
                }
                _ => return Err(format!("{flag:?} is not a launch option")),
            }
        }
        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_harness_gets_its_own_spelling_of_one_setting() {
        let opts = |model: Option<&str>, permissions| Options {
            model: model.map(str::to_string),
            permissions,
        };
        assert_eq!(
            opts(Some("opus"), Permissions::Plan).flags("claude"),
            Ok(vec![
                "--model".into(),
                "opus".into(),
                "--permission-mode".into(),
                "plan".into()
            ])
        );
        assert_eq!(
            opts(None, Permissions::Bypass).flags("codex"),
            Ok(vec!["--dangerously-bypass-approvals-and-sandbox".into()])
        );
        assert_eq!(opts(None, Permissions::Ask).flags("devin"), Ok(vec![]));
        // A setting a harness does not have is refused, never dropped.
        assert!(opts(None, Permissions::Plan).flags("codex").is_err());
        assert!(opts(None, Permissions::Bypass).flags("opencode").is_err());
        assert!(opts(Some("opus"), Permissions::Ask).flags("devin").is_err());
    }

    #[test]
    fn a_model_is_an_id_and_never_a_flag() {
        for good in [
            "opus",
            "claude-opus-4-1-20250805",
            "gpt-5-codex",
            "sonnet[1m]",
        ] {
            assert!(model_ok("claude", good), "{good}");
        }
        for bad in [
            "",
            "-x",
            "--dangerously-skip-permissions",
            "a b",
            "$(x)",
            "a;b",
            "a\nb",
        ] {
            assert!(!model_ok("claude", bad), "{bad:?}");
        }
        assert!(
            !model_ok("opencode", "claude-sonnet-4"),
            "opencode wants provider/model"
        );
        assert!(model_ok("opencode", "anthropic/claude-sonnet-4"));
    }

    #[test]
    fn options_survive_a_command_line_and_nothing_else_does() {
        let options = Options {
            model: Some("anthropic/claude-sonnet-4".into()),
            permissions: Permissions::AcceptEdits,
        };
        assert_eq!(Options::from_args(&options.to_args()), Ok(options));
        assert_eq!(Options::from_args(&[]), Ok(Options::default()));
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(Options::from_args(&args(&["--model"])).is_err());
        assert!(Options::from_args(&args(&["--permissions", "root"])).is_err());
        assert!(Options::from_args(&args(&["--exec", "x"])).is_err());
        assert!(Options::from_args(&args(&["--model", "a", "--model", "b"])).is_err());
    }

    #[test]
    fn every_permission_word_reads_back() {
        for agent in ["claude", "codex"] {
            for p in permissions_for(agent) {
                assert_eq!(Permissions::parse(p.word()), Some(*p));
            }
        }
        assert_eq!(permissions_for("claude")[0], Permissions::Ask);
    }
}
