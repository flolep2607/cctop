//! One changed file, read in full for the diff view: its whole diff against
//! the sidebar's base, and both of its versions.
//!
//! The sidebar's [`changes::read`](crate::changes::read) keeps a few hundred
//! lines of each file's diff and no file contents, which is right for a list
//! and wrong for reading one file: a diff view cut at line 400 is a view of
//! the wrong file, and the unchanged stretches between hunks cannot be opened
//! without the text they stand for. So the view asks for its file once, when
//! it is opened, and draws from what came back.
//!
//! Both versions are read rather than re-diffed: the hunks are git's, which is
//! what every other diff of this checkout shows, and the versions are only
//! there to fill in what a hunk leaves out.

use std::path::Path;

use crate::changes::{diff_cmd, git, run_capped};

/// A file's diff past this many bytes is not drawn: the view would be a
/// scrollbar, and reading it would hold the worker for the length of it.
const MAX_DIFF_BYTES: usize = 4 << 20;
/// Each version is read up to this; a longer one keeps its hunks and loses
/// only the ability to open the stretches past the cut.
const MAX_VERSION_BYTES: usize = 4 << 20;

/// What the diff view shows for one file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileDiff {
    /// The diff from the first hunk on, without git's header; for an
    /// untracked file, every line added, with no hunk header.
    pub diff: Vec<String>,
    /// The file at the base, and as it is now, by line. `None` where it does
    /// not exist (added, deleted) or could not be read whole.
    pub old: Option<Vec<String>>,
    pub new: Option<Vec<String>>,
    /// Git or the bytes say binary: there are no lines to show.
    pub binary: bool,
    /// The diff ran past [`MAX_DIFF_BYTES`] and is not shown.
    pub too_big: bool,
}

/// Read `path` in the checkout at `root`, against `base`.
///
/// Runs git twice and reads a file, so it belongs on a worker thread.
pub fn read(root: &Path, base: &str, path: &str, untracked: bool) -> Result<FileDiff, String> {
    // A path from the sidebar is relative to the checkout and git's own
    // spelling; one that climbs out of it is not a path this view asked for.
    if path.is_empty() || Path::new(path).is_absolute() || path.split('/').any(|p| p == "..") {
        return Err(format!("{path:?} is not a path in this checkout"));
    }
    let new = read_new(root, path);
    if untracked {
        let Some(new) = new else {
            return Err(format!("{path} could not be read"));
        };
        return Ok(match new {
            Version::Binary => FileDiff {
                binary: true,
                ..FileDiff::default()
            },
            Version::Lines(lines) => FileDiff {
                diff: lines.iter().map(|l| format!("+{l}")).collect(),
                new: Some(lines),
                ..FileDiff::default()
            },
        });
    }
    let (patch, cut) = run_capped(
        diff_cmd(root).args(["-p", "-U3", base, "--", path]),
        MAX_DIFF_BYTES,
    )?;
    if cut {
        return Ok(FileDiff {
            too_big: true,
            ..FileDiff::default()
        });
    }
    let binary = patch.lines().any(|l| l.starts_with("Binary files "));
    let diff: Vec<String> = match patch.lines().position(|l| l.starts_with("@@")) {
        Some(at) => patch.lines().skip(at).map(str::to_string).collect(),
        None => Vec::new(),
    };
    let old = read_old(root, base, path);
    let lines = |v: Option<Version>| match v {
        Some(Version::Lines(lines)) => Some(lines),
        _ => None,
    };
    let binary =
        binary || matches!(new, Some(Version::Binary)) || matches!(old, Some(Version::Binary));
    Ok(FileDiff {
        diff,
        old: lines(old),
        new: lines(new),
        binary,
        too_big: false,
    })
}

enum Version {
    Lines(Vec<String>),
    Binary,
}

/// Text as lines, or binary by git's own test: a NUL in the first 8000 bytes.
fn version(bytes: &[u8]) -> Version {
    if bytes.iter().take(8000).any(|&b| b == 0) {
        return Version::Binary;
    }
    Version::Lines(
        String::from_utf8_lossy(bytes)
            .lines()
            .map(str::to_string)
            .collect(),
    )
}

/// The file as it is in the working tree.
fn read_new(root: &Path, path: &str) -> Option<Version> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(root.join(path))
        .ok()?
        .take(MAX_VERSION_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= MAX_VERSION_BYTES).then(|| version(&bytes))
}

/// The file at `base`, from git's object store: `git cat-file` on the blob
/// rather than `git show`, which would run the checkout's textconv.
fn read_old(root: &Path, base: &str, path: &str) -> Option<Version> {
    let (text, cut) = run_capped(
        git(root).args(["cat-file", "blob", &format!("{base}:{path}")]),
        MAX_VERSION_BYTES,
    )
    .ok()?;
    if cut {
        return None;
    }
    Some(version(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q"]);
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "x"]);
        dir
    }

    fn head(dir: &Path) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A tracked file's diff starts at its first hunk, and both versions come
    /// back whole.
    #[test]
    fn a_changed_file_comes_with_both_versions() {
        let dir = repo();
        std::fs::write(dir.path().join("a.txt"), "one\nTWO\nthree\nfour\n").unwrap();
        let read = read(dir.path(), &head(dir.path()), "a.txt", false).unwrap();
        assert!(read.diff[0].starts_with("@@"), "{:?}", read.diff);
        assert!(read.diff.contains(&"-two".to_string()));
        assert!(read.diff.contains(&"+TWO".to_string()));
        assert_eq!(read.old.unwrap(), ["one", "two", "three"]);
        assert_eq!(read.new.unwrap(), ["one", "TWO", "three", "four"]);
        assert!(!read.binary && !read.too_big);
    }

    /// An untracked file is all added, and a binary one is said to be.
    #[test]
    fn untracked_and_binary_files() {
        let dir = repo();
        std::fs::write(dir.path().join("new.txt"), "x\ny\n").unwrap();
        let new = read(dir.path(), &head(dir.path()), "new.txt", true).unwrap();
        assert_eq!(new.diff, ["+x", "+y"]);
        std::fs::write(dir.path().join("b.bin"), [0u8, 1, 2]).unwrap();
        assert!(
            read(dir.path(), &head(dir.path()), "b.bin", true)
                .unwrap()
                .binary
        );
    }

    /// A path out of the checkout is not read.
    #[test]
    fn a_path_out_of_the_checkout_is_refused() {
        let dir = repo();
        for bad in ["../etc/passwd", "/etc/passwd", ""] {
            assert!(read(dir.path(), "HEAD", bad, true).is_err(), "{bad}");
        }
    }
}
