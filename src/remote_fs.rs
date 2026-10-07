//! What the launcher asks a host about its directories, and whether one of
//! them can be used as a sandbox's working directory.
//!
//! Three questions, each one command line over the host's shared master (see
//! [`ssh_master`](crate::ssh_master)): what is in this directory, where are the
//! git repositories under the home, and is this directory usable. Every one is
//! plain POSIX `sh` — the host's login shell may be anything, and nothing is
//! installed there — and every one is bounded, by a timeout here and by a cap
//! on what it prints.
//!
//! The usability rules live here too, in [`verdict`], because both ends need
//! them: the field marks a suggestion it could not launch in, and `cctop
//! sandbox` refuses the same directory for the same reason before it mounts
//! anything.

use crate::ssh_master::Runner;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// How long one directory listing may take. Short, because someone is typing
/// and waiting on it; a host that takes longer is reported as slow rather than
/// waited for.
pub const LIST_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the repository scan may take. It walks three levels of the home
/// directory, which on a large one is a few seconds of `stat`.
pub const REPOS_TIMEOUT: Duration = Duration::from_secs(10);

/// Entries one listing returns at most: a directory with thousands of children
/// is not being browsed by eye, and the field shows six.
const LIST_CAP: usize = 500;

/// Repositories the scan returns at most.
const REPOS_CAP: usize = 200;

/// Expand a leading `~` on the host, where the home it means is. Shared by
/// every script below, which all take the path as typed in `$1`.
const EXPAND: &str = r#"p=$1
case $p in "~"|"") p=$HOME ;; "~/"*) p=$HOME/${p#"~/"} ;; esac"#;

/// `text` as one argument to `sh -c <script> cctop <text>`, as a command line
/// for the host's login shell.
fn script_line(script: &str, arg: &str) -> String {
    use crate::sandbox::sh_quote;
    format!("exec sh -c {} cctop {}", sh_quote(script), sh_quote(arg))
}

/// One entry of a remote directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// Readable and searchable: something can be listed and run in it.
    pub readable: bool,
    pub writable: bool,
}

/// A remote directory's subdirectories, and the name the host resolves it to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listing {
    /// The directory, absolute and with symlinks resolved: where the mount
    /// would go, so where the local rules have to be checked.
    pub resolved: String,
    pub dirs: Vec<Entry>,
}

/// List the subdirectories of `dir` (as typed: `~`, `~/src`, `/srv`) on
/// `host`. `Ok(None)` is a directory that is not there, which is an answer
/// rather than a failure.
pub fn list(runner: &dyn Runner, host: &str, dir: &str) -> Result<Option<Listing>, String> {
    // A glob rather than `find -printf` or `ls`: the first is GNU only, the
    // second marks a symlink to a directory as a link. `*/`-style tests on the
    // glob's own results follow links, which is what a cd into one does.
    let script = format!(
        r#"{EXPAND}
cd -- "$p" 2>/dev/null || {{ echo missing; exit 0; }}
printf 'dir=%s\n' "$(pwd -P)"
n=0
for f in * .*; do
  case $f in .|..) continue ;; esac
  [ -d "$f" ] || continue
  m=-; [ -r "$f" ] && [ -x "$f" ] && m=r
  [ -w "$f" ] && m=${{m}}w
  printf '%s\t%s\n' "$m" "$f"
  n=$((n+1)); [ $n -ge {LIST_CAP} ] && break
done
exit 0"#
    );
    let out = runner.run(host, &script_line(&script, dir), LIST_TIMEOUT)?;
    Ok(parse_listing(&out))
}

fn parse_listing(out: &str) -> Option<Listing> {
    let mut lines = out.lines();
    let resolved = lines.next()?.strip_prefix("dir=")?.to_string();
    let mut dirs: Vec<Entry> = lines
        .filter_map(|line| {
            let (mode, name) = line.split_once('\t')?;
            (!name.is_empty()).then(|| Entry {
                name: name.to_string(),
                readable: mode.starts_with('r'),
                writable: mode.ends_with('w'),
            })
        })
        .collect();
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    Some(Listing { resolved, dirs })
}

/// The git repositories under the host's home, as `~/…` paths.
///
/// The same shape as the local scan — three levels, hidden directories
/// skipped, a repository's own subdirectories not offered — so the two halves
/// of the field read as one list. `timeout` where the host has it, so a scan
/// cut off here does not go on walking there.
pub fn repos(runner: &dyn Runner, host: &str) -> Result<Vec<String>, String> {
    let script = format!(
        r#"cd || exit 0
t=; command -v timeout >/dev/null 2>&1 && t="timeout {secs}"
$t find . -mindepth 1 -maxdepth 4 -name .git -print -prune -o -name '.*' -prune 2>/dev/null | head -n {REPOS_CAP}"#,
        secs = REPOS_TIMEOUT.as_secs().saturating_sub(1).max(1),
    );
    let out = runner.run(host, &script_line(&script, ""), REPOS_TIMEOUT)?;
    Ok(parse_repos(&out))
}

fn parse_repos(out: &str) -> Vec<String> {
    let mut found: Vec<String> = out
        .lines()
        .filter_map(|line| {
            let rel = line.strip_prefix("./")?.strip_suffix("/.git")?;
            (!rel.is_empty()).then(|| format!("~/{rel}"))
        })
        .collect();
    found.sort();
    // A repository inside one already listed is that one's business.
    let mut kept: Vec<String> = Vec::new();
    for repo in found {
        if !kept
            .iter()
            .any(|outer| repo.starts_with(&format!("{outer}/")))
        {
            kept.push(repo);
        }
    }
    kept
}

/// What the host says about one directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteFacts {
    pub exists: bool,
    pub is_dir: bool,
    pub readable: bool,
    pub writable: bool,
    /// Absolute, symlinks resolved, when it is a directory that could be
    /// entered.
    pub resolved: Option<String>,
}

/// Ask the host about `path` (as typed).
pub fn check(runner: &dyn Runner, host: &str, path: &str) -> Result<RemoteFacts, String> {
    let script = format!(
        r#"{EXPAND}
[ -e "$p" ] && echo exists
[ -d "$p" ] && echo dir
[ -r "$p" ] && [ -x "$p" ] && echo readable
[ -w "$p" ] && echo writable
cd -- "$p" 2>/dev/null && printf 'root=%s\n' "$(pwd -P)"
exit 0"#
    );
    let out = runner.run(host, &script_line(&script, path), LIST_TIMEOUT)?;
    Ok(parse_check(&out))
}

pub fn parse_check(out: &str) -> RemoteFacts {
    let mut facts = RemoteFacts::default();
    for line in out.lines() {
        match line {
            "exists" => facts.exists = true,
            "dir" => facts.is_dir = true,
            "readable" => facts.readable = true,
            "writable" => facts.writable = true,
            _ => {
                if let Some(root) = line.strip_prefix("root=").filter(|r| r.starts_with('/')) {
                    facts.resolved = Some(root.to_string());
                }
            }
        }
    }
    facts
}

/// The host's home directory, which is also the first thing asked over a new
/// master — the proof that commands run there, not only that ssh connected.
pub fn home(runner: &dyn Runner, host: &str) -> Result<String, String> {
    let out = runner.run(host, "printf '%s\\n' \"$HOME\"", LIST_TIMEOUT)?;
    let home = out.lines().next().unwrap_or_default().trim().to_string();
    match home.starts_with('/') {
        true => Ok(home),
        false => Err("the host did not say where its home is".to_string()),
    }
}

/// `path` as typed, made absolute against the host's `home` without asking the
/// host: what the field can work out for a suggestion before anything is
/// resolved there. Symlinks are not resolved, which [`check`] does.
pub fn absolute(path: &str, home: &str) -> String {
    let joined = match path {
        "" | "~" => home.to_string(),
        _ => match path.strip_prefix("~/") {
            Some(rest) => format!("{}/{rest}", home.trim_end_matches('/')),
            None if path.starts_with('/') => path.to_string(),
            None => format!("{}/{path}", home.trim_end_matches('/')),
        },
    };
    let mut out = PathBuf::from("/");
    for part in Path::new(&joined).components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(name) => out.push(name),
            _ => {}
        }
    }
    out.to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// Whether a directory can be the sandbox's
// ---------------------------------------------------------------------------

/// One line of `/proc/self/mountinfo`: where something is mounted, and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub point: PathBuf,
    pub fstype: String,
}

/// Every mount in a mountinfo table, in the table's order.
pub fn parse_mountinfo(table: &str) -> Vec<Mount> {
    table
        .lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let point = left.split(' ').nth(4)?;
            let fstype = right.split(' ').next()?;
            Some(Mount {
                point: PathBuf::from(unescape_mountinfo(point)),
                fstype: fstype.to_string(),
            })
        })
        .collect()
}

/// This process's mounts, read where the kernel keeps them rather than from
/// `mount`, whose output is for people.
pub fn mounts_here() -> Vec<Mount> {
    std::fs::read_to_string("/proc/self/mountinfo")
        .map(|t| parse_mountinfo(&t))
        .unwrap_or_default()
}

/// mountinfo spells a space, tab, newline and backslash as octal escapes.
pub fn unescape_mountinfo(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 4 <= bytes.len()
            && let Ok(code) = u8::from_str_radix(&field[i + 1..i + 4], 8)
        {
            out.push(code);
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Filesystems a sandbox's mount cannot go inside.
///
/// The kernel's own (`proc`, `sysfs`…) are not directories anyone works in;
/// another machine's (`9p` and `drvfs` are WSL's Windows drives, `nfs`,
/// `cifs`, any `fuse`) would put a FUSE mount on top of a network or FUSE
/// filesystem, which either fails outright or mounts somewhere nobody else on
/// this machine sees the same way; a read-only image (`squashfs`, a snap) has
/// nowhere to make the mount point.
fn foreign(fstype: &str) -> bool {
    fstype.starts_with("fuse")
        || fstype.starts_with("nfs")
        || fstype.starts_with("cgroup")
        || matches!(
            fstype,
            "9p" | "v9fs"
                | "drvfs"
                | "cifs"
                | "smb3"
                | "smbfs"
                | "autofs"
                | "proc"
                | "sysfs"
                | "devtmpfs"
                | "devpts"
                | "securityfs"
                | "debugfs"
                | "tracefs"
                | "configfs"
                | "binfmt_misc"
                | "mqueue"
                | "hugetlbfs"
                | "pstore"
                | "bpf"
                | "squashfs"
                | "iso9660"
        )
}

/// Directories that are this machine's plumbing, whatever is mounted there.
const SYSTEM: [&str; 4] = ["/proc", "/sys", "/dev", "/run"];

/// What this machine has at the path the mount would go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Here {
    /// Nothing; `creatable` is whether this user can make it.
    Missing { creatable: bool },
    /// A directory; mounting over a non-empty one would hide what is in it.
    Dir { empty: bool },
    /// A file, a socket — something that is not a directory.
    NotDir,
    /// A FUSE mount whose daemon has gone: a sandbox's own debris, which the
    /// sandbox clears before mounting.
    StaleFuse,
}

/// Look at `path` on this machine.
pub fn here(path: &Path) -> Here {
    match std::fs::metadata(path) {
        Ok(meta) if !meta.is_dir() => Here::NotDir,
        Ok(_) => Here::Dir {
            empty: std::fs::read_dir(path).is_ok_and(|mut d| d.next().is_none()),
        },
        Err(e) if e.raw_os_error() == Some(libc::ENOTCONN) => Here::StaleFuse,
        Err(_) => Here::Missing {
            creatable: path
                .ancestors()
                .skip(1)
                .find(|dir| dir.exists())
                .is_some_and(writable),
        },
    }
}

/// Whether this user can create entries in `dir`.
fn writable(dir: &Path) -> bool {
    let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: a valid C string for the duration of the call.
    unsafe { libc::access(c.as_ptr(), libc::W_OK | libc::X_OK) == 0 }
}

/// Why `root` cannot be a sandbox's working directory, or `Ok` if it can.
///
/// `root` is the host's directory as the host resolves it, which is also where
/// the mount goes here — one path on both machines is the design. So it has to
/// work on both:
///
/// - on the host: there, a directory, readable and writable (an agent that
///   cannot write is not much of an agent, and refusing up front beats every
///   edit failing);
/// - here: not `/` and not under `/proc`, `/sys`, `/dev` or `/run`; not
///   already a mount point (a stale one of the sandbox's own is fine — it is
///   cleared); not inside a filesystem a FUSE mount cannot go inside (a WSL
///   Windows drive, a network share, another FUSE mount); not covering a
///   mount below it; and either an empty directory, or missing and creatable
///   by this user.
///
/// `remote` is `None` when the host could not be asked, and only the local
/// half is judged. The reasons are short, because the field shows them beside
/// a suggestion; the sandbox puts its own context around them.
pub fn verdict(
    root: &Path,
    mounts: &[Mount],
    here: &Here,
    remote: Option<&RemoteFacts>,
) -> Result<(), String> {
    if let Some(r) = remote {
        if !r.exists {
            return Err("does not exist on the host".into());
        }
        if !r.is_dir {
            return Err("not a directory on the host".into());
        }
        if !r.readable {
            return Err("not readable on the host".into());
        }
        if !r.writable {
            return Err("read-only on the host".into());
        }
    }
    if root == Path::new("/") {
        return Err("the root directory — mounted locally".into());
    }
    if let Some(sys) = SYSTEM.iter().find(|s| root.starts_with(s)) {
        return Err(format!("{sys} is system plumbing here"));
    }
    // The innermost mount holding `root`; the table lists a mount after the
    // one it sits on, so the last match is the one on top.
    let holder = mounts
        .iter()
        .rev()
        .filter(|m| root.starts_with(&m.point))
        .max_by_key(|m| m.point.components().count());
    if let Some(m) = holder {
        if m.point == root {
            if !(m.fstype.starts_with("fuse") && *here == Here::StaleFuse) {
                return Err(format!("mounted locally ({})", m.fstype));
            }
        } else if foreign(&m.fstype) {
            return Err(format!(
                "inside {} here, mounted locally ({})",
                m.point.display(),
                m.fstype
            ));
        }
    }
    if let Some(m) = mounts
        .iter()
        .find(|m| m.point != root && m.point.starts_with(root))
    {
        return Err(format!(
            "would cover {}, mounted locally ({})",
            m.point.display(),
            m.fstype
        ));
    }
    match here {
        Here::NotDir => Err("a file here, not a directory".into()),
        Here::Dir { empty: false } => Err("not empty here — the mount would hide it".into()),
        Here::Missing { creatable: false } => Err("can't be created here without sudo".into()),
        Here::Dir { empty: true } | Here::Missing { creatable: true } | Here::StaleFuse => Ok(()),
    }
}

/// For tests here and elsewhere: a [`Runner`] that runs the command line with
/// a local `sh`, the way sshd would hand it to the login shell, in a home of
/// the test's choosing. No ssh, no network.
#[cfg(test)]
pub struct LocalSh {
    pub home: PathBuf,
}

#[cfg(test)]
impl Runner for LocalSh {
    fn run(&self, _host: &str, command: &str, timeout: Duration) -> Result<String, String> {
        let mut cmd = std::process::Command::new("sh");
        // In the home, as sshd starts the login shell.
        cmd.arg("-c")
            .arg(command)
            .env("HOME", &self.home)
            .current_dir(&self.home);
        crate::ssh_master::run_bounded(cmd, timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_home() -> (tempfile::TempDir, LocalSh) {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().canonicalize().expect("canonical");
        (dir, LocalSh { home })
    }

    #[test]
    fn a_listing_names_the_subdirectories_and_what_can_be_done_in_them() {
        let (_keep, sh) = fake_home();
        for d in ["src", "docs", ".hidden", "with space"] {
            std::fs::create_dir(sh.home.join(d)).expect("mkdir");
        }
        std::fs::write(sh.home.join("file"), "").expect("file");
        std::os::unix::fs::symlink(sh.home.join("src"), sh.home.join("link")).expect("link");
        let ro = sh.home.join("docs");
        std::fs::set_permissions(&ro, std::os::unix::fs::PermissionsExt::from_mode(0o555))
            .expect("chmod");

        let listing = list(&sh, "box", "~").expect("ran").expect("there");
        assert_eq!(listing.resolved, sh.home.to_string_lossy());
        let names: Vec<&str> = listing.dirs.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, [".hidden", "docs", "link", "src", "with space"]);
        let docs = &listing.dirs[1];
        assert!(docs.readable);
        // Root can write anywhere, so only assert the bit when it means something.
        // SAFETY: getuid cannot fail.
        if unsafe { libc::getuid() } != 0 {
            assert!(!docs.writable, "{docs:?}");
        }
        assert!(listing.dirs[3].writable);

        // A subdirectory by its `~` path, and one that is not there.
        let src = list(&sh, "box", "~/src").expect("ran").expect("there");
        assert!(src.dirs.is_empty());
        assert_eq!(list(&sh, "box", "~/nope").expect("ran"), None);
        std::fs::set_permissions(&ro, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod back");
    }

    #[test]
    fn the_repository_scan_finds_checkouts_and_not_what_is_inside_them() {
        let (_keep, sh) = fake_home();
        for repo in [
            "api",
            "code/web",
            "code/web/vendor/sub",
            "a/b/c",
            ".config/x",
        ] {
            std::fs::create_dir_all(sh.home.join(repo).join(".git")).expect("repo");
        }
        // Four levels down: past the scan.
        std::fs::create_dir_all(sh.home.join("a/b/c/d/.git")).expect("deep");
        let found = repos(&sh, "box").expect("ran");
        assert_eq!(found, ["~/a/b/c", "~/api", "~/code/web"]);
    }

    #[test]
    fn a_check_reports_what_the_host_has_at_the_path() {
        let (_keep, sh) = fake_home();
        std::fs::create_dir(sh.home.join("work")).expect("mkdir");
        std::fs::write(sh.home.join("f"), "").expect("file");
        let work = check(&sh, "box", "~/work").expect("ran");
        assert!(work.exists && work.is_dir && work.readable && work.writable);
        assert_eq!(
            work.resolved.as_deref(),
            Some(sh.home.join("work").to_str().expect("utf8"))
        );
        let file = check(&sh, "box", "~/f").expect("ran");
        assert!(file.exists && !file.is_dir);
        assert_eq!(
            check(&sh, "box", "~/gone").expect("ran"),
            RemoteFacts::default()
        );
        assert_eq!(
            home(&sh, "box").as_deref(),
            Ok(sh.home.to_str().expect("utf8"))
        );
    }

    #[test]
    fn a_typed_path_is_made_absolute_against_the_hosts_home() {
        assert_eq!(absolute("~", "/home/f"), "/home/f");
        assert_eq!(absolute("", "/home/f"), "/home/f");
        assert_eq!(absolute("~/src/", "/home/f"), "/home/f/src");
        assert_eq!(absolute("src", "/home/f/"), "/home/f/src");
        assert_eq!(absolute("/srv/../opt", "/home/f"), "/opt");
    }

    const WSL: &str = "\
22 1 8:32 / / rw,relatime - ext4 /dev/sdc rw
23 22 0:20 / /proc rw - proc proc rw
40 22 0:44 / /mnt/c rw,noatime - 9p C:\\134 rw
41 22 0:45 / /mnt/wsl rw - tmpfs none rw
50 22 0:50 / /home/f/remote rw,nosuid - fuse.sshfs box:/x rw
51 22 0:51 / /srv/data rw - nfs4 nas:/data rw
52 22 0:52 / /tmp rw - tmpfs tmpfs rw";

    fn ok_remote() -> RemoteFacts {
        RemoteFacts {
            exists: true,
            is_dir: true,
            readable: true,
            writable: true,
            resolved: None,
        }
    }

    fn judge(path: &str, here: Here) -> Result<(), String> {
        verdict(
            Path::new(path),
            &parse_mountinfo(WSL),
            &here,
            Some(&ok_remote()),
        )
    }

    const FREE: Here = Here::Missing { creatable: true };

    #[test]
    fn a_path_that_is_mounted_here_cannot_be_mounted_on() {
        assert_eq!(judge("/home/f/work", FREE), Ok(()));
        assert_eq!(judge("/tmp/x", FREE), Ok(()), "tmpfs is this machine's own");
        let reason = |p: &str, h: Here| judge(p, h).expect_err(p);
        assert_eq!(reason("/mnt/c", FREE), "mounted locally (9p)");
        assert!(reason("/mnt/c/Users/f", FREE).contains("inside /mnt/c"));
        assert!(reason("/srv/data/x", FREE).contains("nfs4"));
        assert!(reason("/home/f/remote/sub", FREE).contains("fuse.sshfs"));
        assert_eq!(
            reason("/home/f/remote", Here::Dir { empty: true }),
            "mounted locally (fuse.sshfs)"
        );
        // Unless it is a dead sandbox's mount, which the sandbox clears.
        assert_eq!(judge("/home/f/remote", Here::StaleFuse), Ok(()));
        assert!(reason("/mnt", FREE).contains("would cover /mnt/c"));
        assert!(reason("/", FREE).contains("root"));
        assert!(reason("/proc/1", FREE).contains("/proc"));
        assert!(reason("/run/user/1000/x", FREE).contains("/run"));
        assert!(reason("/dev/shm/x", FREE).contains("/dev"));
    }

    #[test]
    fn a_path_has_to_be_somewhere_the_mount_can_go() {
        let reason = |h: Here| judge("/home/f/work", h).expect_err("refused");
        assert_eq!(judge("/home/f/work", Here::Dir { empty: true }), Ok(()));
        assert!(reason(Here::Dir { empty: false }).contains("not empty"));
        assert!(reason(Here::NotDir).contains("file"));
        assert!(reason(Here::Missing { creatable: false }).contains("sudo"));
    }

    #[test]
    fn a_path_has_to_be_usable_on_the_host() {
        let with = |r: RemoteFacts| {
            verdict(Path::new("/home/f/w"), &[], &FREE, Some(&r)).expect_err("refused")
        };
        assert!(with(RemoteFacts::default()).contains("does not exist"));
        assert!(
            with(RemoteFacts {
                is_dir: false,
                ..ok_remote()
            })
            .contains("not a directory")
        );
        assert!(
            with(RemoteFacts {
                readable: false,
                ..ok_remote()
            })
            .contains("not readable")
        );
        assert!(
            with(RemoteFacts {
                writable: false,
                ..ok_remote()
            })
            .contains("read-only")
        );
        // Not asked: only this side is judged.
        assert_eq!(verdict(Path::new("/home/f/w"), &[], &FREE, None), Ok(()));
    }

    #[test]
    fn here_reads_what_is_at_a_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(here(dir.path()), Here::Dir { empty: true });
        std::fs::write(dir.path().join("f"), "").expect("file");
        assert_eq!(here(dir.path()), Here::Dir { empty: false });
        assert_eq!(here(&dir.path().join("f")), Here::NotDir);
        assert_eq!(
            here(&dir.path().join("a/b")),
            Here::Missing { creatable: true }
        );
    }
}
