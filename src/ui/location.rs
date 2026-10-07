//! Where the launcher starts an agent: this machine, or a directory on an ssh
//! host.
//!
//! A remote host is a *location*, not a kind of agent. The launcher's
//! directory field (`c`) takes `host:path` the way `scp` does, offers the hosts
//! in `~/.ssh/config` beside the local directories, and once a host is in the
//! field completes paths on it the way it completes local ones. Whichever agent
//! is then picked starts there — through `cctop sandbox` for an agent with a
//! way to run its commands on the host, over plain `ssh -t` for the shell — or
//! is shown greyed out with the reason it cannot.
//!
//! Completion needs the host to answer quickly, so the moment a host is in the
//! field an ssh ControlMaster is started for it on a background thread — never
//! prompting, see [`ssh_master`](crate::ssh_master) — and each directory is
//! listed over it once and remembered. The per-host state is a small machine:
//! connecting, then ready (with the host's home) or offline (with why), and
//! the field says which while it is not ready.

use super::*;
use crate::remote_fs::{self, Listing, RemoteFacts};
use crate::ssh_master::Runner;

/// What the field's text names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typed<'a> {
    /// A path on this machine, a bare word to match against known projects, or
    /// nothing.
    Local(&'a str),
    /// `host:path`, scp-style. An empty path is the host's home.
    Remote { host: &'a str, path: &'a str },
}

/// Read the field's text as a location.
///
/// `scp`'s rule, near enough: a colon before any slash makes the part before
/// it a host. Anything that starts like a path — `/`, `~`, `.` — is local
/// whatever colons follow, since a directory name may have one and a host name
/// may not start that way. An IPv6 address is written `[addr]:path`.
pub fn parse(text: &str) -> Typed<'_> {
    let text = text.trim();
    if text.is_empty() || text.starts_with(['/', '~', '.']) {
        return Typed::Local(text);
    }
    if text.starts_with('[') {
        return match text.find("]:") {
            Some(end) if end > 1 => Typed::Remote {
                host: &text[..=end],
                path: &text[end + 2..],
            },
            _ => Typed::Local(text),
        };
    }
    match text.split_once(':') {
        Some((host, path))
            if !host.is_empty() && !host.contains('/') && !host.contains(char::is_whitespace) =>
        {
            Typed::Remote { host, path }
        }
        _ => Typed::Local(text),
    }
}

/// Split a remote path being typed into the directory to list and the start of
/// a name in it. A path with no slash is a name in the home, as `scp host:x`
/// reads it.
fn split_remote(path: &str) -> (String, String) {
    match path.rfind('/') {
        None => ("~".to_string(), path.to_string()),
        Some(0) => ("/".to_string(), path[1..].to_string()),
        Some(at) => (path[..at].to_string(), path[at + 1..].to_string()),
    }
}

/// `name` inside `dir`, spelled the way the field spells paths.
fn join_remote(dir: &str, name: &str) -> String {
    match dir {
        "/" => format!("/{name}"),
        dir => format!("{}/{name}", dir.trim_end_matches('/')),
    }
}

/// One host's connection, as far as the launcher knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conn {
    /// The master is being started on a background thread.
    Connecting,
    /// Commands run there; `home` is the host's `$HOME`.
    Ready { home: String },
    /// It could not be reached without asking anything, and why. The launch
    /// tab can still get there: it has a terminal to ask on.
    Offline(String),
}

/// One remote directory's listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dir {
    Pending,
    Listed(Listing),
    /// The host says there is no such directory.
    Missing,
    Failed(String),
}

/// Everything known about one host this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostState {
    pub conn: Conn,
    /// The repository scan's answer, once; `None` until it lands. Asked once
    /// per host per run — it walks the home directory there.
    pub repos: Option<Vec<String>>,
    /// Listings by directory as typed (`~`, `~/src`, `/srv`), kept for the
    /// run: a directory seen once is completed again without a round trip.
    pub dirs: HashMap<String, Dir>,
}

impl HostState {
    fn connecting() -> HostState {
        HostState {
            conn: Conn::Connecting,
            repos: None,
            dirs: HashMap::new(),
        }
    }
}

/// One thing the field offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// A directory on this machine, which exists.
    Dir(std::path::PathBuf),
    /// An ssh host, offered as `name:` — taking it puts the host in the field
    /// and starts connecting.
    Host { name: String, aliases: Vec<String> },
    /// A directory on a host, and why it cannot be used if it cannot.
    Remote {
        host: String,
        path: String,
        problem: Option<String>,
    },
}

impl Hit {
    /// What the field holds once this is taken.
    pub fn text(&self) -> String {
        match self {
            Hit::Dir(dir) => crate::util::tildify(&dir.to_string_lossy()),
            Hit::Host { name, .. } => format!("{name}:"),
            Hit::Remote { host, path, .. } => format!("{host}:{path}"),
        }
    }

    /// What Tab fills in: the text, ready to go one level deeper.
    fn fill(&self) -> String {
        match self {
            Hit::Host { .. } => self.text(),
            _ => format!("{}/", self.text().trim_end_matches('/')),
        }
    }
}

/// Where a remote launch is going, as accepted from the field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTarget {
    pub host: String,
    /// As typed: `~` and `~/…` are the host's home.
    pub path: String,
    /// The directory as the host resolves it, when the host could be asked.
    pub resolved: Option<String>,
    pub facts: Option<RemoteFacts>,
    /// Why the sandbox could not use it, as of the check. Blocks the agents
    /// that mount; the shell, which mounts nothing, still goes.
    pub problem: Option<String>,
    /// Why it was taken without asking the host: offline, or not connected
    /// yet. The sandbox checks it in the tab instead.
    pub unchecked: Option<String>,
}

impl RemoteTarget {
    pub fn spelled(&self) -> String {
        format!("{}:{}", self.host, self.path)
    }
}

/// What the worker is asked to find out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    Connect(String),
    List { host: String, dir: String },
    Repos(String),
    Check { host: String, path: String },
}

/// What it found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Connected {
        host: String,
        result: Result<String, String>,
    },
    Listed {
        host: String,
        dir: String,
        result: Result<Option<Listing>, String>,
    },
    Repos {
        host: String,
        result: Result<Vec<String>, String>,
    },
    Checked {
        host: String,
        path: String,
        result: Result<RemoteFacts, String>,
    },
}

/// Answer one [`Ask`]. Blocking — up to a connect deadline or a listing
/// timeout — so the worker runs it on a thread of its own.
///
/// `connect` and `runner` are passed in so the tests can stand in for ssh.
pub fn answer(
    ask: Ask,
    runner: &dyn Runner,
    connect: &dyn Fn(&str) -> Result<(), String>,
) -> Answer {
    match ask {
        Ask::Connect(host) => {
            let result = connect(&host).and_then(|()| remote_fs::home(runner, &host));
            Answer::Connected { host, result }
        }
        Ask::List { host, dir } => {
            let result = remote_fs::list(runner, &host, &dir);
            Answer::Listed { host, dir, result }
        }
        Ask::Repos(host) => {
            let result = remote_fs::repos(runner, &host);
            Answer::Repos { host, result }
        }
        Ask::Check { host, path } => {
            let result = remote_fs::check(runner, &host, &path);
            Answer::Checked { host, path, result }
        }
    }
}

/// How the picked launcher row would reach a remote location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// Through `cctop sandbox --agent <agent>`.
    Sandbox(String),
    /// The login shell, over `ssh -t`: it mounts nothing and needs nothing.
    Shell(String),
    /// Not at all, and the sentence that says so.
    No(String),
}

/// Look the launcher row's command up: an agent goes by the sandbox's table,
/// the user's own shell goes over ssh, and anything else stays here.
pub fn reach_of(argv: &[String]) -> Reach {
    let label = tabs::label_of(argv);
    let name = label
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    if crate::alias::AGENTS.split_whitespace().any(|a| a == name) {
        return match crate::sandbox::reach(&name) {
            crate::sandbox::Reach::Local => Reach::No(crate::sandbox::local_only(&name)),
            _ => Reach::Sandbox(name),
        };
    }
    let shell = std::env::var("SHELL").unwrap_or_default();
    match argv {
        [only] if !shell.is_empty() && *only == shell => Reach::Shell(name),
        _ => Reach::No(crate::sandbox::local_only(&name)),
    }
}

/// What a remote tab is called: the agent, then where it works.
///
/// The arrow is the one the table's Host column puts after a sandboxed row's
/// host, so a tab and its row read as the same thing. The agent first, because
/// a tab's harness is read off the front of its name and that is what picks
/// which screen phrases mean "working" and "asking".
pub(super) fn remote_label(agent: &str, host: &str) -> String {
    format!("{agent} ⇄ {host}")
}

/// The command a sandboxed launch runs, as an argv for the pane.
///
/// Through `env`, which is how every launch here already carries a variable,
/// and which [`tabs::label_of`] knows to look past. The hold variable keeps a
/// setup failure on screen instead of closing the tab on it.
pub(super) fn sandbox_argv(
    exe: &std::path::Path,
    agent: &str,
    host: &str,
    path: &str,
) -> Vec<String> {
    let path = match path.trim() {
        "" => "~",
        path => path,
    };
    vec![
        "env".to_string(),
        format!("{}=1", crate::sandbox::ENV_HOLD),
        exe.to_string_lossy().into_owned(),
        "sandbox".to_string(),
        "--agent".to_string(),
        agent.to_string(),
        format!("{host}:{path}"),
    ]
}

/// The command a remote shell runs: ssh with a terminal, into the directory.
///
/// Over the launcher's master when it is up (`ControlMaster=auto` uses a live
/// socket and otherwise connects, prompting in the tab as ssh at a prompt
/// would). `~` is expanded there, by the host's `sh`.
pub(super) fn shell_argv(host: &str, path: &str) -> Vec<String> {
    use crate::sandbox::sh_quote;
    let script = r#"p=$1
case $p in "~"|"") p=$HOME ;; "~/"*) p=$HOME/${p#"~/"} ;; esac
cd -- "$p" || exit 1
exec "${SHELL:-sh}" -l"#;
    let mut argv = vec!["ssh".to_string(), "-t".to_string()];
    if let Some(socket) = crate::ssh_master::socket_for(host) {
        argv.extend([
            "-S".to_string(),
            socket.to_string_lossy().into_owned(),
            "-o".to_string(),
            "ControlMaster=auto".to_string(),
        ]);
    }
    argv.extend([
        host.to_string(),
        "--".to_string(),
        format!("exec sh -c {} cctop {}", sh_quote(script), sh_quote(path)),
    ]);
    argv
}

/// Hosts offered beside the local directories before one is in the field, at
/// most: enough to show that hosts are on offer, few enough to leave room for
/// the projects.
const HOSTS_SHOWN: usize = 2;

impl App {
    /// Start connecting to `host` unless that has begun already — or retry it,
    /// when `retry` and it was offline.
    pub(super) fn preconnect(&mut self, host: &str, retry: bool) {
        let fresh = match self.ssh_states.get(host) {
            None => true,
            Some(state) => retry && matches!(state.conn, Conn::Offline(_)),
        };
        if fresh {
            self.ssh_states
                .insert(host.to_string(), HostState::connecting());
            let _ = self
                .tx
                .send(worker::Request::Location(Ask::Connect(host.to_string())));
        }
    }

    /// The suggestions for `host:path`: what is listed under the directory
    /// being typed, and, for a bare name, the repositories the scan found.
    ///
    /// Asks for what it does not have yet — a connection, a listing — and
    /// offers what it has; the answer redraws the list when it lands.
    pub(super) fn remote_hits(&mut self, host: &str, path: &str) -> Vec<Hit> {
        self.preconnect(host, false);
        let Some(state) = self.ssh_states.get(host) else {
            return Vec::new();
        };
        let Conn::Ready { home } = &state.conn else {
            return Vec::new();
        };
        let home = home.clone();
        let (dir, fragment) = split_remote(path.trim());
        let mounts = remote_fs::mounts_here();
        let local = |abs: &str| {
            let at = std::path::Path::new(abs);
            remote_fs::verdict(at, &mounts, &remote_fs::here(at), None).err()
        };

        let mut hits: Vec<Hit> = Vec::new();
        // A bare name is matched against the repositories as well as the home's
        // own directories: what is remembered about a project is its name.
        if !path.contains('/')
            && let Some(repos) = &state.repos
        {
            let wanted = fragment.to_lowercase();
            hits.extend(
                repos
                    .iter()
                    .filter(|r| r.to_lowercase().contains(&wanted))
                    .map(|r| Hit::Remote {
                        host: host.to_string(),
                        path: r.clone(),
                        problem: local(&remote_fs::absolute(r, &home)),
                    }),
            );
        }
        match state.dirs.get(&dir) {
            Some(Dir::Listed(listing)) => {
                let wanted = fragment.to_lowercase();
                for entry in &listing.dirs {
                    if entry.name.starts_with('.') && !fragment.starts_with('.') {
                        continue;
                    }
                    if !entry.name.to_lowercase().starts_with(&wanted) {
                        continue;
                    }
                    let path = join_remote(&dir, &entry.name);
                    if hits
                        .iter()
                        .any(|h| matches!(h, Hit::Remote { path: p, .. } if *p == path))
                    {
                        continue;
                    }
                    let problem = match (entry.readable, entry.writable) {
                        (false, _) => Some("not readable on the host".to_string()),
                        (_, false) => Some("read-only on the host".to_string()),
                        _ => local(&join_remote(&listing.resolved, &entry.name)),
                    };
                    hits.push(Hit::Remote {
                        host: host.to_string(),
                        path,
                        problem,
                    });
                }
            }
            Some(_) => {}
            None => {
                if let Some(state) = self.ssh_states.get_mut(host) {
                    state.dirs.insert(dir.clone(), Dir::Pending);
                }
                let _ = self.tx.send(worker::Request::Location(Ask::List {
                    host: host.to_string(),
                    dir,
                }));
            }
        }
        hits.truncate(dirs::MAX_HITS);
        hits
    }

    /// The hosts matching a bare word, as hits.
    pub(super) fn host_hits(&self, word: &str) -> Vec<Hit> {
        let needle = word.trim().to_lowercase();
        self.ssh_hosts
            .iter()
            .filter(|h| h.matches(&needle))
            .map(|h| Hit::Host {
                name: h.name.clone(),
                aliases: h.aliases.clone(),
            })
            .collect()
    }

    /// Local suggestions with the matching hosts after them, sharing the
    /// field's few lines: hosts get at most [`HOSTS_SHOWN`] of them unless the
    /// directories leave more.
    pub(super) fn with_hosts(&self, local: Vec<Hit>, word: &str) -> Vec<Hit> {
        let hosts = self.host_hits(word);
        let reserve = hosts.len().min(HOSTS_SHOWN);
        let mut out: Vec<Hit> = local.into_iter().take(dirs::MAX_HITS - reserve).collect();
        let room = dirs::MAX_HITS - out.len();
        out.extend(hosts.into_iter().take(room));
        out
    }

    /// An answer from the worker: move the host's state on, and redraw the
    /// suggestions if the field is open and nothing in it is highlighted —
    /// the same rule [`App::got_repos`] keeps, so a highlight never moves.
    pub(super) fn got_location(&mut self, answer: Answer) {
        match answer {
            Answer::Connected { host, result } => {
                let Some(state) = self.ssh_states.get_mut(&host) else {
                    return;
                };
                match result {
                    Ok(home) => {
                        state.conn = Conn::Ready { home };
                        if state.repos.is_none() {
                            state.repos = Some(Vec::new());
                            let _ = self
                                .tx
                                .send(worker::Request::Location(Ask::Repos(host.clone())));
                        }
                    }
                    Err(why) => state.conn = Conn::Offline(why),
                }
            }
            Answer::Listed { host, dir, result } => {
                if let Some(state) = self.ssh_states.get_mut(&host) {
                    let listed = match result {
                        Ok(Some(listing)) => Dir::Listed(listing),
                        Ok(None) => Dir::Missing,
                        Err(why) => Dir::Failed(why),
                    };
                    state.dirs.insert(dir, listed);
                }
            }
            Answer::Repos { host, result } => {
                if let Some(state) = self.ssh_states.get_mut(&host) {
                    state.repos = Some(result.unwrap_or_default());
                }
            }
            Answer::Checked { host, path, result } => {
                if self.launch_cwd_checking.as_ref() == Some(&(host.clone(), path.clone())) {
                    self.launch_cwd_checking = None;
                    if self.mode == Mode::LaunchCwd {
                        self.checked_remote(host, path, result);
                    }
                }
            }
        }
        if self.mode == Mode::LaunchCwd && self.launch_cwd_pick.is_none() {
            self.launch_cwd_suggest();
        }
        self.needs_redraw = true;
    }

    /// Take `host:path` from the field: checked on the host first when it is
    /// connected, taken as it is when it is not.
    ///
    /// Not connected means the host would not answer without a prompt, or has
    /// not answered yet; neither is a reason to refuse it — the sandbox in the
    /// tab connects interactively and checks the same things there.
    pub(super) fn accept_remote(&mut self, host: &str, path: &str) {
        let path = match path.trim() {
            "" => "~".to_string(),
            path => path.to_string(),
        };
        let unchecked = match self.ssh_states.get(host).map(|s| &s.conn) {
            Some(Conn::Ready { .. }) => {
                self.launch_cwd_checking = Some((host.to_string(), path.clone()));
                let _ = self.tx.send(worker::Request::Location(Ask::Check {
                    host: host.to_string(),
                    path,
                }));
                self.needs_redraw = true;
                return;
            }
            Some(Conn::Offline(why)) => why.clone(),
            _ => "not connected yet".to_string(),
        };
        self.take_remote(RemoteTarget {
            host: host.to_string(),
            path,
            resolved: None,
            facts: None,
            problem: None,
            unchecked: Some(unchecked),
        });
    }

    /// The host's answer about the directory being accepted.
    ///
    /// Refused in the field when the host has no such directory — that is a
    /// typo, and the field is where it can be fixed. Anything else that stops
    /// a mount is kept as the target's problem: it greys out the agents that
    /// mount, and leaves the shell, which does not, free to go there.
    fn checked_remote(&mut self, host: String, path: String, result: Result<RemoteFacts, String>) {
        let facts = match result {
            Ok(facts) => facts,
            Err(why) => {
                self.refuse_launch_cwd(format!("couldn't check on {host}: {why}"));
                return;
            }
        };
        if !facts.exists || !facts.is_dir {
            let what = match facts.exists {
                false => "no such directory",
                true => "not a directory",
            };
            self.refuse_launch_cwd(format!("{what} on {host}"));
            return;
        }
        let problem = facts.resolved.as_deref().and_then(|root| {
            let at = std::path::Path::new(root);
            remote_fs::verdict(
                at,
                &remote_fs::mounts_here(),
                &remote_fs::here(at),
                Some(&facts),
            )
            .err()
        });
        self.take_remote(RemoteTarget {
            host,
            path,
            resolved: facts.resolved.clone(),
            facts: Some(facts),
            problem,
            unchecked: None,
        });
    }

    fn take_remote(&mut self, target: RemoteTarget) {
        self.launch_remote = Some(target);
        self.launch_cwd_bad = false;
        self.launch_cwd_why = None;
        self.mode = Mode::Launch;
        self.needs_redraw = true;
    }

    /// Keep the field open and say why what is in it was not taken.
    pub(super) fn refuse_launch_cwd(&mut self, why: String) {
        self.launch_cwd_bad = true;
        self.launch_cwd_why = Some(why);
        self.needs_redraw = true;
    }

    /// The line under the field while a host is in it and not ready: what is
    /// being waited on, or why it will not come. `true` marks a warning.
    pub fn location_note(&self) -> Option<(String, bool)> {
        if let Some((host, path)) = &self.launch_cwd_checking {
            return Some((format!("checking {host}:{path}…"), false));
        }
        let Typed::Remote { host, path } = parse(&self.launch_cwd_input) else {
            return None;
        };
        let state = self.ssh_states.get(host)?;
        match &state.conn {
            Conn::Connecting => Some((format!("connecting to {host}…"), false)),
            Conn::Offline(why) => Some((
                // Said in the tab's terms when a prompt is all it needs: that
                // is a host that works, only not from here.
                match why.contains("prompt") {
                    true => format!("{host}: {why} — the tab will ask"),
                    false => format!("{host} offline: {why}"),
                },
                true,
            )),
            Conn::Ready { .. } => {
                let (dir, _) = split_remote(path.trim());
                match state.dirs.get(&dir) {
                    Some(Dir::Pending) => Some((format!("listing {dir}…"), false)),
                    Some(Dir::Missing) => Some((format!("{dir} isn't there on {host}"), true)),
                    Some(Dir::Failed(why)) => Some((format!("couldn't list {dir}: {why}"), true)),
                    _ => None,
                }
            }
        }
    }

    /// Why the launcher row `choice` cannot start in the remote location, or
    /// `None` if it can — or if the location is this machine.
    pub fn launch_blocked(&self, choice: &tabs::Choice) -> Option<String> {
        let target = self.launch_remote.as_ref()?;
        match choice {
            // Reattaching lands wherever that agent already is.
            tabs::Choice::Waiting(_) => None,
            tabs::Choice::Handoff(_) => Some("a handoff starts on this machine".to_string()),
            tabs::Choice::Start(argv) => match reach_of(argv) {
                Reach::No(why) => Some(why),
                Reach::Shell(_) => None,
                Reach::Sandbox(_) => target
                    .problem
                    .as_ref()
                    .map(|why| format!("can't mount {} here: {why}", target.path)),
            },
        }
    }

    /// Start the launcher's pick in the remote location.
    ///
    /// The local half of the usability rules is asked again here, since the
    /// check in the field may be minutes old and the mount point is on this
    /// machine; the host's half was asked when the field took the path.
    pub(super) fn launch_remote_choice(&mut self, choice: &tabs::Choice, target: &RemoteTarget) {
        if let Some(why) = self.launch_blocked(choice) {
            self.set_status(why);
            return;
        }
        let tabs::Choice::Start(argv) = choice else {
            return;
        };
        let (agent, argv) = match reach_of(argv) {
            Reach::No(why) => {
                self.set_status(why);
                return;
            }
            Reach::Shell(name) => (name, shell_argv(&target.host, &target.path)),
            Reach::Sandbox(agent) => {
                if let Some(root) = &target.resolved {
                    let at = std::path::Path::new(root);
                    if let Err(why) = remote_fs::verdict(
                        at,
                        &remote_fs::mounts_here(),
                        &remote_fs::here(at),
                        target.facts.as_ref(),
                    ) {
                        self.set_status(format!("Can't start {agent} in {root}: {why}"));
                        return;
                    }
                }
                let exe = match std::env::current_exe() {
                    Ok(exe) => exe,
                    Err(error) => {
                        self.set_status(format!("Could not find cctop's own binary: {error}"));
                        return;
                    }
                };
                let argv = sandbox_argv(&exe, &agent, &target.host, &target.path);
                // The account `p` picked, carried the way a local launch
                // carries it: as the environment the sandbox, and so the agent
                // it starts, inherits.
                let argv = match Self::profile_provider(std::slice::from_ref(&agent))
                    .and_then(|p| self.chosen_profile(p))
                {
                    Some(profile) => crate::config::argv_under_profile(argv, profile),
                    None => argv,
                };
                (agent, argv)
            }
        };
        let host = target.host.clone();
        let Some(own) = self.own_preferring_rmux(Deferred::Launch, || {
            crate::rmux::free_name(&format!("{agent}-{host}"))
        }) else {
            return;
        };
        let mut pane = match tabs::Pane::launch(&argv, None, own) {
            Ok(pane) => pane,
            Err(error) => {
                self.set_status(format!("Could not start {agent} on {host}: {error}"));
                return;
            }
        };
        let label = remote_label(&agent, &host);
        pane.label = label.clone();
        if let Some(provider) = Self::profile_provider(std::slice::from_ref(&agent)) {
            pane.profile = self.chosen_profile(provider).map(|p| p.name.clone());
        }
        let kept = match pane.outlives_cctop() {
            true => " — it will outlive cctop",
            false => "",
        };
        match self.launch_into {
            LaunchInto::Split { stacked } => {
                let Some(tab) = self.active_tab() else { return };
                tab.split(pane, stacked);
            }
            LaunchInto::Tab => {
                self.tabs.push(tabs::Tab::new(pane));
                self.go_to_tab(self.tabs.len());
            }
        }
        self.set_status(format!("Started {label} in {}{kept}", target.spelled()));
    }
}

/// Shared prefix of several strings, cut to a whole character.
pub(super) fn shared_prefix(texts: &[String]) -> String {
    let Some(first) = texts.first() else {
        return String::new();
    };
    let mut end = first.len();
    for text in &texts[1..] {
        end = first
            .char_indices()
            .zip(text.chars())
            .find(|((_, a), b)| a != b)
            .map_or(end.min(text.len()), |((i, _), _)| i.min(end));
    }
    first[..end].to_string()
}

/// What Tab fills in when nothing is highlighted and the hits are not all
/// local directories: the one hit, ready to go deeper, or as far as several
/// agree.
pub(super) fn complete_mixed(typed: &str, hits: &[Hit]) -> Option<String> {
    let filled = match hits {
        [] => return None,
        [one] => one.fill(),
        many => {
            let texts: Vec<String> = many.iter().map(Hit::text).collect();
            shared_prefix(&texts)
        }
    };
    (filled.chars().count() > typed.trim().chars().count() && filled != typed).then_some(filled)
}

/// Take what Tab or Enter fills in for a highlighted hit.
pub(super) fn fill_of(hit: &Hit) -> String {
    hit.fill()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_fs::LocalSh;
    use ratatui::crossterm::event::KeyCode;
    use std::sync::mpsc::{Receiver, channel};

    #[test]
    fn the_field_reads_host_colon_path_as_a_remote_location() {
        let remote = |host, path| Typed::Remote { host, path };
        assert_eq!(parse("procdb:~/proj"), remote("procdb", "~/proj"));
        assert_eq!(parse("procdb:"), remote("procdb", ""));
        assert_eq!(parse(" me@10.0.0.5:/srv "), remote("me@10.0.0.5", "/srv"));
        assert_eq!(parse("[::1]:/srv"), remote("[::1]", "/srv"));
        // A path that starts like one is local, colons and all.
        for local in [
            "", "~", "~/a:b", "/srv/x:y", "./x:y", "a/b:c", "cctop", "[::1]/x",
        ] {
            assert_eq!(parse(local), Typed::Local(local.trim()), "{local}");
        }
        assert_eq!(split_remote(""), ("~".into(), "".into()));
        assert_eq!(split_remote("pro"), ("~".into(), "pro".into()));
        assert_eq!(split_remote("~/src/a"), ("~/src".into(), "a".into()));
        assert_eq!(split_remote("~/src/"), ("~/src".into(), "".into()));
        assert_eq!(split_remote("/s"), ("/".into(), "s".into()));
        assert_eq!(join_remote("/", "srv"), "/srv");
        assert_eq!(join_remote("~/src/", "api"), "~/src/api");
    }

    /// The agents that have a way to run their commands on the host go through
    /// the sandbox; the shell goes over ssh; the rest are refused by name.
    #[test]
    fn each_launcher_row_knows_whether_it_can_go_remote() {
        let argv = |s: &str| vec![s.to_string()];
        assert_eq!(reach_of(&argv("claude")), Reach::Sandbox("claude".into()));
        assert_eq!(
            reach_of(&argv("/home/x/.opencode/bin/opencode")),
            Reach::Sandbox("opencode".into())
        );
        for agent in ["codex", "devin", "pi"] {
            assert_eq!(
                reach_of(&argv(agent)),
                Reach::No(format!("{agent} can't run commands on a remote host yet"))
            );
        }
        if let Ok(shell) = std::env::var("SHELL")
            && !shell.is_empty()
        {
            assert!(matches!(reach_of(&argv(&shell)), Reach::Shell(_)));
        }
        assert!(matches!(reach_of(&argv("htop")), Reach::No(_)));
    }

    #[test]
    fn a_remote_launch_names_the_agent_and_the_host() {
        let exe = std::path::Path::new("/usr/local/bin/cctop");
        assert_eq!(
            sandbox_argv(exe, "opencode", "devbox", "  "),
            [
                "env",
                "CCTOP_SANDBOX_HOLD=1",
                "/usr/local/bin/cctop",
                "sandbox",
                "--agent",
                "opencode",
                "devbox:~"
            ]
        );
        let label = remote_label("opencode", "procdb");
        assert_eq!(tabs::harness_of(&label), "opencode");
        let shell = shell_argv("devbox", "~/src");
        assert_eq!(shell[..2], ["ssh", "-t"]);
        assert!(shell.contains(&"devbox".to_string()));
        // The path goes to the host's sh as an argument, `~` and all.
        let home = shell_argv("devbox", "~");
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(
                home.last()
                    .expect("line")
                    .replace("exec \"${SHELL:-sh}\" -l", "pwd"),
            )
            .env("HOME", "/")
            .output()
            .expect("sh");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "/\n");
    }

    /// An App whose worker requests can be read back.
    fn app_with_requests() -> (App, Receiver<worker::Request>) {
        let (tx, rx) = channel();
        (
            App::with_prefs(Plan::Retail, tx, crate::ui::UiPrefs::default()),
            rx,
        )
    }

    /// Answer every pending location request with `runner`, as the worker
    /// would — a stand-in ssh that runs the command here, in a fake home.
    fn serve(app: &mut App, rx: &Receiver<worker::Request>, runner: &dyn Runner, online: bool) {
        for _ in 0..10 {
            let asks: Vec<Ask> = rx
                .try_iter()
                .filter_map(|r| match r {
                    worker::Request::Location(ask) => Some(ask),
                    _ => None,
                })
                .collect();
            if asks.is_empty() {
                return;
            }
            for ask in asks {
                let connect = |_: &str| match online {
                    true => Ok(()),
                    false => Err("needs a password or key prompt".to_string()),
                };
                app.got_location(answer(ask, runner, &connect));
            }
        }
    }

    fn typed(app: &mut App, text: &str) {
        for c in text.chars() {
            app.on_key(crate::ui::tests::key(KeyCode::Char(c)));
        }
    }

    fn remote_paths(app: &App) -> Vec<String> {
        app.launch_cwd_hits
            .iter()
            .filter_map(|h| match h {
                Hit::Remote { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect()
    }

    /// Typing a host connects to it, and the host's directories and
    /// repositories are then offered and completed like local ones.
    #[test]
    fn a_host_in_the_field_is_connected_to_and_its_paths_complete() {
        let home = tempfile::tempdir().expect("home");
        let home_path = home.path().canonicalize().expect("canonical");
        std::fs::create_dir_all(home_path.join("src/api/.git")).expect("repo");
        std::fs::create_dir_all(home_path.join("src/web")).expect("dir");
        std::fs::create_dir_all(home_path.join("notes")).expect("dir");
        let sh = LocalSh {
            home: home_path.clone(),
        };

        let (mut app, rx) = app_with_requests();
        app.mode = Mode::Launch;
        app.edit_launch_cwd();
        app.launch_cwd_input.clear();
        typed(&mut app, "box:");
        assert_eq!(
            app.ssh_states.get("box").map(|s| &s.conn),
            Some(&Conn::Connecting)
        );
        assert_eq!(
            app.location_note(),
            Some(("connecting to box…".to_string(), false))
        );
        serve(&mut app, &rx, &sh, true);
        assert_eq!(
            app.ssh_states.get("box").map(|s| s.conn.clone()),
            Some(Conn::Ready {
                home: home_path.to_string_lossy().into_owned()
            })
        );
        // The home's directories, and the repository the scan found.
        assert_eq!(remote_paths(&app), ["~/src/api", "~/notes", "~/src"]);

        // Into a directory: listed once, then completed from the cache.
        app.launch_cwd_input.set("box:~/src/".to_string());
        app.launch_cwd_edited();
        serve(&mut app, &rx, &sh, true);
        assert_eq!(remote_paths(&app), ["~/src/api", "~/src/web"]);
        // Tab completes as far as they agree, then outright.
        app.launch_cwd_input.set("box:~/src/w".to_string());
        app.launch_cwd_edited();
        assert!(
            rx.try_iter().next().is_none(),
            "a listed directory is not asked again"
        );
        app.on_key(crate::ui::tests::key(KeyCode::Tab));
        assert_eq!(&*app.launch_cwd_input, "box:~/src/web/");
        // ↓ picks, Enter takes it after the host says it is usable.
        app.launch_cwd_input.set("box:~/src/".to_string());
        app.launch_cwd_edited();
        app.on_key(crate::ui::tests::key(KeyCode::Down));
        app.on_key(crate::ui::tests::key(KeyCode::Down));
        app.on_key(crate::ui::tests::key(KeyCode::Enter));
        assert!(app.launch_cwd_checking.is_some());
        serve(&mut app, &rx, &sh, true);
        assert_eq!(app.mode, Mode::Launch);
        let target = app.launch_remote.clone().expect("taken");
        assert_eq!(target.spelled(), "box:~/src/web");
        assert_eq!(
            target.resolved.as_deref(),
            Some(home_path.join("src/web").to_str().expect("utf8"))
        );
        // The stand-in host is this machine, so the local half of the rules
        // sees the directory itself: empty, and usable.
        assert_eq!(target.problem, None);
    }

    /// A directory the mount would hide is marked, not hidden: it is still
    /// the way into the directories under it.
    #[test]
    fn an_unusable_remote_directory_is_marked_with_why() {
        let home = tempfile::tempdir().expect("home");
        let home_path = home.path().canonicalize().expect("canonical");
        std::fs::create_dir_all(home_path.join("full/inside")).expect("dir");
        let sh = LocalSh { home: home_path };
        let (mut app, rx) = app_with_requests();
        app.mode = Mode::LaunchCwd;
        app.launch_cwd_input.set("box:f".to_string());
        app.launch_cwd_edited();
        serve(&mut app, &rx, &sh, true);
        assert_eq!(
            app.launch_cwd_hits,
            vec![Hit::Remote {
                host: "box".into(),
                path: "~/full".into(),
                problem: Some("not empty here — the mount would hide it".into()),
            }]
        );
        // Taken anyway, it is kept with its problem, which greys the agents
        // that mount and not the shell.
        app.on_key(crate::ui::tests::key(KeyCode::Down));
        app.on_key(crate::ui::tests::key(KeyCode::Enter));
        serve(&mut app, &rx, &sh, true);
        let target = app.launch_remote.clone().expect("taken");
        assert!(target.problem.is_some());
        let claude = tabs::Choice::Start(vec!["claude".into()]);
        assert!(
            app.launch_blocked(&claude)
                .is_some_and(|why| why.contains("can't mount ~/full here"))
        );
        // A path the host does not have is refused in the field.
        app.edit_launch_cwd();
        app.launch_cwd_input.set("box:~/nope".to_string());
        app.launch_cwd_edited();
        app.on_key(crate::ui::tests::key(KeyCode::Enter));
        serve(&mut app, &rx, &sh, true);
        assert_eq!(app.mode, Mode::LaunchCwd);
        assert_eq!(
            app.launch_cwd_why.as_deref(),
            Some("no such directory on box")
        );
    }

    /// A host that would need a prompt goes offline with the reason, the TUI
    /// never waits on it, and Enter still takes the location for the tab to
    /// check. Reopening the field tries again.
    #[test]
    fn a_host_that_will_not_connect_is_offline_and_still_usable() {
        let (mut app, rx) = app_with_requests();
        let sh = LocalSh {
            home: std::path::PathBuf::from("/"),
        };
        app.mode = Mode::LaunchCwd;
        app.launch_cwd_input.set("box:~/x".to_string());
        app.launch_cwd_edited();
        serve(&mut app, &rx, &sh, false);
        assert_eq!(
            app.ssh_states.get("box").map(|s| s.conn.clone()),
            Some(Conn::Offline("needs a password or key prompt".into()))
        );
        assert!(app.launch_cwd_hits.is_empty());
        let (note, warn) = app.location_note().expect("a note");
        assert!(warn && note.contains("needs a password"), "{note}");

        app.on_key(crate::ui::tests::key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::Launch);
        let target = app.launch_remote.clone().expect("taken");
        assert_eq!(
            target.unchecked.as_deref(),
            Some("needs a password or key prompt")
        );

        // Opening the field again retries an offline host.
        app.edit_launch_cwd();
        assert_eq!(
            app.ssh_states.get("box").map(|s| s.conn.clone()),
            Some(Conn::Connecting)
        );
        // And Esc keeps the location that was taken.
        app.on_key(crate::ui::tests::key(KeyCode::Esc));
        assert_eq!(app.launch_remote, Some(target));
    }

    /// The repository scan runs once per host, however often it connects.
    #[test]
    fn the_repository_scan_is_asked_once_per_host() {
        let (mut app, rx) = app_with_requests();
        app.preconnect("box", false);
        app.preconnect("box", false);
        let connected = || Answer::Connected {
            host: "box".into(),
            result: Ok("/home/x".into()),
        };
        app.got_location(connected());
        app.got_location(connected());
        let asks: Vec<Ask> = rx
            .try_iter()
            .filter_map(|r| match r {
                worker::Request::Location(a) => Some(a),
                _ => None,
            })
            .collect();
        assert_eq!(asks, [Ask::Connect("box".into()), Ask::Repos("box".into())]);
    }

    /// Hosts sit beside the local directories, as `name:`, and taking one puts
    /// it in the field rather than closing it.
    #[test]
    fn hosts_are_offered_beside_directories_and_taking_one_connects() {
        let (mut app, rx) = app_with_requests();
        app.mode = Mode::Launch;
        app.edit_launch_cwd();
        app.ssh_hosts = vec![crate::ssh_config::Host {
            name: "nz-b-procurementdb1".into(),
            aliases: vec!["procdb".into()],
        }];
        app.launch_cwd_input.clear();
        typed(&mut app, "procdb");
        assert_eq!(
            app.launch_cwd_hits.last().map(Hit::text).as_deref(),
            Some("nz-b-procurementdb1:")
        );
        let at = app.launch_cwd_hits.len() - 1;
        app.launch_cwd_pick = Some(at);
        app.on_key(crate::ui::tests::key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::LaunchCwd, "a host is not yet a folder");
        assert_eq!(&*app.launch_cwd_input, "nz-b-procurementdb1:");
        assert!(rx.try_iter().any(|r| matches!(
            r,
            worker::Request::Location(Ask::Connect(h)) if h == "nz-b-procurementdb1"
        )));
    }

    #[test]
    fn completion_of_mixed_hits_goes_as_far_as_they_agree() {
        let remote = |p: &str| Hit::Remote {
            host: "h".into(),
            path: p.into(),
            problem: None,
        };
        assert_eq!(
            complete_mixed("h:~/s", &[remote("~/src"), remote("~/srv")]),
            Some("h:~/sr".into())
        );
        assert_eq!(
            complete_mixed("h:~/sr", &[remote("~/src")]),
            Some("h:~/src/".into())
        );
        assert_eq!(
            complete_mixed("h:~/sr", &[remote("~/src"), remote("~/srv")]),
            None
        );
        let host = Hit::Host {
            name: "procdb".into(),
            aliases: vec![],
        };
        assert_eq!(complete_mixed("pro", &[host]), Some("procdb:".into()));
        assert_eq!(shared_prefix(&["héllo".into(), "hélp".into()]), "hél");
    }
}
