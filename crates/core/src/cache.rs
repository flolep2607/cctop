//! On-disk caches: extracted session data, and persisted UI preferences.

use crate::config;
use crate::session::{Session, SessionData};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Identity of the code that produced a cache entry, so a mismatch discards the
/// whole cache. This replaces the pile of ad-hoc `_hasSubagentsField`-style
/// probes the JS version accumulated as its schema drifted.
///
/// It is derived by `build.rs` from the bytes of every source that decides what
/// an entry means — `src/session/`, this file, `pricing.rs`, `config.rs` —
/// rather than being a number a human remembers to bump. The manual version was
/// forgotten exactly when it mattered: a field added to `SessionData` without a
/// bump deserializes as `None` under `#[serde(default)]`, and a finished
/// transcript never changes again, so its panel stays blank forever.
///
/// The tradeoff is deliberate: *any* edit to a parser invalidates every cached
/// session, whitespace and comments included. A re-parse costs a fraction of a
/// second per session and happens once; serving a stale wrong panel costs
/// correctness and lasts until the user thinks to pass `--clear-cache`.
const CACHE_VERSION: &str = env!("CCTOP_CACHE_HASH");

/// One cached extraction.
///
/// The transcript path is a field rather than something recovered from the key.
/// The key names the transcript — the path itself where a file holds one
/// session, the path and the session id where it holds several — and a path may
/// contain `|` itself, so a key can never be split back into its parts. That is
/// not hypothetical: `save` used to recover the path with `key.split('|').next()`,
/// which truncated an awkward path, and those sessions were dropped on every
/// save and re-parsed forever.
#[derive(Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    /// Which session inside `path` this describes.
    ///
    /// The key names it too, but the key is a rendering of it — `Path::display`
    /// is — and this is the field to read when someone asks what an entry in the
    /// cache file is about.
    #[serde(default)]
    session: String,
    /// Which version of the transcript is stored: the part of the key that moves
    /// when the file does.
    ///
    /// A hit is this string being equal to the one derived from the transcript
    /// right now, which is the whole invalidation rule. It used to be embedded
    /// in the map key, which meant the superseded entry had to be *found* before
    /// it could be dropped; see [`CostCache::put`].
    #[serde(default)]
    stamp: String,
    /// Unix millis of the write that stored this, used to evict the oldest
    /// entries when the cache outgrows its bound.
    #[serde(default)]
    stored_at: u64,
    data: SessionData,
}

/// A transcript's identity in the cache, as two halves.
///
/// Which slot the entry occupies, and which version of the transcript that slot
/// holds. Splitting them is what lets [`CostCache::put`] replace an entry
/// without looking for it: a session has one slot, whatever its file has done
/// since, so storing a new version is a single insert with nothing to search
/// for first.
#[derive(Debug, PartialEq, Eq)]
pub struct DiskId {
    /// The map key: the transcript, plus the session inside it for the one
    /// provider that keeps many sessions in one file.
    origin: String,
    /// What the stored version has to match to be served.
    stamp: String,
}

#[derive(Deserialize)]
struct DiskCache {
    version: String,
    entries: HashMap<String, Entry>,
}

/// Serializing view, so `save` writes the live map instead of cloning it.
#[derive(Serialize)]
struct DiskCacheRef<'a> {
    version: &'a str,
    entries: &'a HashMap<String, Entry>,
}

/// Upper bound on retained entries. Sessions are never explicitly deleted from
/// the user's disk, so without this the cache only ever grows.
///
/// Raised from 2000 once entries stopped carrying the tool history, which took
/// them from ~67 KB to under 2 KB. The old figure was set when 2000 entries
/// meant a 124 MB file, and a machine with 2000 sessions — a shared server is
/// the ordinary case — sat exactly on it, re-parsing whatever spilled over on
/// every single run.
const MAX_ENTRIES: usize = 20_000;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Where a session's cache entry lives, and which version of the transcript that
/// entry holds. `None` where no honest identity can be formed.
///
/// Most providers give a session its own transcript, so the file *is* the
/// session's: one slot per file, and the stamp alone says whether what is
/// stored still describes it. OpenCode keeps every session in one database,
/// where the file says nothing about any single one — its size and mtime move
/// when *any* session is written — so its slot has to name the session as well,
/// and its stamp is the `time_updated` that discovery already read out of the
/// `session` row and put in `last_active`. That is exactly the stamp that moves
/// when this session gains a message, so it invalidates for the session that
/// changed and no other — better than an mtime, which cannot tell them apart.
/// A trace from a 2019-session machine put 1478 of them in OpenCode, 84% of all
/// the time spent extracting, repeated in full on every run because none of it
/// could be kept.
///
/// Windsurf keeps its conversations in one settings blob much as OpenCode does,
/// but nothing in that blob dates them per conversation, so there is no honest
/// key to build and it stays uncached.
fn disk_id(session: &Session, file: &Path) -> Option<DiskId> {
    match session.provider {
        crate::pricing::Provider::Windsurf => None,
        crate::pricing::Provider::OpenCode => Some(DiskId {
            // NUL, because a path can hold `|` but never a NUL, and this is the
            // one place the two halves have to be told apart. It is only ever
            // compared whole, never split, so this is belt and braces.
            origin: format!("{}\u{0}{}", file.display(), session.session_id),
            stamp: format!(
                "{}|p{}",
                session.last_active,
                crate::pricing::pricing_epoch()
            ),
        }),
        _ => cache_key(file),
    }
}

/// A file-per-session transcript's cache identity: the slot it occupies, and the
/// stamp that says whether what was read from it still describes it.
///
/// The stamp is size, mtime and the pricing generation. Any append changes size
/// or mtime, so a stale entry can never be mistaken for a fresh one; and it
/// carries the pricing generation because cached entries hold computed costs
/// rather than raw tokens, so a refreshed rate table has to invalidate them just
/// as an appended transcript does.
///
/// For Claude sessions it also folds in the newest subagent mtime. The parent's
/// own mtime doesn't move while a subagent streams into its own file, and
/// neither does the `subagents/` directory mtime, which only changes when files
/// are added or removed.
fn cache_key(path: &Path) -> Option<DiskId> {
    let meta = std::fs::metadata(path).ok()?;
    let mut stamp = format!(
        "|{}|{}|p{}",
        meta.len(),
        config::file_mtime_ms(path),
        crate::pricing::pricing_epoch()
    );
    let sub_dir = path.with_extension("").join("subagents");
    if sub_dir.is_dir() {
        // Every transcript a session has, workflow agents included, and every
        // directory one could be removed from — a run's as well as the top's.
        let newest = crate::session::transcript_files(path)[1..]
            .iter()
            .map(|f| config::file_mtime_ms(f))
            .chain(std::iter::once(config::file_mtime_ms(&sub_dir)))
            .chain(
                crate::session::workflow_run_dirs(&sub_dir)
                    .iter()
                    .map(|run| config::file_mtime_ms(run)),
            )
            .max()
            .unwrap_or(0);
        stamp.push_str(&format!("|{newest}"));
    }
    Some(DiskId {
        origin: path.display().to_string(),
        stamp,
    })
}

pub struct CostCache {
    entries: Mutex<HashMap<String, Entry>>,
    dirty: Mutex<bool>,
}

impl Default for CostCache {
    fn default() -> Self {
        Self::load()
    }
}

/// Remove only the persisted session extraction cache.
///
/// UI preferences and the downloaded pricing table are intentionally retained:
/// this is the cache that can otherwise preserve outdated parser output.
pub fn clear_session_cache() -> anyhow::Result<bool> {
    match std::fs::remove_file(&*config::COST_CACHE_FILE) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

impl CostCache {
    pub fn load() -> Self {
        let _span = crate::trace::span("cache.load");
        let text = std::fs::read_to_string(&*config::COST_CACHE_FILE).ok();
        crate::trace::fact(
            "cache file",
            match &text {
                Some(t) => format!(
                    "{} ({} bytes)",
                    crate::trace::redact(&config::COST_CACHE_FILE),
                    t.len()
                ),
                None => "absent".into(),
            },
        );
        // A cache the running binary cannot use is the single most confusing
        // thing to debug from the outside: everything re-parses, every run, and
        // nothing says why. The three ways it happens look identical from a
        // stopwatch and want different fixes, so they are reported apart —
        // unreadable means the file is damaged and should be deleted, stale
        // means the binary changed and the next run will be fine.
        let parsed = match text.as_deref().map(serde_json::from_str::<DiskCache>) {
            None => None,
            Some(Ok(c)) => {
                if c.version != CACHE_VERSION {
                    crate::trace::fact(
                        "cache state",
                        format!("stale — file {} vs binary {CACHE_VERSION}", c.version),
                    );
                }
                Some(c)
            }
            Some(Err(error)) => {
                crate::trace::fact("cache state", format!("unreadable — {error}"));
                None
            }
        };
        let mut entries = parsed
            .filter(|c| c.version == CACHE_VERSION)
            .map(|c| c.entries)
            .unwrap_or_default();
        let stored = entries.len();
        // Deleted transcripts are the only entries worth a `stat`, and once per
        // process start is enough: an entry whose file merely *changed* is
        // superseded by key on the next `put`, at no IO cost at all.
        entries.retain(|_, e| e.path.exists());
        crate::trace::fact(
            "cache entries",
            format!("{} usable, {} gone", entries.len(), stored - entries.len()),
        );
        CostCache {
            entries: Mutex::new(entries),
            dirty: Mutex::new(false),
        }
    }

    /// The extraction stored for `id`, if it is the version of the transcript
    /// that `id` names.
    ///
    /// The stamp comparison is the invalidation rule and it used to be implicit:
    /// the stamp was part of the map key, so a hit *was* the match. Comparing it
    /// here instead is the same rule stated where it is read.
    pub fn get(&self, id: &DiskId) -> Option<Arc<SessionData>> {
        // Still a copy of the extraction, because `Entry` is the on-disk shape
        // and cannot be an `Arc` without serde's `rc` feature — but one, where
        // this used to take two: one here and another inserting the same data
        // into the memory layer. Wrapping it here lets those two share the copy
        // this line already paid for.
        self.entries
            .lock()
            .ok()?
            .get(&id.origin)
            .filter(|e| e.stamp == id.stamp)
            .map(|e| Arc::new(e.data.clone()))
    }

    /// Store `data` as the current version of `id`, replacing whatever version
    /// of the same transcript was there.
    ///
    /// One insert, whatever the map holds. The superseded entry used to have to
    /// be found first — a `retain` over every entry comparing paths, on every
    /// store — so filling a cache of N sessions cost O(N²): measured on this
    /// machine, 0.56 s to fill 2000 and 90 s to fill 20 000, against 0.006 s and
    /// 0.05 s now.
    ///
    /// The alternative was a reverse index from `(path, session)` to the live
    /// key, and it was not taken: it works, but it stores a second copy of every
    /// path and session id purely to find what the map could be keyed by
    /// instead. Measured at 235 bytes of extra heap per entry — a permanent cost
    /// on a cache that may hold 20 000 of them, to save a scan that happens once
    /// per session per run.
    ///
    /// A file holding several sessions is what makes the origin more than a
    /// path, and it is now structural rather than a comparison: two OpenCode
    /// sessions in one database get two origins, so neither can evict the other.
    /// Matched on the path alone, every one of them counted as superseding every
    /// other and the cache held exactly one OpenCode session however many there
    /// were.
    pub fn put(&self, id: DiskId, path: &Path, session: &str, data: &SessionData) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(
                id.origin,
                Entry {
                    path: path.to_path_buf(),
                    session: session.to_string(),
                    stamp: id.stamp,
                    stored_at: now_ms(),
                    data: data.clone(),
                },
            );
        }
        if let Ok(mut d) = self.dirty.lock() {
            *d = true;
        }
    }

    /// Write the cache back.
    ///
    /// This runs on a timer while the UI is up, so it touches the filesystem
    /// once — for the write itself. It used to `stat` and `read_dir` every
    /// cached transcript to re-derive its key, and to clone the whole entry map
    /// (hundreds of MB-scale sessions) before serializing.
    pub fn save(&self) {
        if !self.dirty.lock().map(|d| *d).unwrap_or(false) {
            return;
        }
        let _span = crate::trace::span("cache.save");
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        evict_oldest(&mut entries);

        let _ = std::fs::create_dir_all(&*config::CACHE_DIR);
        if write_atomically(&config::COST_CACHE_FILE, &entries).is_ok()
            && let Ok(mut d) = self.dirty.lock()
        {
            *d = false;
        }
    }
}

/// Serialize `entries` to `path`, leaving the old file untouched unless the new
/// one was written whole.
///
/// Write-then-rename, because a crash or a full disk mid-write would otherwise
/// leave truncated JSON in place of every cached session.
///
/// The buffer is flushed by hand rather than left to `BufWriter`'s drop, which
/// flushes and then discards whatever the flush said. `to_writer` returns `Ok`
/// once the bytes are *buffered*, so without this a write failing on its last
/// block — a full disk, a quota, a disconnected network home — still reported
/// success and renamed a truncated file over a good cache. Nothing could read
/// it afterwards, and a cache that parses as nothing is indistinguishable from
/// an empty one, so every session re-parsed on every run from then on.
fn write_atomically(path: &Path, entries: &HashMap<String, Entry>) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("json.tmp");
    let written = (|| -> std::io::Result<()> {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        serde_json::to_writer(
            &mut out,
            &DiskCacheRef {
                version: CACHE_VERSION,
                entries,
            },
        )?;
        out.flush()
    })();
    if written.is_ok()
        && let Err(error) = std::fs::rename(&tmp, path)
    {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Drop the least recently stored entries once the cache exceeds `MAX_ENTRIES`.
fn evict_oldest(entries: &mut HashMap<String, Entry>) {
    if entries.len() <= MAX_ENTRIES {
        return;
    }
    // Sorted by key as well as age: a startup burst stores many entries in the
    // same millisecond, and "everything older than the cutoff" would then throw
    // away far more than the overflow.
    let mut order: Vec<(u64, String)> = entries
        .iter()
        .map(|(k, e)| (e.stored_at, k.clone()))
        .collect();
    order.sort_unstable();
    for (_, key) in order.into_iter().take(entries.len() - MAX_ENTRIES) {
        entries.remove(&key);
    }
}

// ---------------------------------------------------------------------------
// In-memory layer
// ---------------------------------------------------------------------------

/// Extracted data plus the inputs it was derived from. Both must still match
/// for a hit, so a mid-session pricing refresh recomputes costs rather than
/// serving the figures computed before rates were available.
///
/// The data is shared rather than held by value. Almost all of a `SessionData`
/// is in two fields a row never reads: the tool history, measured at ~31 KB per
/// session and 83% of a 2000-session cache, and the context series, another 15%.
/// A row reads about fifteen scalars off it, so handing each row its own copy
/// meant a walk deep-copied all of that per session, every walk, for fields no
/// row can reach. `Arc` makes a hit a refcount bump, and the one reader that
/// does want those fields takes its own copy deliberately (see
/// [`Store::session_data_fresh`]).
struct MemEntry {
    mtime: u64,
    pricing_epoch: u64,
    data: Arc<SessionData>,
    /// How long the parse behind `data` took, and when it finished. Together
    /// with `size` they bound how much of a core one growing transcript may
    /// consume.
    parsed_in: std::time::Duration,
    parsed_at: std::time::Instant,
    /// Transcript size when it was stored, a proxy for what re-parsing costs.
    size: u64,
}

/// Bytes a re-parse reads: the transcript plus any subagent transcripts beside
/// it, which Claude's extractor reads end to end on every parse too. Counting
/// only the main file let a session whose bulk lives in its subagents — a
/// 640 KB transcript over 4 MB of them — slip under the size floor and be
/// re-read whole on every tick.
fn file_size(path: &Path) -> u64 {
    crate::session::transcript_files(path)
        .iter()
        .map(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0))
        .sum()
}

/// How many times its own parse cost a transcript must wait before being parsed
/// again, so each one costs at most `1/N` of a core no matter how large it is.
const REPARSE_BACKOFF: u32 = 20;

/// Above this, a transcript also gets a size-based floor on its re-parse
/// interval.
const LARGE_TRANSCRIPT_BYTES: u64 = 1 << 20;

/// Seconds of floor per megabyte, and the ceiling on that floor.
const FLOOR_SECS_PER_MB: u64 = 10;
const MAX_FLOOR_SECS: u64 = 60;

/// Minimum time between re-parses of a transcript of this size.
///
/// The proportional backoff alone does not bind in practice: a 3.7 MB
/// transcript parses in ~50 ms, so `parse × 20` is a one-second window — under
/// the default two-second refresh, meaning every tick re-parses the whole file.
/// Parse cost tracks size, so size gives the floor a scale the measurement
/// cannot: nothing under a megabyte gets one at all (small sessions stay live,
/// which is where lag would actually be noticed), then ten seconds per megabyte
/// up to a minute. That 3.7 MB file lands on a 30 s floor — under 0.2% of a
/// core instead of 2.5% — and a 20 MB one cannot exceed 0.2% either.
fn reparse_floor(size: u64) -> std::time::Duration {
    if size < LARGE_TRANSCRIPT_BYTES {
        return std::time::Duration::ZERO;
    }
    let mb = size / LARGE_TRANSCRIPT_BYTES;
    std::time::Duration::from_secs((mb * FLOOR_SECS_PER_MB).min(MAX_FLOOR_SECS))
}

/// Whether stale-but-cached data should be served instead of re-parsing.
///
/// A live session appends every few seconds and the cache key is size+mtime, so
/// every append invalidates the entry and the whole file is parsed again. Cheap
/// transcripts stay effectively real-time; only the expensive ones back off.
fn reuse_stale(parsed_in: std::time::Duration, since: std::time::Duration, size: u64) -> bool {
    since < parsed_in * REPARSE_BACKOFF || since < reparse_floor(size)
}

#[derive(Default)]
pub struct Store {
    mem: Mutex<HashMap<String, MemEntry>>,
    disk: CostCache,
}

impl Store {
    pub fn new() -> Self {
        Store {
            mem: Mutex::new(HashMap::new()),
            disk: CostCache::load(),
        }
    }

    /// Extracted data for a table row, which may be served stale to bound CPU.
    ///
    /// Shared, because this is the path thousands of rows take per walk and
    /// none of them reads the fields that make a `SessionData` large. Callers
    /// that want to keep one past the borrow take their own copy; that is one
    /// session's worth, not a table's.
    pub fn session_data(&self, session: &Session) -> Arc<SessionData> {
        self.data(session, true)
    }

    /// Extracted data for the session the user has open, never served
    /// incomplete and stale only within the same bound the rows already use.
    ///
    /// The row-level refresh backs off on expensive transcripts because it pays
    /// that cost once per session, thousands of times over. The open panels are
    /// one session, but the transcript does not know that: a full parse per
    /// append on a huge file stalls the worker the whole TUI waits on, and a
    /// lagging panel beats a lagging monitor. So the same backoff applies —
    /// just never to an entry that came off disk, because what this path exists
    /// for is exactly the fields that copy dropped.
    ///
    /// It is also the only path that sees the fields the cache drops — the tool
    /// history and the context series — so a cached copy of those, which is
    /// always empty, has to be refused rather than displayed as an empty panel.
    ///
    /// Shared like the row path, so opening a panel on a session the walk has
    /// already extracted costs no copy at all. The one that does pay is the
    /// panel: `App::panel_data` owns a plain `SessionData`, so the run loop
    /// takes its own copy as it takes the answer off the channel. That is the
    /// right place for it — the panel is the reader that wants the fields that
    /// make an extraction large, it is one session, and it happens on
    /// selection.
    pub fn session_data_fresh(&self, session: &Session) -> Arc<SessionData> {
        self.data(session, false)
    }

    fn data(&self, session: &Session, allow_stale: bool) -> Arc<SessionData> {
        let Some(file) = session.data_file.as_ref() else {
            // A running process with no transcript yet.
            return Arc::new(SessionData::default());
        };
        let mem_key = session.key();
        let mtime = crate::session::effective_mtime_ms(session);
        let epoch = crate::pricing::pricing_epoch();

        if let Ok(mem) = self.mem.lock()
            && let Some(entry) = mem.get(&mem_key)
            && entry.pricing_epoch == epoch
        {
            // A copy that came off disk carries no tool history and no context
            // series, so it answers a row but not a panel.
            if entry.mtime == mtime && (allow_stale || entry.data.complete) {
                crate::trace::add("served from memory", 1);
                return entry.data.clone();
            }
            // ponytail: re-parse backoff, not incremental parsing. A transcript is
            // append-only, so the right fix is to parse only the appended bytes —
            // which means persisting each provider's mid-file extractor state,
            // including Claude's per-request dedup map, or a request whose lines
            // straddle the boundary gets counted twice. Until then, bound the
            // waste: a transcript costing 500ms to parse is re-read every 10s
            // instead of every 2s, while cheap ones stay effectively live.
            //
            // The panel path joins the backoff for a *complete* entry: it came
            // from a real parse, so it carries the fields a disk copy drops,
            // and serving it a few seconds old is what keeps a giant transcript
            // from being re-read end to end on every append.
            if (allow_stale || entry.data.complete)
                && reuse_stale(entry.parsed_in, entry.parsed_at.elapsed(), entry.size)
            {
                crate::trace::add("served stale to bound cpu", 1);
                return entry.data.clone();
            }
        }

        let disk_id = disk_id(session, file);
        // Rows only: what the disk holds is missing exactly the fields the panel
        // is opened to read, so serving it there would draw an empty panel over
        // a session that has plenty to show.
        if allow_stale
            && let Some(id) = &disk_id
            && let Some(data) = self.disk.get(id)
        {
            if let Ok(mut mem) = self.mem.lock() {
                mem.insert(
                    mem_key,
                    MemEntry {
                        mtime,
                        pricing_epoch: epoch,
                        data: data.clone(),
                        // A disk hit says nothing about what parsing this costs,
                        // so claim nothing and let the next change be parsed —
                        // subject to the size floor, which needs no measurement.
                        parsed_in: std::time::Duration::ZERO,
                        parsed_at: std::time::Instant::now(),
                        size: file_size(file),
                    },
                );
            }
            crate::trace::add("served from cache file", 1);
            return data;
        }

        let _span = crate::trace::span("extract.parse");
        // Split by provider too. The aggregate says extraction is slow; it
        // cannot say whether that is one harness's transcripts or all of them,
        // and the two have nothing in common to fix.
        let _by_provider = crate::trace::span(session.provider.extract_trace_name());
        crate::trace::add("transcripts parsed", 1);
        // Only where the file backs this session alone. OpenCode keeps every
        // session in one database and Windsurf every conversation in one blob,
        // both read by indexed lookup rather than end to end — so charging each
        // session the whole file would report a 70 MB database ten times over
        // and drown the figure this is here to give.
        if !matches!(
            session.provider,
            crate::pricing::Provider::OpenCode | crate::pricing::Provider::Windsurf
        ) {
            crate::trace::add("parsed_bytes", file_size(file));
        }
        let started = std::time::Instant::now();
        let mut data = match session.provider {
            crate::pricing::Provider::Claude => crate::session::claude::extract(file),
            crate::pricing::Provider::Codex => crate::session::codex::extract(file),
            crate::pricing::Provider::Cursor => crate::session::cursor::extract(file),
            crate::pricing::Provider::Devin => crate::session::devin::extract(file),
            crate::pricing::Provider::OpenCode => {
                crate::session::opencode::extract(file, &session.session_id)
            }
            crate::pricing::Provider::Gemini => crate::session::gemini::extract(file),
            crate::pricing::Provider::Pi => crate::session::pi::extract(file),
            crate::pricing::Provider::Windsurf => {
                crate::session::windsurf::extract(file, &session.session_id)
            }
        };
        let parsed_in = started.elapsed();
        // Trim before anything sees it, so the cached copy and this one agree.
        data.finalize();
        // Wrapped once here, so the memory layer, the disk layer and the caller
        // all hold the same extraction: `put` copies for the disk shape, and a
        // caller that keeps the data to read past the borrow copies for itself.
        let data = Arc::new(data);
        if let Ok(mut mem) = self.mem.lock() {
            mem.insert(
                mem_key,
                MemEntry {
                    mtime,
                    pricing_epoch: epoch,
                    data: data.clone(),
                    parsed_in,
                    parsed_at: std::time::Instant::now(),
                    size: file_size(file),
                },
            );
        }
        // Never persist a failed parse; it would stick until the file changes.
        if let Some(id) = disk_id
            && data.error.is_none()
        {
            self.disk.put(id, file, &session.session_id, &data);
        }
        data
    }

    /// Drop every cached copy of a deleted session.
    pub fn evict(&self, session: &Session) {
        if let Ok(mut mem) = self.mem.lock() {
            mem.remove(&session.key());
        }
        // Derived the same way it was stored, or a shared-database session
        // would leave its entry behind — and now that those are cached, that
        // entry would outlive the session it describes.
        if let Some(file) = session.data_file.as_ref()
            && let Some(id) = disk_id(session, file)
        {
            if let Ok(mut e) = self.disk.entries.lock() {
                e.remove(&id.origin);
            }
            if let Ok(mut d) = self.disk.dirty.lock() {
                *d = true;
            }
        }
    }

    pub fn save(&self) {
        self.disk.save();
    }
}

// ---------------------------------------------------------------------------
// UI preferences
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    pub bottom_tab: usize,
    pub live_only: bool,
    // The table's sort is deliberately not here. Clicking a column header is one
    // pixel away from clicking the row under it, and persisting that turned a
    // misclick on CPU% into every later launch opening sorted by CPU with
    // nothing on screen explaining why. It resets to newest-first each run;
    // sorting within a run is unchanged.
    pub inactivity_filter: Option<String>,
    pub agent_live_filter: bool,
    pub tool_show_diff: bool,
    /// Session keys whose subagents are shown as child rows.
    ///
    /// Unlike the table's sort, this is worth persisting: expanding a session is
    /// a deliberate act about one session, not a click that lands a pixel away
    /// from something else.
    pub expanded: Vec<String>,
    /// What the Subagents panel sorts by, and its direction — with the table's
    /// own reading of each: ascending for `last` is newest-first, so `last ↑`
    /// means what the table's `last ▲` means (`columns::compare`).
    pub subagent_sort_col: String,
    pub subagent_sort_asc: bool,
    /// Only show sessions whose cost reaches this floor (0 disables it).
    pub cost_floor: f64,
    /// Ring the bell and raise a desktop notification when a session needs you.
    /// Opt-in and remembered, because whether a terminal may make noise is a
    /// property of the room you sit in, not of this run.
    pub notify: bool,
    /// Update to a newer release as the UI starts, when the hourly check has
    /// already found one.
    ///
    /// On by default, unlike [`UiPrefs::notify`]: a bell is about the room you
    /// are sitting in and has to be asked for, while an update is about the
    /// binary and is what someone running a downloaded tool wants by the time
    /// they notice a version exists. Off means off for good — `--no-auto-update`
    /// is the way to skip one run.
    pub auto_update: bool,
    /// A version the user has already said "not now" to.
    ///
    /// Only the cargo-install path asks, and only about the version named here —
    /// so a decline is remembered until the next release rather than forever,
    /// and the question does not come back on every launch in between.
    pub declined_update: Option<String>,
    /// The shell alias block has been written once. Kept here so removing the
    /// block — by flag or by hand — isn't undone by the next launch.
    pub shell_alias_installed: bool,
    /// Table columns the user has hidden, by column id. Unlike the sort order
    /// this is deliberate and effortful to redo, so it survives the run.
    pub hidden_columns: Vec<String>,
    /// Chosen theme name, or `None` to follow the built-in default.
    pub theme: Option<String>,
    /// Claude profile a new tab launches under, by name.
    ///
    /// Remembered because a profile is an account, and somebody working out of
    /// their work login is working out of it all afternoon — having every new
    /// tab revert to the personal one would be a mistake made silently, in the
    /// place where it costs the most to notice late.
    pub claude_profile: Option<String>,
    /// Codex profile a new tab launches under, by name. Separate from
    /// [`Self::claude_profile`] because they are separate accounts on separate
    /// subscriptions, and a prefs file written before Codex had profiles still
    /// reads correctly.
    pub codex_profile: Option<String>,
    /// Recent `/` queries, newest first, so a search worth running twice does
    /// not have to be typed twice. Capped at [`MAX_SEARCH_HISTORY`].
    pub search_history: Vec<String>,
    /// The table is drawn as a tree of repositories and their checkouts.
    ///
    /// Persisted, unlike the sort: it is a key pressed on purpose to change how
    /// the whole table reads, and someone who works that way works that way
    /// every launch.
    pub tree: bool,
    /// Keys of the tree's folded groups. Kept even while the tree is off, so
    /// turning it back on restores the shape it was left in.
    pub collapsed_groups: Vec<String>,
}

/// How many past queries are remembered.
///
/// Long enough to hold a session's worth of searching, short enough that ↑ is
/// still a faster way back to a query than retyping it.
pub const MAX_SEARCH_HISTORY: usize = 20;

impl Default for UiPrefs {
    fn default() -> Self {
        UiPrefs {
            bottom_tab: 0,
            live_only: false,
            inactivity_filter: None,
            agent_live_filter: false,
            tool_show_diff: false,
            expanded: Vec::new(),
            subagent_sort_col: "last".into(),
            // Ascending for `last` is newest-first, so this default keeps the
            // panel showing the most recently active agent first — what
            // `last ↓` showed under the panel's old, opposite semantics.
            subagent_sort_asc: true,
            cost_floor: 0.0,
            notify: false,
            auto_update: true,
            declined_update: None,
            shell_alias_installed: false,
            hidden_columns: Vec::new(),
            theme: None,
            claude_profile: None,
            codex_profile: None,
            search_history: Vec::new(),
            tree: false,
            collapsed_groups: Vec::new(),
        }
    }
}

impl UiPrefs {
    pub fn load() -> Self {
        std::fs::read_to_string(&*config::UI_PREFS_FILE)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        // Every key a test presses that changes a preference ends here, and
        // `CACHE_DIR` is the developer's real one: the suite was overwriting
        // their `ui-prefs.json` with defaults on every run.
        if crate::under_test() {
            return;
        }
        let _ = std::fs::create_dir_all(&*config::CACHE_DIR);
        if let Ok(text) = serde_json::to_string(self) {
            let _ = std::fs::write(&*config::UI_PREFS_FILE, text);
        }
    }
}

/// The build script, pulled into the test binary so the cache version can be
/// re-derived here instead of being asserted against a copy of its logic.
#[cfg(test)]
#[allow(dead_code)]
mod build_script {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/build.rs"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    #[test]
    fn reparse_backoff_scales_with_parse_cost() {
        use std::time::Duration;
        const SMALL: u64 = 64 * 1024;
        // A cheap transcript is re-read almost immediately…
        assert!(!reuse_stale(
            Duration::from_millis(5),
            Duration::from_millis(200),
            SMALL
        ));
        // …an expensive one waits proportionally longer than the refresh interval.
        assert!(reuse_stale(
            Duration::from_millis(500),
            Duration::from_secs(2),
            SMALL
        ));
        assert!(!reuse_stale(
            Duration::from_millis(500),
            Duration::from_secs(11),
            SMALL
        ));
        // Something never parsed here (a disk-cache hit) claims no budget.
        assert!(!reuse_stale(Duration::ZERO, Duration::ZERO, SMALL));
    }

    /// Regression: the proportional backoff alone left multi-MB live sessions
    /// re-parsed on every tick, because they parse far faster than a 2s refresh.
    #[test]
    fn large_transcripts_get_a_floor_the_refresh_interval_cannot_beat() {
        use std::time::Duration;
        let big = 4 * (1 << 20);
        // A 50ms parse would otherwise permit a re-parse after one second.
        assert!(reuse_stale(
            Duration::from_millis(50),
            Duration::from_secs(2),
            big
        ));
        assert!(!reuse_stale(
            Duration::from_millis(50),
            Duration::from_secs(41),
            big
        ));
        // Small sessions keep feeling live: no floor at all.
        assert_eq!(reparse_floor(900 * 1024), Duration::ZERO);
        // …and the floor is capped, so a huge file is not frozen indefinitely.
        assert_eq!(
            reparse_floor(500 * (1 << 20)),
            Duration::from_secs(MAX_FLOOR_SECS)
        );
    }

    #[test]
    fn cache_version_is_derived_and_stable() {
        assert!(!CACHE_VERSION.is_empty(), "build.rs must set the hash");
        assert_eq!(CACHE_VERSION.len(), 16);
        assert!(CACHE_VERSION.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(CACHE_VERSION, env!("CCTOP_CACHE_HASH"));
    }

    /// The real source set, as the build script sees it. `cargo test` runs with
    /// the package root as the working directory, which is what makes the build
    /// script's relative roots resolve here too.
    fn hashed_sources() -> Vec<(String, Vec<u8>)> {
        let (files, _dirs) = build_script::sources();
        assert!(
            !files.is_empty(),
            "expected to run from the package root, found no sources"
        );
        build_script::read_all(&files)
    }

    /// Every provider's extractor decides what ends up in a cache entry, so
    /// every provider's file has to be in the hashed set. A new one that nobody
    /// remembered to list is exactly the failure this whole mechanism exists to
    /// prevent.
    #[test]
    fn the_hashed_set_covers_every_parser_and_the_shipped_hash_matches_it() {
        let sources = hashed_sources();
        let names: Vec<&str> = sources.iter().map(|(p, _)| p.as_str()).collect();
        for provider in Provider::ALL {
            let expected = format!("src/session/{}.rs", provider.as_str());
            assert!(names.contains(&expected.as_str()), "{expected} not hashed");
        }
        assert!(names.contains(&"src/session/mod.rs"));
        assert!(names.contains(&"src/config.rs"));
        assert!(names.contains(&"src/pricing.rs"));
        // …and what the binary carries is the digest of exactly that set, so the
        // walk cannot silently drift from what was compiled in.
        assert_eq!(
            format!("{:016x}", build_script::digest(&sources)),
            CACHE_VERSION
        );
    }

    /// The bug this replaced: `context_breakdown` was added to `SessionData`
    /// without bumping the hand-written version, so cached entries kept
    /// deserializing it as `None` and the panel stayed blank forever. Changing
    /// the shape must change the version, with nobody having to remember.
    #[test]
    fn changing_session_data_changes_the_derived_hash() {
        let base = hashed_sources();
        let before = build_script::digest(&base);
        assert_eq!(before, build_script::digest(&base), "digest is a function");

        let mut edited = base.clone();
        let model = edited
            .iter_mut()
            .find(|(p, _)| p == "src/session/mod.rs")
            .expect("the data model is hashed");
        model
            .1
            .extend_from_slice(b"\n// a new cached field lands here\n");
        assert_ne!(
            before,
            build_script::digest(&edited),
            "an edit to the data model must invalidate the cache"
        );

        // Relocating a parser is a change too, even byte for byte.
        let mut moved = base.clone();
        moved[0].0 = format!("src/session/renamed_{}", moved[0].0);
        assert_ne!(before, build_script::digest(&moved));
    }

    /// Regression: `save` used to recover the transcript path with
    /// `key.split('|').next()`, which truncates a path that contains `|`. The
    /// derived key then never matched, the entry was dropped on every save, and
    /// that session was re-parsed forever.
    ///
    /// Round-tripped through a real file now, because that is where the split
    /// happened: an entry that only has to survive in memory never had to be
    /// recoverable from its key.
    #[test]
    fn entries_survive_a_pipe_in_the_transcript_path() {
        let dir = std::env::temp_dir().join(format!("cctop-pipe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a|b.jsonl");
        std::fs::write(&file, "x").unwrap();
        let out = dir.join("cost-cache.json");

        let cache = CostCache {
            entries: Mutex::new(HashMap::new()),
            dirty: Mutex::new(false),
        };
        let id = cache_key(&file).unwrap();
        assert!(id.origin.contains("a|b"), "the key embeds the awkward path");
        cache.put(
            DiskId {
                origin: id.origin.clone(),
                stamp: id.stamp.clone(),
            },
            &file,
            "sess",
            &SessionData::default(),
        );
        write_atomically(&out, &cache.entries.lock().unwrap()).unwrap();

        let back: DiskCache =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        let entries = back.entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[&id.origin].path, file);
        assert_eq!(entries[&id.origin].stamp, id.stamp);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A new version of a transcript replaces the old one, which is what keeps
    /// the cache from growing without re-`stat`ing everything on save. It does
    /// so by landing in the same slot, so there is nothing to search for first.
    #[test]
    fn a_changed_transcript_supersedes_its_own_entry() {
        let path = Path::new("/tmp/whatever.jsonl");
        let cache = CostCache {
            entries: Mutex::new(HashMap::new()),
            dirty: Mutex::new(false),
        };
        let put = |stamp: &str| {
            cache.put(
                DiskId {
                    origin: "o".into(),
                    stamp: stamp.into(),
                },
                path,
                "sess",
                &SessionData::default(),
            )
        };
        put("k1");
        put("k2");
        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries["o"].stamp, "k2");
    }

    /// The other half of superseding: what is stored is served only while the
    /// transcript still says the same thing, and an entry that has moved on is
    /// not served as though it had not.
    #[test]
    fn an_entry_is_served_only_while_its_stamp_still_matches() {
        let dir = std::env::temp_dir().join(format!("cctop-stamp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.jsonl");
        std::fs::write(&file, "one").unwrap();

        let cache = CostCache {
            entries: Mutex::new(HashMap::new()),
            dirty: Mutex::new(false),
        };
        let before = cache_key(&file).unwrap();
        cache.put(
            DiskId {
                origin: before.origin.clone(),
                stamp: before.stamp.clone(),
            },
            &file,
            "sess",
            &SessionData::default(),
        );
        assert!(cache.get(&before).is_some(), "an unchanged transcript hits");

        std::fs::write(&file, "one, and rather more of it").unwrap();
        let after = cache_key(&file).unwrap();
        assert_eq!(after.origin, before.origin, "the slot does not move");
        assert!(
            cache.get(&after).is_none(),
            "an appended transcript must not be served the old extraction"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn eviction_keeps_the_newest_entries() {
        let mut entries = HashMap::new();
        for i in 0..(MAX_ENTRIES + 10) {
            entries.insert(
                format!("k{i}"),
                Entry {
                    path: PathBuf::from(format!("/tmp/{i}.jsonl")),
                    session: format!("s{i}"),
                    stamp: String::new(),
                    stored_at: i as u64,
                    data: SessionData::default(),
                },
            );
        }
        evict_oldest(&mut entries);
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert!(!entries.contains_key("k9"));
        assert!(entries.contains_key("k10"));
    }

    #[test]
    fn cache_key_changes_with_content() {
        let dir = std::env::temp_dir().join(format!("cctop-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.jsonl");
        std::fs::write(&f, "one").unwrap();
        let k1 = cache_key(&f).unwrap();
        std::fs::write(&f, "one plus more").unwrap();
        let k2 = cache_key(&f).unwrap();
        assert_eq!(k1.origin, k2.origin, "the slot is the file");
        assert_ne!(k1.stamp, k2.stamp, "size change must invalidate the key");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A workflow agent streams into its own file while the parent sits waiting
    /// on the run, so its writes are what has to move the parent's key.
    #[test]
    fn cache_key_moves_when_a_workflow_agent_writes() {
        let dir = std::env::temp_dir().join(format!("cctop-wf-{}", std::process::id()));
        let f = dir.join("s.jsonl");
        let run = dir
            .join("s")
            .join("subagents")
            .join("workflows")
            .join("wf_1");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(&f, "x").unwrap();
        let agent = run.join("agent-a1.jsonl");
        std::fs::write(&agent, "one").unwrap();
        let at = |secs| std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        let touch = |secs| {
            std::fs::File::options()
                .write(true)
                .open(&agent)
                .unwrap()
                .set_modified(at(secs))
                .unwrap()
        };
        // Both far in the future, past every directory's own mtime.
        touch(4_000_000_000);
        let k1 = cache_key(&f).unwrap();
        touch(4_000_000_100);
        let k2 = cache_key(&f).unwrap();
        assert_ne!(
            k1.stamp, k2.stamp,
            "a workflow agent's write left the key as it was"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Regression: cached entries hold computed costs, so the key must change
    /// when the pricing table does. Otherwise sessions priced before the rate
    /// table loaded keep reporting $0.00 forever — their transcripts are
    /// finished and will never change again to force a re-parse.
    #[test]
    fn cache_key_carries_pricing_generation() {
        let dir = std::env::temp_dir().join(format!("cctop-price-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.jsonl");
        std::fs::write(&f, "x").unwrap();
        let id = cache_key(&f).unwrap();
        assert!(
            id.stamp
                .contains(&format!("|p{}", crate::pricing::pricing_epoch())),
            "stamp {} must embed the pricing epoch",
            id.stamp
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A session's tool history is the bulk of what an extraction produces —
    /// measured at 83% of a cache holding 2000 sessions, with the context series
    /// another 15%. Neither is on a row, and persisting them made the cache file
    /// slower to read than the transcripts it stood in for. What the row does
    /// need is distilled first, so it has to survive the round trip.
    #[test]
    fn the_cache_drops_the_tool_history_but_keeps_what_a_row_needs() {
        let mut data = SessionData::default();
        data.metrics.tool_details.insert(
            "Edit".into(),
            vec![crate::session::ToolDetail {
                d: "src/main.rs".into(),
                ts: "2026-01-01T00:00".into(),
                ..Default::default()
            }],
        );
        data.context_series.push(crate::session::CtxPoint {
            ts: "2026-01-01T00:00".into(),
            window: 1234,
            after_compaction: false,
        });
        data.costs.total = 4.25;
        data.finalize();
        assert_eq!(data.recent_writes, ["src/main.rs"]);
        assert!(data.complete, "a fresh extraction is complete");

        let back: SessionData =
            serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();

        assert!(
            back.metrics.tool_details.is_empty(),
            "history was persisted"
        );
        assert!(back.context_series.is_empty(), "series was persisted");
        assert_eq!(back.recent_writes, ["src/main.rs"]);
        assert_eq!(back.costs.total, 4.25);
        // …and it must own up to being partial, or the panel would draw the
        // empty history above as though the session had never used a tool.
        assert!(
            !back.complete,
            "a restored entry must not claim completeness"
        );
    }

    /// A stale entry that came from a real parse may answer the panel too: it
    /// carries the fields a disk copy drops, and a transcript expensive enough
    /// to back the rows off is just as expensive for the one reader watching it.
    /// The file has to be over a megabyte for the size floor to apply, or a
    /// cheap parse's own measured backoff is too small to observe.
    #[test]
    fn a_stale_parse_serves_the_panel_within_the_backoff() {
        let dir = std::env::temp_dir().join(format!("cctop-stale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("a.jsonl");
        let assistant = |request: &str, out: u64| {
            format!(
                r#"{{"type":"assistant","timestamp":"2026-08-05T10:00:00.000Z","requestId":"{request}","message":{{"id":"m_{request}","role":"assistant","model":"claude-opus-5","content":[{{"type":"text","text":"a"}}],"usage":{{"input_tokens":100,"output_tokens":{out}}}}}}}"#
            )
        };
        // A whitespace-only line bulks the file out without adding a record.
        let body = format!("{}\n{}", assistant("r1", 5), " ".repeat(1 << 20));
        std::fs::write(&transcript, &body).unwrap();

        let store = Store::new();
        let mut s = Session::new(crate::pricing::Provider::Claude, "s1".into());
        s.data_file = Some(transcript.clone());
        let first = store.session_data_fresh(&s);
        assert_eq!(first.tokens.output, 5);
        assert!(first.complete);

        // The transcript grew, but the entry is inside its re-parse bound, so
        // the panel gets the parse it already paid for rather than a second
        // one — before this, the fresh path re-read the whole file every time.
        let written = std::fs::metadata(&transcript).unwrap().modified().unwrap();
        std::fs::write(&transcript, format!("{body}\n{}", assistant("r2", 50))).unwrap();
        // A growth a coarse clock could stamp with the same mtime would be
        // the cache's other path, so the change is made unmistakable rather
        // than waited for.
        std::fs::File::options()
            .write(true)
            .open(&transcript)
            .unwrap()
            .set_modified(written + std::time::Duration::from_secs(1))
            .unwrap();
        let second = store.session_data_fresh(&s);
        assert_eq!(
            second.tokens.output, 5,
            "inside the backoff the panel is served the previous parse"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The panel is the one view that reads the fields the cache drops, so a
    /// cache hit is not enough for it even when the transcript has not changed.
    #[test]
    fn a_restored_entry_satisfies_a_row_but_not_a_panel() {
        let fresh = {
            let mut d = SessionData::default();
            d.finalize();
            d
        };
        let restored: SessionData =
            serde_json::from_str(&serde_json::to_string(&fresh).unwrap()).unwrap();

        // This is the condition `Store::data` applies to a memory hit.
        let serves = |data: &SessionData, allow_stale: bool| allow_stale || data.complete;
        assert!(serves(&restored, true), "a row may be served from cache");
        assert!(!serves(&restored, false), "a panel may not");
        assert!(serves(&fresh, false), "a real parse serves anything");
    }

    /// The bug this replaced: `to_writer` reports success once the bytes reach
    /// the buffer, and `BufWriter`'s drop flushed the rest while discarding any
    /// error — so a write that failed on its last block still renamed truncated
    /// JSON over a good cache. What came back parsed as nothing, which reads
    /// exactly like an empty cache, so every session re-parsed on every run and
    /// nothing ever said why.
    #[test]
    fn a_saved_cache_reads_back_whole() {
        let dir = std::env::temp_dir().join(format!("cctop-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("a.jsonl");
        std::fs::write(&transcript, "x").unwrap();
        let file = dir.join("cost-cache.json");

        // Big enough to span many buffers, which is where a lost flush shows.
        let mut entries = HashMap::new();
        for i in 0..500 {
            let mut data = SessionData::default();
            data.costs.total = i as f64;
            data.title = Some("x".repeat(500));
            entries.insert(
                format!("k{i}"),
                Entry {
                    path: transcript.clone(),
                    session: format!("s{i}"),
                    stamp: format!("|{i}|0|p0"),
                    stored_at: i,
                    data,
                },
            );
        }
        write_atomically(&file, &entries).expect("the write must report its own failure");

        let back: DiskCache = serde_json::from_str(&std::fs::read_to_string(&file).unwrap())
            .expect("a saved cache must parse");
        assert_eq!(back.version, CACHE_VERSION);
        assert_eq!(back.entries.len(), 500);
        assert_eq!(back.entries["k499"].data.costs.total, 499.0);
        // The scratch file must not be left behind next to the real one.
        assert!(!file.with_extension("json.tmp").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A cost comes back as the same number of bits it went in as.
    ///
    /// The test above stores `i as f64`, and whole numbers survive anything —
    /// which is why a cache that was quietly corrupting a third of its floats
    /// passed it. A cost is the product of a token count and a rate, so it is
    /// almost never a whole number, and serde_json's default `f64` parser is
    /// not correctly rounded: measured over the shortest-repr output it
    /// produces, 29.6% of finite floats came back one ULP away. That is
    /// invisible in a total rounded to cents and not invisible in a total that
    /// is not — and, worse, the cache then stores the wrong figure as though it
    /// had computed it, so nothing downstream can tell the two apart.
    ///
    /// Bit equality is the assertion because `assert_eq!` on two `f64`s that
    /// differ by one ULP passes in every direction that matters here: the
    /// difference is far below the tolerance a comparison applies.
    #[test]
    fn a_saved_cost_comes_back_as_the_number_it_was() {
        // Values a real run produces: token counts times a rate, at the scales
        // the pricing table carries.
        let costs = [
            0.001_625_000_000_000_000_1_f64,
            0.042_535_301_669_349_68_f64,
            1.0000000000000002,
            0.1 + 0.2,
            1e-7,
            1.7976931348623157e308,
            2.2250738585072014e-308,
            1234.5678901234567,
        ];
        let dir = std::env::temp_dir().join(format!("cctop-float-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("a.jsonl");
        std::fs::write(&transcript, "x").unwrap();

        let mut entries = HashMap::new();
        for (i, cost) in costs.iter().enumerate() {
            let mut data = SessionData::default();
            data.costs.total = *cost;
            entries.insert(
                format!("k{i}"),
                Entry {
                    path: transcript.clone(),
                    session: format!("s{i}"),
                    stamp: format!("s{i}"),
                    stored_at: i as u64,
                    data,
                },
            );
        }
        let file = dir.join("cost-cache.json");
        write_atomically(&file, &entries).expect("the write must succeed");

        let back: DiskCache = serde_json::from_str(&std::fs::read_to_string(&file).unwrap())
            .expect("a saved cache must parse");
        for (i, cost) in costs.iter().enumerate() {
            let got = back.entries[&format!("k{i}")].data.costs.total;
            assert_eq!(
                got.to_bits(),
                cost.to_bits(),
                "cost {i} ({cost:e}) came back as {got:e}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A destination that cannot be written has to be reported, not swallowed:
    /// silently keeping the old cache forever is how a machine ends up
    /// re-parsing everything on every run with nothing to show for it.
    #[test]
    fn an_unwritable_destination_is_an_error() {
        let path = Path::new("/nonexistent-directory-cctop/cost-cache.json");
        assert!(write_atomically(path, &HashMap::new()).is_err());
    }

    /// Every OpenCode session names the same database file, so the key has to
    /// come from the session. Sharing one would have served all 1478 of them
    /// whatever the first one extracted.
    #[test]
    fn shared_database_sessions_get_a_key_each() {
        let db = Path::new("/tmp/opencode.db");
        let key_of = |id: &str, updated: &str| {
            let mut s = Session::new(crate::pricing::Provider::OpenCode, id.into());
            s.last_active = updated.into();
            disk_id(&s, db).expect("a shared-database session must still be keyable")
        };

        // The slot has to name the session: one database, one slot each.
        let a = key_of("a", "2026-01-01");
        assert_ne!(
            a.origin,
            key_of("b", "2026-01-01").origin,
            "two sessions in one file shared a slot"
        );
        // …and a session that gained a message must not be served its old copy.
        assert_ne!(a.stamp, key_of("a", "2026-01-02").stamp);
        // …while one that did not change keeps its entry, which is the point.
        assert_eq!(a, key_of("a", "2026-01-01"));
    }

    /// Windsurf shares a blob the way OpenCode shares a database, but nothing
    /// in it dates a single conversation — so there is no honest key, and
    /// inventing one would serve a stale panel rather than a slow correct one.
    #[test]
    fn windsurf_stays_uncached_for_want_of_a_stamp() {
        let s = Session::new(crate::pricing::Provider::Windsurf, "x".into());
        assert!(disk_id(&s, Path::new("/tmp/state.vscdb")).is_none());
    }

    #[test]
    fn cache_key_absent_for_missing_file() {
        assert!(cache_key(Path::new("/nonexistent/nope.jsonl")).is_none());
    }

    #[test]
    fn prefs_roundtrip_defaults() {
        let p = UiPrefs::default();
        let text = serde_json::to_string(&p).unwrap();
        let back: UiPrefs = serde_json::from_str(&text).unwrap();
        assert_eq!(back.subagent_sort_col, "last");
        assert!(!back.live_only);
    }

    /// Unknown fields must not fail the parse. Every prefs file written before
    /// the table's sort stopped being persisted still carries `sort_col`, and a
    /// hard error there would throw away the rest of the file with it.
    #[test]
    fn prefs_tolerate_missing_and_unknown_fields() {
        let back: UiPrefs =
            serde_json::from_str(r#"{"bottom_tab":3,"sort_col":"cpu","future_field":1}"#).unwrap();
        assert_eq!(back.bottom_tab, 3);
        assert_eq!(back.subagent_sort_col, "last"); // filled from Default
    }

    /// What a process restart actually does: write the map out, read it back,
    /// and serve from what came off the disk.
    ///
    /// Every other test here exercises the map in memory, which is the half that
    /// changed least — the map is keyed by origin now, and the stamp that used
    /// to be part of that key is a field of the entry, so an entry that only
    /// has to survive a `put` and a `get` proves nothing about a file. This one
    /// goes through `write_atomically` and a real deserialisation, which is the
    /// path a wrong field name or a missing `#[serde(default)]` would break.
    #[test]
    fn a_saved_entry_comes_back_serving_what_went_in() {
        let dir = std::env::temp_dir().join(format!("cctop-rt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.jsonl");
        std::fs::write(&file, "one").unwrap();

        let mut s = Session::new(crate::pricing::Provider::Claude, "sess1".into());
        s.data_file = Some(file.clone());
        let id = disk_id(&s, &file).expect("a claude transcript is always keyable");

        // Figures a caller would notice being wrong, rather than a default.
        let mut data = SessionData::default();
        data.costs.total = 1.25;
        data.tokens.output = 7;
        data.title = Some("a session worth finding".into());

        let cache = CostCache {
            entries: Mutex::new(HashMap::new()),
            dirty: Mutex::new(false),
        };
        cache.put(
            DiskId {
                origin: id.origin.clone(),
                stamp: id.stamp.clone(),
            },
            &file,
            "sess1",
            &data,
        );

        let out = dir.join("cost-cache.json");
        write_atomically(&out, &cache.entries.lock().unwrap()).expect("the save must succeed");

        // Read the file back as a cold process would, then ask it for the entry.
        let reopened: DiskCache =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).expect("must parse");
        assert_eq!(reopened.entries.len(), 1);
        let back = CostCache {
            entries: Mutex::new(reopened.entries),
            dirty: Mutex::new(false),
        };
        let hit = back.get(&id).expect("a matching stamp must be served");
        assert_eq!(hit.costs.total, 1.25);
        assert_eq!(hit.tokens.output, 7);
        assert_eq!(hit.title.as_deref(), Some("a session worth finding"));

        // And the slot is the origin, so the entry is found under it — not under
        // a key that has to be re-derived from the file.
        let stored = back.entries.lock().unwrap();
        assert!(stored.contains_key(&id.origin), "keyed by origin");
        assert_eq!(
            stored[&id.origin].path, file,
            "the path survived the round trip"
        );
        assert_eq!(
            stored[&id.origin].stamp, id.stamp,
            "the stamp is what gates a hit"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Two sessions in one transcript get one slot each, and neither evicts the
    /// other.
    ///
    /// The origin is the path alone for a file-per-session provider, so this is
    /// the case where the slot has to be widened to include the session id — and
    /// it is the case that used to hold exactly one OpenCode session however
    /// many there were.
    #[test]
    fn a_shared_file_gives_each_session_its_own_slot() {
        let mut first = Session::new(crate::pricing::Provider::OpenCode, "one".into());
        first.data_file = Some(PathBuf::from("/tmp/shared.db"));
        first.last_active = "2026-01-01T00:00:00Z".into();
        let mut second = Session::new(crate::pricing::Provider::OpenCode, "two".into());
        second.data_file = Some(PathBuf::from("/tmp/shared.db"));
        second.last_active = "2026-01-01T00:00:00Z".into();

        // Derived per call, as the loader does: a `DiskId` is owned and moves
        // into the store, so there is nothing to hand out and also keep.
        let id = |s: &Session| disk_id(s, Path::new("/tmp/shared.db")).unwrap();
        let (a, b) = (id(&first), id(&second));
        assert_ne!(a.origin, b.origin, "one file, two slots");

        let cache = CostCache {
            entries: Mutex::new(HashMap::new()),
            dirty: Mutex::new(false),
        };
        let mut data = SessionData::default();
        data.costs.total = 3.0;
        cache.put(id(&first), Path::new("/tmp/shared.db"), "one", &data);
        cache.put(id(&second), Path::new("/tmp/shared.db"), "two", &data);
        assert_eq!(
            cache.entries.lock().unwrap().len(),
            2,
            "neither evicted the other"
        );
        assert!(cache.get(&a).is_some());
        assert!(cache.get(&b).is_some());
    }

    /// Prefs written before these fields existed must still load. The
    /// container-level `#[serde(default)]` is what guarantees it, so pin the
    /// behaviour rather than the attribute.
    #[test]
    fn prefs_gain_hidden_columns_and_theme_without_breaking_old_files() {
        let old: UiPrefs = serde_json::from_str(r#"{"bottom_tab":1}"#).unwrap();
        assert!(old.hidden_columns.is_empty());
        assert_eq!(old.theme, None);

        let prefs = UiPrefs {
            hidden_columns: vec!["cpu".into(), "mem".into()],
            theme: Some("mono".into()),
            ..Default::default()
        };
        let back: UiPrefs = serde_json::from_str(&serde_json::to_string(&prefs).unwrap()).unwrap();
        assert_eq!(back.hidden_columns, ["cpu", "mem"]);
        assert_eq!(back.theme.as_deref(), Some("mono"));
    }
}
