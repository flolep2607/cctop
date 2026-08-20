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
    // Windows takes either separator, and the field is the user's own line: a
    // Tab that answered `C:/src/a` with `C:\src\alpha` would respell the path
    // it was completing, and the check below would never be true again — two
    // spellings of one directory never compare equal, so Tab on a finished path
    // would redraw it forever.
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

#[cfg(test)]
mod tests {
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

        // Spelled with `/` throughout, which Windows accepts as readily as its
        // own separator: the assertions below are about what the field does
        // with a path, not about which of the two the platform prefers.
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

    /// Tab answers in the spelling it was asked in.
    ///
    /// On Windows this is the difference between a field that settles and one
    /// that rewrites itself: `suggest` joins with `\`, the line was typed with
    /// `/`, and a completion that swapped them would leave the two spellings
    /// unequal forever — so Tab would keep "completing" a path that was already
    /// finished. Elsewhere there is only one separator and this is a tautology.
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
