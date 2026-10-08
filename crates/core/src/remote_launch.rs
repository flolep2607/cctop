//! Reading `host:path` and turning it into a launch on that host — the parts
//! both launchers need, the terminal's directory field and the web page's.
//!
//! They lived in the terminal UI until the web launcher wanted the same
//! answers, and `cctop-serve` does not depend on `cctop-ui`. What is here is
//! pure: what the text means, what to suggest under it given what the host
//! said, and the argv that starts an agent there. Asking the host is
//! [`remote_fs`](crate::remote_fs)'s; connecting to it is
//! [`ssh_master`](crate::ssh_master)'s.

use crate::remote_fs::Listing;
use crate::sandbox::sh_quote;

/// What a directory field's text names.
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
/// may not start that way. An IPv6 address is written `[addr]:path`. A "host"
/// that [`host_ok`] refuses is not one, so the text stays local.
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
        Some((host, path)) if host_ok(host) => Typed::Remote { host, path },
        _ => Typed::Local(text),
    }
}

/// Whether `host` may be handed to ssh as its destination.
///
/// The destination is a positional argument, and ssh reads one that starts
/// with `-` as an option — `-oProxyCommand=…` is a command line run here. The
/// web launcher takes the host from a request, so this is the line between
/// "a host name" and "an ssh option somebody typed", and it is drawn in the
/// parser so neither launcher can get past it.
pub fn host_ok(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 255
        && !host.starts_with('-')
        && !host.contains('/')
        && !host.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Split a remote path being typed into the directory to list and the start of
/// a name in it. A path with no slash is a name in the home, as `scp host:x`
/// reads it.
pub fn split_remote(path: &str) -> (String, String) {
    match path.rfind('/') {
        None => ("~".to_string(), path.to_string()),
        Some(0) => ("/".to_string(), path[1..].to_string()),
        Some(at) => (path[..at].to_string(), path[at + 1..].to_string()),
    }
}

/// `name` inside `dir`, spelled the way the field spells paths.
pub fn join_remote(dir: &str, name: &str) -> String {
    match dir {
        "/" => format!("/{name}"),
        dir => format!("{}/{name}", dir.trim_end_matches('/')),
    }
}

/// One directory on a host worth offering, and why it cannot be used if it
/// cannot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Suggestion {
    pub path: String,
    pub problem: Option<String>,
}

/// What to offer under `path` (as typed after `host:`), given the host's
/// repositories and the listing of the directory being typed in, as far as
/// either is known.
///
/// A bare name is matched against the repositories as well as the home's own
/// directories: what is remembered about a project is its name. The
/// repositories come first and in the scan's order — newest first — and a
/// listed directory that is one of them is not offered twice. Hidden
/// directories only when the name being typed starts with a dot.
pub fn suggest(path: &str, repos: Option<&[String]>, listing: Option<&Listing>) -> Vec<Suggestion> {
    let path = path.trim();
    let (dir, fragment) = split_remote(path);
    let wanted = fragment.to_lowercase();
    let mut out: Vec<Suggestion> = Vec::new();
    if !path.contains('/')
        && let Some(repos) = repos
    {
        out.extend(
            repos
                .iter()
                .filter(|r| r.to_lowercase().contains(&wanted))
                .map(|r| Suggestion {
                    path: r.clone(),
                    // Not listed with its permissions: the check on taking it
                    // asks.
                    problem: None,
                }),
        );
    }
    for entry in listing.map(|l| l.dirs.as_slice()).unwrap_or_default() {
        if entry.name.starts_with('.') && !fragment.starts_with('.') {
            continue;
        }
        if !entry.name.to_lowercase().starts_with(&wanted) {
            continue;
        }
        let path = join_remote(&dir, &entry.name);
        if out.iter().any(|s| s.path == path) {
            continue;
        }
        let problem = match (entry.readable, entry.writable) {
            (false, _) => Some("not readable on the host".to_string()),
            (_, false) => Some("read-only on the host".to_string()),
            _ => None,
        };
        out.push(Suggestion { path, problem });
    }
    out
}

/// The command a sandboxed launch runs, as an argv for a pane or an rmux
/// session.
///
/// Through `env`, which is how every launch here already carries a variable,
/// and which the tab labeller knows to look past. The hold variable keeps a
/// setup failure on screen instead of closing the tab on it — which is also
/// where a host that wants a password asks for it, since the sandbox's own
/// connect has a terminal to ask on.
pub fn sandbox_argv(exe: &std::path::Path, agent: &str, host: &str, path: &str) -> Vec<String> {
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
pub fn shell_argv(host: &str, path: &str) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_fs::Entry;

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

    /// ssh reads a destination that starts with a dash as an option, and
    /// `-oProxyCommand=` runs a command here: never a host.
    #[test]
    fn an_ssh_option_is_never_read_as_a_host() {
        for text in [
            "-oProxyCommand=touch /tmp/x:~",
            "-F:x",
            "a b:~",
            "a\tb:~",
            "a\u{7}:~",
        ] {
            assert!(matches!(parse(text), Typed::Local(_)), "{text:?}");
        }
        assert!(host_ok("me@box.example.com"));
        assert!(!host_ok("-oProxyCommand=x"));
        assert!(!host_ok(""));
        assert!(!host_ok(&"h".repeat(256)));
    }

    #[test]
    fn suggestions_put_the_repositories_first_and_mark_what_cannot_be_used() {
        let entry = |name: &str, readable, writable| Entry {
            name: name.to_string(),
            readable,
            writable,
        };
        let listing = Listing {
            resolved: "/home/f".into(),
            dirs: vec![
                entry(".cache", true, true),
                entry("api", true, true),
                entry("apt", true, false),
                entry("locked", false, false),
            ],
        };
        let repos = ["~/code/api".to_string(), "~/api".to_string()];
        let paths = |path: &str| -> Vec<(String, Option<String>)> {
            suggest(path, Some(&repos), Some(&listing))
                .into_iter()
                .map(|s| (s.path, s.problem))
                .collect()
        };
        assert_eq!(
            paths("ap"),
            [
                ("~/code/api".into(), None),
                ("~/api".into(), None),
                ("~/apt".into(), Some("read-only on the host".into())),
            ]
        );
        assert_eq!(
            paths("lo"),
            [("~/locked".into(), Some("not readable on the host".into()))]
        );
        assert_eq!(paths(".c"), [("~/.cache".into(), None)]);
        // Past a slash it is a directory being browsed: no repositories.
        assert_eq!(
            suggest("~/a", Some(&repos), Some(&listing))
                .iter()
                .map(|s| s.path.as_str())
                .collect::<Vec<_>>(),
            ["~/api", "~/apt"]
        );
        assert!(suggest("x", None, None).is_empty());
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
}
