//! Directory suggestions for the launcher's `in` field.
//!
//! Typing a working directory from memory is the one part of starting an agent
//! that has no answer on screen: the path is somewhere in a shell history, and
//! a character wrong means the agent reads its way into the wrong repository —
//! or, since the field is checked, simply refuses with the cursor still at the
//! end of a long line.
//!
//! So the field offers what it can see. An empty or half-typed name is matched
//! against the directories cctop already knows agents have run in; anything
//! that reads as a path is completed against the filesystem itself. Both come
//! back as absolute directories that exist, which is what lets Enter on a
//! suggestion skip the check entirely.

use std::path::{Path, PathBuf};

/// Suggestions offered at once. Enough to recognise the one you meant, few
/// enough that the list stays under the launcher rather than replacing it —
/// the agent being started is half of what the directory is chosen for.
pub(super) const MAX_HITS: usize = 6;

/// The directory being spelled and the fragment of a name after it, when the
/// text reads as a path at all.
///
/// `None` for a bare word: `cctop` names no directory to look inside, and
/// resolving it against the process's own working directory would offer
/// children of wherever cctop happens to have been started.
fn split(expanded: &str) -> Option<(PathBuf, String)> {
    // A trailing separator is the whole point of typing one: everything in
    // here, not the directory itself again.
    if expanded.ends_with('/') || expanded.ends_with(std::path::MAIN_SEPARATOR) {
        return Some((PathBuf::from(expanded), String::new()));
    }
    let path = Path::new(expanded);
    let parent = path.parent()?;
    if parent.as_os_str().is_empty() {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some((parent.to_path_buf(), name))
}

/// Subdirectories of `parent` whose name starts with `fragment`.
///
/// Hidden directories only once the fragment asks for them by its leading dot.
/// A repository's `.git` and its siblings would otherwise be most of what a
/// bare `~/` offers, pushing the projects under it off the list.
fn children(parent: &Path, fragment: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let wanted = fragment.to_lowercase();
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && !fragment.starts_with('.') {
                return None;
            }
            name.to_lowercase()
                .starts_with(&wanted)
                .then(|| e.path().to_path_buf())
        })
        .collect();
    out.sort();
    out
}

/// What the field can offer for `typed`, given the directories agents are
/// already known to have run in.
///
/// `known` is consulted for the part nobody can type from memory — which
/// projects exist — and the filesystem for the part it cannot know, which is
/// everything below them. Matching `known` on a substring rather than a prefix
/// is deliberate: what is remembered about a project is its name, not the
/// directory tree it sits in.
pub(super) fn suggest(typed: &str, known: &[PathBuf]) -> Vec<PathBuf> {
    let typed = typed.trim();
    let expanded = crate::util::untildify(typed);
    match split(&expanded) {
        Some((parent, fragment)) => children(&parent, &fragment)
            .into_iter()
            .take(MAX_HITS)
            .collect(),
        None => {
            let wanted = typed.to_lowercase();
            known
                .iter()
                .filter(|dir| dir.to_string_lossy().to_lowercase().contains(&wanted))
                .take(MAX_HITS)
                .cloned()
                .collect()
        }
    }
}

/// The most `typed` can be filled in without choosing between `hits` — what Tab
/// is for.
///
/// A single hit completes to it outright, with the separator that invites going
/// deeper. Several complete to as far as they agree, which is the part of the
/// path that was going to be typed either way. `None` when there is nothing to
/// add, so Tab on an already-complete field does nothing rather than redrawing
/// it identically.
pub(super) fn complete(typed: &str, hits: &[PathBuf]) -> Option<String> {
    let expanded = crate::util::untildify(typed.trim());
    let filled = match hits {
        [] => return None,
        [one] => {
            let mut s = crate::util::tildify(&one.to_string_lossy());
            s.push('/');
            s
        }
        many => {
            let shared = shared_prefix(many);
            if shared.chars().count() <= expanded.chars().count() {
                return None;
            }
            crate::util::tildify(&shared)
        }
    };
    let sep = std::path::MAIN_SEPARATOR;
    let filled = match sep != '/' && typed.contains('/') && !typed.contains(sep) {
        true => filled.replace(sep, "/"),
        false => filled,
    };
    (filled != typed).then_some(filled)
}

/// The longest string every path starts with, cut to a whole character.
fn shared_prefix(paths: &[PathBuf]) -> String {
    let spellings: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into()).collect();
    let mut prefix = String::new();
    let Some(first) = spellings.first() else {
        return prefix;
    };
    for (i, c) in first.char_indices() {
        let upto = i + c.len_utf8();
        if spellings
            .iter()
            .all(|s| s.len() >= upto && s[..upto] == first[..upto])
        {
            prefix.push(c);
        } else {
            break;
        }
    }
    prefix
}

/// The git repositories under `home`, two levels deep, newest first.
///
/// What cctop knows about a project comes from the agents that ran in it, so a
/// repository nobody has launched an agent in yet is invisible to it — and that
/// is precisely the repository somebody has just pulled and wants to try. This
/// is where those come from.
///
/// Two levels, because that is where repositories actually sit: `~/project` and
/// `~/code/project` are both common, and `~/code/project/sub` is not where
/// anyone keeps the thing they would name. A deeper walk finds nothing new and
/// pays for every directory on the machine, which is why the cost of the next
/// level is the whole reason for stopping here rather than a rule of thumb. It
/// also bounds the walk: a symlink cycle would otherwise be a walk with no end,
/// and a repository is never descended into, so nothing under one is followed
/// either way.
///
/// Only directories that are not hidden, and only at the two levels: a hidden
/// home entry is configuration rather than a project, and skipping them also
/// keeps `.cache` and `.local` — which are large and full of directories that
/// are not anybody's work — out of the walk entirely.
///
/// Newest first, by the `.git` directory's own mtime: a repository pulled this
/// morning outranks one last touched last spring, which is the same preference
/// the field already applies to what agents have run in, and the reason the two
/// parts of the list read as one.
///
/// Whether `path` is a directory once a symlink is resolved.
///
/// Separate from a directory read's entry type because the two disagree about a
/// link: `read_dir` reports what the entry *is*, which for a symlink is a
/// symlink, and this asks what it *points at*.
fn path_is_dir(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_dir())
}

/// `home` is taken as an argument rather than read from the environment, so the
/// scan is a function that can be pointed at a directory and tested.
pub(super) fn repos_under(home: &Path) -> Vec<PathBuf> {
    let mut found: Vec<(u64, PathBuf)> = Vec::new();
    let mut frontier = vec![home.to_path_buf()];
    for _ in 0..2 {
        let mut next = Vec::new();
        for dir in frontier {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                // `file_type` comes from the directory read where the platform
                // carries it, so this does not stat a thing we are about to read.
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                // A symlinked checkout is followed, deliberately: a repository linked into
                // `~/code` from elsewhere on the disk is a perfectly ordinary
                // place to keep it, and refusing to offer it would be the same
                // wrong answer as not offering a repository at all. `file_type`
                // reports the *link's* own type rather than its target's, so the
                // link is skipped here and recognised below — following it by
                // asking is what costs a `stat`, and only a link pays one.
                if kind.is_symlink() {
                    if !path_is_dir(&entry.path()) {
                        continue;
                    }
                } else if !kind.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                if name.to_string_lossy().starts_with('.') {
                    continue;
                }
                let path = entry.path();
                // The `.git` test is a `stat` too, and it is asked of every
                // candidate rather than of every entry, so a link costs one here
                // and a plain directory costs none.
                if path.join(".git").exists() {
                    found.push((crate::config::file_mtime_ms(&path.join(".git")), path));
                    // Not descended into: a repository's own subdirectories are
                    // its business, and this is a list of repositories.
                } else {
                    next.push(path);
                }
            }
        }
        frontier = next;
    }
    // Newest first, then by path so two repositories touched in the same
    // millisecond — which a fresh `git clone` of two things can be — do not
    // swap places between two scans.
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.into_iter().map(|(_, path)| path).collect()
}

#[cfg(test)]
mod tests {
    /// A home directory to scan, with the layouts that decide what is found.
    struct Home {
        /// Kept, not just its path: dropping a `TempDir` deletes the directory,
        /// and a home directory that is not there is not a home directory.
        dir: tempfile::TempDir,
    }

    impl Home {
        fn new() -> Self {
            Home {
                dir: tempfile::tempdir().expect("tempdir"),
            }
        }
        fn path(&self) -> &Path {
            self.dir.path()
        }
        /// A repository, as `git clone` leaves one.
        fn repo(&self, rel: &str) -> PathBuf {
            let path = self.path().join(rel);
            std::fs::create_dir_all(path.join(".git")).expect("repo");
            std::fs::write(path.join(".git/HEAD"), "ref: refs/heads/main\n").expect("HEAD");
            path
        }
        fn dir(&self, rel: &str) -> PathBuf {
            let path = self.path().join(rel);
            std::fs::create_dir_all(&path).expect("dir");
            path
        }
    }

    /// Backdate a path, so two repositories can be told apart by when they were
    /// last written rather than by what they are called.
    ///
    /// `utimensat` rather than a crate: this is the only place the tests need to
    /// write a timestamp into the past, and it is a syscall the codebase already
    /// links for its own reasons.
    fn filetime_set(path: &Path, when: std::time::SystemTime) {
        let secs = when
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after 1970")
            .as_secs() as libc::time_t;
        let times = [
            libc::timespec {
                tv_sec: secs,
                tv_nsec: 0,
            },
            libc::timespec {
                tv_sec: secs,
                tv_nsec: 0,
            },
        ];
        // A `CString`, not `OsStr::as_bytes`: `utimensat` reads a C string to its
        // NUL, so a pointer to the raw path bytes is not one — and it works
        // only when the byte after the path happens to be a zero, which is a
        // property of the allocator rather than of this code. Under a full test
        // run it reliably was not, and the call failed.
        let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
            .expect("a path with no interior NUL");
        // SAFETY: `c` is a NUL-terminated string holding exactly the bytes of
        // `path` and outliving the call, `times` holds the two entries
        // `utimensat` reads for the two file times being set, and the flags ask
        // it to follow a symlink — the default — rather than set the link's own.
        let rc = unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), times.as_ptr(), 0) };
        assert_eq!(
            rc,
            0,
            "utimensat on {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        );
    }

    /// The scan is the answer to "I just pulled a repository": both layouts that
    /// put a project one and two levels down are found, and a directory that is
    /// not a repository is not offered as one.
    #[test]
    fn a_repository_is_found_whether_it_sits_at_the_top_or_one_level_down() {
        let home = Home::new();
        let top = home.repo("project");
        let nested = home.repo("code/other");

        assert_eq!(repos_under(home.path()), vec![nested, top]);
    }

    /// A directory that merely contains a repository is not itself one, and a
    /// plain directory is not offered at all — the difference between `~/code`
    /// and `~/code/project` is the whole point of going two levels.
    #[test]
    fn a_directory_merely_holding_repositories_is_not_itself_one() {
        let home = Home::new();
        home.repo("code/project");
        let plain = home.dir("notes");

        let found = repos_under(home.path());
        assert!(!found.contains(&plain), "{found:?}");
        assert!(!found.contains(&home.path().join("code")), "{found:?}");
    }

    /// Hidden directories are configuration, not projects, and the large ones
    /// under the home directory are where a naive walk spends its time.
    #[test]
    fn hidden_directories_are_not_scanned() {
        let home = Home::new();
        let hidden = home.repo(".config/thing");
        let cache = home.repo(".cache/vcs/thing");

        assert_eq!(repos_under(home.path()), Vec::<PathBuf>::new());
        assert!(!repos_under(home.path()).contains(&hidden));
        assert!(!repos_under(home.path()).contains(&cache));
    }

    /// Two levels is a decision, not an accident: a repository below that is not
    /// found, and the scan does not walk into it to find out.
    #[test]
    fn the_scan_stops_at_two_levels() {
        let home = Home::new();
        let too_deep = home.repo("code/project/vendor/thing");

        assert_eq!(repos_under(home.path()), Vec::<PathBuf>::new());
        assert!(!repos_under(home.path()).contains(&too_deep));
    }

    /// A checkout linked into the home directory from elsewhere is still a place to
    /// start an agent, so it is offered — and following the link cannot run away,
    /// because a repository is never descended into.
    #[test]
    fn a_repository_reached_through_a_symlink_is_offered() {
        let outside = tempfile::tempdir().expect("outside");
        let repo = outside.path().join("elsewhere");
        std::fs::create_dir_all(repo.join(".git")).expect("repo");
        let home = Home::new();
        let link = home.path().join("linked");
        std::os::unix::fs::symlink(&repo, &link).expect("symlink");

        assert_eq!(repos_under(home.path()), vec![link]);
    }

    /// A repository inside a repository is that repository's business, not a
    /// separate suggestion — a submodule is the case that makes this matter.
    #[test]
    fn a_repository_inside_a_repository_is_not_a_separate_answer() {
        let home = Home::new();
        let outer = home.repo("project");
        let inner = home.repo("project/vendor/sub");

        assert_eq!(repos_under(home.path()), vec![outer]);
        assert!(!repos_under(home.path()).contains(&inner));
    }

    /// A freshly pulled repository outranks an old one, which is the reason for
    /// asking the filesystem at all.
    #[test]
    fn a_repository_is_offered_before_one_pulled_long_ago() {
        let home = Home::new();
        let old = home.repo("old");
        let new = home.repo("new");
        // Two different times, so the order cannot come from the paths.
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 60 * 24);
        filetime_set(&old.join(".git"), long_ago);

        assert_eq!(repos_under(home.path()), vec![new, old]);
    }
    use super::*;

    /// A path being spelled is completed against the filesystem, which is the
    /// half of the field no list of known projects can answer.
    #[test]
    fn a_typed_path_offers_the_directories_under_it() {
        let root = tempfile::tempdir().expect("tempdir");
        for name in ["alpha", "album", "beta", ".hidden"] {
            std::fs::create_dir(root.path().join(name)).expect("mkdir");
        }
        std::fs::write(root.path().join("afile"), "").expect("write");

        let base = root
            .path()
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        // A trailing separator asks for everything inside, files excluded and
        // dotted names left out until they are asked for by name.
        let all = suggest(&format!("{base}/"), &[]);
        let names: Vec<String> = all
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["album", "alpha", "beta"]);

        let hidden = suggest(&format!("{base}/.h"), &[]);
        assert_eq!(hidden.len(), 1, "asked for by its dot: {hidden:?}");

        // Two that agree on `al` complete only as far as they agree.
        let hits = suggest(&format!("{base}/al"), &[]);
        assert_eq!(hits.len(), 2);
        assert_eq!(
            complete(&format!("{base}/a"), &hits),
            Some(format!("{base}/al"))
        );

        // One hit completes outright, with the separator that carries on.
        let one = suggest(&format!("{base}/be"), &[]);
        assert_eq!(
            complete(&format!("{base}/be"), &one),
            Some(format!("{base}/beta/"))
        );

        // Nothing left to add: Tab on a finished path must not redraw it.
        assert_eq!(complete(&format!("{base}/al"), &hits), None);
    }

    /// Tab fills a path in and then leaves it alone: a completion that redrew a
    /// path already finished would have Tab "completing" it forever.
    #[test]
    fn completing_keeps_the_separator_that_was_typed() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(root.path().join("beta")).expect("mkdir");
        let base = root
            .path()
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");

        let hits = suggest(&format!("{base}/be"), &[]);
        assert_eq!(hits.len(), 1, "{hits:?}");
        let filled = complete(&format!("{base}/be"), &hits).expect("something to add");
        assert_eq!(filled, format!("{base}/beta/"));
        // And having been filled in, it is finished: nothing more to add.
        assert_eq!(complete(&filled, &suggest(&filled, &[])), None);
    }

    /// The empty field is the case the feature exists for: nothing typed, and
    /// the projects agents have run in listed without having to be remembered.
    #[test]
    fn a_bare_word_matches_the_projects_already_known() {
        let known = vec![
            PathBuf::from("/home/x/cctop"),
            PathBuf::from("/home/x/work/api"),
            PathBuf::from("/srv/cctop-fork"),
        ];
        assert_eq!(suggest("", &known), known);
        // Substring, not prefix: what is remembered is the name, not the tree.
        assert_eq!(
            suggest("cctop", &known),
            vec![
                PathBuf::from("/home/x/cctop"),
                PathBuf::from("/srv/cctop-fork")
            ]
        );
        // Case is not something anyone recalls about a directory either.
        assert_eq!(suggest("API", &known).len(), 1);
        assert!(suggest("nothing-like-it", &known).is_empty());
    }

    /// A bare word names no directory to read, and must not be resolved against
    /// wherever cctop was started — that would offer children of an unrelated
    /// directory as if they were matches.
    #[test]
    fn a_bare_word_never_reads_the_filesystem() {
        assert_eq!(split("cctop"), None);
        assert!(suggest("src", &[]).is_empty());
    }
}
