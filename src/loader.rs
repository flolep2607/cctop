//! Loading pipeline: discover sessions, extract data in parallel, annotate.

use crate::cache::Store;
use crate::pricing::{Plan, Provider};
use crate::proc;
use crate::session::{self, ContextUsage, Session, SessionData};
use crate::util;
use chrono::Utc;
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Half-life for the token- and cost-rate exponential moving averages. Short
/// enough to react to a burst, long enough not to jitter between refreshes.
const EMA_HALF_LIFE_MS: f64 = 10_000.0;

/// Cores an aggregate CPU reading is measured against.
///
/// A process's CPU% is measured against one core, so summing the agents on a
/// 128-core machine reads 9000% — a true number answering a question nobody
/// asked, and one the sparkline beside it cannot draw, since that is scaled to
/// 100. Divided by the cores available it becomes the share of the machine,
/// which is what a summary line is for. The per-row `CPU%` column keeps the
/// per-core reading, because "this agent is using a whole core" is exactly what
/// you want from a row and htop spells it the same way.
///
/// `available_parallelism` rather than a physical core count, so a container
/// with a CPU quota is measured against what it may actually use.
static CORES: std::sync::LazyLock<f32> = std::sync::LazyLock::new(|| {
    std::thread::available_parallelism().map_or(1.0, |n| n.get() as f32)
});

/// Per-session state carried across refreshes to derive rates.
#[derive(Debug, Clone)]
struct RateState {
    ts_ms: i64,
    tokens: u64,
    cost: f64,
    ema_tokens: f64,
    ema_cost: f64,
}

/// Threads used for background re-extraction.
///
/// Rayon defaults to one per core, which is right when someone is waiting for the
/// first table and wrong forever after: it turns each refresh into a spike across
/// every core, so the machine alternates between idle and fully committed. A
/// monitor should hum rather than stampede, so background walks get a quarter of
/// the machine and take proportionally longer, which nobody is waiting on.
fn gentle_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get() / 4)
        .unwrap_or(2)
        .clamp(2, 4)
}

#[derive(Default)]
pub struct Loader {
    store: OnceLock<Store>,
    /// Small pool for background extraction; `None` if one can't be built, which
    /// just means falling back to rayon's default.
    gentle: Option<rayon::ThreadPool>,
    collector: proc::Collector,
    rates: HashMap<String, RateState>,
    /// Last known-good context reading, so a tail read that misses doesn't
    /// blank the CTX% column for a frame.
    context_cache: HashMap<String, ContextUsage>,
    /// Activity state and last tool per session, so a walk re-reads only the
    /// transcripts that can still change.
    tail_cache: HashMap<String, (session::ActivityState, String)>,
}

impl Loader {
    pub fn new() -> Self {
        Loader {
            store: OnceLock::new(),
            gentle: rayon::ThreadPoolBuilder::new()
                .num_threads(gentle_threads())
                .thread_name(|i| format!("cctop-extract-{i}"))
                .build()
                .ok(),
            collector: proc::Collector::new(),
            rates: HashMap::new(),
            context_cache: HashMap::new(),
            tail_cache: HashMap::new(),
        }
    }

    /// How the last walk matched processes to sessions. See `cctop why`.
    pub fn attributions(&self) -> &[crate::proc::Attribution] {
        self.collector.attributions()
    }

    pub fn store(&self) -> &Store {
        self.store.get_or_init(Store::new)
    }

    /// Run `job` on the background pool, or inline when there isn't one.
    ///
    /// Anything that fans out across every session on disk belongs here rather
    /// than on rayon's default pool: the reason [`gentle_threads`] exists is
    /// that a monitor should not seize the machine to do work nobody is
    /// blocking on, and a transcript scan is exactly that kind of work.
    pub fn gently<R: Send>(&self, job: impl FnOnce() -> R + Send) -> R {
        match self.gentle.as_ref() {
            Some(pool) => pool.install(job),
            None => job(),
        }
    }

    /// Discover, extract, and annotate every session.
    pub fn load(&mut self, plan: Plan) -> Vec<Session> {
        // The CLI's caller is always waiting on the result.
        self.load_progressive(plan, true, |_| {}, |_| {})
    }

    /// Discover sessions first, then report each fully annotated row as soon as
    /// its transcript finishes loading. Callbacks run on the loader worker and
    /// Rayon threads respectively, so they must stay cheap and non-blocking.
    pub fn load_progressive<D, A>(
        &mut self,
        plan: Plan,
        eager: bool,
        on_discovered: D,
        on_annotated: A,
    ) -> Vec<Session>
    where
        D: FnOnce(&[Session]),
        A: Fn(&Session) + Sync,
    {
        let _span = crate::trace::span("walk");
        let mut sessions = session::list_all();

        // Process state and labels are cheap enough to include in the first
        // visible rows; transcript tails and full extraction can follow.
        {
            let _span = crate::trace::span("walk.processes");
            self.attach_processes(&mut sessions);
        }
        let labels: Vec<String> = sessions.iter().map(|s| s.label_source.clone()).collect();
        for (s, label) in sessions.iter_mut().zip(util::abbreviate_paths(&labels)) {
            s.abbrev_label = label;
        }
        on_discovered(&sessions);

        self.attach_tail_state(&mut sessions, eager);
        let store = self.store();

        // Extraction dominates wall time on large transcripts, and each session
        // is independent, so fan out across cores and publish rows individually
        // instead of withholding the entire table for the slowest transcript.
        //
        // How wide to fan out depends on who is waiting: the first table should
        // arrive as fast as the machine allows, while a background refresh that
        // nobody asked for should not seize every core to do it.
        let step = |s: &mut Session| {
            annotate(s, &store.session_data(s), plan);
            on_annotated(s);
        };
        {
            let _span = crate::trace::span("walk.extract");
            match self.gentle.as_ref().filter(|_| !eager) {
                Some(pool) => pool.install(|| sessions.par_iter_mut().for_each(step)),
                None => sessions.par_iter_mut().for_each(step),
            }
        }

        self.update_rates(&mut sessions);

        sessions
    }

    fn attach_processes(&mut self, sessions: &mut Vec<Session>) {
        let metrics = self.collector.collect(sessions);
        let mut matched = std::collections::HashSet::new();

        for s in sessions.iter_mut() {
            let key = s.key();
            if let Some(pm) = metrics.get(&key) {
                s.process = Some(pm.clone());
                s.harness = harness_from_process(s, &pm.command).into();
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
            } else if s.provider == Provider::Cursor {
                // Cursor has a shared editor process rather than one process
                // per native-agent transcript. A freshly growing transcript is
                // the only trustworthy per-session liveness signal available.
                s.inferred_running = util::parse_ts(&s.last_active)
                    .map(|d| util::now_ms() - d.timestamp_millis() < 90_000)
                    .unwrap_or(false);
            }
        }

        // Running agents with no transcript yet still deserve a row.
        for (key, pm) in &metrics {
            if matched.contains(key) {
                continue;
            }
            let Some(orphan) = self.collector.orphans().get(key) else {
                // In particular, never turn an unattributed Codex app-server
                // PID into a synthetic session with no rollout file.
                continue;
            };
            let provider = orphan.provider;
            let cwd = orphan.cwd.clone();
            let now = Utc::now().to_rfc3339();

            let id = key.split(':').nth(1).unwrap_or(key).to_string();
            let mut s = Session::new(provider, id);
            s.started_at = now.clone();
            s.last_active = now;
            s.label_source = cwd;
            s.process = Some(pm.clone());
            s.harness = harness_from_process(&s, &pm.command).into();
            sessions.push(s);
        }
    }

    /// Read the last tool and context usage from transcript tails.
    ///
    /// Fanned out, because each session's tail is an independent read and there
    /// are as many as have ever been recorded. Serially this was the largest
    /// thing left in a walk once extraction was cached — 2.07s of a 3.03s run
    /// on a 2020-session machine, done one session at a time on 52 cores.
    ///
    /// Which sessions need reading is decided first, so the parallel pass
    /// borrows nothing of the loader, and the caches are folded back in
    /// afterwards. The alternative — locking them — would serialise the reads
    /// again for the sake of two maps that are only written once each.
    fn attach_tail_state(&mut self, sessions: &mut [Session], eager: bool) {
        let _span = crate::trace::span("walk.tails");
        // A stopped session's transcript cannot change, so its tail only needs
        // reading once.
        //
        // ponytail: keyed by session, not by mtime, so a stopped session whose
        // file is edited behind cctop's back keeps its old dot until it runs
        // again. Extraction still notices, so only the tail-derived fields go
        // stale; re-check the mtime here if that ever matters.
        let wanted: Vec<Option<bool>> = sessions
            .iter()
            .map(|s| {
                let running = s.is_running();
                if !running && self.tail_cache.contains_key(&s.key()) {
                    return None; // already known, and it cannot have changed
                }
                Some(running || !self.context_cache.contains_key(&s.key()))
            })
            .collect();

        let read = || {
            sessions
                .par_iter()
                .zip(wanted.par_iter())
                .map(|(s, want)| want.map(|context| read_tail(s, context)))
                .collect::<Vec<_>>()
        };
        // Same bargain as extraction: seize the machine only when somebody is
        // waiting for the first table.
        let reads = match self.gentle.as_ref().filter(|_| !eager) {
            Some(pool) => pool.install(read),
            None => read(),
        };

        for (s, read) in sessions.iter_mut().zip(reads) {
            let key = s.key();
            let Some(read) = read else {
                if let Some((state, tool)) = self.tail_cache.get(&key) {
                    s.activity_state = *state;
                    s.last_tool = tool.clone();
                }
                s.context = self.context_cache.get(&key).copied();
                continue;
            };
            self.apply_tail(s, read, &key);
        }
    }

    /// Copy one tail read onto its row and remember what it found.
    fn apply_tail(&mut self, s: &mut Session, read: TailRead, key: &str) {
        s.activity_state = read.state;
        // The transcript is the baseline, and every Claude Code session has one
        // whether or not cctop's hooks are installed. A live agent's own report
        // is fresher and outranks it, which `App::apply_reports` applies on
        // top of this.
        s.permission = read.permission;
        if s.is_running() {
            s.last_tool = read.last_tool;
        }
        if let Some(ctx) = read.context {
            self.context_cache.insert(key.to_string(), ctx);
        }
        s.context = self.context_cache.get(key).copied();
        self.tail_cache
            .insert(key.to_string(), (s.activity_state, s.last_tool.clone()));
    }

    /// Activity dot, last tool, and context window for one row.
    ///
    /// The single-session path, for the light refresh. The full walk reads every
    /// session at once through [`Self::attach_tail_state`].
    fn tail_state(&mut self, s: &mut Session) {
        let key = s.key();
        if !s.is_running()
            && let Some((state, tool)) = self.tail_cache.get(&key)
        {
            s.activity_state = *state;
            s.last_tool = tool.clone();
            s.context = self.context_cache.get(&key).copied();
            return;
        }
        let want_context = s.is_running() || !self.context_cache.contains_key(&key);
        let read = read_tail(s, want_context);
        self.apply_tail(s, read, &key);
    }

    /// Refresh only what can have moved since the last full walk.
    ///
    /// Discovery is what costs at scale: walking every provider's directories and
    /// stat-ing thousands of transcripts, nearly all of which belong to sessions
    /// that ended days ago. New sessions appear rarely compared to how often the
    /// running ones change, so the caller carries the last full result forward and
    /// pays here only for the rows that are actually moving.
    ///
    /// Returns the rows that changed, so the caller can ship those instead of
    /// copying the whole table back to the UI.
    pub fn refresh_live(&mut self, plan: Plan, sessions: &mut Vec<Session>) -> Vec<Session> {
        let _span = crate::trace::span("refresh-live");
        // A row that has just stopped changed too, and its cleared process has to
        // reach the table, so remember who was live before the process sweep.
        let was_live: std::collections::HashSet<String> = sessions
            .iter()
            .filter(|s| s.is_running())
            .map(Session::key)
            .collect();

        // Runs first: it can also append a row for an agent that started since the
        // last walk and has no transcript yet, and those need annotating too.
        self.attach_processes(sessions);

        let moved: Vec<usize> = sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_running() || was_live.contains(&s.key()))
            .map(|(i, _)| i)
            .collect();
        {
            let store = self.store();
            for &i in &moved {
                let s = &mut sessions[i];
                annotate(s, &store.session_data(s), plan);
            }
        }
        // Separate pass: `store()` borrows self immutably, `tail_state` mutably.
        for &i in &moved {
            self.tail_state(&mut sessions[i]);
        }

        // Rates are pure arithmetic over figures already in hand, and the EMAs
        // decay with wall time, so every row still gets one.
        self.update_rates(sessions);
        moved.iter().map(|&i| sessions[i].clone()).collect()
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

/// Infer the host application from a matched process without confusing it with
/// the model. The Codex binary may be installed under Cursor's server
/// extension directory, which does not make the running harness Cursor.
/// Everything a tail read produces for one session.
struct TailRead {
    state: session::ActivityState,
    permission: Option<crate::hook::Permission>,
    last_tool: String,
    context: Option<ContextUsage>,
}

/// Read one session's tail, borrowing nothing that the caller holds — which is
/// what lets every session be read at once.
fn read_tail(s: &Session, want_context: bool) -> TailRead {
    // Split by provider and by step: this is the dominant cost of a walk once
    // extraction is cached, and "the tails are slow" cannot say whether that is
    // reading transcript text or querying a database.
    let _by_provider = crate::trace::span(match s.provider {
        Provider::Claude => "tails.claude",
        Provider::Codex => "tails.codex",
        Provider::Cursor => "tails.cursor",
        Provider::Gemini => "tails.gemini",
        Provider::OpenCode => "tails.opencode",
        Provider::Pi => "tails.pi",
        Provider::Windsurf => "tails.windsurf",
    });
    let (state, permission) = {
        let _span = crate::trace::span("tails.state");
        session::live_state(s)
    };
    let last_tool = match s.is_running() {
        false => String::new(),
        true => match s.provider {
            Provider::Claude => session::claude::extract_last_tool(s),
            Provider::Codex => session::codex::extract_last_tool(s),
            Provider::Cursor | Provider::Gemini | Provider::Windsurf => String::new(),
            Provider::OpenCode => session::opencode::extract_last_tool(s),
            Provider::Pi => session::pi::extract_last_tool(s),
        },
    };
    let context = want_context
        .then(|| {
            let _span = crate::trace::span("tails.context");
            match s.provider {
                Provider::Claude => session::claude::extract_context(s),
                Provider::Codex => session::codex::extract_context(s),
                Provider::OpenCode => session::opencode::extract_context(s),
                Provider::Cursor | Provider::Gemini | Provider::Pi | Provider::Windsurf => None,
            }
        })
        .flatten();
    TailRead {
        state,
        permission,
        last_tool,
        context,
    }
}

/// Copy an extraction's figures onto the row that owns it.
///
/// Shared by the full walk and the light refresh so the two can never disagree
/// about what a row's numbers mean.
fn annotate(s: &mut Session, data: &SessionData, plan: Plan) {
    let m = &data.metrics;
    s.input_tokens = data.tokens.all_input();
    s.output_tokens = data.tokens.output;
    s.tool_count = m.tool_count;
    s.tool_errors = m.tool_errors;
    s.compactions = data.compactions;
    s.total_cost = (s.cost_available && !plan.includes(s.provider)).then_some(data.costs.total);
    s.cost_is_free = s.cost_available
        && data.costs.total == 0.0
        && data.tokens.all_input() + data.tokens.output > 0;
    if !data.last_model.is_empty() {
        s.model = data.last_model.clone();
    }
    if s.title.is_none() {
        s.title = data.title.clone();
    }
    s.cost_hour = data.cost_this_hour();
    s.cost_today = data.cost_today();
    s.costs_by_day = data.costs_by_day.clone();
    s.costs_by_hour = data.costs_by_hour.clone();
    s.subagents_cost = data.subagents.iter().map(|sa| sa.cost).sum();
    s.subagents = data.subagents.clone();
    s.recent_writes = recent_writes(&data.recent_writes, &s.label_source);
}

/// Resolve the session's recent writes against the directory it runs in.
///
/// The paths themselves are distilled during extraction — see
/// [`session::SessionData::recent_writes`] — because the tool history they come
/// from is not persisted. Only this step needs the cwd, and only this step is
/// cheap enough to redo on every walk.
fn recent_writes(paths: &[String], cwd: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    paths
        .iter()
        .map(|p| crate::collide::normalise(p, cwd))
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

fn harness_from_process(session: &Session, command: &str) -> &'static str {
    let command = command.to_ascii_lowercase();
    if command.contains("anysphere.cursor")
        || command.contains("cursor.app/")
        || command.contains("/cursor-server/bin/")
    {
        return "Cursor";
    }
    if command.contains("hermes") {
        return "Hermes";
    }
    match session.surface {
        session::Surface::Editor => "Cursor",
        session::Surface::DesktopCode => "Claude Desktop",
        session::Surface::DesktopCowork => "Claude Cowork",
        session::Surface::Cli => match session.provider {
            Provider::Claude => "ClaudeCode",
            Provider::Codex => "Codex",
            Provider::Cursor => "Cursor",
            Provider::Gemini => "Gemini",
            Provider::OpenCode => "OpenCode",
            Provider::Pi => "Pi",
            Provider::Windsurf => "Windsurf",
        },
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
    pub total_cursor: usize,
    pub total_gemini: usize,
    pub total_opencode: usize,
    pub total_pi: usize,
    pub total_windsurf: usize,
    pub active_1h: usize,
    pub active_24h: usize,
    pub active_7d: usize,
    /// Sessions that are currently backed by a detected process or a trusted
    /// provider-specific liveness inference. This deliberately differs from
    /// the recent-activity windows above.
    pub running: usize,
    pub total_input: u64,
    pub total_output: u64,
    /// Share of the whole machine the agents are using, 0-100 — not the sum of
    /// their per-core readings. See [`CORES`].
    pub total_cpu: f32,
    pub total_memory: u64,
    pub total_tools: u64,
    pub spend_total: f64,
    pub spend_claude: f64,
    pub spend_codex: f64,
    pub spend_cursor: f64,
    pub spend_gemini: f64,
    pub spend_opencode: f64,
    pub spend_pi: f64,
    /// Always zero: Windsurf records no local accounting. Kept so the
    /// per-provider breakdown stays exhaustive rather than silently skipping a
    /// provider that might start reporting cost later.
    pub spend_windsurf: f64,
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
    /// Today's spend per project label, largest first.
    ///
    /// Grouped by the abbreviated working directory rather than by session,
    /// because the question the Overview answers is "where did today's money
    /// go", and one piece of work is usually several sessions of it.
    pub top_today: Vec<(String, f64)>,
    /// Today's spend per model, largest first. Distinct from [`models`], which
    /// counts sessions ever seen and so is dominated by whatever was in fashion
    /// months ago.
    ///
    /// [`models`]: Self::models
    pub models_today: Vec<(String, f64)>,
}

/// Largest first, ties broken by name so the Overview does not reshuffle two
/// equal rows every frame.
fn descending(map: HashMap<String, f64>) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = map.into_iter().filter(|(_, v)| *v > 0.0).collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
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
    let mut today_by_project: HashMap<String, f64> = HashMap::new();
    let mut today_by_model: HashMap<String, f64> = HashMap::new();

    for s in sessions {
        match s.provider {
            Provider::Claude => st.total_claude += 1,
            Provider::Codex => st.total_codex += 1,
            Provider::Cursor => st.total_cursor += 1,
            Provider::Gemini => st.total_gemini += 1,
            Provider::OpenCode => st.total_opencode += 1,
            Provider::Pi => st.total_pi += 1,
            Provider::Windsurf => st.total_windsurf += 1,
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
        if s.is_running() {
            st.running += 1;
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
                Provider::Cursor => st.spend_cursor += cost,
                Provider::Gemini => st.spend_gemini += cost,
                Provider::OpenCode => st.spend_opencode += cost,
                Provider::Pi => st.spend_pi += cost,
                Provider::Windsurf => st.spend_windsurf += cost,
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
                    let project = match s.abbrev_label.is_empty() {
                        true => "—",
                        false => s.abbrev_label.as_str(),
                    };
                    *today_by_project.entry(project.to_string()).or_default() += amount;
                    for (model, spend) in models {
                        *today_by_model.entry(util::short_model(model)).or_default() += spend;
                    }
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

    st.spend_total = st.spend_claude
        + st.spend_codex
        + st.spend_cursor
        + st.spend_gemini
        + st.spend_opencode
        + st.spend_pi
        + st.spend_windsurf;
    st.spend_calendar_month = st.monthly_daily.iter().sum();
    st.top_today = descending(today_by_project);
    st.models_today = descending(today_by_model);
    // Summed per-core above; reported as a share of the machine.
    st.total_cpu /= *CORES;
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
        assert_eq!(st.running, 0);
    }

    #[test]
    fn stats_count_only_running_sessions_as_live() {
        let mut running = session_with_day(&util::local_date_key(&Utc::now()), 0.0);
        running.process = Some(crate::proc::ProcInfo::default());
        let recently_stopped = session_with_day(&util::local_date_key(&Utc::now()), 0.0);

        let st = compute_stats(&[running, recently_stopped]);

        assert_eq!(st.active_1h, 2);
        assert_eq!(st.running, 1);
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

    /// Summed per-core readings are what a process reports and not what a
    /// machine has: four agents each using a whole core is 400% of a core and,
    /// on anything with four or more of them, a fraction of the box. The
    /// headline figure answers the second question — the row answers the first.
    #[test]
    fn agent_cpu_is_a_share_of_the_machine_not_a_sum_of_cores() {
        let busy = |cpu: f32| {
            let mut s = Session::new(Provider::Claude, "x".into());
            s.process = Some(crate::proc::ProcInfo {
                cpu,
                ..Default::default()
            });
            s
        };
        let st = compute_stats(&[busy(100.0), busy(100.0), busy(50.0)]);

        // 250% of a core, divided by however many this machine has.
        let expected = 250.0 / *CORES;
        assert!(
            (st.total_cpu - expected).abs() < 1e-3,
            "got {}, expected {expected}",
            st.total_cpu
        );
        // …and on any machine with three or more cores it cannot read above 100,
        // which is the scale the sparkline beside it is drawn against.
        if *CORES >= 3.0 {
            assert!(st.total_cpu <= 100.0, "{} is off the chart", st.total_cpu);
        }
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

    /// The paths reach a row from the extraction now that the tool history they
    /// were derived from is no longer cached — so a cached session and a
    /// re-parsed one must still light the same collision warning. Resolving them
    /// against the cwd stays here, which is why a relative path has to arrive
    /// spelled absolutely.
    #[test]
    fn a_row_takes_its_recent_writes_from_the_extraction() {
        let mut s = Session::new(Provider::Claude, "x".into());
        s.label_source = "/repo".into();
        let data = SessionData {
            recent_writes: vec![
                "src/main.rs".into(),
                "/repo/src/lib.rs".into(),
                // Two spellings of one file collapse once resolved.
                "./src/main.rs".into(),
            ],
            ..Default::default()
        };

        annotate(&mut s, &data, Plan::Retail);

        // Spelled with the host's separator, because that is what `normalise`
        // resolves to and what the collision check compares.
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            s.recent_writes,
            [
                format!("{sep}repo{sep}src{sep}main.rs"),
                format!("{sep}repo{sep}src{sep}lib.rs"),
            ]
        );
    }

    #[test]
    fn harness_distinguishes_cursor_from_model() {
        let s = Session::new(Provider::Codex, "x".into());
        assert_eq!(
            harness_from_process(
                &s,
                "/home/flo/.cursor-server/extensions/openai.chatgpt/bin/codex app-server"
            ),
            "Codex"
        );
        assert_eq!(
            harness_from_process(&s, "/opt/Cursor.app/Contents/MacOS/Cursor"),
            "Cursor"
        );
        assert_eq!(harness_from_process(&s, "codex resume x"), "Codex");

        let mut other = Session::new(Provider::Claude, "x".into());
        assert_eq!(
            harness_from_process(&other, "claude --resume x"),
            "ClaudeCode"
        );
        other.provider = Provider::OpenCode;
        assert_eq!(
            harness_from_process(&other, "opencode --session x"),
            "OpenCode"
        );
        other.provider = Provider::Pi;
        assert_eq!(harness_from_process(&other, "pi --session x"), "Pi");
        assert_eq!(harness_from_process(&other, "hermes run"), "Hermes");
    }
}
