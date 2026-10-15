//! Opening a session that lives on another machine, in a tab on this one.
//!
//! A remote row used to be read-only: its process, its transcript and its
//! terminal are all over there. Reading it was enough to watch; to *work* in
//! it you had to go to that machine. This is the way over. The tab here runs a
//! transport, and the cctop on the far side decides what the tab shows there,
//! because only it can see what it decides from:
//!
//! - the session is live in that cctop's rmux: the tab attaches to it;
//! - it has ended: it is resumed there, in its project, inside that cctop's
//!   rmux — so it survives the connection dropping, and the next open
//!   reattaches instead of starting a rival;
//! - it is live in some terminal cctop did not start: nothing can take that
//!   terminal over, so the tab says so and is a shell in its project instead;
//! - the far cctop is missing or too old to do any of this: the tab says what
//!   to update and is a shell there.
//!
//! The deciding is [`open_here`], run on the far side as `cctop mux open`. The
//! transport is one of two, whichever [`crate::reach`] says the row's machine
//! is reached by — the same answer the conversation reader goes by:
//!
//! - **A sibling on the Cloudflare account** ([`crate::peer`]) needs no ssh
//!   route at all, which between a laptop and a server behind someone else's
//!   firewall is often the one that does not exist. The tab
//!   runs [`bridge`] (`cctop mux reach`), which opens a signed WebSocket to the
//!   sibling's `/api/term/<provider>/<id>`; the sibling runs `cctop mux open`
//!   on a pty and pumps it through ([`crate::ws`] has the framing).
//! - **An ssh `--host`** ([`crate::fleet`]): `ssh -t` over the launcher's
//!   ControlMaster socket when one is up, so a host that wants a password asks
//!   for it in the tab, once.
//!
//! The launcher reaches a sibling the same way for an agent that does not
//! exist yet: `machine:path` in its `in` field, for a machine on the account,
//! runs [`bridge_new`] (`cctop mux launch`), which opens the sibling's
//! [`NEW_ROUTE`]; the sibling runs `cctop mux new` ([`new_here`]), which starts
//! the agent in its rmux, in that directory, and attaches. What is asked for
//! travels as a [`NewAgent`] — words, never an argv — and the far side builds
//! the command itself. A sibling too old to know the route answers "no such
//! page", and the tab says to update it.
//!
//! Which of the sibling's accounts it runs as is picked here, from the names
//! the sibling's `/api/accounts` gave ([`crate::peer::accounts`]), and asked
//! for by name on a route of its own ([`NEW_AS_ROUTE`]): the far side looks
//! the name up among its own profiles and refuses one it does not have.
//!
//! Every name in here — the host, the session id, the provider — came out of
//! another machine's answer, so each is checked before it goes anywhere near a
//! command line: [`remote_launch::host_ok`] for an ssh destination, [`id_ok`]
//! for a session id, and everything bound for a shell through
//! [`sh_quote`](crate::sandbox::sh_quote).

use crate::fleet::Host;
use crate::reach::Reach;
use crate::remote_launch::host_ok;
use crate::sandbox::sh_quote;
use crate::session::Session;
use std::path::{Path, PathBuf};

/// Whether `id` may be handed on as a session id.
///
/// Every harness's ids are uuids, `rollout-<time>-<uuid>`, `ses_…` or cctop's
/// own `_pid_<n>`: letters, digits and `-_.`. Anything else did not come from
/// a harness, and a dash in front could read as an option to whatever is
/// handed it — so it goes nowhere.
pub fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && !id.starts_with(['-', '.'])
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// Where a sibling is asked for a terminal on a *new* agent, rather than on
/// one of its sessions.
///
/// A route of its own rather than a new shape of `/api/term/`, because of what
/// an older sibling does with it: one that predates this answers a path it
/// has never heard of with the router's "no such page", which [`refusal`]
/// reads as "update cctop there". The same request under `/api/term/` would
/// reach the old session route and come back "no such session", which says
/// nothing about updating.
pub const NEW_ROUTE: &str = "/api/new-term/";

/// [`NEW_ROUTE`] with an account of the far machine's to start it under.
///
/// A prefix of its own for the same reason [`NEW_ROUTE`] has one, and it
/// matters more here: a sibling that knows [`NEW_ROUTE`] but not accounts
/// would answer a fifth segment there with "not a launch this cctop
/// understands", which reads as a bad request rather than as "update it" — and
/// a sibling that dropped the segment instead would start the agent under the
/// wrong subscription. Under this prefix every older cctop answers "no such
/// page", and [`refusal`] says to update it. A launch with no account still
/// goes to [`NEW_ROUTE`], so an old sibling starts it as it always did.
pub const NEW_AS_ROUTE: &str = "/api/new-term-as/";

/// Whether `name` may be handed on as an account to launch under.
///
/// What [`valid_account_name`](crate::quota::valid_account_name) takes, less a
/// leading dash or dot: a token account reaches the far side's argv as
/// `cctop as <name>`, where a dash would read as an option. Whether it is an
/// account that machine *has* is the far side's to say
/// ([`NewAgent::profile_here`]).
pub fn account_ok(name: &str) -> bool {
    name.len() <= 100 && !name.starts_with(['-', '.']) && crate::quota::valid_account_name(name)
}

/// A new agent to start on a sibling — what the launcher's `machine:path`
/// asks for when the machine is reached through its Cloudflare tunnel.
///
/// The far side gets words, not a command: the agent's name, a directory, and
/// the [`Options`](crate::launch_opts::Options) as names for settings. It
/// checks each one again ([`NewAgent::check`]) and builds the argv itself, so
/// nothing in the request is ever run as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgent {
    pub agent: String,
    /// As the launcher took it: `~`, `~/…`, or absolute. Empty is the home.
    pub dir: String,
    pub options: crate::launch_opts::Options,
    /// One of the far machine's accounts for this agent, by name — the
    /// launcher learns them from the sibling's `/api/accounts`. `None` is
    /// whichever that machine starts it under by default.
    pub account: Option<String>,
}

/// Whether `dir` may be handed on as the directory to start an agent in.
///
/// Absolute or under the home, the two spellings the launcher's field takes,
/// and nothing a terminal or a shell would read as anything but a path. Empty
/// is the home. Whether it *exists* is the far side's to say, and it says so
/// before the terminal opens ([`NewAgent::problem_here`]).
pub fn dir_ok(dir: &str) -> bool {
    dir.len() <= 4096
        && !dir.chars().any(char::is_control)
        && (dir.is_empty() || dir == "~" || dir.starts_with("~/") || dir.starts_with('/'))
}

impl NewAgent {
    /// Every part checked: an agent cctop starts, a directory spelled as one,
    /// settings that agent takes, and an account name for an agent that is
    /// started under one.
    pub fn check(&self) -> Result<(), String> {
        if !crate::alias::AGENTS
            .split_whitespace()
            .any(|known| known == self.agent)
        {
            return Err(format!("{:?} is not an agent cctop starts", self.agent));
        }
        if !dir_ok(&self.dir) {
            return Err(format!(
                "{:?} is not a directory cctop will pass on",
                self.dir
            ));
        }
        if let Some(account) = &self.account {
            if !account_ok(account) {
                return Err(format!("{account:?} is not an account name"));
            }
            // Only a harness an env var points at an account: anything else
            // would start under its one account and say it had another.
            let takes = crate::pricing::Provider::parse(&self.agent)
                .and_then(crate::config::profile_env)
                .is_some();
            if !takes {
                return Err(format!("{} is not started under an account", self.agent));
            }
        }
        self.options.flags(&self.agent).map(|_| ())
    }

    /// The route on the sibling, every part of it in the path so that the
    /// signature covers it: the agent, the permission setting, then the model
    /// and the directory as hex — either may hold a `/`, and a path segment
    /// that may hold one cannot be split back apart. `-` stands for none.
    ///
    /// With an account it is [`NEW_AS_ROUTE`] and the account's name, in hex
    /// too, last; without one, [`NEW_ROUTE`] as before.
    pub fn route(&self) -> String {
        let hex_or_dash = |s: Option<&str>| match s.filter(|s| !s.is_empty()) {
            Some(s) => hex(s),
            None => "-".to_string(),
        };
        let parts = format!(
            "{}/{}/{}/{}",
            self.agent,
            self.options.permissions.word(),
            hex_or_dash(self.options.model.as_deref()),
            hex_or_dash(Some(&self.dir)),
        );
        match &self.account {
            Some(account) => format!("{NEW_AS_ROUTE}{parts}/{}", hex(account)),
            None => format!("{NEW_ROUTE}{parts}"),
        }
    }

    /// Read back what [`NewAgent::route`] wrote, after the [`NEW_ROUTE`]
    /// prefix, and check it.
    pub fn from_route(rest: &str) -> Result<NewAgent, String> {
        Self::parse_route(rest, false)
    }

    /// Read back what [`NewAgent::route`] wrote, after the [`NEW_AS_ROUTE`]
    /// prefix, and check it. The account is not optional here: a route that
    /// says it carries one and does not is not a launch.
    pub fn from_route_as(rest: &str) -> Result<NewAgent, String> {
        Self::parse_route(rest, true)
    }

    fn parse_route(rest: &str, with_account: bool) -> Result<NewAgent, String> {
        let parts: Vec<&str> = rest.split('/').collect();
        let (agent, permissions, model, dir, account) = match (with_account, &parts[..]) {
            (false, [agent, permissions, model, dir]) => (*agent, *permissions, *model, *dir, None),
            (true, [agent, permissions, model, dir, account]) => {
                (*agent, *permissions, *model, *dir, Some(unhex(account)?))
            }
            _ => return Err("not a launch this cctop understands".into()),
        };
        let unhex_or_none = |s: &str| match s {
            "-" => Ok(None),
            s => unhex(s).map(Some),
        };
        let spec = NewAgent {
            agent: agent.to_string(),
            dir: unhex_or_none(dir)?.unwrap_or_default(),
            options: crate::launch_opts::Options::from_words(
                unhex_or_none(model)?.as_deref(),
                Some(permissions),
            )?,
            account,
        };
        spec.check()?;
        Ok(spec)
    }

    /// As arguments to `cctop mux new` or `cctop mux launch`: the agent, the
    /// directory, then the settings as flags of cctop's own, the account
    /// last.
    pub fn args(&self) -> Vec<String> {
        let mut args = vec![self.agent.clone(), self.dir.clone()];
        args.extend(self.options.to_args());
        if let Some(account) = &self.account {
            args.extend(["--account".to_string(), account.clone()]);
        }
        args
    }

    /// Read back what [`NewAgent::args`] wrote, and check it.
    pub fn from_args(args: &[String]) -> Result<NewAgent, String> {
        let [agent, dir, rest @ ..] = args else {
            return Err(
                "usage: AGENT DIR [--model MODEL] [--permissions SETTING] [--account NAME]".into(),
            );
        };
        // `--account` is the launch's, not the harness's: taken off the end
        // here, and the rest handed to the options, which refuse anything
        // else — an `--account` anywhere but last among them.
        let (account, rest) = match rest {
            [options @ .., flag, name] if flag == "--account" => (Some(name.clone()), options),
            rest => (None, rest),
        };
        let spec = NewAgent {
            agent: agent.clone(),
            dir: dir.clone(),
            options: crate::launch_opts::Options::from_args(rest)?,
            account,
        };
        spec.check()?;
        Ok(spec)
    }

    /// The account this machine would start it under: `None` for its default,
    /// or the one named — refused when this machine has no such account for
    /// this agent, never quietly swapped for the default, which would spend a
    /// subscription nobody picked.
    pub fn profile_here(&self) -> Result<Option<&'static crate::config::Profile>, String> {
        let Some(name) = &self.account else {
            return Ok(None);
        };
        crate::pricing::Provider::parse(&self.agent)
            .and_then(|provider| crate::config::launchable_named(provider, name))
            .map(Some)
            .ok_or_else(|| format!("no {} account {name} on this machine", self.agent))
    }

    /// Why this machine cannot start it, asked before a terminal is opened
    /// for it: the agent is not installed here, the directory is not one, or
    /// the account is not one of this machine's.
    pub fn problem_here(&self) -> Option<String> {
        if !crate::actions::agents().contains(&self.agent) {
            return Some(format!("{} is not installed on this machine", self.agent));
        }
        let dir = self.dir.trim();
        if !dir.is_empty() && !Path::new(&crate::util::untildify(dir)).is_dir() {
            return Some(format!("{dir} is not a directory on this machine"));
        }
        self.profile_here().err()
    }
}

/// Lowercase hex of `s`'s bytes.
fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

/// The string [`hex`] wrote, or why this is not one.
fn unhex(s: &str) -> Result<String, String> {
    let bad = || format!("{s:?} is not hex");
    if !s.len().is_multiple_of(2) {
        return Err(bad());
    }
    let bytes = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()
        .ok_or_else(bad)?;
    String::from_utf8(bytes).map_err(|_| bad())
}

/// What a tab here runs to start `spec` on the sibling `machine`: this
/// cctop's `mux launch`, which bridges to the sibling's [`NEW_ROUTE`].
pub fn new_argv(machine: &str, exe: &Path, spec: &NewAgent) -> Result<Vec<String>, String> {
    if !host_ok(machine) {
        return Err(format!("{machine:?} is not a machine name"));
    }
    spec.check()?;
    let mut argv = vec![
        exe.to_string_lossy().into_owned(),
        "mux".into(),
        "launch".into(),
        machine.to_string(),
    ];
    argv.extend(spec.args());
    Ok(argv)
}

/// `cctop mux new AGENT DIR [--model M] [--permissions P] [--account A]`: on
/// the machine the agent is to run on, start it in rmux — under that account,
/// one of this machine's — the way the browser's launcher does, and attach to
/// it. `--check` answers `ok`.
///
/// In rmux, not on the pty this runs on, so the agent outlives the
/// connection: the tab on the other machine closing detaches, and the agent
/// is one this machine's dashboard lists and its `R` reattaches to.
pub fn new_here(args: &[String]) -> Result<(), String> {
    if let [flag] = args
        && flag == "--check"
    {
        println!("ok");
        return Ok(());
    }
    let spec = NewAgent::from_args(args)?;
    // Asked again here, not only by the route: this is what starts it, and a
    // name the route let through must still be an account by the time it does.
    let profile = spec.profile_here()?;
    eprint!("Starting {} on this machine…\r\n", spec.agent);
    let done = crate::actions::launch_new(&spec.agent, Some(&spec.dir), &spec.options, profile)
        .map_err(|(_, why)| why)?;
    match done.rmux {
        Some(name) => attach(&name),
        None => Err(done.message),
    }
}

/// The local rmux session a remote tab's transport runs in.
///
/// Named after the session and its machine, so opening the same row twice
/// switches to the tab already there, and a cctop restarted here finds the tab
/// still open. The machine is in the name because two machines can each hold a
/// session of one id — a transcript copied between them — and those are two
/// tabs.
pub fn tab_name(host: &str, provider: &str, id: &str) -> String {
    crate::rmux::name_for_session(&format!("{provider}-on-{host}"), id)
}

/// What a tab here runs to open `provider`/`id` on the machine `reach` names.
///
/// `dir` is the session's project as the far side reported it: the shell the
/// tab falls back to starts there. `exe` is this cctop, which the peer
/// transport runs as its bridge.
pub fn argv(
    reach: &Reach,
    exe: &Path,
    provider: &str,
    id: &str,
    dir: &str,
) -> Result<Vec<String>, String> {
    if !id_ok(id) {
        return Err(format!("{id:?} is not a session id cctop will pass on"));
    }
    if crate::pricing::Provider::parse(provider).is_none() {
        return Err(format!("{provider:?} is not a harness cctop knows"));
    }
    match reach {
        Reach::Peer(peer) => {
            let machine = &peer.machine;
            if !host_ok(machine) {
                return Err(format!("{machine:?} is not a machine name"));
            }
            Ok(vec![
                exe.to_string_lossy().into_owned(),
                "mux".into(),
                "reach".into(),
                machine.clone(),
                provider.into(),
                id.into(),
            ])
        }
        Reach::Ssh(host) => ssh_argv(host, provider, id, dir),
    }
}

/// The far side's half of an ssh open: run `cctop mux open`, and when that
/// cctop cannot — absent, or older than `mux open` — say what to update and
/// be a shell in the project instead.
///
/// Asked with `--check` first rather than read off the version: the version a
/// command arrived in is a guess about what a build supports, and this asks
/// the build. An old cctop answers the unknown word with a usage error, and a
/// missing one with the shell's "not found", and both fall through alike.
///
/// The command is the host's `--host box:/path/to/cctop` spelling, which the
/// user wrote and the poll already runs as typed — so it is `eval`ed, as the
/// poll's command line is read by the far shell. Everything after it arrived
/// from that machine and is passed as an argument, never read as code.
const SSH_SCRIPT: &str = r#"c=$1; shift
if eval "$c mux open --check" >/dev/null 2>&1; then eval "exec $c mux open \"\$@\""; fi
d=$3
printf '%s\r\n' "cctop is missing here, or too old to open sessions for another machine." "Update it on this machine (cctop --update) and open the row again." "Meanwhile, a shell in $d:" >&2
case $d in "~"|"") d=$HOME ;; "~/"*) d=$HOME/${d#"~/"} ;; esac
cd -- "$d" 2>/dev/null || cd
exec "${SHELL:-sh}" -l"#;

/// How long a failed transport waits before saying why.
///
/// The tab's client attaches to the local rmux session a moment after the
/// session's program starts, and what the program printed before that is not
/// on the screen the client is handed. An ssh refused at once, or a sibling
/// that cannot be found, fails inside that moment — and a tab that came up
/// blank and then vanished is exactly what "the tab closes itself" was. Said
/// after this, the reason lands on a screen someone is looking at.
const SETTLE: std::time::Duration = std::time::Duration::from_secs(1);

/// What runs the ssh client in the tab: run it, and when it fails, wait out
/// [`SETTLE`], say what ssh said and how it ended, and keep the tab until
/// Enter. A clean exit — detaching from the far rmux — closes the tab at once.
///
/// ssh's own diagnostics ("Connection refused", a key it would not take) go to
/// a file until then rather than the screen, for the reason [`SETTLE`] gives.
/// Nothing interactive is lost by it: ssh asks for passwords and host keys on
/// the terminal itself, and with `-t` the far side's output is all on stdout.
///
/// Read and dismissed, it exits 0: a failure would have rmux keep the pane
/// frozen (`mux::REMAIN_ON_EXIT`) for a reason the person has just read and
/// closed.
const HOLD_SCRIPT: &str = r#"e=$(mktemp 2>/dev/null) || e=/dev/null
"$@" 2>"$e"; s=$?
if [ "$s" -ne 0 ]; then
  sleep 1
  cat "$e" >&2
  printf '\r\n%s exited %s. Press Enter to close this tab.\r\n' "$1" "$s" >&2
  read -r _
  s=0
fi
[ "$e" = /dev/null ] || rm -f "$e"
exit "$s""#;

fn ssh_argv(host: &Host, provider: &str, id: &str, dir: &str) -> Result<Vec<String>, String> {
    if !host_ok(&host.target) {
        return Err(format!("{:?} is not an ssh host", host.target));
    }
    let dir = match dir.trim() {
        "" => "~",
        dir => dir,
    };
    let mut argv = vec![
        "sh".to_string(),
        "-c".to_string(),
        HOLD_SCRIPT.to_string(),
        "cctop-hold".to_string(),
        "ssh".to_string(),
        "-t".to_string(),
        // A host that never answers — a dropped VPN, a firewall that eats the
        // SYN — would otherwise leave the tab blank for as long as the kernel
        // keeps trying, which is minutes. Long enough for a slow link, short
        // enough that the reason arrives while someone is still looking.
        "-o".to_string(),
        "ConnectTimeout=20".to_string(),
    ];
    // Over the launcher's master when it is up, so a host that asked for a
    // password once does not ask again; `auto` connects afresh otherwise, and
    // asks in the tab.
    if let Some(socket) = crate::ssh_master::socket_for(&host.target) {
        argv.extend([
            "-S".to_string(),
            socket.to_string_lossy().into_owned(),
            "-o".to_string(),
            "ControlMaster=auto".to_string(),
        ]);
    }
    argv.extend([
        host.target.clone(),
        "--".to_string(),
        format!(
            "exec sh -c {} cctop-open {} {} {} {}",
            sh_quote(SSH_SCRIPT),
            sh_quote(&host.command),
            sh_quote(provider),
            sh_quote(id),
            sh_quote(dir),
        ),
    ]);
    Ok(argv)
}

/// What the far side does with an open, decided from what it can see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Become a client of this rmux session.
    Attach(String),
    /// Start the harness back up on the transcript, in rmux, then attach.
    Resume,
    /// Nothing to attach to or resume: say why, and be a shell.
    Shell(String),
}

/// Decide an open on the machine that has the session.
///
/// `holding` is [`rmux::holding`](crate::rmux::holding) and `exists`
/// [`rmux::exists`](crate::rmux::exists), passed in so the decision can be
/// tested without a daemon.
pub fn decide(
    found: Option<&Session>,
    id: &str,
    holding: impl Fn(u32) -> Option<String>,
    exists: impl Fn(&str) -> bool,
) -> Step {
    let Some(session) = found else {
        return Step::Shell(format!(
            "No session {id} on this machine any more — it may have been deleted."
        ));
    };
    // A resume from this or any machine's cctop is named this way, so it is
    // the first place a live agent is.
    let resumed = crate::rmux::name_for_session(session.provider.as_str(), &session.session_id);
    if exists(&resumed) {
        return Step::Attach(resumed);
    }
    if session.is_running() {
        // Launched by cctop under a name of its own (`cctop-claude-2`), and
        // found by the agent's pid instead.
        if let Some(name) = session.root_pid().and_then(&holding) {
            return Step::Attach(name);
        }
        return Step::Shell(format!(
            "{} is running in a terminal cctop did not start, so its screen cannot be taken \
             over from another machine. Start sessions with cctop to be able to.",
            session.display_label()
        ));
    }
    Step::Resume
}

/// `cctop mux open PROVIDER ID [DIR]`: on the machine that has the session,
/// attach to it, resume it, or say why not and be a shell — see the module
/// docs. `--check` answers `ok`, which is how the ssh transport tells a cctop
/// that has this from one that does not.
///
/// Returns only on failure, or when the attach ends: every success path either
/// becomes the rmux client or `exec`s the shell.
pub fn open_here(args: &[String]) -> Result<(), String> {
    let (provider, id, dir) = match args {
        [flag] if flag == "--check" => {
            println!("ok");
            return Ok(());
        }
        [provider, id] => (provider, id, None),
        [provider, id, dir] => (provider, id, Some(dir.as_str())),
        _ => return Err("usage: cctop mux open PROVIDER SESSION-ID [DIR]".into()),
    };
    if !id_ok(id) {
        return Err(format!("{id:?} is not a session id"));
    }
    let fallback_dir = dir.map(|d| PathBuf::from(crate::util::untildify(d)));
    eprint!("Finding the session on this machine…\r\n");
    let mut loader = crate::loader::Loader::new();
    let sessions = loader.load(crate::pricing::Plan::Retail);
    loader.store().save();
    let found = sessions
        .iter()
        .find(|s| s.remote.is_none() && s.provider.as_str() == provider && s.session_id == *id);
    let step = decide(found, id, crate::rmux::holding, crate::rmux::exists);
    let work_dir = found.and_then(Session::work_dir).or(fallback_dir);
    match step {
        Step::Attach(name) => attach(&name),
        Step::Shell(why) => shell_in(work_dir.as_deref(), &why),
        Step::Resume => {
            let session = found.expect("Resume is only decided for a session found");
            match crate::actions::resume(session) {
                Ok(done) => match done.rmux {
                    Some(name) => attach(&name),
                    None => shell_in(work_dir.as_deref(), &done.message),
                },
                Err((_, why)) => shell_in(work_dir.as_deref(), &format!("Could not resume: {why}")),
            }
        }
    }
}

/// Either end's result, with a failure held on screen until Enter when there
/// is a terminal to hold it on — and then reported as done, since it has been
/// said.
///
/// Both ends run as the only program of a tab, so a failure that ended them
/// closed the tab as it was printed. Over a sibling's socket it was worse: an
/// exit status does not cross it, the bridge saw the far pty close as it would
/// after a detach, and exited 0. Held, the reason stays until it is read. Done
/// rather than failed after the hold, so ssh and the bridge do not then hold
/// the same failure a second time. See [`SETTLE`] for the wait.
pub fn held(result: Result<(), String>) -> Result<(), String> {
    use std::io::IsTerminal;
    match result {
        Err(why) if std::io::stdin().is_terminal() => {
            std::thread::sleep(SETTLE);
            eprint!("\r\n{why}\r\n\r\nPress Enter to close this tab.\r\n");
            let _ = std::io::stdin().read_line(&mut String::new());
            Ok(())
        }
        other => other,
    }
}

/// Become the client of `name` for a tab on another machine — following
/// input, since that tab has no cctop of this machine's to take the window
/// for it when it is typed into (see [`crate::letterbox`]).
fn attach(name: &str) -> Result<(), String> {
    let args = ["attach", "--follow", name].map(String::from);
    match crate::mux::main(&args) {
        0 => Ok(()),
        // `mux::main` has already said why on stderr.
        _ => Err(format!("could not attach to {name}")),
    }
}

/// Say `why`, then become a login shell in `dir` (or the home, when that is
/// gone too) — the tab is still somewhere useful on that machine.
fn shell_in(dir: Option<&Path>, why: &str) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let dir = dir
        .filter(|d| d.is_dir())
        .map(Path::to_path_buf)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("/"));
    eprint!(
        "{why}\r\nHere is a shell in {} instead.\r\n",
        crate::util::tildify(&dir.to_string_lossy())
    );
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "sh".into());
    let error = std::process::Command::new(&shell)
        .arg("-l")
        .current_dir(&dir)
        .exec();
    Err(format!("could not start {shell}: {error}"))
}

/// How often the bridge says something when the terminal says nothing.
///
/// Cloudflare's edge closes a WebSocket that has carried nothing for 100
/// seconds, and an agent waiting for input is silent for far longer than that.
/// Each one is a ping whose pong is the corner's round trip, so this is also
/// how stale that number gets on a tab nobody types in. Hence five seconds
/// rather than anything near the edge's limit: a ping is a few bytes.
const KEEPALIVE: std::time::Duration = std::time::Duration::from_secs(5);

/// How long the sibling has to answer an open or a resume. The edge answers
/// in well under a second, or with a 5xx page; past this, the try is counted
/// as one that did not reach it, and the next one is made.
const HANDSHAKE: std::time::Duration = std::time::Duration::from_secs(15);

/// How long a write to an open socket may make no progress before the
/// connection is taken for dropped.
const WRITE_STALL: std::time::Duration = std::time::Duration::from_secs(30);

/// The most of a refusal the bridge will read before giving up on the rest.
const MAX_HEAD: usize = 64 * 1024;

/// How long the bridge keeps trying to get back to a sibling it lost.
///
/// A connection through Cloudflare's edge does not last for ever: the edge
/// restarts, cloudflared moves to another of its connections, a laptop's
/// network changes. Each of those ends the socket without a word, and the
/// bridge used to read that as the far side closing and take the tab with it
/// — "the tab closes itself after a while". The far side keeps the terminal
/// for as long ([`terminal`] resumes it), and the session over there lives in
/// its rmux besides, so a fresh open reattaches when a resume cannot be had.
/// Long enough to ride out a tunnel reconnecting; short enough
/// that a machine that is really gone is said to be within a couple of
/// minutes.
const RECONNECT_FOR: std::time::Duration = std::time::Duration::from_secs(120);

/// How the bridge's socket ended.
#[derive(Debug, PartialEq)]
enum End {
    /// The far side closed it on purpose, or this terminal went away: the
    /// tab's work is over. `Some` carries a reason the far side gave for a
    /// close that was not a plain ending.
    Closed(Option<String>),
    /// It went away without a close: the network, not either end, decided.
    Dropped(String),
}

/// `cctop mux reach MACHINE PROVIDER ID`: the tab's end of a sibling's
/// terminal. Finds the sibling, opens `/api/term/<provider>/<id>` there,
/// signed as this machine, and pumps this terminal through it until either
/// side closes — reconnecting, for up to [`RECONNECT_FOR`], when the
/// connection is dropped instead.
pub fn bridge(args: &[String]) -> Result<(), String> {
    let [machine, provider, id] = args else {
        return Err("usage: cctop mux reach MACHINE PROVIDER SESSION-ID".into());
    };
    if !host_ok(machine) || !id_ok(id) || crate::pricing::Provider::parse(provider).is_none() {
        return Err("that is not a machine, harness and session cctop will pass on".into());
    }
    eprint!("Reaching {machine}…\r\n");
    terminal(
        machine,
        &format!("/api/term/{provider}/{id}"),
        Old {
            too_old: "to open its sessions on another machine",
            again: "open the row again",
        },
        true,
    )
}

/// How to finish the sentence for a sibling that does not know the route:
/// what it is too old to do, and what to do once it is updated.
#[derive(Clone, Copy)]
struct Old {
    too_old: &'static str,
    again: &'static str,
}

/// `cctop mux launch MACHINE AGENT DIR [--model M] [--permissions P]
/// [--account A]`: the tab's end of a new agent on a sibling. Opens the
/// sibling's [`NEW_ROUTE`] (or [`NEW_AS_ROUTE`], for an account), where its
/// `cctop mux new` starts the agent, and pumps this terminal through it — the
/// launcher's way onto a machine with no ssh route.
pub fn bridge_new(args: &[String]) -> Result<(), String> {
    let [machine, rest @ ..] = args else {
        return Err(
            "usage: cctop mux launch MACHINE AGENT DIR [--model M] [--permissions P] [--account A]"
                .into(),
        );
    };
    if !host_ok(machine) {
        return Err(format!("{machine:?} is not a machine name"));
    }
    let spec = NewAgent::from_args(rest)?;
    eprint!("Reaching {machine} to start {}…\r\n", spec.agent);
    // Resumed, never reopened: opening the route again would start a second
    // agent.
    terminal(
        machine,
        &spec.route(),
        Old {
            // One that knows the plain route but not accounts is too old for
            // the account alone; saying "to start agents" would be wrong.
            too_old: match spec.account {
                Some(_) => "to start agents under one of its accounts",
                None => "to start agents for another machine",
            },
            again: "start it again",
        },
        false,
    )
}

/// Open a terminal socket at `path` on the sibling `machine`, signed as this
/// machine, and pump this terminal through it until either side closes.
///
/// A dropped connection is *resumed* when the sibling gave a token for it
/// (see [`crate::ws::RESUME_HEADER`]): the same terminal over there, sent
/// what this one missed, nothing started again. When it cannot be — the
/// sibling let it go, or is older than resume — the route is opened afresh
/// if `reopen`, which is only for a route that is safe to open twice, and
/// redraws the screen.
///
// ponytail: a new agent's tab is only ever resumed, never reopened. Its route
// starts an agent, so a second open would start a second one; when the far
// side has let the terminal go (gone past its grace, too much missed, or a
// sibling older than resume), the agent lives on in that sibling's rmux, and
// once it has written a transcript its row's `R` gets back to it.
fn terminal(machine: &str, path: &str, old: Old, reopen: bool) -> Result<(), String> {
    let peer = crate::peer::lookup(machine)?;
    let mut conn = connect(&peer, machine, path, old, None)?;
    // Raw for the tab's whole life, reconnects included: a terminal back in
    // cooked mode while the socket is away echoes every key typed at it onto
    // the far side's screen, where nothing would ever erase it.
    let _raw = Raw(crossterm::terminal::enable_raw_mode().is_ok());
    // How much of the far terminal's output this one has been handed since
    // it was opened: where a resume picks up.
    let mut received = 0u64;
    // One across reconnects: a resume carries on the same byte stream, so
    // the shadow screen it keeps is still the far side's; a fresh open
    // redraws over it.
    let mut predictor = crate::predict::Predictor::new(
        crate::predict::Mode::configured(),
        conn.size.1,
        conn.size.0,
    );
    let mut opened = std::time::Instant::now();
    let mut notice = None;
    // The numbers the dashboard's corner shows for this tab, kept beside the
    // session for as long as this bridge runs.
    let link = crate::link_health::Writer::from_env();
    loop {
        let why = match pump(
            &mut conn.up,
            &mut conn.decoder,
            conn.size,
            &mut received,
            &mut predictor,
            &link,
            machine,
            notice.take(),
        )? {
            End::Closed(None) => return Ok(()),
            End::Closed(Some(why)) => return Err(format!("{machine} closed the terminal: {why}")),
            End::Dropped(why) => why,
        };
        crate::ws::trace(
            "bridge",
            format_args!(
                "{machine} {path}: dropped after {:.1}s, {received} bytes in: {why}",
                opened.elapsed().as_secs_f64()
            ),
        );
        link.set_state(crate::link_health::State::Reconnecting);
        let Back {
            conn: again,
            resumed,
            unsent,
        } = reconnect(
            &peer,
            Lost {
                machine,
                path,
                old,
                reopen,
                why: &why,
            },
            conn.token.take().map(|token| (token, received)),
            &predictor,
            &mut std::io::stdout(),
        )?;
        conn = again;
        link.set_state(crate::link_health::State::Connected);
        opened = std::time::Instant::now();
        if !resumed {
            received = 0;
        }
        // The bottom row as the far side left it, whatever `pump` had up
        // there when the socket went — "not answering", say — which a quick
        // reconnect would otherwise leave standing. Then guesses whose keys
        // went down with the old socket are painted back.
        let mut back = Vec::new();
        if predictor.noticed() {
            back = predictor.unnotice();
        }
        back.extend(predictor.reconnected(resumed));
        if !back.is_empty() {
            use std::io::Write;
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(&back);
            let _ = stdout.flush();
        }
        if unsent > 0 {
            notice = Some(unsent_text(machine));
        }
    }
}

/// What was lost, for [`reconnect`]: which terminal, on which sibling, why,
/// and what may be done about it.
struct Lost<'a> {
    machine: &'a str,
    path: &'a str,
    old: Old,
    /// Whether the route may be opened afresh when it cannot be resumed.
    reopen: bool,
    why: &'a str,
}

/// How long getting back may take before the tab says so. A tunnel that
/// recovers inside this is a drop nobody sees: no notice, nothing to clean.
/// Short under test, so the notice is seen without a test waiting for it.
const NOTICE_AFTER: std::time::Duration = if cfg!(test) {
    std::time::Duration::from_secs(1)
} else {
    std::time::Duration::from_secs(2)
};

/// The longest wait between tries to get back.
const RETRY_MAX: std::time::Duration = std::time::Duration::from_secs(5);

/// How long to wait after try `n` (from 1) failed: quick at first, while a
/// blip is likeliest to be over — the edge answers 530 for a few seconds
/// after the tunnel's connection moves — then backing off to [`RETRY_MAX`].
fn retry_wait(n: u32) -> std::time::Duration {
    let ms = 250u64 << n.saturating_sub(1).min(8);
    std::time::Duration::from_millis(ms).min(RETRY_MAX)
}

/// The bottom row while getting back takes a while. Keys typed now are
/// dropped once back (see [`reconnect`]), so it says so before any are.
fn notice_text(machine: &str, why: &str) -> String {
    format!(" {machine} lost: {why} · reconnecting, typing not sent ")
}

/// The bottom row once back, when keys typed while away were dropped: a
/// reconnect quick enough to show nothing would otherwise lose them unseen.
fn unsent_text(machine: &str) -> String {
    format!(" {machine} back · typing while away not sent ")
}

/// How long [`unsent_text`] stays up, unless a key takes it down first.
const UNSENT_FOR: std::time::Duration = std::time::Duration::from_secs(3);

/// What [`reconnect`] got back to.
struct Back {
    conn: Conn,
    /// Whether it carries on the old byte stream, rather than a fresh one.
    resumed: bool,
    /// How many bytes typed while away were thrown out.
    unsent: usize,
}

/// One try at [`connect`], made on a thread of its own so the tab can say
/// it is away while the try is still blocked: a network that drops packets
/// holds a connect for its timeout, then a handshake for its.
fn try_connect(
    peer: &std::sync::Arc<crate::peer::Peer>,
    machine: &str,
    path: &str,
    old: Old,
    resume: Option<(String, u64)>,
) -> std::sync::mpsc::Receiver<Result<Conn, Failed>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (peer, machine, path) = (
        std::sync::Arc::clone(peer),
        machine.to_owned(),
        path.to_owned(),
    );
    std::thread::spawn(move || {
        let asked = resume.as_ref().map(|(token, at)| (token.as_str(), *at));
        let _ = tx.send(connect(&peer, &machine, &path, old, asked));
    });
    rx
}

/// Throw out what was typed at the tab while it was away, saying how much.
fn drop_typed() -> usize {
    let mut queued: libc::c_int = 0;
    // SAFETY: FIONREAD writes one int, and tcflush on stdin only discards
    // what is queued on it.
    unsafe {
        if libc::ioctl(0, libc::FIONREAD, &mut queued) != 0 {
            queued = 0;
        }
        if libc::tcflush(0, libc::TCIFLUSH) != 0 {
            return 0;
        }
    }
    usize::try_from(queued).unwrap_or(0)
}

/// Get back to a sibling after the socket dropped: resume `resume` (its
/// token, and how much was received) when there is one, and open afresh when
/// not and the route may be.
///
/// Nothing is said for the first [`NOTICE_AFTER`]: most drops are the tunnel
/// moving, and over before anyone could read a word about them. Past that, a
/// one-line notice goes over the bottom row (see
/// [`crate::predict::Predictor::notice`]), and once back — or on giving up —
/// the row is painted as the far side left it, from the predictor's shadow.
/// The far side is asked for nothing, so a sibling of any version is left
/// with a clean screen.
///
/// Keys typed while the tab was away are dropped, not sent: they were typed
/// at a screen that was not answering, and would land on one that has moved
/// on — an Enter meant for a prompt that has since gone. How many is
/// returned, for the tab to say so once back.
fn reconnect(
    peer: &std::sync::Arc<crate::peer::Peer>,
    lost: Lost,
    mut resume: Option<(String, u64)>,
    predictor: &crate::predict::Predictor,
    screen: &mut dyn std::io::Write,
) -> Result<Back, String> {
    let Lost {
        machine,
        path,
        old,
        reopen,
        why,
    } = lost;
    let ended = |gone: &str| {
        format!(
            "lost {machine}: {gone}. The agent goes on running there — \
             its row's R gets back to it once it has a session"
        )
    };
    if resume.is_none() && !reopen {
        return Err(ended(why));
    }
    let since = std::time::Instant::now();
    let mut said = false;
    let mut draw = |bytes: Vec<u8>| {
        let _ = screen.write_all(&bytes);
        let _ = screen.flush();
    };
    let trace = |what: std::fmt::Arguments| crate::ws::trace("bridge", what);
    let mut tries = 0u32;
    let due = since + NOTICE_AFTER;
    loop {
        tries += 1;
        let asking = resume.is_some();
        let pending = try_connect(peer, machine, path, old, resume.clone());
        // The notice goes up when it is due, even with the try still out.
        let got = loop {
            if said {
                break pending.recv().ok();
            }
            let left = due.saturating_duration_since(std::time::Instant::now());
            match pending.recv_timeout(left) {
                Ok(got) => break Some(got),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    draw(predictor.notice(&notice_text(machine, why)));
                    said = true;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break None,
            }
        };
        let got = got.unwrap_or_else(|| Err(Failed::Unreached("the try to connect died".into())));
        match got {
            Ok(conn) => {
                // A sibling that read the resume answers with its token; one
                // too old to answers like a fresh open.
                let resumed = asking && conn.token.is_some();
                let unsent = drop_typed();
                if said {
                    draw(predictor.unnotice());
                }
                trace(format_args!(
                    "{machine} {path}: {} after {:.1}s, try {tries}{}",
                    if resumed { "resumed" } else { "reopened" },
                    since.elapsed().as_secs_f64(),
                    if said { ", notice shown" } else { "" },
                ));
                return Ok(Back {
                    conn,
                    resumed,
                    unsent,
                });
            }
            // The far terminal is gone: open afresh at once, which redraws,
            // or — where that would start a second agent — say so.
            Err(Failed::Gone(gone)) => {
                trace(format_args!("{machine} {path}: try {tries}: gone: {gone}"));
                resume = None;
                if !reopen {
                    if said {
                        draw(predictor.unnotice());
                    }
                    return Err(ended(&gone));
                }
                continue;
            }
            // A refusal is the far side's answer — the session has gone, or
            // this machine is no longer let in — and asking again will not
            // change it.
            Err(Failed::Refused(no)) => {
                trace(format_args!("{machine} {path}: try {tries}: refused: {no}"));
                if said {
                    draw(predictor.unnotice());
                }
                return Err(no);
            }
            Err(Failed::Unreached(away)) if since.elapsed() >= RECONNECT_FOR => {
                trace(format_args!("{machine} {path}: gave up: {away}"));
                if said {
                    draw(predictor.unnotice());
                }
                return Err(format!(
                    "lost {machine} and could not reach it again for {}s: {away}",
                    RECONNECT_FOR.as_secs()
                ));
            }
            Err(Failed::Unreached(away)) => {
                trace(format_args!(
                    "{machine} {path}: try {tries}: unreached: {away}"
                ));
            }
        }
        // Nor does a wait between tries hold the notice past its time.
        let next = std::time::Instant::now() + retry_wait(tries);
        if !said && due < next {
            std::thread::sleep(due.saturating_duration_since(std::time::Instant::now()));
            draw(predictor.notice(&notice_text(machine, why)));
            said = true;
        }
        std::thread::sleep(next.saturating_duration_since(std::time::Instant::now()));
    }
}

/// Why an open did not become a terminal.
enum Failed {
    /// The sibling answered, and the answer was no.
    Refused(String),
    /// The sibling was not reached, or stopped answering partway.
    Unreached(String),
    /// The terminal a resume asked for is not there any more.
    Gone(String),
}

impl From<Failed> for String {
    fn from(failed: Failed) -> String {
        match failed {
            Failed::Refused(why) | Failed::Unreached(why) | Failed::Gone(why) => why,
        }
    }
}

/// A terminal socket, open.
struct Conn {
    up: crate::peer::Upstream,
    /// Holding whatever arrived behind the handshake's answer.
    decoder: crate::ws::Decoder,
    /// This terminal's size when it was opened.
    size: (u16, u16),
    /// What to resume it with, when the sibling gave one.
    token: Option<String>,
}

/// Open the sibling's terminal at `path`, sized to this one — or, with
/// `resume`, get back to the one its token names, from that offset.
fn connect(
    peer: &crate::peer::Peer,
    machine: &str,
    path: &str,
    old: Old,
    resume: Option<(&str, u64)>,
) -> Result<Conn, Failed> {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let key = crate::ws::new_key();
    let headers = [
        ("Upgrade".to_string(), "websocket".to_string()),
        ("Connection".to_string(), "Upgrade".to_string()),
        ("Sec-WebSocket-Key".to_string(), key.clone()),
        ("Sec-WebSocket-Version".to_string(), "13".to_string()),
        // The far side's output compressed, when it can: a sibling older
        // than that answers without the header, and nothing is.
        (
            "Sec-WebSocket-Extensions".to_string(),
            crate::ws::DEFLATE.to_string(),
        ),
        (
            crate::ws::RESUME_HEADER.to_string(),
            match resume {
                Some((token, offset)) => format!("{token} {offset}"),
                None => crate::ws::RESUME_NEW.to_string(),
            },
        ),
    ];
    let mut up = peer
        .socket(path, &format!("cols={cols}&rows={rows}"), &headers)
        .map_err(Failed::Unreached)?;
    // The handshake is a request like any other, and an edge that took the
    // connection but never answers it would otherwise hold the tab past
    // `RECONNECT_FOR`, frozen and saying nothing.
    let _ = up.socket().set_read_timeout(Some(HANDSHAKE));
    let _ = up.socket().set_write_timeout(Some(HANDSHAKE));

    let mut got = Vec::new();
    let end = loop {
        if let Some(at) = got.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        if got.len() > MAX_HEAD
            || !up
                .read_ready(&mut got)
                .map_err(|e| Failed::Unreached(format!("{machine} hung up: {e}")))?
        {
            return Err(Failed::Unreached(format!(
                "{machine} hung up before it answered"
            )));
        }
    };
    let head = String::from_utf8_lossy(&got[..end]).into_owned();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    if status != 101 {
        let mut body = got[end..].to_vec();
        let wanted = header(&head, "content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
            .min(MAX_HEAD);
        while body.len() < wanted && up.read_ready(&mut body).unwrap_or(false) {}
        let body = String::from_utf8_lossy(&body);
        if status == 410 {
            let said = body.lines().next().unwrap_or("").trim();
            let said = said.split_once(": ").map_or(said, |(_, why)| why);
            return Err(Failed::Gone(said.to_string()));
        }
        let why = refusal(machine, status, &body, old.too_old, old.again);
        // The edge's own pages — a 502 while the tunnel is down, a 530 while
        // its connector is away — are Cloudflare saying the sibling is not
        // there right now, not the sibling saying no.
        return Err(match status {
            502..=504 | 520..=530 => Failed::Unreached(why),
            _ => Failed::Refused(why),
        });
    }
    if header(&head, "sec-websocket-accept") != Some(crate::ws::accept_key(&key).as_str()) {
        return Err(Failed::Refused(format!(
            "{machine} answered with something that is not a terminal"
        )));
    }
    // Upgraded: reads only ever follow a `poll`, and `SILENCE` is the clock
    // on a far side that says nothing. A write is a few keys, which a live
    // socket takes at once; one that cannot is a connection gone, to be
    // reconnected rather than blocked on.
    let _ = up.socket().set_read_timeout(None);
    let _ = up.socket().set_write_timeout(Some(WRITE_STALL));
    let mut decoder = crate::ws::Decoder::default();
    if header(&head, "sec-websocket-extensions").is_some_and(crate::ws::deflate_agreed) {
        decoder = decoder.inflating();
    }
    decoder.push(&got[end..]);
    Ok(Conn {
        up,
        decoder,
        size: (cols, rows),
        token: header(&head, crate::ws::RESUME_HEADER).map(str::to_string),
    })
}

/// What a close frame's payload says about why: nothing for a plain ending
/// (no code, or 1000), the reason — or the code, when it gave none — for
/// anything else.
fn close_reason(payload: &[u8]) -> Option<String> {
    let [hi, lo, reason @ ..] = payload else {
        return None;
    };
    let code = u16::from_be_bytes([*hi, *lo]);
    if code == 1000 {
        return None;
    }
    let reason = String::from_utf8_lossy(reason).trim().to_string();
    Some(match reason.is_empty() {
        true => format!("close code {code}"),
        false => reason,
    })
}

/// A header's value out of a response head, by name.
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// What to tell the person in the tab when the sibling said no.
///
/// A sibling that has never heard of the route answers it like any unknown
/// page, which is the one refusal that means "update that machine" rather
/// than whatever it says.
fn refusal(machine: &str, status: u16, body: &str, too_old: &str, again: &str) -> String {
    let said = body.lines().next().unwrap_or("").trim();
    match status {
        404 if said.ends_with("no such page") || said.is_empty() => format!(
            "{machine}'s cctop is too old {too_old} — \
             update it there (cctop --update) and {again}"
        ),
        _ => format!("{machine} refused: {said}"),
    }
}

/// Restores the terminal however the bridge ends.
struct Raw(bool);

impl Drop for Raw {
    fn drop(&mut self) {
        if self.0 {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
}

/// How long the sibling may say nothing — not even the answer to a
/// keepalive — before the connection is taken for dead.
///
/// A socket whose far end vanished without a FIN, which is how a dropped
/// network ends one, stays open here for as long as the kernel cares to
/// retry: the tab would sit frozen rather than close. Over a minute of
/// keepalives unanswered is a connection that is not coming back.
const SILENCE: std::time::Duration = std::time::Duration::from_secs(75);

/// How long a ping may go unanswered before the tab says the far side is
/// not answering — and how long the far side may have said nothing before a
/// key typed asks it. A live socket's pong is back in well under a second.
const UNANSWERED: std::time::Duration = std::time::Duration::from_secs(5);

// Eight, because each is a different piece of the one tab's state that
// outlives a socket; a struct of them would be a name for this argument list.
#[allow(clippy::too_many_arguments)]
fn pump(
    up: &mut crate::peer::Upstream,
    decoder: &mut crate::ws::Decoder,
    mut size: (u16, u16),
    received: &mut u64,
    predictor: &mut crate::predict::Predictor,
    link: &crate::link_health::Writer,
    machine: &str,
    notice: Option<String>,
) -> Result<End, String> {
    use crate::ws::{Op, frame};
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    // Raw (from `terminal`), so every key goes to the far side as typed —
    // Ctrl+C included, which is the agent's to see and not this bridge's to
    // die of.
    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    // A second handle on it for the loop, which `deliver` holds the first:
    // guesses drawn as keys are typed, and taken back when late.
    let mut screen = std::io::stdout();
    let mut said = std::time::Instant::now();
    let mut heard = std::time::Instant::now();
    let mut buf = [0u8; 16 * 1024];
    let mut incoming = Vec::new();
    let dropped = |e: std::io::Error| End::Dropped(e.to_string());
    // Sized to this terminal, which may have changed while it was away.
    let back = predictor.resize(size.1, size.0);
    screen.write_all(&back).map_err(|e| e.to_string())?;
    // A notice from getting back, up for a moment or until the next key.
    let mut notice_until = None;
    if let Some(text) = notice {
        screen
            .write_all(&predictor.notice(&text))
            .map_err(|e| e.to_string())?;
        screen.flush().map_err(|e| e.to_string())?;
        notice_until = Some(std::time::Instant::now() + UNSENT_FOR);
    }
    // Whether the bottom row says the far side is not answering.
    let mut quiet = false;
    // When the keepalive in flight went out: its pong is a round trip.
    let mut pinged: Option<std::time::Instant> = None;
    // Every frame decoded so far; `Some` once one of them ends the socket.
    let mut deliver = |decoder: &mut crate::ws::Decoder,
                       up: &mut crate::peer::Upstream,
                       predictor: &mut crate::predict::Predictor,
                       pinged: &mut Option<std::time::Instant>|
     -> Result<Option<End>, String> {
        loop {
            let (op, payload) = match decoder.frame() {
                Ok(Some(next)) => next,
                Ok(None) => break,
                // A stream that no longer parses is a connection gone wrong
                // in transit; a fresh one starts clean.
                Err(why) => return Ok(Some(End::Dropped(why))),
            };
            match op {
                Op::Close => return Ok(Some(End::Closed(close_reason(&payload)))),
                Op::Ping => {
                    if let Err(e) = up.write_all(&frame(Op::Pong, &payload, true)) {
                        return Ok(Some(dropped(e)));
                    }
                }
                Op::Pong => {
                    if let Some(at) = pinged.take() {
                        predictor.rtt_sample(at.elapsed());
                        link.rtt(predictor.srtt());
                    }
                }
                // Output, counted once it is on this terminal: what a resume
                // asks the rest from. Each is followed by whatever guesses
                // are still out, drawn again over it.
                Op::Binary => {
                    stdout.write_all(&payload).map_err(|e| e.to_string())?;
                    *received += payload.len() as u64;
                    link.received_raw(payload.len());
                    let over = predictor.on_output(&payload, std::time::Instant::now());
                    stdout.write_all(&over).map_err(|e| e.to_string())?;
                }
                Op::Text | Op::Other(_) => {
                    stdout.write_all(&payload).map_err(|e| e.to_string())?;
                    let over = predictor.on_output(&payload, std::time::Instant::now());
                    stdout.write_all(&over).map_err(|e| e.to_string())?;
                }
            }
        }
        stdout.flush().map_err(|e| e.to_string())?;
        Ok(None)
    };
    // Anything that arrived with the handshake's answer.
    if let Some(end) = deliver(decoder, up, predictor, &mut pinged)? {
        return Ok(end);
    }
    // A ping straight away, so the corner has a round trip as soon as the
    // link is up rather than at the first key typed or the first keepalive
    // — until then it can only say `—`.
    if let Err(e) = up.write_all(&frame(Op::Ping, b"", true)) {
        return Ok(dropped(e));
    }
    pinged = Some(std::time::Instant::now());
    loop {
        let mut fds = [
            libc::pollfd {
                fd: stdin.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: up.socket().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // Woken often while a guess is out, so one nothing answers is taken
        // back in time.
        let timeout = if predictor.waiting() { 50 } else { 1000 };
        // SAFETY: two valid pollfds on fds that outlive the call.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(std::io::Error::last_os_error().to_string());
        }
        // A resize is polled for rather than caught: SIGWINCH would need a
        // handler for one number that a second's lag does not hurt.
        if let Ok(now) = crossterm::terminal::size()
            && now != size
        {
            size = now;
            let back = predictor.resize(now.1, now.0);
            screen.write_all(&back).map_err(|e| e.to_string())?;
            let resize = crate::ws::resize_message(now.0, now.1);
            if let Err(e) = up.write_all(&frame(Op::Text, resize.as_bytes(), true)) {
                return Ok(dropped(e));
            }
            said = std::time::Instant::now();
        }
        if said.elapsed() >= KEEPALIVE {
            if let Err(e) = up.write_all(&frame(Op::Ping, b"", true)) {
                return Ok(dropped(e));
            }
            said = std::time::Instant::now();
            pinged = Some(said);
        }
        let late = predictor.tick(std::time::Instant::now());
        if !late.is_empty() {
            screen.write_all(&late).map_err(|e| e.to_string())?;
            screen.flush().map_err(|e| e.to_string())?;
        }
        if notice_until.is_some_and(|until| std::time::Instant::now() >= until) {
            notice_until = None;
            screen
                .write_all(&predictor.take_notice())
                .map_err(|e| e.to_string())?;
            screen.flush().map_err(|e| e.to_string())?;
        }
        // A socket dead without a FIN is only given up on at `SILENCE`; until
        // then, keys typed go nowhere, and the tab says so as soon as a ping
        // goes unanswered rather than looking alive.
        if !quiet && pinged.is_some_and(|at| at.elapsed() >= UNANSWERED) {
            quiet = true;
            notice_until = None;
            let text = format!(" {machine} not answering · keys may not arrive ");
            screen
                .write_all(&predictor.notice(&text))
                .map_err(|e| e.to_string())?;
            screen.flush().map_err(|e| e.to_string())?;
        }
        if heard.elapsed() >= SILENCE {
            return Ok(End::Dropped(format!(
                "no answer for {}s",
                SILENCE.as_secs()
            )));
        }
        if fds[0].revents != 0 {
            match stdin.read(&mut buf) {
                Ok(n) if n > 0 => {
                    if notice_until.take().is_some() {
                        screen
                            .write_all(&predictor.take_notice())
                            .map_err(|e| e.to_string())?;
                        screen.flush().map_err(|e| e.to_string())?;
                    }
                    let guess = predictor.on_key(&buf[..n], std::time::Instant::now());
                    if let Err(e) = up.write_all(&frame(Op::Binary, &buf[..n], true)) {
                        return Ok(dropped(e));
                    }
                    link.sent(n);
                    said = std::time::Instant::now();
                    // A key into a socket that has said nothing for a while
                    // asks whether anyone is there, so a dead one is noticed
                    // in seconds rather than at the next keepalive.
                    if pinged.is_none() && heard.elapsed() >= UNANSWERED {
                        if let Err(e) = up.write_all(&frame(Op::Ping, b"", true)) {
                            return Ok(dropped(e));
                        }
                        pinged = Some(said);
                    }
                    if !guess.is_empty() {
                        screen.write_all(&guess).map_err(|e| e.to_string())?;
                        screen.flush().map_err(|e| e.to_string())?;
                    }
                }
                // This terminal went away: the far side's attach is detached
                // by the socket closing, and the agent there carries on.
                _ => {
                    let _ = up.write_all(&frame(Op::Close, b"", true));
                    return Ok(End::Closed(None));
                }
            }
        }
        if fds[1].revents != 0 {
            incoming.clear();
            let open = match up.read_ready(&mut incoming) {
                Ok(open) => open,
                Err(e) => return Ok(dropped(e)),
            };
            if !incoming.is_empty() {
                heard = std::time::Instant::now();
                link.received_wire(incoming.len());
                // A typed key that came back is a sample too, taken in
                // `on_output`; read off here rather than there, once a read.
                link.rtt(predictor.srtt());
            }
            decoder.push(&incoming);
            if let Some(end) = deliver(decoder, up, predictor, &mut pinged)? {
                return Ok(end);
            }
            if quiet && !incoming.is_empty() {
                quiet = false;
                screen
                    .write_all(&predictor.take_notice())
                    .map_err(|e| e.to_string())?;
                screen.flush().map_err(|e| e.to_string())?;
            }
            // The socket ended with no close frame: nobody said goodbye, so
            // nobody meant it.
            if !open {
                return Ok(End::Dropped("the connection ended".into()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sibling(machine: &str) -> Reach {
        Reach::Peer(std::sync::Arc::new(crate::peer::Peer::fixed(
            machine,
            "http://127.0.0.1:9",
            "made-up",
        )))
    }

    fn host(target: &str, command: &str) -> Host {
        Host {
            target: target.into(),
            command: command.into(),
        }
    }

    /// A close with no code, or 1000, is the terminal ending and closes the
    /// tab quietly; any other code is a reason the tab holds on screen.
    #[test]
    fn only_a_plain_close_ends_the_tab_without_a_word() {
        assert_eq!(close_reason(b""), None);
        assert_eq!(close_reason(&1000u16.to_be_bytes()), None);
        let mut garbled = 1002u16.to_be_bytes().to_vec();
        garbled.extend(b"frame too large");
        assert_eq!(close_reason(&garbled).as_deref(), Some("frame too large"));
        assert_eq!(
            close_reason(&1011u16.to_be_bytes()).as_deref(),
            Some("close code 1011")
        );
    }

    #[test]
    fn a_session_id_is_letters_digits_and_dashes() {
        for good in [
            "4ebf1ab4-2ef8-4fb2-a7d5-d445b5026dc9",
            "rollout-2025-01-01T10-00-00-4ebf1ab4",
            "ses_abc123",
            "_pid_4242",
        ] {
            assert!(id_ok(good), "{good}");
        }
        for bad in [
            "",
            "-oProxyCommand=x",
            "--check",
            "..",
            "a/b",
            "a b",
            "$(touch /tmp/x)",
            "a;b",
            "a'b",
            "a\nb",
        ] {
            assert!(!id_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_hostile_host_or_id_builds_nothing() {
        let exe = Path::new("/usr/bin/cctop");
        let ssh = Reach::Ssh(host("-oProxyCommand=touch /tmp/pwned", "cctop"));
        assert!(argv(&ssh, exe, "claude", "abc", "/w").is_err());
        let peer = sibling("-x");
        assert!(argv(&peer, exe, "claude", "abc", "/w").is_err());
        let fine = Reach::Ssh(host("box", "cctop"));
        assert!(argv(&fine, exe, "claude", "$(reboot)", "/w").is_err());
        assert!(argv(&fine, exe, "nonsense", "abc", "/w").is_err());
    }

    #[test]
    fn a_peer_tab_runs_this_cctops_bridge() {
        let got = argv(
            &sibling("computer36"),
            Path::new("/usr/bin/cctop"),
            "claude",
            "4ebf1ab4",
            "/srv/x",
        )
        .expect("argv");
        assert_eq!(
            got,
            [
                "/usr/bin/cctop",
                "mux",
                "reach",
                "computer36",
                "claude",
                "4ebf1ab4"
            ]
        );
    }

    /// The ssh line hands the far shell one script and four arguments; the
    /// path, however hostile, is an argument. Run here with a fake `cctop`
    /// that records what it was asked, the far side is checked end to end.
    #[test]
    fn the_ssh_line_runs_mux_open_and_falls_back_to_a_shell() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = dir.path().join("log");
        let fake = dir.path().join("cctop");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\nprintf '%s|' \"$@\" >> {}\necho >> {}\n",
                log.display(),
                log.display()
            ),
        )
        .expect("fake");
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");
        let hostile = "/w/$(touch pwned)'; touch pwned2";
        let line = ssh_argv(
            &host("box", &fake.to_string_lossy()),
            "claude",
            "abc-1",
            hostile,
        )
        .expect("argv");
        assert_eq!(line[..2], ["sh", "-c"]);
        assert_eq!(line[4..6], ["ssh", "-t"]);
        let remote = line.last().expect("command");
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(remote)
            .current_dir(dir.path())
            .output()
            .expect("sh");
        assert!(out.status.success(), "{out:?}");
        let said = std::fs::read_to_string(&log).expect("log");
        assert_eq!(
            said,
            format!("mux|open|--check|\nmux|open|claude|abc-1|{hostile}|\n")
        );
        assert!(!dir.path().join("pwned").exists());
        assert!(!dir.path().join("pwned2").exists());

        // A cctop that does not know `mux open` — here, one that fails every
        // command — gets the message and a shell in the project instead.
        let project = dir.path().join("proj");
        std::fs::create_dir(&project).expect("mkdir");
        let shell = dir.path().join("fake-shell");
        std::fs::write(&shell, "#!/bin/sh\npwd\n").expect("fake shell");
        std::fs::set_permissions(&shell, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");
        let line = ssh_argv(
            &host("box", "false"),
            "claude",
            "abc-1",
            &project.to_string_lossy(),
        )
        .expect("argv");
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(line.last().expect("command"))
            .env("SHELL", &shell)
            .output()
            .expect("sh");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("cctop --update"), "{err}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            project.to_string_lossy()
        );
    }

    #[test]
    fn the_far_side_attaches_resumes_or_explains() {
        let mut s = Session::new(crate::pricing::Provider::Claude, "abc".into());
        let resumed = crate::rmux::name_for_session("claude", "abc");

        // Gone: a shell, and why.
        assert!(matches!(
            decide(None, "abc", |_| None, |_| false),
            Step::Shell(why) if why.contains("abc")
        ));
        // Ended: resumed.
        assert_eq!(decide(Some(&s), "abc", |_| None, |_| false), Step::Resume);
        // Already resumed by some cctop: attached.
        assert_eq!(
            decide(Some(&s), "abc", |_| None, |n| n == resumed),
            Step::Attach(resumed.clone())
        );
        // Live in a plain terminal: no takeover.
        s.inferred_running = true;
        assert!(matches!(
            decide(Some(&s), "abc", |_| Some("x".into()), |_| false),
            Step::Shell(why) if why.contains("did not start")
        ));
    }

    /// How a fake sibling answers one request.
    enum Answer {
        /// `410 Gone`, as for a terminal that can no longer be resumed.
        Gone,
        /// An upgrade, with a resume token or — like a sibling older than
        /// resume — without.
        Upgrade(Option<&'static str>),
        /// The edge's 530 while the sibling's tunnel is away.
        Away,
        /// An upgrade that takes past [`NOTICE_AFTER`] to come: a try held
        /// up on a network that drops packets.
        Late(&'static str),
    }

    /// A sibling that answers each request in turn with the next of
    /// `answers`, and reports the `Cctop-Resume` each one asked with.
    fn fake_sibling(
        answers: Vec<Answer>,
    ) -> (
        std::sync::Arc<crate::peer::Peer>,
        std::sync::mpsc::Receiver<String>,
    ) {
        use std::io::{BufRead, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (asked, heard) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for answer in answers {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let (mut key, mut resume) = (String::new(), String::new());
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let line = line.trim_end();
                    if line.is_empty() {
                        break;
                    }
                    if let Some((k, v)) = line.split_once(':') {
                        match k.to_ascii_lowercase().as_str() {
                            "sec-websocket-key" => key = v.trim().to_string(),
                            "cctop-resume" => resume = v.trim().to_string(),
                            _ => {}
                        }
                    }
                }
                asked.send(resume).unwrap();
                let reply = match answer {
                    Answer::Gone => {
                        let body = "410 Gone: that terminal has ended\n";
                        format!(
                            "HTTP/1.1 410 Gone\r\nContent-Length: {}\r\n\r\n{body}",
                            body.len()
                        )
                    }
                    Answer::Away => "HTTP/1.1 530 \r\nContent-Length: 0\r\n\r\n".to_string(),
                    Answer::Late(token) => {
                        std::thread::sleep(NOTICE_AFTER + std::time::Duration::from_millis(600));
                        format!(
                            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
                             Connection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\
                             Cctop-Resume: {token}\r\n\r\n",
                            crate::ws::accept_key(&key),
                        )
                    }
                    Answer::Upgrade(token) => format!(
                        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
                         Connection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n{}\r\n",
                        crate::ws::accept_key(&key),
                        token.map_or(String::new(), |t| format!("Cctop-Resume: {t}\r\n")),
                    ),
                };
                stream.write_all(reply.as_bytes()).unwrap();
            }
        });
        let peer = std::sync::Arc::new(crate::peer::Peer::fixed(
            "box",
            &format!("http://127.0.0.1:{port}"),
            "made-up",
        ));
        (peer, heard)
    }

    const OLD: Old = Old {
        too_old: "to do that",
        again: "try again",
    };

    /// [`reconnect`] to "box", drawing on a screen nobody looks at.
    fn back(
        peer: &std::sync::Arc<crate::peer::Peer>,
        path: &str,
        resume: Option<(String, u64)>,
        reopen: bool,
        why: &str,
    ) -> Result<(Conn, bool), String> {
        let lost = Lost {
            machine: "box",
            path,
            old: OLD,
            reopen,
            why,
        };
        let predictor = crate::predict::Predictor::new(crate::predict::Mode::Auto, 24, 80);
        reconnect(peer, lost, resume, &predictor, &mut Vec::new())
            .map(|back| (back.conn, back.resumed))
    }

    /// Coming back with a token asks for the bytes after what was received,
    /// and an answer with a token is a resume.
    #[test]
    fn a_dropped_tab_asks_to_resume_from_what_it_received() {
        let (peer, asked) = fake_sibling(vec![Answer::Upgrade(Some("id.s2"))]);
        let resume = Some(("id.s1".to_string(), 42));
        let (conn, resumed) = back(&peer, "/api/term/claude/a", resume, false, "x").expect("back");
        assert!(resumed);
        assert_eq!(conn.token.as_deref(), Some("id.s2"));
        assert_eq!(asked.recv().unwrap(), "id.s1 42");
    }

    /// A terminal the sibling let go: a session's tab opens afresh at once
    /// (asking for a token for next time), and a new agent's tab — which
    /// must never open its route twice — says where the agent went.
    #[test]
    fn a_resume_that_is_gone_reopens_only_where_that_is_safe() {
        let (peer, asked) = fake_sibling(vec![Answer::Gone, Answer::Upgrade(Some("id.new"))]);
        let resume = Some(("id.old".to_string(), 7));
        let (conn, resumed) =
            back(&peer, "/api/term/claude/a", resume.clone(), true, "x").expect("reopened");
        assert!(!resumed, "a fresh open redraws from nothing");
        assert_eq!(conn.token.as_deref(), Some("id.new"));
        assert_eq!(asked.recv().unwrap(), "id.old 7");
        assert_eq!(asked.recv().unwrap(), crate::ws::RESUME_NEW);

        let (peer, asked) = fake_sibling(vec![Answer::Gone]);
        let why = back(&peer, NEW_ROUTE, resume, false, "x")
            .err()
            .expect("not reopened");
        assert!(why.contains("goes on running there"), "{why}");
        assert!(why.contains("that terminal has ended"), "{why}");
        assert_eq!(asked.recv().unwrap(), "id.old 7");
        assert!(asked.try_recv().is_err(), "asked only once");
    }

    fn lost_box() -> Lost<'static> {
        Lost {
            machine: "box",
            path: "/api/term/claude/a",
            old: OLD,
            reopen: false,
            why: "the connection ended",
        }
    }

    /// A tunnel back within the notice's wait is a drop nobody sees: nothing
    /// is drawn on the tab at all.
    #[test]
    fn a_quick_reconnect_draws_nothing() {
        let (peer, _asked) = fake_sibling(vec![Answer::Away, Answer::Upgrade(Some("id.s2"))]);
        let predictor = crate::predict::Predictor::new(crate::predict::Mode::Auto, 24, 80);
        let mut screen = Vec::new();
        let resume = Some(("id.s1".to_string(), 1));
        let back = reconnect(&peer, lost_box(), resume, &predictor, &mut screen).expect("back");
        assert!(back.resumed);
        assert!(screen.is_empty(), "{}", String::from_utf8_lossy(&screen));
    }

    /// Past the wait, one line goes on the bottom row; once back, that row
    /// is painted as the far side left it, so nothing of it stays.
    #[test]
    fn a_slow_reconnect_says_so_on_one_line_and_takes_it_back() {
        let (peer, _asked) = fake_sibling(vec![
            Answer::Away,
            Answer::Away,
            Answer::Away,
            Answer::Upgrade(Some("id.s2")),
        ]);
        let mut predictor = crate::predict::Predictor::new(crate::predict::Mode::Auto, 24, 80);
        let far = b"\x1b[2J\x1b[24;1Hstatus line\x1b[3;5H";
        predictor.on_output(far, std::time::Instant::now());
        let mut screen = Vec::new();
        let resume = Some(("id.s1".to_string(), 1));
        reconnect(&peer, lost_box(), resume, &predictor, &mut screen).expect("back");
        let said = String::from_utf8_lossy(&screen);
        assert_eq!(
            said.matches("box lost: the connection ended").count(),
            1,
            "{said}"
        );
        assert!(!said.contains('\n'), "nothing scrolls: {said:?}");
        assert!(screen.ends_with(&predictor.unnotice()));
        let mut here = vt100::Parser::new(24, 80, 0);
        here.process(far);
        let before = here.screen().contents_formatted();
        here.process(&screen);
        assert_eq!(here.screen().contents_formatted(), before);
    }

    /// A try that is still blocked when the notice is due does not hold it
    /// back: the tab says it is away while it waits.
    #[test]
    fn a_try_held_up_still_shows_the_notice_on_time() {
        let (peer, _asked) = fake_sibling(vec![Answer::Late("id.s2")]);
        let predictor = crate::predict::Predictor::new(crate::predict::Mode::Auto, 24, 80);
        let mut screen = Vec::new();
        let resume = Some(("id.s1".to_string(), 1));
        let back = reconnect(&peer, lost_box(), resume, &predictor, &mut screen).expect("back");
        assert!(back.resumed);
        let said = String::from_utf8_lossy(&screen);
        assert!(said.contains("box lost: the connection ended"), "{said}");
        assert!(screen.ends_with(&predictor.unnotice()));
    }

    /// Quick tries while a blip is likeliest to be over, then no more than
    /// one every few seconds; four go in before anything is said.
    #[test]
    fn retries_start_quick_and_back_off() {
        let ms = |n| retry_wait(n).as_millis();
        assert_eq!(
            [ms(1), ms(2), ms(3), ms(4), ms(5)],
            [250, 500, 1000, 2000, 4000]
        );
        assert_eq!(retry_wait(6), RETRY_MAX);
        assert_eq!(retry_wait(60), RETRY_MAX);
        let waited = |n: u32| (1..n).map(retry_wait).sum::<std::time::Duration>();
        let silent = (1..)
            .take_while(|&n| waited(n) < std::time::Duration::from_secs(2))
            .count();
        assert_eq!(silent, 4);
    }

    /// A sibling older than resume gives no token: a session's tab reopens
    /// as it always did, and a new agent's tab is not reconnected at all.
    #[test]
    fn a_sibling_older_than_resume_is_reopened_as_before() {
        let (peer, asked) = fake_sibling(vec![Answer::Upgrade(None)]);
        let (conn, resumed) = back(&peer, "/api/term/claude/a", None, true, "x").expect("open");
        assert!(!resumed);
        assert_eq!(conn.token, None);
        assert_eq!(asked.recv().unwrap(), crate::ws::RESUME_NEW);
        let (peer, asked) = fake_sibling(vec![]);
        let why = back(&peer, NEW_ROUTE, None, false, "it dropped")
            .err()
            .expect("not reconnected");
        assert!(why.contains("it dropped"), "{why}");
        assert!(asked.try_recv().is_err(), "nothing was asked");
    }

    #[test]
    fn a_refusal_from_an_old_sibling_says_to_update_it() {
        let old = refusal(
            "pc",
            404,
            "404 Not Found: no such page\n",
            "to do that",
            "try again",
        );
        assert!(old.contains("cctop --update"), "{old}");
        assert!(old.contains("too old to do that"), "{old}");
        let gone = refusal(
            "pc",
            404,
            "404 Not Found: no such session on this machine\n",
            "to do that",
            "try again",
        );
        assert!(gone.contains("no such session"), "{gone}");
    }

    fn spec(agent: &str, dir: &str) -> NewAgent {
        NewAgent {
            agent: agent.into(),
            dir: dir.into(),
            options: crate::launch_opts::Options::default(),
            account: None,
        }
    }

    /// The route carries every part of the launch in its path, where the
    /// signature covers it, and reads back to the same launch — including a
    /// model and a directory that hold slashes and spaces.
    #[test]
    fn a_new_agent_survives_its_route_and_its_command_line() {
        let launch = NewAgent {
            agent: "opencode".into(),
            dir: "/srv/my project/api".into(),
            options: crate::launch_opts::Options {
                model: Some("anthropic/claude-sonnet-4".into()),
                permissions: crate::launch_opts::Permissions::Ask,
            },
            account: None,
        };
        let route = launch.route();
        let rest = route.strip_prefix(NEW_ROUTE).expect("prefix");
        assert_eq!(rest.split('/').count(), 4, "{route}");
        assert!(!rest.contains(' '), "{route}");
        assert_eq!(NewAgent::from_route(rest), Ok(launch.clone()));
        assert_eq!(NewAgent::from_args(&launch.args()), Ok(launch.clone()));

        let home = spec("claude", "");
        assert!(
            home.route().ends_with("/claude/ask/-/-"),
            "{}",
            home.route()
        );
        assert_eq!(
            NewAgent::from_route(home.route().strip_prefix(NEW_ROUTE).unwrap()),
            Ok(home)
        );
    }

    /// Whatever arrives is checked before it is anything: an agent cctop does
    /// not start, a directory that is not spelled as one, a setting the agent
    /// does not have, hex that is not hex, a path of the wrong shape.
    #[test]
    fn a_hostile_launch_is_refused_before_it_starts_anything() {
        for bad in [
            spec("sh", "~"),
            spec("-x", "~"),
            spec("claude", "relative/dir"),
            spec("claude", "-oProxyCommand=x"),
            spec("claude", "~/a\nb"),
            NewAgent {
                options: crate::launch_opts::Options {
                    model: None,
                    permissions: crate::launch_opts::Permissions::Plan,
                },
                ..spec("codex", "~")
            },
            NewAgent {
                options: crate::launch_opts::Options {
                    model: Some("--dangerously-skip-permissions".into()),
                    permissions: crate::launch_opts::Permissions::Ask,
                },
                ..spec("claude", "~")
            },
        ] {
            assert!(bad.check().is_err(), "{bad:?}");
            assert!(
                new_argv("box", Path::new("/usr/bin/cctop"), &bad).is_err(),
                "{bad:?}"
            );
        }
        for rest in [
            "claude/ask/-",
            "claude/ask/-/-/x",
            "claude/root/-/-",
            "claude/ask/zz/-",
            "claude/ask/-/abc",
            "claude/ask/-/ff",
            "sh/ask/-/-",
        ] {
            assert!(NewAgent::from_route(rest).is_err(), "{rest}");
        }
        assert!(new_argv("-x", Path::new("/usr/bin/cctop"), &spec("claude", "~")).is_err());
    }

    /// An account rides last, in hex, on a route of its own — and reads back
    /// from the route and from the command line to the same launch.
    #[test]
    fn an_account_survives_its_route_and_its_command_line() {
        let launch = NewAgent {
            options: crate::launch_opts::Options {
                model: Some("opus".into()),
                permissions: crate::launch_opts::Permissions::Plan,
            },
            account: Some("work.2".into()),
            ..spec("claude", "~/src")
        };
        let route = launch.route();
        let rest = route.strip_prefix(NEW_AS_ROUTE).expect("prefix");
        assert_eq!(rest.split('/').count(), 5, "{route}");
        assert!(rest.ends_with(&format!("/{}", hex("work.2"))), "{route}");
        assert_eq!(NewAgent::from_route_as(rest), Ok(launch.clone()));
        assert_eq!(NewAgent::from_args(&launch.args()), Ok(launch.clone()));
        assert_eq!(
            launch.args()[launch.args().len() - 2..],
            ["--account", "work.2"]
        );
        // Each prefix reads only its own shape: no account where one is
        // promised, and none smuggled onto the route that has none.
        assert!(NewAgent::from_route(rest).is_err());
        let bare = spec("claude", "~/src").route();
        let bare = bare.strip_prefix(NEW_ROUTE).expect("prefix");
        assert!(NewAgent::from_route_as(bare).is_err());
    }

    /// What a cctop older than accounts does with a launch under one: its
    /// router knows [`NEW_ROUTE`] at most, which the account's route does not
    /// start with, so it answers "no such page" — and the tab says to update
    /// it, rather than the launch going ahead under that machine's default.
    /// A launch with no account is still the route it always was.
    #[test]
    fn an_old_sibling_never_starts_an_account_launch_under_another_account() {
        let launch = NewAgent {
            account: Some("work".into()),
            ..spec("claude", "~")
        };
        let route = launch.route();
        assert!(!route.starts_with(NEW_ROUTE), "{route}");
        assert!(!NEW_AS_ROUTE.starts_with(NEW_ROUTE));
        assert!(spec("claude", "~").route().starts_with(NEW_ROUTE));
        let old = refusal(
            "box",
            404,
            "404 Not Found: no such page\n",
            "to start agents for another machine",
            "start it again",
        );
        assert!(old.contains("cctop --update"), "{old}");
    }

    /// An account name is checked as strictly as a model: one a far `cctop
    /// as` could read as an option, or that is not a name at all, goes
    /// nowhere; so does an account for an agent that is not started under
    /// one.
    #[test]
    fn a_hostile_account_is_refused_before_it_starts_anything() {
        for bad in ["", "-x", "--help", ".hidden", "a b", "a/b", "$(x)", "a\nb"] {
            let launch = NewAgent {
                account: Some(bad.into()),
                ..spec("claude", "~")
            };
            assert!(launch.check().is_err(), "{bad:?}");
            assert!(
                new_argv("box", Path::new("/usr/bin/cctop"), &launch).is_err(),
                "{bad:?}"
            );
        }
        let no_accounts = NewAgent {
            account: Some("work".into()),
            ..spec("opencode", "~")
        };
        assert!(no_accounts.check().is_err());
        assert!(account_ok("work-2_x.y"));
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // `--account` only last, once, and with a value.
        assert!(NewAgent::from_args(&args(&["claude", "~", "--account"])).is_err());
        assert!(
            NewAgent::from_args(&args(&["claude", "~", "--account", "a", "--model", "opus"]))
                .is_err()
        );
        assert!(
            NewAgent::from_args(&args(&["claude", "~", "--account", "a", "--account", "b"]))
                .is_err()
        );
    }

    /// The far side starts it only under an account it has: an unknown name
    /// is refused, never swapped for the default; a known one is that
    /// profile; none is the default.
    #[test]
    fn the_far_side_starts_only_under_an_account_it_has() {
        crate::config::add_launchable(crate::config::Profile {
            provider: crate::pricing::Provider::Claude,
            name: "cctop-test-remote-acct".into(),
            dir: std::env::temp_dir().join(".claude-cctop-test-remote-acct"),
            source: crate::config::AccountSource::Directory,
        });
        let under = |name: &str| NewAgent {
            account: Some(name.into()),
            ..spec("claude", "")
        };
        let found = under("cctop-test-remote-acct")
            .profile_here()
            .expect("known")
            .expect("a profile");
        assert_eq!(found.name, "cctop-test-remote-acct");
        let why = under("cctop-test-no-such-acct")
            .profile_here()
            .expect_err("unknown");
        assert!(why.contains("no claude account"), "{why}");
        // Codex has its own accounts; Claude's name is not one of them.
        let codex = NewAgent {
            account: Some("cctop-test-remote-acct".into()),
            ..spec("codex", "")
        };
        assert!(codex.profile_here().is_err());
        assert_eq!(spec("claude", "").profile_here(), Ok(None));
        assert!(
            new_here(&under("cctop-test-no-such-acct").args())
                .expect_err("refused")
                .contains("no claude account")
        );
    }

    #[test]
    fn a_new_agent_tab_runs_this_cctops_launch_bridge() {
        let launch = NewAgent {
            options: crate::launch_opts::Options {
                model: Some("opus".into()),
                permissions: crate::launch_opts::Permissions::Plan,
            },
            ..spec("claude", "~/src/api")
        };
        assert_eq!(
            new_argv("computer36", Path::new("/usr/bin/cctop"), &launch).expect("argv"),
            [
                "/usr/bin/cctop",
                "mux",
                "launch",
                "computer36",
                "claude",
                "~/src/api",
                "--model",
                "opus",
                "--permissions",
                "plan"
            ]
        );
    }

    /// An ssh that fails keeps the tab until Enter and says what ssh said and
    /// how it ended, then closes the tab cleanly; one that ends cleanly closes the
    /// tab at once and says nothing.
    #[test]
    fn a_failed_ssh_holds_the_tab_open_and_a_clean_one_does_not() {
        use std::io::Write;
        let run = |script: &str| {
            let mut child = std::process::Command::new("sh")
                .args(["-c", HOLD_SCRIPT, "cctop-hold", "sh", "-c", script])
                .stdin(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("sh");
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(b"\n")
                .expect("enter");
            child.wait_with_output().expect("output")
        };
        let failed = run("echo 'Connection refused' >&2; exit 255");
        assert!(failed.status.success(), "dismissed, it closes the tab");
        let said = String::from_utf8_lossy(&failed.stderr);
        assert!(said.contains("Connection refused"), "{said}");
        assert!(said.contains("sh exited 255. Press Enter"), "{said}");
        let clean = run("echo bye >&2; exit 0");
        assert!(clean.status.success());
        assert!(clean.stderr.is_empty(), "{:?}", clean.stderr);
    }
}
