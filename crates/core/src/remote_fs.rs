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
//! anything. They are the host's rules alone. Where the mount goes on this
//! machine is [`mount_point`]'s choice, and it always has one.

use crate::ssh_master::Runner;
use std::path::{Path, PathBuf};
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

/// Why the host's `root` cannot be a sandbox's working directory, or `Ok` if
/// it can.
///
/// Only the host is judged: the directory has to be there, a directory, and
/// readable and writable — an agent that cannot write is not much of an agent,
/// and refusing up front beats every edit failing. Nothing on this machine can
/// refuse it any more, since a path that cannot be mounted on here is mounted
/// somewhere else instead (see [`mount_point`]).
///
/// The reasons are short, because the field shows them beside a suggestion;
/// the sandbox puts its own context around them.
pub fn verdict(remote: &RemoteFacts) -> Result<(), String> {
    if !remote.exists {
        return Err("does not exist on the host".into());
    }
    if !remote.is_dir {
        return Err("not a directory on the host".into());
    }
    if !remote.readable {
        return Err("not readable on the host".into());
    }
    if !remote.writable {
        return Err("read-only on the host".into());
    }
    Ok(())
}

/// Why a mount cannot go at `at` on this machine, or `None` if it can.
///
/// - not `/` and not under `/proc`, `/sys`, `/dev` or `/run`;
/// - not already a mount point (a stale one of the sandbox's own is fine — it
///   is cleared);
/// - not inside a filesystem a FUSE mount cannot go inside (a WSL Windows
///   drive, a network share, another FUSE mount — which includes another
///   sandbox's mount, where making the directory would make it on that host);
/// - not covering a mount below it;
/// - an empty directory, or missing and creatable by this user.
pub fn mount_problem(at: &Path, mounts: &[Mount], here: &Here) -> Option<String> {
    if at == Path::new("/") {
        return Some("the root directory".into());
    }
    if let Some(sys) = SYSTEM.iter().find(|s| at.starts_with(s)) {
        return Some(format!("{sys} is system plumbing here"));
    }
    // The innermost mount holding `at`; the table lists a mount after the one
    // it sits on, so the last match is the one on top.
    let holder = mounts
        .iter()
        .rev()
        .filter(|m| at.starts_with(&m.point))
        .max_by_key(|m| m.point.components().count());
    if let Some(m) = holder {
        if m.point == at {
            if !(m.fstype.starts_with("fuse") && *here == Here::StaleFuse) {
                return Some(format!("mounted here already ({})", m.fstype));
            }
        } else if foreign(&m.fstype) {
            return Some(format!(
                "inside {}, mounted here ({})",
                m.point.display(),
                m.fstype
            ));
        }
    }
    if let Some(m) = mounts
        .iter()
        .find(|m| m.point != at && m.point.starts_with(at))
    {
        return Some(format!(
            "would cover {}, mounted here ({})",
            m.point.display(),
            m.fstype
        ));
    }
    match here {
        Here::NotDir => Some("a file here, not a directory".into()),
        Here::Dir { empty: false } => Some("not empty here".into()),
        Here::Missing { creatable: false } => Some("can't be created here".into()),
        Here::Dir { empty: true } | Here::Missing { creatable: true } | Here::StaleFuse => None,
    }
}

/// How many cctop-owned places one host's directory may be mounted at once —
/// the first is the stable one, the rest are for a second and third sandbox
/// in the same directory at the same time.
const SLOTS: u32 = 9;

/// The longest mount point taken as it is. A path is at most 4096 bytes, and
/// the files under the mount need room of their own beneath it.
const POINT_MAX: usize = 2048;

/// Where the host's `root` is mounted on this machine.
///
/// **At `root` itself when that can be done**, because then a path means the
/// same file on both machines and nothing has to translate between them: an
/// empty directory, or a missing one this user can create, that no other mount
/// is in the way of.
///
/// **Otherwise under `base`** — `~/.cache/cctop/remote` — at
/// `<base>/<host>/<root>`. This is the usual case for a home directory: the
/// host's `/home/someone` is rarely there to be created on this machine
/// without sudo, and `/home/me` is not empty. The path is a function of the
/// host and the directory alone, so it is the same every launch: Claude Code
/// files its history per working directory, and a `--resume` of yesterday's
/// session has to land in the directory that session was in.
///
/// It is also long and unmistakably cctop's, which is what makes the
/// translation of a command's text safe (see `sandbox::PathMap`): nothing a
/// model writes contains it by accident.
///
/// A second sandbox in the same directory while the first is running finds
/// the stable place taken, and gets `<host>~2`, then `~3`. A stale mount at
/// any of them is fine — the sandbox clears it. `here` is how each candidate is
/// looked at, so tests can say what is there without making it.
pub fn mount_point(
    host: &str,
    root: &Path,
    mounts: &[Mount],
    here: &dyn Fn(&Path) -> Here,
    base: &Path,
) -> Result<PathBuf, String> {
    if mount_problem(root, mounts, &here(root)).is_none() {
        return Ok(root.to_path_buf());
    }
    let mut why = String::new();
    for slot in 1..=SLOTS {
        let at = cache_point(base, host, root, slot);
        match mount_problem(&at, mounts, &here(&at)) {
            None => return Ok(at),
            Some(problem) => why = format!("{}: {problem}", at.display()),
        }
    }
    Err(format!("nowhere to mount it here — {why}"))
}

/// `<base>/<host>/<root>`, or `<base>/<host>~<slot>/<root>` past the first.
///
/// The host is spelled as typed, so `procdb` and `procdb.example.com` are two
/// places, as they are two names; bytes a directory name should not carry are
/// replaced. A name or a path too long for the filesystem is shortened to a
/// hash of itself — still stable — with the directory's last name kept, so
/// the path still says what it is.
pub fn cache_point(base: &Path, host: &str, root: &Path, slot: u32) -> PathBuf {
    let mut name: String = host
        .chars()
        .map(
            |c| match c.is_ascii_alphanumeric() || "._-@:+,=".contains(c) {
                true => c,
                false => '_',
            },
        )
        .collect();
    if matches!(name.as_str(), "" | "." | "..") {
        name = format!("_{name}");
    }
    if name.len() > 200 {
        name = format!("{}-{:012x}", &name[..180], fnv(host.as_bytes()));
    }
    if slot > 1 {
        name = format!("{name}~{slot}");
    }
    let dir = base.join(name);
    let rel = root.strip_prefix("/").unwrap_or(root);
    let full = dir.join(rel);
    match full.as_os_str().len() <= POINT_MAX {
        true => full,
        false => {
            let last = root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            dir.join(format!(
                "long-{:012x}",
                fnv(root.as_os_str().as_encoded_bytes())
            ))
            .join(last)
        }
    }
}

/// FNV-1a, not `DefaultHasher`: the mount point has to be the same from one
/// cctop build to the next, or an update would move every remote session's
/// history.
fn fnv(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash & 0xffff_ffff_ffff
}

/// For tests here and elsewhere: a [`Runner`] that runs the command line with
/// a local `sh`, the way sshd would hand it to the login shell, in a home of
/// the test's choosing. No ssh, no network.
#[cfg(any(test, feature = "test-support"))]
pub struct LocalSh {
    pub home: PathBuf,
}

#[cfg(any(test, feature = "test-support"))]
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

    fn problem(path: &str, here: Here) -> Option<String> {
        mount_problem(Path::new(path), &parse_mountinfo(WSL), &here)
    }

    const FREE: Here = Here::Missing { creatable: true };

    #[test]
    fn a_path_that_is_mounted_here_cannot_be_mounted_on() {
        assert_eq!(problem("/home/f/work", FREE), None);
        assert_eq!(problem("/tmp/x", FREE), None, "tmpfs is this machine's own");
        let reason = |p: &str, h: Here| problem(p, h).expect(p);
        assert_eq!(reason("/mnt/c", FREE), "mounted here already (9p)");
        assert!(reason("/mnt/c/Users/f", FREE).contains("inside /mnt/c"));
        assert!(reason("/srv/data/x", FREE).contains("nfs4"));
        assert!(reason("/home/f/remote/sub", FREE).contains("fuse.sshfs"));
        assert_eq!(
            reason("/home/f/remote", Here::Dir { empty: true }),
            "mounted here already (fuse.sshfs)"
        );
        // Unless it is a dead sandbox's mount, which the sandbox clears.
        assert_eq!(problem("/home/f/remote", Here::StaleFuse), None);
        assert!(reason("/mnt", FREE).contains("would cover /mnt/c"));
        assert!(reason("/", FREE).contains("root"));
        assert!(reason("/proc/1", FREE).contains("/proc"));
        assert!(reason("/run/user/1000/x", FREE).contains("/run"));
        assert!(reason("/dev/shm/x", FREE).contains("/dev"));
    }

    #[test]
    fn a_path_has_to_be_somewhere_the_mount_can_go() {
        let reason = |h: Here| problem("/home/f/work", h).expect("refused");
        assert_eq!(problem("/home/f/work", Here::Dir { empty: true }), None);
        assert!(reason(Here::Dir { empty: false }).contains("not empty"));
        assert!(reason(Here::NotDir).contains("file"));
        assert!(reason(Here::Missing { creatable: false }).contains("created"));
    }

    /// Only the host can make a directory unusable now: nothing about this
    /// machine is asked, so nothing about it can refuse.
    #[test]
    fn a_path_has_to_be_usable_on_the_host() {
        let with = |r: RemoteFacts| verdict(&r).expect_err("refused");
        assert_eq!(verdict(&ok_remote()), Ok(()));
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
    }

    const BASE: &str = "/home/f/.cache/cctop/remote";

    /// What [`mount_point`] picks, given what each candidate is said to be.
    fn pick(root: &str, table: &str, here: &dyn Fn(&Path) -> Here) -> Result<PathBuf, String> {
        mount_point(
            "procdb",
            Path::new(root),
            &parse_mountinfo(table),
            here,
            Path::new(BASE),
        )
    }

    #[test]
    fn the_same_path_is_used_when_it_can_be() {
        // Missing and creatable, or an empty directory: mounted where it is.
        assert_eq!(
            pick("/srv/api", WSL, &|_| FREE),
            Ok(PathBuf::from("/srv/api"))
        );
        assert_eq!(
            pick("/home/f/empty", WSL, &|_| Here::Dir { empty: true }),
            Ok(PathBuf::from("/home/f/empty"))
        );
    }

    #[test]
    fn a_path_that_cannot_be_mounted_on_goes_under_the_cache() {
        let cached = PathBuf::from(format!("{BASE}/procdb/home/florian.leprat"));
        // The everyday case: the host's home is someone else's here, under a
        // `/home` this user cannot write.
        let root_owned = |p: &Path| match p.starts_with(BASE) {
            true => FREE,
            false => Here::Missing { creatable: false },
        };
        assert_eq!(
            pick("/home/florian.leprat", WSL, &root_owned),
            Ok(cached.clone())
        );
        // A directory here with files in it, which the mount would hide.
        let full = |p: &Path| match p.starts_with(BASE) {
            true => FREE,
            false => Here::Dir { empty: false },
        };
        assert_eq!(pick("/home/florian.leprat", WSL, &full), Ok(cached));
        // Inside a Windows drive, already a mount, the root itself: all fine,
        // just not at the same path.
        for (root, rel) in [
            ("/mnt/c/Users/f", "mnt/c/Users/f"),
            ("/home/f/remote", "home/f/remote"),
            ("/", ""),
        ] {
            let want = Path::new(BASE).join("procdb").join(rel);
            assert_eq!(pick(root, WSL, &|_| FREE), Ok(want), "{root}");
        }
    }

    /// A second sandbox in the same directory finds the stable place taken by
    /// the first, and goes next door; a dead one's mount is reused.
    #[test]
    fn a_taken_cache_point_moves_to_the_next_slot() {
        let first = format!("{BASE}/procdb/home/x");
        let table = format!("{WSL}\n60 22 0:60 / {first} rw,nosuid - fuse.sshfs procdb:/home/x rw");
        let busy = |p: &Path| match p.starts_with(BASE) {
            true if p == Path::new(&first) => Here::Dir { empty: true },
            true => FREE,
            false => Here::Missing { creatable: false },
        };
        assert_eq!(
            pick("/home/x", &table, &busy),
            Ok(PathBuf::from(format!("{BASE}/procdb~2/home/x")))
        );
        let stale = |p: &Path| match p.starts_with(BASE) {
            true if p == Path::new(&first) => Here::StaleFuse,
            true => FREE,
            false => Here::Missing { creatable: false },
        };
        assert_eq!(pick("/home/x", &table, &stale), Ok(PathBuf::from(&first)));
        // A directory under a running sandbox's mount would be made on that
        // host: never a candidate.
        let outer =
            format!("{WSL}\n61 22 0:61 / {BASE}/procdb/home rw - fuse.sshfs procdb:/home rw");
        assert_eq!(
            pick("/home/x", &outer, &busy),
            Ok(PathBuf::from(format!("{BASE}/procdb~2/home/x")))
        );
    }

    #[test]
    fn a_cache_point_is_a_safe_and_bounded_name() {
        let base = Path::new(BASE);
        let at = |host: &str, root: &str| cache_point(base, host, Path::new(root), 1);
        assert_eq!(
            at("me@10.0.0.5", "/srv"),
            Path::new(BASE).join("me@10.0.0.5/srv")
        );
        assert_eq!(at("[::1]", "/srv"), Path::new(BASE).join("_::1_/srv"));
        assert_eq!(at("..", "/srv"), Path::new(BASE).join("_../srv"));
        assert_eq!(at("a/b", "/srv"), Path::new(BASE).join("a_b/srv"));
        // The same answer every time: the history depends on it.
        assert_eq!(at("procdb", "/home/x"), at("procdb", "/home/x"));
        let long_host = "h".repeat(300);
        let name = at(&long_host, "/srv");
        let host_part = name
            .strip_prefix(base)
            .expect("under base")
            .components()
            .next()
            .expect("host");
        assert!(host_part.as_os_str().len() <= 255, "{name:?}");
        let deep = format!("/{}/end", "d/".repeat(1200));
        let short = at("procdb", &deep);
        assert!(short.as_os_str().len() <= POINT_MAX, "{short:?}");
        assert!(short.ends_with("end"));
        assert_eq!(short, at("procdb", &deep));
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
