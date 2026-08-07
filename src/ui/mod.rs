//! Terminal UI: application state, the worker thread, and the event loop.

pub mod columns;
mod input;
mod modals;
pub mod panels;
pub mod render;
pub mod spark;
mod table;
pub mod tabs;
pub mod theme;

use crate::cache::UiPrefs;
use crate::cli::Args;
use crate::loader::{Loader, Stats};
use crate::pricing::{Plan, Provider};
use crate::quota::Quota;
use crate::session::{Session, SessionData};
use columns::ColumnId;
use ratatui::crossterm::cursor::Show;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
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

/// Gap between full directory walks.
///
/// Only a walk can notice a session that didn't exist before, and it costs one
/// `stat` per transcript ever recorded — thousands of them, nearly all belonging
/// to sessions that ended long ago. A filesystem watch reports creations and
/// removals as they happen, so this is the safety net for whatever the watch
/// misses (or for when no watch could be established at all) rather than the way
/// new sessions are normally found. `r` still forces one immediately.
const FULL_WALK_INTERVAL: Duration = Duration::from_secs(60);

/// Gap between walks while a created file has yet to become a session.
///
/// Short, because this is the window in which a session the user just started is
/// missing from the table; bounded, because the walk is the expensive one and a
/// file may sit there for a while before the model first answers.
const PENDING_WALK_INTERVAL: Duration = Duration::from_secs(3);

/// How long typing has to pause before the query is scanned for.
///
/// A scan reads every transcript on disk, so it waits for a word rather than
/// chasing each character of one. Short enough that finishing a word and
/// looking up finds the results already there.
const SCAN_DEBOUNCE: Duration = Duration::from_millis(300);

/// Shortest query worth reading every transcript for.
const MIN_SCAN_CHARS: usize = 3;

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
    /// Confirming that a session already running elsewhere should be resumed
    /// anyway, which puts a second agent on the same transcript.
    ResumeConfirm,
    /// Confirming a quit that would take the hosted agent down with it.
    QuitConfirm,
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
    /// Text input typed into the selected session's tmux pane.
    SendKeys,
    /// Picking which agent a new tab or split should run.
    Launch,
    /// The agent-integration panel: what is installed where, and whether the
    /// agents are actually reporting in.
    Hooks,
}

/// Where the agent picked in `Mode::Launch` ends up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchInto {
    /// A tab of its own.
    Tab,
    /// Alongside the panes already in the current tab, arranged the given way.
    Split { stacked: bool },
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
    /// Update the running sessions without re-walking every provider directory.
    RefreshLive,
    /// Extract full data for one session, to populate the bottom panels.
    Data(Box<Session>),
    Delete(Box<Session>),
    Terminate {
        session_key: String,
        pid: u32,
    },
    /// Type a line into the terminal hosting a live session.
    SendKeys {
        pid: u32,
        text: String,
    },
    /// Look for `query` inside every listed session's transcript.
    Scan {
        query: String,
        targets: Vec<crate::session::search::Target>,
    },
    Shutdown,
}

enum Response {
    /// Cheap discovery result, shown before transcript extraction completes.
    Discovered(Vec<Session>),
    /// One row whose transcript has finished loading.
    Annotated(Box<Session>),
    Sessions(Box<(Vec<Session>, Stats)>),
    /// Only the rows that moved during a light refresh, plus recomputed totals.
    /// Shipping these instead of the whole table is the point of the light path:
    /// copying thousands of rows back every couple of seconds is the cost being
    /// avoided.
    LiveRows(Box<(Vec<Session>, Stats)>),
    Data(String, Box<SessionData>),
    Quota(Box<Quota>),
    /// Pricing landed, so cached costs are stale and a reload is due.
    PricingReady,
    /// A newer release exists. Reported once; cctop never updates itself.
    UpdateAvailable(String),
    Terminated {
        session_key: String,
        result: Result<(), String>,
    },
    Deleted {
        session_key: String,
        result: Result<(), String>,
    },
    KeysSent {
        result: Result<(), String>,
    },
    /// A finished transcript scan: session key -> the text around its match.
    /// The query comes back with it, because the user has usually typed more by
    /// the time a scan over thousands of transcripts lands.
    Scanned {
        query: String,
        hits: HashMap<String, String>,
    },
}

/// Everything about a session the `/` filter can match without reading a
/// transcript, lowercased.
///
/// The columns on screen, plus the two things a row shows only part of: the
/// full working directory, because the table abbreviates it to fit and
/// `~/src/work/api` is what someone types; and the branch, which is a column
/// but is read from disk rather than carried on the row.
fn search_haystack(s: &Session) -> String {
    format!(
        "{} {} {} {} {} {} {}",
        s.display_label(),
        s.model,
        s.harness,
        s.provider.as_str(),
        s.session_id,
        s.label_source,
        columns::branch_of(s).unwrap_or_default(),
    )
    .to_ascii_lowercase()
}

/// Remembered scan results, keyed by session and query. `None` is a remembered
/// *miss*, which is the answer worth caching most: a miss costs a full read of
/// the transcript, a hit usually stops early.
type ScanCache = HashMap<(String, String), Option<String>>;

/// Entries kept before the scan cache is dropped wholesale.
///
/// Reached only by someone who has run many distinct queries over many
/// sessions; forgetting everything then costs one re-scan rather than the
/// bookkeeping an eviction policy would need for a cache this cheap to refill.
const MAX_SCAN_CACHE: usize = 20_000;

/// Search every target's transcript for `needle`, in parallel.
///
/// Running sessions are never cached: their transcripts grow, so today's "not
/// found" is not tomorrow's, and the one case where a stale answer is most
/// visible is the session the user is watching right now.
fn scan(
    cache: &mut ScanCache,
    targets: &[crate::session::search::Target],
    needle: &str,
) -> HashMap<String, String> {
    use rayon::prelude::*;
    let found: Vec<(&crate::session::search::Target, Option<String>)> = targets
        .par_iter()
        .map(|target| {
            let memo = (!target.running)
                .then(|| cache.get(&(target.key.clone(), needle.to_string())))
                .flatten();
            match memo {
                Some(remembered) => (target, remembered.clone()),
                None => (
                    target,
                    crate::session::search::find(target, needle).map(|hit| hit.snippet),
                ),
            }
        })
        .collect();

    if cache.len() + found.len() > MAX_SCAN_CACHE {
        cache.clear();
    }
    let mut hits = HashMap::new();
    for (target, snippet) in found {
        if !target.running {
            cache.insert((target.key.clone(), needle.to_string()), snippet.clone());
        }
        if let Some(snippet) = snippet {
            hits.insert(target.key.clone(), snippet);
        }
    }
    hits
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
        // The last full walk's rows, kept so a light refresh has something to
        // update in place.
        let mut live_rows: Vec<Session> = Vec::new();
        // What earlier transcript scans found, so refining a query re-reads only
        // what it has to. See `scan`.
        let mut scans: ScanCache = HashMap::new();
        while let Ok(req) = rx.recv() {
            match req {
                Request::Refresh => {
                    // Row-by-row publishing exists so the first table isn't
                    // withheld for the slowest transcript. Later refreshes end
                    // with a `Sessions` payload that replaces everything anyway,
                    // so streaming them too only buys a redundant repaint — at the
                    // cost of one message and one table scan per session, per
                    // refresh, forever.
                    let first_load = !sent_initial_discovery;
                    sent_initial_discovery = true;
                    let sessions = loader.load_progressive(
                        plan,
                        // Only the first table has someone waiting for it.
                        first_load,
                        |sessions| {
                            if first_load {
                                let _ = tx.send(Response::Discovered(sessions.to_vec()));
                            }
                        },
                        |session| {
                            if first_load {
                                let _ = tx.send(Response::Annotated(Box::new(session.clone())));
                            }
                        },
                    );
                    let stats = crate::loader::compute_stats(&sessions);
                    // The light path needs its own copy to carry forward; one clone
                    // per full walk replaces one per refresh.
                    live_rows = sessions.clone();
                    if tx
                        .send(Response::Sessions(Box::new((sessions, stats))))
                        .is_err()
                    {
                        break;
                    }
                }
                Request::RefreshLive => {
                    if live_rows.is_empty() {
                        // Nothing walked yet, so there is nothing to update.
                        continue;
                    }
                    let moved = loader.refresh_live(plan, &mut live_rows);
                    let stats = crate::loader::compute_stats(&live_rows);
                    if tx
                        .send(Response::LiveRows(Box::new((moved, stats))))
                        .is_err()
                    {
                        break;
                    }
                }
                Request::Data(session) => {
                    // The open panels are the one view where staleness shows, so
                    // this path never accepts a backed-off entry.
                    let data = loader.store().session_data_fresh(&session);
                    if tx
                        .send(Response::Data(session.key(), Box::new(data)))
                        .is_err()
                    {
                        break;
                    }
                }
                Request::Delete(session) => {
                    let result = match session.provider {
                        Provider::Claude => crate::session::claude::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Codex => crate::session::codex::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Cursor => crate::session::cursor::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Gemini => crate::session::gemini::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::OpenCode => crate::session::opencode::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Pi => {
                            crate::session::pi::delete(&session).map_err(|error| error.to_string())
                        }
                        Provider::Windsurf => crate::session::windsurf::delete(&session)
                            .map_err(|error| error.to_string()),
                    };
                    if result.is_ok() {
                        loader.store().evict(&session);
                    }
                    if tx
                        .send(Response::Deleted {
                            session_key: session.key(),
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
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
                Request::SendKeys { pid, text } => {
                    let result = crate::inject::send_line(pid, &text);
                    if tx.send(Response::KeysSent { result }).is_err() {
                        break;
                    }
                }
                Request::Scan { query, targets } => {
                    let needle = query.to_ascii_lowercase();
                    let hits = loader.gently(|| scan(&mut scans, &targets, &needle));
                    if tx.send(Response::Scanned { query, hits }).is_err() {
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
    /// Search the transcripts as well as the columns.
    ///
    /// Off by default, and deliberately: the metadata filter answers instantly
    /// from memory, while this one reads every transcript on disk. Turning it on
    /// is how you say the reading is worth it.
    pub search_content: bool,
    /// The query [`App::scan_hits`] belongs to.
    ///
    /// Kept because a scan over thousands of transcripts outlives the keystroke
    /// that started it: hits for "flyw" must not be applied to "flywheel".
    pub scan_query: String,
    /// Session key -> the transcript text around its match.
    pub scan_hits: HashMap<String, String>,
    /// A scan is out with the worker.
    pub scanning: bool,
    /// When the query last changed, so a burst of typing costs one scan.
    scan_typed_at: Option<Instant>,
    /// Queries run before, newest first, walked with ↑/↓ in the filter modal.
    pub search_history: Vec<String>,
    /// Where ↑/↓ has walked to in `search_history`, and the query that was
    /// being typed before the walk started, so ↓ can put it back.
    history_cursor: Option<(usize, String)>,
    pub age_filter: Option<AgeFilter>,
    pub age_cursor: usize,
    pub live_only: bool,

    /// Session keys the user has marked (Space) for a batch action.
    pub marked: HashSet<String>,
    /// Sessions whose deletion has been accepted by the worker but not yet
    /// completed. They remain visible until the provider reports success.
    pub deleting: HashSet<String>,
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
    /// Line being typed into the selected session's terminal.
    pub send_input: String,
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
    pub context_scroll: u16,
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
    /// Version of a newer published release, when one exists.
    pub update_available: Option<String>,
    pub status: Option<(String, Instant)>,
    /// When cctop started, used by the tool-activity "live" filter.
    pub started_at: String,
    /// The same moment as an `Instant`, which is what the tab-bar blink is
    /// phased against — a wall clock can jump, and a blink that stutters when
    /// NTP steps the clock looks like a bug.
    started: Instant,

    /// Bell and desktop notifications, and who rang last.
    pub notify: crate::notify::Notifier,

    /// Workspace tabs beyond the dashboard, each holding one or more terminals.
    pub tabs: Vec<tabs::Tab>,
    /// Which tab is on screen: `0` is the dashboard, `1..=tabs.len()` index
    /// `tabs`. Zero-length `tabs` is the ordinary case and costs nothing — no
    /// tab bar is drawn and cctop looks exactly as it did before.
    pub tab: usize,
    /// What each session's own hooks last said about it, keyed by session id.
    ///
    /// Only sessions whose agent has cctop's hooks installed appear here, so an
    /// absent entry is the ordinary case and means "fall back to the transcript"
    /// rather than "nothing is happening".
    pub hooked: HashMap<String, crate::hook::Reported>,
    /// The integration's state, as of the last time the panel was opened.
    ///
    /// Rebuilt on opening and after every action rather than every frame: it
    /// reads three files off disk and scans a directory, which is nothing to do
    /// once and wasteful to do sixty times a second behind a closed panel.
    pub hooks: Option<crate::hook::Report>,
    /// The socket the agents push their events to. `None` only where there is
    /// none to be had — a non-unix build — in which case every estimate carries
    /// on exactly as it did before hooks existed.
    pub listener: Option<crate::hook::Listener>,
    /// The command highlighted in the launcher.
    pub launch_cursor: usize,
    /// Where the launcher's pick will go.
    pub launch_into: LaunchInto,
    /// Directory a launched agent starts in — the selected session's project,
    /// captured when the launcher opens because the selection can move under it.
    pub launch_cwd: Option<std::path::PathBuf>,
    /// The agent this cctop launched, as `(pid, label)`.
    ///
    /// Set only for `cctop <agent>`, and only while it is alive — the loop exits
    /// as soon as it is not. It is what `A` goes back to after F12, and the
    /// reason quitting asks first: the agent is on a pty this process owns and
    /// does not survive it.
    pub hosted: Option<(u32, String)>,

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
            // Newest first: `Last` compares reversed, so ascending is most
            // recently active at the top.
            sort_col: ColumnId::Last,
            sort_asc: true,
            sortby_cursor: 0,
            search: String::new(),
            search_content: false,
            scan_query: String::new(),
            scan_hits: HashMap::new(),
            scanning: false,
            scan_typed_at: None,
            search_history: prefs.search_history.clone(),
            history_cursor: None,
            age_filter,
            age_cursor,
            live_only: prefs.live_only,
            marked: HashSet::new(),
            deleting: HashSet::new(),
            batch: BatchKind::Delete,
            follow: false,
            refresh_secs: 2.0,
            cost_floor: prefs.cost_floor,
            cost_input: String::new(),
            send_input: String::new(),
            list_height: 0,
            bottom_tab: prefs.bottom_tab.min(panels::TABS.len() - 1),
            panel_data: None,
            panel_key: String::new(),
            panel_stamp: String::new(),
            info_scroll: 0,
            cost_scroll: 0,
            config_scroll: 0,
            proc_scroll: 0,
            context_scroll: 0,
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
            notify: crate::notify::Notifier::new(prefs.notify),
            update_available: None,
            status: None,
            started_at: chrono::Utc::now().to_rfc3339(),
            started: Instant::now(),
            prefs,
            tx,
            tabs: Vec::new(),
            tab: 0,
            hooked: HashMap::new(),
            hooks: None,
            listener: None,
            launch_cursor: 0,
            launch_into: LaunchInto::Tab,
            launch_cwd: None,
            hosted: None,
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
        self.prefs.inactivity_filter = self.age_filter.map(|a| a.key().to_string());
        self.prefs.agent_live_filter = self.tool_live_only;
        self.prefs.tool_show_diff = self.tool_show_diff;
        self.prefs.subagent_sort_col = self.subagent_sort.0.key().to_string();
        self.prefs.subagent_sort_asc = self.subagent_sort.1;
        self.prefs.cost_floor = self.cost_floor;
        self.prefs.notify = self.notify.enabled;
        self.prefs.search_history = self.search_history.clone();
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
                if !self.matches_query(s, &query) {
                    return false;
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
        match tab {
            // Performance and Processes read a live process tree.
            1 | 2 => self.selected_session().is_some_and(Session::is_running),
            // Only Claude transcripts report the per-request usage the context
            // breakdown is reconstructed from. Gated on the provider rather than
            // on the extracted data, so the tab doesn't vanish while it loads.
            7 => self
                .selected_session()
                .is_some_and(|s| s.provider == Provider::Claude),
            _ => true,
        }
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
            6 => bump(&mut self.config_scroll),
            _ => bump(&mut self.context_scroll),
        }
        self.needs_redraw = true;
    }

    /// Copy something useful about the selection to the clipboard.
    fn copy_selection(&mut self) {
        let Some(s) = self.selected_session() else {
            return;
        };
        let text = match self.bottom_tab {
            // From the Info tab, the resume command is the most useful thing —
            // and for the providers that have none, the transcript's path is.
            0 => match s.resume_argv() {
                Some(argv) => argv.join(" "),
                None => s
                    .data_file
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| s.session_id.clone()),
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
        self.matches_query(s, &self.search.to_ascii_lowercase())
    }

    /// Whether a session matches `query`, which must already be lowercase.
    ///
    /// Content search widens the filter rather than replacing it: a query that
    /// names a project still finds that project's sessions, and the transcripts
    /// add whatever else mentions it. Hits only count while they belong to the
    /// query being typed — until the scan for a longer query lands, its rows are
    /// the metadata matches alone, which is a filter narrowing as you type
    /// rather than showing results for a query you have moved on from.
    fn matches_query(&self, s: &Session, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        if search_haystack(s).contains(query) {
            return true;
        }
        self.search_content && self.scan_query == query && self.scan_hits.contains_key(&s.key())
    }

    /// The transcript text around the selected session's content match.
    pub fn selected_snippet(&self) -> Option<&str> {
        let s = self.selected_session()?;
        (self.search_content && self.scan_query == self.search.to_ascii_lowercase())
            .then(|| self.scan_hits.get(&s.key()))
            .flatten()
            .map(String::as_str)
    }

    /// Note that the query changed, so the scan can be rescheduled.
    ///
    /// Every edit lands here, including the ones that only shorten the query:
    /// hits for a longer query are not hits for a shorter one, and leaving them
    /// applied would leave rows on screen that no longer match anything.
    pub(super) fn search_edited(&mut self) {
        self.history_cursor = None;
        self.scan_typed_at = Some(Instant::now());
        self.refilter();
    }

    /// Turn transcript searching on or off.
    pub(super) fn toggle_content_search(&mut self) {
        self.search_content = !self.search_content;
        if !self.search_content {
            // Results for a search nobody is running any more; keeping them
            // would make the next toggle show stale rows for an instant.
            self.scan_hits.clear();
            self.scan_query.clear();
        }
        self.scan_typed_at = Some(Instant::now());
        self.refilter();
    }

    /// Send the current query off to be scanned, once the typing has settled.
    ///
    /// Called every loop iteration rather than on each keystroke: a scan reads
    /// every transcript on disk, and firing one per character would spend the
    /// whole budget on prefixes of the word being typed.
    pub(super) fn tick_scan(&mut self) {
        if !self.search_content || self.scanning {
            return;
        }
        let query = self.search.to_ascii_lowercase();
        if query == self.scan_query {
            self.scan_typed_at = None;
            return;
        }
        // A one- or two-character query matches nearly every transcript, so it
        // is the most expensive scan to run and the least useful to read.
        // Deleting back to that length drops the results with it, rather than
        // leaving a count on screen for a query no longer being asked.
        if query.chars().count() < MIN_SCAN_CHARS {
            if !self.scan_query.is_empty() {
                self.scan_query.clear();
                self.scan_hits.clear();
                self.refilter();
                self.needs_redraw = true;
            }
            return;
        }
        match self.scan_typed_at {
            Some(at) if at.elapsed() < SCAN_DEBOUNCE => return,
            _ => {}
        }
        self.scan_typed_at = None;
        let targets: Vec<crate::session::search::Target> = self
            .sessions
            .iter()
            .map(crate::session::search::Target::of)
            .collect();
        if self.tx.send(Request::Scan { query, targets }).is_ok() {
            self.scanning = true;
            self.needs_redraw = true;
        }
    }

    /// Fold in a finished scan.
    fn scanned(&mut self, query: String, hits: HashMap<String, String>) {
        self.scanning = false;
        self.scan_query = query;
        self.scan_hits = hits;
        self.refilter();
        self.needs_redraw = true;
    }

    /// Record the query that was just run, so ↑ can bring it back.
    pub(super) fn remember_query(&mut self) {
        let query = self.search.trim().to_string();
        if query.is_empty() {
            return;
        }
        // Re-running a query moves it to the front rather than adding a second
        // copy, which is what makes a short history worth walking.
        self.search_history.retain(|q| q != &query);
        self.search_history.insert(0, query);
        self.search_history
            .truncate(crate::cache::MAX_SEARCH_HISTORY);
        self.save_prefs();
    }

    /// Walk the query history: `1` towards older entries, `-1` back towards
    /// what was being typed when the walk started.
    pub(super) fn history_step(&mut self, delta: isize) {
        if self.search_history.is_empty() {
            return;
        }
        let (at, typed) = match self.history_cursor.take() {
            Some((at, typed)) => (at as isize + delta, typed),
            // Nothing walked yet: ↓ has nowhere older to come back from.
            None if delta < 0 => return,
            None => (0, self.search.clone()),
        };
        // Stepping back past the newest entry restores the partial query, which
        // is the one thing the history itself cannot hold.
        if at < 0 {
            self.search = typed;
        } else {
            let at = (at as usize).min(self.search_history.len() - 1);
            self.search = self.search_history[at].clone();
            self.history_cursor = Some((at, typed));
        }
        self.scan_typed_at = Some(Instant::now());
        self.refilter();
        self.needs_redraw = true;
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
        let marked: Vec<Session> = self.marked_sessions().into_iter().cloned().collect();
        let mut requested = 0;
        let mut acted_on: Vec<String> = Vec::new();
        let mut failed = 0;
        for s in &marked {
            let key = s.key();
            match kind {
                BatchKind::Delete => {
                    if self.tx.send(Request::Delete(Box::new(s.clone()))).is_ok() {
                        self.deleting.insert(key.clone());
                        requested += 1;
                        acted_on.push(key);
                    } else {
                        failed += 1;
                    }
                }
                BatchKind::Kill => match session_root_pid(s) {
                    Some(pid) => {
                        self.tx
                            .send(Request::Terminate {
                                session_key: key.clone(),
                                pid,
                            })
                            .ok();
                        acted_on.push(key);
                    }
                    None => failed += 1,
                },
            }
        }
        for key in &acted_on {
            self.marked.remove(key);
        }
        self.set_status(match kind {
            BatchKind::Delete => {
                if failed == 0 {
                    format!("Deleting {requested} session(s)…")
                } else {
                    format!("Deleting {requested} session(s), {failed} failed to start")
                }
            }
            BatchKind::Kill => format!(
                "Kill sent to {} session(s){}",
                acted_on.len(),
                if failed > 0 {
                    format!(" ({} skipped)", failed)
                } else {
                    String::new()
                }
            ),
        });
    }

    /// Let the notifier see this refresh, and ring if anything crossed.
    ///
    /// Called from the event loop only when the rows actually moved. It has to
    /// run on this thread: the bell and the OSC 9 sequence go straight to
    /// stdout, which ratatui owns, and only here is it certain that no frame is
    /// halfway through being flushed.
    fn check_bells(&mut self) {
        self.notify.observe(&self.sessions);
    }

    /// Turn the bell on or off, and remember which.
    fn toggle_notifications(&mut self) {
        self.notify.enabled = !self.notify.enabled;
        self.save_prefs();
        self.set_status(if self.notify.enabled {
            "Notifications on — bell and desktop alert when a session needs you"
        } else {
            "Notifications off"
        });
    }

    /// Jump the selection to whichever session rang last.
    fn jump_to_bell(&mut self) {
        let Some(key) = self.notify.last.as_ref().map(|r| r.key.clone()) else {
            self.set_status("Nothing has rung yet");
            return;
        };
        match self
            .visible
            .iter()
            .position(|&i| self.sessions[i].key() == key)
        {
            Some(row) => {
                self.selected = row;
                self.ensure_available_tab();
                self.needs_redraw = true;
            }
            // Answering it is the point, so say why it can't be reached rather
            // than moving the cursor somewhere arbitrary.
            None => self.set_status("The session that rang is hidden by the current filter"),
        }
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

/// Half-period of the tab-bar blink. Slow enough to read the title through,
/// fast enough to catch the eye.
const BLINK_MS: u128 = 600;

// ---------------------------------------------------------------------------
// Workspace tabs
// ---------------------------------------------------------------------------

impl App {
    /// The tab on screen, or `None` on the dashboard.
    pub fn active_tab(&mut self) -> Option<&mut tabs::Tab> {
        self.tabs.get_mut(self.tab.checked_sub(1)?)
    }

    /// The pane the keyboard belongs to, or `None` on the dashboard.
    pub fn focused_pane(&mut self) -> Option<&mut tabs::Pane> {
        self.active_tab()?.focused_mut()
    }

    /// What tab `index` wants, if anything. `0` is the dashboard, which never
    /// asks for itself.
    ///
    /// The tab you are already looking at is excluded — its own focused pane is
    /// in front of you, so blinking its title tells you nothing you cannot see.
    pub fn tab_attention(&self, index: usize) -> Option<tabs::Attention> {
        let tab = self.tabs.get(index.checked_sub(1)?)?;
        tab.attention(index == self.tab, &|pid| self.pane_signal(pid))
    }

    /// Fold in whatever the agents have reported.
    ///
    /// These outrank anything read off disk or off a screen: an agent saying
    /// "my turn is over" is the fact those are both estimating.
    ///
    /// Returns whether anything arrived, and whether the set of sessions itself
    /// changed — one that has just started or just ended is a row to go and
    /// find or forget now, rather than at the next poll.
    fn apply_hooks(&mut self, events: Vec<crate::hook::Event>) -> (bool, bool) {
        let changed = !events.is_empty();
        let mut lifecycle = false;
        for event in events {
            lifecycle |= event.reported.signal.is_lifecycle();
            match event.reported.signal {
                // Nothing more will be said about it, and leaving the last
                // signal behind would have the row claim a state forever.
                crate::hook::Signal::Ended => {
                    self.hooked.remove(&event.session_id);
                }
                _ => {
                    self.hooked.insert(event.session_id, event.reported);
                }
            }
        }
        (changed, lifecycle)
    }

    /// Open the integration panel, reading the current state off disk.
    pub fn open_hooks(&mut self) {
        self.hooks = Some(self.hook_status());
        self.mode = Mode::Hooks;
        self.needs_redraw = true;
    }

    /// The integration's state, scoped to whichever project the cursor is on.
    fn hook_status(&self) -> crate::hook::Report {
        crate::hook::status(self.hook_project().as_deref(), self.listener.as_ref())
    }

    /// The project a `project`-scoped install would write into: the directory
    /// of the selected session, when it has one on this machine.
    pub fn hook_project(&self) -> Option<std::path::PathBuf> {
        self.selected_session()
            .map(|s| std::path::PathBuf::from(&s.label_source))
            .filter(|dir| dir.is_dir())
    }

    /// Install or remove from the panel, and show what happened.
    pub fn set_hooks(&mut self, scope: crate::hook::Scope, install: bool) {
        let result = match install {
            true => crate::hook::install(&scope),
            false => crate::hook::remove(&scope),
        };
        // Codex is configured machine-wide or not at all, so it follows the user
        // scope only. Its outcome is folded into the panel below either way, so
        // a failure here needs no message of its own.
        if scope == crate::hook::Scope::User {
            let _ = match install {
                true => crate::hook::codex_install(),
                false => crate::hook::codex_remove(),
            };
        }
        match result {
            Ok(what) => self.set_status(&what),
            Err(e) => self.set_status(e.to_string()),
        }
        self.hooks = Some(self.hook_status());
    }

    /// What the agents have actually said, newest state per session, as
    /// `(project, state)` pairs for the panel.
    ///
    /// The project rather than the session id: an id names nothing to a reader,
    /// and this list is the answer to "is the thing I just installed working",
    /// which needs a name you recognise.
    pub fn reporting(&self) -> Vec<(String, &'static str)> {
        let mut rows: Vec<(String, &'static str)> = self
            .hooked
            .values()
            .map(|r| {
                let name = std::path::Path::new(&r.cwd)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "—".into());
                (name, r.signal.label())
            })
            .collect();
        rows.sort();
        rows
    }

    /// What a session's own hooks last said about it, if it has any.
    fn hooked_signal(&self, session_id: &str) -> Option<crate::hook::Signal> {
        self.hooked.get(session_id).map(|r| r.signal)
    }

    /// What has been reported about the agent running as `pid`, if anything.
    ///
    /// Hooks first, because the agent said it outright; the transcript second,
    /// which can only report the question and not the finished turn; nothing at
    /// all if the session has not been discovered yet, which leaves the caller
    /// to fall back to the pane's screen.
    ///
    /// Scans the table rather than keeping an index: there are a handful of
    /// panes and this runs once per frame, so a map would be state to keep
    /// correct in exchange for nothing measurable.
    fn pane_signal(&self, pid: u32) -> Option<crate::hook::Signal> {
        self.sessions
            .iter()
            .filter(|session| session_root_pid(session) == Some(pid))
            .find_map(|session| {
                self.hooked_signal(&session.session_id).or({
                    match session.activity_state {
                        crate::session::ActivityState::WaitingForInput => {
                            Some(crate::hook::Signal::NeedsInput)
                        }
                        _ => None,
                    }
                })
            })
    }

    /// Whether any tab is currently asking to be looked at.
    pub fn any_attention(&self) -> bool {
        (1..=self.tabs.len()).any(|i| self.tab_attention(i).is_some())
    }

    /// Which half of the blink cycle we are in.
    pub fn blink_on(&self) -> bool {
        (self.started.elapsed().as_millis() / BLINK_MS).is_multiple_of(2)
    }

    /// Show `tab`, clamped to what exists.
    pub fn show_tab(&mut self, tab: usize) {
        self.tab = tab.min(self.tabs.len());
        self.needs_redraw = true;
    }

    /// Move `delta` tabs along, wrapping through the dashboard.
    pub fn cycle_workspace(&mut self, delta: isize) {
        let count = self.tabs.len() as isize + 1;
        self.tab = (self.tab as isize + delta).rem_euclid(count) as usize;
        self.needs_redraw = true;
    }

    /// Open the launcher, remembering where the pick should go and which
    /// directory it should start in.
    pub fn launch_prompt(&mut self, into: LaunchInto) {
        if matches!(into, LaunchInto::Split { .. }) && self.active_tab().is_none() {
            self.set_status("Nothing to split — open a tab first");
            return;
        }
        if tabs::harnesses().is_empty() {
            self.set_status("No agent found in PATH, and $SHELL is not set");
            return;
        }
        self.launch_into = into;
        self.launch_cursor = 0;
        // A split lands next to an agent already working somewhere; a new tab
        // takes its cue from whatever row the cursor is on.
        self.launch_cwd = match into {
            LaunchInto::Split { .. } => self.launch_cwd.clone(),
            LaunchInto::Tab => self
                .selected_session()
                .map(|s| std::path::PathBuf::from(&s.label_source))
                .filter(|dir| dir.is_dir()),
        };
        self.mode = Mode::Launch;
    }

    /// Reopen the selected session in a tab of its own.
    ///
    /// This is the one way into a session cctop did not start. `a` shows an
    /// agent's live terminal, but only for the agents cctop hosts — there is no
    /// pty to borrow otherwise. Resuming instead starts a *new* agent and hands
    /// it the transcript, which is what the harnesses themselves offer and works
    /// whether the session ended an hour ago or is running in another window.
    pub(super) fn resume_selected(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let Some(argv) = session.resume_argv() else {
            self.set_status(format!(
                "{} sessions cannot be resumed from a shell",
                session.provider.as_str()
            ));
            return;
        };
        if !crate::shim::is_command(&argv[0]) {
            self.set_status(format!("{} is not installed on this machine", argv[0]));
            return;
        }
        // Two agents appending to one transcript is not something any of the
        // harnesses coordinate, so the running case asks first.
        if session.is_running() {
            self.mode = Mode::ResumeConfirm;
            return;
        }
        self.resume_now();
    }

    /// Resume the selected session, having decided that it should be.
    pub(super) fn resume_now(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let Some(argv) = session.resume_argv() else {
            return;
        };
        // The transcript is full of paths relative to where the agent ran, so a
        // resumed session belongs in the same directory.
        let cwd = session.work_dir();
        let name = format!("{} · {}", session.display_label(), argv[0]);
        self.open_tab(&argv, cwd, &name);
    }

    /// Start `argv` in a new tab, reporting what happened either way.
    fn open_tab(&mut self, argv: &[String], cwd: Option<std::path::PathBuf>, what: &str) {
        let pane = match tabs::Pane::launch(argv, cwd.as_deref()) {
            Ok(pane) => pane,
            Err(error) => {
                self.set_status(format!("Could not start {what}: {error}"));
                return;
            }
        };
        self.tabs.push(tabs::Tab::new(pane));
        self.tab = self.tabs.len();
        let where_ = cwd
            .map(|dir| format!(" in {}", crate::util::tildify(&dir.to_string_lossy())))
            .unwrap_or_default();
        self.set_status(format!("Resumed {what}{where_}"));
    }

    /// Start the launcher's pick.
    pub fn launch_selected(&mut self) {
        let commands = tabs::harnesses();
        let Some(argv) = commands.get(self.launch_cursor) else {
            return;
        };
        let cwd = self.launch_cwd.clone();
        let pane = match tabs::Pane::launch(argv, cwd.as_deref()) {
            Ok(pane) => pane,
            Err(error) => {
                self.set_status(format!("Could not start {}: {error}", tabs::label_of(argv)));
                return;
            }
        };
        let label = pane.label.clone();
        match self.launch_into {
            LaunchInto::Split { stacked } => {
                let Some(tab) = self.active_tab() else { return };
                tab.stacked = stacked;
                tab.panes.push(pane);
                tab.focus = tab.panes.len() - 1;
            }
            LaunchInto::Tab => {
                self.tabs.push(tabs::Tab::new(pane));
                self.tab = self.tabs.len();
            }
        }
        let where_ = cwd
            .map(|dir| format!(" in {}", dir.display()))
            .unwrap_or_default();
        self.set_status(format!("Started {label}{where_}"));
    }

    /// Close the focused pane, taking the agent with it when cctop started it.
    pub fn close_pane(&mut self) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        if tab.focus < tab.panes.len() {
            tab.panes.remove(tab.focus);
        }
        tab.focus = tab.focus.min(tab.panes.len().saturating_sub(1));
        self.drop_empty_tabs();
    }

    /// Forget the tabs whose agents have all exited, keeping the view on
    /// something that still exists.
    pub fn drop_empty_tabs(&mut self) {
        let mut index = 0;
        self.tabs.retain(|tab| {
            index += 1;
            if !tab.panes.is_empty() {
                return true;
            }
            // The tabs after this one shift down by one, so a view sitting on
            // any of them has to follow — otherwise closing tab 1 silently
            // moves you to what used to be tab 2.
            if self.tab >= index {
                self.tab -= 1;
            }
            false
        });
        self.tab = self.tab.min(self.tabs.len());
    }

    /// Put the selected agent's own terminal on screen, in a tab of its own.
    ///
    /// Only possible for sessions cctop launched: the shim holding the pty is
    /// what has a copy of the output to give away.
    pub(super) fn attach_selected(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let label = format!("{} · {}", session.abbrev_label, session.model);
        let Some(pid) = session_root_pid(session) else {
            self.set_status("Selected session has no local process");
            return;
        };
        if !self.open_view(pid, label) {
            self.set_status(
                "Only sessions started by cctop can be attached — start them as `cctop claude`",
            );
        }
    }

    /// Go back to the agent this cctop launched.
    ///
    /// Without this, F12 would be a one-way door: a freshly started agent has
    /// written no transcript yet, so it has no row in the table to press `a` on.
    pub(super) fn attach_hosted(&mut self) {
        let Some((pid, label)) = self.hosted.clone() else {
            self.set_status("No agent was launched by this cctop — start one as `cctop claude`");
            return;
        };
        if !self.open_view(pid, label) {
            self.set_status("The agent's terminal is gone");
        }
    }

    /// Show the agent running as `pid`, reusing the pane already on it rather
    /// than opening a second window onto one terminal.
    fn open_view(&mut self, pid: u32, label: String) -> bool {
        if let Some((index, tab)) = self
            .tabs
            .iter_mut()
            .enumerate()
            .find(|(_, tab)| tab.panes.iter().any(|pane| pane.pid == pid))
        {
            tab.focus = tab
                .panes
                .iter()
                .position(|pane| pane.pid == pid)
                .unwrap_or(0);
            self.tab = index + 1;
            self.needs_redraw = true;
            return true;
        }
        let Some(pane) = tabs::Pane::view_of(pid, label) else {
            return false;
        };
        self.tabs.push(tabs::Tab::new(pane));
        self.tab = self.tabs.len();
        self.needs_redraw = true;
        true
    }
}

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

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the UI. `hosted` is an agent cctop launched for this session, which it
/// shows attached and outlives by nothing: when the agent exits, so does cctop,
/// so `cctop claude` gets you back to your shell the way `claude` would.
pub fn run(args: &Args, hosted: Option<crate::shim::Hosted>) -> anyhow::Result<i32> {
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
    spawn_quota_poller(res_tx.clone());

    // One cached check per day, off the UI thread. Only ever reports: replacing
    // the binary stays behind an explicit `--update`.
    std::thread::spawn(move || {
        if let Some(version) = crate::update::available_update() {
            let _ = res_tx.send(Response::UpdateAvailable(version));
        }
    });

    let mut app = App::new(args.plan, req_tx.clone());
    app.refresh_secs = args.delay;
    let _ = req_tx.send(Request::Refresh);

    // Attach before the first frame: the agent cctop was asked to launch is the
    // reason it is running, so it should be on screen and not behind a keypress.
    // Its own session row appears later, once it has written a transcript.
    let mut hosted = hosted;
    if let Some(hosted) = hosted.as_ref() {
        app.hosted = Some((hosted.pid, hosted.label.clone()));
        app.attach_hosted();
    }

    // `ratatui::init` installs a hook that leaves the alt screen and raw mode on
    // panic, but it knows nothing about the mouse capture enabled below, nor does
    // its restore path make the cursor visible again. Without this, a panic can
    // leave the user's shell receiving mouse escape sequences or with no cursor.
    // Installed after `init` so it runs before ratatui's restore hook.
    let mut terminal = ratatui::init();
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous_hook(info);
    }));
    let _ = execute!(std::io::stdout(), EnableMouseCapture);

    // Established before the loop so the first tick already has it; `None` just
    // means discovery falls back to the periodic walk.
    let watch = crate::watch::Watch::start();
    app.listener = crate::hook::Listener::start();
    // A hook naming a cctop that has since been moved or deleted fires nothing
    // at all, so it is repointed here rather than left to look installed while
    // reporting nothing. Anything narrower than that is left for the panel.
    for fixed in crate::hook::repair(app.hook_project().as_deref()) {
        app.set_status(&fixed);
    }
    // What repair deliberately would not touch: an install registering fewer
    // events than this cctop wants, or a settings file that will not parse.
    // Both look installed and quietly deliver less than they should, so they
    // are worth one line on the way in — an install that is simply absent is
    // not, since that is a choice and nagging about it is what makes people
    // stop reading the status line.
    if app
        .hook_status()
        .claude
        .iter()
        .any(|s| s.health.is_problem())
    {
        app.set_status("Agent hooks need attention — press h");
    }

    let result = event_loop(
        &mut app,
        &mut terminal,
        &res_rx,
        &req_tx,
        watch.as_ref(),
        hosted.as_mut(),
    );

    // Before the terminal is restored, so the agent's hangup does not race the
    // screen being handed back.
    app.tabs.clear();
    drop(hosted);

    restore_terminal();

    let _ = req_tx.send(Request::Shutdown);
    // The worker persists newly extracted transcript data while shutting down.
    // Joining it matters: returning from main immediately would otherwise kill
    // the detached thread mid-save, forcing every launch to parse all sessions
    // from scratch again.
    let _ = worker.join();
    app.save_prefs();
    result
}

/// Undo every terminal mode the TUI may have changed.
///
/// `ratatui::restore` intentionally only disables raw mode and leaves the
/// alternate screen; it does not restore cursor visibility. Keep this separate
/// so regular exits, input errors, and panics all use the same cleanup path.
fn restore_terminal() {
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    let _ = execute!(std::io::stdout(), Show);
    ratatui::restore();
    // Send Show once more after leaving the alternate screen. Some terminals
    // scope cursor state to the active screen buffer.
    let _ = execute!(std::io::stdout(), Show);
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
    watch: Option<&crate::watch::Watch>,
    mut hosted: Option<&mut crate::shim::Hosted>,
) -> anyhow::Result<i32> {
    let mut last_refresh = Instant::now();
    let mut last_full_walk = Instant::now();
    let mut layout = render::Layout::default();
    let mut refresh_in_flight = true;
    let mut last_blink = true;

    loop {
        // Drain everything the workers have produced.
        let mut annotated_rows_changed = false;
        // Only a refresh can move a session between busy and waiting, so the
        // notifier is fed here rather than once per loop iteration — that would
        // rebuild its map five times a second over rows that hadn't moved.
        let mut rows_changed = false;
        loop {
            match res_rx.try_recv() {
                Ok(Response::Discovered(sessions)) => {
                    app.sessions = sessions;
                    app.stats = crate::loader::compute_stats(&app.sessions);
                    app.refilter();
                    rows_changed = true;
                }
                Ok(Response::Annotated(session)) => {
                    // Match on the key's two fields rather than on `key()`: that
                    // formats a String per candidate, so a scan over thousands of
                    // rows allocated thousands of times — per arriving row, on the
                    // thread that also has to answer the keyboard.
                    let found = app.sessions.iter_mut().find(|s| {
                        s.provider == session.provider && s.session_id == session.session_id
                    });
                    if let Some(existing) = found {
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
                    rows_changed = true;
                }
                Ok(Response::LiveRows(payload)) => {
                    let (rows, stats) = *payload;
                    for row in rows {
                        let found = app
                            .sessions
                            .iter_mut()
                            .find(|s| s.provider == row.provider && s.session_id == row.session_id);
                        match found {
                            Some(existing) => *existing = row,
                            // A session that started since the last full walk.
                            None => app.sessions.push(row),
                        }
                    }
                    app.stats = stats;
                    app.push_history();
                    app.refilter();
                    refresh_in_flight = false;
                    rows_changed = true;
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
                Ok(Response::UpdateAvailable(version)) => {
                    app.update_available = Some(version);
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
                Ok(Response::Deleted {
                    session_key,
                    result,
                }) => {
                    app.deleting.remove(&session_key);
                    match result {
                        Ok(()) => {
                            app.sessions.retain(|session| session.key() != session_key);
                            app.marked.remove(&session_key);
                            app.stats = crate::loader::compute_stats(&app.sessions);
                            app.refilter();
                            app.set_status("Deleted session");
                        }
                        Err(error) => {
                            app.set_status(format!("Could not delete {session_key}: {error}"))
                        }
                    }
                }
                Ok(Response::KeysSent { result }) => match result {
                    Ok(()) => app.set_status("Sent to the session's terminal"),
                    Err(error) => app.set_status(error),
                },
                Ok(Response::Scanned { query, hits }) => app.scanned(query, hits),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        if annotated_rows_changed {
            // A burst can contain hundreds of rows. Recompute and sort once
            // after draining it rather than once per transcript.
            app.stats = crate::loader::compute_stats(&app.sessions);
            app.refilter();
        }
        if rows_changed {
            app.check_bells();
        }

        app.sync_panel_data();
        app.tick_scan();

        // Every tab, not just the visible one: an agent whose output nobody
        // reads eventually blocks on writing it.
        let mut drawn = false;
        for tab in &mut app.tabs {
            drawn |= tab.pump();
        }
        let closed = app.tabs.iter_mut().fold(false, |any, tab| tab.reap() | any);
        if closed {
            app.drop_empty_tabs();
        }
        if drawn || closed {
            app.needs_redraw = true;
        }

        // Hook events arrive whenever an agent hits one, which is not on any
        // tick of ours, so they are drained here alongside everything else.
        if let Some(events) = app.listener.as_ref().map(crate::hook::Listener::drain) {
            let (changed, lifecycle) = app.apply_hooks(events);
            app.needs_redraw |= changed;
            // A session that has just begun or ended is a row to find or forget
            // now. Waiting for the next poll would leave an agent the user just
            // started missing from the table for as long as the interval, which
            // is precisely the moment they are looking for it.
            if lifecycle && !refresh_in_flight {
                let _ = req_tx.send(Request::Refresh);
                refresh_in_flight = true;
                last_refresh = Instant::now();
            }
        }

        // A blinking tab is the one thing on screen that changes with no event
        // behind it, so the loop has to ask for the frame itself — but only on
        // the half-cycle it actually flips, not on every poll.
        let phase = app.blink_on();
        if phase != last_blink && app.any_attention() {
            app.needs_redraw = true;
        }
        last_blink = phase;

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
        // Attached, the same wait is what stands between a keystroke and seeing
        // it echoed, so it drops to a frame's worth.
        let idle_wait = match app.tab {
            0 => Duration::from_millis(200),
            _ => Duration::from_millis(16),
        };
        let wait = refresh_every
            .checked_sub(last_refresh.elapsed())
            .unwrap_or(Duration::ZERO)
            .min(idle_wait);
        if event::poll(wait)? {
            match event::read()? {
                Event::Key(key) => app.on_key(key),
                Event::Mouse(m) => app.on_mouse(m, &layout),
                Event::Resize(_, _) => app.needs_redraw = true,
                _ => {}
            }
        }

        // The agent cctop was launched to run has finished, so cctop has nothing
        // left to do either: hand its exit code back and get out of the way.
        if let Some(hosted) = hosted.as_mut()
            && let Some(code) = hosted.finished()
        {
            return Ok(code);
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
            // Walking every provider directory is what scales with the number of
            // sessions ever created, while what the user watches scales with the
            // number running now. So the fast tick updates the running rows and
            // the walk — the only thing that can notice a *new* session — runs on
            // its own slower cadence.
            let watched_change = watch.is_some_and(crate::watch::Watch::took_structural_change);
            // A transcript is created before it is summarizable — the model name
            // only arrives with the first assistant message — so the walk the
            // create earned can find nothing. Keep walking, at a cadence between
            // the fast tick and the safety net, until the file becomes a session.
            let awaiting = !watched_change
                && last_full_walk.elapsed() >= PENDING_WALK_INTERVAL
                && watch.is_some_and(|w| {
                    w.awaiting_discovery(|path| {
                        app.sessions
                            .iter()
                            .any(|s| s.data_file.as_deref() == Some(path))
                    })
                });
            let full_due =
                watched_change || awaiting || last_full_walk.elapsed() >= FULL_WALK_INTERVAL;
            if full_due {
                last_full_walk = Instant::now();
            }
            let _ = req_tx.send(if full_due {
                Request::Refresh
            } else {
                Request::RefreshLive
            });
        }
    }
    // Quit was pressed rather than the agent exiting, so there is no exit code
    // to inherit.
    Ok(0)
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

    /// A reported state is kept until the session says it is over, and the
    /// events that change *which sessions exist* ask for a rescan while the
    /// ones that only change a state do not.
    #[test]
    fn a_reported_state_is_kept_until_the_session_ends() {
        let event = |id: &str, signal: crate::hook::Signal| crate::hook::Event {
            session_id: id.into(),
            reported: crate::hook::Reported {
                signal,
                cwd: "/w/proj".into(),
            },
        };
        let mut app = test_app();

        // Nothing arriving is not "nothing is happening": an absent entry means
        // fall back to the transcript, so it must stay absent.
        assert_eq!(app.apply_hooks(Vec::new()), (false, false));
        assert!(app.hooked_signal("a").is_none());

        // A start is a row to go and find now.
        assert_eq!(
            app.apply_hooks(vec![event("a", crate::hook::Signal::Started)]),
            (true, true)
        );
        // Ordinary state changes are not worth a rescan of the disk.
        assert_eq!(
            app.apply_hooks(vec![
                event("a", crate::hook::Signal::Busy),
                event("a", crate::hook::Signal::Idle),
            ]),
            (true, false)
        );
        assert_eq!(app.hooked_signal("a"), Some(crate::hook::Signal::Idle));
        assert_eq!(app.reporting(), vec![("proj".to_string(), "idle")]);

        // And an ended session is forgotten rather than left claiming its last
        // state forever — which is also a rescan, since the row is going.
        assert_eq!(
            app.apply_hooks(vec![event("a", crate::hook::Signal::Ended)]),
            (true, true)
        );
        assert!(app.hooked_signal("a").is_none());
        assert!(app.reporting().is_empty());
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

    /// The table abbreviates the working directory to fit its column, so the
    /// filter has to match the full path — otherwise the directory someone
    /// types is one the row is not admitting to.
    #[test]
    fn search_matches_the_full_working_directory() {
        let mut app = test_app();
        let mut deep = session("aaa", false, "/home/x/work/api/services/billing");
        // What the table actually shows for that row.
        deep.abbrev_label = "…/billing".into();
        app.sessions = vec![deep, session("bbb", false, "/home/x/other")];

        app.search = "work/api".into();
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.sessions[app.visible[0]].session_id, "aaa");
    }

    /// Content hits widen the filter, and only for the query they were found
    /// for: a scan that lands after the user has typed another character must
    /// not put its rows back on screen.
    #[test]
    fn transcript_hits_widen_the_filter_for_their_own_query_only() {
        let mut app = test_app();
        app.sessions = vec![
            session("aaa", false, "/home/x/alpha"),
            session("bbb", false, "/home/x/beta"),
        ];
        let hit = |key: &str| HashMap::from([(key.to_string(), "…flywheel…".to_string())]);

        // No content search: a word only the transcript knows finds nothing.
        app.search = "flywheel".into();
        app.refilter();
        assert!(app.visible.is_empty());

        // With one, the session whose transcript matched joins the metadata
        // matches rather than replacing them.
        app.search_content = true;
        app.scanned("flywheel".into(), hit("claude:bbb"));
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.sessions[app.visible[0]].session_id, "bbb");

        app.search = "alpha".into();
        app.scanned("alpha".into(), hit("claude:bbb"));
        assert_eq!(
            app.visible.len(),
            2,
            "metadata and content matches, not one"
        );

        // Hits belonging to a query that has since been extended are ignored.
        app.search = "alphabet".into();
        app.refilter();
        assert!(app.visible.is_empty());
        assert!(app.selected_snippet().is_none());
    }

    /// Turning content search off has to take its results with it, or the rows
    /// it found stay on screen with nothing matching them.
    #[test]
    fn leaving_content_search_drops_its_rows() {
        let mut app = test_app();
        app.sessions = vec![session("aaa", false, "/home/x/alpha")];
        app.search = "flywheel".into();
        app.search_content = true;
        app.scanned(
            "flywheel".into(),
            HashMap::from([("claude:aaa".into(), "…flywheel…".into())]),
        );
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.selected_snippet(), Some("…flywheel…"));

        app.toggle_content_search();
        assert!(app.visible.is_empty());
        assert!(app.selected_snippet().is_none());
    }

    /// A scan is worth its cost only once there is a word to look for, and only
    /// for a query that isn't already answered.
    #[test]
    fn a_scan_waits_for_a_word_and_for_the_typing_to_settle() {
        let mut app = test_app();
        app.sessions = vec![session("aaa", false, "/home/x/alpha")];
        app.search_content = true;

        // Too short to be worth reading every transcript for.
        app.search = "fl".into();
        app.search_edited();
        app.scan_typed_at = None;
        app.tick_scan();
        assert!(!app.scanning);

        // Long enough, but the user is still typing.
        app.search = "flywheel".into();
        app.search_edited();
        app.tick_scan();
        assert!(!app.scanning, "fired before the debounce elapsed");

        // Settled.
        app.scan_typed_at = Some(Instant::now() - SCAN_DEBOUNCE);
        app.tick_scan();
        assert!(app.scanning);

        // And the answer to a query already scanned for is not scanned again.
        app.scanned("flywheel".into(), HashMap::new());
        app.tick_scan();
        assert!(!app.scanning);
    }

    /// ↑ walks back through past queries and ↓ returns, ending on whatever was
    /// half-typed when the walk began.
    #[test]
    fn the_query_history_walks_both_ways() {
        let mut app = test_app();
        app.search_history = vec!["newest".into(), "older".into()];

        app.search = "half-typ".into();
        app.history_step(1);
        assert_eq!(app.search, "newest");
        app.history_step(1);
        assert_eq!(app.search, "older");
        // The end of the history is a floor, not a wrap.
        app.history_step(1);
        assert_eq!(app.search, "older");

        app.history_step(-1);
        assert_eq!(app.search, "newest");
        app.history_step(-1);
        assert_eq!(app.search, "half-typ");
        // Nothing older to come back from any more.
        app.history_step(-1);
        assert_eq!(app.search, "half-typ");
    }

    /// Re-running a query moves it to the front rather than filling the history
    /// with copies of the search someone runs most.
    #[test]
    fn a_repeated_query_is_remembered_once() {
        let mut app = test_app();
        app.search = "alpha".into();
        app.remember_query();
        app.search = "beta".into();
        app.remember_query();
        app.search = "alpha".into();
        app.remember_query();
        assert_eq!(app.search_history, vec!["alpha", "beta"]);

        // An abandoned modal leaves nothing behind.
        app.search = "   ".into();
        app.remember_query();
        assert_eq!(app.search_history, vec!["alpha", "beta"]);
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
    fn batch_delete_keeps_marked_sessions_visible_until_worker_confirms() {
        let mut app = test_app();
        app.sessions = vec![
            session("a", false, "/x"),
            session("b", false, "/x"),
            session("c", false, "/x"),
        ];
        app.refilter();
        // Mark a and c by identity, not by row: the three fixtures are created in
        // the same instant, so which row each lands on depends on how the clock
        // happened to tick. Selecting by index made this assert the sort order,
        // and it failed on hosts where those timestamps came out equal or out of
        // creation order.
        let row = |app: &App, id: &str| {
            app.visible
                .iter()
                .position(|&i| app.sessions[i].session_id == id)
                .expect("fixture is visible")
        };
        app.selected = row(&app, "a");
        app.toggle_mark();
        app.selected = row(&app, "c");
        app.toggle_mark();
        assert_eq!(app.marked.len(), 2);
        assert_eq!(app.marked_sessions().len(), 2);

        app.batch(BatchKind::Delete);
        assert_eq!(app.mode, Mode::BatchConfirm);
        app.batch_execute();
        assert_eq!(app.sessions.len(), 3);
        assert_eq!(app.deleting.len(), 2);
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

    /// The other half of the bell: hearing it is useless if the row it came
    /// from is somewhere in a list of a dozen.
    #[test]
    fn b_jumps_the_selection_to_the_session_that_rang() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "/x/a"), session("b", true, "/x/b")];
        app.refilter();
        let target = app.sessions[1].key();
        app.notify.last = Some(crate::notify::Rang {
            key: target.clone(),
            label: "b".into(),
            reason: crate::notify::Reason::NeedsInput,
            at: Instant::now(),
        });

        app.selected = 0;
        app.jump_to_bell();
        assert_eq!(app.selected_session().map(Session::key), Some(target));

        // Filtered out of the table, the bell has nowhere to land — and says so
        // rather than moving the cursor to an unrelated row.
        app.search = "x/a".into();
        app.refilter();
        app.selected = 0;
        app.jump_to_bell();
        assert_eq!(app.selected, 0);
        assert!(app.status.is_some());
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
