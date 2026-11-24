//! Where Cursor keeps its agent chats: transcripts under each project's
//! `agent-transcripts/`, and the title and real folder of each in the global
//! `state.vscdb`'s `composerHeaders`. Reading the turns stays in cctop-core's
//! `session::cursor`.

use super::{Discovered, readonly, transcript_files};
use crate::mounts::{Mount, mounts_here};
use crate::time::ms_to_rfc3339;
use crate::{Provider, Surface, paths};
use rusqlite::{Connection, OpenFlags};
use std::collections::{HashMap, HashSet};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub fn created_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.created())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or_else(|| paths::file_mtime_ms(path))
}

fn project_slug(path: &Path, roots: &[PathBuf]) -> String {
    path.ancestors()
        .find(|p| {
            p.parent()
                .is_some_and(|parent| roots.iter().any(|root| parent == root))
        })
        .and_then(Path::file_name)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A subagent's transcript, `<parent>/subagents/<id>.jsonl`: part of its
/// parent's session, never one of its own. Only the immediate directory is
/// asked, so a root or home with `subagents` somewhere in its path hides
/// nothing.
fn is_subagent(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == "subagents")
}

/// The row one transcript makes, or `None` for a subagent's or a stray file.
pub fn summarize(path: PathBuf, roots: &[PathBuf]) -> Option<Discovered> {
    let session_id = path.file_stem()?.to_string_lossy().into_owned();
    if !paths::is_full_uuid(&session_id) || is_subagent(&path) {
        return None;
    }
    // Only files in Cursor's documented agent-transcripts subtree are sessions.
    if !path
        .components()
        .any(|c| c.as_os_str() == "agent-transcripts")
    {
        return None;
    }

    let mut session = Discovered::new(Provider::Cursor, session_id);
    session.surface = Surface::Editor;
    session.started_at = ms_to_rfc3339(created_ms(&path) as i64);
    session.last_active = ms_to_rfc3339(paths::file_mtime_ms(&path) as i64);
    // A subagent writes its own file while the parent's sits still waiting on
    // it, and the row is live for as long as either is being written.
    let touched = transcript_files(&path)
        .iter()
        .map(|f| paths::file_mtime_ms(f))
        .max()
        .unwrap_or(0);
    session.touched_at = ms_to_rfc3339(touched as i64);
    session.label_source = project_slug(&path, roots);
    session.data_file = Some(path);
    Some(session)
}

/// The mounts this process sees, read once: the table changes when someone
/// mounts a drive, which a running cctop can live without noticing.
static MOUNTS: LazyLock<Vec<Mount>> = LazyLock::new(mounts_here);

/// Whether `path` is on a Windows drive seen through WSL — the 9P mount
/// `/mnt/c` is, or WSL 1's drvfs. Asked of the mount table rather than of a
/// `/mnt/` prefix, because `automount.root` can put the drives anywhere.
pub fn on_windows_drive(path: &Path, mounts: &[Mount]) -> bool {
    mounts
        .iter()
        .filter(|m| path.starts_with(&m.point))
        .max_by_key(|m| m.point.as_os_str().len())
        .is_some_and(|m| matches!(m.fstype.as_str(), "9p" | "v9fs" | "drvfs"))
}

/// `file:` URI for SQLite's `immutable` open. A Windows profile name can hold
/// a space, and a `?` or `#` would end the path early, so those are escaped
/// along with `%` itself.
pub fn immutable_uri(path: &Path) -> String {
    use percent_encoding::{AsciiSet, CONTROLS, percent_encode};
    const ESCAPE: &AsciiSet = &CONTROLS.add(b' ').add(b'%').add(b'?').add(b'#');
    format!(
        "file:{}?immutable=1",
        percent_encode(path.as_os_str().as_bytes(), ESCAPE)
    )
}

/// Open a Cursor global database for reading.
///
/// Under WSL it usually sits on the Windows drive, written by a Cursor on the
/// other side of 9P. An ordinary read-only open there is not read-only in
/// effect: SQLite creates `-shm` and `-wal` files in Cursor's directory, and
/// its locks and shared memory mean nothing to a writer that cannot see them.
/// `immutable=1` takes no lock and writes nothing. Its price is that it reads
/// only what Cursor has checkpointed into the main file, and that a checkpoint
/// landing mid-read can surface as an error or a value that will not parse.
/// Both fail closed — the row keeps what its transcript says — and everything
/// read here is enrichment, so that is the price worth paying. On a local
/// filesystem locks work, and the ordinary open sees uncheckpointed commits
/// too.
///
/// Opened per call and never pooled: an immutable connection is promised the
/// file never changes, so one kept open would never see anything new.
pub fn open(db: &Path) -> rusqlite::Result<Connection> {
    match on_windows_drive_really(db, &MOUNTS) {
        Some(real) => open_immutable(&real),
        None => readonly(db),
    }
}

/// The file behind `db` when that is on a Windows drive. Asked of the resolved
/// path, since `~/.config/Cursor` linked into `/mnt/c` sits on the root mount
/// as written and on 9P in fact.
pub fn on_windows_drive_really(db: &Path, mounts: &[Mount]) -> Option<PathBuf> {
    let real = std::fs::canonicalize(db).unwrap_or_else(|_| db.to_path_buf());
    on_windows_drive(&real, mounts).then_some(real)
}

pub fn open_immutable(db: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        immutable_uri(db),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
}

/// What `composerHeaders` says about one chat.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Header {
    title: Option<String>,
    created_ms: Option<i64>,
    /// The workspace folder, only when it exists on this machine.
    dir: Option<PathBuf>,
}

impl Header {
    pub fn apply(&self, s: &mut Discovered) {
        if let Some(title) = &self.title {
            s.title = Some(title.clone());
        }
        if let Some(ms) = self.created_ms {
            s.started_at = ms_to_rfc3339(ms);
        }
        // The real folder rather than the project slug, which turns `/` and
        // `_` alike into `-` and so is not a path; with it the row has a
        // working directory and its recent writes resolve.
        if let Some(dir) = &self.dir {
            s.label_source = dir.to_string_lossy().into_owned();
        }
    }
}

/// The workspace a chat was opened in, as a folder here.
///
/// A WSL folder opened through Remote-WSL is `vscode-remote://wsl+<distro>`
/// with the Linux path as it is; one opened as a plain `\\wsl$\<Distro>\…`
/// folder is a `file` URI on authority `wsl$` whose path starts with the
/// distro. A multi-root workspace has no single folder at all, and a Windows
/// folder is no folder here, so both keep the slug — which is what `is_dir`
/// settles without having to know every shape.
pub fn workspace_dir(path: Option<String>, authority: Option<String>) -> Option<PathBuf> {
    let path = path?;
    let path = match authority.as_deref() {
        // `\\wsl.localhost\` is the spelling Windows 11 gives the same folder.
        Some("wsl$" | "wsl.localhost") => path
            .trim_start_matches('/')
            .split_once('/')
            .map(|(_distro, rest)| format!("/{rest}"))?,
        _ => path,
    };
    let dir = PathBuf::from(path);
    dir.is_dir().then_some(dir)
}

/// Every chat's header, by id, across `dbs`; the first database to name a
/// chat wins.
///
/// One query over `composerHeaders`, a table of about a kilobyte a chat — never
/// the `composerData` values, which are a hundred kilobytes each. An older
/// Cursor without the table, or a database that will not open, adds nothing.
pub fn headers_in(dbs: &[PathBuf]) -> HashMap<String, Header> {
    let mut all = HashMap::new();
    for db in dbs {
        let Ok(conn) = open(db) else { continue };
        let Ok(mut stmt) = conn.prepare(
            "SELECT composerId, createdAt, json_extract(value, '$.name'), \
             json_extract(value, '$.workspaceIdentifier.uri.path'), \
             json_extract(value, '$.workspaceIdentifier.uri.authority') \
             FROM composerHeaders",
        ) else {
            continue;
        };
        let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Header {
                    created_ms: row.get(1).ok(),
                    title: row
                        .get::<_, Option<String>>(2)
                        .ok()
                        .flatten()
                        .filter(|t| !t.is_empty()),
                    dir: workspace_dir(row.get(3).ok().flatten(), row.get(4).ok().flatten()),
                },
            ))
        }) else {
            continue;
        };
        for (id, header) in rows.flatten() {
            all.entry(id).or_insert(header);
        }
    }
    all
}

/// Every transcript under `roots`, and the ids of those that are a
/// subagent's.
pub fn transcripts_in(roots: &[PathBuf]) -> (Vec<PathBuf>, HashSet<String>) {
    // Transcripts live only under each project's `agent-transcripts/`; the
    // rest of a project directory is `mcps/` (up to ~130 files a project) and
    // `canvases/` with whole `node_modules` trees, which a walk of the root
    // would read only to find nothing. Recursive inside, for the
    // `<id>/<id>.jsonl` layout and its `subagents/`.
    let files: Vec<PathBuf> = roots
        .iter()
        .flat_map(|root| {
            paths::list_dir(root).into_iter().flat_map(move |slug| {
                paths::rglob(&root.join(slug).join("agent-transcripts"), ".jsonl")
            })
        })
        .collect();
    // Cursor sometimes keeps a subagent's transcript at the top level as well,
    // as `<id>/<id>.jsonl`, which only its id gives away.
    let subagents: HashSet<String> = files
        .iter()
        .filter(|f| is_subagent(f))
        .filter_map(|f| f.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    (files, subagents)
}

/// Newest first, one row per chat id.
///
/// Cursor can retain a copied transcript under two project slugs. Its UUID
/// remains the stable identity, so keep the newest copy.
pub fn newest_copy_only<T>(rows: &mut Vec<T>, found: impl Fn(&T) -> &Discovered) {
    rows.sort_by(|a, b| found(b).last_active.cmp(&found(a).last_active));
    let mut seen = HashSet::new();
    rows.retain(|s| seen.insert(found(s).session_id.clone()));
}

/// The rows under `roots`, titled and placed from `dbs`.
pub fn list_in(roots: &[PathBuf], dbs: &[PathBuf]) -> Vec<Discovered> {
    let (files, subagents) = transcripts_in(roots);
    let mut rows: Vec<_> = files
        .into_iter()
        .filter_map(|path| summarize(path, roots))
        .filter(|s| !subagents.contains(&s.session_id))
        .collect();
    newest_copy_only(&mut rows, |s| s);
    if !rows.is_empty() {
        let headers = headers_in(dbs);
        for s in &mut rows {
            if let Some(h) = headers.get(&s.session_id) {
                h.apply(s);
            }
        }
    }
    rows
}

/// Every Cursor chat on this machine.
pub fn list() -> Vec<Discovered> {
    list_in(&paths::cursor_projects_roots(), &paths::cursor_global_dbs())
}
