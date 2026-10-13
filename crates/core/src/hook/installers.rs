//! Asking each harness to call `cctop hook`, in its own dialect: Claude
//! Code's (and Gemini's and Cursor's) JSON settings, Codex's TOML, the
//! OpenCode plugin and Claude Code's `/yolo` command.

use super::*;

// ---------------------------------------------------------------------------
// Installation
// ---------------------------------------------------------------------------

/// Events cctop asks Claude Code to tell it about: all of them but one.
///
/// This used to be a deliberately small set, on the grounds that every hook
/// fire costs the agent a process spawn and an event cctop would only use to
/// redraw something it can already see is not worth the fork. That reasoning
/// was right about the fork and wrong about the cost of being conservative: a
/// settings file that names only the events this version knows about is one
/// that has to be rewritten every time cctop learns another, on every machine,
/// by everybody. [`repair`] softens that — it tops an install up on the way up
/// — but it can only add what *this* binary knows to ask for, so an agent
/// already running still says nothing about the rest until it restarts.
///
/// So the settings file now says "tell cctop everything", and the binary
/// decides what any of it means. Adding an event cctop acts on is a change to
/// [`signal_of`] alone, and takes effect on the next fire rather than the next
/// install.
///
/// What that costs is real and worth naming: `MessageDisplay` fires once per
/// batch of streamed lines, several times per assistant message, where the rest
/// fire a handful of times in a session. It buys the one thing nothing else
/// says — a long answer with no tool calls in it raises nothing at all between
/// `UserPromptSubmit` and `Stop`, and a turn with no events is one whose
/// liveness falls back to guessing at a still screen.
///
/// `WorktreeCreate` is the single exception, and it is not about cost.
/// It does not observe worktree creation — it **replaces** it. Claude Code
/// stops calling `git worktree` and takes the path from the hook's stdout, and
/// "if the hook fails or produces no path, worktree creation fails with an
/// error". This hook writes nothing to stdout by construction (see the module
/// docs), so installing it would break `claude --worktree`, every subagent with
/// `isolation: "worktree"`, and every backgrounded session Claude Code isolates
/// in one. It is the one event on offer that could take a session down, which
/// is the thing this module exists not to do.
pub(super) const CLAUDE_EVENTS: &[&str] = &[
    "Setup",
    "SessionStart",
    "InstructionsLoaded",
    "UserPromptSubmit",
    "UserPromptExpansion",
    "MessageDisplay",
    "PreToolUse",
    "PermissionRequest",
    "PermissionDenied",
    "PostToolUse",
    "PostToolUseFailure",
    "PostToolBatch",
    "Notification",
    "SubagentStart",
    "SubagentStop",
    "TaskCreated",
    "TaskCompleted",
    "Stop",
    "StopFailure",
    "TeammateIdle",
    "ConfigChange",
    "CwdChanged",
    "DirectoryAdded",
    "FileChanged",
    "WorktreeRemove",
    "PreCompact",
    "PostCompact",
    "SessionEnd",
    "Elicitation",
    "ElicitationResult",
];

/// The same list in Gemini CLI's vocabulary.
///
/// It has no `Stop`: the end of a turn is the end of its agent loop, which is
/// `AfterAgent`.
///
/// `AfterTool` used to be left out, on the grounds that `BeforeTool` already
/// says the agent is working. It does not: it says a tool call is *in flight*,
/// which cctop reads as a held permission prompt once the screen goes still,
/// and Gemini has nothing else to say the call came back. Without the pair, a
/// Gemini session spent every tool call looking like it was asking.
pub(super) const GEMINI_EVENTS: &[&str] = &[
    "BeforeAgent",
    "AfterAgent",
    "BeforeTool",
    "AfterTool",
    "Notification",
    "SessionStart",
    "SessionEnd",
    "PreCompress",
];

/// The same list in Cursor's vocabulary.
///
/// `beforeShellExecution` stands in for a tool call: it is the one Cursor
/// reports that a monitor can do anything with, and skipping `beforeReadFile`
/// and `afterFileEdit` keeps the fan-out down to roughly what the other
/// harnesses cost.
///
/// Cursor also reads Claude Code's `settings.json` for hooks of its own accord,
/// so with both installed each moment arrives twice. That is a wasted process
/// spawn and nothing worse — the second event carries the same fact as the
/// first, and applying it again changes nothing.
pub(super) const CURSOR_EVENTS: &[&str] = &[
    "stop",
    "beforeSubmitPrompt",
    "beforeShellExecution",
    "sessionStart",
    "sessionEnd",
    "preCompact",
    "subagentStop",
];

/// The same list in Codex's vocabulary.
///
/// Codex grew a hook framework of its own, and it borrowed Claude Code's
/// spelling wholesale — same event names, same nested `hooks` object, same JSON
/// on stdin — so everything that reads a Claude Code event already reads a
/// Codex one. What it does not have is the pair of failure events: Codex's
/// `PostToolUse` fires for a command that exited non-zero as well as one that
/// succeeded, and a turn that dies has no second ending, so nothing here needs
/// to close those out.
///
/// `notify` stays installed alongside. It says only that a turn finished, but it
/// says it without being trusted first, and until somebody has run `/hooks`
/// inside Codex these hooks deliver nothing at all.
const CODEX_EVENTS: &[&str] = &[
    "Stop",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "SessionStart",
    "SessionEnd",
    "PreCompact",
    "PostCompact",
    "SubagentStop",
];

/// The word that separates the binary from the event in a command cctop wrote.
///
/// Entries are recognised by their command text rather than by a marker key:
/// the file belongs to the user and their other tools, and its schema has no
/// room for a comment.
const MARKER: &str = " hook ";

/// The same for the one entry that may decide: `<cctop> yolo-hook <Event>`.
/// See [`yolo_hook`].
const YOLO_MARKER: &str = " yolo-hook ";

/// The events `cctop yolo-hook` is installed for, beside the observer, in
/// Claude Code's settings: a permission dialog about to go up, the only
/// moment YOLO answers — see [`yolo_hook`] for why not `PreToolUse` — and the
/// expansion of a typed `/yolo`, the only command it answers.
pub(super) const CLAUDE_DECIDING: &[&str] = &["PermissionRequest", "UserPromptExpansion"];

/// How a missing `yolo-hook` entry is named in a health report.
pub(super) fn yolo_hook_label(event: &str) -> &'static str {
    match event {
        "UserPromptExpansion" => "UserPromptExpansion (yolo-hook)",
        _ => "PermissionRequest (yolo-hook)",
    }
}

/// The matcher a deciding entry is installed with, where it has one: the
/// `UserPromptExpansion` entry fires for `/yolo` only, so Claude Code spawns
/// nothing for any other command. [`typed_yolo`] checks the name anyway — a
/// matcher in a file the person can edit is a filter, not a guarantee.
pub(super) fn deciding_matcher(event: &str) -> Option<&'static str> {
    (event == "UserPromptExpansion").then_some(YOLO_COMMAND)
}

/// The argument that tells `cctop hook` its payload is a Codex one, arriving in
/// argv rather than on stdin.
pub(super) const CODEX_SELECTOR: &str = "codex";

/// The same, for the plugin cctop installs into OpenCode.
const OPENCODE_SELECTOR: &str = "opencode";

/// Whether this first argument means "the event is the next argument" rather
/// than naming an event to read from stdin.
pub(super) fn is_argv_payload(word: &str) -> bool {
    matches!(word, CODEX_SELECTOR | OPENCODE_SELECTOR)
}

/// An agent cctop can ask to report on itself.
///
/// Each one is asked in its own way — three different config files, a `notify`
/// program, and a plugin — but every one of them ends up spawning
/// `cctop hook`, and what comes back is the same [`Event`] whichever it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Gemini,
    Cursor,
    Codex,
    OpenCode,
}

/// Every harness an install touches, in the order they are reported.
pub const HARNESSES: [Harness; 5] = [
    Harness::Claude,
    Harness::Gemini,
    Harness::Cursor,
    Harness::Codex,
    Harness::OpenCode,
];

/// How a harness spells one entry in its `hooks` object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    /// Claude Code and Gemini CLI: a wrapper holding a list of commands, so a
    /// `matcher` can select which tools an entry applies to.
    Nested,
    /// Cursor: the command entry itself, directly under the event name, in a
    /// file whose root also carries a `version`.
    Flat,
}

/// What an install has to write for one harness in one scope.
pub(super) enum Config {
    /// A JSON settings file with a `hooks` object, one array per event.
    Json {
        path: PathBuf,
        shape: Shape,
        events: &'static [&'static str],
        /// The events that also get `cctop yolo-hook`, beside the observer.
        deciding: &'static [&'static str],
    },
    /// Codex's single `notify` program, in TOML.
    Notify(PathBuf),
    /// A plugin file cctop owns outright and can therefore write and delete
    /// whole, rather than merging into somebody else's document.
    Plugin(PathBuf),
    /// Claude Code's `/yolo`, a skill file cctop owns outright like the
    /// plugin. `commands` is where a command file of the same name would
    /// be, which the skill would hide; `hooks` is the settings file of the
    /// same scope, whose state decides whether a missing skill is an install
    /// that is short or no install at all.
    Skill {
        path: PathBuf,
        commands: PathBuf,
        hooks: PathBuf,
    },
}

impl Config {
    /// The file this writes.
    pub(super) fn path(&self) -> &Path {
        match self {
            Config::Json { path, .. }
            | Config::Notify(path)
            | Config::Plugin(path)
            | Config::Skill { path, .. } => path,
        }
    }

    /// Write cctop into this one file, and say what happened to it.
    pub(super) fn install(&self, exe: &str) -> anyhow::Result<String> {
        match self {
            Config::Json {
                path,
                shape,
                events,
                deciding,
            } => {
                json_install(path, *shape, events, deciding, exe)?;
                let note = match self.note() {
                    Some(note) => format!(" — {note}"),
                    None => String::new(),
                };
                Ok(format!(
                    "added {} hooks to {}{note}",
                    events.len(),
                    path.display()
                ))
            }
            Config::Notify(path) => {
                notify_install(path, exe)?;
                Ok("notify points at cctop".to_string())
            }
            Config::Plugin(path) => {
                plugin_install(path, exe)?;
                Ok(format!("wrote plugin {}", path.display()))
            }
            Config::Skill { path, commands, .. } => {
                skill_install(path, commands, exe)?;
                Ok(format!("added {SKILL_LABEL} at {}", path.display()))
            }
        }
    }

    /// Take cctop back out of this one file.
    fn remove(&self) -> anyhow::Result<String> {
        match self {
            Config::Json { path, .. } => {
                let removed = json_remove(path)?;
                Ok(format!("removed {removed} hooks from {}", path.display()))
            }
            Config::Notify(path) => notify_remove(path),
            Config::Plugin(path) => {
                let removed = plugin_remove(path)?;
                Ok(match removed {
                    true => format!("removed plugin {}", path.display()),
                    false => "nothing of cctop's installed".to_string(),
                })
            }
            Config::Skill { path, .. } => Ok(match skill_remove(path)? {
                true => format!("removed {SKILL_LABEL} from {}", path.display()),
                false => format!("no {SKILL_LABEL} of cctop's installed"),
            }),
        }
    }

    /// What is in this one file right now.
    pub(super) fn health(&self) -> Health {
        match self {
            Config::Json {
                path,
                shape,
                events,
                deciding,
            } => json_health(path, *shape, events, deciding),
            Config::Notify(path) => notify_health(path),
            Config::Plugin(path) => plugin_health(path),
            Config::Skill {
                path,
                commands,
                hooks,
            } => skill_health(path, commands, hooks),
        }
    }

    /// What a reader needs told about this file beyond its state.
    ///
    /// Only Codex has one: its hooks are inert until a person has looked at
    /// them, and an install that reads as done while delivering nothing is
    /// worse than one that says what is left to do.
    pub(super) fn note(&self) -> Option<&'static str> {
        match self {
            Config::Json { events, .. } if std::ptr::eq(*events, CODEX_EVENTS) => {
                Some("trust them with /hooks inside Codex")
            }
            _ => None,
        }
    }
}

impl Harness {
    /// The name to print. Long enough to be unambiguous in a list of five.
    pub fn label(self) -> &'static str {
        match self {
            Harness::Claude => "Claude Code",
            Harness::Gemini => "Gemini CLI",
            Harness::Cursor => "Cursor",
            Harness::Codex => "Codex",
            Harness::OpenCode => "OpenCode",
        }
    }

    /// Every file an install writes for this harness at this scope, in the order
    /// they are reported. Empty where the harness has no such scope.
    ///
    /// A list because Codex has two: hooks, which say everything, and `notify`,
    /// which says only that a turn ended but works the moment it is written.
    /// Every other harness has exactly one.
    pub(super) fn configs(self, scope: &Scope) -> Vec<Config> {
        let project = match scope {
            Scope::User => None,
            Scope::Project(dir) => Some(dir.as_path()),
        };
        match self {
            Harness::Claude => {
                // Honours the same `$CLAUDE_CONFIG_DIR` override as the rest
                // of cctop.
                let hooks = match project {
                    None => crate::config::CLAUDE_CONFIG_DIR.join("settings.json"),
                    Some(dir) => dir.join(".claude").join("settings.json"),
                };
                let settings = Config::Json {
                    path: hooks.clone(),
                    shape: Shape::Nested,
                    events: CLAUDE_EVENTS,
                    deciding: CLAUDE_DECIDING,
                };
                match project {
                    // `/yolo` is one person's switch for their own sessions,
                    // so it is theirs alone: a project's `.claude/` is checked
                    // in, and a command that hands out permission is not one
                    // to give everyone who clones it.
                    Some(_) => vec![settings],
                    None => {
                        let dir = &*crate::config::CLAUDE_CONFIG_DIR;
                        vec![
                            settings,
                            Config::Skill {
                                path: dir.join("skills").join("yolo").join("SKILL.md"),
                                commands: dir.join("commands").join("yolo.md"),
                                hooks,
                            },
                        ]
                    }
                }
            }
            Harness::Gemini => vec![Config::Json {
                path: match project {
                    None => crate::config::GEMINI_HOME.join("settings.json"),
                    Some(dir) => dir.join(".gemini").join("settings.json"),
                },
                shape: Shape::Nested,
                events: GEMINI_EVENTS,
                deciding: &[],
            }],
            Harness::Cursor => vec![Config::Json {
                path: match project {
                    None => crate::config::CURSOR_HOME.join("hooks.json"),
                    Some(dir) => dir.join(".cursor").join("hooks.json"),
                },
                shape: Shape::Flat,
                events: CURSOR_EVENTS,
                deciding: &[],
            }],
            Harness::Codex => {
                let hooks = Config::Json {
                    path: match project {
                        None => crate::config::CODEX_HOME.join("hooks.json"),
                        Some(dir) => dir.join(".codex").join("hooks.json"),
                    },
                    shape: Shape::Nested,
                    events: CODEX_EVENTS,
                    // ponytail: Codex keeps the key press. Its hooks take
                    // Claude Code's shape, but whether it honours a decision
                    // from one has not been checked against it.
                    deciding: &[],
                };
                match project {
                    // `notify` is a single machine-wide program, so only the
                    // user scope has one; a project install is hooks alone.
                    Some(_) => vec![hooks],
                    None => vec![
                        hooks,
                        Config::Notify(crate::config::CODEX_HOME.join("config.toml")),
                    ],
                }
            }
            Harness::OpenCode => vec![Config::Plugin(
                match project {
                    None => crate::config::OPENCODE_CONFIG_DIR.clone(),
                    Some(dir) => dir.join(".opencode"),
                }
                .join("plugins")
                .join(PLUGIN_FILE),
            )],
        }
    }

    /// The first file an install for this scope would write.
    ///
    /// For the tests: the report names each file from its own entry.
    #[cfg(test)]
    pub(super) fn config_file(self, scope: &Scope) -> Option<PathBuf> {
        self.configs(scope)
            .first()
            .map(|config| config.path().to_path_buf())
    }

    /// Ask this harness to report, leaving everything else in its config alone.
    pub(super) fn install(self, scope: &Scope, exe: &str) -> Vec<String> {
        let configs = self.configs(scope);
        if configs.is_empty() {
            return vec![format!("{}: has no {} scope", self.label(), scope.label())];
        }
        configs
            .iter()
            .map(|config| match config.install(exe) {
                Ok(what) => format!("{}: {what}", self.label()),
                Err(e) => format!("{}: {e}", self.label()),
            })
            .collect()
    }

    /// The state of the first file this harness writes, which is the whole of
    /// it for every harness but Codex.
    ///
    /// For the tests only: everything else wants one entry per file, which is
    /// what [`harness_status`] gives it.
    #[cfg(test)]
    pub(super) fn health(self, scope: &Scope) -> Option<Health> {
        self.configs(scope).first().map(Config::health)
    }

    /// Take cctop back out, leaving every other entry untouched.
    fn remove(self, scope: &Scope) -> Vec<String> {
        self.configs(scope)
            .iter()
            .map(|config| match config.remove() {
                Ok(what) => format!("{}: {what}", self.label()),
                Err(e) => format!("{}: {e}", self.label()),
            })
            .collect()
    }
}

/// Which settings file an install writes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The per-user config of every harness — every session on this machine.
    User,
    /// One project's checked-in config: sessions started in that directory,
    /// shared with whoever else checks the file out, which is why it is never
    /// the default.
    Project(PathBuf),
}

impl Scope {
    pub fn label(&self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::Project(_) => "project",
        }
    }

    /// Parse the word a `--install-hooks` argument carries.
    pub fn parse(word: &str, cwd: &Path) -> Option<Scope> {
        match word {
            "user" | "global" => Some(Scope::User),
            "project" | "local" => Some(Scope::Project(cwd.to_path_buf())),
            _ => None,
        }
    }
}

/// This binary's absolute path, which is what an installed hook has to name.
///
/// A bare `cctop` would depend on the agent's `PATH`, which is not this shell's.
/// The absolute path is what makes the hook fire at all.
pub(super) fn own_exe() -> anyhow::Result<String> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .ok_or_else(|| anyhow::anyhow!("could not find cctop's own path"))
}

/// Ask every harness that has this scope to report, and say what happened to
/// each.
///
/// One harness cannot fail the others. A `notify` slot already holding somebody
/// else's program, a settings file that is not valid JSON, an agent that is not
/// installed at all — each of those is one line of the answer, not the end of
/// the run, because they are independent files and half an install is better
/// than none.
pub fn install(scope: &Scope) -> Vec<String> {
    let exe = match own_exe() {
        Ok(exe) => exe,
        Err(e) => return vec![e.to_string()],
    };
    HARNESSES
        .iter()
        .flat_map(|h| h.install(scope, &exe))
        .collect()
}

/// Take cctop back out of every harness, leaving every other entry untouched.
pub fn remove(scope: &Scope) -> Vec<String> {
    HARNESSES
        .iter()
        .flat_map(|h| h.remove(scope))
        .filter(|line| !line.is_empty())
        .collect()
}

/// Add cctop's hooks to one JSON settings file, leaving everything else alone.
///
/// The file is the user's, and by the time cctop sees it their other tools have
/// usually put hooks in it — so this merges into the arrays rather than writing
/// them, and never reorders or reformats what it did not add.
///
/// A file that already says exactly this is not written at all. That is what
/// makes [`repair`] safe to run at every start, from every cctop at once: the
/// file is read fresh here, after whatever health check sent the caller, so a
/// second cctop that got there first leaves this one nothing to do, and an
/// install that is already right keeps its bytes, its formatting and its mtime.
fn json_install(
    path: &Path,
    shape: Shape,
    events: &[&str],
    deciding: &[&str],
    exe: &str,
) -> anyhow::Result<()> {
    let before = read_settings(path)?;
    let mut root = before.clone();
    json_merge(&mut root, path, shape, events, deciding, exe)?;
    if root == before {
        return Ok(());
    }
    write_settings(path, &root)
}

/// What [`json_install`] does to a document, without the file around it — so
/// that [`json_health`] can ask whether an install would change anything by
/// running the very code that would make the change.
pub(super) fn json_merge(
    root: &mut serde_json::Map<String, serde_json::Value>,
    path: &Path,
    shape: Shape,
    events: &[&str],
    deciding: &[&str],
    exe: &str,
) -> anyhow::Result<()> {
    // Cursor versions its hooks file and ignores one without the field. Only
    // written when absent, so a file that already declares a newer version is
    // not quietly downgraded.
    if shape == Shape::Flat {
        root.entry("version")
            .or_insert_with(|| serde_json::json!(1));
    }

    let hooks = root
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("`hooks` in {} is not an object", path.display()))?;

    // An event an older cctop registered and this one no longer wants is
    // swept as [`json_remove`] sweeps it: left behind, it keeps firing in a
    // form nothing reads, and it is one of the differences that make an
    // install outdated.
    // An array emptied here goes with its key; one the user left empty stays.
    let mut emptied = Vec::new();
    for (event, value) in hooks.iter_mut() {
        if !events.contains(&event.as_str())
            && let Some(list) = value.as_array_mut()
            && drop_ours(list) > 0
            && list.is_empty()
        {
            emptied.push(event.clone());
        }
    }
    for event in emptied {
        // `shift_remove`, as in `json_remove`: the user's key order stays.
        hooks.shift_remove(&event);
    }

    for event in events {
        let list = hooks
            .entry(*event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("`hooks.{event}` is not an array"))?;
        // Idempotent: running the installer twice must not fire twice, and an
        // entry left by an older cctop at a path that has since moved is
        // replaced rather than added to.
        drop_ours(list);
        list.extend(our_entries_for(shape, event, deciding, exe));
    }
    Ok(())
}

/// The entries an install writes for one event, in order: the observer, and
/// beside it the deciding `yolo-hook` where this event has one.
fn our_entries_for(
    shape: Shape,
    event: &str,
    deciding: &[&str],
    exe: &str,
) -> Vec<serde_json::Value> {
    let mut commands = vec![(hook_command(exe, event), None)];
    // Its own entry, beside the observer's rather than inside it: the
    // harness runs an event's hooks in parallel, so the observer reports
    // exactly as it does without it.
    if deciding.contains(&event) {
        commands.push((yolo_hook_command(exe, event), deciding_matcher(event)));
    }
    commands
        .into_iter()
        .map(|(command, matcher)| {
            let command = serde_json::json!({
                "type": "command",
                "command": command,
            });
            match (shape, matcher) {
                (Shape::Nested, Some(matcher)) => {
                    serde_json::json!({ "matcher": matcher, "hooks": [command] })
                }
                (Shape::Nested, None) => serde_json::json!({ "hooks": [command] }),
                (Shape::Flat, _) => command,
            }
        })
        .collect()
}

/// cctop's entries in one settings document, event by event, and nothing else.
///
/// Where they sit among the user's entries is left out on purpose. An install
/// appends, so another tool that appends after it leaves cctop's entry
/// somewhere other than last, and comparing whole lists would call that
/// outdated and move it back on every start — a rewrite of the user's file
/// for a difference that changes nothing about what fires.
pub(super) fn our_entries(
    root: &serde_json::Map<String, serde_json::Value>,
) -> std::collections::BTreeMap<&str, Vec<&serde_json::Value>> {
    root.get("hooks")
        .and_then(|h| h.as_object())
        .into_iter()
        .flatten()
        .filter_map(|(event, value)| {
            let ours: Vec<_> = value.as_array()?.iter().filter(|e| is_ours(e)).collect();
            (!ours.is_empty()).then_some((event.as_str(), ours))
        })
        .collect()
}

/// Take cctop's entries out of one JSON settings file, and say how many went.
///
/// Every event is swept, not just the ones this cctop would install: an entry
/// written by an older version that has since dropped an event is still cctop's
/// to clean up, and leaving it behind would keep firing at a monitor that no
/// longer reports it installed.
fn json_remove(path: &Path) -> anyhow::Result<usize> {
    let mut root = read_settings(path)?;
    let mut removed = 0;
    if let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for (_, value) in hooks.iter_mut() {
            if let Some(list) = value.as_array_mut() {
                removed += drop_ours(list);
            }
        }
        // An event whose only entry was ours goes too, rather than leaving an
        // empty array behind in someone else's file.
        hooks.retain(|_, value| !value.as_array().is_some_and(|l| l.is_empty()));
        // And the `hooks` object itself, once cctop's were all it held.
        // `shift_remove`, not `remove`: under `preserve_order` the latter is a
        // swap, which would move the file's last key into the hole.
        if hooks.is_empty() {
            root.shift_remove("hooks");
        }
    }
    if removed > 0 {
        write_settings(path, &root)?;
    }
    Ok(removed)
}

/// Take cctop's commands out of one event's list, and say how many went.
///
/// By command, not by entry: cctop writes a wrapper holding only its own
/// command, but a user may add theirs into the same one, and dropping the
/// wrapper whole would delete a hook that was never cctop's. A wrapper left
/// with nothing in it goes, like an entry that was cctop's alone.
fn drop_ours(list: &mut Vec<serde_json::Value>) -> usize {
    let is_our_hook = |h: &serde_json::Value| {
        h.get("command")
            .and_then(|c| c.as_str())
            .is_some_and(is_our_command)
    };
    let mut removed = 0;
    for entry in list.iter_mut() {
        if let Some(inner) = entry.get_mut("hooks").and_then(|h| h.as_array_mut())
            && inner.iter().any(|h| !is_our_hook(h))
        {
            let before = inner.len();
            inner.retain(|h| !is_our_hook(h));
            removed += before - inner.len();
        }
    }
    let before = list.len();
    list.retain(|entry| !is_ours(entry));
    removed + before - list.len()
}

/// Whether a settings entry is one cctop wrote, in either shape.
fn is_ours(entry: &serde_json::Value) -> bool {
    entry_commands(entry).any(is_our_command)
}

/// Whether a command line is one the installer wrote: `<a cctop> hook <Event>`.
///
/// Matching the literal string `cctop hook` was the obvious thing and was
/// wrong: it assumes the binary is named exactly `cctop`, so anyone running
/// `cctop-0.1.12`, a renamed build, or cargo's own test binary had their
/// entries go unrecognised — which made installing twice register the hooks
/// twice, and removing leave them all behind. What is actually distinctive is
/// the shape: a program whose *file name* mentions cctop, the word `hook`, and
/// one bare event name.
pub(super) fn is_our_command(command: &str) -> bool {
    let Some((exe, event)) = split_ours(command) else {
        return false;
    };
    let event = event.trim();
    !event.is_empty()
        && !event.contains(char::is_whitespace)
        && Path::new(exe.trim())
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("cctop"))
}

/// The command lines one entry names, whichever shape it is written in.
///
/// Both are accepted everywhere rather than per harness: the two shapes are
/// told apart by what the entry holds, and an agent that grows the other one —
/// Cursor already reads Claude Code's nested files — needs no second reader.
pub(super) fn entry_commands(entry: &serde_json::Value) -> impl Iterator<Item = &str> {
    let nested = entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|inner| inner.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|h| h.get("command").and_then(|c| c.as_str()));
    let flat = entry.get("command").and_then(|c| c.as_str());
    nested.chain(flat)
}

/// The command line the installer writes for one event.
///
/// The harnesses hand this to a shell, so a path with a space in it — an
/// `/Applications/My Tools/cctop`, a home directory with a space — has to be
/// quoted, or every fire runs `/Applications/My` and exits 127. A path with
/// nothing the shell would read is written bare, so an install that already
/// works is byte-for-byte what it was.
pub(super) fn hook_command(exe: &str, event: &str) -> String {
    command_line(exe, MARKER, event)
}

/// The command line for the deciding entry: see [`yolo_hook`].
pub(super) fn yolo_hook_command(exe: &str, event: &str) -> String {
    command_line(exe, YOLO_MARKER, event)
}

fn command_line(exe: &str, marker: &str, event: &str) -> String {
    format!("{}{marker}{event}", crate::sandbox::sh_quote(exe))
}

/// A command line cut at whichever of cctop's two words it carries, into the
/// binary and the event. The two cannot be confused: `yolo-hook` has no space
/// before its `hook`.
fn split_ours(command: &str) -> Option<(&str, &str)> {
    command
        .rsplit_once(YOLO_MARKER)
        .or_else(|| command.rsplit_once(MARKER))
}

/// The cctop an installed command names, taken back out of the command text.
///
/// Everything before the last `hook` is the path, which is why this splits on
/// the marker rather than on whitespace: an install from before the path was
/// quoted wrote a spaced path bare, and it still has to be recognised to be
/// repaired. A quoted path is unquoted, the inverse of [`hook_command`].
pub(super) fn recorded_exe(command: &str) -> Option<String> {
    if !is_our_command(command) {
        return None;
    }
    let exe = split_ours(command)?.0.trim();
    Some(
        match exe.strip_prefix('\'').and_then(|e| e.strip_suffix('\'')) {
            Some(quoted) => quoted.replace(r"'\''", "'"),
            None => exe.to_string(),
        },
    )
}

pub(super) fn read_settings(
    path: &Path,
) -> anyhow::Result<serde_json::Map<String, serde_json::Value>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(e) => return Err(e.into()),
    };
    let value: serde_json::Value = serde_json::from_str(text.trim()).map_err(|e| {
        // Refusing is the only safe move: rewriting a file cctop could not parse
        // would throw away whatever the user has in it.
        anyhow::anyhow!("{} is not valid JSON ({e}); fix it first", path.display())
    })?;
    match value {
        serde_json::Value::Object(map) => Ok(map),
        _ => anyhow::bail!("{} is not a JSON object", path.display()),
    }
}

/// The file a write to `path` should land in: the link's target when `path` is
/// a symlink, `path` itself otherwise.
///
/// A config symlinked in from a dotfiles repository is common, and the
/// write-and-rename below would replace the link with a plain file — after
/// which the user's next change to the repository silently stops reaching the
/// agent. A dangling link is left to be replaced, since there is nothing behind
/// it to keep in step.
fn through_link(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(target) if path.is_symlink() => target,
        _ => path.to_path_buf(),
    }
}

fn write_settings(
    path: &Path,
    root: &serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<()> {
    // Written beside the target and renamed: a crash mid-write would otherwise
    // leave the user with no settings at all, which breaks their agent far more
    // thoroughly than a missing hook.
    let text = format!("{}\n", serde_json::to_string_pretty(root)?);
    crate::util::write_atomic(&through_link(path), text.as_bytes(), false)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Codex
// ---------------------------------------------------------------------------

/// The `notify` value cctop installs.
///
/// Codex runs this program once a turn and appends the event as one more
/// argument, so the JSON arrives in argv rather than on stdin — which is why
/// `hook` needs to be told the payload is a Codex one before it looks for it.
pub(super) fn codex_notify(exe: &str) -> Vec<String> {
    vec![exe.to_string(), "hook".into(), CODEX_SELECTOR.into()]
}

/// Point Codex's `notify` at cctop.
///
/// Codex allows exactly one `notify` program, so unlike the hook arrays this
/// cannot merge: an existing entry that is not ours is left alone and reported,
/// because replacing someone's desktop-notification script with a monitor is not
/// a trade cctop gets to make for them.
fn notify_install(path: &Path, exe: &str) -> anyhow::Result<()> {
    let mut doc = read_codex(path)?;

    if let Some(existing) = codex_notify_argv(&doc)
        && !existing.iter().any(|a| a.contains("cctop"))
    {
        anyhow::bail!(
            "{} already sets notify = {existing:?}; remove it first if you want cctop to have it",
            path.display()
        );
    }

    // Already this, exactly: nothing to write. See [`json_install`].
    if codex_notify_argv(&doc).is_some_and(|argv| argv == codex_notify(exe)) {
        return Ok(());
    }

    let mut array = toml_edit::Array::new();
    for arg in codex_notify(exe) {
        array.push(arg);
    }
    doc["notify"] = toml_edit::value(array);
    write_codex(path, &doc)
}

/// Take cctop back out of Codex's `notify`, leaving another tool's alone.
fn notify_remove(path: &Path) -> anyhow::Result<String> {
    let mut doc = read_codex(path)?;
    match codex_notify_argv(&doc) {
        Some(argv) if argv.iter().any(|a| a.contains("cctop")) => {
            doc.remove("notify");
            write_codex(path, &doc)?;
            Ok(format!("removed from notify in {}", path.display()))
        }
        Some(_) => Ok(format!(
            "notify in {} belongs to something else; left alone",
            path.display()
        )),
        None => Ok(format!("was not notifying cctop ({})", path.display())),
    }
}

/// The `notify` program Codex is configured to run, as its argument vector.
pub(super) fn codex_notify_argv(doc: &toml_edit::DocumentMut) -> Option<Vec<String>> {
    let array = doc.get("notify")?.as_array()?;
    Some(
        array
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
    )
}

pub(super) fn read_codex(path: &Path) -> anyhow::Result<toml_edit::DocumentMut> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    // Same refusal as the JSON side, and for the same reason: a config cctop
    // cannot parse is one it must not rewrite. `toml_edit` is used rather than a
    // plain deserializer so the user's comments and layout survive the edit —
    // and the refusal names the line's position without quoting it, as every
    // other reader of a file that may hold a token does.
    crate::quota::parse_toml(path, &text)
}

fn write_codex(path: &Path, doc: &toml_edit::DocumentMut) -> anyhow::Result<()> {
    // Owner-only from the moment it exists, like every other writer of a file
    // cctop rewrites: this one can hold an auth token.
    crate::util::write_atomic(&through_link(path), doc.to_string().as_bytes(), true)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// OpenCode
// ---------------------------------------------------------------------------

/// The plugin file cctop owns. Named for cctop so that a file cctop did not
/// write is never the one it deletes.
const PLUGIN_FILE: &str = "cctop.ts";

/// The events the OpenCode plugin forwards, in OpenCode's own vocabulary.
///
/// A list rather than a literal inside the plugin text so that the same names
/// can be looked for when reading a plugin file back: a plugin written by an
/// older cctop is short exactly the names added since, which is the same
/// shortfall a stale `settings.json` has and is reported the same way.
const OPENCODE_EVENTS: &[&str] = &[
    "session.idle",
    "session.created",
    "session.deleted",
    "session.compacted",
    "permission.asked",
    "tool.execute.before",
    "tool.execute.after",
];

/// The names a plugin file has to carry for the OpenCode of `api` that cctop
/// wants.
///
/// OpenCode 2 registers the two tool moments as hooks and reads the asking from
/// the API, so its half is looked for under the names that API and those hooks
/// use rather than the ones 1 announced them with — and it has to say when a
/// prompt was answered as well as when one went up, which 1's half never had to.
fn wanted(api: crate::opencode::Api) -> Vec<&'static str> {
    match api {
        crate::opencode::Api::V1 => OPENCODE_EVENTS.to_vec(),
        crate::opencode::Api::V2 => vec![
            "session.idle",
            "session.created",
            "session.deleted",
            "session.compacted",
            "permission.asked",
            "permission.replied",
            "execute.before",
            "execute.after",
        ],
    }
}

/// Which dialects a plugin file on disk carries, read back out of its exports.
///
/// They cannot be mistaken for one another, which is the point: OpenCode 2
/// refuses anything without a default export, so a version 1 file on a version 2
/// server is not loaded at all — it says so in a log line and carries on, and
/// the session looks to cctop like one that is merely quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginShape {
    /// A version 1 plugin: a named export and nothing else.
    V1,
    /// A version 2 plugin: a default export and nothing else.
    V2,
    /// What cctop writes now: both halves in one file, which is right on every
    /// version and is what makes an upgrade a non-event.
    Both,
}

fn plugin_shape(text: &str) -> PluginShape {
    match (
        text.contains("export default"),
        text.contains("export const cctop"),
    ) {
        (true, true) => PluginShape::Both,
        (true, false) => PluginShape::V2,
        (false, _) => PluginShape::V1,
    }
}

/// The events `text` does not forward to the OpenCode of `api` that cctop wants.
pub(super) fn plugin_shortfall(text: &str, api: crate::opencode::Api) -> Vec<&'static str> {
    let for_this_one = wanted(api);
    // A file carrying both halves forwards everything on every version, so it is
    // checked against the union and nothing else. One carrying a single half is
    // short of *everything* on the other version, which is not a shortfall to be
    // filled in by editing: the file is replaced whole, which is what an install
    // does anyway. It is reported so that an OpenCode upgraded under a cctop
    // installed before this one is rewritten rather than going quiet.
    let carried = match plugin_shape(text) {
        PluginShape::Both => {
            let mut both = wanted(crate::opencode::Api::V1);
            for name in wanted(crate::opencode::Api::V2) {
                if !both.contains(&name) {
                    both.push(name);
                }
            }
            both
        }
        PluginShape::V1 if api == crate::opencode::Api::V1 => for_this_one,
        PluginShape::V2 if api == crate::opencode::Api::V2 => for_this_one,
        PluginShape::V1 | PluginShape::V2 => return for_this_one,
    };
    carried
        .iter()
        .filter(|event| !text.contains(**event))
        .copied()
        .collect()
}

/// The line the plugin records this binary's path on, and how its own state is
/// read back out.
const PLUGIN_MARKER: &str = "const CCTOP = ";

/// The plugin cctop writes, as a file rather than as a string in here.
///
/// A real `.ts` file because it is a real program: an editor highlights it, a
/// formatter can be run over it, and a diff of it is a diff of JavaScript rather
/// than of Rust lines with every brace doubled to survive `format!`. Embedded
/// with `include_str!` for the reason [`cctop_serve`] embeds its pages the same
/// way — an installed cctop is one binary, and a plugin cctop cannot find is a
/// plugin that reports nothing.
///
/// The one line that is per-install is a placeholder: `PLUGIN_PATH`, replaced
/// with this binary's path at install time, and read back out of the installed
/// file by [`plugin_exe`].
const PLUGIN: &str = include_str!("../assets/opencode-plugin.ts");

/// What the placeholder is written as, quoted, so the substitution is one
/// replacement of a whole JSON string rather than a splice inside one.
const PLUGIN_PATH: &str = "\"$CCTOP\"";

/// The two exports whole, for the tests that take one of them out. Which
/// dialect a file is comes from the shorter markers in [`plugin_shape`], because
/// a file written for one version alone has a shorter line here.
#[cfg(test)]
const PLUGIN_DEFAULT_EXPORT: &str = "export default { id: \"cctop\", setup, server: cctopV1 }";
#[cfg(test)]
const PLUGIN_NAMED_EXPORT: &str = "export const cctop = cctopV1";

/// The plugin cctop writes, for every OpenCode there is.
///
/// OpenCode has no hook commands to register: extensions are code it loads at
/// startup, so the only way in is a file, and cctop writes the whole of it. That
/// makes this the one integration that runs *inside* the agent's process rather
/// than beside it, which is why every line of the plugin is wrapped: a plugin
/// that throws is a plugin that can spoil the session it is watching, and there
/// is no exit code to hide behind here.
///
/// # One file for both OpenCodes
///
/// Version 2 replaced the plugin API outright and does not load a version 1
/// plugin at all — it logs a warning nobody opens and carries on, so a session
/// on version 2 reports nothing and looks merely quiet. The obvious answer is to
/// write whichever file the installed version wants, which is what this did
/// first, and it has one bad property: it is only right until somebody upgrades.
///
/// So the file carries both dialects instead, and neither can be run on the
/// wrong OpenCode by accident:
///
/// - A named export is the version 1 plugin, and is what version 1 has always
///   loaded — including 1.14, which knows nothing about object entrypoints.
/// - The default export is `{ id, setup, server }`: version 2 calls `setup`,
///   and version 1.18.29 and newer call `server`, which is the same version 1
///   plugin again. Verified against 1.14.25, 1.18.33 and 2.0.20 rather than
///   assumed: 1.18.33 calls *both*, handing `setup` a context with no `event`,
///   `tool` or `permission` on it, which is why every use of them is optional
///   and the whole body is guarded. Version 2 calls only `setup`, so a version 2
///   session is never reported twice.
///
/// Both halves hand each moment to `cctop hook` the same way and under the same
/// names, so everything downstream of the plugin reads one vocabulary.
pub(super) fn plugin_source(exe: &str) -> String {
    PLUGIN.replace(
        PLUGIN_PATH,
        &serde_json::Value::String(exe.to_string()).to_string(),
    )
}

/// The same plugin with one entry point taken out.
///
/// Not what cctop installs — this is for the tests, which need a file that is
/// *only* version 1 or *only* version 2 to prove that one written for the other
/// is noticed rather than read as installed. Taken out of the real file rather
/// than written twice, so the two fixtures cannot drift from what ships: each
/// keeps every name cctop wants, which is exactly the state being tested — a
/// plugin that says the right things and is loaded by nothing.
#[cfg(test)]
fn plugin_source_for(exe: &str, api: crate::opencode::Api) -> String {
    let text = plugin_source(exe);
    match api {
        crate::opencode::Api::V1 => text.replace(PLUGIN_DEFAULT_EXPORT, ""),
        crate::opencode::Api::V2 => text.replace(PLUGIN_NAMED_EXPORT, ""),
    }
}

/// Write the plugin, replacing whatever cctop left there before.
fn plugin_install(path: &Path, exe: &str) -> anyhow::Result<()> {
    let source = plugin_source(exe);
    // Already this, exactly: nothing to write. See [`json_install`].
    if std::fs::read_to_string(path).is_ok_and(|text| text == source) {
        return Ok(());
    }
    crate::util::write_atomic(path, source.as_bytes(), false)?;
    Ok(())
}

/// Delete the plugin, and say whether there was one. A file at that name that
/// cctop did not write is left alone, however unlikely that is.
fn plugin_remove(path: &Path) -> anyhow::Result<bool> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
        Ok(text) if !text.contains(PLUGIN_MARKER) => Ok(false),
        Ok(_) => {
            std::fs::remove_file(path)?;
            Ok(true)
        }
    }
}

/// The cctop a plugin file names, read back out of the line that records it.
pub(super) fn plugin_exe(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.starts_with(PLUGIN_MARKER))?;
    // Written by `serde_json`, so it is read back the same way rather than by
    // trimming quotes: a path with a backslash or a quote in it survives.
    serde_json::from_str::<String>(line[PLUGIN_MARKER.len()..].trim()).ok()
}

// ---------------------------------------------------------------------------
// Claude Code's /yolo
// ---------------------------------------------------------------------------

/// What the panel and the install lines call the skill.
pub(super) const SKILL_LABEL: &str = "/yolo";

/// The line of the skill's frontmatter that marks it cctop's and records the
/// binary that installed it, as a JSON string, the way the plugin's marker does.
/// `metadata` is the map Claude Code documents as free-form and left to the
/// tooling that wrote it.
const SKILL_MARKER: &str = "  cctop: ";

/// `/yolo`, `/yolo off` and `/yolo status` in Claude Code.
///
/// The answer does not come from here. `cctop yolo-hook UserPromptExpansion`
/// ([`yolo_typed`]) blocks the command before it expands and shows the person
/// the result as the block's reason, so no model turn runs for a switch. The
/// skill is still needed, because Claude Code only expands, and so only fires
/// that hook for, a command it knows; a name with no skill or command file
/// behind it is "Unknown command".
///
/// So its body is only what the model sees when the hook did not answer —
/// hooks not reinstalled since an upgrade, or a Claude Code too old to have
/// the event — and it runs nothing: an earlier version ran `cctop yolo` from
/// a `` !`…` `` line, which Claude Code runs through the Bash tool's own code,
/// so the command could not tell the person from the model, and the turn's
/// grant for that line covered the model's own Bash calls too.
///
/// - `disable-model-invocation`, so the model cannot run `/yolo` through its
///   Skill tool, and its description stays out of the model's context.
/// - the marker, so a `/yolo` that is not cctop's is never overwritten.
///
/// A skill rather than a file in `commands/`, because the docs mirrored in
/// `docs/harnesses/claude/skills.md` say command files are the older form and
/// to prefer a skill for new work; the two behave the same.
fn skill_source(exe: &str) -> String {
    // YAML's double-quoted strings are a superset of JSON's, so serde's
    // quoting is a correct YAML quoting of any path at all.
    let quote = |text: &str| serde_json::Value::String(text.to_string()).to_string();
    format!(
        "---\n\
         description: {description}\n\
         argument-hint: \"[on|off|status]\"\n\
         disable-model-invocation: true\n\
         metadata:\n\
         {SKILL_MARKER}{exe_json}\n\
         ---\n\
         cctop answers /yolo itself, from a hook, before this text is used. If \
         you are reading this, that hook did not run, so YOLO was not changed.\n\
         \n\
         Tell the user exactly that, in one line, and that `cctop --install-hooks` \
         installs the hook (it needs Claude Code's UserPromptExpansion hook event). \
         Run nothing, and do not try to switch YOLO any other way.\n",
        description = quote(
            "Switch cctop's YOLO on for this session, so every permission prompt \
             is allowed until it ends. /yolo off stops it; /yolo status says which."
        ),
        exe_json = quote(exe),
    )
}

/// The cctop a skill file names, read back out of its marker.
fn skill_exe(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.starts_with(SKILL_MARKER))?;
    serde_json::from_str::<String>(line[SKILL_MARKER.len()..].trim()).ok()
}

/// The file in the way of cctop's `/yolo`, when there is one: a skill at its
/// path that cctop did not write, or a command file of the same name, which
/// the skill would hide. Either is the person's own `/yolo`.
fn foreign_yolo(path: &Path, commands: &Path) -> Option<PathBuf> {
    match std::fs::read_to_string(path) {
        Ok(text) if skill_exe(&text).is_none() => return Some(path.to_path_buf()),
        Ok(_) => return None,
        Err(_) => {}
    }
    commands.exists().then(|| commands.to_path_buf())
}

/// Write `/yolo`, unless the person has a `/yolo` of their own.
fn skill_install(path: &Path, commands: &Path, exe: &str) -> anyhow::Result<()> {
    if let Some(theirs) = foreign_yolo(path, commands) {
        anyhow::bail!(
            "{} is a {SKILL_LABEL} of your own, so cctop's is not installed",
            theirs.display()
        );
    }
    let before = std::fs::read_to_string(path).ok();
    let source = skill_source(exe);
    // Already this, exactly: nothing to write. See [`json_install`].
    if before.is_some_and(|text| text == source) {
        return Ok(());
    }
    crate::util::write_atomic(path, source.as_bytes(), false)?;
    Ok(())
}

/// Delete cctop's `/yolo`, and its directory once that is empty, and say
/// whether there was one. Anything else at that path is left alone.
fn skill_remove(path: &Path) -> anyhow::Result<bool> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
        Ok(text) if skill_exe(&text).is_none() => Ok(false),
        Ok(_) => {
            std::fs::remove_file(path)?;
            // Only when empty: `remove_dir` refuses anything else, which is
            // the point — a file the person put beside it stays.
            if let Some(dir) = path.parent() {
                let _ = std::fs::remove_dir(dir);
            }
            Ok(true)
        }
    }
}

/// What `/yolo` has to say.
///
/// Missing is only "not installed" when the hooks beside it are not cctop's
/// either. With them installed, it is an install from before `/yolo` existed
/// — short of something this version writes, which [`repair`] fills in the
/// way it fills in a hook event added since.
fn skill_health(path: &Path, commands: &Path, hooks: &Path) -> Health {
    if let Some(theirs) = foreign_yolo(path, commands) {
        return Health::Foreign(theirs);
    }
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match json_health(hooks, Shape::Nested, CLAUDE_EVENTS, CLAUDE_DECIDING) {
                Health::Absent | Health::Unreadable(_) => Health::Absent,
                _ => Health::Partial(vec![SKILL_LABEL]),
            }
        }
        Err(e) => Health::Unreadable(e.to_string()),
        Ok(text) => {
            let exe = skill_exe(&text);
            // An older form — the one that ran a shell line — is outdated
            // like any other difference, and the rewrite drops the line.
            let outdated = exe.as_deref().is_some_and(|exe| text != skill_source(exe));
            verdict(exe, Vec::new(), outdated)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One event Claude Code offers is never asked for, because installing it
    /// would break the session.
    ///
    /// `WorktreeCreate` replaces `git worktree` rather than observing it, and
    /// takes the new worktree's path from the hook's stdout. This hook writes
    /// nothing to stdout by construction, and a `WorktreeCreate` hook that
    /// produces no path fails worktree creation outright — so installing it
    /// would break `claude --worktree`, every `isolation: "worktree"` subagent,
    /// and every backgrounded session Claude Code isolates in one.
    ///
    /// Everything else Claude Code raises is registered, so that adding an
    /// event cctop cares about is a change to [`signal_of`] and not to
    /// everybody's settings file. This is the one that cannot join them.
    #[test]
    fn the_event_that_would_replace_git_is_never_installed() {
        assert!(
            !CLAUDE_EVENTS.contains(&"WorktreeCreate"),
            "installing WorktreeCreate makes worktree creation fail — see the \
             note above CLAUDE_EVENTS"
        );
    }

    /// The settings file belongs to the user and their other tools. Installing
    /// must not disturb a hook cctop did not write, and removing must put the
    /// file back exactly as it was found.
    /// Regression: an entry was dropped whole if any command in it was cctop's,
    /// so a user's own command added into the same wrapper went with it on the
    /// next install, repair or remove.
    #[test]
    fn a_users_command_sharing_a_wrapper_with_ours_survives() {
        let dir = scratch("hooks-shared");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let theirs = serde_json::json!({"type": "command", "command": "notify-send done"});
        let settings = serde_json::json!({
            "hooks": {"Stop": [{"hooks": [
                {"type": "command", "command": "/old/cctop hook Stop"},
                theirs,
            ]}]}
        });
        std::fs::write(&path, settings.to_string()).unwrap();

        let commands = |path: &Path| -> Vec<String> {
            let root = read_settings(path).unwrap();
            let list = root["hooks"]["Stop"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            list.iter()
                .flat_map(|e| entry_commands(e).map(str::to_string).collect::<Vec<_>>())
                .collect()
        };
        install(&scope);
        assert!(commands(&path).contains(&"notify-send done".to_string()));
        remove(&scope);
        assert_eq!(commands(&path), ["notify-send done"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_with_a_space_is_quoted_for_the_shell_and_read_back() {
        let spaced = "/home/My User/.cargo/bin/cctop";
        let command = hook_command(spaced, "Stop");
        assert_eq!(command, "'/home/My User/.cargo/bin/cctop' hook Stop");
        // What the harness does with it: hand it to a shell, which must see one
        // program, not `/home/My` with arguments.
        let words = std::process::Command::new("sh")
            .args(["-c", &format!("set -- {command}; echo \"$#|$1\"")])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&words.stdout).trim(),
            format!("3|{spaced}")
        );
        assert_eq!(recorded_exe(&command).as_deref(), Some(spaced));

        let quote = "/opt/it's/cctop";
        assert_eq!(
            recorded_exe(&hook_command(quote, "Stop")).as_deref(),
            Some(quote)
        );
        // A path the shell reads as-is stays bare, so a working install is
        // not rewritten.
        assert_eq!(
            hook_command("/usr/local/bin/cctop", "Stop"),
            "/usr/local/bin/cctop hook Stop"
        );
    }

    #[test]
    fn a_symlinked_settings_file_stays_a_symlink() {
        let dir = scratch("hooks-link");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let dotfiles = dir.join("dotfiles");
        std::fs::create_dir_all(&dotfiles).unwrap();
        let real = dotfiles.join("settings.json");
        std::fs::write(&real, r#"{"model": "opus"}"#).unwrap();
        std::os::unix::fs::symlink(&real, &path).unwrap();

        install(&scope);
        assert!(path.is_symlink(), "the link was replaced by a plain file");
        let written = read_settings(&real).unwrap();
        assert_eq!(written["model"], "opus");
        assert!(
            written.contains_key("hooks"),
            "the hooks never reached the target"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_round_trip_keeps_the_users_key_order() {
        let dir = scratch("hooks-order");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Deliberately not alphabetical, and `hooks` is not last, so a sorted
        // map and a swap-remove would each move something.
        let theirs = r#"{"zeta": 1, "hooks": {}, "model": "opus", "alpha": {"y": 1, "b": 2}}"#;
        std::fs::write(&path, theirs).unwrap();
        let keys =
            |path: &Path| -> Vec<String> { read_settings(path).unwrap().keys().cloned().collect() };

        install(&scope);
        assert_eq!(keys(&path), ["zeta", "hooks", "model", "alpha"]);
        let alpha: Vec<String> = read_settings(&path).unwrap()["alpha"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(alpha, ["y", "b"]);
        remove(&scope);
        assert_eq!(keys(&path), ["zeta", "model", "alpha"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn installing_leaves_another_tools_hooks_exactly_as_they_were() {
        let dir = scratch("hooks");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let theirs = serde_json::json!({
            "model": "opus",
            "hooks": {
                "SessionStart": [{"hooks": [{"type": "command", "command": "/theirs.mjs"}]}]
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&theirs).unwrap()).unwrap();

        install(&scope);
        let after = read_settings(&path).unwrap();
        assert_eq!(after["model"], "opus", "an unrelated setting was disturbed");
        assert_eq!(
            after["hooks"]["SessionStart"][0], theirs["hooks"]["SessionStart"][0],
            "another tool's hook was disturbed"
        );
        assert_eq!(
            after["hooks"]["SessionStart"].as_array().unwrap().len(),
            2,
            "cctop's own SessionStart hook was not added alongside it"
        );
        assert_eq!(after["hooks"]["Stop"].as_array().unwrap().len(), 1);

        // Twice is once: an installer that doubled up would fire twice a turn.
        install(&scope);
        assert_eq!(
            read_settings(&path).unwrap()["hooks"]["Stop"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);

        remove(&scope);
        assert_eq!(read_settings(&path).unwrap(), *theirs.as_object().unwrap());
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Absent);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Each harness is asked in its own dialect, and one install does the lot.
    ///
    /// The shapes are not interchangeable: Cursor ignores a hooks file with no
    /// `version` and does not read Claude Code's nested wrapper in its own file,
    /// and OpenCode has no command to register at all — it loads a plugin. An
    /// install that wrote Claude's shape everywhere would look installed in the
    /// panel and deliver nothing.
    #[test]
    fn every_harness_is_asked_in_its_own_dialect() {
        let dir = scratch("dialects");
        let scope = Scope::Project(dir.clone());
        let done = install(&scope);
        assert_eq!(
            done.len(),
            5,
            "every harness with a project scope should have been written: {done:?}"
        );

        // Claude Code, Gemini CLI and Codex: the nested wrapper, under their own
        // names. Codex borrowed Claude Code's shape outright, which is why one
        // reader serves both — but it is written to its own file, and `notify`
        // is the machine-wide half it does not have here.
        for (harness, event) in [
            (Harness::Claude, "Stop"),
            (Harness::Gemini, "AfterAgent"),
            (Harness::Codex, "PermissionRequest"),
        ] {
            let path = harness.config_file(&scope).unwrap();
            let root = read_settings(&path).unwrap();
            let command = root["hooks"][event][0]["hooks"][0]["command"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            assert!(
                is_our_command(&command),
                "{} wrote {command:?} for {event}",
                harness.label()
            );
            assert!(command.ends_with(event), "the event has to be named");
            assert_eq!(harness.health(&scope), Some(Health::Installed));
        }

        // Cursor: the command entry directly, under a versioned root.
        let path = Harness::Cursor.config_file(&scope).unwrap();
        assert!(path.ends_with(".cursor/hooks.json"));
        let root = read_settings(&path).unwrap();
        assert_eq!(root["version"], 1, "Cursor ignores an unversioned file");
        let command = root["hooks"]["stop"][0]["command"].as_str().unwrap();
        assert!(is_our_command(command), "Cursor got {command:?}");

        // OpenCode: a plugin file, naming this binary.
        let path = Harness::OpenCode.config_file(&scope).unwrap();
        assert!(path.ends_with(".opencode/plugins/cctop.ts"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(plugin_exe(&text).as_deref(), own_exe().ok().as_deref());
        assert!(
            text.contains(&format!("\"hook\", \"{OPENCODE_SELECTOR}\"")),
            "the plugin has to call the hook the way `hook` reads it"
        );
        for event in wanted(crate::opencode::api()) {
            assert!(text.contains(event), "the plugin forwards no {event}");
        }
        // A plugin from an older cctop forwards fewer events, and cctop owns the
        // whole file — so it is a shortfall to be filled in rather than
        // something to leave alone. Nothing else notices: the exe line, which is
        // all health used to read, is identical. `session.compacted` rather than
        // one of the tool moments, because which of those a file is expected to
        // register is up to the OpenCode that is installed.
        std::fs::write(&path, text.replace("session.compacted", "session.gone")).unwrap();
        assert!(
            Harness::OpenCode.health(&scope).unwrap().is_problem(),
            "a plugin missing an event read as fine"
        );
        assert!(!repair_in(std::slice::from_ref(&scope)).fixed.is_empty());
        assert_eq!(
            Harness::OpenCode.health(&scope).unwrap(),
            Health::Installed,
            "the plugin was not rewritten"
        );

        // And every one of them comes back out.
        remove(&scope);
        for harness in HARNESSES {
            assert!(
                matches!(harness.health(&scope), None | Some(Health::Absent)),
                "{} was left behind",
                harness.label()
            );
        }
        assert!(
            !Harness::OpenCode.config_file(&scope).unwrap().exists(),
            "the plugin file was left on disk"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One file, both OpenCodes.
    ///
    /// This is what makes an upgrade a non-event: the file cctop installed when
    /// the machine was on one version is still the right file on the other, so
    /// an OpenCode upgraded under a cctop that never runs again keeps reporting
    /// instead of going quiet until somebody reinstalls the hooks.
    ///
    /// The shape is the one OpenCode's own migration guide documents — a named
    /// export for version 1, a default export carrying `setup` for version 2 and
    /// `server` for version 1.18.29 and newer — and it was checked against real
    /// binaries rather than assumed: 1.14.25 loads the named export, 1.18.33 calls
    /// `server` *and* `setup` (with a context carrying none of version 2's
    /// domains, which is why the body is guarded), and 2.0.20 calls `setup` alone,
    /// so a version 2 session is never reported twice.
    #[test]
    fn one_plugin_file_carries_both_open_codes() {
        let text = plugin_source("/usr/bin/cctop");
        // Both entrypoints, and the V1 function reachable from both.
        assert!(
            text.contains("export const cctop = cctopV1"),
            "1.x loads a named export"
        );
        assert!(
            text.contains("export default { id: \"cctop\", setup, server: cctopV1 }"),
            "2 loads setup, 1.18.29+ loads server"
        );
        assert!(
            text.contains("export default"),
            "a V2 plugin default-exports"
        );
        assert!(text.contains("id: \"cctop\""), "and names itself");
        // Each half reads its own version's payload key, and neither reads the
        // other's: the wrong key is how a port reports nothing at all.
        assert!(
            text.contains("event.properties"),
            "the V1 half reads `properties`"
        );
        assert!(text.contains("event?.data"), "the V2 half reads `data`");
        // One spawn, shared, so there is one thing to be wrong about.
        assert_eq!(
            text.matches("node:child_process").count(),
            1,
            "the two halves share one way of handing an event over"
        );
        assert!(text.contains("const CCTOP = \"/usr/bin/cctop\""));
        assert!(text.contains(&format!("\"hook\", \"{OPENCODE_SELECTOR}\"")));
    }

    /// The file OpenCode 2 loads is a different dialect from the one OpenCode 1
    /// loads, and only one of the two names every event cctop wants: the tool
    /// moments are hooks there, so they are registered as `execute.before` and
    /// reported as the names the rest of cctop reads.
    #[test]
    fn the_plugin_for_opencode_two_is_written_in_the_api_it_loads() {
        let text = plugin_source("/usr/bin/cctop");
        assert!(
            text.contains(PLUGIN_DEFAULT_EXPORT),
            "a V2 plugin default-exports"
        );
        assert!(text.contains("id: \"cctop\""), "and names itself");
        assert!(text.contains("ctx.event?.subscribe"), "it subscribes");
        assert!(
            text.contains("event?.data"),
            "V2 payloads are under `data`, where 1 had `properties`"
        );
        assert!(text.contains("ctx.tool.hook(\"execute.before\""));
        assert!(text.contains("ctx.tool.hook(\"execute.after\""));
        for event in wanted(crate::opencode::Api::V2) {
            assert!(text.contains(event), "the plugin registers no {event}");
        }
        // The stream carries four of them; the asking and the tool moments come
        // from the API and the hooks, which is why they are not in this set.
        let streamed = text
            .split("const STREAMED = new Set([")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .unwrap_or_default();
        assert_eq!(
            streamed,
            r#""session.idle","session.created","session.deleted","session.compacted""#
        );
    }

    /// The one line that is per-install, and the one way the file can come out
    /// wrong in a way nothing else would notice: a plugin that loaded, said
    /// nothing, and looked installed — because it still had the placeholder
    /// where the binary's path should be.
    #[test]
    fn the_binarys_path_is_substituted_and_leaves_nothing_behind() {
        assert_eq!(
            PLUGIN.matches(PLUGIN_PATH).count(),
            1,
            "one placeholder, and only one, or the substitution is a guess"
        );
        for exe in ["/usr/local/bin/cctop", "/home/some body/bin/cctop"] {
            let text = plugin_source(exe);
            assert!(!text.contains(PLUGIN_PATH), "{exe} left the placeholder in");
            assert!(
                !text.contains("$CCTOP"),
                "a path that is not quoted JSON cannot be read back"
            );
            // A path with a quote or a backslash in it has to survive, because
            // health reads this line back and a splice inside a quoted string
            // would not.
            assert_eq!(plugin_exe(&text).as_deref(), Some(exe));
        }
    }

    /// The version 1 half, on its own, is the plugin OpenCode 1 has always
    /// loaded: a named export of a function returning its hooks, reading the
    /// payload key 1 puts it under.
    #[test]
    fn the_version_one_half_is_the_plugin_opencode_one_has_always_loaded() {
        let text = plugin_source_for("/usr/bin/cctop", crate::opencode::Api::V1);
        assert!(text.contains("export const cctop = cctopV1"));
        assert!(text.contains("event.properties"));
        assert!(!text.contains("export default"));
        for event in OPENCODE_EVENTS {
            assert!(text.contains(event), "the plugin forwards no {event}");
        }
    }

    /// The failure a single-dialect file has when it meets the other OpenCode:
    /// it names the right binary and every event cctop wants, and forwards
    /// nothing, because the server does not load it. Nothing else can see that,
    /// so health has to — and the file cctop writes now cannot be in that state.
    #[test]
    fn a_single_dialect_file_is_short_of_everything_on_the_other_version() {
        let v1 = plugin_source_for("/usr/bin/cctop", crate::opencode::Api::V1);
        let v2 = plugin_source_for("/usr/bin/cctop", crate::opencode::Api::V2);
        let both = plugin_source("/usr/bin/cctop");
        assert_eq!(plugin_shape(&v1), PluginShape::V1);
        assert_eq!(plugin_shape(&v2), PluginShape::V2);
        assert_eq!(plugin_shape(&both), PluginShape::Both);
        // Each single-dialect file is right for its own OpenCode...
        assert!(plugin_shortfall(&v1, crate::opencode::Api::V1).is_empty());
        assert!(plugin_shortfall(&v2, crate::opencode::Api::V2).is_empty());
        // ...and forwards nothing on the other, which is what gets it rewritten.
        assert_eq!(
            plugin_shortfall(&v1, crate::opencode::Api::V2),
            wanted(crate::opencode::Api::V2)
        );
        assert_eq!(
            plugin_shortfall(&v2, crate::opencode::Api::V1),
            wanted(crate::opencode::Api::V1)
        );
        // The file cctop writes is right on both, and carries every name.
        for api in [crate::opencode::Api::V1, crate::opencode::Api::V2] {
            assert!(
                plugin_shortfall(&both, api).is_empty(),
                "the file cctop writes is short on {api:?}"
            );
        }
        for api in [crate::opencode::Api::V1, crate::opencode::Api::V2] {
            for event in wanted(api) {
                assert!(both.contains(event), "the plugin carries no {event}");
            }
        }
    }

    /// The plugin for OpenCode 2 is written in the API that OpenCode 2 has.
    ///
    /// This is the part a translation cannot do. OpenCode 2 raises no event
    /// when it stops to ask — not for a permission, not for a question, not
    /// when either is answered — so a file that only listened to the stream
    /// would report a blocked session as a working one, which is the state a
    /// held prompt has always been hardest to see.
    #[test]
    fn the_plugin_for_opencode_two_asks_the_api_what_is_waiting() {
        let text = plugin_source("/usr/bin/cctop");
        assert!(
            text.contains("ctx.permission.list"),
            "the asking is a row in an API, not an event"
        );
        assert!(text.contains("report(\"permission.asked\""));
        assert!(
            text.contains("report(\"permission.replied\""),
            "nothing in OpenCode 2 says a prompt was answered"
        );
        // And the question is the same fact as the permission: 2 counts an
        // agent's question as a request whose action is `question`.
        assert!(text.contains("askOf"));
    }

    /// A prompt is said once, an answer once, and a session that has gone quiet
    /// stops being asked about — unless something is still pending, which is
    /// the whole reason to keep asking.
    #[test]
    fn the_poll_says_each_moment_once_and_forgets() {
        let text = plugin_source_for("/usr/bin/cctop", crate::opencode::Api::V2);
        assert!(text.contains("const POLL_MS = 1000"));
        assert!(text.contains("const FORGET_MS = 120000"));
        assert!(
            text.contains("if (now - at > FORGET_MS && !pending0?.size)"),
            "a prompt left up is still up, and cctop's claim has to keep being true"
        );
        // Told what it has already said, so a request that stays pending is not
        // re-announced once a second.
        assert!(text.contains("const told = waiting.get(sessionID)"));
    }

    /// Recognising our own entries has to survive the binary being called
    /// something other than exactly `cctop`.
    ///
    /// Regression. Matching the literal `cctop hook` looked equivalent and was
    /// not: a versioned or renamed binary produced entries the installer could
    /// no longer see, so installing twice registered every hook twice and
    /// removing left the lot behind. Cargo's own test binary is named that way,
    /// which is how this surfaced.
    #[test]
    fn an_entry_is_recognised_however_the_binary_is_named() {
        for command in [
            "/usr/local/bin/cctop hook Stop",
            "/home/me/.cargo/bin/cctop-0.1.12 hook PreToolUse",
            "/target/debug/deps/cctop-9f2c1a hook SessionEnd",
            // A path with spaces in it: the path ends at the last `hook`, not
            // at the first space.
            "/Applications/My Tools/cctop hook Stop",
        ] {
            assert!(is_our_command(command), "{command} was not recognised");
        }
        assert_eq!(
            recorded_exe("/Applications/My Tools/cctop hook Stop").as_deref(),
            Some("/Applications/My Tools/cctop")
        );

        for command in [
            // Somebody else's program, whatever it is doing.
            "/usr/bin/something-else Stop",
            "/opt/theirs/notify hook Stop",
            // A `hook` that is not the installer's: no event, or a whole
            // command line after it.
            "/usr/local/bin/cctop hook ",
            "/usr/local/bin/cctop hook Stop && rm -rf /",
            // The word in a directory name rather than the program's.
            "/home/me/cctop/scripts/theirs.sh hook Stop",
        ] {
            assert!(!is_our_command(command), "{command} was claimed as ours");
            assert_eq!(recorded_exe(command), None);
        }
    }

    /// A settings file cctop cannot parse must be refused, not rewritten — the
    /// alternative is destroying whatever the user actually had in it.
    #[test]
    fn an_unparsable_settings_file_is_refused_rather_than_replaced() {
        let dir = scratch("badjson");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{ this is not json").unwrap();
        assert!(read_settings(&path).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ this is not json"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Codex's config is hand-written and full of comments, so the edit has to
    /// leave everything it did not touch byte for byte.
    #[test]
    fn editing_codexs_config_keeps_the_comments_around_it() {
        let dir = scratch("codex");
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "# my settings\nmodel = \"gpt-5.6-terra\"\n\n[tui]\n# keep this\nnotifications = true\n",
        )
        .unwrap();

        let mut doc = read_codex(&path).unwrap();
        let mut array = toml_edit::Array::new();
        for arg in codex_notify("/usr/local/bin/cctop") {
            array.push(arg);
        }
        doc["notify"] = toml_edit::value(array);
        write_codex(&path, &doc).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my settings"), "a comment was lost");
        assert!(text.contains("# keep this"), "a comment was lost");
        assert!(text.contains(r#"notify = ["/usr/local/bin/cctop", "hook", "codex"]"#));

        // And the argv Codex will run is the one `hook` knows how to read.
        let argv = codex_notify_argv(&read_codex(&path).unwrap()).unwrap();
        assert_eq!(argv[1], "hook");
        assert_eq!(argv[2], CODEX_SELECTOR);

        doc = read_codex(&path).unwrap();
        doc.remove("notify");
        write_codex(&path, &doc).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("notify ="));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Claude Code gets the deciding entry beside the observer, for
    /// `PermissionRequest` and — matched to `/yolo` alone —
    /// `UserPromptExpansion`; health notices either missing, repair puts it
    /// back, and uninstall takes both.
    #[test]
    fn the_yolo_hook_is_installed_beside_the_observer_and_removed_with_it() {
        let dir = scratch("yolo-install");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        install(&scope);
        let exe = own_exe().unwrap();
        let commands = |event: &str| -> Vec<String> {
            read_settings(&path).unwrap()["hooks"][event]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|e| entry_commands(e).map(str::to_string).collect::<Vec<_>>())
                .collect()
        };
        assert_eq!(
            commands("PermissionRequest"),
            [
                hook_command(&exe, "PermissionRequest"),
                yolo_hook_command(&exe, "PermissionRequest")
            ]
        );
        assert_eq!(
            commands("UserPromptExpansion"),
            [
                hook_command(&exe, "UserPromptExpansion"),
                yolo_hook_command(&exe, "UserPromptExpansion")
            ]
        );
        // The observer hears every command; the deciding entry is spawned
        // for `/yolo` alone.
        let matchers: Vec<Option<String>> =
            read_settings(&path).unwrap()["hooks"]["UserPromptExpansion"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    e.get("matcher")
                        .and_then(|m| m.as_str())
                        .map(str::to_string)
                })
                .collect();
        assert_eq!(matchers, [None, Some("yolo".to_string())]);
        for event in CLAUDE_EVENTS
            .iter()
            .filter(|e| !CLAUDE_DECIDING.contains(e))
        {
            assert_eq!(commands(event), [hook_command(&exe, event)], "{event}");
        }
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);
        // Nobody else decides: no other harness was given one.
        for harness in [Harness::Gemini, Harness::Cursor, Harness::Codex] {
            if let Some(file) = harness.config_file(&scope) {
                let text = std::fs::read_to_string(&file).unwrap_or_default();
                assert!(!text.contains(YOLO_MARKER.trim()), "{}", harness.label());
            }
        }

        // An install from before the hook: health says what is short, and
        // repair fills it in.
        let mut root = read_settings(&path).unwrap();
        root["hooks"]["PermissionRequest"]
            .as_array_mut()
            .unwrap()
            .retain(|e| !entry_commands(e).any(|c| c.contains(YOLO_MARKER)));
        write_settings(&path, &root).unwrap();
        assert_eq!(
            Harness::Claude.health(&scope).unwrap(),
            Health::Partial(vec![yolo_hook_label("PermissionRequest")])
        );
        repair_in(std::slice::from_ref(&scope));
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);
        assert_eq!(commands("PermissionRequest").len(), 2, "repair doubled up");

        // The same for an install from before `/yolo` was answered by a hook:
        // the expansion entry missing is a shortfall repair fills in, and the
        // entry without its matcher is an older form it rewrites.
        let mut root = read_settings(&path).unwrap();
        root["hooks"]["UserPromptExpansion"]
            .as_array_mut()
            .unwrap()
            .retain(|e| !entry_commands(e).any(|c| c.contains(YOLO_MARKER)));
        write_settings(&path, &root).unwrap();
        assert_eq!(
            Harness::Claude.health(&scope).unwrap(),
            Health::Partial(vec![yolo_hook_label("UserPromptExpansion")])
        );
        repair_in(std::slice::from_ref(&scope));
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);
        let mut root = read_settings(&path).unwrap();
        for entry in root["hooks"]["UserPromptExpansion"].as_array_mut().unwrap() {
            entry.as_object_mut().unwrap().shift_remove("matcher");
        }
        write_settings(&path, &root).unwrap();
        assert!(matches!(
            Harness::Claude.health(&scope).unwrap(),
            Health::Outdated { .. }
        ));
        repair_in(std::slice::from_ref(&scope));
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);
        assert_eq!(
            commands("UserPromptExpansion").len(),
            2,
            "repair doubled up"
        );

        remove(&scope);
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Absent);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("hook"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The deciding command is recognised as cctop's, under any binary name,
    /// and the path read back out of it.
    #[test]
    fn a_yolo_hook_command_is_recognised_as_ours() {
        for exe in ["/usr/local/bin/cctop", "/Applications/My Tools/cctop-0.29"] {
            let command = yolo_hook_command(exe, "PermissionRequest");
            assert!(is_our_command(&command), "{command}");
            assert_eq!(recorded_exe(&command).as_deref(), Some(exe));
        }
        assert!(!is_our_command(
            "/usr/bin/other yolo-hook PermissionRequest"
        ));
    }

    /// The three paths a `/yolo` test touches, in a scratch directory of its
    /// own: never the machine's own `~/.claude`.
    fn yolo_paths(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = scratch(&format!("yolo-skill-{name}"));
        (
            dir.join("skills").join("yolo").join("SKILL.md"),
            dir.join("commands").join("yolo.md"),
            dir.join("settings.json"),
        )
    }

    /// `/yolo` is the person's alone and runs nothing: no `!` line, no grant,
    /// and the model cannot invoke it. The answer comes from the
    /// `UserPromptExpansion` hook; the body is only the fallback when that
    /// did not run.
    #[test]
    fn the_yolo_skill_runs_nothing() {
        let text = skill_source("/usr/bin/cctop");
        assert!(text.starts_with("---\n"));
        assert!(text.contains("\ndisable-model-invocation: true\n"));
        assert!(!text.contains("!`"), "{text}");
        assert!(!text.contains("allowed-tools"), "{text}");
        assert!(!text.contains("yolo on"), "{text}");
        assert!(text.contains("--install-hooks"));
        assert_eq!(skill_exe(&text).as_deref(), Some("/usr/bin/cctop"));
    }

    /// A path YAML would misread is quoted and read back intact.
    #[test]
    fn a_spaced_path_survives_the_yaml() {
        let exe = "/opt/my tools/it's \"q\"/cctop";
        assert_eq!(skill_exe(&skill_source(exe)).as_deref(), Some(exe));
    }

    /// Written again only when it would change.
    #[test]
    fn installing_yolo_twice_keeps_its_bytes() {
        let (path, commands, _) = yolo_paths("twice");
        skill_install(&path, &commands, "/usr/bin/cctop").unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        assert_eq!(first, skill_source("/usr/bin/cctop"));
        skill_install(&path, &commands, "/usr/bin/cctop").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        skill_install(&path, &commands, "/opt/cctop").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            skill_source("/opt/cctop")
        );
    }

    /// A `/yolo` of the person's own — a skill at the same path, or a command
    /// file the skill would hide — is reported and never written over.
    #[test]
    fn a_yolo_of_the_persons_own_is_left_alone() {
        let (path, commands, hooks) = yolo_paths("theirs");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "---\ndescription: mine\n---\nhello\n").unwrap();
        assert!(skill_install(&path, &commands, "/usr/bin/cctop").is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "---\ndescription: mine\n---\nhello\n"
        );
        assert_eq!(
            skill_health(&path, &commands, &hooks),
            Health::Foreign(path.clone())
        );
        assert!(!skill_health(&path, &commands, &hooks).is_problem());
        assert!(!skill_remove(&path).unwrap());
        assert!(path.exists());

        let (path, commands, hooks) = yolo_paths("theirs-command");
        std::fs::create_dir_all(commands.parent().unwrap()).unwrap();
        std::fs::write(&commands, "my own /yolo\n").unwrap();
        assert!(skill_install(&path, &commands, "/usr/bin/cctop").is_err());
        assert!(!path.exists());
        assert_eq!(
            skill_health(&path, &commands, &hooks),
            Health::Foreign(commands.clone())
        );
    }

    /// Missing is "not installed" on a machine without cctop's hooks, and a
    /// shortfall to fill in on one with them — an install from before /yolo.
    #[test]
    fn a_missing_yolo_is_short_only_beside_cctops_hooks() {
        let (path, commands, hooks) = yolo_paths("missing");
        assert_eq!(skill_health(&path, &commands, &hooks), Health::Absent);
        let exe = own_exe().unwrap();
        json_install(&hooks, Shape::Nested, CLAUDE_EVENTS, CLAUDE_DECIDING, &exe).unwrap();
        let health = skill_health(&path, &commands, &hooks);
        assert_eq!(health, Health::Partial(vec![SKILL_LABEL]));
        assert_eq!(health.shortfall(), Some((1, None)));

        skill_install(&path, &commands, &exe).unwrap();
        assert_eq!(skill_health(&path, &commands, &hooks), Health::Installed);
    }

    /// Any difference from what this version writes is an older cctop's —
    /// above all the one that ran `cctop yolo` from a `!` line with a grant,
    /// which repair rewrites without it.
    #[test]
    fn an_older_yolo_reads_as_outdated() {
        let (path, commands, hooks) = yolo_paths("older");
        let exe = own_exe().unwrap();
        skill_install(&path, &commands, &exe).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        // The form #214 wrote: a key, a grant and a shell line.
        let older = format!(
            "---\ndescription: \"x\"\ndisable-model-invocation: true\n\
             allowed-tools: \"Bash({exe} yolo --slash=k *)\"\nmetadata:\n\
             {SKILL_MARKER}{}\n  cctop-key: \"k\"\n---\n!`{exe} yolo --slash=k $ARGUMENTS`\n",
            serde_json::Value::String(exe.clone())
        );
        std::fs::write(&path, &older).unwrap();
        assert_eq!(
            skill_health(&path, &commands, &hooks),
            Health::Outdated { exe: None }
        );
        skill_install(&path, &commands, &exe).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        std::fs::write(&path, text.replace("Run nothing", "Run it")).unwrap();
        assert_eq!(
            skill_health(&path, &commands, &hooks),
            Health::Outdated { exe: None }
        );
    }

    /// Uninstall takes cctop's file and its emptied directory, and nothing
    /// the person put beside it.
    #[test]
    fn removing_yolo_takes_only_cctops_file() {
        let (path, commands, _) = yolo_paths("remove");
        skill_install(&path, &commands, "/usr/bin/cctop").unwrap();
        assert!(skill_remove(&path).unwrap());
        assert!(!path.exists());
        assert!(!path.parent().unwrap().exists());
        assert!(!skill_remove(&path).unwrap());

        skill_install(&path, &commands, "/usr/bin/cctop").unwrap();
        let note = path.parent().unwrap().join("notes.md");
        std::fs::write(&note, "mine").unwrap();
        assert!(skill_remove(&path).unwrap());
        assert!(note.exists());
    }
}
