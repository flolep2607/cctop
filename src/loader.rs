//! Loading pipeline: discover sessions, extract data in parallel, annotate.

use crate::cache::Store;
use crate::pricing::{Plan, Provider};
use crate::proc;
use crate::session::{self, ContextUsage, Session};
use crate::util;
use chrono::Utc;
use rayon::prelude::*;
use std::collections::HashMap;

/// Half-life for the token- and cost-rate exponential moving averages. Short
/// enough to react to a burst, long enough not to jitter between refreshes.
const EMA_HALF_LIFE_MS: f64 = 10_000.0;

/// Per-session state carried across refreshes to derive rates.
#[derive(Debug, Clone)]
struct RateState {
    ts_ms: i64,
    tokens: u64,
    cost: f64,
    ema_tokens: f64,
    ema_cost: f64,
}

#[derive(Default)]
pub struct Loader {
    store: Store,
    collector: proc::Collector,
    rates: HashMap<String, RateState>,
    /// Last known-good context reading, so a tail read that misses doesn't
    /// blank the CTX% column for a frame.
    context_cache: HashMap<String, ContextUsage>,
}

impl Loader {
    pub fn new() -> Self {
        Loader {
            store: Store::new(),
            collector: proc::Collector::new(),
            rates: HashMap::new(),
            context_cache: HashMap::new(),
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Discover, extract, and annotate every session.
    pub fn load(&mut self, plan: Plan) -> Vec<Session> {
        let mut sessions = session::list_all();

        // Extraction dominates wall time on large transcripts, and each session
        // is independent, so fan out across cores.
        let annotations: Vec<_> = sessions
            .par_iter()
            .map(|s| {
                let data = self.store.session_data(s);
                let m = &data.metrics;
                (
                    data.tokens.all_input(),
                    data.tokens.output,
                    m.tool_count,
                    data.costs.total,
                    data.last_model.clone(),
                    data.title.clone(),
                    data.cost_this_hour(),
                    data.cost_today(),
                    data.costs_by_day.clone(),
                    data.costs_by_hour.clone(),
                    data.subagents.clone(),
                )
            })
            .collect();

        for (s, a) in sessions.iter_mut().zip(annotations) {
            s.input_tokens = a.0;
            s.output_tokens = a.1;
            s.tool_count = a.2;
            s.total_cost = (!plan.includes(s.provider)).then_some(a.3);
            if !a.4.is_empty() {
                s.model = a.4;
            }
            if s.title.is_none() {
                s.title = a.5;
            }
            s.cost_hour = a.6;
            s.cost_today = a.7;
            s.costs_by_day = a.8;
            s.costs_by_hour = a.9;
            s.subagents_cost = a.10.iter().map(|sa| sa.cost).sum();
            s.subagents = a.10;
        }

        self.attach_processes(&mut sessions);
        self.attach_tail_state(&mut sessions);
        self.update_rates(&mut sessions);

        let labels: Vec<String> = sessions.iter().map(|s| s.label_source.clone()).collect();
        for (s, label) in sessions.iter_mut().zip(util::abbreviate_paths(&labels)) {
            s.abbrev_label = label;
        }

        sessions
    }

    fn attach_processes(&mut self, sessions: &mut Vec<Session>) {
        let metrics = self.collector.collect(sessions);
        let mut matched = std::collections::HashSet::new();

        for s in sessions.iter_mut() {
            let key = s.key();
            if let Some(pm) = metrics.get(&key) {
                s.process = Some(pm.clone());
                matched.insert(key);
            } else if s.surface == session::Surface::DesktopCowork {
                // Cowork runs in a cloud VM, so there is no local process. Recent
                // activity is the only available liveness signal; CPU and memory
                // stay unavailable by design.
                let recent = util::parse_ts(&s.last_active)
                    .map(|d| util::now_ms() - d.timestamp_millis() < 90_000)
                    .unwrap_or(false);
                s.process = recent.then(|| proc::ProcInfo {
                    command: "(cowork VM)".into(),
                    ..Default::default()
                });
            }
        }

        // Running agents with no transcript yet still deserve a row.
        for (key, pm) in &metrics {
            if matched.contains(key) {
                continue;
            }
            let orphan = self.collector.orphans().get(key);
            let provider = orphan.map(|o| o.provider).unwrap_or(Provider::Claude);
            let cwd = orphan.map(|o| o.cwd.clone()).unwrap_or_default();
            let now = Utc::now().to_rfc3339();

            let id = key.split(':').nth(1).unwrap_or(key).to_string();
            let mut s = Session::new(provider, id);
            s.started_at = now.clone();
            s.last_active = now;
            s.label_source = cwd;
            s.process = Some(pm.clone());
            sessions.push(s);
        }
    }

    /// Read the last tool and context usage from transcript tails.
    fn attach_tail_state(&mut self, sessions: &mut [Session]) {
        for s in sessions.iter_mut() {
            let key = s.key();
            if s.is_running() {
                s.last_tool = match s.provider {
                    Provider::Claude => session::claude::extract_last_tool(s),
                    Provider::Codex => session::codex::extract_last_tool(s),
                    Provider::OpenCode => session::opencode::extract_last_tool(s),
                    Provider::Pi => session::pi::extract_last_tool(s),
                };
            }
            if s.is_running() || !self.context_cache.contains_key(&key) {
                let fresh = match s.provider {
                    Provider::Claude => session::claude::extract_context(s),
                    Provider::Codex => session::codex::extract_context(s),
                    Provider::OpenCode | Provider::Pi => None,
                };
                if let Some(ctx) = fresh {
                    self.context_cache.insert(key.clone(), ctx);
                }
            }
            s.context = self.context_cache.get(&key).copied();
        }
    }

    /// Fold this refresh's deltas into each session's smoothed rates.
    fn update_rates(&mut self, sessions: &mut [Session]) {
        let now = util::now_ms();
        for s in sessions.iter_mut() {
            let key = s.key();
            let tokens = s.input_tokens + s.output_tokens;
            let cost = s.total_cost.unwrap_or(0.0);

            let Some(prev) = self.rates.get_mut(&key) else {
                // First sighting: no interval to measure a rate over yet.
                self.rates.insert(
                    key,
                    RateState {
                        ts_ms: now,
                        tokens,
                        cost,
                        ema_tokens: 0.0,
                        ema_cost: 0.0,
                    },
                );
                continue;
            };

            let dt_ms = (now - prev.ts_ms) as f64;
            if dt_ms < 100.0 {
                s.tokens_per_min = prev.ema_tokens;
                s.cost_per_min = prev.ema_cost;
                continue;
            }

            let dt_min = dt_ms / 60_000.0;
            let inst_tokens = tokens.saturating_sub(prev.tokens) as f64 / dt_min;
            let inst_cost = (cost - prev.cost).max(0.0) / dt_min;
            let alpha = 1.0 - 2f64.powf(-dt_ms / EMA_HALF_LIFE_MS);

            prev.ema_tokens = alpha * inst_tokens + (1.0 - alpha) * prev.ema_tokens;
            prev.ema_cost = alpha * inst_cost + (1.0 - alpha) * prev.ema_cost;
            prev.ts_ms = now;
            prev.tokens = tokens;
            prev.cost = cost;

            s.tokens_per_min = prev.ema_tokens;
            s.cost_per_min = prev.ema_cost;
        }
    }
}

// ---------------------------------------------------------------------------
// Aggregate statistics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub total: usize,
    pub total_claude: usize,
    pub total_codex: usize,
    pub total_opencode: usize,
    pub total_pi: usize,
    pub active_1h: usize,
    pub active_24h: usize,
    pub active_7d: usize,
    pub total_input: u64,
    pub total_output: u64,
    pub total_cpu: f32,
    pub total_memory: u64,
    pub total_tools: u64,
    pub spend_total: f64,
    pub spend_claude: f64,
    pub spend_codex: f64,
    pub spend_opencode: f64,
    pub spend_pi: f64,
    pub spend_hour: f64,
    pub spend_today: f64,
    pub spend_week: f64,
    pub spend_month: f64,
    /// Smoothed live spend rate across billable sessions, in USD per minute.
    pub spend_per_min: f64,
    /// Spend since the 1st of the current calendar month.
    pub spend_calendar_month: f64,
    /// Today's spend bucketed by hour (24 entries).
    pub daily_hourly: Vec<f64>,
    /// This calendar month's spend bucketed by day.
    pub monthly_daily: Vec<f64>,
    pub models: HashMap<String, usize>,
}

pub fn compute_stats(sessions: &[Session]) -> Stats {
    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    let midnight = util::local_midnight_today();
    let today_key = util::local_date_key(&midnight);
    let week_key = util::local_date_key(&(midnight - chrono::Duration::days(6)));
    let month_key = util::local_date_key(&(midnight - chrono::Duration::days(29)));
    let hour_key = util::local_hour_key(&now);

    let days = util::days_in_current_month() as usize;
    let month_start_key = format!("{}-01", &today_key[..7]);

    let mut st = Stats {
        daily_hourly: vec![0.0; 24],
        monthly_daily: vec![0.0; days],
        ..Default::default()
    };
    st.total = sessions.len();

    for s in sessions {
        match s.provider {
            Provider::Claude => st.total_claude += 1,
            Provider::Codex => st.total_codex += 1,
            Provider::OpenCode => st.total_opencode += 1,
            Provider::Pi => st.total_pi += 1,
        }
        if let Some(la) = util::parse_ts(&s.last_active) {
            let age = now_ms - la.timestamp_millis();
            if age < 3_600_000 {
                st.active_1h += 1;
            }
            if age < 86_400_000 {
                st.active_24h += 1;
            }
            if age < 604_800_000 {
                st.active_7d += 1;
            }
        }

        st.total_input += s.input_tokens;
        st.total_output += s.output_tokens;
        st.total_tools += s.tool_count;
        if let Some(p) = &s.process {
            st.total_cpu += p.cpu;
            st.total_memory += p.memory;
        }
        if let Some(cost) = s.total_cost {
            match s.provider {
                Provider::Claude => st.spend_claude += cost,
                Provider::Codex => st.spend_codex += cost,
                Provider::OpenCode => st.spend_opencode += cost,
                Provider::Pi => st.spend_pi += cost,
            }
            st.spend_per_min += s.cost_per_min;

            // A missing total means this provider is included in the selected
            // billing plan. Its retail-equivalent buckets must not leak back
            // into the overview totals.
            for (day, models) in &s.costs_by_day {
                let amount: f64 = models.values().sum();
                if day.as_str() >= month_key.as_str() {
                    st.spend_month += amount;
                }
                if day.as_str() >= week_key.as_str() {
                    st.spend_week += amount;
                }
                if day.as_str() >= today_key.as_str() {
                    st.spend_today += amount;
                }
                if day.as_str() >= month_start_key.as_str()
                    && let Some(day_part) = day.get(8..10)
                    && let Ok(day_num) = day_part.parse::<usize>()
                    && (1..=days).contains(&day_num)
                {
                    st.monthly_daily[day_num - 1] += amount;
                }
            }
            for (key, models) in &s.costs_by_hour {
                let amount: f64 = models.values().sum();
                if key == &hour_key {
                    st.spend_hour += amount;
                }
                if key.starts_with(&today_key)
                    && let Some(hour_part) = key.get(11..13)
                    && let Ok(hour) = hour_part.parse::<usize>()
                    && hour < 24
                {
                    st.daily_hourly[hour] += amount;
                }
            }
        }

        if !s.model.is_empty() {
            *st.models.entry(util::short_model(&s.model)).or_insert(0) += 1;
        }
    }

    st.spend_total = st.spend_claude + st.spend_codex + st.spend_opencode + st.spend_pi;
    st.spend_calendar_month = st.monthly_daily.iter().sum();
    st
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    fn session_with_day(day: &str, amount: f64) -> Session {
        let mut s = Session::new(Provider::Claude, "x".into());
        s.total_cost = Some(amount);
        s.costs_by_day
            .insert(day.to_string(), HashMap::from([("m".into(), amount)]));
        s.last_active = Utc::now().to_rfc3339();
        s
    }

    #[test]
    fn stats_bucket_today_into_calendar_month() {
        let today = util::local_date_key(&Utc::now());
        let st = compute_stats(&[session_with_day(&today, 2.5)]);
        assert_eq!(st.total, 1);
        assert!((st.spend_today - 2.5).abs() < 1e-9);
        assert!((st.spend_calendar_month - 2.5).abs() < 1e-9);
        assert_eq!(st.active_1h, 1);
    }

    #[test]
    fn stats_exclude_old_days_from_windows() {
        let st = compute_stats(&[session_with_day("2001-01-01", 9.0)]);
        assert_eq!(st.spend_today, 0.0);
        assert_eq!(st.spend_week, 0.0);
        assert_eq!(st.spend_month, 0.0);
        // Lifetime total still counts it.
        assert!((st.spend_claude - 9.0).abs() < 1e-9);
    }

    #[test]
    fn bundled_plan_hides_cost_but_keeps_tokens() {
        let mut s = Session::new(Provider::Claude, "x".into());
        s.total_cost = None;
        s.input_tokens = 100;
        s.cost_per_min = 1.25;
        let today = util::local_date_key(&Utc::now());
        let hour = util::local_hour_key(&Utc::now());
        s.costs_by_day
            .insert(today, HashMap::from([("m".into(), 3.0)]));
        s.costs_by_hour
            .insert(hour, HashMap::from([("m".into(), 2.0)]));
        let st = compute_stats(&[s]);
        assert_eq!(st.spend_claude, 0.0);
        assert_eq!(st.spend_today, 0.0);
        assert_eq!(st.spend_hour, 0.0);
        assert_eq!(st.spend_calendar_month, 0.0);
        assert_eq!(st.spend_per_min, 0.0);
        assert_eq!(st.total_input, 100);
    }

    #[test]
    fn stats_sum_billable_live_spend_rate() {
        let mut a = Session::new(Provider::Claude, "a".into());
        a.total_cost = Some(4.0);
        a.cost_per_min = 0.25;
        let mut b = Session::new(Provider::Codex, "b".into());
        b.total_cost = Some(6.0);
        b.cost_per_min = 0.75;

        let st = compute_stats(&[a, b]);
        assert!((st.spend_per_min - 1.0).abs() < 1e-9);
    }
}
