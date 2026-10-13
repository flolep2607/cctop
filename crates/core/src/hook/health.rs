//! Whether what the installers wrote is still there and still right, the
//! report the UI draws from that, and the repair run at every start.

use super::*;

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// What one JSON settings file has to say about cctop.
pub(super) fn json_health(
    path: &Path,
    shape: Shape,
    events: &[&'static str],
    deciding: &[&'static str],
) -> Health {
    let root = match read_settings(path) {
        Err(e) => return Health::Unreadable(e.to_string()),
        Ok(root) => root,
    };
    // Cursor ignores a hooks file with no `version`, so one without it is not
    // installed however many entries it holds.
    if shape == Shape::Flat && root.get("version").is_none() {
        return Health::Absent;
    }
    let hooks = root.get("hooks").and_then(|h| h.as_object());
    let mut missing = Vec::new();
    let mut recorded: Option<String> = None;
    for event in events {
        let ours: Vec<&str> = hooks
            .and_then(|h| h.get(*event))
            .and_then(|v| v.as_array())
            .map(|list| list.as_slice())
            .unwrap_or_default()
            .iter()
            .flat_map(entry_commands)
            .filter(|c| is_our_command(c))
            .collect();
        if ours.is_empty() {
            missing.push(*event);
            continue;
        }
        let exe = ours.iter().find_map(|c| recorded_exe(c));
        // A command the installer would not write today — a spaced path left
        // bare by an older cctop — is counted as missing, so that `repair`
        // rewrites it instead of calling it installed while every fire fails.
        let has = |wanted: &dyn Fn(&str) -> String| {
            ours.iter()
                .any(|c| exe.as_deref().is_some_and(|e| *c == wanted(e)))
        };
        if !has(&|e| hook_command(e, event)) {
            missing.push(*event);
        }
        if deciding.contains(event) && !has(&|e| yolo_hook_command(e, event)) {
            missing.push(yolo_hook_label(event));
        }
        recorded = recorded.or(exe);
    }
    // Every event there and well formed, and still not necessarily what this
    // version writes: a timeout, a matcher, an event since dropped, a field an
    // older cctop added. Asked of the installer itself, at the binary the file
    // already names, so "outdated" can never drift from what an install does.
    let outdated = missing.is_empty()
        && recorded.as_deref().is_some_and(|exe| {
            let mut want = root.clone();
            json_merge(&mut want, path, shape, events, deciding, exe).is_ok()
                && our_entries(&want) != our_entries(&root)
        });
    verdict(recorded, missing, outdated)
}

/// What Codex's config has to say.
pub(super) fn notify_health(path: &Path) -> Health {
    match read_codex(path) {
        Err(e) => Health::Unreadable(e.to_string()),
        Ok(doc) => match codex_notify_argv(&doc).as_deref() {
            None | Some([]) => Health::Absent,
            // Somebody else's notify program, which cctop reports and leaves.
            Some([exe, ..]) if !exe.contains("cctop") => Health::Other {
                exe: exe.clone(),
                missing: Vec::new(),
            },
            Some(argv @ [exe, ..]) => verdict(
                Some(exe.clone()),
                Vec::new(),
                *argv != codex_notify(exe)[..],
            ),
        },
    }
}

/// What the OpenCode plugin file has to say.
pub(super) fn plugin_health(path: &Path) -> Health {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Health::Absent,
        Err(e) => Health::Unreadable(e.to_string()),
        Ok(text) => match plugin_exe(&text) {
            None => Health::Unreadable(format!("{} is not a cctop plugin", path.display())),
            // A plugin from an older cctop forwards fewer events, and the file
            // says which: the names are in it verbatim. Reported as a shortfall
            // so it is filled in the same way a stale settings file is.
            //
            // A plugin written for the *other* OpenCode is short all of them,
            // and that is not a shortfall to be filled in by editing: the file
            // is replaced whole, which is what an install does anyway. The
            // difference is that it has to be reported at all. A V1 plugin on a
            // V2 server names this binary and every event cctop wants, and
            // forwards nothing, because OpenCode 2 does not load it — it says so
            // in a log line and carries on, and the session looks to cctop like
            // one that is merely quiet.
            //
            // A plugin carrying everything is still outdated when it is not
            // the text this version writes: the file is cctop's outright, so
            // any difference at all is an older cctop's.
            Some(exe) => {
                let outdated = text != plugin_source(&exe);
                verdict(
                    Some(exe),
                    plugin_shortfall(&text, crate::opencode::api()),
                    outdated,
                )
            }
        },
    }
}

/// Turn "cctop is recorded here, at this path, missing these events, in an
/// older form or not" into the one verdict every harness is reported with.
///
/// A shortfall outranks the form: filling it in is the same whole rewrite, so
/// the report names the more specific of the two.
pub(super) fn verdict(
    recorded: Option<String>,
    missing: Vec<&'static str>,
    outdated: bool,
) -> Health {
    let own = own_exe().ok();
    match recorded {
        None => Health::Absent,
        Some(exe) if !Path::new(&exe).exists() => Health::Broken(exe),
        Some(exe) if Some(exe.as_str()) != own.as_deref() => match missing.is_empty() && outdated {
            true => Health::Outdated { exe: Some(exe) },
            false => Health::Other { exe, missing },
        },
        Some(_) if !missing.is_empty() => Health::Partial(missing),
        Some(_) if outdated => Health::Outdated { exe: None },
        Some(_) => Health::Installed,
    }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// How an installed hook is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    /// Nothing of cctop's is in this file.
    Absent,
    /// Every wanted event is registered, at this binary.
    Installed,
    /// Registered, but not for everything cctop now wants — an install from an
    /// older cctop that knew fewer events.
    Partial(Vec<&'static str>),
    /// Registered at a path that is not this binary but does exist. Two cctops
    /// on one machine is a choice, not a fault, so the path is reported and
    /// left — but the events are still counted, because "some other cctop
    /// installed this" is no reason for the file to be missing half of them.
    /// That combination used to report only the path, which hid a stale install
    /// behind a line that read as fine.
    Other {
        exe: String,
        missing: Vec<&'static str>,
    },
    /// Every event registered, but not in the form this version writes: a
    /// command line, an argument, a timeout, a matcher or any other field an
    /// older cctop wrote differently. `exe` is the other cctop the entries
    /// name, and `None` when they name this one.
    ///
    /// Its own state rather than a kind of [`Health::Partial`], because nothing
    /// is missing: every event fires. What it fires may be the old way of
    /// invoking cctop, which after an update is exactly what nobody would
    /// notice until something read wrong.
    Outdated { exe: Option<String> },
    /// Registered at a path that is gone. The hooks are firing nothing at all,
    /// and this is the one case worth fixing without being asked.
    Broken(String),
    /// The file could not be read, so nothing can be said about it.
    Unreadable(String),
    /// Something of the person's own is where cctop would write — their own
    /// `/yolo` — so cctop's is not installed, by choice rather than by fault.
    Foreign(PathBuf),
}

impl Health {
    /// Whether this is something the user would want to see rather than a
    /// working install or an absence they chose.
    pub fn is_problem(&self) -> bool {
        match self {
            Health::Partial(_)
            | Health::Outdated { .. }
            | Health::Broken(_)
            | Health::Unreadable(_) => true,
            Health::Other { missing, .. } => !missing.is_empty(),
            Health::Absent | Health::Installed | Health::Foreign(_) => false,
        }
    }

    /// The events this file should register and does not, and the binary they
    /// belong at — this one, unless another cctop already owns the entries.
    pub(super) fn shortfall(&self) -> Option<(usize, Option<&str>)> {
        match self {
            Health::Partial(missing) => Some((missing.len(), None)),
            Health::Other { exe, missing } if !missing.is_empty() => {
                Some((missing.len(), Some(exe.as_str())))
            }
            _ => None,
        }
    }
}

/// What one of a harness's config files, in one scope, has to say.
#[derive(Debug, Clone)]
pub struct ScopeStatus {
    pub harness: Harness,
    pub scope: Scope,
    pub path: PathBuf,
    pub health: Health,
    /// What a reader needs told beyond the state — Codex's hooks needing to be
    /// trusted before they fire, and nothing else so far.
    pub note: Option<&'static str>,
}

/// The whole integration, in one value the CLI and the UI both render.
#[derive(Debug, Clone)]
pub struct Report {
    /// Every harness in every scope that was looked at, in install order.
    pub entries: Vec<ScopeStatus>,
    /// Whether this cctop is receiving events, and how many others also are.
    pub listening: bool,
    pub peers: usize,
}

/// Inspect every file one harness writes in one scope.
///
/// One entry per file, which for Codex is two: hooks and `notify` are installed
/// and go wrong independently, and one line covering both would have to lie
/// about one of them.
pub fn harness_status(harness: Harness, scope: Scope) -> Vec<ScopeStatus> {
    harness
        .configs(&scope)
        .iter()
        .map(|config| ScopeStatus {
            harness,
            scope: scope.clone(),
            path: config.path().to_path_buf(),
            health: config.health(),
            note: config.note(),
        })
        .collect()
}

/// Whether Codex's hooks are written anywhere that would apply.
///
/// Written, not working: Codex will not run a hook until a person has reviewed
/// and trusted it, and it records that trust against a hash of the hook in a
/// place it does not document — so nothing on disk here can say whether it
/// happened. The caller pairs this with whether Codex has actually reported, and
/// the two together are what distinguish "not set up" from "set up and waiting
/// on you".
pub fn codex_hooks_installed(cwd: Option<&Path>) -> bool {
    let mut scopes = vec![Scope::User];
    if let Some(dir) = cwd {
        scopes.push(Scope::Project(dir.to_path_buf()));
    }
    scopes.iter().any(|scope| {
        Harness::Codex.configs(scope).iter().any(|config| {
            matches!(config, Config::Json { .. }) && config.health() != Health::Absent
        })
    })
}

/// Inspect the whole integration. `cwd` decides which project scope is looked
/// at; `listener` is this instance's, when it has one.
pub fn status(cwd: Option<&Path>, listener: Option<&Listener>) -> Report {
    let mut scopes = vec![Scope::User];
    if let Some(dir) = cwd {
        scopes.push(Scope::Project(dir.to_path_buf()));
    }
    // Harness first, then scope: the answer to "is Claude Code reporting" is
    // both of its lines together, and reading them apart is how a project
    // install gets mistaken for the user one.
    let entries = HARNESSES
        .iter()
        .flat_map(|harness| {
            scopes
                .iter()
                .flat_map(|scope| harness_status(*harness, scope.clone()))
        })
        .collect();

    Report {
        entries,
        listening: listener.is_some(),
        peers: listener.map(Listener::peer_count).unwrap_or(0),
    }
}

impl Report {
    /// The report as lines to print or draw, each tagged with whether it is
    /// something to worry about.
    ///
    /// One line per harness, and a second only where a scope has something to
    /// say. Five agents times two scopes is a wall of "not installed" that
    /// buries the line that matters, and the panel this is drawn in is exactly
    /// as tall as the lines it is handed.
    pub fn lines(&self) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        for harness in HARNESSES {
            let mine: Vec<&ScopeStatus> = self
                .entries
                .iter()
                .filter(|entry| entry.harness == harness)
                .collect();
            if mine.iter().all(|entry| entry.health == Health::Absent) {
                if let Some(entry) = mine.first() {
                    out.push((
                        format!(
                            "{}: not installed ({})",
                            harness.label(),
                            entry.path.display()
                        ),
                        false,
                    ));
                }
                continue;
            }
            // Only the scopes with something to say. A user install and no
            // project one is the ordinary shape, and printing the absence of the
            // second doubles the panel to say nothing.
            for entry in mine.iter().filter(|entry| entry.health != Health::Absent) {
                let (text, bad) = describe(&entry.health);
                // The note rides on the end rather than replacing the state:
                // Codex's hooks really are installed once they are written, and
                // still deliver nothing until a person has trusted them.
                let note = match entry.note {
                    Some(note) if entry.health == Health::Installed => format!(" — {note}"),
                    _ => String::new(),
                };
                out.push((
                    format!(
                        "{} ({}) {}: {text}{note}",
                        harness.label(),
                        entry.scope.label(),
                        entry.path.display()
                    ),
                    bad,
                ));
            }
        }
        out.push(match (self.listening, self.peers) {
            (false, _) => ("Listener: not running".into(), true),
            (true, 0) => ("Listener: receiving".into(), false),
            (true, n) => (
                format!("Listener: receiving, alongside {n} other cctop(s)"),
                false,
            ),
        });
        out
    }
}

fn describe(health: &Health) -> (String, bool) {
    match health {
        Health::Absent => ("not installed".into(), false),
        Health::Installed => ("installed".into(), false),
        // The skill on its own: the file is the whole of what is missing.
        Health::Partial(missing) if missing[..] == [SKILL_LABEL] => (
            format!("{SKILL_LABEL} missing; cctop writes it at its next start"),
            true,
        ),
        Health::Partial(missing) => (
            format!("installed, but missing {}", missing.join(", ")),
            true,
        ),
        Health::Other { exe, missing } if missing.is_empty() => {
            (format!("installed, pointing at {exe}"), false)
        }
        Health::Other { exe, missing } => (
            format!(
                "{exe} is installed here, and missing {}",
                missing.join(", ")
            ),
            true,
        ),
        Health::Outdated { exe: None } => ("installed, in an older cctop's form".into(), true),
        Health::Outdated { exe: Some(exe) } => (
            format!("{exe} is installed here, in an older cctop's form"),
            true,
        ),
        Health::Broken(exe) => (format!("points at {exe}, which is gone"), true),
        Health::Unreadable(why) => (why.clone(), true),
        Health::Foreign(path) => (
            format!(
                "{} is your own {SKILL_LABEL}, so cctop's is not installed",
                path.display()
            ),
            false,
        ),
    }
}

/// What one pass of [`repair`] did, and what it could not.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Repair {
    /// One line per file rewritten, ready for a status line.
    pub fixed: Vec<String>,
    /// Whether something is still wrong that repair would not or could not
    /// fix: a file that will not parse, or a write that failed. Worth one line
    /// pointing at the panel; everything repair did fix is not.
    pub needs_attention: bool,
}

impl Repair {
    /// The pass as lines for a terminal, for the entry points that have no
    /// status line of their own to put it on. Empty when there was nothing to
    /// do, which is the ordinary case and not worth a line.
    pub fn terminal_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .fixed
            .iter()
            .map(|fixed| format!("cctop: hooks: {fixed}"))
            .collect();
        if self.needs_attention {
            lines.push("cctop: agent hooks need attention — run `cctop --hooks-status`".into());
        }
        lines
    }
}

/// Fix the three ways an install can look present and deliver less than it
/// should: a recorded cctop that is gone, a set of events that is short, and
/// entries in a form an older cctop wrote.
///
/// All three are cases where there is no behaviour to preserve and nothing the
/// user could have meant. A hook naming a binary that no longer exists fires
/// nothing. An install written by an older cctop registers the events *that*
/// version knew about, so every event added since is one the agent never
/// mentions — and the states those events close out are exactly the ones that
/// otherwise stick: a tool call that failed goes on reading as a held question,
/// a turn that died on an API error goes on reading as work in progress. And an
/// entry in an older form invokes cctop the way an older cctop wanted, which
/// after an update that changed it is a hook nobody would think to reinstall.
///
/// The one thing repair will not do is move an install between binaries. Two
/// cctops on one machine is a choice, and events are delivered to every cctop
/// listening whichever binary fires them — so a shortfall or an old form in an
/// install another cctop owns is fixed *at that binary*, leaving its choice
/// alone. Only an install naming a cctop that is gone is repointed, because
/// there is nothing left there to respect.
///
/// Meant to run at every start of anything long-lived, off the thread that
/// draws: when there is nothing to do it is one read per config file and no
/// write, and each write it does make is re-read, compared and renamed into
/// place (see [`json_install`]), so two cctops starting at once cannot tear a
/// file between them.
pub fn repair(cwd: Option<&Path>) -> Repair {
    let mut scopes = vec![Scope::User];
    if let Some(dir) = cwd {
        scopes.push(Scope::Project(dir.to_path_buf()));
    }
    repair_in(&scopes)
}

/// [`repair`] over exactly the scopes named.
///
/// Split out for the tests, which must not be able to reach the machine's own
/// config: repair *writes*, and a test that swept [`Scope::User`] would edit the
/// settings of whoever ran `cargo test`.
pub(super) fn repair_in(scopes: &[Scope]) -> Repair {
    let Ok(own) = own_exe() else {
        return Repair::default();
    };
    let mut repair = Repair::default();
    for harness in HARNESSES {
        for scope in scopes {
            for config in harness.configs(scope) {
                let health = config.health();
                // Written whole either way: `install` replaces cctop's entries
                // for every event it wants, so topping up a short install,
                // refreshing an old one and repointing a dead one are the same
                // write with a different path.
                let (exe, what) = match &health {
                    Health::Broken(_) => (own.clone(), "repointed at this cctop".to_string()),
                    Health::Outdated { exe } => (
                        exe.clone().unwrap_or_else(|| own.clone()),
                        "brought up to date".to_string(),
                    ),
                    _ => match health.shortfall() {
                        None => {
                            repair.needs_attention |= health.is_problem();
                            continue;
                        }
                        Some((count, at)) => (
                            at.unwrap_or(&own).to_string(),
                            match config {
                                Config::Skill { .. } => "added".to_string(),
                                _ => format!("filled in {count} missing hooks"),
                            },
                        ),
                    },
                };
                // The skill is named, so its line does not read as a second
                // rewrite of the settings file beside it.
                let which = match config {
                    Config::Skill { .. } => format!(" {SKILL_LABEL}"),
                    _ => String::new(),
                };
                match config.install(&exe) {
                    Ok(_) => repair.fixed.push(format!(
                        "{} ({}){which}: {what}",
                        harness.label(),
                        scope.label()
                    )),
                    Err(_) => repair.needs_attention = true,
                }
            }
        }
    }
    repair
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spaced_path_an_older_cctop_left_bare_counts_as_missing() {
        let dir = scratch("hooks-spaced");
        let bin = dir.join("My Tools");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("cctop");
        std::fs::write(&exe, "").unwrap();
        let path = dir.join("settings.json");
        let bare = format!("{} hook Stop", exe.display());
        let settings = serde_json::json!({"hooks": {"Stop": [{"hooks": [
            {"type": "command", "command": bare},
        ]}]}});
        std::fs::write(&path, settings.to_string()).unwrap();

        match json_health(&path, Shape::Nested, &["Stop"], &[]) {
            Health::Other {
                exe: found,
                missing,
            } => {
                assert_eq!(found, exe.display().to_string());
                assert_eq!(missing, ["Stop"]);
            }
            other => panic!("expected the bare entry to be flagged, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An install from an older cctop registers fewer events than this one
    /// wants. That is not "installed" — the events it does not know about are
    /// silently never delivered — so it has to be visible.
    #[test]
    fn an_install_missing_newer_events_reads_as_partial() {
        let dir = scratch("partial");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let exe = own_exe().unwrap();
        std::fs::write(
            &path,
            serde_json::json!({
                "hooks": {"Stop": [{"hooks": [{"type": "command", "command": format!("{exe} hook Stop")}]}]}
            })
            .to_string(),
        )
        .unwrap();

        match Harness::Claude.health(&scope).unwrap() {
            Health::Partial(missing) => {
                assert!(missing.contains(&"SessionEnd"));
                assert!(!missing.contains(&"Stop"));
            }
            other => panic!("expected Partial, got {other:?}"),
        }

        // And installing over it fills the gap without doubling `Stop`.
        install(&scope);
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);
        assert_eq!(
            read_settings(&path).unwrap()["hooks"]["Stop"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A hook naming a cctop that no longer exists fires nothing at all, and
    /// nothing is lost by repointing it. One that names a different cctop that
    /// *does* exist is somebody's second install, and must be left alone.
    #[test]
    fn a_hook_pointing_at_a_deleted_binary_is_repaired_but_a_live_one_is_not() {
        let dir = scratch("repair");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        let gone = dir.join("moved-away-cctop");
        // A whole install, the deciding entry included, so the one thing
        // repair could find wrong with it is where it points.
        let write_pointing_at = |exe: &Path| {
            let hooks: serde_json::Map<String, serde_json::Value> = CLAUDE_EVENTS
                .iter()
                .map(|e| {
                    let mut entries = vec![serde_json::json!({"hooks": [{"type": "command",
                        "command": format!("{} hook {e}", exe.display())}]})];
                    if CLAUDE_DECIDING.contains(e) {
                        let mut entry = serde_json::json!({"hooks": [{"type": "command",
                            "command": format!("{} yolo-hook {e}", exe.display())}]});
                        if let Some(matcher) = deciding_matcher(e) {
                            entry["matcher"] = matcher.into();
                        }
                        entries.push(entry);
                    }
                    ((*e).to_string(), serde_json::Value::Array(entries))
                })
                .collect();
            std::fs::write(&path, serde_json::json!({"hooks": hooks}).to_string()).unwrap();
        };

        write_pointing_at(&gone);
        assert_eq!(
            Harness::Claude.health(&scope).unwrap(),
            Health::Broken(gone.display().to_string())
        );
        assert!(
            !repair_in(std::slice::from_ref(&scope)).fixed.is_empty(),
            "a dead path was not repaired"
        );
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);

        // A path that exists is another install, whoever made it.
        let other = dir.join("other-cctop");
        std::fs::write(&other, "").unwrap();
        write_pointing_at(&other);
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(
            repair_in(std::slice::from_ref(&scope)).fixed.is_empty(),
            "somebody else's live install was taken over"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An install written by an older cctop is short a few events, and every
    /// event it is short is a state that then sticks. Repair fills it in on the
    /// way up, without moving the install between binaries.
    ///
    /// Regression, and the shape of a real report: hooks installed months ago
    /// registered `PreToolUse` and no `PostToolUse`, so every tool call in every
    /// session stayed in flight — which cctop draws as a held permission prompt.
    /// The panel called it partial and nothing else did, so it went unnoticed
    /// until somebody wondered why the tabs kept blinking. Worse, an install
    /// naming a *different* cctop reported only that path and swallowed the
    /// shortfall entirely.
    #[test]
    fn an_install_short_of_events_is_filled_in_where_it_stands() {
        let dir = scratch("shortfall");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        // Somebody else's live cctop, registering the two events an old version
        // knew and none of the ones that close a state out.
        let theirs = dir.join("their-cctop");
        std::fs::write(&theirs, "").unwrap();
        let hooks: serde_json::Map<String, serde_json::Value> = ["Stop", "PreToolUse"]
            .iter()
            .map(|e| {
                (
                    (*e).to_string(),
                    serde_json::json!([{"hooks": [{"type": "command",
                        "command": format!("{} hook {e}", theirs.display())}]}]),
                )
            })
            .collect();
        std::fs::write(&path, serde_json::json!({"hooks": hooks}).to_string()).unwrap();

        let health = Harness::Claude.health(&scope).unwrap();
        match &health {
            Health::Other { exe, missing } => {
                assert_eq!(exe, &theirs.display().to_string());
                assert!(missing.contains(&"PostToolUse"), "{missing:?}");
            }
            other => panic!("expected Other with a shortfall, got {other:?}"),
        }
        assert!(
            health.is_problem(),
            "a shortfall behind another cctop's path read as fine"
        );

        assert!(
            !repair_in(std::slice::from_ref(&scope)).fixed.is_empty(),
            "the shortfall was not filled"
        );
        let health = Harness::Claude.health(&scope).unwrap();
        assert_eq!(
            health,
            Health::Other {
                exe: theirs.display().to_string(),
                missing: Vec::new()
            },
            "the events were filled in, but the install changed hands"
        );
        // And nothing left to do the second time.
        assert!(repair_in(std::slice::from_ref(&scope)).fixed.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A whole Claude Code install at `exe`, in a form an older cctop might
    /// have written: every event and the deciding entry there, each wrapper
    /// carrying a matcher and every command a timeout this version does not
    /// write — the shape of a hook whose invocation changed under an update.
    fn old_form_install(exe: &str) -> serde_json::Value {
        let hooks: serde_json::Map<String, serde_json::Value> = CLAUDE_EVENTS
            .iter()
            .map(|e| {
                let mut commands = vec![hook_command(exe, e)];
                if CLAUDE_DECIDING.contains(e) {
                    commands.push(yolo_hook_command(exe, e));
                }
                let entries = commands
                    .into_iter()
                    .map(|c| {
                        serde_json::json!({"matcher": "*", "hooks": [{"type": "command",
                            "command": c, "timeout": 5}]})
                    })
                    .collect();
                ((*e).to_string(), serde_json::Value::Array(entries))
            })
            .collect();
        serde_json::json!({ "hooks": hooks })
    }

    /// The issue's case: an update changed how a hook is written, and the
    /// entries an older cctop wrote — every event present, nothing missing —
    /// are rewritten into this version's form without anyone reinstalling.
    /// The user's own hooks, in the same event and in the same wrapper as
    /// cctop's, come through untouched.
    #[test]
    fn an_install_in_an_older_form_is_refreshed_and_the_users_hooks_survive() {
        let dir = scratch("outdated");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let own = own_exe().unwrap();

        let mut doc = old_form_install(&own);
        let stop = doc["hooks"]["Stop"].as_array_mut().unwrap();
        // The user's own entry, beside cctop's.
        stop.push(
            serde_json::json!({"hooks": [{"type": "command", "command": "notify-send done"}]}),
        );
        // And one of theirs in the wrapper cctop's sits in.
        stop[0]["hooks"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"type": "command", "command": "say stopped"}));
        // An event this cctop does not register, holding one of theirs.
        doc["hooks"]["NotACctopEvent"] =
            serde_json::json!([{"hooks": [{"type": "command", "command": "beep"}]}]);
        // And an event an older cctop registered and this one no longer does.
        doc["hooks"]["Retired"] = serde_json::json!([{"hooks": [{"type": "command",
            "command": hook_command(&own, "Retired")}]}]);
        std::fs::write(&path, doc.to_string()).unwrap();

        let health = Harness::Claude.health(&scope).unwrap();
        assert_eq!(health, Health::Outdated { exe: None });
        assert!(health.is_problem(), "an old form read as fine");

        let repair = repair_in(std::slice::from_ref(&scope));
        assert_eq!(
            repair.fixed,
            vec!["Claude Code (project): brought up to date"]
        );
        assert!(!repair.needs_attention);
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);

        let root = read_settings(&path).unwrap();
        // cctop's entries are exactly what an install writes now.
        let mut fresh = serde_json::Map::new();
        json_merge(
            &mut fresh,
            &path,
            Shape::Nested,
            CLAUDE_EVENTS,
            CLAUDE_DECIDING,
            &own,
        )
        .unwrap();
        assert_eq!(our_entries(&root), our_entries(&fresh));
        // Every one of the user's survived, the wrapper's matcher included.
        let all: Vec<&str> = root["hooks"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|list| list.as_array().unwrap())
            .flat_map(entry_commands)
            .collect();
        for theirs in ["notify-send done", "say stopped", "beep"] {
            assert!(all.contains(&theirs), "{theirs} was lost: {all:?}");
        }
        let shared = root["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| entry_commands(e).any(|c| c == "say stopped"))
            .unwrap();
        assert_eq!(shared["matcher"], "*", "the user's wrapper was rewritten");
        assert!(
            root["hooks"].get("Retired").is_none(),
            "a retired event was left"
        );

        // And the second pass has nothing to do.
        assert!(repair_in(std::slice::from_ref(&scope)).fixed.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Repair runs at every start, so the ordinary case — everything already
    /// right — must cost a read and nothing else: no rewrite, so not even the
    /// mtime moves. The user's formatting is part of what that keeps.
    #[test]
    fn an_install_already_up_to_date_is_not_rewritten() {
        let dir = scratch("up-to-date");
        let scope = Scope::Project(dir.clone());
        for harness in HARNESSES {
            harness.install(&scope, &own_exe().unwrap());
        }
        let claude = Harness::Claude.config_file(&scope).unwrap();
        // Reformatted by hand, and moved after somebody else's entry: neither
        // is a difference in what fires.
        let mut root = read_settings(&claude).unwrap();
        root["hooks"]["Stop"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"hooks": [{"type": "command", "command": "theirs"}]}));
        root["hooks"]["Stop"].as_array_mut().unwrap().reverse();
        std::fs::write(&claude, serde_json::to_string(&root).unwrap()).unwrap();

        let files: Vec<PathBuf> = HARNESSES
            .iter()
            .flat_map(|h| h.configs(&scope))
            .map(|c| c.path().to_path_buf())
            .collect();
        let long_ago =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        for file in &files {
            std::fs::File::options()
                .write(true)
                .open(file)
                .unwrap()
                .set_modified(long_ago)
                .unwrap();
        }
        let before: Vec<Vec<u8>> = files.iter().map(|f| std::fs::read(f).unwrap()).collect();

        let repair = repair_in(std::slice::from_ref(&scope));
        assert_eq!(
            repair,
            Repair::default(),
            "an up-to-date install was touched"
        );
        for (file, bytes) in files.iter().zip(&before) {
            assert_eq!(
                &std::fs::read(file).unwrap(),
                bytes,
                "{} changed",
                file.display()
            );
            assert_eq!(
                std::fs::metadata(file).unwrap().modified().unwrap(),
                long_ago,
                "{} was rewritten",
                file.display()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Another cctop's install in an older form is refreshed *at that cctop*:
    /// repair keeps its rule of never moving an install between live binaries.
    #[test]
    fn another_cctops_outdated_install_is_refreshed_at_its_own_binary() {
        let dir = scratch("outdated-other");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let theirs = dir.join("their-cctop");
        std::fs::write(&theirs, "").unwrap();
        let theirs = theirs.display().to_string();
        std::fs::write(&path, old_form_install(&theirs).to_string()).unwrap();

        assert_eq!(
            Harness::Claude.health(&scope).unwrap(),
            Health::Outdated {
                exe: Some(theirs.clone())
            }
        );
        assert!(!repair_in(std::slice::from_ref(&scope)).fixed.is_empty());
        assert_eq!(
            Harness::Claude.health(&scope).unwrap(),
            Health::Other {
                exe: theirs.clone(),
                missing: Vec::new()
            },
            "the form was refreshed, but the install changed hands"
        );
        let mut fresh = serde_json::Map::new();
        json_merge(
            &mut fresh,
            &path,
            Shape::Nested,
            CLAUDE_EVENTS,
            CLAUDE_DECIDING,
            &theirs,
        )
        .unwrap();
        assert_eq!(
            our_entries(&read_settings(&path).unwrap()),
            our_entries(&fresh)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The plugin is cctop's file outright, so any difference from what this
    /// version writes is an older cctop's, even with every event in it.
    #[test]
    fn an_opencode_plugin_from_an_older_cctop_is_rewritten() {
        let dir = scratch("outdated-plugin");
        let scope = Scope::Project(dir.clone());
        let path = Harness::OpenCode.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let own = own_exe().unwrap();
        std::fs::write(
            &path,
            format!("{}\n// an older cctop\n", plugin_source(&own)),
        )
        .unwrap();

        assert_eq!(
            Harness::OpenCode.health(&scope).unwrap(),
            Health::Outdated { exe: None }
        );
        assert!(!repair_in(std::slice::from_ref(&scope)).fixed.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), plugin_source(&own));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Codex reports twice, and the two halves are installed and go wrong
    /// independently: hooks say everything but are inert until a person has
    /// trusted them, and `notify` says only that a turn ended but works the
    /// moment it is written.
    ///
    /// Codex grew its hook framework after cctop had settled for `notify`, and
    /// it borrowed Claude Code's spelling wholesale — so what this mostly checks
    /// is that the reader needed nothing new, and that a report carrying one
    /// line per file tells the truth about both.
    #[test]
    fn codex_is_asked_through_its_hooks_and_its_notify_both() {
        // Only the project scope is written here. `$CODEX_HOME` resolves once
        // per process, so a test that pointed it somewhere else would be
        // gambling on running first — and losing that gamble means editing the
        // config of whoever ran `cargo test`.
        let dir = scratch("codex-hooks");
        let scope = Scope::Project(dir.clone());
        let done = install(&scope);
        let codex: Vec<&String> = done.iter().filter(|l| l.starts_with("Codex:")).collect();
        assert_eq!(codex.len(), 1, "the project scope is hooks alone: {done:?}");
        assert!(
            codex[0].contains("/hooks inside Codex"),
            "an install that reads as done while delivering nothing has to say \
             so: {:?}",
            codex[0]
        );

        // The hooks file is Claude Code's shape, in Codex's own file.
        let hooks = dir.join(".codex").join("hooks.json");
        let root = read_settings(&hooks).unwrap();
        let command = root["hooks"]["PermissionRequest"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap_or_default();
        assert!(is_our_command(command), "Codex got {command:?}");

        let entries = harness_status(Harness::Codex, scope.clone());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].health, Health::Installed);
        assert!(entries[0].note.is_some(), "the trust step went unsaid");

        // The user scope is the one that also has `notify`, which is a single
        // machine-wide program. Only its shape is checked: writing it would mean
        // writing the real `$CODEX_HOME`.
        let both = Harness::Codex.configs(&Scope::User);
        assert_eq!(both.len(), 2, "the user scope is hooks and notify");
        assert!(matches!(both[0], Config::Json { .. }));
        assert!(both[0].path().ends_with("hooks.json"));
        assert!(matches!(both[1], Config::Notify(_)));
        assert!(both[1].path().ends_with("config.toml"));
        assert!(both[1].note().is_none(), "notify needs no trusting");

        // And a Codex hook event needs nothing new to be understood: the payload
        // is Claude Code's, so the same reader takes it.
        let raw = br#"{"session_id":"thr_123","cwd":"/home/flo/cctop","hook_event_name":"PermissionRequest","model":"gpt-5.6","permission_mode":"default","tool_name":"Bash","tool_input":{"command":"rm -rf build"}}"#;
        let line = envelope("PermissionRequest", raw, &[]).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.session_id, "thr_123");
        assert_eq!(event.reported.signal, Signal::NeedsInput);
        assert_eq!(event.reported.permission, Some(Permission::Ask));

        remove(&scope);
        for entry in harness_status(Harness::Codex, scope) {
            assert_eq!(
                entry.health,
                Health::Absent,
                "{} was left",
                entry.path.display()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
