//! A session's checkout, for the session page's diff: what changed in it since
//! the session started, and one file's diff in full.
//!
//! The same two reads the terminal's sidebar and diff view make —
//! [`changes::read`] for the list, [`diff_file::read`] for a file — so the page
//! and the terminal agree about what a session changed and against which
//! commit. The page lays the diff out itself; this only hands over git's hunks
//! and the file's two versions.
//!
//! Both are reads of the session's own checkout on this machine. A sibling's
//! row is answered by the sibling (see [`crate::remote::peer_of`]); an ssh
//! host's has no checkout here to read and says so.
//!
//! A read-only link gets the list and the hunks, which is what the report
//! already shows of a session's edits, but not the two whole versions of a
//! file: those are the file, past anything the session touched. Without them
//! the page cannot open the unchanged stretches between hunks, and says so.

use std::net::TcpStream;

use cctop_core::{changes, diff_file};
use serde::Serialize;

use crate::api::find;
use crate::http::{self, Request, json};
use crate::{Access, NO_SUCH_SESSION, Shared, current};

/// What changed in a session's checkout.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "CheckoutChanges"))]
pub(crate) struct Changes {
    /// The checkout's root, with `~` for the home.
    pub root: String,
    /// The commit the diff is against, which a file's read is asked against.
    pub base: String,
    /// How that commit was chosen, in a phrase.
    pub base_note: String,
    pub files: Vec<File>,
    /// Files past the list's limit, counted and not listed.
    pub more_files: usize,
}

/// One changed file.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "CheckoutFile"))]
pub(crate) struct File {
    pub path: String,
    /// `null` for a binary file.
    pub added: Option<u64>,
    pub removed: Option<u64>,
    /// Never added to git: all of it new.
    pub untracked: bool,
}

/// One file's diff, as the page draws it.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "CheckoutDiff"))]
pub(crate) struct Diff {
    /// Git's diff from the first hunk on.
    pub diff: Vec<String>,
    /// The file at the base and now, by line; `null` where it does not exist,
    /// could not be read whole, or the link may not see it.
    pub old: Option<Vec<String>>,
    pub new: Option<Vec<String>>,
    pub binary: bool,
    pub too_big: bool,
}

/// The session `id`, when it is this machine's: or the answer already sent.
fn local<'a>(
    snapshot: &'a crate::Snapshot,
    stream: &mut TcpStream,
    request: &Request,
    id: &str,
) -> Option<&'a cctop_core::session::Session> {
    let Some(session) = find(&snapshot.sessions, id) else {
        http::respond_error(stream, Some(request), 404, NO_SUCH_SESSION);
        return None;
    };
    if session.remote.is_some() {
        http::respond_error(
            stream,
            Some(request),
            409,
            "this session's checkout is on another machine; its own cctop serve shows it",
        );
        return None;
    }
    Some(session)
}

/// `/api/changes/<id>`: the checkout's changed files, or `null` when the
/// session's directory is in no repository.
pub(crate) fn api_changes(shared: &Shared, stream: &mut TcpStream, request: &Request, id: &str) {
    let snapshot = current(shared);
    let Some(session) = local(&snapshot, stream, request, id) else {
        return;
    };
    let Some(dir) = session.work_dir() else {
        return json(stream, request, &None::<Changes>);
    };
    match changes::read(&dir, changes::since(session)) {
        Ok(found) => json(
            stream,
            request,
            &found.map(|c| Changes {
                root: cctop_core::util::tildify(&c.root.to_string_lossy()),
                base: c.base.commit.clone(),
                base_note: c.base.kind.describe().to_string(),
                more_files: c.more_files,
                files: c
                    .files
                    .into_iter()
                    .map(|f| File {
                        path: f.path,
                        added: f.added,
                        removed: f.removed,
                        untracked: f.untracked,
                    })
                    .collect(),
            }),
        ),
        Err(why) => http::respond_error(stream, Some(request), 500, &why),
    }
}

/// A commit's object id as git prints it: forty hex digits, or sixty-four in
/// a SHA-256 repository. Anything else is not a base this page was given.
fn is_commit(base: &str) -> bool {
    matches!(base.len(), 40 | 64) && base.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `/api/diff/<id>?path=…&base=…&untracked=1`: one file of the checkout,
/// against the base `/api/changes` named.
pub(crate) fn api_diff(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    id: &str,
    access: Access,
) {
    let snapshot = current(shared);
    let Some(session) = local(&snapshot, stream, request, id) else {
        return;
    };
    let (Some(path), Some(base)) = (request.query.get("path"), request.query.get("base")) else {
        return http::respond_error(stream, Some(request), 400, "a diff needs a path and a base");
    };
    if !is_commit(base) {
        return http::respond_error(stream, Some(request), 400, "the base is not a commit id");
    }
    let untracked = request.query.get("untracked").is_some_and(|v| v == "1");
    // The checkout's root, as the list found it: a path is relative to it.
    let Some(dir) = session.work_dir() else {
        return http::respond_error(
            stream,
            Some(request),
            404,
            "the session's directory is gone",
        );
    };
    let Some((_, root)) = cctop_core::worktree::locate(&dir) else {
        return http::respond_error(stream, Some(request), 404, "not a git checkout");
    };
    // `diff_file::read` refuses a path that is absolute or climbs out.
    match diff_file::read(&root, base, path, untracked) {
        Ok(view) => {
            let whole = access == Access::Full;
            json(
                stream,
                request,
                &Diff {
                    diff: view.diff,
                    old: view.old.filter(|_| whole),
                    new: view.new.filter(|_| whole),
                    binary: view.binary,
                    too_big: view.too_big,
                },
            );
        }
        Err(why) => http::respond_error(stream, Some(request), 400, &why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a commit id passes as a base: not a ref, not an option, not a
    /// path, not a range.
    #[test]
    fn a_base_is_a_commit_id_and_nothing_else() {
        assert!(is_commit(&"a".repeat(40)));
        assert!(is_commit(&"0123456789abcdef".repeat(4)));
        for bad in [
            "HEAD",
            "main",
            "--output=/tmp/x",
            "../x",
            "",
            &"g".repeat(40),
            &"a".repeat(39),
        ] {
            assert!(!is_commit(bad), "{bad:?}");
        }
    }
}
