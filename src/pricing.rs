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

/// Look up a model in the LiteLLM table: exact, then `anthropic.`-prefixed,
/// then the first key containing the model name.
fn litellm_entry(model: &str) -> Option<LitellmEntry> {
    let guard = LITELLM.read().ok()?;
    let table = guard.as_ref()?;
    if let Some(e) = table.get(model) {
        return Some(e.clone());
    }
    if let Some(e) = table.get(&format!("anthropic.{model}")) {
        return Some(e.clone());
    }
    table
        .iter()
        .find(|(k, _)| k.contains(model))
        .map(|(_, v)| v.clone())
}

/// Context window reported by LiteLLM, if known.
pub fn litellm_max_input_tokens(model: &str) -> Option<u64> {
    litellm_entry(model).and_then(|e| e.max_input_tokens)
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
    OpenCode,
    Pi,
}

impl Provider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            Provider::Cursor => "cursor",
            Provider::OpenCode => "opencode",
            Provider::Pi => "pi",
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
