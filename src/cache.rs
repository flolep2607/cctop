//! On-disk caches: extracted session data, and persisted UI preferences.

use crate::config;
use crate::session::{Session, SessionData};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// Bumped whenever `SessionData`'s shape changes. A mismatch discards the whole
/// cache, which replaces the pile of ad-hoc `_hasSubagentsField`-style probes
/// the JS version accumulated as its schema drifted.
// Version 3 invalidated sessions cached before the codex-auto-review -> GPT-5.2
// pricing alias existed. Version 4 re-extracted Codex tool activity after `exec`
// wrappers began being unwrapped. Version 5 avoided mistaking `tools.*` text
// inside a patch for the wrapper's actual nested call. Version 6 also ignores
// `await tools.*` text inside quoted patch content. Version 7 re-extracts
// Codex web calls so their query is shown instead of `response_length`.
// Version 8 captures OpenCode edit detail, deltas, and duration. Version 9
// records whether each tool call failed.
const CACHE_VERSION: u32 = 9;

#[derive(Serialize, Deserialize)]
struct DiskCache {
    version: u32,
    entries: HashMap<String, SessionData>,
}

impl Default for DiskCache {
    fn default() -> Self {
        DiskCache {
            version: CACHE_VERSION,
            entries: HashMap::new(),
        }
    }
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
    entries: Mutex<HashMap<String, SessionData>>,
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
        let entries = std::fs::read_to_string(&*config::COST_CACHE_FILE)
            .ok()
            .and_then(|t| serde_json::from_str::<DiskCache>(&t).ok())
            .filter(|c| c.version == CACHE_VERSION)
            .map(|c| c.entries)
            .unwrap_or_default();
        CostCache {
            entries: Mutex::new(entries),
            dirty: Mutex::new(false),
        }
    }

    pub fn get(&self, key: &str) -> Option<SessionData> {
        self.entries.lock().ok()?.get(key).cloned()
    }

    pub fn put(&self, key: String, data: &SessionData) {
        if let Ok(mut e) = self.entries.lock() {
            e.insert(key, data.clone());
        }
        if let Ok(mut d) = self.dirty.lock() {
            *d = true;
        }
    }

    /// Write the cache back, dropping entries whose transcript has changed or
    /// been deleted since they were stored.
    pub fn save(&self) {
        if !self.dirty.lock().map(|d| *d).unwrap_or(false) {
            return;
        }
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        entries.retain(|key, _| {
            let Some(path) = key.split('|').next() else {
                return false;
            };
            cache_key(Path::new(path)).as_deref() == Some(key)
        });

        let _ = std::fs::create_dir_all(&*config::CACHE_DIR);
        if let Ok(text) = serde_json::to_string(&DiskCache {
            version: CACHE_VERSION,
            entries: entries.clone(),
        }) {
            let _ = std::fs::write(&*config::COST_CACHE_FILE, text);
        }
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
    /// they bound how much of a core one growing transcript may consume.
    parsed_in: std::time::Duration,
    parsed_at: std::time::Instant,
}

/// How many times its own parse cost a transcript must wait before being parsed
/// again, so each one costs at most `1/N` of a core no matter how large it is.
const REPARSE_BACKOFF: u32 = 20;

/// Whether stale-but-cached data should be served instead of re-parsing.
///
/// A live session appends every few seconds and the cache key is size+mtime, so
/// every append invalidates the entry and the whole file is parsed again. Cheap
/// transcripts stay effectively real-time; only the expensive ones back off.
fn reuse_stale(parsed_in: std::time::Duration, since: std::time::Duration) -> bool {
    since < parsed_in * REPARSE_BACKOFF
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

    /// Extracted data for the session the user has open, never served stale.
    ///
    /// The row-level refresh backs off on expensive transcripts because it pays
    /// that cost once per session, thousands of times over. The open panels are
    /// one session — the one place where a few seconds of lag is actually visible,
    /// and the one place where a full parse per change is affordable.
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
            if entry.mtime == mtime {
                return entry.data.clone();
            }
            // ponytail: re-parse backoff, not incremental parsing. A transcript is
            // append-only, so the right fix is to parse only the appended bytes —
            // which means persisting each provider's mid-file extractor state,
            // including Claude's per-request dedup map, or a request whose lines
            // straddle the boundary gets counted twice. Until then, bound the
            // waste: a transcript costing 500ms to parse is re-read every 10s
            // instead of every 2s, while cheap ones stay effectively live.
            if allow_stale && reuse_stale(entry.parsed_in, entry.parsed_at.elapsed()) {
                return entry.data.clone();
            }
        }

        // OpenCode stores every session in one database. A path-only disk key
        // would make all sessions alias the first extracted row, so keep those
        // entries in the session-keyed memory cache only.
        let disk_key = (session.provider != crate::pricing::Provider::OpenCode)
            .then(|| cache_key(file))
            .flatten();
        if let Some(key) = &disk_key
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
                        // so claim nothing and let the next change be parsed.
                        parsed_in: std::time::Duration::ZERO,
                        parsed_at: std::time::Instant::now(),
                    },
                );
            }
            return data;
        }

        let started = std::time::Instant::now();
        let data = match session.provider {
            crate::pricing::Provider::Claude => crate::session::claude::extract(file),
            crate::pricing::Provider::Codex => crate::session::codex::extract(file),
            crate::pricing::Provider::Cursor => crate::session::cursor::extract(file),
            crate::pricing::Provider::OpenCode => {
                crate::session::opencode::extract(file, &session.session_id)
            }
            crate::pricing::Provider::Pi => crate::session::pi::extract(file),
        };
        let parsed_in = started.elapsed();
        if let Ok(mut mem) = self.mem.lock() {
            mem.insert(
                mem_key,
                MemEntry {
                    mtime,
                    pricing_epoch: epoch,
                    data: data.clone(),
                    parsed_in,
                    parsed_at: std::time::Instant::now(),
                },
            );
        }
        // Never persist a failed parse; it would stick until the file changes.
        if let Some(key) = disk_key
            && data.error.is_none()
        {
            self.disk.put(key, &data);
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
    pub sort_col: String,
    pub sort_asc: bool,
    pub inactivity_filter: Option<String>,
    pub agent_live_filter: bool,
    pub tool_show_diff: bool,
    pub subagent_sort_col: String,
    pub subagent_sort_asc: bool,
    /// Only show sessions whose cost reaches this floor (0 disables it).
    pub cost_floor: f64,
}

impl Default for UiPrefs {
    fn default() -> Self {
        UiPrefs {
            bottom_tab: 0,
            live_only: false,
            sort_col: "active".into(),
            sort_asc: true,
            inactivity_filter: None,
            agent_live_filter: false,
            tool_show_diff: false,
            subagent_sort_col: "last".into(),
            subagent_sort_asc: false,
            cost_floor: 0.0,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reparse_backoff_scales_with_parse_cost() {
        use std::time::Duration;
        // A cheap transcript is re-read almost immediately…
        assert!(!reuse_stale(
            Duration::from_millis(5),
            Duration::from_millis(200)
        ));
        // …an expensive one waits proportionally longer than the refresh interval.
        assert!(reuse_stale(
            Duration::from_millis(500),
            Duration::from_secs(2)
        ));
        assert!(!reuse_stale(
            Duration::from_millis(500),
            Duration::from_secs(11)
        ));
        // Something never parsed here (a disk-cache hit) claims no budget.
        assert!(!reuse_stale(Duration::ZERO, Duration::ZERO));
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

    #[test]
    fn cache_key_absent_for_missing_file() {
        assert!(cache_key(Path::new("/nonexistent/nope.jsonl")).is_none());
    }

    #[test]
    fn prefs_roundtrip_defaults() {
        let p = UiPrefs::default();
        let text = serde_json::to_string(&p).unwrap();
        let back: UiPrefs = serde_json::from_str(&text).unwrap();
        assert_eq!(back.sort_col, "active");
        assert!(back.sort_asc);
    }

    #[test]
    fn prefs_tolerate_missing_and_unknown_fields() {
        let back: UiPrefs = serde_json::from_str(r#"{"bottom_tab":3,"future_field":1}"#).unwrap();
        assert_eq!(back.bottom_tab, 3);
        assert_eq!(back.sort_col, "active"); // filled from Default
    }
}
