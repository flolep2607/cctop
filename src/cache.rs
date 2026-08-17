//! On-disk caches: extracted session data, and persisted UI preferences.

use crate::config;
use crate::session::{Session, SessionData};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
/// The key embeds it — `{path}|{len}|{mtime}|p{epoch}` — but a path may contain
/// `|` itself, and splitting on the first one truncated it, so those sessions
/// were dropped on every save and re-parsed forever.
#[derive(Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    /// Unix millis of the write that stored this, used to evict the oldest
    /// entries when the cache outgrows its bound.
    #[serde(default)]
    stored_at: u64,
    data: SessionData,
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

/// Identity of a transcript's *content*: any append changes size or mtime, so a
/// stale entry can never be mistaken for a fresh one.
///
/// For Claude sessions the key also folds in the newest subagent mtime. The
/// parent's own mtime doesn't move while a subagent streams into its own file,
/// and neither does the `subagents/` directory mtime, which only changes when
/// files are added or removed.
/// The key also carries the pricing generation, because cached entries hold
/// computed costs rather than raw tokens: a refreshed rate table has to
/// invalidate them just as an appended transcript does.
pub fn cache_key(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let mut key = format!(
        "{}|{}|{}|p{}",
        path.display(),
        meta.len(),
        config::file_mtime_ms(path),
        crate::pricing::pricing_epoch()
    );
    let sub_dir = path.with_extension("").join("subagents");
    if let Ok(rd) = std::fs::read_dir(&sub_dir) {
        let newest = rd
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl"))
            .map(|e| config::file_mtime_ms(&e.path()))
            .chain(std::iter::once(config::file_mtime_ms(&sub_dir)))
            .max()
            .unwrap_or(0);
        key.push_str(&format!("|{newest}"));
    }
    Some(key)
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
        let mut entries = std::fs::read_to_string(&*config::COST_CACHE_FILE)
            .ok()
            .and_then(|t| serde_json::from_str::<DiskCache>(&t).ok())
            .filter(|c| c.version == CACHE_VERSION)
            .map(|c| c.entries)
            .unwrap_or_default();
        // Deleted transcripts are the only entries worth a `stat`, and once per
        // process start is enough: an entry whose file merely *changed* is
        // superseded by key on the next `put`, at no IO cost at all.
        entries.retain(|_, e| e.path.exists());
        CostCache {
            entries: Mutex::new(entries),
            dirty: Mutex::new(false),
        }
    }

    pub fn get(&self, key: &str) -> Option<SessionData> {
        self.entries.lock().ok()?.get(key).map(|e| e.data.clone())
    }

    pub fn put(&self, key: String, path: &Path, data: &SessionData) {
        if let Ok(mut entries) = self.entries.lock() {
            // The old key for this transcript can never be hit again — the file
            // has moved past it — so drop it here rather than re-deriving every
            // key from the filesystem at save time.
            entries.retain(|k, e| k == &key || e.path != path);
            entries.insert(
                key,
                Entry {
                    path: path.to_path_buf(),
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
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        evict_oldest(&mut entries);

        let _ = std::fs::create_dir_all(&*config::CACHE_DIR);
        // Write-then-rename: a crash or a full disk mid-write would otherwise
        // leave truncated JSON, throwing away every cached session.
        let tmp = config::COST_CACHE_FILE.with_extension("json.tmp");
        let wrote = std::fs::File::create(&tmp).is_ok_and(|file| {
            serde_json::to_writer(
                std::io::BufWriter::new(&file),
                &DiskCacheRef {
                    version: CACHE_VERSION,
                    entries: &entries,
                },
            )
            .is_ok()
        });
        if wrote && std::fs::rename(&tmp, &*config::COST_CACHE_FILE).is_ok() {
            if let Ok(mut d) = self.dirty.lock() {
                *d = false;
            }
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }
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
struct MemEntry {
    mtime: u64,
    pricing_epoch: u64,
    data: SessionData,
    /// How long the parse behind `data` took, and when it finished. Together
    /// with `size` they bound how much of a core one growing transcript may
    /// consume.
    parsed_in: std::time::Duration,
    parsed_at: std::time::Instant,
    /// Transcript size when it was stored, a proxy for what re-parsing costs.
    size: u64,
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
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
    pub fn session_data(&self, session: &Session) -> SessionData {
        self.data(session, true)
    }

    /// Extracted data for the session the user has open, never served stale and
    /// never served incomplete.
    ///
    /// The row-level refresh backs off on expensive transcripts because it pays
    /// that cost once per session, thousands of times over. The open panels are
    /// one session — the one place where a few seconds of lag is actually visible,
    /// and the one place where a full parse per change is affordable.
    ///
    /// It is also the only path that sees the fields the cache drops — the tool
    /// history and the context series — so a cached copy of those, which is
    /// always empty, has to be refused rather than displayed as an empty panel.
    pub fn session_data_fresh(&self, session: &Session) -> SessionData {
        self.data(session, false)
    }

    fn data(&self, session: &Session, allow_stale: bool) -> SessionData {
        let Some(file) = session.data_file.as_ref() else {
            // A running process with no transcript yet.
            return SessionData::default();
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
                return entry.data.clone();
            }
            // ponytail: re-parse backoff, not incremental parsing. A transcript is
            // append-only, so the right fix is to parse only the appended bytes —
            // which means persisting each provider's mid-file extractor state,
            // including Claude's per-request dedup map, or a request whose lines
            // straddle the boundary gets counted twice. Until then, bound the
            // waste: a transcript costing 500ms to parse is re-read every 10s
            // instead of every 2s, while cheap ones stay effectively live.
            if allow_stale && reuse_stale(entry.parsed_in, entry.parsed_at.elapsed(), entry.size) {
                return entry.data.clone();
            }
        }

        // OpenCode stores every session in one database, and Windsurf every
        // conversation in a workspace in one settings blob. A path-only disk key
        // would make all sessions alias the first extracted row, so keep those
        // entries in the session-keyed memory cache only.
        let disk_key = (!matches!(
            session.provider,
            crate::pricing::Provider::OpenCode | crate::pricing::Provider::Windsurf
        ))
        .then(|| cache_key(file))
        .flatten();
        // Rows only: what the disk holds is missing exactly the fields the panel
        // is opened to read, so serving it there would draw an empty panel over
        // a session that has plenty to show.
        if allow_stale
            && let Some(key) = &disk_key
            && let Some(data) = self.disk.get(key)
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
            return data;
        }

        let started = std::time::Instant::now();
        let mut data = match session.provider {
            crate::pricing::Provider::Claude => crate::session::claude::extract(file),
            crate::pricing::Provider::Codex => crate::session::codex::extract(file),
            crate::pricing::Provider::Cursor => crate::session::cursor::extract(file),
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
        if let Some(key) = disk_key
            && data.error.is_none()
        {
            self.disk.put(key, file, &data);
        }
        data
    }

    /// Drop every cached copy of a deleted session.
    pub fn evict(&self, session: &Session) {
        if let Ok(mut mem) = self.mem.lock() {
            mem.remove(&session.key());
        }
        if let Some(file) = session.data_file.as_ref()
            && let Some(key) = cache_key(file)
        {
            if let Ok(mut e) = self.disk.entries.lock() {
                e.remove(&key);
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
    pub subagent_sort_col: String,
    pub subagent_sort_asc: bool,
    /// Only show sessions whose cost reaches this floor (0 disables it).
    pub cost_floor: f64,
    /// Ring the bell and raise a desktop notification when a session needs you.
    /// Opt-in and remembered, because whether a terminal may make noise is a
    /// property of the room you sit in, not of this run.
    pub notify: bool,
    /// The shell alias block has been written once. Kept here so removing the
    /// block — by flag or by hand — isn't undone by the next launch.
    pub shell_alias_installed: bool,
    /// Table columns the user has hidden, by column id. Unlike the sort order
    /// this is deliberate and effortful to redo, so it survives the run.
    pub hidden_columns: Vec<String>,
    /// Chosen theme name, or `None` to follow the built-in default.
    pub theme: Option<String>,
    /// Recent `/` queries, newest first, so a search worth running twice does
    /// not have to be typed twice. Capped at [`MAX_SEARCH_HISTORY`].
    pub search_history: Vec<String>,
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
            subagent_sort_asc: false,
            cost_floor: 0.0,
            notify: false,
            shell_alias_installed: false,
            hidden_columns: Vec::new(),
            theme: None,
            search_history: Vec::new(),
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
        for provider in [
            "claude", "codex", "cursor", "gemini", "opencode", "pi", "windsurf",
        ] {
            let expected = format!("src/session/{provider}.rs");
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
    /// Unix-only because the awkward path cannot exist elsewhere: `|` is not a
    /// legal character in a Windows filename, so there the bug is unreachable
    /// and the test is just a failing `create_dir_all`.
    #[cfg(unix)]
    #[test]
    fn entries_survive_a_pipe_in_the_transcript_path() {
        let dir = std::env::temp_dir().join(format!("cctop-pipe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a|b.jsonl");
        std::fs::write(&file, "x").unwrap();

        let cache = CostCache {
            entries: Mutex::new(HashMap::new()),
            dirty: Mutex::new(false),
        };
        let key = cache_key(&file).unwrap();
        assert!(key.contains("a|b"), "the key embeds the awkward path");
        cache.put(key.clone(), &file, &SessionData::default());

        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[&key].path, file);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A new key for the same transcript replaces the old one, which is what
    /// keeps the cache from growing without re-`stat`ing everything on save.
    #[test]
    fn a_changed_transcript_supersedes_its_own_entry() {
        let path = Path::new("/tmp/whatever.jsonl");
        let cache = CostCache {
            entries: Mutex::new(HashMap::new()),
            dirty: Mutex::new(false),
        };
        cache.put("k1".into(), path, &SessionData::default());
        cache.put("k2".into(), path, &SessionData::default());
        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key("k2"));
    }

    #[test]
    fn eviction_keeps_the_newest_entries() {
        let mut entries = HashMap::new();
        for i in 0..(MAX_ENTRIES + 10) {
            entries.insert(
                format!("k{i}"),
                Entry {
                    path: PathBuf::from(format!("/tmp/{i}.jsonl")),
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
        assert_ne!(k1, k2, "size change must invalidate the key");
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
        let key = cache_key(&f).unwrap();
        assert!(
            key.contains(&format!("|p{}", crate::pricing::pricing_epoch())),
            "key {key} must embed the pricing epoch"
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
