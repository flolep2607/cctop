//! Per-token pricing tables, billing-plan resolution, and the LiteLLM fallback.
//!
//! Built-in tables cover the models we see most often; anything else falls back
//! to the LiteLLM database, cached on disk for 24h. Unknown models price at zero
//! rather than failing, so a new model release never crashes the display.

use crate::config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

#[derive(Debug, Clone, Copy, Default)]
pub struct ClaudePricing {
    pub input: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
    pub cache_read: f64,
    pub output: f64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CodexPricing {
    pub input: f64,
    pub cached_input: f64,
    pub output: f64,
}

/// Rates for a model with no built-in table, taken straight from LiteLLM.
///
/// Used by harnesses that let the user point at an arbitrary provider, where the
/// model can be anything and the harness may not price it itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct GenericPricing {
    pub input: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub output: f64,
}

/// USD per million tokens.
const fn claude(input: f64, cw5m: f64, cw1h: f64, cr: f64, output: f64) -> ClaudePricing {
    ClaudePricing {
        input,
        cache_write_5m: cw5m,
        cache_write_1h: cw1h,
        cache_read: cr,
        output,
    }
}

static CLAUDE_TABLE: LazyLock<HashMap<&'static str, ClaudePricing>> = LazyLock::new(|| {
    let opus = claude(5.0, 6.25, 10.0, 0.5, 25.0);
    let sonnet = claude(3.0, 3.75, 6.0, 0.3, 15.0);
    let fable = claude(10.0, 12.5, 20.0, 1.0, 50.0);
    HashMap::from([
        ("fable-5", fable),
        ("claude-fable-5", fable),
        ("claude-opus-5", opus),
        ("claude-opus-4-8", opus),
        ("claude-opus-4-7", opus),
        ("claude-opus-4-6", opus),
        ("claude-opus-4-5-20251101", opus),
        ("claude-sonnet-5", sonnet),
        ("claude-sonnet-4-6", sonnet),
        ("claude-sonnet-4-5-20250929", sonnet),
        (
            "claude-haiku-4-5-20251001",
            claude(1.0, 1.25, 2.0, 0.1, 5.0),
        ),
    ])
});

static CODEX_TABLE: LazyLock<HashMap<&'static str, CodexPricing>> = LazyLock::new(|| {
    // `codex-auto-review` is an internal label emitted by Codex review
    // sessions. It runs GPT-5.2, so retain the useful label in the UI while
    // applying GPT-5.2's published token rates.
    let gpt_5_2 = CodexPricing {
        input: 1.75,
        cached_input: 0.175,
        output: 14.0,
    };
    HashMap::from([
        ("gpt-5.2", gpt_5_2),
        ("gpt-5.2-codex", gpt_5_2),
        ("codex-auto-review", gpt_5_2),
        (
            "gpt-5.3-codex",
            CodexPricing {
                input: 1.75,
                cached_input: 0.175,
                output: 14.0,
            },
        ),
        (
            "codex-mini-latest",
            CodexPricing {
                input: 1.5,
                cached_input: 0.375,
                output: 6.0,
            },
        ),
    ])
});

// ---------------------------------------------------------------------------
// LiteLLM fallback
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LitellmEntry {
    #[serde(default)]
    pub input_cost_per_token: f64,
    #[serde(default)]
    pub output_cost_per_token: f64,
    #[serde(default)]
    pub cache_read_input_token_cost: f64,
    /// Absent and zero mean different things: a listing that omits the field
    /// says nothing about cache writes, while an explicit 0 means the provider
    /// writes to cache for free. Only the latter may price a write at nothing.
    #[serde(default)]
    pub cache_creation_input_token_cost: Option<f64>,
    #[serde(default)]
    pub max_input_tokens: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct PricingCache {
    #[serde(rename = "_fetchedAt")]
    fetched_at: u64,
    data: HashMap<String, serde_json::Value>,
}

static LITELLM: LazyLock<RwLock<Option<HashMap<String, LitellmEntry>>>> =
    LazyLock::new(|| RwLock::new(None));

/// Generation stamp for the currently loaded pricing table.
///
/// Cached session data holds *computed* costs, so it must be invalidated when
/// the rates behind those costs change — not only when a transcript grows.
/// Without this, sessions priced before the table was fetched keep reporting
/// $0.00 forever, because their transcripts never change again.
static PRICING_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn pricing_epoch() -> u64 {
    PRICING_EPOCH.load(std::sync::atomic::Ordering::Relaxed)
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn parse_entries(raw: &HashMap<String, serde_json::Value>) -> HashMap<String, LitellmEntry> {
    raw.iter()
        .filter_map(|(k, v)| {
            serde_json::from_value::<LitellmEntry>(v.clone())
                .ok()
                .map(|e| (k.clone(), e))
        })
        .collect()
}

fn install(raw: HashMap<String, serde_json::Value>, epoch: u64) {
    if let Ok(mut guard) = LITELLM.write() {
        *guard = Some(parse_entries(&raw));
    }
    PRICING_EPOCH.store(epoch, std::sync::atomic::Ordering::Relaxed);
}

/// Install a fixed table so tests that exercise the LiteLLM fallback do not
/// depend on whatever the machine happens to have cached.
///
/// The table is process-wide and the test harness is threaded, so this hands
/// back a guard that has to live for the rest of the test: without it, two tests
/// in different modules install over each other and each reads the other's
/// rates.
#[cfg(test)]
#[must_use = "hold the guard until the test is done reading the table"]
pub fn install_test_table(
    rows: &[(&str, serde_json::Value)],
) -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let raw = rows
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect();
    install(raw, 1);
    guard
}

/// Load pricing from the disk cache if it is fresh. Returns `true` on success.
pub fn load_cached_pricing() -> bool {
    let Ok(text) = std::fs::read_to_string(&*config::PRICING_CACHE_FILE) else {
        return false;
    };
    let Ok(cache) = serde_json::from_str::<PricingCache>(&text) else {
        return false;
    };
    let fresh = unix_secs().saturating_sub(cache.fetched_at) < config::PRICING_CACHE_MAX_AGE_SECS;
    let epoch = cache.fetched_at;
    install(cache.data, epoch);
    fresh
}

/// Fetch the LiteLLM pricing database and refresh the disk cache.
///
/// Blocking; call from a background thread. Falls back to any stale cached copy
/// when the network is unavailable.
pub fn refresh_pricing_blocking() {
    if load_cached_pricing() {
        return; // cache is fresh, nothing to do
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build()
        .into();
    let fetched = agent
        .get(config::LITELLM_URL)
        .call()
        .ok()
        .and_then(|mut resp| resp.body_mut().read_to_string().ok())
        .and_then(|text| serde_json::from_str::<HashMap<String, serde_json::Value>>(&text).ok());

    let Some(data) = fetched else { return }; // keep whatever the stale load installed

    let now = unix_secs();
    let _ = std::fs::create_dir_all(&*config::CACHE_DIR);
    if let Ok(text) = serde_json::to_string(&PricingCache {
        fetched_at: now,
        data: data.clone(),
    }) {
        let _ = std::fs::write(&*config::PRICING_CACHE_FILE, text);
    }
    install(data, now);
}

/// Whether `key` is LiteLLM's name for `model` under some route prefix.
///
/// Models arrive vendor-qualified (`moonshotai/kimi-k2.5`) while LiteLLM lists
/// the same model once per route it can be reached by: `openrouter/moonshotai/
/// kimi-k2.5`, `bedrock/us-east-1/moonshotai.kimi-k2.5`, `moonshotai.kimi-k2.5`.
/// Lower-casing and treating `.` and `/` alike as separators makes those one
/// name — safe here because an exact match is tried first, so nothing that is
/// listed verbatim ever reaches this.
fn names_model(key: &str, model: &str) -> bool {
    let norm = |s: &str| s.to_ascii_lowercase().replace('.', "/");
    let (key, model) = (norm(key), norm(model));
    key == model || key.ends_with(&format!("/{model}"))
}

/// Whether an entry carries a rate worth using. Some rows exist only to record a
/// route's context window and leave both costs at zero; matching one of those is
/// no better than not matching at all, and it hides a real listing further down
/// the ladder.
fn priced(e: &LitellmEntry) -> bool {
    e.input_cost_per_token > 0.0 || e.output_cost_per_token > 0.0
}

/// The shortest priced key satisfying `pred`, ties broken alphabetically.
///
/// Both parts matter. Shortest means fewest route prefixes, so a vendor listing
/// wins over a reseller's. Deterministic means the same model prices the same on
/// every run: `HashMap` iteration order is randomised per process, so picking any
/// match made a session's cost jump between runs — and the figure gets cached, so
/// it stuck.
fn best_match(
    table: &HashMap<String, LitellmEntry>,
    pred: impl Fn(&str) -> bool,
) -> Option<&LitellmEntry> {
    table
        .iter()
        .filter(|(k, v)| priced(v) && pred(k))
        .min_by(|a, b| (a.0.len(), a.0).cmp(&(b.0.len(), b.0)))
        .map(|(_, v)| v)
}

/// Look up a model in the LiteLLM table.
///
/// Two names are tried — as given, and with the vendor prefix dropped, since
/// `anthropic/claude-sonnet-4-5` is listed bare — and they are tried tier by tier
/// so an exact listing always beats a fuzzy match on either form. Last tier is any
/// key containing the name, which is what answers for an undated model whose only
/// listing is dated.
fn lookup<'a>(table: &'a HashMap<String, LitellmEntry>, model: &str) -> Option<&'a LitellmEntry> {
    let stem = model.rsplit('/').next().filter(|s| *s != model);
    let names = || std::iter::once(model).chain(stem);
    table
        .get(model)
        .or_else(|| table.get(&format!("anthropic.{model}")))
        // A stem is already a guess, so it has to land on a real rate to count.
        .or_else(|| names().find_map(|n| table.get(n).filter(|e| priced(e))))
        .or_else(|| names().find_map(|n| best_match(table, |k| names_model(k, n))))
        .or_else(|| names().find_map(|n| best_match(table, |k| k.contains(n))))
}

fn litellm_entry(model: &str) -> Option<LitellmEntry> {
    let guard = LITELLM.read().ok()?;
    lookup(guard.as_ref()?, model).cloned()
}

/// Context window reported by LiteLLM, if known.
pub fn litellm_max_input_tokens(model: &str) -> Option<u64> {
    litellm_entry(model).and_then(|e| e.max_input_tokens)
}

/// Rates for any model LiteLLM lists, whatever route it is reached by.
///
/// `None` means the model is genuinely unknown, which the caller needs to tell
/// apart from a free one: a harness that reports no cost of its own has nothing
/// to fall back on, and inventing $0.00 would read as "this was free".
pub fn resolve_generic(model: &str) -> Option<GenericPricing> {
    let e = litellm_entry(model)?;
    let input = e.input_cost_per_token * 1e6;
    Some(GenericPricing {
        input,
        cache_read: e.cache_read_input_token_cost * 1e6,
        // A listing that omits the write rate says nothing about it; base input
        // is the closest defensible guess, and understating it to zero would
        // silently drop a real charge.
        cache_write: e.cache_creation_input_token_cost.map_or(input, |c| c * 1e6),
        output: e.output_cost_per_token * 1e6,
    })
}

pub fn resolve_claude(model: &str) -> ClaudePricing {
    if let Some(p) = CLAUDE_TABLE.get(model) {
        return *p;
    }
    match litellm_entry(model) {
        Some(e) => {
            let input = e.input_cost_per_token * 1e6;
            ClaudePricing {
                input,
                // LiteLLM omits cache-write rates; Anthropic prices them at
                // 1.25x (5m) and 2x (1h) of base input.
                cache_write_5m: input * 1.25,
                cache_write_1h: input * 2.0,
                cache_read: e.cache_read_input_token_cost * 1e6,
                output: e.output_cost_per_token * 1e6,
            }
        }
        None => ClaudePricing::default(),
    }
}

pub fn resolve_codex(model: &str) -> CodexPricing {
    if let Some(p) = CODEX_TABLE.get(model) {
        return *p;
    }
    match litellm_entry(model) {
        Some(e) => CodexPricing {
            input: e.input_cost_per_token * 1e6,
            cached_input: e.cache_read_input_token_cost * 1e6,
            output: e.output_cost_per_token * 1e6,
        },
        None => CodexPricing::default(),
    }
}

// ---------------------------------------------------------------------------
// Billing plans
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Standard usage-based API pricing.
    Retail,
    /// Claude Max: Claude usage bundled, Codex still billed at retail.
    Max,
    /// Everything bundled; show costs as `incl`.
    Included,
}

impl Plan {
    pub fn parse(s: &str) -> Option<Plan> {
        match s.to_ascii_lowercase().as_str() {
            "retail" | "default" => Some(Plan::Retail),
            "max" | "claude-max" => Some(Plan::Max),
            "included" | "enterprise" | "not-billed" => Some(Plan::Included),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Plan::Retail => "retail",
            Plan::Max => "max",
            Plan::Included => "included",
        }
    }

    /// Whether this plan bundles the given provider's usage.
    pub fn includes(&self, provider: Provider) -> bool {
        matches!(
            (self, provider),
            (Plan::Included, _) | (Plan::Max, Provider::Claude)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    Claude,
    Codex,
    Cursor,
    Gemini,
    OpenCode,
    Pi,
    Windsurf,
}

impl Provider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            Provider::Cursor => "cursor",
            Provider::Gemini => "gemini",
            Provider::OpenCode => "opencode",
            Provider::Pi => "pi",
            Provider::Windsurf => "windsurf",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_table_hits_before_litellm() {
        let p = resolve_claude("claude-opus-4-5-20251101");
        assert_eq!(p.input, 5.0);
        assert_eq!(p.output, 25.0);
        assert_eq!(p.cache_write_1h, 10.0);
    }

    #[test]
    fn unknown_model_prices_at_zero_not_panic() {
        let p = resolve_claude("claude-does-not-exist-9");
        assert_eq!(p.input, 0.0);
        let c = resolve_codex("gpt-nope");
        assert_eq!(c.output, 0.0);
    }

    #[test]
    fn auto_review_uses_gpt_5_2_pricing_without_renaming() {
        let auto = resolve_codex("codex-auto-review");
        let gpt = resolve_codex("gpt-5.2");
        assert_eq!(auto.input, gpt.input);
        assert_eq!(auto.cached_input, gpt.cached_input);
        assert_eq!(auto.output, gpt.output);
    }

    /// Key shapes taken from the real LiteLLM table, which lists a model once per
    /// route it can be reached by, at that route's price.
    fn kimi_table() -> HashMap<String, LitellmEntry> {
        let entry = |input: f64| LitellmEntry {
            input_cost_per_token: input,
            ..Default::default()
        };
        [
            ("moonshotai.kimi-k2.5", 6e-7),
            ("moonshot/kimi-k2.5", 6e-7),
            ("openrouter/moonshotai/kimi-k2.5", 7e-7),
            ("bedrock/ap-south-1/moonshotai.kimi-k2.5", 7.2e-7),
            ("together_ai/moonshotai/Kimi-K2.5", 5e-7),
            ("claude-sonnet-4-5-20250929", 3e-6),
            ("gpt-5.2", 1.75e-6),
            // Real shape of a route that records only a context window.
            ("perplexity/anthropic/claude-sonnet-4-5", 0.0),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), entry(v)))
        .collect()
    }

    /// A vendor-qualified model is listed only behind route prefixes, and the
    /// route decides the price — so the match has to be the vendor's own listing
    /// and it has to be the same one every run.
    #[test]
    fn a_routed_model_resolves_to_its_vendor_listing() {
        let table = kimi_table();
        for _ in 0..8 {
            let e = lookup(&table, "moonshotai/kimi-k2.5").expect("no match");
            assert_eq!(e.input_cost_per_token, 6e-7);
        }
        // A dated variant still answers for the undated name.
        assert!(lookup(&table, "claude-sonnet-4-5").is_some());
        assert!(lookup(&table, "gpt-nope").is_none());
    }

    /// A vendor-prefixed name whose bare form is listed must take the bare price,
    /// not a reseller route that happens to be shorter — and never an unpriced row.
    #[test]
    fn a_vendor_prefix_falls_back_to_the_bare_listing() {
        let table = kimi_table();
        let e = lookup(&table, "anthropic/claude-sonnet-4-5").expect("no match");
        assert_eq!(e.input_cost_per_token, 3e-6);
        assert_eq!(
            lookup(&table, "openai/gpt-5.2").map(|e| e.input_cost_per_token),
            Some(1.75e-6)
        );
    }

    #[test]
    fn plan_bundling() {
        assert!(!Plan::Retail.includes(Provider::Claude));
        assert!(Plan::Max.includes(Provider::Claude));
        assert!(!Plan::Max.includes(Provider::Codex));
        assert!(Plan::Included.includes(Provider::Codex));
    }

    #[test]
    fn plan_aliases() {
        assert_eq!(Plan::parse("MAX"), Some(Plan::Max));
        assert_eq!(Plan::parse("not-billed"), Some(Plan::Included));
        assert_eq!(Plan::parse("bogus"), None);
    }
}
