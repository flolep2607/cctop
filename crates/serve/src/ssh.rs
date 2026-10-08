//! The web launcher's way to a remote folder: the ssh hosts, completion of
//! `host:path`, and whether a folder there can be worked in.
//!
//! The same questions the terminal's directory field asks (see `location.rs`
//! in `cctop-ui`), asked here on a browser's behalf. That makes every one of
//! them an ssh connection started by an HTTP request, so the rules are:
//!
//! - **The full token only.** Every route here goes through `may_act`, like
//!   `/api/launch`: a read-only link, a `--no-actions` serve and anything but
//!   a JSON POST are refused before ssh is touched. That includes the host
//!   list — the names in `~/.ssh/config` are internal hostnames, and a
//!   read-only link is one that is handed around.
//! - **Never a prompt.** Connections are `ssh_master::connect`'s: BatchMode, no
//!   terminal, stdin from `/dev/null`. A host that wants a password, a
//!   passphrase or a host-key answer comes back offline with that reason, and
//!   the launch goes ahead anyway in an rmux session, where the sandbox asks on
//!   the session page's terminal. No secret ever goes through this server.
//! - **Let go of at exit.** A connect takes a lease on the shared master, and
//!   `ssh_master::release_all` gives every lease back — the dashboard's exit
//!   already calls it, and a standalone `cctop serve` does on SIGINT, SIGTERM
//!   and SIGHUP (see `release_on_signal`).
//!
//! The ssh and the connect are held as trait objects so the tests stand a
//! local `sh` in for the host and never reach one.

use crate::http::{self, Request};
use cctop_core::remote_fs::{self, Listing};
use cctop_core::remote_launch;
use cctop_core::ssh_config;
use cctop_core::ssh_master::{self, Runner};
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long an offline answer is kept before the host is tried again. Long
/// enough that a page typing after a host that wants a password does not start
/// a connect per keystroke, short enough that unlocking the key and typing on
/// gets through.
const OFFLINE_KEPT: Duration = Duration::from_secs(10);

/// The most a page is offered at once: more than the terminal's six, since a
/// page has the room, and few enough to read.
const MAX_HITS: usize = 12;

/// The longest path a request may name. A path is at most 4096 bytes on any
/// host this will meet.
const PATH_MAX: usize = 4096;

/// Connects to an ssh host, the master taken on lease: `ssh_master::connect`
/// in the server.
type Connect = dyn Fn(&str) -> Result<(), String> + Send + Sync;

/// The hosts on offer: `ssh_config::hosts` in the server.
type Hosts = dyn Fn() -> Vec<ssh_config::Host> + Send + Sync;

/// What this server can reach over ssh, and what it has learned of each host.
pub struct Reach {
    runner: Arc<dyn Runner>,
    connect: Box<Connect>,
    hosts: Box<Hosts>,
    known: Mutex<HashMap<String, Arc<Mutex<HostState>>>>,
}

#[derive(Default)]
struct HostState {
    conn: Option<Conn>,
    /// The repository scan's answer, once it lands. Asked once per host per
    /// run on a thread of its own — it walks the home, and a page typing a path
    /// should not wait on it.
    repos: Option<Vec<String>>,
    scanning: bool,
}

enum Conn {
    Ready { home: String },
    Offline { why: String, at: Instant },
}

/// What the page is told about a host it asked about.
#[derive(serde::Serialize)]
struct Offline {
    ready: bool,
    why: String,
    /// The host works, only not without asking — so the launch tab will ask.
    prompt: bool,
}

impl Reach {
    /// ssh for real: the shared master, the user's `~/.ssh/config`.
    pub fn ssh() -> Reach {
        Reach {
            runner: Arc::new(ssh_master::Ssh),
            connect: Box::new(|host| ssh_master::connect(host).map(|_| ())),
            hosts: Box::new(ssh_config::hosts),
            known: Mutex::default(),
        }
    }

    /// For tests: commands run by `runner`, connects answered by `connect`,
    /// and `hosts` on offer. Nothing here reaches ssh unless they do.
    #[cfg(test)]
    pub fn with(
        runner: Arc<dyn Runner>,
        connect: Box<Connect>,
        hosts: Vec<ssh_config::Host>,
    ) -> Reach {
        Reach {
            runner,
            connect,
            hosts: Box::new(move || hosts.clone()),
            known: Mutex::default(),
        }
    }

    /// A reach that reaches nothing, for tests that never ask it to.
    #[cfg(test)]
    pub fn nowhere() -> Reach {
        struct Refuse;
        impl Runner for Refuse {
            fn run(&self, _: &str, _: &str, _: Duration) -> Result<String, String> {
                Err("no ssh in tests".into())
            }
        }
        Reach::with(
            Arc::new(Refuse),
            Box::new(|_| Err("no ssh in tests".into())),
            Vec::new(),
        )
    }

    fn state(&self, host: &str) -> Arc<Mutex<HostState>> {
        let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(known.entry(host.to_string()).or_default())
    }

    /// The host's home once commands run there, or why they do not.
    ///
    /// Connects when it has not yet, under the host's own lock so two requests
    /// for one host start one master. Blocks for up to the connect deadline;
    /// each request has a thread of its own.
    fn home(&self, host: &str) -> Result<String, String> {
        let state = self.state(host);
        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
        match &state.conn {
            Some(Conn::Ready { home }) => return Ok(home.clone()),
            Some(Conn::Offline { why, at }) if at.elapsed() < OFFLINE_KEPT => {
                return Err(why.clone());
            }
            _ => {}
        }
        let result =
            (self.connect)(host).and_then(|()| remote_fs::home(self.runner.as_ref(), host));
        state.conn = Some(match &result {
            Ok(home) => Conn::Ready { home: home.clone() },
            Err(why) => Conn::Offline {
                why: why.clone(),
                at: Instant::now(),
            },
        });
        result
    }

    /// The repositories found so far, starting the scan the first time.
    fn repos(&self, host: &str) -> (Option<Vec<String>>, bool) {
        let state = self.state(host);
        let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
        if guard.repos.is_none() && !guard.scanning {
            guard.scanning = true;
            let (runner, host, state) = (
                Arc::clone(&self.runner),
                host.to_string(),
                Arc::clone(&state),
            );
            let _ = std::thread::Builder::new()
                .name("cctop-serve-repos".into())
                .spawn(move || {
                    let found = remote_fs::repos(runner.as_ref(), &host).unwrap_or_default();
                    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                    state.repos = Some(found);
                    state.scanning = false;
                });
        }
        (guard.repos.clone(), guard.scanning)
    }
}

fn offline(why: String) -> Offline {
    Offline {
        ready: false,
        prompt: why.contains("prompt") || why.contains("host key"),
        why,
    }
}

/// `host` and `path` from a request body, or the 400 that says what is wrong.
fn target(body: &serde_json::Value) -> Result<(String, String), String> {
    let field = |name: &str| {
        body.get(name)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let (host, path) = (field("host"), field("path"));
    if !remote_launch::host_ok(&host) {
        return Err(format!("{host:?} is not an ssh host name"));
    }
    if path.len() > PATH_MAX {
        return Err("that path is longer than any host allows".into());
    }
    Ok((host, path))
}

fn json<T: serde::Serialize>(stream: &mut TcpStream, request: &Request, value: &T) {
    let body = serde_json::to_string(value).unwrap_or_default();
    http::respond(
        stream,
        Some(request),
        200,
        "application/json; charset=utf-8",
        body.as_bytes(),
    );
}

/// `POST /api/ssh/<what>`, already past `may_act`.
pub fn route(
    reach: &Reach,
    stream: &mut TcpStream,
    request: &Request,
    what: &str,
    body: &serde_json::Value,
) {
    match what {
        "hosts" => hosts(reach, stream, request),
        "complete" => complete(reach, stream, request, body),
        "check" => check(reach, stream, request, body),
        _ => http::respond_error(stream, Some(request), 404, "no such route"),
    }
}

/// The hosts in `~/.ssh/config`, names and aliases only.
fn hosts(reach: &Reach, stream: &mut TcpStream, request: &Request) {
    let hosts: Vec<serde_json::Value> = (reach.hosts)()
        .into_iter()
        .filter(|h| remote_launch::host_ok(&h.name))
        .map(|h| serde_json::json!({"name": h.name, "aliases": h.aliases}))
        .collect();
    json(stream, request, &serde_json::json!({ "hosts": hosts }));
}

/// What to offer under `host:path`: the directory being typed in, listed on
/// the host, and for a bare name the repositories its home holds.
fn complete(reach: &Reach, stream: &mut TcpStream, request: &Request, body: &serde_json::Value) {
    let (host, path) = match target(body) {
        Ok(t) => t,
        Err(why) => return http::respond_error(stream, Some(request), 400, &why),
    };
    let home = match reach.home(&host) {
        Ok(home) => home,
        Err(why) => return json(stream, request, &offline(why)),
    };
    let (dir, _) = remote_launch::split_remote(&path);
    let (listing, missing, failed): (Option<Listing>, bool, Option<String>) =
        match remote_fs::list(reach.runner.as_ref(), &host, &dir) {
            Ok(Some(listing)) => (Some(listing), false, None),
            Ok(None) => (None, true, None),
            Err(why) => (None, false, Some(why)),
        };
    let (repos, scanning) = match path.contains('/') {
        true => (None, false),
        false => reach.repos(&host),
    };
    let mut hits = remote_launch::suggest(&path, repos.as_deref(), listing.as_ref());
    hits.truncate(MAX_HITS);
    json(
        stream,
        request,
        &serde_json::json!({
            "ready": true,
            "home": home,
            "dir": dir,
            "missing": missing,
            "failed": failed,
            "scanning": scanning,
            "hits": hits,
        }),
    );
}

/// Whether `host:path` can be a sandbox's working directory, in
/// `remote_fs::verdict`'s words.
fn check(reach: &Reach, stream: &mut TcpStream, request: &Request, body: &serde_json::Value) {
    let (host, path) = match target(body) {
        Ok(t) => t,
        Err(why) => return http::respond_error(stream, Some(request), 400, &why),
    };
    if let Err(why) = reach.home(&host) {
        return json(stream, request, &offline(why));
    }
    let path = match path.as_str() {
        "" => "~".to_string(),
        _ => path,
    };
    match remote_fs::check(reach.runner.as_ref(), &host, &path) {
        Ok(facts) => json(
            stream,
            request,
            &serde_json::json!({
                "ready": true,
                "problem": remote_fs::verdict(&facts).err(),
                "resolved": facts.resolved,
            }),
        ),
        Err(why) => json(
            stream,
            request,
            &offline(format!("couldn't check on {host}: {why}")),
        ),
    }
}

/// Why `host:path` should not be launched in, when the host can be asked
/// without a prompt and says so. A host that cannot be asked is no reason to
/// refuse: the sandbox connects in the rmux session, asks there, and checks
/// the same things before it mounts anything.
pub fn launch_problem(reach: &Reach, host: &str, path: &str) -> Option<String> {
    reach.home(host).ok()?;
    let path = match path.trim() {
        "" => "~",
        path => path,
    };
    let facts = remote_fs::check(reach.runner.as_ref(), host, path).ok()?;
    remote_fs::verdict(&facts)
        .err()
        .map(|why| format!("can't work in {host}:{path}: {why}"))
}

/// Give back every master this process took a lease on when a standalone
/// `cctop serve` is stopped, then exit as the signal would have.
///
/// A self-pipe, because the handler itself may do nothing but write: letting
/// go runs `ssh -O exit`, which is a process and a lock. The dashboard does
/// not need this — its own exit calls `release_all`.
pub fn release_on_signal() {
    static PIPE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
    extern "C" fn on_signal(signal: libc::c_int) {
        let fd = PIPE.load(std::sync::atomic::Ordering::Relaxed);
        if fd >= 0 {
            let byte = signal as u8;
            // SAFETY: write(2) is async-signal-safe, and the byte outlives it.
            unsafe { libc::write(fd, (&raw const byte).cast(), 1) };
        }
    }
    let mut fds = [-1; 2];
    // SAFETY: a two-int array for pipe2 to fill.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return;
    }
    PIPE.store(fds[1], std::sync::atomic::Ordering::Relaxed);
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: the handler only performs a write(2).
        unsafe {
            libc::signal(
                signal,
                on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t,
            )
        };
    }
    let read_fd = fds[0];
    let _ = std::thread::Builder::new()
        .name("cctop-serve-signal".into())
        .spawn(move || {
            let mut byte = [0u8; 1];
            // SAFETY: the read end is ours alone and outlives the thread.
            if unsafe { libc::read(read_fd, byte.as_mut_ptr().cast(), 1) } == 1 {
                ssh_master::release_all();
                std::process::exit(128 + i32::from(byte[0]));
            }
        });
}

/// A host that is this machine's `sh` in a temporary home, and connects
/// that are counted rather than made.
#[cfg(test)]
pub(crate) fn local_host(online: bool) -> (tempfile::TempDir, Reach, Arc<Mutex<u32>>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().canonicalize().expect("canonical");
    let connects = Arc::new(Mutex::new(0));
    let counted = Arc::clone(&connects);
    let reach = Reach::with(
        Arc::new(cctop_core::remote_fs::LocalSh { home }),
        Box::new(move |_| {
            *counted.lock().expect("count") += 1;
            match online {
                true => Ok(()),
                false => Err("needs a password or key prompt".into()),
            }
        }),
        vec![ssh_config::Host {
            name: "devbox".into(),
            aliases: vec!["dev".into()],
        }],
    );
    (dir, reach, connects)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_that_would_prompt_is_offline_and_not_asked_again_at_once() {
        let (_keep, reach, connects) = local_host(false);
        assert_eq!(
            reach.home("devbox"),
            Err("needs a password or key prompt".into())
        );
        assert!(reach.home("devbox").is_err());
        assert_eq!(*connects.lock().expect("count"), 1);
        assert!(offline("needs a password or key prompt".into()).prompt);
        assert!(!offline("unknown host".into()).prompt);
    }

    #[test]
    fn a_launch_into_an_unusable_folder_says_why_and_an_offline_host_is_let_through() {
        let (keep, reach, _) = local_host(true);
        std::fs::create_dir(keep.path().join("work")).expect("mkdir");
        assert_eq!(launch_problem(&reach, "devbox", "~/work"), None);
        let why = launch_problem(&reach, "devbox", "~/gone").expect("refused");
        assert!(why.contains("does not exist on the host"), "{why}");
        let (_keep, offline, _) = local_host(false);
        assert_eq!(launch_problem(&offline, "devbox", "~/gone"), None);
    }

    #[test]
    fn a_request_names_a_host_not_an_ssh_option() {
        let body = |host: &str| serde_json::json!({"host": host, "path": "~"});
        assert!(target(&body("devbox")).is_ok());
        assert!(target(&body("-oProxyCommand=touch /tmp/x")).is_err());
        assert!(target(&body("")).is_err());
        assert!(target(&serde_json::json!({"host": "a", "path": "x".repeat(5000)})).is_err());
    }
}
