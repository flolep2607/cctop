//! What a session was handed and never used, and what the instructions it was
//! handed cost.
//!
//! Every skill, agent and MCP server Claude Code knows about is described in
//! the prompt before the conversation starts, and that prefix is read again —
//! from the cache, at the cache-read price — on every request the session
//! makes. A skill nobody invokes is therefore not free just because it is
//! never run. Usually it is close to free, and the [value
//! floor](super::optimize) is what keeps a list of forty skills at a few cents
//! each from being printed as forty chores.
//!
//! # Not crying wolf
//!
//! "Never used" is the easiest claim in this module to get wrong, so each part
//! of it is read from the strictest source available:
//!
//! - **Offered** comes from the transcript, never from the configuration: a
//!   session counts toward a server only if its own listing names that server.
//!   A server switched off, never approved, or added yesterday is in no
//!   listing from before then, so it cannot be judged by sessions that never
//!   had it.
//! - **Used** is any of the ways in: a tool call, an MCP resource read, a slash
//!   command, the `Skill` tool, the model reading the skill's own `SKILL.md`,
//!   or a delegation to the agent.
//! - **Still there** is the configuration as it stands now, read by
//!   [`super::inventory`]. Something removed or disabled since is not reported
//!   from the transcripts of before, and something cctop cannot trace to a
//!   file — Claude Code's own built-in skills, a managed server — is not
//!   reported at all, since there is nothing to point at.
//! - **Often enough**: [`MIN_SESSIONS`] sessions offered it, so one week of
//!   not needing a tool is not a verdict on it.

use super::inventory::{Inventory, Resolved, Source};
use super::optimize::{Basis, Class, Finding, short_path};
use super::{Analysis, plural};
use crate::session::claude::CHARS_PER_TOKEN;
use std::collections::{BTreeMap, HashMap, HashSet};

/// How many sessions have to have been offered something, without using it,
/// before it is called unused.
///
/// The same five as the cross-session re-read detector: the point where it
/// stops looking like a run of work that happened not to need it.
pub(super) const MIN_SESSIONS: usize = 5;

/// The size past which Claude Code itself warns that a memory file is large.
///
/// Borrowed rather than chosen. A threshold cctop invented would be an
/// opinion about how long somebody's instructions should be; this is the
/// harness's own, and a file past it is one the harness has already said
/// something about.
pub(super) const MEMORY_WARN_CHARS: u64 = 40_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Kind {
    Mcp,
    Skill,
    Agent,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::Mcp, Kind::Skill, Kind::Agent];

    /// What this session was offered, or `None` where its transcript did not
    /// say — which is not the same as being offered nothing.
    fn offered(self, a: &Analysis) -> Option<&HashMap<String, u64>> {
        let l = &a.loadout;
        match self {
            Kind::Mcp => l.mcp_listed.then_some(&l.mcp),
            Kind::Skill => l.skills_listed.then_some(&l.skills),
            Kind::Agent => l.agents_listed.then_some(&l.agents),
        }
    }

    fn used(self, a: &Analysis) -> HashSet<&str> {
        match self {
            Kind::Mcp => a.used_mcp.iter().map(String::as_str).collect(),
            Kind::Agent => a.used_agents.iter().map(String::as_str).collect(),
            Kind::Skill => {
                let mut out: HashSet<&str> = a.used_skills.iter().map(String::as_str).collect();
                // A model that opens the skill's own file has used it, by a
                // route that leaves no `Skill` call behind.
                for path in &a.read_paths {
                    if let Some(dir) = path.strip_suffix("/SKILL.md")
                        && let Some((_, name)) = dir.rsplit_once('/')
                    {
                        out.insert(name);
                    }
                }
                out
            }
        }
    }

    fn resolve(self, inv: &Inventory, cwd: &str, name: &str) -> Resolved {
        match self {
            Kind::Mcp => inv.mcp(cwd, name),
            Kind::Skill => inv.skill(cwd, name),
            Kind::Agent => inv.agent(cwd, name),
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Kind::Mcp => "MCP server",
            Kind::Skill => "skill",
            Kind::Agent => "agent",
        }
    }
}

/// What something in the prompt prefix cost one session.
///
/// Its characters, at the fitted [`CHARS_PER_TOKEN`], once per request, at the
/// price this session paid for a cached token. The first request writes it to
/// the cache rather than reading it, at a higher price, so this is a little
/// under — and the characters are counted, but the tokens are not, which is
/// why everything priced here is `estimated`.
fn prefix_cost(a: &Analysis, chars: u64) -> (u64, f64) {
    let tokens = chars as f64 / CHARS_PER_TOKEN * a.loadout.requests as f64;
    (
        tokens.round() as u64,
        a.cached_rate.map_or(0.0, |r| tokens * r),
    )
}

/// One name, from one source, across the sessions that were offered it.
#[derive(Debug, Default)]
struct Tally {
    sessions: HashSet<usize>,
    tokens: u64,
    usd: f64,
}

/// Names a reader can scan: the first few, and how many more.
fn names_line(names: &[String]) -> String {
    const SHOWN: usize = 6;
    let mut shown: Vec<String> = names.iter().take(SHOWN).map(|n| format!("`{n}`")).collect();
    if names.len() > SHOWN {
        shown.push(format!("and {} more", names.len() - SHOWN));
    }
    shown.join(", ")
}

/// A name as the reader would recognise it, inside the group it is listed
/// under: `debug` rather than `engineering:debug` under the `engineering`
/// plugin, `Gmail` rather than `claude_ai_Gmail` under the connectors.
fn display(source: &Source, kind: Kind, name: &str) -> String {
    let stripped = match (source, kind) {
        (Source::Plugin(p), Kind::Mcp) => {
            name.strip_prefix(&format!("plugin_{}_", crate::session::mcp_server_key(p)))
        }
        (Source::Plugin(p), _) => name.strip_prefix(&format!("{p}:")),
        (Source::Connector, _) => {
            return name
                .strip_prefix("claude_ai_")
                .unwrap_or(name)
                .replace('_', " ");
        }
        (Source::SyncedSkills, _) => name.split_once(':').map(|(_, bare)| bare),
        _ => None,
    };
    stripped.unwrap_or(name).to_string()
}

/// What to do about one source, in the user's own terms.
fn remedy(inv: &Inventory, source: &Source, items: &[(Kind, String)]) -> String {
    let tilde = |p: &std::path::Path| crate::util::tildify(&p.to_string_lossy());
    let names: Vec<String> = items
        .iter()
        .map(|(kind, name)| display(source, *kind, name))
        .collect();
    let list = names_line(&names);
    match source {
        Source::McpUser(file) => format!(
            "{list}, in `mcpServers` in {}. `claude mcp remove <name> -s user` takes one out; \
             `/mcp` can switch one off for just the projects that never call it.",
            tilde(file)
        ),
        Source::McpLocal { account, project } => format!(
            "{list}, defined for {} only, under `projects` in {}. `claude mcp remove <name> \
             -s local`, run there, takes one out.",
            crate::util::tildify(project),
            tilde(account)
        ),
        Source::McpProject(file) => format!(
            "{list}, in {}. That file is shared with everyone who clones the project, so \
             naming them in `disabledMcpjsonServers` in .claude/settings.local.json stops \
             them loading for you without taking them from anyone else.",
            tilde(file)
        ),
        Source::Connector => format!(
            "{list}, connected through your claude.ai account. `/mcp` switches one off per \
             project and leaves it working in claude.ai itself."
        ),
        Source::Plugin(name) => {
            // Said per kind, because "datadog" and "debug" in one list do not
            // say which is the server and which the skill.
            let part = |kind: Kind| -> Option<String> {
                let of: Vec<String> = items
                    .iter()
                    .filter(|(k, _)| *k == kind)
                    .map(|(k, n)| display(source, *k, n))
                    .collect();
                (!of.is_empty())
                    .then(|| format!("{} {}", plural_word(of.len(), kind), names_line(&of)))
            };
            let parts: Vec<String> = Kind::ALL.into_iter().filter_map(part).collect();
            let switch = match inv.plugin_key(name) {
                Some(key) => format!(
                    ", or set `\"{key}\": false` under `enabledPlugins` in {}",
                    tilde(&inv.user_settings())
                ),
                None => String::new(),
            };
            format!(
                "Nothing it adds was used — {}. A plugin comes and goes whole: `/plugin` \
                 disables it{switch}.",
                parts.join("; ")
            )
        }
        Source::Skills(dir) => format!(
            "{list}, in {}. Every skill there is described to every session, used or not; \
             moving one out of the directory takes it off the list.",
            tilde(dir)
        ),
        Source::Agents(dir) => format!(
            "{list}, in {}. Every agent there is described to every session, used or not.",
            tilde(dir)
        ),
        Source::SyncedSkills => format!(
            "{list}, synced from your claude.ai account rather than kept here, so the switch \
             for them is on claude.ai, not in this machine's skills directory."
        ),
    }
}

/// `skill` / `skills`, without the count [`plural`] would put in front.
fn plural_word(n: usize, kind: Kind) -> String {
    match n {
        1 => kind.noun().to_string(),
        _ => format!("{}s", kind.noun()),
    }
}

/// Things offered to at least [`MIN_SESSIONS`] sessions and never used, one
/// finding per file or switch that would remove them.
pub(super) fn never_used(live: &[&Analysis], inv: &Inventory) -> Vec<Finding> {
    // (kind, name, source) -> where it was offered.
    let mut offered: HashMap<(Kind, String, Source), Tally> = HashMap::new();
    let mut used: HashSet<(Kind, String, Source)> = HashSet::new();
    // Plugins any part of which was used: those cannot be removed in part.
    let mut plugin_used: HashSet<String> = HashSet::new();

    for (i, a) in live.iter().enumerate() {
        for kind in Kind::ALL {
            for name in kind.used(a) {
                if let Resolved::At(src) = kind.resolve(inv, &a.cwd, name) {
                    if let Source::Plugin(p) = &src {
                        plugin_used.insert(p.clone());
                    }
                    used.insert((kind, name.to_string(), src));
                }
            }
            let Some(list) = kind.offered(a) else {
                continue;
            };
            for (name, &chars) in list {
                let Resolved::At(src) = kind.resolve(inv, &a.cwd, name) else {
                    continue;
                };
                let t = offered.entry((kind, name.clone(), src)).or_default();
                t.sessions.insert(i);
                let (tokens, usd) = prefix_cost(a, chars);
                t.tokens += tokens;
                t.usd += usd;
            }
        }
    }

    let mut groups: BTreeMap<Source, Vec<(Kind, String, Tally)>> = BTreeMap::new();
    for ((kind, name, src), tally) in offered {
        if tally.sessions.len() < MIN_SESSIONS
            || used.contains(&(kind, name.clone(), src.clone()))
            || !still_offered(live, inv, kind, &name, &src)
        {
            continue;
        }
        groups.entry(src).or_default().push((kind, name, tally));
    }

    let mut out = Vec::new();
    for (src, mut items) in groups {
        if let Source::Plugin(p) = &src
            && plugin_used.contains(p)
        {
            continue;
        }
        items.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        let names: Vec<(Kind, String)> = items.iter().map(|(k, n, _)| (*k, n.clone())).collect();
        let sessions: HashSet<usize> = items
            .iter()
            .flat_map(|(_, _, t)| t.sessions.iter().copied())
            .collect();
        let tokens: u64 = items.iter().map(|(_, _, t)| t.tokens).sum();
        let usd: f64 = items.iter().map(|(_, _, t)| t.usd).sum();
        let title = match &src {
            Source::Plugin(p) => format!(
                "Plugin `{p}` unused in {}",
                plural(sessions.len(), "session")
            ),
            _ => {
                let kind = items[0].0;
                let verb = match kind {
                    Kind::Mcp => "called",
                    _ => "used",
                };
                format!(
                    "{} never {verb} in {}",
                    plural(items.len(), kind.noun()),
                    plural(sessions.len(), "session")
                )
            }
        };
        out.push(Finding {
            class: Class::Fix,
            title,
            remedy: remedy(inv, &src, &names),
            tokens,
            usd,
            // The characters are counted; their tokens are fitted.
            basis: Basis::Estimated,
            sessions: sessions.len(),
            floor: false,
        });
    }
    out
}

/// Whether the newest session that could have been offered `name` from `src`
/// was.
///
/// The configuration says whether a file still defines something, but not for
/// a connector or a synced skill, which live on an account cctop cannot read.
/// The newest listing can say: if the last session in scope no longer had it,
/// it has been dealt with, and fifty older transcripts saying otherwise are
/// history rather than advice.
fn still_offered(
    live: &[&Analysis],
    inv: &Inventory,
    kind: Kind,
    name: &str,
    src: &Source,
) -> bool {
    live.iter()
        .filter_map(|a| kind.offered(a).map(|list| (a, list)))
        .filter(|(a, _)| kind.resolve(inv, &a.cwd, name) == Resolved::At(src.clone()))
        .max_by(|(a, _), (b, _)| a.last_active.cmp(&b.last_active))
        .is_some_and(|(_, list)| list.contains_key(name))
}

/// What the memory files cost, and any that are past the harness's own
/// warning.
///
/// A `Note`, deliberately, and phrased as a price. CLAUDE.md is the cheapest
/// place to tell an agent something — several of the other findings
/// recommend putting more in it — so a report that called its cost waste
/// would contradict itself. The one objective line is the harness's own: past
/// [`MEMORY_WARN_CHARS`], Claude Code warns about the file, and the share past
/// that line is offered as a `Habit`.
pub(super) fn memory(live: &[&Analysis]) -> Vec<Finding> {
    // Worktrees carry their own copy of the checkout's CLAUDE.md; it is one
    // file to edit, so it is one row.
    let mut files: HashMap<String, (u64, Tally)> = HashMap::new();
    for (i, a) in live.iter().enumerate() {
        for (path, &chars) in &a.loadout.memory {
            if chars == 0 {
                continue;
            }
            let (size, t) = files.entry(super::checkout_path(path)).or_default();
            *size = (*size).max(chars);
            t.sessions.insert(i);
            let (tokens, usd) = prefix_cost(a, chars);
            t.tokens += tokens;
            t.usd += usd;
        }
    }
    if files.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut ranked: Vec<(&String, &(u64, Tally))> = files.iter().collect();
    ranked.sort_by(|a, b| {
        b.1.1
            .usd
            .partial_cmp(&a.1.1.usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.1.1.tokens.cmp(&a.1.1.tokens))
            .then(a.0.cmp(b.0))
    });
    for (path, (chars, t)) in &ranked {
        if *chars > MEMORY_WARN_CHARS {
            // Only the part past the warning is offered as a saving; the rest
            // is instructions somebody chose to give.
            let share = (*chars - MEMORY_WARN_CHARS) as f64 / *chars as f64;
            out.push(Finding {
                class: Class::Habit,
                title: format!("{} is past Claude Code's size warning", short_path(path)),
                remedy: format!(
                    "{} is {} characters, and Claude Code warns past {}. It is re-read on \
                     every request, so the part past the line is the part worth moving into \
                     files the agent opens when it needs them.",
                    crate::util::tildify(path),
                    chars,
                    MEMORY_WARN_CHARS
                ),
                tokens: (t.tokens as f64 * share).round() as u64,
                usd: t.usd * share,
                basis: Basis::Estimated,
                sessions: t.sessions.len(),
                floor: false,
            });
        }
    }

    let sessions: HashSet<usize> = files
        .values()
        .flat_map(|(_, t)| t.sessions.iter().copied())
        .collect();
    let largest = ranked
        .iter()
        .take(3)
        .map(|(p, (chars, _))| {
            format!(
                "{} (~{} tokens)",
                crate::util::tildify(p),
                (*chars as f64 / CHARS_PER_TOKEN).round() as u64
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    out.push(Finding {
        class: Class::Note,
        title: format!(
            "{} re-read on every request, in {}",
            plural(files.len(), "memory file"),
            plural(sessions.len(), "session")
        ),
        remedy: format!(
            "Largest: {largest}. That is what standing instructions cost, not waste — a line \
             that saves the agent one search pays for itself. Only a line nobody needs any \
             more is worth cutting."
        ),
        tokens: files.values().map(|(_, t)| t.tokens).sum(),
        usd: files.values().map(|(_, t)| t.usd).sum(),
        basis: Basis::Estimated,
        sessions: sessions.len(),
        floor: false,
    });
    out
}

#[cfg(test)]
mod tests {
    use super::super::inventory::Roots;
    use super::*;
    use crate::insight::Task;
    use crate::pricing::Provider;

    /// A configuration root of its own, so no test reads the real one.
    struct Machine {
        dir: tempfile::TempDir,
    }

    impl Machine {
        fn new() -> Machine {
            Machine {
                dir: tempfile::tempdir().unwrap(),
            }
        }
        fn path(&self, rel: &str) -> std::path::PathBuf {
            self.dir.path().join(rel)
        }
        fn write(&self, rel: &str, body: &str) {
            let p = self.path(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        fn roots(&self) -> Roots {
            Roots {
                account: self.path("home/.claude.json"),
                claude_dir: self.path("home/.claude"),
            }
        }
        fn project(&self) -> String {
            self.path("work/app").to_string_lossy().into_owned()
        }
        fn inventory(&self, sessions: &[Analysis]) -> Inventory {
            Inventory::load(&self.roots(), sessions.iter().map(|a| a.cwd.as_str()))
        }
    }

    /// A Claude session in `cwd` that was offered `servers` (with a few
    /// hundred characters each) and made 100 requests at $1 per million
    /// cached tokens.
    fn offered(cwd: &str, ts: &str, servers: &[&str]) -> Analysis {
        let mut a = crate::insight::optimize::tests::session(10.0, 1, Task::Coding);
        a.provider = Provider::Claude;
        a.cwd = cwd.to_string();
        a.last_active = ts.to_string();
        a.loadout.mcp_listed = true;
        a.loadout.requests = 100;
        a.cached_rate = Some(1e-6);
        for s in servers {
            a.loadout.mcp.insert(s.to_string(), 550);
        }
        a
    }

    fn sessions(cwd: &str, n: usize, servers: &[&str]) -> Vec<Analysis> {
        (0..n)
            .map(|i| offered(cwd, &format!("2026-09-{:02}T00:00:00Z", i + 1), servers))
            .collect()
    }

    fn found(sessions: &[Analysis], inv: &Inventory) -> Vec<Finding> {
        let refs: Vec<&Analysis> = sessions.iter().collect();
        never_used(&refs, inv)
    }

    const ACCOUNT: &str = r#"{
        "mcpServers": {
            "tracker": {"command": "npx", "env": {"TRACKER_TOKEN": "sk-live-SECRET123"}},
            "search": {"type": "http", "url": "https://x", "headers": {"Authorization": "Bearer SECRET456"}}
        }
    }"#;

    #[test]
    fn a_server_offered_often_and_never_called_is_a_fix_naming_its_file() {
        let m = Machine::new();
        m.write("home/.claude.json", ACCOUNT);
        let s = sessions(&m.project(), 6, &["tracker"]);
        let f = found(&s, &m.inventory(&s));
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].class, Class::Fix);
        assert_eq!(f[0].sessions, 6);
        assert!(f[0].remedy.contains("`tracker`"));
        assert!(f[0].remedy.contains(".claude.json"), "{}", f[0].remedy);
        // 550 chars / 2.75 = 200 tokens, × 100 requests × 6 sessions.
        assert_eq!(f[0].tokens, 120_000);
        assert!((f[0].usd - 0.12).abs() < 1e-9);
        assert_eq!(f[0].basis, Basis::Estimated);
    }

    #[test]
    fn a_server_called_once_anywhere_in_its_scope_is_not() {
        let m = Machine::new();
        m.write("home/.claude.json", ACCOUNT);
        let mut s = sessions(&m.project(), 6, &["tracker"]);
        // Used from a different project: a user-wide server is one server.
        let mut elsewhere = offered("/elsewhere", "2026-08-01T00:00:00Z", &["tracker"]);
        elsewhere.used_mcp.insert("tracker".into());
        s.push(elsewhere);
        assert!(found(&s, &m.inventory(&s)).is_empty());
    }

    #[test]
    fn a_server_read_only_through_its_resources_counts_as_used() {
        let m = Machine::new();
        m.write("home/.claude.json", ACCOUNT);
        let mut s = sessions(&m.project(), 6, &["tracker"]);
        // What `analyse` does with a resource read.
        s[0].used_mcp.insert("tracker".into());
        assert!(found(&s, &m.inventory(&s)).is_empty());
    }

    #[test]
    fn too_few_sessions_is_not_a_verdict() {
        let m = Machine::new();
        m.write("home/.claude.json", ACCOUNT);
        let s = sessions(&m.project(), MIN_SESSIONS - 1, &["tracker"]);
        assert!(found(&s, &m.inventory(&s)).is_empty());
    }

    /// Configured is not offered. A server that only reached the last two
    /// transcripts has been judged by two sessions, however many came before.
    #[test]
    fn sessions_that_were_never_offered_it_do_not_count() {
        let m = Machine::new();
        m.write("home/.claude.json", ACCOUNT);
        let mut s = sessions(&m.project(), 8, &[]);
        s.extend(sessions(&m.project(), 2, &["tracker"]));
        assert!(found(&s, &m.inventory(&s)).is_empty());
    }

    #[test]
    fn a_server_disabled_since_is_not_reported_from_old_transcripts() {
        let m = Machine::new();
        let project = m.project();
        m.write(
            "home/.claude.json",
            &format!(
                r#"{{"mcpServers": {{"tracker": {{"command": "x"}}}},
                    "projects": {{"{project}": {{"disabledMcpServers": ["tracker"]}}}}}}"#
            ),
        );
        let s = sessions(&project, 6, &["tracker"]);
        assert!(found(&s, &m.inventory(&s)).is_empty());
    }

    #[test]
    fn a_project_server_switched_off_in_local_settings_is_not_reported() {
        let m = Machine::new();
        m.write(
            "work/app/.mcp.json",
            r#"{"mcpServers": {"db": {"command": "x"}}}"#,
        );
        m.write(
            "work/app/.claude/settings.local.json",
            r#"{"disabledMcpjsonServers": ["db"]}"#,
        );
        let s = sessions(&m.project(), 6, &["db"]);
        assert!(found(&s, &m.inventory(&s)).is_empty());
    }

    /// A server removed from the configuration is not reported from the
    /// transcripts of before, and neither is anything cctop cannot trace to a
    /// file: there is nothing to point the reader at.
    #[test]
    fn a_server_no_file_defines_is_not_reported() {
        let m = Machine::new();
        m.write("home/.claude.json", "{}");
        let s = sessions(&m.project(), 6, &["tracker"]);
        assert!(found(&s, &m.inventory(&s)).is_empty());
    }

    #[test]
    fn user_and_project_servers_are_told_apart() {
        let m = Machine::new();
        m.write("home/.claude.json", ACCOUNT);
        m.write(
            "work/app/.mcp.json",
            r#"{"mcpServers": {"db": {"command": "x"}}}"#,
        );
        let mut s = sessions(&m.project(), 6, &["tracker", "db"]);
        // The project server in another directory is somebody else's file.
        s.extend(sessions("/elsewhere", 6, &["tracker"]));
        let f = found(&s, &m.inventory(&s));
        assert_eq!(f.len(), 2, "{f:?}");
        let user = f.iter().find(|f| f.remedy.contains("`tracker`")).unwrap();
        let project = f.iter().find(|f| f.remedy.contains("`db`")).unwrap();
        assert_eq!(user.sessions, 12, "everywhere it was offered");
        assert_eq!(project.sessions, 6, "only where the project is");
        assert!(
            project.remedy.contains("work/app/.mcp.json"),
            "{}",
            project.remedy
        );
        assert!(project.remedy.contains("settings.local.json"));
    }

    /// A local server shadows a user one of the same name, which is the file a
    /// remedy has to name.
    #[test]
    fn a_local_definition_wins_over_the_users() {
        let m = Machine::new();
        let project = m.project();
        m.write(
            "home/.claude.json",
            &format!(
                r#"{{"mcpServers": {{"tracker": {{"command": "x"}}}},
                    "projects": {{"{project}": {{"mcpServers": {{"tracker": {{"command": "y"}}}}}}}}}}"#
            ),
        );
        let s = sessions(&project, 6, &["tracker"]);
        let f = found(&s, &m.inventory(&s));
        assert_eq!(f.len(), 1);
        assert!(f[0].remedy.contains("-s local"), "{}", f[0].remedy);
    }

    #[test]
    fn a_connector_dropped_from_the_newest_session_has_been_dealt_with() {
        let m = Machine::new();
        let mut s = sessions(&m.project(), 6, &["claude_ai_Gmail"]);
        let f = found(&s, &m.inventory(&s));
        assert_eq!(f.len(), 1);
        assert!(f[0].remedy.contains("/mcp"));
        s.push(offered(&m.project(), "2026-10-01T00:00:00Z", &[]));
        assert!(found(&s, &m.inventory(&s)).is_empty());
    }

    /// The configuration can hold an API key in `env` and a bearer token in
    /// `headers`. Only names and paths may ever reach the output.
    #[test]
    fn secrets_never_reach_a_finding() {
        let m = Machine::new();
        m.write("home/.claude.json", ACCOUNT);
        let s = sessions(&m.project(), 6, &["tracker", "search"]);
        let f = found(&s, &m.inventory(&s));
        assert!(!f.is_empty());
        for finding in &f {
            let text = format!("{} {}", finding.title, finding.remedy);
            for secret in [
                "SECRET",
                "sk-live",
                "Bearer",
                "TRACKER_TOKEN",
                "https://x",
                "npx",
            ] {
                assert!(!text.contains(secret), "{secret} leaked: {text}");
            }
        }
    }

    fn with_skills(cwd: &str, n: usize, skills: &[&str]) -> Vec<Analysis> {
        let mut s = sessions(cwd, n, &[]);
        for a in &mut s {
            a.loadout.skills_listed = true;
            for k in skills {
                a.loadout.skills.insert(k.to_string(), 275);
            }
        }
        s
    }

    #[test]
    fn a_skill_used_by_slash_tool_or_by_reading_it_is_used() {
        let m = Machine::new();
        for name in ["alpha", "beta", "gamma", "delta"] {
            m.write(
                &format!("home/.claude/skills/{name}/SKILL.md"),
                &format!("---\nname: {name}\ndescription: d\n---\n"),
            );
        }
        let mut s = with_skills(&m.project(), 6, &["alpha", "beta", "gamma", "delta"]);
        s[0].used_skills.insert("alpha".into());
        s[1].used_skills.insert("beta".into());
        s[2].read_paths.insert(
            m.path("home/.claude/skills/gamma/SKILL.md")
                .to_string_lossy()
                .into(),
        );
        let f = found(&s, &m.inventory(&s));
        assert_eq!(f.len(), 1);
        assert!(
            f[0].title.starts_with("1 skill never used"),
            "{}",
            f[0].title
        );
        assert!(f[0].remedy.contains("`delta`") && !f[0].remedy.contains("`alpha`"));
    }

    /// Claude Code's own skills are in every listing and in no directory
    /// cctop can point at; they are not the user's to remove.
    #[test]
    fn a_built_in_skill_is_never_reported() {
        let m = Machine::new();
        let s = with_skills(&m.project(), 6, &["update-config", "loop"]);
        assert!(found(&s, &m.inventory(&s)).is_empty());
    }

    #[test]
    fn a_plugin_is_reported_whole_or_not_at_all() {
        let m = Machine::new();
        m.write(
            "home/.claude/plugins/installed_plugins.json",
            r#"{"plugins": {"design@market": [{"scope": "user"}]}}"#,
        );
        let mut s = with_skills(&m.project(), 6, &["design:critique", "design:copy"]);
        let f = found(&s, &m.inventory(&s));
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].title.contains("Plugin `design`"));
        assert!(f[0].remedy.contains("design@market"));

        // One of its skills in use, and the plugin cannot be removed in part.
        s[0].used_skills.insert("design:copy".into());
        assert!(found(&s, &m.inventory(&s)).is_empty());
    }

    #[test]
    fn a_disabled_plugin_is_not_reported() {
        let m = Machine::new();
        m.write(
            "home/.claude/plugins/installed_plugins.json",
            r#"{"plugins": {"design@market": [{"scope": "user"}]}}"#,
        );
        m.write(
            "home/.claude/settings.json",
            r#"{"enabledPlugins": {"design@market": false}}"#,
        );
        let s = with_skills(&m.project(), 6, &["design:critique"]);
        assert!(found(&s, &m.inventory(&s)).is_empty());
    }

    #[test]
    fn an_unused_agent_names_its_directory() {
        let m = Machine::new();
        m.write(
            "work/app/.claude/agents/reviewer.md",
            "---\nname: reviewer\n---\n",
        );
        let mut s = sessions(&m.project(), 6, &[]);
        for a in &mut s {
            a.loadout.agents_listed = true;
            a.loadout.agents.insert("reviewer".into(), 300);
        }
        let f = found(&s, &m.inventory(&s));
        assert_eq!(f.len(), 1);
        assert!(
            f[0].remedy.contains("work/app/.claude/agents"),
            "{}",
            f[0].remedy
        );

        s[3].used_agents.insert("reviewer".into());
        assert!(found(&s, &m.inventory(&s)).is_empty());
    }

    /// Each server costs cents here. They are still detected, and a report
    /// that hides them says how many it hid and what they came to.
    #[test]
    fn small_unused_servers_land_below_the_floor_and_are_counted() {
        let m = Machine::new();
        m.write("home/.claude.json", ACCOUNT);
        let s = sessions(&m.project(), 6, &["tracker"]);
        let refs: Vec<&Analysis> = s.iter().collect();
        let inv = m.inventory(&s);
        let (kept, below) = crate::insight::optimize::triage_with(&refs, |_| Some(inv));
        assert!(!kept.iter().any(|f| f.remedy.contains("`tracker`")));
        let hidden = below
            .iter()
            .find(|f| f.remedy.contains("`tracker`"))
            .expect("detected, and accounted for below the bar");
        assert!(hidden.usd > 0.0 && hidden.usd < 5.0);
    }

    /// The same finding, priced high enough to clear the bar.
    #[test]
    fn an_expensive_unused_server_clears_the_floor() {
        let m = Machine::new();
        m.write("home/.claude.json", ACCOUNT);
        let mut s = sessions(&m.project(), 6, &["tracker"]);
        for a in &mut s {
            a.loadout.requests = 10_000;
            a.cached_rate = Some(5e-6);
        }
        let refs: Vec<&Analysis> = s.iter().collect();
        let inv = m.inventory(&s);
        let (kept, _) = crate::insight::optimize::triage_with(&refs, |_| Some(inv));
        assert!(kept.iter().any(|f| f.remedy.contains("`tracker`")));
    }

    #[test]
    fn memory_is_a_price_not_an_accusation() {
        let mut s = sessions("/work/app", 3, &[]);
        for a in &mut s {
            a.loadout.memory.insert("/work/app/CLAUDE.md".into(), 2_750);
        }
        // A worktree's copy is the same file to edit.
        s[2].loadout.memory.clear();
        s[2].loadout.memory.insert(
            "/work/app/.claude/worktrees/agent-x/CLAUDE.md".into(),
            2_750,
        );
        let refs: Vec<&Analysis> = s.iter().collect();
        let f = memory(&refs);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].class, Class::Note);
        assert!(f[0].title.starts_with("1 memory file"), "{}", f[0].title);
        assert_eq!(f[0].sessions, 3);
        // 1,000 tokens × 100 requests × 3 sessions at $1/M.
        assert!((f[0].usd - 0.3).abs() < 1e-9);
        assert!(f[0].remedy.contains("not waste"));
    }

    #[test]
    fn only_the_part_of_a_memory_file_past_the_harness_warning_is_a_saving() {
        let mut s = sessions("/work/app", 2, &[]);
        for a in &mut s {
            a.loadout
                .memory
                .insert("/work/app/CLAUDE.md".into(), 80_000);
        }
        let refs: Vec<&Analysis> = s.iter().collect();
        let f = memory(&refs);
        let habit = f
            .iter()
            .find(|f| f.class == Class::Habit)
            .expect("past 40k");
        let note = f.iter().find(|f| f.class == Class::Note).unwrap();
        assert!(
            (habit.usd - note.usd / 2.0).abs() < 1e-9,
            "half of it is past the line"
        );
    }

    #[test]
    fn a_listing_is_read_from_the_transcript_shape() {
        let mut l = crate::session::Loadout::default();
        l.note(&serde_json::json!({
            "type": "skill_listing",
            "content": "- run-cctop: Build and drive it.\n  More about it.\n- design:ux-copy: Words.",
        }));
        l.note(&serde_json::json!({
            "type": "agent_listing_delta",
            "addedLines": ["- Explore: built in", "- reviewer: mine"],
            "builtInTypes": ["Explore"],
        }));
        l.note(&serde_json::json!({
            "type": "deferred_tools_delta",
            "addedNames": ["WebFetch", "mcp__claude_ai_Gmail__send", "mcp__claude_ai_Gmail__read"],
        }));
        l.note(&serde_json::json!({
            "type": "mcp_instructions_delta",
            "addedNames": ["claude.ai Gmail"],
            "addedBlocks": ["## Gmail\nUse it."],
        }));
        assert_eq!(l.skills.len(), 2);
        assert_eq!(
            l.skills["run-cctop"],
            33 + 17,
            "a continuation line is its entry's"
        );
        assert!(l.skills.contains_key("design:ux-copy"));
        assert_eq!(l.agents.keys().collect::<Vec<_>>(), ["reviewer"]);
        let names = "mcp__claude_ai_Gmail__send".len() + "mcp__claude_ai_Gmail__read".len() + 2;
        assert_eq!(
            l.mcp["claude_ai_Gmail"],
            (names + "## Gmail\nUse it.".len()) as u64
        );
        assert!(l.listed());
    }
}
