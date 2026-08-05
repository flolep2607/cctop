//! Terminal UI: application state, the worker thread, and the event loop.

pub mod columns;
pub mod panels;
pub mod render;
pub mod spark;
pub mod theme;

use crate::cache::UiPrefs;
use crate::cli::Args;
use crate::loader::{Loader, Stats};
use crate::pricing::{Plan, Provider};
use crate::quota::Quota;
use crate::session::{Session, SessionData};
use columns::{COLUMNS, ColumnId};
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::crossterm::execute;
use spark::History;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

/// Baseline gap between usage checks.
///
/// Quota moves slowly, and the endpoints throttle aggressively — a 30s poll was
/// enough to earn a sustained 429 with a ~15 minute `retry-after`. When a
/// provider asks for longer, `retry_delay_secs` honours that instead.
const QUOTA_INTERVAL_SECS: u64 = 300;

/// How often the poller wakes to see whether any provider is due.
const QUOTA_TICK: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    List,
    Help,
    Search,
    SortBy,
    AgeFilter,
    /// Confirming deletion of the selected session.
    DeleteConfirm,
    /// Explaining why a running session can't be deleted.
    DeleteBlocked,
    /// Confirming termination of the selected live session.
    KillConfirm,
    /// Explaining why a live session cannot be terminated locally.
    KillBlocked,
    /// Confirming a batch action over all marked sessions.
    BatchConfirm,
    /// Explaining why a batch delete couldn't proceed (a marked session is running).
    BatchDeleteBlocked,
    /// Explaining why a batch kill couldn't proceed (a marked session has no root PID).
    BatchKillBlocked,
    /// Numeric input for the cost floor filter.
    CostFilter,
}

/// The pending batch action shown in `Mode::BatchConfirm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchKind {
    Delete,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgeFilter {
    Day,
    Week,
    Month,
}

impl AgeFilter {
    pub fn max_age_ms(&self) -> i64 {
        match self {
            AgeFilter::Day => 86_400_000,
            AgeFilter::Week => 604_800_000,
            AgeFilter::Month => 2_592_000_000,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            AgeFilter::Day => "Last 24 hours",
            AgeFilter::Week => "Last 7 days",
            AgeFilter::Month => "Last 30 days",
        }
    }

    pub fn short(&self) -> &'static str {
        match self {
            AgeFilter::Day => "1 day",
            AgeFilter::Week => "1 week",
            AgeFilter::Month => "1 month",
        }
    }

    pub fn key(&self) -> &'static str {
        match self {
            AgeFilter::Day => "1d",
            AgeFilter::Week => "1w",
            AgeFilter::Month => "1mo",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "1d" => Some(AgeFilter::Day),
            "1w" => Some(AgeFilter::Week),
            "1mo" => Some(AgeFilter::Month),
            _ => None,
        }
    }
}

/// Options offered by the age-filter modal, "no filter" last.
pub const AGE_OPTIONS: [Option<AgeFilter>; 4] = [
    Some(AgeFilter::Day),
    Some(AgeFilter::Week),
    Some(AgeFilter::Month),
    None,
];

// ---------------------------------------------------------------------------
// Worker protocol
// ---------------------------------------------------------------------------

enum Request {
    Refresh,
    /// Extract full data for one session, to populate the bottom panels.
    Data(Box<Session>),
    Delete(Box<Session>),
    Terminate {
        session_key: String,
        pid: u32,
    },
    Shutdown,
}

enum Response {
    /// Cheap discovery result, shown before transcript extraction completes.
    Discovered(Vec<Session>),
    /// One row whose transcript has finished loading.
    Annotated(Box<Session>),
    Sessions(Box<(Vec<Session>, Stats)>),
    Data(String, Box<SessionData>),
    Quota(Box<Quota>),
    /// Pricing landed, so cached costs are stale and a reload is due.
    PricingReady,
    Terminated {
        session_key: String,
        result: Result<(), String>,
    },
}

/// Owns the `Loader` and does all filesystem and parsing work off the UI thread,
/// so a slow scan can never stall input or rendering.
fn spawn_worker(
    plan: Plan,
    rx: Receiver<Request>,
    tx: Sender<Response>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut loader = Loader::new();
        let mut sent_initial_discovery = false;
        while let Ok(req) = rx.recv() {
            match req {
                Request::Refresh => {
                    let publish_discovery = !sent_initial_discovery;
                    sent_initial_discovery = true;
                    let sessions = loader.load_progressive(
                        plan,
                        |sessions| {
                            if publish_discovery {
                                let _ = tx.send(Response::Discovered(sessions.to_vec()));
                            }
                        },
                        |session| {
                            let _ = tx.send(Response::Annotated(Box::new(session.clone())));
                        },
                    );
                    let stats = crate::loader::compute_stats(&sessions);
                    if tx
                        .send(Response::Sessions(Box::new((sessions, stats))))
                        .is_err()
                    {
                        break;
                    }
                }
                Request::Data(session) => {
                    let data = loader.store().session_data(&session);
                    if tx
                        .send(Response::Data(session.key(), Box::new(data)))
                        .is_err()
                    {
                        break;
                    }
                }
                Request::Delete(session) => {
                    match session.provider {
                        Provider::Claude => {
                            crate::session::claude::delete(&session);
                        }
                        Provider::Codex => {
                            crate::session::codex::delete(&session);
                        }
                        Provider::Cursor => {
                            crate::session::cursor::delete(&session);
                        }
                        Provider::OpenCode => {
                            let _ = crate::session::opencode::delete(&session);
                        }
                        Provider::Pi => {
                            let _ = crate::session::pi::delete(&session);
                        }
                    }
                    loader.store().evict(&session);
                }
                Request::Terminate { session_key, pid } => {
                    let result = crate::proc::terminate(pid);
                    if tx
                        .send(Response::Terminated {
                            session_key,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Request::Shutdown => break,
            }
        }
        loader.store().save();
    })
}

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

pub struct App {
    pub sessions: Vec<Session>,
    /// Indices into `sessions`, after filtering and sorting.
    pub visible: Vec<usize>,
    pub stats: Stats,
    pub selected: usize,
    pub scroll: usize,
    pub plan: Plan,
    pub mode: Mode,

    pub sort_col: ColumnId,
    pub sort_asc: bool,
    pub sortby_cursor: usize,

    pub search: String,
    pub age_filter: Option<AgeFilter>,
    pub age_cursor: usize,
    pub live_only: bool,

    /// Session keys the user has marked (Space) for a batch action.
    pub marked: HashSet<String>,
    /// The action pending confirmation in `Mode::BatchConfirm`.
    pub batch: BatchKind,
    /// Follow mode: keep the selected row centered.
    pub follow: bool,
    /// Seconds between automatic refreshes (adjustable live with +/-/=).
    pub refresh_secs: f64,
    /// Only show sessions whose total cost reaches this floor.
    pub cost_floor: f64,
    /// Raw digits being typed into the cost-floor modal.
    pub cost_input: String,
    /// Table viewport height (rows), recorded during draw so Ctrl+U/Ctrl+D can
    /// page by half a screen.
    pub list_height: u16,

    pub bottom_tab: usize,
    pub panel_data: Option<SessionData>,
    panel_key: String,
    /// `last_active` of the session when its panel data was requested, so an
    /// append can be told apart from an unchanged session.
    panel_stamp: String,
    pub info_scroll: u16,
    pub cost_scroll: u16,
    pub config_scroll: u16,
    pub proc_scroll: u16,
    pub subagent_scroll: u16,
    pub tool_scroll: u16,
    /// Pin the tool log to its newest entry. Tool Activity is an append-only
    /// feed, so following the tail is the useful default; scrolling up releases
    /// the pin, and scrolling back to the bottom restores it.
    pub tool_follow: bool,
    /// Last computed maximum scroll for the tool log, recorded during draw so
    /// the key handler knows where the bottom is.
    pub tool_max_scroll: u16,
    pub tool_tab: usize,
    pub tool_live_only: bool,
    /// Show each edit's diff inline beneath its row.
    pub tool_show_diff: bool,
    /// Invocation whose full argument is expanded, keyed by `detail_key`.
    pub tool_expanded: Option<String>,
    /// Which invocation owns each rendered line, so a click maps to an entry.
    pub tool_owners: Vec<Option<String>>,
    pub subagent_sort: (panels::SubagentSort, bool),

    pub cpu_history: HashMap<String, History>,
    pub mem_history: HashMap<String, History>,
    pub global_cpu: History,
    pub global_spend: History,

    pub quota: Quota,
    pub status: Option<(String, Instant)>,
    /// When cctop started, used by the tool-activity "live" filter.
    pub started_at: String,

    prefs: UiPrefs,
    tx: Sender<Request>,
    pub needs_redraw: bool,
    pub should_quit: bool,
}

impl App {
    fn new(plan: Plan, tx: Sender<Request>) -> Self {
        Self::with_prefs(plan, tx, UiPrefs::load())
    }

    /// Build with explicit preferences.
    ///
    /// Tests use this with `UiPrefs::default()`; going through `new` would load
    /// whatever is on the developer's disk and make results machine-dependent.
    fn with_prefs(plan: Plan, tx: Sender<Request>, prefs: UiPrefs) -> Self {
        let sort_col = columns::column_by_key(&prefs.sort_col)
            .map(|c| c.id)
            .unwrap_or(ColumnId::Last);
        let age_filter = prefs
            .inactivity_filter
            .as_deref()
            .and_then(AgeFilter::parse);
        let age_cursor = AGE_OPTIONS
            .iter()
            .position(|o| *o == age_filter)
            .unwrap_or(AGE_OPTIONS.len() - 1);

        App {
            sessions: Vec::new(),
            visible: Vec::new(),
            stats: Stats::default(),
            selected: 0,
            scroll: 0,
            plan,
            mode: Mode::List,
            sort_col,
            sort_asc: prefs.sort_asc,
            sortby_cursor: 0,
            search: String::new(),
            age_filter,
            age_cursor,
            live_only: prefs.live_only,
            marked: HashSet::new(),
            batch: BatchKind::Delete,
            follow: false,
            refresh_secs: 2.0,
            cost_floor: prefs.cost_floor,
            cost_input: String::new(),
            list_height: 0,
            bottom_tab: prefs.bottom_tab.min(panels::TABS.len() - 1),
            panel_data: None,
            panel_key: String::new(),
            panel_stamp: String::new(),
            info_scroll: 0,
            cost_scroll: 0,
            config_scroll: 0,
            proc_scroll: 0,
            subagent_scroll: 0,
            tool_scroll: 0,
            tool_follow: true,
            tool_max_scroll: 0,
            tool_tab: 0,
            tool_live_only: prefs.agent_live_filter,
            tool_show_diff: prefs.tool_show_diff,
            tool_expanded: None,
            tool_owners: Vec::new(),
            subagent_sort: (
                panels::SubagentSort::parse(&prefs.subagent_sort_col),
                prefs.subagent_sort_asc,
            ),
            cpu_history: HashMap::new(),
            mem_history: HashMap::new(),
            global_cpu: History::default(),
            global_spend: History::default(),
            quota: Quota::default(),
            status: None,
            started_at: chrono::Utc::now().to_rfc3339(),
            prefs,
            tx,
            needs_redraw: true,
            should_quit: false,
        }
    }

    /// The highlighted session, if there is one.
    ///
    /// `visible` holds indices into `sessions`, and a refresh replaces
    /// `sessions` before `refilter` rebuilds `visible` — so between those two
    /// steps the indices can outrun the list. Resolving through `get` keeps this
    /// accessor total instead of panicking on that window.
    pub fn selected_session(&self) -> Option<&Session> {
        self.visible
            .get(self.selected)
            .and_then(|&i| self.sessions.get(i))
    }

    fn save_prefs(&mut self) {
        self.prefs.bottom_tab = self.bottom_tab;
        self.prefs.live_only = self.live_only;
        self.prefs.sort_col = columns::column(self.sort_col).key.to_string();
        self.prefs.sort_asc = self.sort_asc;
        self.prefs.inactivity_filter = self.age_filter.map(|a| a.key().to_string());
        self.prefs.agent_live_filter = self.tool_live_only;
        self.prefs.tool_show_diff = self.tool_show_diff;
        self.prefs.subagent_sort_col = self.subagent_sort.0.key().to_string();
        self.prefs.subagent_sort_asc = self.subagent_sort.1;
        self.prefs.cost_floor = self.cost_floor;
        self.prefs.save();
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
        self.needs_redraw = true;
    }

    /// Apply filters and sorting, then rebuild the visible index list.
    ///
    /// Selection is tracked by session key rather than row number, so a refresh
    /// that reorders the table doesn't move the cursor off whatever the user was
    /// looking at.
    pub fn refilter(&mut self) {
        let anchor = self.selected_session().map(|s| s.key());
        let now = chrono::Utc::now();
        let now_ms = now.timestamp_millis();
        let query = self.search.to_ascii_lowercase();

        let mut visible: Vec<usize> = (0..self.sessions.len())
            .filter(|&i| {
                let s = &self.sessions[i];
                if self.live_only && !s.is_running() {
                    return false;
                }
                if let Some(age) = self.age_filter {
                    let ts = if s.last_active.is_empty() {
                        &s.started_at
                    } else {
                        &s.last_active
                    };
                    let within = crate::util::parse_ts(ts)
                        .map(|d| now_ms - d.timestamp_millis() <= age.max_age_ms())
                        .unwrap_or(false);
                    if !within {
                        return false;
                    }
                }
                if !query.is_empty() {
                    let haystack = format!(
                        "{} {} {} {} {}",
                        s.display_label(),
                        s.model,
                        s.harness,
                        s.provider.as_str(),
                        s.session_id
                    )
                    .to_ascii_lowercase();
                    if !haystack.contains(&query) {
                        return false;
                    }
                }
                if self.cost_floor > 0.0 {
                    // Sessions with unknown cost are kept: the floor can't say
                    // they're below it. "Included" cost (None) counts as zero.
                    let cost = if s.cost_available {
                        s.total_cost.unwrap_or(0.0)
                    } else {
                        // Unknown — can't disqualify.
                        return true;
                    };
                    if cost < self.cost_floor {
                        return false;
                    }
                }
                true
            })
            .collect();

        let col = self.sort_col;
        let asc = self.sort_asc;
        visible.sort_by(|&a, &b| {
            let ord = columns::compare(col, &self.sessions[a], &self.sessions[b], &now);
            if asc { ord } else { ord.reverse() }
        });

        self.visible = visible;
        self.selected = anchor
            .and_then(|key| {
                self.visible
                    .iter()
                    .position(|&i| self.sessions[i].key() == key)
            })
            .unwrap_or(self.selected)
            .min(self.visible.len().saturating_sub(1));
        self.ensure_available_tab();
        self.needs_redraw = true;
    }

    /// Fold this refresh's figures into the overview history buffers.
    fn push_history(&mut self) {
        // This is a rate, not a refresh delta, so its meaning is stable when
        // the user changes --delay or a filesystem scan takes longer.
        self.global_spend.push(self.stats.spend_per_min);
        self.global_cpu.push(self.stats.total_cpu as f64);

        for s in &self.sessions {
            let Some(p) = &s.process else { continue };
            let key = s.key();
            self.cpu_history
                .entry(key.clone())
                .or_default()
                .push(p.cpu as f64);
            self.mem_history
                .entry(key)
                .or_default()
                .push(p.memory as f64 / (1024.0 * 1024.0));
        }
    }

    /// Ask the worker for the selected session's full data if it isn't loaded.
    fn sync_panel_data(&mut self) {
        let Some(session) = self.selected_session() else {
            self.panel_data = None;
            self.panel_key.clear();
            return;
        };
        let key = session.key();
        let stamp = session.last_active.clone();
        let switched = key != self.panel_key;
        // A live session keeps growing, so re-request whenever its newest
        // activity moves — otherwise the panels freeze at whatever the session
        // looked like when it was selected.
        let grew = !switched && stamp != self.panel_stamp;
        if !switched && !grew {
            return;
        }

        let session = session.clone();
        self.panel_key = key;
        self.panel_stamp = stamp;

        if switched {
            // Only blank the panels when moving to a different session; doing it
            // on every append would flash "Loading…" twice a second.
            self.panel_data = None;
            self.info_scroll = 0;
            self.cost_scroll = 0;
            self.config_scroll = 0;
            self.proc_scroll = 0;
            self.subagent_scroll = 0;
            self.tool_scroll = 0;
            self.tool_follow = true;
            self.tool_tab = 0;
            self.tool_expanded = None;
        }
        let _ = self.tx.send(Request::Data(Box::new(session)));
    }

    fn move_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last as isize) as usize;
        self.ensure_available_tab();
        self.needs_redraw = true;
    }

    fn tab_available(&self, tab: usize) -> bool {
        !matches!(tab, 1 | 2) || self.selected_session().is_some_and(Session::is_running)
    }

    fn ensure_available_tab(&mut self) {
        if !self.tab_available(self.bottom_tab) {
            self.bottom_tab = 0;
        }
    }

    fn set_sort(&mut self, col: ColumnId) {
        if self.sort_col == col {
            self.sort_asc = !self.sort_asc;
        } else {
            self.sort_col = col;
            self.sort_asc = true;
        }
        self.refilter();
        self.save_prefs();
    }

    /// Expand or collapse the invocation under a clicked log row.
    fn toggle_tool_expansion(&mut self, row_offset: usize) {
        let line = self.tool_scroll as usize + row_offset;
        let Some(Some(key)) = self.tool_owners.get(line) else {
            return;
        };
        let key = key.clone();
        // Clicking the open entry again closes it.
        self.tool_expanded = (self.tool_expanded.as_deref() != Some(key.as_str())).then_some(key);
        // Expanding grows the log, which would otherwise slide the row away.
        self.tool_follow = false;
        self.needs_redraw = true;
    }

    /// Move through the Tool Activity sidebar, which filters the log by tool.
    fn cycle_tool_filter(&mut self, delta: isize) {
        let n = self
            .panel_data
            .as_ref()
            .map(|d| panels::tool_tabs(d).len())
            .unwrap_or(0);
        if n == 0 {
            return;
        }
        let n = n as isize;
        self.tool_tab = (((self.tool_tab as isize + delta) % n + n) % n) as usize;
        // A different filter is a different log, so start at its newest entry.
        self.tool_follow = true;
        self.bottom_tab = 3;
        self.needs_redraw = true;
    }

    /// Move to the next or previous bottom panel, wrapping at both ends.
    fn cycle_tab(&mut self, delta: isize) {
        let n = panels::TABS.len() as isize;
        let mut next = self.bottom_tab;
        for _ in 0..panels::TABS.len() {
            next = (((next as isize + delta) % n + n) % n) as usize;
            if self.tab_available(next) {
                self.bottom_tab = next;
                break;
            }
        }
        self.save_prefs();
        self.needs_redraw = true;
    }

    fn scroll_active_panel(&mut self, delta: i32) {
        let bump = |v: &mut u16| *v = (*v as i32 + delta).max(0) as u16;
        match self.bottom_tab {
            0 => bump(&mut self.info_scroll),
            1 => {} // Performance is a fixed-size chart pair
            2 => bump(&mut self.proc_scroll),
            3 => {
                let next = (self.tool_scroll as i32 + delta).clamp(0, self.tool_max_scroll as i32);
                self.tool_scroll = next as u16;
                // Re-pin once the user scrolls back down to the newest entry.
                self.tool_follow = self.tool_scroll >= self.tool_max_scroll;
            }
            4 => bump(&mut self.subagent_scroll),
            5 => bump(&mut self.cost_scroll),
            _ => bump(&mut self.config_scroll),
        }
        self.needs_redraw = true;
    }

    /// Copy something useful about the selection to the clipboard.
    fn copy_selection(&mut self) {
        let Some(s) = self.selected_session() else {
            return;
        };
        let text = match self.bottom_tab {
            // From the Info tab, the resume command is the most useful thing.
            0 => match s.provider {
                Provider::Claude => format!("claude --resume {}", s.session_id),
                Provider::Codex => format!("codex resume {}", s.session_id),
                Provider::Cursor => s
                    .data_file
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| s.session_id.clone()),
                Provider::OpenCode => format!("opencode --session {}", s.session_id),
                Provider::Pi => format!("pi --session {}", s.session_id),
            },
            _ => s
                .data_file
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| s.session_id.clone()),
        };
        render::copy_to_clipboard(&text);
        self.set_status(format!("Copied: {}", crate::util::truncate(&text, 60)));
    }

    /// Whether the session matches the active text search.
    fn matches_search(&self, s: &Session) -> bool {
        let query = self.search.to_ascii_lowercase();
        if query.is_empty() {
            return true;
        }
        let haystack = format!(
            "{} {} {} {} {}",
            s.display_label(),
            s.model,
            s.harness,
            s.provider.as_str(),
            s.session_id
        )
        .to_ascii_lowercase();
        haystack.contains(&query)
    }

    /// Jump to the next/previous session matching the active search, wrapping
    /// around both ends. With no search active every visible session matches.
    fn cycle_matches(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let matches: Vec<usize> = self
            .visible
            .iter()
            .copied()
            .filter(|&i| self.matches_search(&self.sessions[i]))
            .collect();
        if matches.is_empty() {
            return;
        }
        let current = self.visible.get(self.selected).copied();
        let pos = current.and_then(|key| matches.iter().position(|&i| i == key));
        let n = matches.len() as isize;
        let next = ((pos.unwrap_or(0) as isize + delta).rem_euclid(n)) as usize;
        self.selected = self
            .visible
            .iter()
            .position(|&i| i == matches[next])
            .unwrap_or(self.selected);
        self.ensure_available_tab();
        self.needs_redraw = true;
    }

    /// Toggle whether the selected session is marked for a batch action.
    fn toggle_mark(&mut self) {
        let Some(s) = self.selected_session() else {
            return;
        };
        let key = s.key();
        if !self.marked.remove(&key) {
            self.marked.insert(key);
        }
        self.needs_redraw = true;
    }

    /// The session keys currently marked, in table order for a stable listing.
    fn marked_sessions(&self) -> Vec<&Session> {
        self.visible
            .iter()
            .filter_map(|&i| self.sessions.get(i))
            .filter(|s| self.marked.contains(&s.key()))
            .collect()
    }

    /// True when every marked session is ready for the given batch action.
    fn batch_ok(&self, kind: BatchKind) -> bool {
        self.marked_sessions().iter().all(|s| match kind {
            BatchKind::Delete => !s.is_running(),
            BatchKind::Kill => session_root_pid(s).is_some(),
        })
    }

    fn unmark_all(&mut self) {
        if self.marked.is_empty() {
            return;
        }
        self.marked.clear();
        self.needs_redraw = true;
    }

    /// Enter the batch-confirm modal if there's anything to do.
    fn batch(&mut self, kind: BatchKind) {
        if self.marked_sessions().is_empty() {
            self.set_status("No sessions marked — press Space to mark");
            return;
        }
        self.batch = kind;
        self.mode = if self.batch_ok(kind) {
            Mode::BatchConfirm
        } else {
            match kind {
                BatchKind::Delete => Mode::BatchDeleteBlocked,
                BatchKind::Kill => Mode::BatchKillBlocked,
            }
        };
        self.needs_redraw = true;
    }

    /// Confirm and run the pending batch action over all marked sessions.
    fn batch_execute(&mut self) {
        let kind = self.batch;
        let marked = self.marked_sessions();
        let mut removed: Vec<String> = Vec::new();
        let mut failed = 0;
        for s in marked {
            let key = s.key();
            match kind {
                BatchKind::Delete => {
                    self.tx.send(Request::Delete(Box::new(s.clone()))).ok();
                    removed.push(key);
                }
                BatchKind::Kill => match session_root_pid(s) {
                    Some(pid) => {
                        self.tx
                            .send(Request::Terminate {
                                session_key: key.clone(),
                                pid,
                            })
                            .ok();
                        removed.push(key);
                    }
                    None => failed += 1,
                },
            }
        }
        for key in &removed {
            self.marked.remove(key);
        }
        self.sessions.retain(|s| !removed.contains(&s.key()));
        self.refilter();
        self.set_status(match kind {
            BatchKind::Delete => {
                if failed == 0 {
                    format!("Deleted {} session(s)", removed.len())
                } else {
                    format!("Deleted {}, {} failed", removed.len(), failed)
                }
            }
            BatchKind::Kill => format!(
                "Kill sent to {} session(s){}",
                removed.len(),
                if failed > 0 {
                    format!(" ({} skipped)", failed)
                } else {
                    String::new()
                }
            ),
        });
    }

    /// Adjust the live refresh interval, clamping to sane bounds.
    fn adjust_refresh(&mut self, delta: f64) {
        self.refresh_secs = (self.refresh_secs + delta).clamp(0.5, 60.0);
        self.needs_redraw = true;
    }

    /// Half the visible table height, used by Ctrl+U/Ctrl+D. Falls back to a
    /// page size before the first draw has recorded a viewport.
    fn half_page(&self) -> usize {
        ((self.list_height as usize / 2).max(1)).min(PAGE as usize)
    }
}

/// Rows moved by PageUp/PageDown and the fallback for half-page scrolls.
const PAGE: isize = 10;

/// PID of the currently live agent root, excluding briefly retained exits.
fn session_root_pid(session: &Session) -> Option<u32> {
    session
        .process
        .as_ref()?
        .process_list
        .iter()
        .find_map(|process| (process.is_root && !process.ghost).then_some(process.pid))
}

// ---------------------------------------------------------------------------
// Event handling
// ---------------------------------------------------------------------------

impl App {
    fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        self.needs_redraw = true;

        // Ctrl-C quits from any mode.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match self.mode {
            Mode::Search => self.on_key_search(key),
            Mode::SortBy => self.on_key_sortby(key),
            Mode::AgeFilter => self.on_key_age(key),
            Mode::DeleteConfirm => self.on_key_delete(key),
            Mode::KillConfirm => self.on_key_kill(key),
            Mode::BatchConfirm | Mode::BatchDeleteBlocked | Mode::BatchKillBlocked => {
                self.on_key_batch(key)
            }
            Mode::CostFilter => self.on_key_cost(key),
            Mode::Help | Mode::DeleteBlocked | Mode::KillBlocked => self.mode = Mode::List,
            Mode::List => self.on_key_list(key),
        }
    }

    fn on_key_search(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.mode = Mode::List,
            KeyCode::Backspace => {
                self.search.pop();
                self.refilter();
            }
            KeyCode::Char(c) => {
                self.search.push(c);
                self.refilter();
            }
            _ => {}
        }
    }

    fn on_key_sortby(&mut self, key: KeyEvent) {
        let n = COLUMNS.len();
        match key.code {
            KeyCode::Esc | KeyCode::F(6) => self.mode = Mode::List,
            KeyCode::Up | KeyCode::Char('k') => {
                self.sortby_cursor = (self.sortby_cursor + n - 1) % n
            }
            KeyCode::Down | KeyCode::Char('j') => self.sortby_cursor = (self.sortby_cursor + 1) % n,
            KeyCode::Enter => {
                self.set_sort(COLUMNS[self.sortby_cursor].id);
                self.mode = Mode::List;
            }
            KeyCode::Char('q') => self.should_quit = true,
            _ => {}
        }
    }

    fn on_key_age(&mut self, key: KeyEvent) {
        let n = AGE_OPTIONS.len();
        match key.code {
            KeyCode::Esc | KeyCode::F(7) => self.mode = Mode::List,
            KeyCode::Up | KeyCode::Char('k') => self.age_cursor = (self.age_cursor + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') => self.age_cursor = (self.age_cursor + 1) % n,
            KeyCode::Enter => {
                self.age_filter = AGE_OPTIONS[self.age_cursor];
                self.refilter();
                self.save_prefs();
                self.mode = Mode::List;
            }
            _ => {}
        }
    }

    fn on_key_delete(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('y')
            && let Some(s) = self.selected_session().cloned()
        {
            let _ = self.tx.send(Request::Delete(Box::new(s.clone())));
            self.sessions.retain(|x| x.key() != s.key());
            self.refilter();
            self.set_status(format!("Deleted session {}", s.session_id));
        }
        self.mode = Mode::List;
    }

    fn on_key_kill(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('y')
            && let Some(pid) = self.selected_session().and_then(session_root_pid)
            && let Some(session) = self.selected_session()
        {
            let _ = self.tx.send(Request::Terminate {
                session_key: session.key(),
                pid,
            });
            self.set_status(format!("Stopping session {}…", session.session_id));
        }
        self.mode = Mode::List;
    }

    fn on_key_batch(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('y') && self.mode == Mode::BatchConfirm {
            self.batch_execute();
        }
        self.mode = Mode::List;
    }

    fn on_key_cost(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::List,
            KeyCode::Enter => {
                if let Ok(v) = self.cost_input.parse::<f64>() {
                    self.cost_floor = v.max(0.0);
                    self.refilter();
                    self.save_prefs();
                    self.set_status(if v > 0.0 {
                        format!("Cost floor: ${v:.2}")
                    } else {
                        "Cost floor cleared".into()
                    });
                }
                self.mode = Mode::List;
            }
            KeyCode::Backspace => {
                self.cost_input.pop();
            }
            KeyCode::Char(c) if (c.is_ascii_digit() || c == '.') && self.cost_input.len() < 12 => {
                self.cost_input.push(c);
            }
            _ => {}
        }
    }

    fn on_key_list(&mut self, key: KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Shift+Up/Down scrolls inside the active bottom panel, since the plain
        // arrows are taken by list navigation and panel switching.
        if shift && matches!(key.code, KeyCode::Up | KeyCode::Down) {
            self.scroll_active_panel(if key.code == KeyCode::Up { -1 } else { 1 });
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::F(10) => self.should_quit = true,
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::PageUp | KeyCode::Char('b') => self.move_selection(-PAGE),
            KeyCode::PageDown => self.move_selection(PAGE),
            KeyCode::Char('g') => {
                self.selected = 0;
                self.ensure_available_tab();
                self.needs_redraw = true;
            }
            KeyCode::Char('G') => {
                self.selected = self.visible.len().saturating_sub(1);
                self.ensure_available_tab();
                self.needs_redraw = true;
            }
            KeyCode::Home => {
                self.selected = 0;
                self.ensure_available_tab();
            }
            KeyCode::End => {
                self.selected = self.visible.len().saturating_sub(1);
                self.ensure_available_tab();
            }
            KeyCode::Char('u') if ctrl => {
                self.move_selection(-(self.half_page() as isize));
            }
            KeyCode::Char('d') if ctrl => {
                self.move_selection(self.half_page() as isize);
            }
            KeyCode::Char('+') | KeyCode::Char('=') => self.adjust_refresh(0.5),
            KeyCode::Char('-') | KeyCode::Char('_') => self.adjust_refresh(-0.5),
            KeyCode::Char('f') => {
                self.follow = !self.follow;
                self.set_status(if self.follow {
                    "Follow mode on"
                } else {
                    "Follow mode off"
                });
            }
            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Char('U') => self.unmark_all(),
            KeyCode::Char('D') => self.batch(BatchKind::Delete),
            KeyCode::Char('K') => self.batch(BatchKind::Kill),
            KeyCode::Char('n') => self.cycle_matches(1),
            KeyCode::Char('N') => self.cycle_matches(-1),
            KeyCode::Char('#') => {
                self.cost_input = if self.cost_floor > 0.0 {
                    format!("{:.2}", self.cost_floor)
                } else {
                    String::new()
                };
                self.mode = Mode::CostFilter;
            }
            KeyCode::Char('H') => self.set_sort(ColumnId::Harness),
            KeyCode::Char('X') => self.set_sort(ColumnId::Context),
            KeyCode::Char('S') => self.set_sort(ColumnId::Tools),

            KeyCode::Tab => self.cycle_tab(1),
            KeyCode::BackTab => self.cycle_tab(-1),
            KeyCode::Char(c @ '1'..='7') => {
                let tab = c as usize - '1' as usize;
                if self.tab_available(tab) {
                    self.bottom_tab = tab;
                    self.save_prefs();
                }
            }
            KeyCode::Char('`') => {
                self.live_only = !self.live_only;
                self.refilter();
                self.save_prefs();
            }

            KeyCode::Char('/') | KeyCode::F(3) => self.mode = Mode::Search,
            KeyCode::Char('?') | KeyCode::F(1) => self.mode = Mode::Help,
            KeyCode::Char('>') | KeyCode::Char('<') | KeyCode::F(6) => {
                self.sortby_cursor = COLUMNS
                    .iter()
                    .position(|c| c.id == self.sort_col)
                    .unwrap_or(0);
                self.mode = Mode::SortBy;
            }
            KeyCode::F(7) => {
                self.age_cursor = AGE_OPTIONS
                    .iter()
                    .position(|o| *o == self.age_filter)
                    .unwrap_or(AGE_OPTIONS.len() - 1);
                self.mode = Mode::AgeFilter;
            }
            KeyCode::Char('r') | KeyCode::F(5) => {
                let _ = self.tx.send(Request::Refresh);
                self.set_status("Refreshing…");
            }
            KeyCode::Char('d') => match self.selected_session() {
                Some(s) if s.is_running() => self.mode = Mode::DeleteBlocked,
                Some(_) => self.mode = Mode::DeleteConfirm,
                None => {}
            },
            KeyCode::Char('k') => match self.selected_session() {
                Some(s) if session_root_pid(s).is_some() => self.mode = Mode::KillConfirm,
                Some(s) if s.is_running() => self.mode = Mode::KillBlocked,
                Some(_) => self.set_status("Selected session is not running"),
                None => {}
            },
            KeyCode::Char('y') => self.copy_selection(),
            KeyCode::Char('L') => {
                self.tool_live_only = !self.tool_live_only;
                self.save_prefs();
            }
            KeyCode::Char('v') => {
                self.tool_show_diff = !self.tool_show_diff;
                self.save_prefs();
            }
            // Move through the Tool Activity filter sidebar.
            KeyCode::Char('[') => self.cycle_tool_filter(-1),
            KeyCode::Char(']') => self.cycle_tool_filter(1),

            // htop muscle memory.
            KeyCode::Char('P') => self.set_sort(ColumnId::Status),
            KeyCode::Char('M') => self.set_sort(ColumnId::Memory),
            KeyCode::Char('T') => self.set_sort(ColumnId::Cost),

            // Arrows move between bottom panels; Shift+arrows scroll within one.
            KeyCode::Left => self.cycle_tab(-1),
            KeyCode::Right => self.cycle_tab(1),
            KeyCode::Esc => {
                // Clear the narrowest active filter first.
                if !self.search.is_empty() {
                    self.search.clear();
                    self.refilter();
                } else if self.age_filter.is_some() {
                    self.age_filter = None;
                    self.refilter();
                    self.save_prefs();
                }
            }
            _ => {}
        }
    }

    fn on_mouse(&mut self, ev: event::MouseEvent, layout: &render::Layout) {
        match ev.kind {
            MouseEventKind::ScrollDown => {
                if layout.in_bottom_panel(ev.row) {
                    self.scroll_active_panel(1);
                } else {
                    self.move_selection(1);
                }
            }
            MouseEventKind::ScrollUp => {
                if layout.in_bottom_panel(ev.row) {
                    self.scroll_active_panel(-1);
                } else {
                    self.move_selection(-1);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.bottom_tab == 3
                    && let Some(offset) = layout.tool_log_row_at(ev.column, ev.row)
                {
                    self.toggle_tool_expansion(offset);
                } else if let Some(idx) = layout.tool_sidebar_at(ev.column, ev.row) {
                    self.tool_tab = idx;
                    self.tool_follow = true;
                    self.needs_redraw = true;
                } else if let Some(tab) = layout.tab_at(ev.column, ev.row) {
                    self.bottom_tab = tab;
                    self.save_prefs();
                    self.needs_redraw = true;
                } else if let Some(col) = layout.header_column_at(ev.column, ev.row) {
                    self.set_sort(col);
                } else if let Some(row) = layout.row_at(ev.row) {
                    let idx = self.scroll + row;
                    if idx < self.visible.len() {
                        self.selected = idx;
                        self.ensure_available_tab();
                        self.needs_redraw = true;
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run(args: &Args) -> anyhow::Result<()> {
    let (req_tx, req_rx) = channel::<Request>();
    let (res_tx, res_rx) = channel::<Response>();
    let worker = spawn_worker(args.plan, req_rx, res_tx.clone());

    // Pricing and quota are network-bound; keep both off the UI thread.
    {
        let tx = res_tx.clone();
        std::thread::spawn(move || {
            crate::pricing::refresh_pricing_blocking();
            let _ = tx.send(Response::PricingReady);
        });
    }
    spawn_quota_poller(res_tx);

    let mut app = App::new(args.plan, req_tx.clone());
    app.refresh_secs = args.delay;
    let _ = req_tx.send(Request::Refresh);

    // `ratatui::init` installs a hook that leaves the alt screen and raw mode on
    // panic, but it knows nothing about the mouse capture enabled below. Without
    // this, a panic leaves the terminal emitting mouse escape sequences into the
    // user's shell. Installed after `init` so it runs before ratatui's restore.
    let mut terminal = ratatui::init();
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
        previous_hook(info);
    }));
    let _ = execute!(std::io::stdout(), EnableMouseCapture);

    let result = event_loop(&mut app, &mut terminal, &res_rx, &req_tx);

    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();

    let _ = req_tx.send(Request::Shutdown);
    // The worker persists newly extracted transcript data while shutting down.
    // Joining it matters: returning from main immediately would otherwise kill
    // the detached thread mid-save, forcing every launch to parse all sessions
    // from scratch again.
    let _ = worker.join();
    app.save_prefs();
    result
}

fn spawn_quota_poller(tx: Sender<Response>) {
    std::thread::spawn(move || {
        let mut quota = Quota::default();
        let (mut claude_due, mut codex_due) = (Instant::now(), Instant::now());

        loop {
            let now = Instant::now();
            let mut changed = false;

            // Each provider is paced by its own last outcome: a throttled one
            // backs off without stalling the other.
            if now >= claude_due {
                let status = crate::quota::fetch_claude();
                claude_due =
                    now + Duration::from_secs(status.retry_delay_secs(QUOTA_INTERVAL_SECS));
                quota.claude = status;
                changed = true;
            }
            if now >= codex_due {
                let status = crate::quota::fetch_codex();
                codex_due = now + Duration::from_secs(status.retry_delay_secs(QUOTA_INTERVAL_SECS));
                quota.codex = status;
                changed = true;
            }

            if changed {
                quota.fetched = true;
                if tx.send(Response::Quota(Box::new(quota.clone()))).is_err() {
                    break;
                }
            }
            std::thread::sleep(QUOTA_TICK);
        }
    });
}

fn event_loop(
    app: &mut App,
    terminal: &mut ratatui::DefaultTerminal,
    res_rx: &Receiver<Response>,
    req_tx: &Sender<Request>,
) -> anyhow::Result<()> {
    let mut last_refresh = Instant::now();
    let mut layout = render::Layout::default();
    let mut refresh_in_flight = true;

    loop {
        // Drain everything the workers have produced.
        let mut annotated_rows_changed = false;
        loop {
            match res_rx.try_recv() {
                Ok(Response::Discovered(sessions)) => {
                    app.sessions = sessions;
                    app.stats = crate::loader::compute_stats(&app.sessions);
                    app.refilter();
                }
                Ok(Response::Annotated(session)) => {
                    let key = session.key();
                    if let Some(existing) = app.sessions.iter_mut().find(|s| s.key() == key) {
                        *existing = *session;
                    } else {
                        app.sessions.push(*session);
                    }
                    annotated_rows_changed = true;
                }
                Ok(Response::Sessions(payload)) => {
                    let (sessions, stats) = *payload;
                    app.sessions = sessions;
                    app.stats = stats;
                    app.push_history();
                    app.refilter();
                    refresh_in_flight = false;
                    annotated_rows_changed = false;
                }
                Ok(Response::Data(key, data)) => {
                    // Discard results for a session the user has already left.
                    if key == app.panel_key {
                        app.panel_data = Some(*data);
                        app.needs_redraw = true;
                    }
                }
                Ok(Response::Quota(q)) => {
                    app.quota = *q;
                    app.needs_redraw = true;
                }
                Ok(Response::PricingReady) => {
                    // Cached costs were computed without rates; recompute them.
                    let _ = req_tx.send(Request::Refresh);
                    refresh_in_flight = true;
                }
                Ok(Response::Terminated {
                    session_key,
                    result,
                }) => match result {
                    Ok(()) => {
                        app.set_status("Termination signal sent");
                        let _ = req_tx.send(Request::Refresh);
                        refresh_in_flight = true;
                    }
                    Err(error) => {
                        app.set_status(format!("Could not stop {session_key}: {error}"));
                    }
                },
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        if annotated_rows_changed {
            // A burst can contain hundreds of rows. Recompute and sort once
            // after draining it rather than once per transcript.
            app.stats = crate::loader::compute_stats(&app.sessions);
            app.refilter();
        }

        app.sync_panel_data();

        // Expire the transient status line.
        if let Some((_, at)) = &app.status
            && at.elapsed() > Duration::from_secs(3)
        {
            app.status = None;
            app.needs_redraw = true;
        }

        if app.needs_redraw {
            terminal.draw(|frame| layout = render::draw(frame, app))?;
            app.needs_redraw = false;
        }

        // Wait for input, but never past the next scheduled refresh. The
        // interval is read live so +/- changes apply on the very next poll.
        let refresh_every = Duration::from_secs_f64(app.refresh_secs);
        let wait = refresh_every
            .checked_sub(last_refresh.elapsed())
            .unwrap_or(Duration::ZERO)
            .min(Duration::from_millis(200));
        if event::poll(wait)? {
            match event::read()? {
                Event::Key(key) => app.on_key(key),
                Event::Mouse(m) => app.on_mouse(m, &layout),
                Event::Resize(_, _) => app.needs_redraw = true,
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }

        // Only one refresh in flight: a scan slower than the interval must not
        // queue up behind itself.
        let refresh_every = Duration::from_secs_f64(app.refresh_secs);
        if last_refresh.elapsed() >= refresh_every && !refresh_in_flight {
            last_refresh = Instant::now();
            refresh_in_flight = true;
            let _ = req_tx.send(Request::Refresh);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        let (tx, rx) = channel();
        // Keep the receiver alive so sends in tests don't fail.
        std::mem::forget(rx);
        App::with_prefs(Plan::Retail, tx, UiPrefs::default())
    }

    /// Regression: these tests once read the developer's real prefs file, so a
    /// persisted `live_only` or age filter silently failed unrelated assertions.
    #[test]
    fn test_app_starts_from_default_prefs() {
        let app = test_app();
        assert!(!app.live_only);
        assert!(app.age_filter.is_none());
        assert!(app.search.is_empty());
        assert_eq!(app.sort_col, ColumnId::Last);
    }

    fn session(id: &str, running: bool, label: &str) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.label_source = label.into();
        s.last_active = chrono::Utc::now().to_rfc3339();
        s.started_at = s.last_active.clone();
        if running {
            s.process = Some(crate::proc::ProcInfo::default());
        }
        s
    }

    #[test]
    fn live_filter_hides_stopped_sessions() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "x"), session("b", false, "y")];
        app.live_only = true;
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.sessions[app.visible[0]].session_id, "a");
    }

    #[test]
    fn live_filter_includes_transcript_inferred_cursor_session() {
        let mut app = test_app();
        let mut cursor = Session::new(Provider::Cursor, "cursor".into());
        cursor.started_at = chrono::Utc::now().to_rfc3339();
        cursor.last_active = cursor.started_at.clone();
        cursor.inferred_running = true;
        app.sessions = vec![cursor];
        app.live_only = true;
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert!(app.sessions[app.visible[0]].is_running());
        assert!(app.sessions[app.visible[0]].process.is_none());
    }

    #[test]
    fn session_root_pid_excludes_ghost_and_child_processes() {
        let mut session = session("a", true, "x");
        session.process.as_mut().unwrap().process_list = vec![
            crate::proc::ProcEntry {
                pid: 1,
                is_root: true,
                ghost: true,
                cpu: 0.0,
                memory: 0,
                args: String::new(),
            },
            crate::proc::ProcEntry {
                pid: 2,
                is_root: false,
                ghost: false,
                cpu: 0.0,
                memory: 0,
                args: String::new(),
            },
            crate::proc::ProcEntry {
                pid: 3,
                is_root: true,
                ghost: false,
                cpu: 0.0,
                memory: 0,
                args: String::new(),
            },
        ];

        assert_eq!(session_root_pid(&session), Some(3));
    }

    #[test]
    fn runtime_tabs_are_unavailable_for_stopped_sessions() {
        let mut app = test_app();
        app.sessions = vec![session("stopped", false, "x")];
        app.refilter();
        assert!(!app.tab_available(1));
        assert!(!app.tab_available(2));

        app.bottom_tab = 0;
        app.cycle_tab(1);
        assert_eq!(app.bottom_tab, 3);
    }

    #[test]
    fn selecting_stopped_session_leaves_runtime_tab() {
        let mut app = test_app();
        app.sessions = vec![
            session("running", true, "x"),
            session("stopped", false, "y"),
        ];
        app.visible = vec![0, 1];
        app.bottom_tab = 1;
        app.move_selection(1);
        assert_eq!(app.bottom_tab, 0);
    }

    #[test]
    fn search_matches_label_and_id_case_insensitively() {
        let mut app = test_app();
        app.sessions = vec![
            session("aaa", false, "/home/x/alpha"),
            session("bbb", false, "/home/x/beta"),
        ];
        app.search = "alpha".into();
        app.refilter();
        assert_eq!(app.visible.len(), 1);

        app.search = "BBB".into();
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.sessions[app.visible[0]].session_id, "bbb");
    }

    #[test]
    fn selection_follows_the_session_across_a_resort() {
        let mut app = test_app();
        app.sessions = vec![session("a", false, "/x/a"), session("b", false, "/x/b")];
        app.sort_col = ColumnId::Project;
        app.sort_asc = true;
        app.refilter();
        app.selected = 1;
        let before = app.selected_session().unwrap().key();

        app.sort_asc = false;
        app.refilter();
        assert_eq!(app.selected_session().unwrap().key(), before);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn selection_stays_in_bounds_when_sessions_disappear() {
        let mut app = test_app();
        app.sessions = (0..5)
            .map(|i| session(&i.to_string(), false, "/x"))
            .collect();
        app.refilter();
        app.selected = 4;
        app.sessions.truncate(2);
        app.refilter();
        assert!(app.selected < app.visible.len());
    }

    #[test]
    fn empty_list_does_not_panic_on_navigation() {
        let mut app = test_app();
        app.refilter();
        app.move_selection(1);
        app.move_selection(-1);
        assert_eq!(app.selected, 0);
        assert!(app.selected_session().is_none());
    }

    #[test]
    fn sort_toggles_on_repeat_and_resets_on_change() {
        let mut app = test_app();
        app.sort_col = ColumnId::Cost;
        app.sort_asc = true;
        app.set_sort(ColumnId::Cost);
        assert!(!app.sort_asc, "same column must flip direction");
        app.set_sort(ColumnId::Cpu);
        assert!(app.sort_asc, "new column starts ascending");
        assert_eq!(app.sort_col, ColumnId::Cpu);
    }

    #[test]
    fn age_filter_roundtrips_through_prefs_keys() {
        for f in [AgeFilter::Day, AgeFilter::Week, AgeFilter::Month] {
            assert_eq!(AgeFilter::parse(f.key()), Some(f));
        }
        assert_eq!(AgeFilter::parse("nope"), None);
    }

    #[test]
    fn age_filter_excludes_old_sessions() {
        let mut app = test_app();
        let mut old = session("old", false, "/x");
        old.last_active = (chrono::Utc::now() - chrono::Duration::days(10)).to_rfc3339();
        app.sessions = vec![session("new", false, "/x"), old];
        app.age_filter = Some(AgeFilter::Day);
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.sessions[app.visible[0]].session_id, "new");
    }

    #[test]
    fn cost_floor_filters_by_total_cost() {
        let mut app = test_app();
        let mut cheap = session("cheap", false, "/x");
        cheap.total_cost = Some(0.50);
        let mut pricey = session("pricey", false, "/x");
        pricey.total_cost = Some(5.00);
        app.sessions = vec![cheap, pricey];
        app.cost_floor = 1.0;
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.sessions[app.visible[0]].session_id, "pricey");
    }

    #[test]
    fn cost_floor_zero_shows_everything() {
        let mut app = test_app();
        app.sessions = vec![session("a", false, "/x"), session("b", false, "/x")];
        app.cost_floor = 0.0;
        app.refilter();
        assert_eq!(app.visible.len(), 2);
    }

    #[test]
    fn cycle_matches_wraps_around_search_matches() {
        let mut app = test_app();
        let now = chrono::Utc::now().to_rfc3339();
        let mk = |id: &str, label: &str| {
            let mut s = session(id, false, label);
            s.last_active = now.clone();
            s.started_at = now.clone();
            s
        };
        app.sessions = vec![mk("a", "/x/match"), mk("b", "/y"), mk("c", "/z/match")];
        app.search = "match".into();
        app.refilter();
        assert_eq!(app.visible.len(), 2);
        app.selected = 0;
        app.cycle_matches(1);
        assert_eq!(app.selected_session().unwrap().session_id, "c");
        app.cycle_matches(1);
        assert_eq!(app.selected_session().unwrap().session_id, "a");
        app.cycle_matches(-1);
        assert_eq!(app.selected_session().unwrap().session_id, "c");
    }

    #[test]
    fn mark_toggle_and_batch_delete_remove_marked_sessions() {
        let mut app = test_app();
        app.sessions = vec![
            session("a", false, "/x"),
            session("b", false, "/x"),
            session("c", false, "/x"),
        ];
        app.refilter();
        // Mark a and c.
        app.selected = 0;
        app.toggle_mark();
        app.selected = 2;
        app.toggle_mark();
        assert_eq!(app.marked.len(), 2);
        assert_eq!(app.marked_sessions().len(), 2);

        app.batch(BatchKind::Delete);
        assert_eq!(app.mode, Mode::BatchConfirm);
        app.batch_execute();
        assert_eq!(app.sessions.len(), 1);
        assert_eq!(app.sessions[0].session_id, "b");
        assert!(app.marked.is_empty());
    }

    #[test]
    fn batch_delete_refuses_when_marked_session_is_running() {
        let mut app = test_app();
        app.sessions = vec![session("a", false, "/x"), session("b", true, "/x")];
        app.refilter();
        app.selected = 0;
        app.toggle_mark();
        app.selected = 1;
        app.toggle_mark();
        app.batch(BatchKind::Delete);
        assert_eq!(app.mode, Mode::BatchDeleteBlocked);
    }

    #[test]
    fn refresh_interval_adjusts_and_clamps() {
        let mut app = test_app();
        app.refresh_secs = 2.0;
        app.adjust_refresh(0.5);
        assert_eq!(app.refresh_secs, 2.5);
        app.adjust_refresh(-10.0);
        assert_eq!(app.refresh_secs, 0.5);
        app.adjust_refresh(100.0);
        assert_eq!(app.refresh_secs, 60.0);
    }
}
