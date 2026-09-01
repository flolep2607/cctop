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
    /// Why each root was given to the session it was given to, newest collect
    /// only. See [`Collector::attributions`].
    attributions: Vec<Attribution>,
}

/// One process, and the rule that decided which session owns it.
///
/// Recorded where the decision is made rather than re-derived by whatever wants
/// to explain it: a second copy of this reasoning is a second copy that can
/// disagree with the first, and the whole value of an explanation is that it is
/// the real one.
#[derive(Debug, Clone)]
pub struct Attribution {
    pub pid: u32,
    /// `provider:session_id`, or `provider:_pid_N` for a process no transcript
    /// claims.
    pub key: String,
    /// The command line, as far as it is worth printing.
    pub argv: String,
    /// The rule: which of the four ways this process found its session.
    pub rule: &'static str,
    /// What the rule matched on — a uuid, a title, a directory.
    pub matched: String,
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

fn tokens_of(p: &sysinfo::Process) -> Vec<String> {
    p.cmd()
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect()
}

/// The executable's comparable command name, if the kernel let us read it.
fn exe_stem(p: &sysinfo::Process) -> Option<String> {
    p.exe()
        .map(|e| command_stem(&e.to_string_lossy()))
        .filter(|n| !n.is_empty())
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
    name == "claude" || tokens.first().is_some_and(|argv0| is_claude_argv0(argv0))
}

fn is_claude_argv0(argv0: &str) -> bool {
    command_stem(argv0) == "claude"
        || argv0.contains("/.claude/remote/ccd-cli/")
        || argv0.contains("\\.claude\\remote\\ccd-cli\\")
        || argv0.contains("/claude/versions/")
        || argv0.contains("\\claude\\versions\\")
}

/// Cheap rejection test, run over every process on the machine before any
/// command line is copied out of `sysinfo`.
///
/// It must stay a strict superset of the per-provider tests below: a process it
/// rejects is never looked at again. Everything it needs is the executable stem
/// and `argv[0]`, both of which are already in hand.
fn could_be_agent(name: &str, argv0: Option<&str>) -> bool {
    matches!(
        name,
        // `node` hosts the JS-packaged builds of codex, opencode and pi.
        "claude" | "node" | "opencode" | "opencode-cli" | "pi"
        // Codex's sandbox launchers, which can be the agent root themselves.
        | "bwrap" | "codex-linux-sandbox"
    ) || is_codex_binary(name)
        || argv0.is_some_and(is_claude_argv0)
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
            attributions: Vec::new(),
        }
    }

    pub fn orphans(&self) -> &HashMap<String, Orphan> {
        &self.orphans
    }

    /// How the last [`collect`](Self::collect) matched processes to sessions.
    ///
    /// The answer to "why does cctop think this session is not running", which
    /// is not a question any other output can answer: a row shows the verdict
    /// and nothing about how it was reached. Read by `cctop why`.
    pub fn attributions(&self) -> &[Attribution] {
        &self.attributions
    }

    /// Aggregate CPU/memory per session, keyed by `provider:session_id`.
    pub fn collect(&mut self, sessions: &[Session]) -> HashMap<String, ProcInfo> {
        // A process's command line and executable never change for its lifetime
        // (an exec makes it a different process, which `sysinfo` notices by its
        // start time and re-reads in full), so re-reading `/proc/<pid>/cmdline`
        // and re-`readlink`ing `/proc/<pid>/exe` on every tick is pure waste.
        // A cwd genuinely can change, so that one stays unconditional.
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .with_cwd(UpdateKind::Always)
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_memory()
                .with_cpu()
                .without_tasks(),
        );
        self.orphans.clear();
        self.attributions.clear();
        // Filled as the decisions below are made, and moved onto `self` at the
        // end: `self` is borrowed immutably as `sys` for the length of this.
        let mut found: Vec<Attribution> = Vec::new();
        let sys = &self.sys;

        // Every process contributes its place in the tree and its resource
        // usage, and nothing else: copying out command lines for all of them is
        // what used to make this scale with the size of the machine rather than
        // with the number of agents. The few processes that turn out to matter
        // read theirs back out of `sys` further down.
        struct Proc {
            ppid: u32,
            cpu: f32,
            memory: u64,
            /// Seconds since the epoch, used to pair concurrent processes in one
            /// directory with the sessions they most plausibly started.
            start_time: u64,
        }
        /// A process that survived the cheap name filter, with the command line
        /// the provider tests need.
        struct Candidate {
            pid: u32,
            name: String,
            tokens: Vec<String>,
            args: String,
        }

        let mut procs: HashMap<u32, Proc> = HashMap::with_capacity(sys.processes().len());
        let mut candidates: Vec<Candidate> = Vec::new();
        for (pid, p) in sys.processes() {
            // Threads share their process's command line, so every one of them
            // matches the same session. Left in, they compete to be picked as
            // the root — nondeterministically, since map order isn't stable —
            // and whichever thread wins reports its own CPU and no children.
            if p.thread_kind().is_some() {
                continue;
            }
            let pid = pid.as_u32();
            procs.insert(
                pid,
                Proc {
                    ppid: p.parent().map(Pid::as_u32).unwrap_or(0),
                    cpu: p.cpu_usage(),
                    memory: p.memory(),
                    start_time: p.start_time(),
                },
            );

            let argv0 = p.cmd().first().map(|a| a.to_string_lossy());
            let name = exe_stem(p).unwrap_or_else(|| match &argv0 {
                Some(a) => command_stem(a),
                None => command_stem(&p.name().to_string_lossy()),
            });
            if !could_be_agent(&name, argv0.as_deref()) {
                continue;
            }
            let tokens = tokens_of(p);
            candidates.push(Candidate {
                pid,
                name,
                args: tokens.join(" "),
                tokens,
            });
        }
        // Map order is not stable, so fix it before anything claims a session:
        // `claim_root` and the title match below both prefer the lowest PID.
        candidates.sort_by_key(|c| c.pid);

        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for (pid, s) in &procs {
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
            current = procs
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

        // Sessions by the id a process would name to reach them, most recently
        // active first. A resume forks the transcript, so one id can lead to
        // several sessions — the original and every session resumed from it —
        // and the newest of those is the one the process is actually in.
        let mut launched_index: HashMap<(crate::pricing::Provider, &str), Vec<&Session>> =
            HashMap::new();
        for s in sessions.iter() {
            launched_index
                .entry((s.provider, s.launched_as()))
                .or_default()
                .push(s);
        }
        for candidates in launched_index.values_mut() {
            candidates.sort_by(|a, b| {
                b.last_active
                    .cmp(&a.last_active)
                    .then_with(|| a.session_id.cmp(&b.session_id))
            });
        }

        for snap in &candidates {
            let pid = snap.pid;
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
                let key = format!("{}:{id}", provider.as_str());
                found.push(Attribution {
                    pid,
                    key: key.clone(),
                    argv: snap.args.clone(),
                    rule: "--session <id> on the command line",
                    matched: id.to_string(),
                });
                claim_root(&mut roots, key, pid);
                continue;
            }

            if let Some(uuid) = resume_uuid(&snap.tokens) {
                let launched = launched_index.get(&(provider, uuid)).map(Vec::as_slice);
                let key = resumed_key(launched, provider, uuid, &roots);
                // The subtle one, and the reason this record exists: the key is
                // usually *not* the uuid on the command line. A resume forks.
                let rule = match key.ends_with(uuid) {
                    true => "--resume <id>, and that id's own transcript",
                    false => "--resume <id>, forwarded to the transcript it forked into",
                };
                found.push(Attribution {
                    pid,
                    key: key.clone(),
                    argv: snap.args.clone(),
                    rule,
                    matched: uuid.to_string(),
                });
                claim_root(&mut roots, key, pid);
                continue;
            }

            // Claude for Mac resumes by title rather than UUID.
            if is_claude
                && let Some(title) = resume_title(&snap.tokens)
                && let Some(session) = sessions
                    .iter()
                    .find(|s| s.surface.is_desktop() && s.title.as_deref() == Some(title.as_str()))
            {
                found.push(Attribution {
                    pid,
                    key: session.key(),
                    argv: snap.args.clone(),
                    rule: "Claude for Mac, matched by window title",
                    matched: title.to_string(),
                });
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
                let Some(current_pid) = current else {
                    break String::new();
                };
                if !seen.insert(current_pid) {
                    break String::new();
                }
                let (Some(s), Some(p)) = (
                    procs.get(&current_pid),
                    sys.process(Pid::from_u32(current_pid)),
                ) else {
                    break String::new();
                };
                if let Some(cwd) = p
                    .cwd()
                    .map(|c| c.to_string_lossy())
                    .filter(|c| !c.is_empty())
                {
                    break cwd.into_owned();
                }
                let tokens = tokens_of(p);
                if let Some(value) = flag_value(&tokens, "--command-cwd")
                    .or_else(|| flag_value(&tokens, "--sandbox-policy-cwd"))
                {
                    break value.to_string();
                }
                current = (s.ppid != 0).then_some(s.ppid);
            };
            by_cwd.entry((provider, cwd)).or_default().push(pid);
        }

        // Map order is not stable, so fix it before attributing anything.
        let mut groups: Vec<((crate::pricing::Provider, String), Vec<u32>)> =
            by_cwd.into_iter().collect();
        groups.sort_by(|a, b| (a.0.0.as_str(), &a.0.1).cmp(&(b.0.0.as_str(), &b.0.1)));

        for ((provider, cwd), mut pids) in groups {
            // Oldest process first, so it pairs with the session that started
            // first and each keeps its own CPU and memory.
            pids.sort_by_key(|pid| (procs.get(pid).map_or(0, |s| s.start_time), *pid));

            let mut claimed = 0;
            if !cwd.is_empty()
                && let Some(candidates) = cwd_index.get(&(provider, cwd.as_str()))
            {
                let live = live_sessions_for_group(candidates, pids.len(), &roots);
                for (pid, session) in pids.iter().zip(&live) {
                    found.push(Attribution {
                        pid: *pid,
                        key: session.key(),
                        argv: String::new(),
                        rule: "no id on the command line; matched by working directory",
                        matched: cwd.clone(),
                    });
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
                found.push(Attribution {
                    pid,
                    key: key.clone(),
                    argv: String::new(),
                    rule: "no transcript claims it; shown as a row of its own",
                    matched: cwd.clone(),
                });
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
        // Only here does a command line get copied for a non-agent process, and
        // only for the handful that are descendants of an agent root.
        let mut result = HashMap::new();
        for (key, root_pid) in roots {
            let pids = descendants(root_pid, &children);
            let mut info = ProcInfo::default();
            for pid in &pids {
                let (Some(snap), Some(p)) = (procs.get(pid), sys.process(Pid::from_u32(*pid)))
                else {
                    continue;
                };
                let args = tokens_of(p).join(" ");
                if *pid == root_pid {
                    info.command = args.clone();
                }
                info.cpu += snap.cpu;
                info.memory += snap.memory;
                info.process_list.push(ProcEntry {
                    pid: *pid,
                    cpu: snap.cpu,
                    memory: snap.memory,
                    args,
                    is_root: *pid == root_pid,
                    ghost: false,
                });
            }
            info.pids = pids.len();
            info.cpu = (info.cpu * 10.0).round() / 10.0;
            result.insert(key, info);
        }
        for (key, info) in &mut result {
            self.apply_linger(key, info);
        }

        // Drop linger state for sessions that are no longer running at all.
        self.ghosts.retain(|k, _| result.contains_key(k));
        found.sort_by_key(|a| a.pid);
        self.attributions = found;
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

/// The session a `--resume <uuid>` process belongs to.
///
/// Not the session called `uuid`, which is the trap this exists to avoid. A
/// resume does not reopen a transcript — it forks one: Claude Code writes a new
/// file under a new id and records the id it was launched from. The command line
/// therefore names a conversation that stopped the moment this process started,
/// and reading it literally hands the live agent to the dead transcript. The
/// running session then shows as stopped, its CPU and memory land on a row
/// nobody is in, and a `resume` of the real session sees nothing running and
/// offers to start a second agent on it.
///
/// So the answer is the newest unclaimed session that was *launched* as `uuid`
/// — the id's own session until it is resumed, and its continuation after that.
/// `candidates` must be ranked most-recently-active first.
///
/// `None` and an empty list both mean no transcript claims that id, which is a
/// real state: a session under a config directory cctop is not reading, or one
/// whose file has been deleted out from under a running agent. That keeps the
/// id itself as the key, so the process still counts as one running thing.
fn resumed_key(
    candidates: Option<&[&Session]>,
    provider: crate::pricing::Provider,
    uuid: &str,
    claimed: &HashMap<String, u32>,
) -> String {
    candidates
        .unwrap_or_default()
        .iter()
        .find(|s| !claimed.contains_key(&s.key()))
        .map(|s| s.key())
        .unwrap_or_else(|| format!("{}:{uuid}", provider.as_str()))
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

    /// A session that has never been resumed answers to its own id, which is
    /// the case that already worked and must keep working.
    #[test]
    fn a_resume_of_a_fresh_session_claims_that_session() {
        let s = session_at(UUID, "t0", "t1");
        let key = resumed_key(
            Some(&[&s]),
            crate::pricing::Provider::Claude,
            UUID,
            &HashMap::new(),
        );
        assert_eq!(key, format!("claude:{UUID}"));
    }

    /// The bug this is here for. `claude --resume X` forks: the agent runs on a
    /// new transcript that records X as where it came from, and X itself stops.
    /// Attributing the process to X marks the conversation nobody is in as
    /// working and the one being typed into as stopped.
    #[test]
    fn a_resume_claims_the_transcript_it_forked_into_not_the_one_it_names() {
        let mut original = session_at(UUID, "t0", "t1");
        original.launch_id = UUID.to_string();
        let mut forked = session_at("forked", "t2", "t3");
        forked.launch_id = UUID.to_string();

        // Ranked most-recently-active first, as the caller guarantees.
        let key = resumed_key(
            Some(&[&forked, &original]),
            crate::pricing::Provider::Claude,
            UUID,
            &HashMap::new(),
        );
        assert_eq!(key, "claude:forked");
    }

    /// Two agents can be resumed from one id. The second process takes the next
    /// session down the ranking rather than piling onto the first one's.
    #[test]
    fn a_second_process_on_one_launch_id_takes_the_next_session() {
        let mut original = session_at(UUID, "t0", "t1");
        original.launch_id = UUID.to_string();
        let mut forked = session_at("forked", "t2", "t3");
        forked.launch_id = UUID.to_string();
        let claimed = HashMap::from([("claude:forked".to_string(), 42)]);

        let key = resumed_key(
            Some(&[&forked, &original]),
            crate::pricing::Provider::Claude,
            UUID,
            &claimed,
        );
        assert_eq!(key, format!("claude:{UUID}"));
    }

    /// The rule `cctop why` prints has to be the rule that ran, so it is worth
    /// pinning that the forked case is spelled as the forked case. A resume
    /// that silently reported "that id's own transcript" while forwarding the
    /// process elsewhere would make the diagnostic lie in exactly the situation
    /// it exists for.
    #[test]
    fn a_forwarded_resume_is_labelled_as_forwarded() {
        let mut original = session_at(UUID, "t0", "t1");
        original.launch_id = UUID.to_string();
        let mut forked = session_at("forked", "t2", "t3");
        forked.launch_id = UUID.to_string();

        let key = resumed_key(
            Some(&[&forked, &original]),
            crate::pricing::Provider::Claude,
            UUID,
            &HashMap::new(),
        );
        // What `collect` branches on to choose the wording.
        assert!(
            !key.ends_with(UUID),
            "a forwarded resume must not look like a direct one: {key}"
        );

        let direct = resumed_key(
            Some(&[&original]),
            crate::pricing::Provider::Claude,
            UUID,
            &HashMap::new(),
        );
        assert!(direct.ends_with(UUID), "a direct resume must look direct");
    }

    /// A transcript cctop cannot see is still a process worth counting, so the
    /// id stays the key and the caller's orphan path picks it up.
    #[test]
    fn a_resume_of_a_transcript_cctop_cannot_see_keeps_the_id() {
        let key = resumed_key(
            None,
            crate::pricing::Provider::Claude,
            UUID,
            &HashMap::new(),
        );
        assert_eq!(key, format!("claude:{UUID}"));
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

    /// `could_be_agent` decides, for every process on the machine, whether its
    /// command line is worth copying at all. If it ever rejects something the
    /// provider tests would have accepted, that agent silently disappears from
    /// cctop — so pin every shape the tests below rely on.
    #[test]
    fn the_cheap_filter_admits_every_agent_shape() {
        let admits = |name: &str, argv: &str| {
            let tokens = toks(argv);
            could_be_agent(name, tokens.first().map(String::as_str))
        };
        // Claude, including the version-named native and remote installs whose
        // executable stem says nothing about Claude.
        assert!(admits("claude", "claude"));
        assert!(admits("2.1.222", "claude --resume"));
        assert!(admits(
            "2.1.222",
            "/home/f/.local/share/claude/versions/2.1.222"
        ));
        assert!(admits("2.1.221", "/home/f/.claude/remote/ccd-cli/2.1.221"));
        // Codex, its release-archive names and its sandbox launchers.
        assert!(admits("codex", "codex resume abc"));
        assert!(admits("codex-x86_64-unknown-linux-musl", ""));
        assert!(admits(
            "codex-linux-sandbox",
            "codex-linux-sandbox app-server"
        ));
        assert!(admits("bwrap", "bwrap -- /opt/codex app-server"));
        // opencode and pi, native and node-hosted.
        assert!(admits("opencode", "opencode"));
        assert!(admits("opencode-cli", "opencode-cli"));
        assert!(admits("pi", "pi"));
        assert!(admits("node", "node /usr/lib/opencode.js"));
        // Everything else on the machine stops here.
        assert!(!admits("firefox", "/usr/bin/firefox"));
        assert!(!admits("cargo", "cargo build"));
        assert!(!admits("2.1.222", "/opt/other/2.1.222"));
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
