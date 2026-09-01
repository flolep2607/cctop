//! What an agent can reach: its instructions, skills, MCP servers and rules.
//!
//! A session's cost and its tool log say what it *did*. This says what it was
//! allowed to do and what was in scope while it did it — the CLAUDE.md it was
//! given, the skills on its path, the MCP servers wired into it, whether it asks
//! before writing, and whether cctop's own hooks are installed to hear about any
//! of it. Two questions people actually ask are answered only by this: "why did
//! it do that" (usually an instruction file nobody remembered was there) and
//! "what can it touch" (usually more than expected).
//!
//! # Why the data and the drawing are separate
//!
//! The terminal has shown most of this for a long time, in
//! [`crate::ui::panels`], as styled lines built straight from the filesystem.
//! A browser cannot use styled lines, and re-reading the same files a second
//! way is how two surfaces come to disagree about which MCP servers exist. So
//! the readers live here and return values; the panel renders them, and so does
//! [`crate::serve`].
//!
//! # It reports the files, not the truth
//!
//! Every harness resolves its own configuration, and none of them document the
//! whole of it. A settings file may be overridden by a flag on the command line,
//! an MCP server may have failed to start, a skill directory may hold something
//! the harness rejected. So this is deliberately phrased as "these are the files
//! that apply to a session in this directory", which is checkable, rather than
//! "this is what the agent has loaded", which is not. Where a harness keeps a
//! setting somewhere cctop cannot read — Windsurf's global rules live in the
//! editor's own settings UI — the entry says so instead of reporting nothing.

use crate::hook::{self, Health, Scope};
use crate::pricing::Provider;
use crate::session::{Session, SessionData};
use crate::util;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The most of an instruction file that is carried.
///
/// Enough to read a CLAUDE.md that someone forgot they wrote, and short of
/// shipping a whole repository's worth of prose to a phone.
const MAX_FILE_CHARS: usize = 8 * 1024;

/// The most skills listed.
const MAX_SKILLS: usize = 60;

/// The most distinct tools reported as used.
const MAX_TOOLS: usize = 30;

/// The most recently written paths listed.
const MAX_WRITES: usize = 20;

/// Everything in scope for one session.
#[derive(Debug, Default, Serialize)]
pub struct Access {
    /// The working directory as the transcript recorded it, spelled with `~`.
    pub cwd: String,
    /// Whether that directory is still there. A session whose checkout was
    /// deleted cannot be resumed into it, and this is why.
    pub cwd_exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub harness: String,
    pub model: String,
    /// How much this session asks before it acts, when something has said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,
    /// What that mode means, in a sentence, since the labels are terse and the
    /// difference between two of them is the whole safety story.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_detail: Option<&'static str>,
    /// PID of the live agent, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Instruction files that apply here, global first.
    pub instructions: Vec<FileRef>,
    /// Settings files that apply here.
    pub configs: Vec<FileRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills_dir: Option<String>,
    pub skills: Vec<Skill>,
    pub mcp: Vec<McpServer>,
    /// Whether cctop's own hooks are installed for this harness.
    pub hooks: Vec<HookState>,
    /// Tools this session has actually used, most-used first.
    pub tools: Vec<ToolCount>,
    /// Paths it wrote lately, newest first.
    pub writes: Vec<String>,
    /// A limit of this harness worth stating rather than leaving as a gap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<&'static str>,
}

/// A file that shapes a session, whether or not it exists.
///
/// Absent files are listed too, and on purpose: "there is no project CLAUDE.md"
/// is the answer to a question people ask, and a list that silently omits it
/// cannot be told from one that never looked.
#[derive(Debug, Serialize)]
pub struct FileRef {
    /// Spelled with `~`, since the browser may not be on this machine.
    pub path: String,
    /// `user` or `project`.
    pub scope: &'static str,
    pub present: bool,
    pub bytes: u64,
    /// The head of it, capped at [`MAX_FILE_CHARS`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub clipped: bool,
}

#[derive(Debug, Serialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct McpServer {
    pub name: String,
    /// `user` or `project`, which is the difference between a server this
    /// machine gives every session and one this repository asked for.
    pub scope: &'static str,
    /// The command that starts it, when the config names one. A remote server
    /// configured by URL has none, and inventing one would be a lie about where
    /// the agent's tool calls are going.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// Whether cctop hears from this harness, per file it installs into.
#[derive(Debug, Serialize)]
pub struct HookState {
    pub harness: String,
    pub scope: &'static str,
    pub path: String,
    /// One word: `installed`, `absent`, `partial`, `other`, `broken`.
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ToolCount {
    pub name: String,
    pub count: u64,
}

/// Read everything in scope for `session`.
///
/// `data` is the extraction if the caller already has it — the tool counts come
/// from there and nowhere else. Absent, everything else is still reported: the
/// files on disk are the expensive half of the answer and they do not need it.
pub fn build(session: &Session, data: Option<&SessionData>) -> Access {
    let cwd = Path::new(&session.label_source);
    let has_cwd = !session.label_source.is_empty();
    let root = claude_root(session);

    let mut access = Access {
        cwd: util::tildify(&session.label_source),
        cwd_exists: has_cwd && cwd.is_dir(),
        branch: crate::ui::columns::branch_of(session),
        harness: session.surface.label(session.provider).to_string(),
        model: session.model.clone(),
        permission: session.permission.map(|p| p.label().to_string()),
        permission_detail: session.permission.map(describe_permission),
        pid: session.root_pid(),
        ..Access::default()
    };

    // A row from another machine names paths on that machine. Reading them here
    // would report whatever sits at the same path locally, which is the failure
    // `Session::remote` exists to prevent.
    if let Some(remote) = &session.remote {
        access.note = Some("this session is on another machine — run cctop serve there");
        access.branch = remote.branch.clone();
        return access;
    }

    let (instructions, configs, skills_dir, note) = layout(session, &root);
    access.instructions = instructions
        .into_iter()
        .map(|(path, scope)| file_ref(&path, scope))
        .collect();
    access.configs = configs
        .into_iter()
        .map(|(path, scope)| file_ref(&path, scope))
        .collect();
    access.skills_dir = skills_dir
        .as_ref()
        .map(|dir| util::tildify(&dir.to_string_lossy()));
    access.skills = skills_dir.map(|dir| skills(&dir)).unwrap_or_default();
    access.mcp = mcp_servers(session, &root);
    access.hooks = hooks(session, cwd);
    access.note = note;

    if let Some(data) = data {
        let mut tools: Vec<ToolCount> = data
            .metrics
            .tools
            .iter()
            .map(|(name, count)| ToolCount {
                name: util::pretty_mcp_name(name),
                count: *count,
            })
            .collect();
        // Most-used first, then by name, so a refresh does not reshuffle the
        // ties a `HashMap` hands over in a different order every time.
        tools.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        tools.truncate(MAX_TOOLS);
        access.tools = tools;
    }
    access.writes = session
        .recent_writes
        .iter()
        .take(MAX_WRITES)
        .map(|p| util::tildify(p))
        .collect();

    access
}

/// Which Claude directory this session belongs to: a profile's, a Claude for Mac
/// session's, or the default.
fn claude_root(session: &Session) -> PathBuf {
    match (session.surface.is_desktop(), &session.mac_meta) {
        (true, Some(meta)) => meta.session_dir.join(".claude"),
        _ => crate::config::CLAUDE_CONFIG_DIR.clone(),
    }
}

type Layout = (
    Vec<(PathBuf, &'static str)>,
    Vec<(PathBuf, &'static str)>,
    Option<PathBuf>,
    Option<&'static str>,
);

/// The files and directories one harness reads, for a session in `cwd`.
///
/// This is the map the terminal panel and the web page share. Adding a harness
/// means adding an arm here and both surfaces learn about it.
fn layout(session: &Session, root: &Path) -> Layout {
    let cwd = Path::new(&session.label_source);
    let project = |name: &str| match session.label_source.is_empty() {
        true => None,
        false => Some((cwd.join(name), "project")),
    };
    let mut instructions: Vec<(PathBuf, &'static str)> = Vec::new();
    let mut configs: Vec<(PathBuf, &'static str)> = Vec::new();
    let mut skills = None;
    let mut note = None;

    match session.provider {
        Provider::Claude => {
            instructions.push((root.join("CLAUDE.md"), "user"));
            instructions.extend(project("CLAUDE.md"));
            configs.push((root.join("settings.json"), "user"));
            // A project's settings live one directory down, which is also where
            // its `settings.local.json` sits — that one is a personal override
            // and is deliberately not listed as if the repository asked for it.
            if !session.label_source.is_empty() {
                configs.push((cwd.join(".claude").join("settings.json"), "project"));
            }
            skills = Some(root.join("skills"));
        }
        Provider::Codex => {
            instructions.push((crate::config::CODEX_HOME.join("AGENTS.md"), "user"));
            instructions.extend(project("AGENTS.md"));
            configs.push((crate::config::CODEX_HOME.join("config.toml"), "user"));
            skills = Some(crate::config::CODEX_HOME.join("skills"));
        }
        Provider::OpenCode => {
            instructions.extend(project("AGENTS.md"));
            configs.push((
                crate::config::OPENCODE_CONFIG_DIR.join("opencode.json"),
                "user",
            ));
        }
        Provider::Pi => {
            instructions.push((crate::config::PI_AGENT_DIR.join("AGENTS.md"), "user"));
            instructions.extend(project("AGENTS.md"));
            configs.push((crate::config::PI_AGENT_DIR.join("settings.json"), "user"));
            skills = Some(crate::config::PI_AGENT_DIR.join("skills"));
        }
        Provider::Gemini => {
            instructions.push((crate::config::GEMINI_HOME.join("GEMINI.md"), "user"));
            instructions.extend(project("GEMINI.md"));
            configs.push((crate::config::GEMINI_HOME.join("settings.json"), "user"));
            skills = Some(crate::config::GEMINI_HOME.join("skills"));
        }
        Provider::Cursor => {
            instructions.extend(project(".cursorrules"));
            note = Some(
                "Cursor keeps its rules and its model settings in the editor, \
                 so only a project's own rules file is readable here",
            );
        }
        Provider::Windsurf => {
            instructions.extend(project(".windsurfrules"));
            note = Some(
                "Windsurf's global rules live in the editor's settings UI \
                 rather than in a file, so only the workspace rules are listed",
            );
        }
    }
    (instructions, configs, skills, note)
}

/// One file, read if it is there.
fn file_ref(path: &Path, scope: &'static str) -> FileRef {
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let text = util::read_head(path, MAX_FILE_CHARS * 4);
    let (head, clipped) = match &text {
        Some(body) => {
            let clipped = body.chars().count() > MAX_FILE_CHARS;
            let head: String = match clipped {
                true => body.chars().take(MAX_FILE_CHARS).collect(),
                false => body.clone(),
            };
            (Some(head), clipped || (bytes as usize) > MAX_FILE_CHARS * 4)
        }
        None => (None, false),
    };
    FileRef {
        path: util::tildify(&path.to_string_lossy()),
        scope,
        present: text.is_some(),
        bytes,
        head,
        clipped,
    }
}

/// Skill names and descriptions out of each `SKILL.md` front matter.
///
/// The front matter is read as lines rather than parsed as YAML: two keys are
/// wanted out of a file whose body is markdown, and a parser would have to be
/// told where the front matter ends anyway.
pub fn skills(dir: &Path) -> Vec<Skill> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for entry in crate::config::list_dir(dir) {
        let (mut name, mut description) = (entry.clone(), String::new());
        if let Some(text) = util::read_head(&dir.join(&entry).join("SKILL.md"), 4096) {
            for line in text.lines().take(20) {
                if let Some(v) = line.strip_prefix("name:") {
                    name = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("description:") {
                    description = v.trim().to_string();
                }
            }
        }
        out.push(Skill { name, description });
        if out.len() >= MAX_SKILLS {
            break;
        }
    }
    out
}

/// Every MCP server configured for this session, user scope first.
fn mcp_servers(session: &Session, root: &Path) -> Vec<McpServer> {
    let cwd = Path::new(&session.label_source);
    let mut out = Vec::new();
    match session.provider {
        Provider::Claude => {
            out.extend(mcp_from_json(&root.join("settings.json"), "user"));
            if !session.label_source.is_empty() {
                // A project's `.mcp.json` is the file a repository uses to hand
                // every clone of itself the same servers.
                out.extend(mcp_from_json(&cwd.join(".mcp.json"), "project"));
            }
        }
        Provider::Codex => out.extend(mcp_from_toml(
            &crate::config::CODEX_HOME.join("config.toml"),
        )),
        Provider::Gemini => {
            out.extend(mcp_from_json(
                &crate::config::GEMINI_HOME.join("settings.json"),
                "user",
            ));
        }
        Provider::Pi => {
            out.extend(mcp_from_json(
                &crate::config::PI_AGENT_DIR.join("settings.json"),
                "user",
            ));
        }
        Provider::OpenCode => {
            out.extend(mcp_from_json(
                &crate::config::OPENCODE_CONFIG_DIR.join("opencode.json"),
                "user",
            ));
        }
        Provider::Cursor | Provider::Windsurf => {}
    }
    out
}

/// Servers out of a JSON config, whether they sit under `mcpServers` or at the
/// top level — a project `.mcp.json` uses either.
pub fn mcp_from_json(path: &Path, scope: &'static str) -> Vec<McpServer> {
    let Some(text) = util::read_head(path, 64 * 1024) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let servers = value.get("mcpServers").unwrap_or(&value);
    let Some(map) = servers.as_object() else {
        return Vec::new();
    };
    map.iter()
        .filter(|(_, cfg)| cfg.is_object())
        .map(|(name, cfg)| McpServer {
            name: name.clone(),
            scope,
            command: cfg
                .get("command")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    cfg.get("url")
                        .and_then(Value::as_str)
                        .map(|url| url.to_string())
                }),
        })
        .collect()
}

/// Servers out of Codex's `config.toml`, by table header.
///
/// Scanning headers rather than parsing the file: the whole question is which
/// `[mcp_servers.<name>]` tables exist, and a TOML parse of a file that may hold
/// anything is more ways to fail for the same answer.
pub fn mcp_from_toml(path: &Path) -> Vec<McpServer> {
    let Some(text) = util::read_head(path, 64 * 1024) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("[mcp_servers.")
                .and_then(|rest| rest.strip_suffix(']'))
                .map(|name| McpServer {
                    name: name.trim_matches('"').to_string(),
                    scope: "user",
                    command: None,
                })
        })
        .collect()
}

/// Whether cctop's hooks are installed for this session's harness.
///
/// Both scopes, because the answer differs between them and a session in a
/// directory with its own settings file is governed by that one. A harness cctop
/// does not integrate with — Pi and Windsurf so far — reports nothing rather
/// than reporting "absent", which would read as a thing to fix.
fn hooks(session: &Session, cwd: &Path) -> Vec<HookState> {
    let Some(harness) = harness_for(session.provider) else {
        return Vec::new();
    };
    let mut scopes = vec![Scope::User];
    if !session.label_source.is_empty() && cwd.is_dir() {
        scopes.push(Scope::Project(cwd.to_path_buf()));
    }
    scopes
        .into_iter()
        .flat_map(|scope| hook::harness_status(harness, scope))
        .map(|status| {
            let (state, detail) = match &status.health {
                Health::Installed => ("installed", None),
                Health::Absent => ("absent", None),
                Health::Partial(missing) => {
                    ("partial", Some(format!("missing {}", missing.join(", "))))
                }
                Health::Other { exe, .. } => ("other", Some(format!("installed at {exe}"))),
                Health::Broken(exe) => ("broken", Some(format!("points at {exe}, which is gone"))),
                Health::Unreadable(why) => ("unreadable", Some(why.clone())),
            };
            HookState {
                harness: status.harness.label().to_string(),
                scope: status.scope.label(),
                path: util::tildify(&status.path.to_string_lossy()),
                state,
                detail: detail.or_else(|| status.note.map(str::to_string)),
            }
        })
        .collect()
}

/// The hook harness matching a provider, where cctop integrates with one.
fn harness_for(provider: Provider) -> Option<hook::Harness> {
    match provider {
        Provider::Claude => Some(hook::Harness::Claude),
        Provider::Codex => Some(hook::Harness::Codex),
        Provider::Cursor => Some(hook::Harness::Cursor),
        Provider::Gemini => Some(hook::Harness::Gemini),
        Provider::OpenCode => Some(hook::Harness::OpenCode),
        Provider::Pi | Provider::Windsurf => None,
    }
}

/// What a permission mode means, spelled out.
///
/// The column has room for one word and the difference between two of those
/// words is whether an agent can write to the disk without being asked, which is
/// worth a sentence somewhere.
fn describe_permission(permission: hook::Permission) -> &'static str {
    match permission {
        hook::Permission::Ask => "asks before anything it is not already allowed to do",
        hook::Permission::AcceptEdits => "writes files without asking; everything else still asks",
        hook::Permission::Plan => "reading and planning only — it cannot act yet",
        hook::Permission::Bypass => "asks about nothing at all",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn session_in(dir: &Path, provider: Provider) -> Session {
        let mut session = Session::new(provider, "s1".into());
        session.label_source = dir.to_string_lossy().into_owned();
        session
    }

    #[test]
    fn a_project_instruction_file_is_read_and_a_missing_one_is_still_listed() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("CLAUDE.md"), "always run the gate").unwrap();
        let access = build(&session_in(dir.path(), Provider::Claude), None);

        let project = access
            .instructions
            .iter()
            .find(|f| f.scope == "project")
            .expect("the project instruction file should be listed");
        assert!(project.present);
        assert_eq!(project.head.as_deref(), Some("always run the gate"));
        // The user-scope file is listed whether or not this machine has one.
        assert!(access.instructions.iter().any(|f| f.scope == "user"));
    }

    /// The absent entry is the answer to "is there a project CLAUDE.md", and a
    /// list that dropped it could not be told from one that never looked.
    #[test]
    fn a_file_that_is_not_there_is_reported_as_absent_rather_than_omitted() {
        let dir = tempfile::tempdir().unwrap();
        let access = build(&session_in(dir.path(), Provider::Claude), None);
        let project = access
            .instructions
            .iter()
            .find(|f| f.scope == "project")
            .unwrap();
        assert!(!project.present);
        assert!(project.head.is_none());
    }

    #[test]
    fn a_long_instruction_file_is_cut_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("CLAUDE.md"),
            "x".repeat(MAX_FILE_CHARS + 500),
        )
        .unwrap();
        let access = build(&session_in(dir.path(), Provider::Claude), None);
        let project = access
            .instructions
            .iter()
            .find(|f| f.scope == "project")
            .unwrap();
        assert!(project.clipped);
        assert_eq!(
            project.head.as_deref().unwrap().chars().count(),
            MAX_FILE_CHARS
        );
    }

    #[test]
    fn skills_come_back_named_and_described_from_their_front_matter() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join("run-cctop");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: run-cctop\ndescription: Build, run and screenshot cctop\n---\n",
        )
        .unwrap();
        let found = skills(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "run-cctop");
        assert_eq!(found[0].description, "Build, run and screenshot cctop");
    }

    #[test]
    fn a_skill_directory_that_does_not_exist_is_empty_not_an_error() {
        assert!(skills(Path::new("/nonexistent/skills")).is_empty());
    }

    #[test]
    fn mcp_servers_are_read_from_either_shape_of_json() {
        let dir = tempfile::tempdir().unwrap();
        let wrapped = dir.path().join("settings.json");
        fs::write(
            &wrapped,
            r#"{"mcpServers":{"linear":{"command":"npx linear-mcp"}}}"#,
        )
        .unwrap();
        let bare = dir.path().join(".mcp.json");
        fs::write(&bare, r#"{"sentry":{"url":"https://mcp.sentry.dev"}}"#).unwrap();

        let user = mcp_from_json(&wrapped, "user");
        assert_eq!(user.len(), 1);
        assert_eq!(user[0].name, "linear");
        assert_eq!(user[0].command.as_deref(), Some("npx linear-mcp"));

        let project = mcp_from_json(&bare, "project");
        assert_eq!(project.len(), 1);
        assert_eq!(project[0].scope, "project");
        // A remote server has a URL and no command, and reporting a command it
        // does not have would misstate where its tool calls go.
        assert_eq!(
            project[0].command.as_deref(),
            Some("https://mcp.sentry.dev")
        );
    }

    #[test]
    fn codex_mcp_servers_come_from_their_table_headers() {
        let dir = tempfile::tempdir().unwrap();
        let toml = dir.path().join("config.toml");
        fs::write(
            &toml,
            "model = \"gpt-5\"\n\n[mcp_servers.playwright]\ncommand = \"npx\"\n\n[mcp_servers.docs]\n",
        )
        .unwrap();
        let servers = mcp_from_toml(&toml);
        let names: Vec<&str> = servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["playwright", "docs"]);
    }

    /// A remote row's paths belong to another filesystem. Reading them here
    /// would report a local file as that machine's configuration.
    #[test]
    fn a_remote_session_reports_nothing_off_this_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = session_in(dir.path(), Provider::Claude);
        fs::write(dir.path().join("CLAUDE.md"), "local file").unwrap();
        session.remote = Some(crate::session::Remote {
            host: "build-box".into(),
            branch: Some("main".into()),
        });
        let access = build(&session, None);
        assert!(access.instructions.is_empty());
        assert!(access.mcp.is_empty());
        assert_eq!(access.branch.as_deref(), Some("main"));
        assert!(access.note.is_some_and(|n| n.contains("another machine")));
    }

    #[test]
    fn tools_used_are_ranked_and_mcp_names_made_readable() {
        let dir = tempfile::tempdir().unwrap();
        let mut data = SessionData::default();
        data.metrics.tools.insert("Read".into(), 3);
        data.metrics
            .tools
            .insert("mcp__linear__list_issues".into(), 9);
        let access = build(&session_in(dir.path(), Provider::Claude), Some(&data));
        assert_eq!(access.tools[0].count, 9);
        assert!(
            !access.tools[0].name.contains("mcp__"),
            "an MCP tool should be spelled for a reader: {}",
            access.tools[0].name
        );
        assert_eq!(access.tools[1].name, "Read");
    }

    /// Cursor and Windsurf keep most of this where cctop cannot read it, and
    /// saying so is better than an empty panel that looks like a bug.
    #[test]
    fn a_harness_that_hides_its_rules_says_where_they_are() {
        let dir = tempfile::tempdir().unwrap();
        let access = build(&session_in(dir.path(), Provider::Windsurf), None);
        assert!(access.note.is_some_and(|n| n.contains("settings UI")));
        // And no hook lines, since cctop does not install into Windsurf at all.
        assert!(access.hooks.is_empty());
    }
}
