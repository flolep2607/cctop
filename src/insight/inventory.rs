//! Where each skill, agent and MCP server a session was offered is defined,
//! and whether it is still switched on.
//!
//! The transcript says what a session was given (see
//! [`crate::session::Loadout`]). It cannot say where that came from, and a
//! finding that says "remove it" without naming the file is a chore handed back
//! to the reader. So this reads Claude Code's configuration — `~/.claude.json`,
//! the settings files, `.mcp.json`, the skills and agents directories, the
//! installed plugins — and answers one question per name: which file would you
//! edit to stop loading it?
//!
//! It also answers the question the old transcripts cannot: is it *still*
//! loaded? A server somebody disabled last week is in fifty transcripts from
//! before then, and reporting it again would be telling them to do what they
//! already did.
//!
//! # Read-only, and names only
//!
//! Nothing here writes. And nothing here keeps a value: an MCP entry can carry
//! an API key in `env` or a bearer token in `headers`, so the only things that
//! leave this module are server names and file paths. The configuration is
//! parsed into the few sets of names the resolvers need and then dropped.
//!
//! ponytail: one profile. Sessions run under another `CLAUDE_CONFIG_DIR` are
//! resolved against this one's files, where their skills and servers are
//! usually not found — and something not found is never reported, so the
//! mistake can only be silence.

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::session::mcp_server_key;

/// Where Claude Code keeps its configuration.
#[derive(Debug, Clone)]
pub struct Roots {
    /// The account file: `~/.claude.json`, or `.claude.json` inside
    /// `$CLAUDE_CONFIG_DIR` when that is set. User-wide and per-project MCP
    /// servers live here, not in `settings.json`.
    pub account: PathBuf,
    /// `~/.claude`, or `$CLAUDE_CONFIG_DIR`.
    pub claude_dir: PathBuf,
}

impl Roots {
    /// This machine's.
    pub fn here() -> Roots {
        let claude_dir = crate::config::CLAUDE_CONFIG_DIR.clone();
        // With the variable set, Claude Code keeps its account file inside that
        // directory rather than beside it — see `config::argv_under_profile`.
        let account = match crate::config::env_dir("CLAUDE_CONFIG_DIR") {
            Some(dir) => dir.join(".claude.json"),
            None => crate::config::HOME.join(".claude.json"),
        };
        Roots {
            account,
            claude_dir,
        }
    }
}

/// The file, directory or switch that would stop something loading.
///
/// What findings are grouped by, because it is the unit somebody acts on: one
/// edit to one file, however many names are in it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    /// `mcpServers` at the top of the account file: every session, everywhere.
    McpUser(PathBuf),
    /// `projects.<dir>.mcpServers` in the account file: one directory, only
    /// for whoever owns this machine.
    McpLocal { account: PathBuf, project: String },
    /// A project's `.mcp.json`, which is checked in and shared with everybody
    /// who clones it.
    McpProject(PathBuf),
    /// A connector on the claude.ai account, which Claude Code picks up too.
    Connector,
    /// A plugin, by name. Its skills, agents and servers come and go together.
    Plugin(String),
    /// A skills directory, user-wide or a project's.
    Skills(PathBuf),
    /// An agents directory, likewise.
    Agents(PathBuf),
    /// Skills synced down from claude.ai rather than kept on this machine.
    SyncedSkills,
}

/// What a name resolves to for a session in one directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    At(Source),
    /// Defined, and switched off since. Not loaded now, so not reported.
    Disabled,
    /// Built into Claude Code, managed by an administrator, passed on the
    /// command line, or deleted since — nothing a finding can point at.
    Unknown,
}

/// One project's configuration, as far as the resolvers need it.
#[derive(Debug, Default)]
struct Project {
    /// `projects.<dir>.mcpServers` keys, by [`mcp_server_key`].
    local: HashSet<String>,
    /// `<dir>/.mcp.json` keys, by [`mcp_server_key`] -> the key as written,
    /// which is what `disabledMcpjsonServers` lists.
    mcp_json: HashMap<String, String>,
    /// `disabledMcpServers`, which switches off any server in this directory,
    /// whatever defined it. Holds display names, keyed.
    disabled: HashSet<String>,
    /// `disabledMcpjsonServers`, from the account file and every settings file
    /// that applies here.
    disabled_json: HashSet<String>,
    skills: HashSet<String>,
    agents: HashSet<String>,
    /// `enabledPlugins` as the project's own settings override it.
    plugins: HashMap<String, bool>,
}

/// Every definition the resolvers can find, read once.
#[derive(Debug, Default)]
pub struct Inventory {
    account: PathBuf,
    claude_dir: PathBuf,
    user_mcp: HashSet<String>,
    user_skills: HashSet<String>,
    user_agents: HashSet<String>,
    /// Skill names synced from claude.ai.
    synced_skills: HashSet<String>,
    /// Installed or synced plugins, by name -> the `name@marketplace` key
    /// `enabledPlugins` uses, where one is known.
    plugins: HashMap<String, Option<String>>,
    /// `enabledPlugins` from the user's settings.
    user_plugins: HashMap<String, bool>,
    projects: HashMap<String, Project>,
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// The keys of an object, which for a server map are the server names.
/// Only the keys: the values are where the secrets are.
fn keys_of(v: Option<&Value>) -> impl Iterator<Item = &str> {
    v.and_then(Value::as_object).into_iter().flat_map(|m| {
        m.iter()
            .filter(|(_, cfg)| cfg.is_object())
            .map(|(k, _)| k.as_str())
    })
}

fn strings_of(v: Option<&Value>) -> impl Iterator<Item = &str> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

/// `enabledPlugins`, keyed by plugin name as well as by its full key, since a
/// listing spells a plugin's skills by the bare name.
fn plugin_switches(settings: &Value) -> HashMap<String, bool> {
    let mut out = HashMap::new();
    if let Some(map) = settings.get("enabledPlugins").and_then(Value::as_object) {
        for (key, on) in map {
            if let Some(on) = on.as_bool() {
                let name = key.split('@').next().unwrap_or(key);
                out.insert(name.to_string(), on);
            }
        }
    }
    out
}

/// Skill names in a skills directory, as the listing spells them.
fn skill_names(dir: &Path) -> HashSet<String> {
    crate::access::skills(dir)
        .into_iter()
        .map(|s| s.name)
        .collect()
}

/// Agent names in an agents directory: the front matter's `name:`, else the
/// file's stem, which is what Claude Code falls back to.
fn agent_names(dir: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    for entry in crate::config::list_dir(dir) {
        let Some(stem) = entry.strip_suffix(".md") else {
            continue;
        };
        let named = crate::util::read_head(&dir.join(&entry), 4096).and_then(|text| {
            text.lines()
                .take(20)
                .find_map(|l| l.strip_prefix("name:").map(|v| v.trim().to_string()))
        });
        out.insert(named.unwrap_or_else(|| stem.to_string()));
    }
    out
}

impl Inventory {
    /// Read the configuration that applies to sessions in `cwds`.
    pub fn load<'a>(roots: &Roots, cwds: impl IntoIterator<Item = &'a str>) -> Inventory {
        let account = read_json(&roots.account).unwrap_or(Value::Null);
        let user_settings =
            read_json(&roots.claude_dir.join("settings.json")).unwrap_or(Value::Null);
        let skills_dir = roots.claude_dir.join("skills");

        let mut inv = Inventory {
            account: roots.account.clone(),
            claude_dir: roots.claude_dir.clone(),
            user_mcp: keys_of(account.get("mcpServers"))
                .map(mcp_server_key)
                .collect(),
            user_skills: skill_names(&skills_dir),
            user_agents: agent_names(&roots.claude_dir.join("agents")),
            user_plugins: plugin_switches(&user_settings),
            ..Inventory::default()
        };

        // Synced skills sit one level further down, under the organisation
        // that provides them.
        let synced = skills_dir.join("synced");
        for org in crate::config::list_dir(&synced) {
            inv.synced_skills.extend(skill_names(&synced.join(org)));
        }

        let plugins_dir = roots.claude_dir.join("plugins");
        if let Some(installed) = read_json(&plugins_dir.join("installed_plugins.json"))
            && let Some(map) = installed.get("plugins").and_then(Value::as_object)
        {
            for key in map.keys() {
                let name = key.split('@').next().unwrap_or(key);
                inv.plugins.insert(name.to_string(), Some(key.clone()));
            }
        }
        // A synced plugin's directory may carry a generation suffix,
        // `modern-web-guidance~g2`, which its skills' names do not.
        let synced = plugins_dir.join("synced");
        for org in crate::config::list_dir(&synced) {
            for entry in crate::config::list_dir(&synced.join(&org)) {
                if synced.join(&org).join(&entry).is_dir() {
                    let name = entry.split('~').next().unwrap_or(&entry);
                    inv.plugins.entry(name.to_string()).or_insert(None);
                }
            }
        }

        let user_disabled_json: Vec<&str> =
            strings_of(user_settings.get("disabledMcpjsonServers")).collect();
        for cwd in cwds {
            if cwd.is_empty() || inv.projects.contains_key(cwd) {
                continue;
            }
            let dir = Path::new(cwd);
            let entry = account.get("projects").and_then(|p| p.get(cwd));
            let settings: Vec<Value> = ["settings.json", "settings.local.json"]
                .iter()
                .filter_map(|f| read_json(&dir.join(".claude").join(f)))
                .collect();
            let mcp_json = read_json(&dir.join(".mcp.json"));
            let mut project = Project {
                local: keys_of(entry.and_then(|e| e.get("mcpServers")))
                    .map(mcp_server_key)
                    .collect(),
                // A `.mcp.json` may or may not wrap its servers in
                // `mcpServers`, the same as `access::mcp_from_json` reads it.
                mcp_json: mcp_json
                    .as_ref()
                    .map(|v| keys_of(Some(v.get("mcpServers").unwrap_or(v))))
                    .into_iter()
                    .flatten()
                    .map(|k| (mcp_server_key(k), k.to_string()))
                    .collect(),
                disabled: strings_of(entry.and_then(|e| e.get("disabledMcpServers")))
                    .map(mcp_server_key)
                    .collect(),
                disabled_json: strings_of(entry.and_then(|e| e.get("disabledMcpjsonServers")))
                    .chain(user_disabled_json.iter().copied())
                    .map(str::to_string)
                    .collect(),
                skills: skill_names(&dir.join(".claude").join("skills")),
                agents: agent_names(&dir.join(".claude").join("agents")),
                plugins: HashMap::new(),
            };
            for s in &settings {
                project
                    .disabled_json
                    .extend(strings_of(s.get("disabledMcpjsonServers")).map(str::to_string));
                // Local after shared, so the personal override wins.
                project.plugins.extend(plugin_switches(s));
            }
            inv.projects.insert(cwd.to_string(), project);
        }
        inv
    }

    fn project(&self, cwd: &str) -> Option<&Project> {
        self.projects.get(cwd)
    }

    /// The plugin a prefixed name belongs to, and whether it is switched off
    /// for `cwd`.
    fn plugin(&self, cwd: &str, name: &str) -> Resolved {
        if !self.plugins.contains_key(name) {
            return Resolved::Unknown;
        }
        let on = self
            .project(cwd)
            .and_then(|p| p.plugins.get(name))
            .or_else(|| self.user_plugins.get(name))
            .copied();
        match on {
            Some(false) => Resolved::Disabled,
            _ => Resolved::At(Source::Plugin(name.to_string())),
        }
    }

    /// Where the MCP server `server` (as its tool names spell it) comes from,
    /// for a session in `cwd`.
    ///
    /// Claude Code's own precedence: a local definition shadows the project's,
    /// which shadows the user's.
    pub fn mcp(&self, cwd: &str, server: &str) -> Resolved {
        let project = self.project(cwd);
        if project.is_some_and(|p| p.disabled.contains(server)) {
            return Resolved::Disabled;
        }
        if project.is_some_and(|p| p.local.contains(server)) {
            return Resolved::At(Source::McpLocal {
                account: self.account.clone(),
                project: cwd.to_string(),
            });
        }
        if let Some(p) = project
            && let Some(key) = p.mcp_json.get(server)
        {
            if p.disabled_json.contains(key) {
                return Resolved::Disabled;
            }
            let file = Path::new(cwd).join(".mcp.json");
            return Resolved::At(Source::McpProject(PathBuf::from(super::checkout_path(
                &file.to_string_lossy(),
            ))));
        }
        if self.user_mcp.contains(server) {
            return Resolved::At(Source::McpUser(self.account.clone()));
        }
        if server.starts_with("claude_ai_") {
            return Resolved::At(Source::Connector);
        }
        // `plugin_<plugin>_<server>`, where either half may hold underscores
        // of its own — so it is matched against the plugins that exist rather
        // than split.
        if let Some(rest) = server.strip_prefix("plugin_") {
            let mut names: Vec<&String> = self
                .plugins
                .keys()
                .filter(|p| rest.starts_with(&format!("{}_", mcp_server_key(p))))
                .collect();
            names.sort_by_key(|p| std::cmp::Reverse(p.len()));
            if let Some(name) = names.first() {
                return self.plugin(cwd, name);
            }
        }
        Resolved::Unknown
    }

    /// Where the skill `name` comes from, for a session in `cwd`.
    pub fn skill(&self, cwd: &str, name: &str) -> Resolved {
        if let Some((prefix, bare)) = name.split_once(':') {
            return match self.plugin(cwd, prefix) {
                Resolved::Unknown if self.synced_skills.contains(bare) => {
                    Resolved::At(Source::SyncedSkills)
                }
                other => other,
            };
        }
        if self.project(cwd).is_some_and(|p| p.skills.contains(name)) {
            let dir = Path::new(cwd).join(".claude").join("skills");
            return Resolved::At(Source::Skills(PathBuf::from(super::checkout_path(
                &dir.to_string_lossy(),
            ))));
        }
        if self.user_skills.contains(name) {
            return Resolved::At(Source::Skills(self.claude_dir.join("skills")));
        }
        Resolved::Unknown
    }

    /// Where the agent type `name` comes from, for a session in `cwd`.
    pub fn agent(&self, cwd: &str, name: &str) -> Resolved {
        if let Some((prefix, _)) = name.split_once(':') {
            return self.plugin(cwd, prefix);
        }
        if self.project(cwd).is_some_and(|p| p.agents.contains(name)) {
            let dir = Path::new(cwd).join(".claude").join("agents");
            return Resolved::At(Source::Agents(PathBuf::from(super::checkout_path(
                &dir.to_string_lossy(),
            ))));
        }
        if self.user_agents.contains(name) {
            return Resolved::At(Source::Agents(self.claude_dir.join("agents")));
        }
        Resolved::Unknown
    }

    /// The `enabledPlugins` key for a plugin, where one is known.
    pub fn plugin_key(&self, name: &str) -> Option<&str> {
        self.plugins.get(name).and_then(|k| k.as_deref())
    }

    /// The user settings file, for a remedy that names it.
    pub fn user_settings(&self) -> PathBuf {
        self.claude_dir.join("settings.json")
    }
}
