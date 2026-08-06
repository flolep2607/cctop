//! OS process attribution: map running agent processes onto sessions.
//!
//! The Node original shelled out to `ps`, then `lsof` in PID chunks, then
//! PowerShell on Windows, re-parsing text output twice a second. `sysinfo`
//! exposes the same facts (parent, cmdline, cwd, start time, CPU, RSS) as typed
//! data on every platform, so all of that goes away.

use crate::session::Session;
use crate::util;
use std::collections::{HashMap, HashSet};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System, UpdateKind};

/// Cycles an exited child stays visible before being dropped from the list.
/// Without this, short-lived tool subprocesses flicker in and out.
const PROC_LINGER_TICKS: u8 = 3;

#[derive(Debug, Clone)]
pub struct ProcEntry {
    pub pid: u32,
    pub cpu: f32,
    pub memory: u64,
    pub args: String,
    pub is_root: bool,
    /// Recently exited; retained briefly so the list doesn't flicker.
    pub ghost: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ProcInfo {
    pub pids: usize,
    pub cpu: f32,
    pub memory: u64,
    pub command: String,
    pub process_list: Vec<ProcEntry>,
}

/// Gracefully stop an agent root process by PID.
///
/// The process table is refreshed immediately before sending the signal, so a
/// PID that exited after the confirmation dialog is never treated as a success.
pub fn terminate(pid: u32) -> Result<(), String> {
    let mut sys = System::new();
    let process_pid = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[process_pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    let process = sys
        .process(process_pid)
        .ok_or_else(|| format!("process {pid} has already exited"))?;
    match process.kill_with(Signal::Term) {
        Some(true) => Ok(()),
        Some(false) => Err(format!("permission denied stopping process {pid}")),
        None => Err("graceful termination is not supported on this platform".into()),
    }
}

/// A running agent with no matching transcript yet.
#[derive(Debug, Clone)]
pub struct Orphan {
    pub provider: crate::pricing::Provider,
    pub cwd: String,
}

pub struct Collector {
    sys: System,
    /// Session key -> pid -> (entry, remaining linger ticks).
    ghosts: HashMap<String, HashMap<u32, (ProcEntry, u8)>>,
    orphans: HashMap<String, Orphan>,
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

/// Background daemons that are not interactive sessions.
fn is_daemon(args: &str) -> bool {
    args.split_whitespace().any(|tok| {
        matches!(tok, "app-server" | "server" | "daemon")
            || tok.ends_with("/app-server")
            || tok.ends_with("/daemon")
    })
}

/// macOS `.app` bundle paths, excluding the Claude Code binary that Claude for
/// Mac ships inside one.
fn is_app_bundle(args: &str) -> bool {
    (args.contains(".app/") || args.contains(".app\\")) && !args.contains("/claude-code/")
}

fn basename(s: &str) -> &str {
    s.rsplit(['/', '\\']).next().unwrap_or(s)
}

/// Strip a path and any `.js`/`.exe` suffix to get a comparable command name.
fn command_stem(s: &str) -> String {
    let base = basename(s);
    base.strip_suffix(".js")
        .or_else(|| base.strip_suffix(".exe"))
        .unwrap_or(base)
        .to_ascii_lowercase()
}

/// Detect a Node process whose *script argument* is `codex`.
///
/// Checking only that "codex" appears somewhere in the command line would match
/// any process with `~/.codex/...` in its arguments.
fn is_node_hosted_codex(tokens: &[String]) -> bool {
    is_node_hosted_agent(tokens, "codex")
}

fn is_codex_process(name: &str, tokens: &[String]) -> bool {
    is_codex_binary(name)
        || is_sandboxed_codex_launcher(name, tokens)
        // The managed sandbox can expose only its argv[0] as the process name;
        // `app-server` is the Codex-specific proof that this is the agent
        // root, rather than a generic sandbox helper.
        || (name == "codex-linux-sandbox" && tokens.iter().any(|t| t == "app-server"))
        || (name == "node" && is_node_hosted_codex(tokens))
}

fn is_sandboxed_codex_launcher(name: &str, tokens: &[String]) -> bool {
    if !matches!(name, "bwrap" | "codex-linux-sandbox") {
        return false;
    }
    let launches_codex = tokens
        .iter()
        .any(|token| is_codex_binary(&command_stem(token)));
    launches_codex
        && (flag_value(tokens, "--command-cwd").is_some()
            || flag_value(tokens, "--sandbox-policy-cwd").is_some())
}

/// Codex release archives keep the target triple in the executable name when
/// users run them without renaming (for example
/// `codex-x86_64-unknown-linux-musl`). Do not use a broad `codex-` prefix here:
/// helper processes such as `codex-linux-sandbox` are descendants, not agent
/// roots in their own right.
fn is_codex_binary(name: &str) -> bool {
    name == "codex"
        || (name.starts_with("codex-")
            && (name.contains("-unknown-linux-")
                || name.ends_with("-apple-darwin")
                || name.ends_with("-pc-windows-msvc")))
}

/// Generic agent daemons do not represent an interactive session, but Codex
/// editor integrations run active sessions through `codex app-server`. Those
/// processes have no UUID argument, so they must reach the cwd-based fallback
/// below instead of being discarded as daemons up front.
fn exclude_agent_process(name: &str, tokens: &[String], args: &str) -> bool {
    is_app_bundle(args) || (is_daemon(args) && !is_codex_process(name, tokens))
}

fn is_node_hosted_agent(tokens: &[String], agent: &str) -> bool {
    tokens
        .iter()
        .skip(1)
        .take(3)
        .find(|t| !t.starts_with('-'))
        .is_some_and(|script| command_stem(script) == agent)
}

/// Claude Code is not always installed as a file called `claude`.
///
/// Both the native installer (`~/.local/share/claude/versions/<version>`) and
/// the remote/web harness (`~/.claude/remote/ccd-cli/<version>`) ship it as a
/// version-named binary, so the executable name is something like `2.1.222`.
/// Since the name is derived from the executable path, it never says `claude`
/// for those installs and the process would be discarded as unrelated.
///
/// `argv[0]` still says `claude`, so trust it first and only fall back to
/// matching the install path for launchers that exec by absolute path.
fn is_claude_binary(name: &str, tokens: &[String]) -> bool {
    if name == "claude" {
        return true;
    }
    tokens
        .first()
        .is_some_and(|argv0| command_stem(argv0) == "claude")
        || tokens.first().is_some_and(|p| {
            p.contains("/.claude/remote/ccd-cli/")
                || p.contains("\\.claude\\remote\\ccd-cli\\")
                || p.contains("/claude/versions/")
                || p.contains("\\claude\\versions\\")
        })
}

/// Locate a `resume` argument and return its value plus the index it came from.
///
/// Both spellings occur in the wild: `--resume VALUE` as separate tokens, and
/// `--resume=VALUE` as one. Handling only the first form silently loses every
/// session started by a harness that uses the second.
fn resume_value(tokens: &[String]) -> Option<(&str, usize)> {
    for (i, t) in tokens.iter().enumerate() {
        if let Some(v) = t.strip_prefix("--resume=") {
            return Some((v, i));
        }
        if t == "resume" || t == "--resume" {
            return tokens.get(i + 1).map(|v| (v.as_str(), i + 1));
        }
    }
    None
}

/// The session UUID a process was resumed with, if it was resumed by UUID.
fn resume_uuid(tokens: &[String]) -> Option<&str> {
    let (value, _) = resume_value(tokens)?;
    crate::config::is_full_uuid(value).then_some(value)
}

fn session_value(tokens: &[String]) -> Option<&str> {
    for (i, token) in tokens.iter().enumerate() {
        if let Some(value) = token.strip_prefix("--session=") {
            return (!value.is_empty()).then_some(value);
        }
        if let Some(value) = token.strip_prefix("-s=") {
            return (!value.is_empty()).then_some(value);
        }
        if token == "--session" || token == "-s" {
            return tokens.get(i + 1).map(String::as_str);
        }
    }
    None
}

fn flag_value<'a>(tokens: &'a [String], flag: &str) -> Option<&'a str> {
    for (i, token) in tokens.iter().enumerate() {
        if let Some(value) = token.strip_prefix(&format!("{flag}=")) {
            return (!value.is_empty()).then_some(value);
        }
        if token == flag {
            return tokens.get(i + 1).map(String::as_str);
        }
    }
    None
}

/// The free-text title a process was resumed with (Claude for Mac form).
///
/// Titles can contain spaces, so for the separate-token spelling everything
/// after the flag belongs to the title.
fn resume_title(tokens: &[String]) -> Option<String> {
    let (value, idx) = resume_value(tokens)?;
    if tokens.get(idx).is_some_and(|t| t.starts_with("--resume=")) {
        return (!value.is_empty()).then(|| value.to_string());
    }
    let rest = tokens.get(idx..)?;
    (!rest.is_empty()).then(|| rest.join(" ").trim().to_string())
}

impl Collector {
    pub fn new() -> Self {
        Collector {
            sys: System::new(),
            ghosts: HashMap::new(),
            orphans: HashMap::new(),
        }
    }

    pub fn orphans(&self) -> &HashMap<String, Orphan> {
        &self.orphans
    }

    /// Aggregate CPU/memory per session, keyed by `provider:session_id`.
    pub fn collect(&mut self, sessions: &[Session]) -> HashMap<String, ProcInfo> {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cmd(UpdateKind::Always)
                .with_cwd(UpdateKind::Always)
                .with_exe(UpdateKind::Always)
                .with_memory()
                .with_cpu()
                .without_tasks(),
        );
        self.orphans.clear();

        // Snapshot into plain data so we're not holding borrows on `self.sys`.
        struct Snap {
            ppid: u32,
            args: String,
            tokens: Vec<String>,
            name: String,
            cwd: String,
            cpu: f32,
            memory: u64,
            /// Seconds since the epoch, used to pair concurrent processes in one
            /// directory with the sessions they most plausibly started.
            start_time: u64,
        }
        let snapshot: HashMap<u32, Snap> = self
            .sys
            .processes()
            .iter()
            // Threads share their process's command line, so every one of them
            // matches the same session. Left in, they compete to be picked as
            // the root — nondeterministically, since map order isn't stable —
            // and whichever thread wins reports its own CPU and no children.
            .filter(|(_, p)| p.thread_kind().is_none())
            .map(|(pid, p)| {
                let tokens: Vec<String> = p
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect();
                let args = tokens.join(" ");
                let name = p
                    .exe()
                    .map(|e| command_stem(&e.to_string_lossy()))
                    .filter(|n| !n.is_empty())
                    .or_else(|| tokens.first().map(|t| command_stem(t)))
                    .unwrap_or_else(|| command_stem(&p.name().to_string_lossy()));
                (
                    pid.as_u32(),
                    Snap {
                        ppid: p.parent().map(Pid::as_u32).unwrap_or(0),
                        args,
                        tokens,
                        name,
                        cwd: p
                            .cwd()
                            .map(|c| c.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        cpu: p.cpu_usage(),
                        memory: p.memory(),
                        start_time: p.start_time(),
                    },
                )
            })
            .collect();

        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for (pid, s) in &snapshot {
            children.entry(s.ppid).or_default().push(*pid);
        }

        // A cctop launched inside the managed Codex sandbox has a bwrap
        // ancestor whose command line contains the Codex launcher. Other
        // bwrap processes are short-lived tool sandboxes and must not affect
        // session liveness.
        let mut current_ancestors = HashSet::new();
        let mut current = Some(std::process::id());
        while let Some(pid) = current {
            if !current_ancestors.insert(pid) {
                break;
            }
            current = snapshot
                .get(&pid)
                .and_then(|s| (s.ppid != 0).then_some(s.ppid));
        }

        // --- Identify agent root processes and attribute them to sessions ---
        let mut roots: HashMap<String, u32> = HashMap::new();
        let mut unmatched: Vec<(u32, crate::pricing::Provider)> = Vec::new();

        // Candidate sessions per working directory, most recently active first,
        // for the cwd-based fallback. A directory usually holds many finished
        // sessions and only the newest are plausibly live, but more than one can
        // be running at once, so keep them all and rank rather than collapsing
        // to a single winner.
        let mut cwd_index: HashMap<(crate::pricing::Provider, &str), Vec<&Session>> =
            HashMap::new();
        for s in sessions.iter().filter(|s| !s.label_source.is_empty()) {
            cwd_index
                .entry((s.provider, s.label_source.as_str()))
                .or_default()
                .push(s);
        }
        for candidates in cwd_index.values_mut() {
            candidates.sort_by(|a, b| {
                b.last_active
                    .cmp(&a.last_active)
                    .then_with(|| a.session_id.cmp(&b.session_id))
            });
        }

        for (&pid, snap) in &snapshot {
            if exclude_agent_process(&snap.name, &snap.tokens, &snap.args) {
                continue;
            }
            let is_claude = is_claude_binary(&snap.name, &snap.tokens);
            let is_codex = is_codex_process(&snap.name, &snap.tokens)
                && (snap.name != "bwrap" || current_ancestors.contains(&pid));
            let is_opencode = matches!(snap.name.as_str(), "opencode" | "opencode-cli")
                || (snap.name == "node" && is_node_hosted_agent(&snap.tokens, "opencode"));
            let is_pi = snap.name == "pi"
                || (snap.name == "node" && is_node_hosted_agent(&snap.tokens, "pi"));
            if !is_claude && !is_codex && !is_opencode && !is_pi {
                continue;
            }
            let provider = if is_opencode {
                crate::pricing::Provider::OpenCode
            } else if is_pi {
                crate::pricing::Provider::Pi
            } else if is_codex {
                crate::pricing::Provider::Codex
            } else {
                crate::pricing::Provider::Claude
            };

            if matches!(
                provider,
                crate::pricing::Provider::OpenCode | crate::pricing::Provider::Pi
            ) && let Some(id) = session_value(&snap.tokens)
            {
                claim_root(&mut roots, format!("{}:{id}", provider.as_str()), pid);
                continue;
            }

            if let Some(uuid) = resume_uuid(&snap.tokens) {
                claim_root(&mut roots, format!("{}:{}", provider.as_str(), uuid), pid);
                continue;
            }

            // Claude for Mac resumes by title rather than UUID.
            if is_claude
                && let Some(title) = resume_title(&snap.tokens)
                && let Some(session) = sessions
                    .iter()
                    .find(|s| s.surface.is_desktop() && s.title.as_deref() == Some(title.as_str()))
            {
                roots.entry(session.key()).or_insert(pid);
                continue;
            }

            unmatched.push((pid, provider));
        }

        // Fallback: match by working directory. Codex's app-server does not
        // carry a rollout UUID in its command line, so an unmatched Codex PID
        // cannot be safely attributed to a transcript. Do not manufacture a
        // running session for it; only a real rollout may own a Codex PID.
        // Resolve every unmatched PID to a directory first, then attribute each
        // directory's processes as a group. Handling them one at a time pointed
        // every process in a directory at that directory's newest session, so a
        // second concurrent agent in the same checkout always looked stopped.
        let mut by_cwd: HashMap<(crate::pricing::Provider, String), Vec<u32>> = HashMap::new();
        for (pid, provider) in unmatched {
            // The Codex worker is often a child of `codex-linux-sandbox`; the
            // child has no useful cwd, while the parent carries the managed
            // workspace in `--command-cwd`. Walk ancestors so the PID still
            // resolves to the rollout that owns that workspace.
            let mut current = Some(pid);
            let mut seen = HashSet::new();
            let cwd = loop {
                let Some(current_pid) = current else { break "" };
                if !seen.insert(current_pid) {
                    break "";
                }
                let Some(s) = snapshot.get(&current_pid) else {
                    break "";
                };
                if !s.cwd.is_empty() {
                    break s.cwd.as_str();
                }
                if let Some(value) = flag_value(&s.tokens, "--command-cwd")
                    .or_else(|| flag_value(&s.tokens, "--sandbox-policy-cwd"))
                {
                    break value;
                }
                current = (s.ppid != 0).then_some(s.ppid);
            };
            by_cwd
                .entry((provider, cwd.to_string()))
                .or_default()
                .push(pid);
        }

        // Map order is not stable, so fix it before attributing anything.
        let mut groups: Vec<((crate::pricing::Provider, String), Vec<u32>)> =
            by_cwd.into_iter().collect();
        groups.sort_by(|a, b| (a.0.0.as_str(), &a.0.1).cmp(&(b.0.0.as_str(), &b.0.1)));

        for ((provider, cwd), mut pids) in groups {
            // Oldest process first, so it pairs with the session that started
            // first and each keeps its own CPU and memory.
            pids.sort_by_key(|pid| (snapshot.get(pid).map_or(0, |s| s.start_time), *pid));

            let mut claimed = 0;
            if !cwd.is_empty()
                && let Some(candidates) = cwd_index.get(&(provider, cwd.as_str()))
            {
                let live = live_sessions_for_group(candidates, pids.len(), &roots);
                for (pid, session) in pids.iter().zip(&live) {
                    claim_root(&mut roots, session.key(), *pid);
                    claimed += 1;
                }
            }

            // Whatever is left has no transcript that can own it.
            for &pid in &pids[claimed..] {
                if provider == crate::pricing::Provider::Codex {
                    continue;
                }
                let key = format!("{}:_pid_{}", provider.as_str(), pid);
                claim_root(&mut roots, key.clone(), pid);
                self.orphans.insert(
                    key,
                    Orphan {
                        provider,
                        cwd: cwd.clone(),
                    },
                );
            }
        }

        // --- Aggregate each root's process subtree ---
        let mut result = HashMap::new();
        for (key, root_pid) in roots {
            let pids = descendants(root_pid, &children);
            let mut info = ProcInfo {
                command: snapshot
                    .get(&root_pid)
                    .map(|s| s.args.clone())
                    .unwrap_or_default(),
                ..Default::default()
            };
            for pid in &pids {
                let Some(snap) = snapshot.get(pid) else {
                    continue;
                };
                info.cpu += snap.cpu;
                info.memory += snap.memory;
                info.process_list.push(ProcEntry {
                    pid: *pid,
                    cpu: snap.cpu,
                    memory: snap.memory,
                    args: snap.args.clone(),
                    is_root: *pid == root_pid,
                    ghost: false,
                });
            }
            info.pids = pids.len();
            info.cpu = (info.cpu * 10.0).round() / 10.0;
            self.apply_linger(&key, &mut info);
            result.insert(key, info);
        }

        // Drop linger state for sessions that are no longer running at all.
        self.ghosts.retain(|k, _| result.contains_key(k));
        result
    }

    /// Re-inject recently exited children so the Processes panel doesn't flicker.
    fn apply_linger(&mut self, key: &str, info: &mut ProcInfo) {
        let cache = self.ghosts.entry(key.to_string()).or_default();
        let live: HashSet<u32> = info.process_list.iter().map(|p| p.pid).collect();

        for p in &info.process_list {
            cache.insert(p.pid, (p.clone(), PROC_LINGER_TICKS));
        }
        cache.retain(|pid, (entry, remaining)| {
            if live.contains(pid) {
                return true;
            }
            if *remaining == 0 {
                return false;
            }
            *remaining -= 1;
            let mut ghost = entry.clone();
            ghost.ghost = true;
            ghost.cpu = 0.0;
            info.process_list.push(ghost);
            true
        });
    }
}

/// Record a candidate root for a session, keeping the lowest PID.
///
/// Several live processes can legitimately claim one session (a resumed session
/// re-launched, or a wrapper that re-execs). Taking whichever the process map
/// happened to yield first made the choice vary between refreshes; the lowest
/// PID is stable and is virtually always the ancestor of the others.
/// Pick which of a directory's sessions are the live ones, ordered oldest first.
///
/// `candidates` must be ranked most-recently-active first: a directory
/// accumulates finished sessions and at most `process_count` of them can be
/// running. Sessions an exact UUID or title match already claimed are skipped,
/// since that attribution is stronger than a shared working directory.
///
/// The result is ordered by start time so the caller can pair it against
/// processes ordered the same way, keeping CPU and memory with the session that
/// actually incurred them.
fn live_sessions_for_group<'a>(
    candidates: &[&'a Session],
    process_count: usize,
    claimed: &HashMap<String, u32>,
) -> Vec<&'a Session> {
    let mut live: Vec<&Session> = candidates
        .iter()
        .copied()
        .filter(|s| !claimed.contains_key(&s.key()))
        .take(process_count)
        .collect();
    live.sort_by_key(|s| util::parse_ts(&s.started_at));
    live
}

fn claim_root(roots: &mut HashMap<String, u32>, key: String, pid: u32) {
    roots
        .entry(key)
        .and_modify(|existing| {
            if pid < *existing {
                *existing = pid;
            }
        })
        .or_insert(pid);
}

/// Breadth-first collection of a PID and all its descendants.
fn descendants(root: u32, children: &HashMap<u32, Vec<u32>>) -> Vec<u32> {
    let mut seen = HashSet::from([root]);
    let mut out = vec![root];
    let mut queue = vec![root];
    while let Some(pid) = queue.pop() {
        let Some(kids) = children.get(&pid) else {
            continue;
        };
        for &child in kids {
            if seen.insert(child) {
                out.push(child);
                queue.push(child);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    const UUID: &str = "7026d578-8cba-4880-b464-9700f1b77b71";

    fn session_at(id: &str, started_at: &str, last_active: &str) -> Session {
        let mut s = Session::new(crate::pricing::Provider::Claude, id.to_string());
        s.started_at = started_at.to_string();
        s.last_active = last_active.to_string();
        s
    }

    /// Regression: every process in a directory used to resolve to that
    /// directory's newest session, so a second agent running in the same
    /// checkout was reported as stopped.
    #[test]
    fn concurrent_sessions_in_one_directory_each_claim_a_process() {
        // Ranked most-recently-active first, as the cwd index provides them.
        let newest = session_at(
            "newest",
            "2026-08-05T12:00:00+00:00",
            "2026-08-05T15:00:00+00:00",
        );
        let older = session_at(
            "older",
            "2026-08-05T09:00:00+00:00",
            "2026-08-05T14:00:00+00:00",
        );
        let stale = session_at(
            "stale",
            "2026-01-01T00:00:00+00:00",
            "2026-01-01T00:00:00+00:00",
        );
        let candidates = vec![&newest, &older, &stale];

        // Two live processes: the two newest sessions are claimed, not just one.
        let live = live_sessions_for_group(&candidates, 2, &HashMap::new());
        let ids: Vec<&str> = live.iter().map(|s| s.session_id.as_str()).collect();
        // Oldest start first, so it pairs with the oldest process.
        assert_eq!(ids, vec!["older", "newest"]);

        // One process still resolves to the newest session alone, as before.
        let single = live_sessions_for_group(&candidates, 1, &HashMap::new());
        assert_eq!(
            single
                .iter()
                .map(|s| s.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["newest"]
        );

        // Nothing is attributed when no process is running in the directory.
        assert!(live_sessions_for_group(&candidates, 0, &HashMap::new()).is_empty());
    }

    /// An exact UUID or title match owns its session; a shared working directory
    /// must not steal it and leave the other process unattributed.
    #[test]
    fn exact_matches_are_not_reclaimed_by_directory_matching() {
        let newest = session_at(
            "newest",
            "2026-08-05T12:00:00+00:00",
            "2026-08-05T15:00:00+00:00",
        );
        let older = session_at(
            "older",
            "2026-08-05T09:00:00+00:00",
            "2026-08-05T14:00:00+00:00",
        );
        let candidates = vec![&newest, &older];

        let mut claimed = HashMap::new();
        claimed.insert(newest.key(), 4242);

        let live = live_sessions_for_group(&candidates, 1, &claimed);
        assert_eq!(
            live.iter()
                .map(|s| s.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["older"]
        );
    }

    #[test]
    fn resume_uuid_extraction() {
        assert_eq!(
            resume_uuid(&toks(&format!("claude --resume {UUID}"))),
            Some(UUID)
        );
        assert_eq!(resume_uuid(&toks("claude --resume My Session")), None);
        assert_eq!(resume_uuid(&toks("claude")), None);
    }

    /// Regression: the remote harness spells this `--resume=UUID`. Handling only
    /// the space-separated form loses every session it starts.
    #[test]
    fn resume_uuid_accepts_equals_form() {
        let t = toks(&format!(
            "/home/f/.claude/remote/ccd-cli/2.1.221 --verbose --resume={UUID} -"
        ));
        assert_eq!(resume_uuid(&t), Some(UUID));
    }

    /// Regression: the native installer lives at
    /// `~/.local/share/claude/versions/<version>`, so the executable name is a
    /// version string. Matching only on the name marked every locally
    /// installed session as stopped, including the one running cctop.
    #[test]
    fn version_named_claude_binaries_are_recognised() {
        // `name` comes from the exe path, so it is the version, not `claude`.
        assert!(is_claude_binary("2.1.222", &toks("claude")));
        assert!(is_claude_binary("2.1.222", &toks("claude --resume")));
        // Launchers that exec by absolute path lose the `claude` argv[0] too.
        assert!(is_claude_binary(
            "2.1.222",
            &toks("/home/f/.local/share/claude/versions/2.1.222")
        ));
        assert!(is_claude_binary(
            "2.1.221",
            &toks("/home/f/.claude/remote/ccd-cli/2.1.221 --verbose")
        ));
        // Unrelated version-named binaries must stay out.
        assert!(!is_claude_binary("2.1.222", &toks("/opt/other/2.1.222")));
        assert!(!is_claude_binary("node", &toks("node server.js")));
    }

    #[test]
    fn resume_title_joins_remaining_args() {
        assert_eq!(
            resume_title(&toks("claude --resume My Long Title")).as_deref(),
            Some("My Long Title")
        );
        // The `=` form carries the whole title in one token.
        assert_eq!(
            resume_title(&toks("claude --resume=Solo")).as_deref(),
            Some("Solo")
        );
    }

    /// Regression: Claude Code installs as a version-named binary under
    /// `~/.claude/remote/ccd-cli/`, so a name check alone never matches it.
    #[test]
    fn version_named_remote_binary_is_recognised() {
        let t = toks("/home/flo/.claude/remote/ccd-cli/2.1.221 --verbose");
        assert!(is_claude_binary(&command_stem(&t[0]), &t));
        assert!(is_claude_binary("claude", &toks("claude")));
        assert!(!is_claude_binary("node", &toks("node server.js")));
        // The remote *server* daemon lives elsewhere and must not match.
        assert!(!is_claude_binary(
            "server",
            &toks("/home/flo/.claude/remote/srv/abc/server --serve")
        ));
    }

    #[test]
    fn node_codex_requires_script_arg_not_stray_path() {
        assert!(is_node_hosted_codex(&toks("node /usr/lib/codex.js run")));
        // A path mentioning .codex must not count as the codex binary.
        assert!(!is_node_hosted_codex(&toks(
            "node /home/f/app.js --config /home/f/.codex/config.toml"
        )));
    }

    #[test]
    fn platform_named_codex_binaries_are_agent_roots() {
        assert!(is_codex_binary("codex"));
        assert!(is_codex_binary("codex-x86_64-unknown-linux-musl"));
        assert!(is_codex_binary("codex-aarch64-apple-darwin"));
        assert!(is_codex_binary("codex-x86_64-pc-windows-msvc"));
        // This is spawned by Codex and must remain part of the root's process
        // tree instead of competing with it for session attribution.
        assert!(!is_codex_binary("codex-linux-sandbox"));
    }

    #[test]
    fn sandbox_wrapper_with_codex_executable_is_an_agent_root() {
        let argv = toks("/home/f/.cursor-server/extensions/openai.chatgpt/bin/codex app-server");
        assert!(is_codex_process("codex-linux-sandbox", &argv));
        assert!(!is_codex_process(
            "codex-linux-sandbox",
            &toks("codex-linux-sandbox --helper")
        ));
    }

    #[test]
    fn codex_app_server_reaches_cwd_session_matching() {
        let native = toks("/opt/codex -c features.code_mode_host=true app-server");
        assert!(is_daemon(&native.join(" ")));
        assert!(!exclude_agent_process("codex", &native, &native.join(" ")));

        let claude = toks("claude app-server");
        assert!(exclude_agent_process("claude", &claude, &claude.join(" ")));
        let helper = toks("codex-code-mode-host");
        assert!(!is_codex_process("codex-code-mode-host", &helper));
    }

    #[test]
    fn node_hosted_agents_are_identified_by_script_name() {
        assert!(is_node_hosted_agent(
            &toks("node /usr/lib/opencode.js --session ses_1"),
            "opencode"
        ));
        assert!(is_node_hosted_agent(&toks("node /usr/lib/pi.js -c"), "pi"));
        assert!(!is_node_hosted_agent(
            &toks("node app.js --config /home/f/.pi/settings.json"),
            "pi"
        ));
    }

    #[test]
    fn session_flag_supports_opencode_and_pi_forms() {
        assert_eq!(
            session_value(&toks("opencode --session ses_123")),
            Some("ses_123")
        );
        assert_eq!(session_value(&toks("opencode -s=ses_123")), Some("ses_123"));
        assert_eq!(session_value(&toks("pi --session=abc123")), Some("abc123"));
    }

    #[test]
    fn sandbox_codex_app_server_uses_command_cwd() {
        let tokens = toks("codex-linux-sandbox --command-cwd /home/f/cctop app-server");
        assert!(is_codex_process("codex-linux-sandbox", &tokens));
        assert_eq!(flag_value(&tokens, "--command-cwd"), Some("/home/f/cctop"));
    }

    #[test]
    fn bwrap_codex_launcher_is_recognized_with_workspace_flags() {
        let tokens = toks(
            "codex-linux-sandbox -- /opt/codex --sandbox-policy-cwd /home/f/cctop --command-cwd /home/f/cctop app-server",
        );
        assert!(is_codex_process("codex-linux-sandbox", &tokens));
        assert!(is_codex_process("bwrap", &tokens));
    }

    #[test]
    fn daemons_and_bundles_excluded() {
        assert!(is_daemon("codex app-server"));
        assert!(!is_daemon("codex resume abc"));
        assert!(is_app_bundle(
            "/Applications/Claude.app/Contents/MacOS/Claude"
        ));
        // The bundled Claude Code binary is a real session process.
        assert!(!is_app_bundle(
            "/Users/x/claude-code/versions/1.2/claude.app/Contents/MacOS/claude"
        ));
    }

    #[test]
    fn command_stem_strips_path_and_extension() {
        assert_eq!(command_stem("/usr/local/bin/claude"), "claude");
        assert_eq!(command_stem("C:\\bin\\codex.exe"), "codex");
        assert_eq!(command_stem("codex.js"), "codex");
    }

    /// Regression: root choice must not depend on hash-map iteration order,
    /// or CPU and the process tree change shape between refreshes.
    #[test]
    fn root_claim_is_order_independent() {
        let mut a = HashMap::new();
        for pid in [900u32, 120, 4000] {
            claim_root(&mut a, "claude:x".into(), pid);
        }
        let mut b = HashMap::new();
        for pid in [4000u32, 900, 120] {
            claim_root(&mut b, "claude:x".into(), pid);
        }
        assert_eq!(a["claude:x"], 120);
        assert_eq!(a, b);
    }

    #[test]
    fn descendants_walks_full_subtree() {
        let children = HashMap::from([(1, vec![2, 3]), (2, vec![4]), (3, vec![]), (4, vec![5])]);
        let mut got = descendants(1, &children);
        got.sort();
        assert_eq!(got, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn descendants_survives_cycles() {
        // A malformed parent chain must not hang the collector.
        let children = HashMap::from([(1, vec![2]), (2, vec![1])]);
        let mut got = descendants(1, &children);
        got.sort();
        assert_eq!(got, vec![1, 2]);
    }
}
