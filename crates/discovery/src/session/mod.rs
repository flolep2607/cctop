//! Reading the files agents write their sessions to.
//!
//! The per-format line parsers are public so cctop-core can use them, but they
//! are internals, not the interface this crate offers: hidden from the docs,
//! and outside what `cargo-semver-checks` holds to a version.

use crate::{Provider, Surface};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

#[doc(hidden)]
pub mod antigravity;
#[doc(hidden)]
pub mod claude;
#[doc(hidden)]
pub mod claude_line;
#[doc(hidden)]
pub mod codex;
#[doc(hidden)]
pub mod codex_line;
#[doc(hidden)]
pub mod cursor;
#[doc(hidden)]
pub mod cursor_line;
#[doc(hidden)]
pub mod devin;
#[doc(hidden)]
pub mod devin_atif;
#[doc(hidden)]
pub mod gemini;
#[doc(hidden)]
pub mod gemini_line;
#[doc(hidden)]
pub mod lenient;
#[doc(hidden)]
pub mod opencode;
#[doc(hidden)]
pub mod opencode_line;
#[doc(hidden)]
pub mod pi;
#[doc(hidden)]
pub mod pi_line;
#[doc(hidden)]
pub mod vibe;
#[doc(hidden)]
pub mod windsurf;

/// Lines above this size are almost always base64 image payloads carrying no
/// token, cost, or tool data. Skipping them keeps large transcripts cheap.
pub const MAX_JSONL_LINE_BYTES: usize = 512 * 1024;

/// Stream a `.jsonl` file's non-blank lines, trimmed, to a reader that parses
/// them into its own type.
///
/// The second argument says the line is over [`MAX_JSONL_LINE_BYTES`]: a
/// reader should take only what it cannot do without from such a line, as
/// cctop-core's `for_each_jsonl` does, rather than all of it.
pub fn for_each_line<F: FnMut(&[u8], bool)>(path: &Path, mut f: F) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut buf = Vec::with_capacity(8 * 1024);

    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        let line = buf.trim_ascii();
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_JSONL_LINE_BYTES {
            f(line, true);
            // Don't let one giant line keep its buffer for the rest of the file.
            buf = Vec::with_capacity(8 * 1024);
            continue;
        }
        f(line, false);
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Tokens {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write_5m: u64,
    #[serde(default)]
    pub cache_write_1h: u64,
    // Codex-only
    #[serde(default)]
    pub input_total: u64,
    #[serde(default)]
    pub cached_input: u64,
    #[serde(default)]
    pub reasoning_output: u64,
    #[serde(default)]
    pub total: u64,
}

impl Tokens {
    /// Everything billed as input, across both providers' shapes.
    pub fn all_input(&self) -> u64 {
        self.input + self.cached_input + self.cache_read + self.cache_write_5m + self.cache_write_1h
    }

    /// A prompt whose cached part is counted inside it, not beside it — the
    /// shape Gemini and Devin both report, as Codex does — split into the
    /// uncached remainder and the cached part, so `all_input` rebuilds the
    /// prompt. A cached count above the prompt is clamped, not underflowed.
    pub fn from_prompt(prompt: u64, cached: u64, output: u64) -> Tokens {
        let cached = cached.min(prompt);
        Tokens {
            input: prompt - cached,
            input_total: prompt,
            cached_input: cached,
            output,
            ..Default::default()
        }
    }

    /// Add `other` into this, every field: one sum shared by every reader, so
    /// a field one provider fills is never summed by one copy and dropped by
    /// another.
    ///
    /// Saturating, for the reason the Claude parser gives at its own sums:
    /// these are numbers read out of a file any process on the machine can
    /// write, and one transcript claiming `u64::MAX` tokens would overflow
    /// every debug build and wrap to a nonsense cost in a release one.
    pub fn add(&mut self, other: &Tokens) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write_5m = self.cache_write_5m.saturating_add(other.cache_write_5m);
        self.cache_write_1h = self.cache_write_1h.saturating_add(other.cache_write_1h);
        self.input_total = self.input_total.saturating_add(other.input_total);
        self.cached_input = self.cached_input.saturating_add(other.cached_input);
        self.reasoning_output = self.reasoning_output.saturating_add(other.reasoning_output);
        self.total = self.total.saturating_add(other.total);
    }
}

/// Paths backing a Claude for Mac session, needed to delete it cleanly.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct MacMeta {
    pub meta_path: PathBuf,
    pub session_dir: PathBuf,
}

/// A session as its agent's files describe it: who, where, since when, and
/// which file. What every provider's `list_sessions` fills in before anything
/// is extracted or annotated. cctop-core's `Session` holds one and adds what
/// it annotates.
///
/// Hidden: its fields follow cctop's needs, and the record other programs
/// read is a smaller one built from it.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct Discovered {
    pub provider: Provider,
    pub surface: Surface,
    pub session_id: String,
    pub started_at: String,
    /// When the session last said or did something: the timestamp of the
    /// newest message or tool call in its transcript, which is what the table's
    /// "last" column shows and sorts by. A transcript is also written to
    /// without anything happening — a title, a summary, a file-history snapshot
    /// on resume — and that is not activity a person would recognise.
    pub last_active: String,
    /// When the transcript was last written to, by whatever: its file's mtime,
    /// newest across subagent transcripts too. Empty for a provider that has no
    /// file of its own to stat. What liveness and cache stamps want, where any
    /// write counts — read it through cctop-core's `Session::touched`.
    pub touched_at: String,
    pub model: String,
    /// Where the agent is hosted, when it can be inferred (for example Cursor
    /// versus a terminal CLI). This is intentionally distinct from `model`.
    pub harness: String,
    /// Working directory the session was launched from.
    pub label_source: String,
    pub data_file: Option<PathBuf>,
    pub title: Option<String>,
    pub mac_meta: Option<MacMeta>,
    /// The session id this session's process was *started* with, which stops
    /// being its own id the moment it is resumed.
    ///
    /// `claude --resume X` does not reopen X's transcript. It forks: a new file
    /// under a new id, which records X as the id it was launched from. So the
    /// only thing on the running process's command line is the id of a
    /// transcript that is now finished, and matching a PID to the session whose
    /// *name* it carries attributes the live agent to the dead conversation —
    /// the running one then shows as stopped while a session nobody is in shows
    /// as working. Matching on this instead follows the fork forward.
    ///
    /// Empty when the harness does not record one, which includes every
    /// provider but Claude and Claude transcripts old enough to predate the
    /// field. Callers fall back to `session_id`, which is
    /// what this holds for a session that was never resumed.
    pub launch_id: String,
    /// Login name of the user whose home this session was read out of, when
    /// that is not the user running cctop.
    ///
    /// Only ever set when cctop is sweeping other homes — running as root, or
    /// told to by `$CCTOP_ALL_USERS` — so `None` on an ordinary run means
    /// "mine" rather than "unknown". Stamped from the transcript's path in
    /// cctop-core's `list_all`, which is the one place every provider's rows meet.
    pub owner: Option<String>,
}

impl AsRef<Discovered> for Discovered {
    fn as_ref(&self) -> &Discovered {
        self
    }
}

impl Discovered {
    /// Stable identity used as a map key across refreshes.
    pub fn key(&self) -> String {
        format!("{}:{}", self.provider.as_str(), self.session_id)
    }

    /// [`Self::key`] written into a buffer the caller owns and reused, for the
    /// passes that match every session against a map keyed by it.
    ///
    /// `key` itself is right almost everywhere and stays: it hands back
    /// something the caller can keep. What it cannot do is a *lookup*, and the
    /// sweeps that need one — the process attribution, the rate table, the tail
    /// cache — run over every session on every refresh, so a fresh `String` per
    /// row is a per-row allocation for a key that is thrown away a line later.
    /// One buffer per pass turns that into one allocation however many rows
    /// there are.
    ///
    /// What is left still costs: a pass that *writes* to one of those maps has
    /// to own the key it writes. That is the minority of the calls, and for the
    /// rate table — the only one retained across walks — it is the first
    /// sighting of a session and never again.
    pub fn key_into<'a>(&self, buf: &'a mut String) -> &'a str {
        use std::fmt::Write as _;
        buf.clear();
        let _ = write!(buf, "{}:{}", self.provider.as_str(), self.session_id);
        buf
    }

    /// When anything was last written for this session: `touched_at`,
    /// or `last_active` for a provider that only has the one.
    pub fn touched(&self) -> &str {
        match self.touched_at.is_empty() {
            true => &self.last_active,
            false => &self.touched_at,
        }
    }

    /// The id a running process would name to mean this session: the one it was
    /// launched with, or its own when it was never resumed from anything.
    pub fn launched_as(&self) -> &str {
        match self.launch_id.is_empty() {
            true => &self.session_id,
            false => &self.launch_id,
        }
    }

    /// A session known by its provider and id alone, everything else blank:
    /// the start every lister fills in from what the agent's files say.
    pub fn new(provider: Provider, session_id: String) -> Self {
        Discovered {
            provider,
            surface: Surface::Cli,
            session_id,
            started_at: String::new(),
            last_active: String::new(),
            touched_at: String::new(),
            model: String::new(),
            harness: String::new(),
            label_source: String::new(),
            data_file: None,
            title: None,
            mac_meta: None,
            launch_id: String::new(),
            owner: None,
        }
    }
}

#[doc(hidden)]
/// Open another harness's SQLite database for reading, and only reading.
///
/// Read-only because the database belongs to an agent that may be writing it
/// right now, and cctop is a monitor: it has no business taking a write lock
/// on somebody else's state. No-mutex because each connection is used by one
/// thread — OpenCode's are kept per thread for that reason — so SQLite's own
/// serialisation would be a lock taken and released for nothing on every
/// call.
pub fn readonly(path: &Path) -> rusqlite::Result<rusqlite::Connection> {
    use rusqlite::OpenFlags;
    rusqlite::Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

/// Hand lines from the head of a file to `f`, which says whether it could
/// read each one, until `max_lines` of them have been read.
pub fn scan_first_lines<F: FnMut(&[u8]) -> bool>(path: &Path, max_lines: usize, mut f: F) {
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let reader = BufReader::with_capacity(64 * 1024, file);
    let mut read = 0;
    for line in reader.lines().map_while(Result::ok) {
        if read >= max_lines {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if f(line.as_bytes()) {
            read += 1;
        }
    }
}

/// Hand lines from the tail of a file to `f`, newest first, until it has
/// read `max_lines` of them; `f` says whether it could read each one. See
/// cctop-core's `read_last_lines`.
pub fn scan_last_lines<F: FnMut(&[u8]) -> bool>(path: &Path, max_lines: usize, mut f: F) {
    const CHUNK: u64 = 64 * 1024;
    let Ok(mut file) = std::fs::File::open(path) else {
        return;
    };
    let Ok(size) = file.metadata().map(|m| m.len()) else {
        return;
    };

    let mut pos = size;
    let mut carry: Vec<u8> = Vec::new();
    let mut read = 0;

    while pos > 0 && read < max_lines {
        let read_size = CHUNK.min(pos);
        pos -= read_size;
        if file.seek(SeekFrom::Start(pos)).is_err() {
            break;
        }
        let mut chunk = vec![0u8; read_size as usize];
        if file.read_exact(&mut chunk).is_err() {
            break;
        }
        // Join raw bytes, not text: a multi-byte character straddling the chunk
        // boundary would otherwise become U+FFFD on both sides and the line
        // would fail to parse. One lossy conversion per assembled line instead.
        chunk.extend_from_slice(&carry);
        let mut lines: Vec<&[u8]> = chunk.split(|b| *b == b'\n').collect();

        // The first element may be a partial line continuing into the previous
        // chunk; hold it back unless we've reached the start of the file.
        carry = if pos > 0 && !lines.is_empty() {
            lines.remove(0).to_vec()
        } else {
            Vec::new()
        };

        for line in lines.iter().rev() {
            if read >= max_lines {
                break;
            }
            let line = line.trim_ascii();
            if line.is_empty() {
                continue;
            }
            if f(String::from_utf8_lossy(line).as_bytes()) {
                read += 1;
            }
        }
    }
}

/// How far back from the end [`last_entry_ms`] looks for a real entry.
///
/// Metadata a harness writes after the last turn — a title, a summary, a usage
/// tally — runs to a handful of lines, not hundreds; a tail with nothing real
/// in it this far up falls back to the file's mtime.
const LAST_ENTRY_LINES: usize = 200;

/// What [`last_entry_ms`] found for a file, and the `(mtime, size)` it was read at.
type LastEntry = (u64, u64, Option<i64>);

static LAST_ENTRY: LazyLock<Mutex<HashMap<PathBuf, LastEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The newest timestamp, in Unix milliseconds, among the entries near the end
/// of `path` that `entry_ms` recognises as a message or tool call, or `None`
/// when there is none — the file missing, or nothing in its tail that counts.
///
/// Discovery asks this of every transcript on every walk, so the answer is
/// kept against the file's mtime and size: a finished session costs a `stat`,
/// and only a file that has been written to since is read again — its tail,
/// not the whole of it.
pub fn last_entry_ms(path: &Path, entry_ms: impl Fn(&[u8]) -> Option<i64>) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let stamp = (crate::paths::mtime_ms_of(&meta), meta.len());
    if let Ok(cache) = LAST_ENTRY.lock()
        && let Some(&(mtime, size, found)) = cache.get(path)
        && (mtime, size) == stamp
    {
        return found;
    }
    let mut newest: Option<i64> = None;
    scan_last_lines(path, LAST_ENTRY_LINES, |bytes| {
        if let Some(ms) = entry_ms(bytes) {
            newest = Some(newest.map_or(ms, |n| n.max(ms)));
        }
        true
    });
    if let Ok(mut cache) = LAST_ENTRY.lock() {
        cache.insert(path.to_path_buf(), (stamp.0, stamp.1, newest));
    }
    newest
}

/// Whether a line is Claude Code reporting an `/effort` switch, and if so
/// whether the switch was into ultracode.
///
/// The command's own output, which Claude Code writes into the transcript in
/// the person's role:
///
/// ```text
/// <local-command-stdout>Set effort level to ultracode (this session only): …
/// ```
///
/// Anchored at the start of the line, so the same words pasted into a prompt
/// (a screenshot of a terminal, say) are not read as a switch.
pub fn effort_switch(text: &str) -> Option<bool> {
    let level = text
        .trim_start()
        .strip_prefix("<local-command-stdout>")?
        .trim_start()
        .strip_prefix("Set effort level to ")?;
    let level: String = level.chars().take_while(|c| c.is_alphanumeric()).collect();
    (!level.is_empty()).then(|| level.eq_ignore_ascii_case("ultracode"))
}

/// The main transcript plus any subagent sidechain transcripts — including the
/// agents a workflow ran, which are one level further down.
///
/// A workflow writes each of its agents to
/// `subagents/workflows/<run>/agent-<id>.jsonl`, beside a `journal.jsonl` that
/// records the run's own progress and is not an agent's transcript at all. So
/// inside a run directory only `agent-` files count, while at the top level
/// every `.jsonl` still does, as it always has. A workflow can run hundreds of
/// agents, and before this every one of them was missing from the session's
/// spend, its subagent list, and the mtime that says it is still working.
pub fn transcript_files(main: &Path) -> Vec<PathBuf> {
    let mut files = vec![main.to_path_buf()];
    let subagents_dir = subagents_dir(main);
    for entry in crate::paths::list_dir(&subagents_dir) {
        if entry.ends_with(".jsonl") {
            files.push(subagents_dir.join(entry));
        }
    }
    for run in workflow_run_dirs(&subagents_dir) {
        for entry in crate::paths::list_dir(&run) {
            if entry.starts_with("agent-") && entry.ends_with(".jsonl") {
                files.push(run.join(entry));
            }
        }
    }
    files
}

/// Where a session's subagents keep their transcripts.
///
/// Claude Code puts them in a directory named after the transcript,
/// `<id>.jsonl` beside `<id>/subagents/`. Cursor already keeps each transcript
/// in a directory of its own, `<id>/<id>.jsonl`, and its subagents in
/// `<id>/subagents/` beside it. A Claude Code transcript's directory is its
/// project, never its own id, so the two cannot be mistaken for each other.
pub fn subagents_dir(main: &Path) -> PathBuf {
    match main.parent() {
        Some(dir) if dir.file_name().is_some() && dir.file_name() == main.file_stem() => {
            dir.join("subagents")
        }
        _ => main.with_extension("").join("subagents"),
    }
}

/// Each workflow run's directory under a session's `subagents/`.
pub fn workflow_run_dirs(subagents_dir: &Path) -> Vec<PathBuf> {
    let workflows = subagents_dir.join("workflows");
    crate::paths::list_dir(&workflows)
        .into_iter()
        .map(|run| workflows.join(run))
        .filter(|run| run.is_dir())
        .collect()
}
