//! Terminal UI: the application state every other module in here operates on.
//!
//! [`App`] is one struct with a field for everything on screen, and the methods
//! that change it are split across this directory by what they are *about*
//! rather than by type: [`select`] owns the cursor, [`filter`] what the table
//! shows, [`launch`] and [`panes`] the agents and the tabs they run in,
//! [`signals`] what those agents report, [`worker`] the thread that does the
//! filesystem work and [`runloop`] the terminal and the event loop. They are
//! `impl App` blocks in sibling modules, so nothing here had to become public
//! to make the split; what stays is the state itself and the few things — the
//! modes, the row type, construction and the toasts — that every one of
//! them touches.

mod ansi;
mod batch;
pub mod columns;
mod dirs;
mod drunk;
mod effects;
mod filter;
mod high;
mod hooks;
mod hyperlink;
mod idle;
mod input;
mod launch;
mod launch_cwd;
mod line_edit;
mod markdown;
pub mod menu;
mod modals;
pub mod panels;
mod panes;
mod preview;
mod profiles;
mod qr;
mod rave;
mod reader;
mod remote;
pub mod render;
mod runloop;
mod scrollbar;
mod seen;
mod select;
mod settings;
mod share;
mod signals;
#[cfg(test)]
mod snapshot;
pub mod spark;
mod styled;
mod table;
pub mod tabs;
pub mod theme;
mod toast;
mod torn;
mod tree;
mod worker;

pub use runloop::run;
use share::{Opening, ShareQr};
use worker::Request;

use crate::cache::UiPrefs;
use crate::loader::Stats;
use crate::pricing::{Plan, Provider};
use crate::quota::Quota;
use crate::session::{Session, SessionData};
use columns::ColumnId;
use spark::History;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

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
    /// Confirming `cctop --update` on the machine in `App::remote_update`.
    RemoteUpdateConfirm,
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
    /// Text input typed into the selected session's rmux pane.
    SendKeys,
    /// Picking which agent a new tab or split should run.
    Launch,
    /// Everything that can be done to the selected row, in one list.
    RowMenu,
    /// Typing the directory the launcher's pick will start in. Drawn as the
    /// launcher with its `in` line in an editable state, so the list of agents
    /// stays visible while the path is being changed.
    LaunchCwd,
    /// The agent-integration panel: what is installed where, and whether the
    /// agents are actually reporting in.
    Hooks,
    /// `cctop optimize` or `cctop compare`, drawn over the table.
    Insight,
    /// The selected session's conversation, read-only — what the report page
    /// shows in a browser, over the table. `i` on a row, or Enter's menu.
    Conversation,
    /// Offering to install rmux, a launch having found it missing.
    TmuxInstall,
    /// Naming or painting a workspace tab, opened by right-clicking it or
    /// with `Alt+r`.
    RenameTab,
    /// The tab switcher: every tab in the bar, narrowed by typing, taken by
    /// Enter. `Alt+t`, for the tabs that have no digit of their own.
    SwitchTab,
    /// The browser panel: whether this cctop is serving its table to one, on
    /// what links, and whether they leave the machine.
    Serve,
    /// A terminal just shared with `W`, as a code a phone can scan. See
    /// [`ShareQr`].
    ShareQr,
    /// What `config.toml` sets and what it could set, keybinds included.
    Settings,
    /// Adding a Claude account: naming it, then `claude setup-token` in a
    /// terminal inside the popup. See [`AddAccount`].
    AddAccount,
}

/// The two things an added Claude account can be.
///
/// Both, because each gives up something the other keeps — see
/// [`AccountSource`](crate::config::AccountSource). A full login is its own
/// `~/.claude-<name>`, and every feature works under it. A token keeps the one
/// `~/.claude` history every account resumes from, and Claude Code lets it make
/// model requests and nothing else: no Remote Control, no claude.ai connectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountKind {
    /// `claude auth login` under `CLAUDE_CONFIG_DIR=~/.claude-<name>`.
    Login,
    /// `claude setup-token`, the token kept in cctop's config.
    Token,
}

/// The add-account popup, from naming the account to its token being saved.
///
/// `claude setup-token` runs on a pty of the popup's own, and the token is read
/// off its screen the moment it is printed — so the whole thing happens without
/// leaving cctop, and nothing is copied or pasted by hand.
///
/// ponytail: `setup-token` authorises whichever claude.ai login the browser
/// already has, and the popup no longer warns about it — the prompt is kept to
/// the one question. `cctop --add-account` still says it.
#[derive(Default)]
pub struct AddAccount {
    pub name: line_edit::LineEdit,
    /// Which kind of account, once the name is in and the choice is made.
    /// `None` with a name accepted is the popup asking.
    pub kind: Option<AccountKind>,
    /// Whether the name has been accepted, which moves the popup on to asking
    /// which kind of account it is.
    pub named: bool,
    /// `setup-token`, once the name is in. Dropping it ends the process, which
    /// is what cancelling the popup should do.
    pub pane: Option<tabs::Pane>,
    /// The sign-in link off its screen, for when no browser opened.
    pub link: Option<String>,
    /// How it ended: the saved account's name, or what went wrong.
    pub outcome: Option<Result<String, String>>,
}

/// A launch that stopped to ask about rmux, and how to pick it up again.
///
/// The launch is re-run from the top rather than resumed mid-way, because
/// answering the question changes the first thing it decides — where the agent
/// is going to live. Both entry points derive everything they need from state
/// the modal does not touch (the table selection, the launcher's snapshot), so
/// running them twice starts one agent, not two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deferred {
    /// [`App::resume_selected`], stopped at the ownership decision.
    Resume,
    /// [`App::launch_selected`], stopped at the same place.
    Launch,
}

/// Where the agent picked in `Mode::Launch` ends up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchInto {
    /// A tab of its own.
    Tab,
    /// Alongside the panes already in the current tab, arranged the given way.
    Split { stacked: bool },
}

/// The image a paste just filed, drawn in the corner for a few seconds.
///
/// Decoded once when it is set rather than per frame — the file has already
/// been handed to the agent as a path by then, so a preview that cannot decode
/// costs the paste nothing and the status line still names it.
pub struct PastePreview {
    /// The filename, which is what the agent was told.
    pub name: String,
    /// When it was pasted; the corner holds it briefly, like the status line.
    pub at: Instant,
    /// Decoded and ready to draw. Halfblocks only: they land in ratatui's
    /// buffer as ordinary cells, so they compose with whatever is underneath,
    /// render over ssh and tmux without protocol support, and a `TestBackend`
    /// can see them.
    pub image: ratatui_image::protocol::StatefulProtocol,
}

/// The pending batch action shown in `Mode::BatchConfirm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchKind {
    Delete,
    Kill,
    /// Stop the idle view's sessions: the marked ones, or every one when none
    /// is marked. Unlike [`BatchKind::Kill`] it skips rather than refuses —
    /// see [`App::reclaim_plan`].
    Reclaim,
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
// Application state
// ---------------------------------------------------------------------------

/// One line of the table: a session, a subagent shown beneath its parent, or
/// in the tree view a heading for the sessions of one repository or checkout.
///
/// Rows rather than session indices, because an expanded session occupies
/// several lines and everything that walks the table — scrolling, the cursor,
/// search, the mouse — has to agree on how many there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Session(usize),
    /// `index` is into the parent's own `subagents`, which is the only place
    /// they exist; they are not sessions and have no entry in `sessions`.
    Subagent {
        parent: usize,
        index: usize,
    },
    /// An index into [`App::groups`].
    Group(usize),
}

impl Row {
    /// The session this row belongs to, which for a child is its parent.
    ///
    /// Actions are addressed to sessions — a subagent has no process to signal
    /// and no transcript of its own to delete — so a child resolves to its
    /// parent. A group heading resolves to nothing: it stands for several
    /// sessions, and an action aimed at "one of them" would be aimed at a row
    /// nobody pointed at. Every action already bails on no session, which is
    /// what makes the heading inert without a guard per key.
    pub fn session(self) -> Option<usize> {
        match self {
            Row::Session(i) => Some(i),
            Row::Subagent { parent, .. } => Some(parent),
            Row::Group(_) => None,
        }
    }

    pub fn is_subagent(self) -> bool {
        matches!(self, Row::Subagent { .. })
    }
}

pub struct App {
    pub sessions: Vec<Session>,
    /// Whether a first load has landed. Discovery is asynchronous, so an empty
    /// `sessions` means "still looking" until this flips — and "you have none"
    /// is a very different thing to tell someone.
    pub loaded: bool,
    /// The table's lines, after filtering, sorting and expansion.
    pub visible: Vec<Row>,
    /// Subagents whose own `SubagentStop` has arrived.
    ///
    /// Held for the run rather than per session: this is the only word cctop
    /// gets that a background subagent has finished, and the transcript it would
    /// otherwise be inferred from cannot say it. Ids are unique per run, so the
    /// set does not need scoping to a parent.
    pub finished_agents: std::collections::HashSet<String>,
    /// Keys of the sessions showing their subagents.
    ///
    /// Keyed rather than indexed because `sessions` is rebuilt wholesale on
    /// every walk, which would leave an index pointing at whatever sorted into
    /// that slot next.
    pub expanded: std::collections::HashSet<String>,
    /// Whether the table is drawn as a tree of repositories and checkouts.
    pub tree: bool,
    /// Keys of the tree groups that are folded.
    pub collapsed: std::collections::HashSet<String>,
    /// The tree's headings, which `Row::Group` indexes. Rebuilt with `visible`.
    pub groups: Vec<tree::Group>,
    /// The tree glyphs leading each row's label, aligned with `visible`. Empty
    /// when the tree is off.
    pub indent: Vec<String>,
    /// How many sessions passed the filters, which is not `visible.len()` once
    /// rows can be headings, children, or folded away.
    pub matched: usize,
    pub stats: Stats,
    pub selected: usize,
    pub scroll: usize,
    pub plan: Plan,
    pub mode: Mode,

    pub sort_col: ColumnId,
    pub sort_asc: bool,
    pub sortby_cursor: usize,

    pub search: line_edit::LineEdit,
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
    /// The idle view: only live sessions quiet for `idle_after`, biggest
    /// first. See the `idle` module.
    pub idle_only: bool,
    /// The sort the idle view replaced, put back when it closes.
    idle_sort: Option<(ColumnId, bool)>,

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
    /// Which profile the launcher will start each harness under, as an index
    /// into that harness's [`crate::config::launchable_for`] list.
    ///
    /// Per harness rather than one index: the launcher cursor moves between
    /// `claude` and `codex`, and an index is only meaningful against the list
    /// it came from — carrying one across would point at whichever account
    /// happened to sit in that position.
    pub launch_profile: HashMap<Provider, usize>,
    /// Which entry of the row menu is under the cursor.
    ///
    /// Only ever points at an entry that can run: the menu draws the blocked
    /// ones to explain them and never lets the cursor rest there. See
    /// [`menu::step`].
    pub menu_cursor: usize,
    /// Raw digits being typed into the cost-floor modal.
    pub cost_input: line_edit::LineEdit,
    /// The directory being typed into the launcher, spelled as the user is
    /// spelling it — `~` and all, expanded only when it is accepted.
    pub launch_cwd_input: line_edit::LineEdit,
    /// Set when the typed directory does not name one, so the field can say so
    /// where it is being typed rather than in a toast across the screen.
    pub launch_cwd_bad: bool,
    /// Directories agents are already known to have run in, newest first, as of
    /// the moment the field opened.
    ///
    /// A snapshot for the same reason the launcher's list of agents is one: a
    /// list that reshuffles under the cursor while a walk lands means Enter
    /// takes a directory other than the one highlighted.
    pub launch_cwd_known: Vec<std::path::PathBuf>,
    /// What the field is currently offering for what has been typed. Every one
    /// of them is a directory that exists, which is what lets a picked
    /// suggestion skip the check the typed path gets.
    pub launch_cwd_hits: Vec<std::path::PathBuf>,
    /// The suggestion under the cursor, when the cursor has left the text.
    /// `None` means the field is being typed in, and Enter takes what is typed.
    pub launch_cwd_pick: Option<usize>,
    /// Line being typed into the selected session's terminal.
    pub send_input: line_edit::LineEdit,
    /// The new name being typed for a tab, and which tab it is for.
    ///
    /// The title as it stood when the rename opened is kept alongside the
    /// index because the bar moves under a modal: a tab whose agent exits is
    /// retired mid-typing and every index after it shifts down one. Checking
    /// the title back means a rename either lands on the tab it was aimed at or
    /// is dropped, rather than renaming whichever tab slid into the slot.
    pub rename_input: line_edit::LineEdit,
    pub rename_tab: usize,
    pub rename_was: String,
    /// The colour the rename modal is offering for the tab.
    ///
    /// Seeded from the tab's own when the prompt opens, so Enter pressed for
    /// the name alone leaves it alone — and so the swatch that would need no
    /// paint is the one already bracketed.
    pub rename_color: Option<theme::Hue>,
    /// When a right-click opened the rename prompt.
    ///
    /// Terminals that paste on the right button — Windows Terminal does, and
    /// keeps doing it while mouse capture is on — send the click *and* the
    /// clipboard, so the prompt opens with somebody's last copy already typed
    /// into it. A paste that lands in the same instant as the click that
    /// opened the field is that echo, not a person, and is dropped.
    pub rename_opened_by_click: Option<Instant>,
    /// What has been typed into the tab switcher, and which row of the
    /// narrowed list the cursor is on. See `App::switch_matches`.
    pub switch_filter: line_edit::LineEdit,
    pub switch_cursor: usize,
    /// Which tabs the switcher lists by what they are doing, cycled with Tab.
    pub switch_state: panes::SwitchState,
    /// Whether the footer's `q Quit` has been clicked once already.
    ///
    /// The share corner's `share_arm` for the other irreversible thing a
    /// pointer can reach. Cleared by any other click, so it only ever spans one
    /// deliberate pair.
    pub quit_arm: bool,
    /// Table viewport height (rows), recorded during draw so Ctrl+U/Ctrl+D can
    /// page by half a screen.
    pub list_height: u16,

    /// Columns the user has hidden outright (`$CCTOP_COLUMNS_HIDE`). These win
    /// over the automatic width-based dropping in [`columns::visible_columns`].
    pub hidden_columns: Vec<ColumnId>,

    /// Scroll offset of the help overlay, which is taller than most terminals.
    pub help_scroll: u16,
    /// Last computed bottom of the help overlay, recorded during draw.
    pub help_max_scroll: u16,
    /// What the help is narrowed to, typed after `/` in it.
    pub help_filter: line_edit::LineEdit,
    /// Whether keys are going into `help_filter` rather than moving the page.
    /// Enter stops typing and keeps the filter, so `j` and `k` scroll the
    /// narrowed page again instead of spelling more of the query.
    pub help_typing: bool,

    /// `[settings]` and `[keys]` from `config.toml`, as last read.
    pub settings: crate::settings::Settings,
    /// Those `[keys]`, applied in front of the dashboard's key handler.
    pub keymap: crate::settings::Keymap,
    /// The file those are read from and written to. `None` in tests, which
    /// must not touch the developer's own config.
    pub settings_file: Option<std::path::PathBuf>,
    /// The file's mtime when it was last read, so an edit made in an editor is
    /// picked up at the next key without a restart.
    pub settings_stamp: u64,
    /// Scroll offset of the settings overlay, kept following the cursor.
    pub settings_scroll: u16,
    /// The panel's row: the settings first, then the keybinds, in the order of
    /// [`SETTINGS`](crate::settings::SETTINGS) and
    /// [`BINDINGS`](crate::settings::BINDINGS).
    pub settings_cursor: usize,
    /// Waiting for the key the cursor's action should move to.
    pub settings_capture: bool,
    /// A setting's value being typed, for the ones that are not a toggle.
    pub settings_input: Option<line_edit::LineEdit>,

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
    /// Last computed maximum scroll for whichever bottom panel was drawn,
    /// recorded during draw because only the renderer knows how many lines the
    /// panel produced and how tall it ended up. Without it a key handler can
    /// clamp the top but not the bottom, and scrolling past the end banks
    /// invisible offsets that then have to be scrolled back through.
    pub panel_max_scroll: u16,
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
    /// What cctop has said lately, held until each has been up long enough to
    /// read. See [`toast`].
    pub toasts: toast::Toasts,
    /// When cctop started, used by the tool-activity "live" filter.
    pub started_at: String,
    /// The same moment as an `Instant`, which is what the tab-bar blink is
    /// phased against — a wall clock can jump, and a blink that stutters when
    /// NTP steps the clock looks like a bug.
    started: Instant,
    /// The easter egg. See [`rave`].
    rave: rave::Rave,
    /// The other one. See [`drunk`].
    drunk: drunk::Drunk,
    /// And the third. See [`high`].
    high: high::High,

    /// Bell and desktop notifications, and who rang last.
    pub notify: crate::notify::Notifier,
    /// The `alert_*` thresholds' state: which have fired, and which rows are
    /// still past theirs. Beside the notifier rather than inside it, because
    /// the bell's question — is it my move? — is about a session's state and
    /// these are about its numbers.
    pub alerts: crate::alert::Alerts,
    /// Which finished turns have not been looked at yet — see [`seen`].
    pub seen: seen::Seen,

    /// The last snapshot from each machine named with `--host`, keyed by the
    /// target as the user spelled it.
    ///
    /// Held apart from `sessions` rather than merged once, because `sessions`
    /// is replaced wholesale by every walk and would drop them; `merge_remotes`
    /// puts the current snapshots back after each replacement.
    pub remotes: HashMap<String, Vec<Session>>,
    /// Machines that failed their last poll, and why.
    ///
    /// Shown rather than logged. A host that has quietly dropped out is worse
    /// than one that was never added: the totals still look complete.
    pub remote_errors: HashMap<String, String>,
    /// What each host's `cctop --version` said, asked once per connection.
    pub remote_versions: HashMap<String, crate::fleet::Probe>,
    /// Hosts whose version skew has been toasted this run. Once per host: the
    /// news is the same every poll, and a toast that comes back every fifteen
    /// seconds is one people learn to stop reading.
    pub remote_skew_told: std::collections::HashSet<String>,
    /// The host an update is being confirmed for, in `RemoteUpdateConfirm`.
    /// Held by name rather than read off the selection, which the table's
    /// next refresh is free to move.
    pub remote_update: Option<String>,
    /// Hosts with a `--update` in flight, so the menu does not offer a second.
    pub remote_updating: std::collections::HashSet<String>,

    /// Which live sessions are working the same ground, recomputed whenever
    /// rows move. The level also rides on each row so the table can sort by it;
    /// this holds the part only the footer and the Info panel need — who, and
    /// which files.
    pub collisions: crate::collide::Map,

    /// Workspace tabs beyond the dashboard, each holding one or more terminals.
    pub tabs: Vec<tabs::Tab>,
    /// The last screen the Preview panel read off a detached tab, kept between
    /// captures so each one replays into the same parser.
    preview: preview::Capture,
    /// Which tab is on screen: `0` is the dashboard, `1..=tabs.len()` index
    /// `tabs`. Zero-length `tabs` is the ordinary case: the bar still shows the
    /// dashboard and its new-tab button, so the feature is findable.
    pub tab: usize,
    /// When the tab bar was last reconciled against the rmux sessions on this
    /// machine. See [`App::sync_shared_tabs`].
    shared_at: Option<Instant>,
    /// The tab being dragged along the bar, indexed as the bar is: `1..=len`,
    /// and never `0` because the dashboard does not move.
    ///
    /// Set on the press and cleared on the release, which is also what makes the
    /// release the bar's rather than the agent's: a drag that started on the bar
    /// and ended over a pane must not be delivered as a click inside it.
    pub(super) drag_tab: Option<usize>,
    /// What each session's own hooks last said about it, keyed by session id.
    ///
    /// Only sessions whose agent has cctop's hooks installed appear here, so an
    /// absent entry is the ordinary case and means "fall back to the transcript"
    /// rather than "nothing is happening".
    pub hooked: HashMap<String, crate::hook::Reported>,
    /// What each tab's agent says on its own screen, by agent pid, while
    /// `read_screen` is on — see [`App::read_screens`].
    pub screen_read: HashMap<u32, crate::hook::Signal>,
    /// The questions a session's subagents are waiting on, by subagent id.
    ///
    /// `hooked` holds one report per session and every event replaces it, so a
    /// subagent's permission prompt was overwritten by whatever a sibling
    /// running beside it did next — and the tab stopped asking while the
    /// question was still on screen. A question is kept here until the
    /// subagent that asked it says something else. See `App::apply_hooks`.
    pub asking_agents: HashMap<String, HashMap<String, crate::hook::Reported>>,
    /// The process tree each session's hooks reported running under, keyed by
    /// session id.
    ///
    /// Kept apart from `hooked` because it answers a different question and
    /// changes on a different clock: `hooked` is what the agent is *doing* and
    /// turns over constantly, while this is *where it is* and is written once
    /// and then repeated. Only the changes go to the worker, which is what makes
    /// storing it separately worth a field — see [`App::note_hook_pids`].
    pub(super) hook_pids: HashMap<String, Vec<u32>>,
    /// The integration's state, as of the last time the panel was opened.
    ///
    /// Rebuilt on opening and after every action rather than every frame: it
    /// reads three files off disk and scans a directory, which is nothing to do
    /// once and wasteful to do sixty times a second behind a closed panel.
    pub hooks: Option<crate::hook::Report>,
    /// Readings of each account's rate-limit windows, for the unused-allowance
    /// figure on the Limits pane. Reloaded when a quota response arrives rather
    /// than held by the poller, so the file has exactly one writer.
    pub burn: crate::burn::Log,
    /// The open report, and how far it is scrolled. `None` while the worker is
    /// still building it, which is what the overlay draws as "working".
    pub insight: Option<String>,
    pub insight_scroll: u16,
    /// Which report was asked for, so the overlay can name itself before the
    /// text arrives.
    pub insight_kind: &'static str,
    /// The open conversation view. `None` while no overlay is up; the
    /// `Conversation` inside it is `None` while the worker is still reading.
    pub chat: Option<ChatView>,
    /// The remote machines this run is reading, kept beside `remotes` because
    /// the rows say only *where* a session lives — reaching it for a
    /// conversation or a served report wants the `Host`, command and all.
    pub remote_hosts: Vec<crate::fleet::Host>,
    /// The socket the agents push their events to. `None` when one could not be
    /// bound, in which case every estimate carries on exactly as it did before
    /// hooks existed.
    pub listener: Option<crate::hook::Listener>,
    /// The command highlighted in the launcher.
    pub launch_cursor: usize,
    /// What the launcher is offering, as it was when it opened.
    ///
    /// A snapshot rather than a live look, for correctness before cost: the list
    /// includes agents that can finish while the modal is up, and a list that
    /// reshuffles under a cursor means Enter starts something other than the row
    /// highlighted. It also keeps a `rmux` subprocess out of the draw loop.
    pub launch_offer: Vec<tabs::Choice>,
    /// Where the launcher's pick will go.
    pub launch_into: LaunchInto,
    /// Directory cctop was started in. Fresh tabs start here, rather than in
    /// whichever historical session happens to be selected in the dashboard.
    pub launch_root: Option<std::path::PathBuf>,
    /// Directory a launched agent starts in, captured when the launcher opens.
    /// Splits retain their tab's directory; a handoff deliberately overrides
    /// this with the source session's project.
    pub launch_cwd: Option<std::path::PathBuf>,
    /// The install the rmux offer is currently showing, so the modal draws the
    /// command that will actually run rather than working it out again.
    pub rmux_install: Option<crate::rmux::Install>,
    /// The launch waiting on the rmux question, or on the install it started.
    pub rmux_deferred: Option<Deferred>,
    /// Whether the offer has been turned down. One "no" holds for the run:
    /// asking again on the next tab would make declining rmux cost more than
    /// accepting it, which is a way of not really offering a choice.
    ///
    /// Not persisted — a decision about this machine belongs in whether rmux is
    /// installed on it, and cctop already reads that directly.
    pub rmux_declined: bool,
    /// The server this cctop is running, when it is running one.
    ///
    /// `cctop serve` is the same server with nobody watching the terminal it
    /// took over. Holding it here is what lets the table and the page be up at
    /// once — and, because the dashboard already walks every session, what lets
    /// the page be fed from the rows on screen rather than from a second scan
    /// of the same disk.
    ///
    /// Dropping it revokes the tunnel and stops the listener, so quitting cctop
    /// takes the page with it. That is the same bargain `serve` makes.
    pub serving: Option<crate::serve::Serving>,
    /// The add-account popup, while `Mode::AddAccount` is up.
    pub add_account: AddAccount,
    /// A tunnel being registered on a thread of its own.
    ///
    /// Registering with Cloudflare's edge is a second or more of network, and
    /// doing it on the UI thread stopped the dashboard dead: no frame, no
    /// keyboard, and nothing on screen saying why. It runs beside the loop
    /// instead, and the corner spins while it does.
    pub share_opening: Option<Opening>,
    /// Whether the footer's share button has been armed by a first click.
    ///
    /// The second click is what opens the tunnel. Not persisted and not carried
    /// between clicks on anything else: arming is a promise about the click
    /// happening now, and one left standing from ten minutes ago is exactly the
    /// stray click it exists to prevent.
    pub share_arm: bool,
    /// Whether the Serve panel is showing its tunnel link as a QR code.
    ///
    /// Off each time the panel opens, and on only for `c`: the code is the link,
    /// and the panel is opened to check on a serve as often as to hand one out.
    pub serve_qr: bool,
    /// The terminal share `W` just made, while `Mode::ShareQr` is up.
    pub share_qr: Option<ShareQr>,
    /// A restart asked for while the agent was mid-turn: which agent, and when.
    ///
    /// Restarting kills the agent, and a turn in flight dies with it. The key
    /// pressed again within [`RESTART_ARM`](launch::RESTART_ARM) is the answer
    /// to that, rather than a dialog: the usual restart is of an idle agent that
    /// has just said an update is installed, and that one should not be asked
    /// anything.
    pub restart_arm: Option<(u32, Instant)>,
    /// Keys bound for a pane, held while they might be a mouse report the
    /// terminal's input was split through. See [`torn`].
    pub torn: torn::Torn,
    /// Why the last attempt to serve failed, kept for the panel to show.
    ///
    /// A port in use or a tunnel that would not register are both answers
    /// somebody has to read, and a status line that has since been overwritten
    /// by a refresh is not where they can read it.
    pub serve_error: Option<String>,
    /// The pane running the install, while one is running.
    ///
    /// Watched for two endings: rmux appearing, which releases the deferred
    /// launch into a rmux-backed pane, and the pane going away without it,
    /// which means the install failed and the launch should stop waiting.
    pub rmux_installing: Option<u32>,
    /// A handoff brief waiting for the agent the launcher is about to start.
    ///
    /// Held across the launcher rather than typed at the moment `H` is pressed,
    /// because the agent that will receive it does not exist yet: `H` writes the
    /// brief and opens the launcher, and whichever agent is picked inherits it.
    pub pending_brief: Option<std::path::PathBuf>,
    /// The transcript behind that brief, when the session it describes is one a
    /// second Claude could be resumed onto directly.
    ///
    /// Held beside the brief rather than instead of it: which of the two is
    /// used is not known until an agent has been picked, and every agent but
    /// Claude still needs the brief. See [`crate::handoff::fork`].
    pub pending_fork: Option<std::path::PathBuf>,
    /// A brief handed to an agent that is still starting up, as
    /// `(pid, line, not before)`.
    ///
    /// An agent cannot be typed at until its TUI is reading the keyboard, and
    /// there is no signal for that — a line sent into the first half-second of
    /// startup is swallowed by whatever the harness prints over it. So the line
    /// waits here and the loop delivers it once the agent has had time to draw.
    pub handoff_send: Option<(u32, String, Instant)>,
    /// The agent this cctop launched, as `(pid, label)`.
    ///
    /// Set only for `cctop <agent>`, and only while it is alive — the loop exits
    /// as soon as it is not. It is what `A` goes back to after F12, and the
    /// reason quitting asks first: the agent is on a pty this process owns and
    /// does not survive it.
    pub hosted: Option<(u32, String)>,
    /// The pasted image the corner is previewing, while it is previewing one.
    pub paste_preview: Option<PastePreview>,

    prefs: UiPrefs,
    tx: Sender<Request>,
    pub needs_redraw: bool,
    pub should_quit: bool,
}

impl App {
    fn new(plan: Plan, tx: Sender<Request>) -> Self {
        let settings = crate::settings::Settings::load();
        let mut prefs = UiPrefs::load();
        if let Some(notify) = settings.notify {
            prefs.notify = notify;
        }
        let mut app = Self::with_prefs(plan, tx, prefs);
        if std::env::var_os("CCTOP_COLUMNS_HIDE").is_none()
            && let Some(hide) = &settings.hide_columns
        {
            app.hidden_columns = columns::parse_hidden(hide);
        }
        app.settings_file = Some(crate::config::CONFIG_FILE.clone());
        app.reload_settings();
        app
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
            burn: crate::burn::Log::load(),
            insight: None,
            insight_scroll: 0,
            insight_kind: "optimize",
            chat: None,
            remote_hosts: Vec::new(),
            loaded: false,
            visible: Vec::new(),
            finished_agents: std::collections::HashSet::new(),
            expanded: prefs
                .expanded
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>(),
            tree: prefs.tree,
            collapsed: prefs.collapsed_groups.iter().cloned().collect(),
            groups: Vec::new(),
            indent: Vec::new(),
            matched: 0,
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
            search: Default::default(),
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
            idle_only: false,
            idle_sort: None,
            marked: HashSet::new(),
            deleting: HashSet::new(),
            batch: BatchKind::Delete,
            follow: false,
            refresh_secs: 2.0,
            cost_floor: prefs.cost_floor,
            // The one used last, or the default when that profile has since
            // gone: a name that no longer resolves must not silently launch
            // under somebody else's account.
            launch_profile: [
                (Provider::Claude, prefs.claude_profile.as_deref()),
                (Provider::Codex, prefs.codex_profile.as_deref()),
            ]
            .into_iter()
            .map(|(provider, remembered)| {
                let at = remembered
                    .and_then(|name| {
                        crate::config::launchable_for(provider)
                            .iter()
                            .position(|p| p.name == name)
                    })
                    .unwrap_or(0);
                (provider, at)
            })
            .collect(),
            menu_cursor: 0,
            cost_input: Default::default(),
            send_input: Default::default(),
            rename_input: Default::default(),
            rename_tab: 0,
            rename_was: String::new(),
            rename_color: None,
            rename_opened_by_click: None,
            switch_filter: Default::default(),
            switch_cursor: 0,
            switch_state: Default::default(),
            quit_arm: false,
            list_height: 0,
            hidden_columns: hidden_columns(&prefs),
            help_scroll: 0,
            help_max_scroll: 0,
            help_filter: Default::default(),
            help_typing: false,
            settings: Default::default(),
            keymap: Default::default(),
            settings_file: None,
            settings_stamp: 0,
            settings_scroll: 0,
            settings_cursor: 0,
            settings_capture: false,
            settings_input: None,
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
            panel_max_scroll: 0,
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
            alerts: crate::alert::Alerts::default(),
            seen: seen::Seen::default(),
            collisions: crate::collide::Map::new(),
            remotes: HashMap::new(),
            remote_errors: HashMap::new(),
            remote_versions: HashMap::new(),
            remote_skew_told: std::collections::HashSet::new(),
            remote_update: None,
            remote_updating: std::collections::HashSet::new(),
            update_available: None,
            toasts: toast::Toasts::default(),
            started_at: chrono::Utc::now().to_rfc3339(),
            started: Instant::now(),
            rave: rave::Rave::default(),
            drunk: drunk::Drunk::default(),
            high: high::High::default(),
            prefs,
            tx,
            tabs: Vec::new(),
            preview: preview::Capture::default(),
            tab: 0,
            shared_at: None,
            drag_tab: None,
            hooked: HashMap::new(),
            screen_read: HashMap::new(),
            asking_agents: HashMap::new(),
            // Loaded rather than started empty, because the row most likely to
            // want a tab blinking is the one blocked on a question — and that
            // is exactly the row that sends nothing until it is answered. See
            // [`hook::load_claims`](crate::hook::load_claims).
            hook_pids: crate::hook::load_claims(),
            hooks: None,
            listener: None,
            launch_cursor: 0,
            launch_offer: Vec::new(),
            launch_into: LaunchInto::Tab,
            launch_root: std::env::current_dir().ok(),
            launch_cwd: None,
            launch_cwd_input: Default::default(),
            launch_cwd_bad: false,
            launch_cwd_known: Vec::new(),
            launch_cwd_hits: Vec::new(),
            launch_cwd_pick: None,
            rmux_install: None,
            rmux_deferred: None,
            rmux_declined: false,
            rmux_installing: None,
            serving: None,
            add_account: AddAccount::default(),
            serve_error: None,
            share_opening: None,
            share_arm: false,
            serve_qr: false,
            share_qr: None,
            restart_arm: None,
            torn: torn::Torn::default(),
            pending_brief: None,
            pending_fork: None,
            handoff_send: None,
            hosted: None,
            paste_preview: None,
            needs_redraw: true,
            should_quit: false,
        }
    }

    fn save_prefs(&mut self) {
        self.prefs.bottom_tab = self.bottom_tab;
        self.prefs.live_only = self.live_only;
        self.prefs.inactivity_filter = self.age_filter.map(|a| a.key().to_string());
        self.prefs.agent_live_filter = self.tool_live_only;
        self.prefs.tool_show_diff = self.tool_show_diff;
        // Sorted so the file does not churn on every save purely because a
        // HashSet iterated in a different order.
        let mut expanded: Vec<String> = self.expanded.iter().cloned().collect();
        expanded.sort();
        self.prefs.expanded = expanded;
        self.prefs.tree = self.tree;
        let mut collapsed: Vec<String> = self.collapsed.iter().cloned().collect();
        collapsed.sort();
        self.prefs.collapsed_groups = collapsed;
        self.prefs.subagent_sort_col = self.subagent_sort.0.key().to_string();
        self.prefs.subagent_sort_asc = self.subagent_sort.1;
        self.prefs.cost_floor = self.cost_floor;
        self.prefs.claude_profile = self
            .chosen_profile(Provider::Claude)
            .map(|p| p.name.clone());
        self.prefs.codex_profile = self.chosen_profile(Provider::Codex).map(|p| p.name.clone());
        self.prefs.notify = self.notify.enabled;
        self.prefs.search_history = self.search_history.clone();
        self.prefs.save();
    }

    /// Say something. It goes up as a toast, alongside whatever else was said
    /// in the last few seconds rather than in place of it.
    fn set_status(&mut self, msg: impl Into<String>) {
        self.toasts.push(msg.into());
        self.needs_redraw = true;
    }

    /// The last thing said, while it is still on screen.
    #[cfg(test)]
    pub(crate) fn status(&self) -> Option<&str> {
        self.toasts.latest()
    }

    /// Open `optimize` or `compare` over the table.
    ///
    /// The report is built on the worker and arrives later, so this opens the
    /// overlay empty rather than blocking the render loop for the second or two
    /// a full re-parse takes.
    fn open_insight(&mut self, which: &'static str) {
        self.insight = None;
        self.insight_scroll = 0;
        self.insight_kind = which;
        self.mode = Mode::Insight;
        let _ = self.tx.send(Request::Insight { which });
    }

    /// A report still being read off disk by the worker.
    ///
    /// Its spinner is what tells that wait from a hang, and the loop asks this
    /// so it knows to keep waking for the frames — the same reason
    /// [`tick_share`](share) turns its spinner while a tunnel dials.
    pub(super) fn insight_loading(&self) -> bool {
        self.mode == Mode::Insight && self.insight.is_none()
    }

    /// Open the conversation view on the selected row.
    ///
    /// Read-only: it never resumes, attaches to, or types into the session —
    /// it only reads the transcript the way the report page does. A remote
    /// row's transcript is read on the machine that has it, over the same ssh
    /// channel the row arrived by.
    pub(super) fn open_conversation(&mut self) {
        if self.on_subagent() {
            self.set_status("A subagent has no transcript of its own — read the session's");
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        let host = session.remote.as_ref().and_then(|remote| {
            self.remote_hosts
                .iter()
                .find(|h| h.target == remote.host)
                .cloned()
        });
        if let Some(remote) = &session.remote
            && host.is_none()
        {
            // A remote row with no `Host` in hand is one whose spec went away
            // since the poll — nothing to ask, and the machine's name is the
            // useful part of the answer.
            self.set_status(format!(
                "{} is on {}, which this run is not connected to",
                session.display_label(),
                remote.host
            ));
            return;
        }
        self.chat = Some(ChatView {
            session: session.clone(),
            host,
            conversation: None,
            error: None,
            back: 0,
            fetching: false,
            raw: false,
            tools_open: false,
            opened: std::collections::HashSet::new(),
            search: reader::Search::default(),
            laid: None,
            visible: 0,
            dirty: false,
            hold: false,
        });
        self.mode = Mode::Conversation;
        self.fetch_chat(None);
    }

    /// Ask the worker for a window of the open conversation.
    ///
    /// `None` is the latest; `Some(seq)` is the page that ends just before that
    /// turn, which is how `u` walks backwards through a long session.
    fn fetch_chat(&mut self, before: Option<usize>) {
        let Some(view) = self.chat.as_mut() else {
            return;
        };
        view.fetching = true;
        let _ = self.tx.send(Request::Chat {
            session: Box::new(view.session.clone()),
            host: view.host.clone(),
            before,
        });
    }

    /// Fold a worker's conversation answer into the open view.
    ///
    /// An answer for a view that has since closed or moved to another row is
    /// dropped: the key is checked because `u` can still be in flight when the
    /// user picks a different session.
    pub(super) fn got_chat(
        &mut self,
        key: String,
        before: Option<usize>,
        result: Result<Box<crate::serve::chat::Conversation>, String>,
    ) {
        let Some(view) = &mut self.chat else {
            return;
        };
        if view.session.key() != key {
            return;
        }
        view.fetching = false;
        match result {
            Ok(mut page) => match before {
                // A page of older turns goes *before* the window already shown
                // — and `back` is a distance from the end, so the read position
                // survives the prepend untouched.
                Some(_) => match &mut view.conversation {
                    Some(current) => {
                        let mut older = std::mem::take(&mut page.turns);
                        older.append(&mut current.turns);
                        current.turns = older;
                        current.earlier = page.earlier;
                        current.supported &= page.supported;
                        if current.note.is_none() {
                            current.note = page.note.take();
                        }
                        // Only the new turns are laid out; the ones already
                        // shown are kept (see [`reader`]).
                        view.relayout(false);
                    }
                    None => view.conversation = Some(*page),
                },
                None => {
                    view.conversation = Some(*page);
                    // A whole new document: a kept turn could be one whose
                    // tool has since returned, so none of them are kept.
                    view.laid = None;
                }
            },
            Err(why) => view.error = Some(why),
        }
        self.needs_redraw = true;
    }

    /// A conversation still being read off disk — or off the wire — by the
    /// worker. Same contract as [`insight_loading`]: the loop keeps waking so
    /// the spinner turns.
    pub(super) fn chat_loading(&self) -> bool {
        self.mode == Mode::Conversation
            && self
                .chat
                .as_ref()
                .is_some_and(|v| v.conversation.is_none() && v.error.is_none())
    }
}

/// The state behind the conversation reader (see [`reader`]).
pub struct ChatView {
    /// The session the view is about — kept because a page of older turns is
    /// asked for on the same row the view was opened on.
    pub session: Session,
    /// The host that can read it, when the row is remote. `Some` exactly when
    /// `session.remote` is — the fetch then goes over ssh rather than to the
    /// local transcript path, which does not exist on this machine.
    pub host: Option<crate::fleet::Host>,
    /// What has been read so far. `None` while the first read is in flight;
    /// `error` says why it came back without one when it did.
    pub conversation: Option<crate::serve::chat::Conversation>,
    /// Why the read failed, when it did.
    pub error: Option<String>,
    /// Rows scrolled back from the bottom. A scrollback's zero is the end:
    /// new turns arriving while it sits there must not move what you are
    /// reading, and a prepend of older turns leaves a distance from the end
    /// exactly where it was.
    pub back: usize,
    /// A fetch is in flight — the spinner's reason to keep turning, and what
    /// keeps a second `u` from asking for the page already coming.
    pub fetching: bool,
    /// Replies shown as the markdown source they were written in, rather than
    /// rendered — `m` flips it, for the times the exact characters matter.
    pub raw: bool,
    /// Every tool call drawn in full rather than as its one line — `t`.
    pub tools_open: bool,
    /// Turns, by `seq`, whose tools are drawn the other way from `tools_open`
    /// — what `Enter` flips, one turn at a time.
    pub opened: std::collections::HashSet<usize>,
    /// `/`: the query and the rows it is on.
    pub search: reader::Search,
    /// The conversation laid out at the last frame's width, kept so a frame
    /// copies the rows it shows rather than rendering every reply again.
    pub laid: Option<reader::Laid>,
    /// Rows the last frame had for text, written by the draw — the only place
    /// it is known — for the keys to page and clamp by.
    pub visible: usize,
    /// Something `laid` was built from has changed; the next frame lays out
    /// again, keeping every turn it can.
    pub dirty: bool,
    /// The next layout keeps the top row in place even at the end; see
    /// [`ChatView::relayout`].
    pub hold: bool,
}

/// Columns the user has hidden outright, which win over the automatic
/// width-based dropping in [`columns::visible_columns`].
///
/// `$CCTOP_COLUMNS_HIDE` is the only source today and is meant to stay an
/// override once a persisted one exists: `UiPrefs` is the natural home for the
/// stored list, but it lives in `cache.rs`, which this module does not own, and
/// carries no such field yet. When it grows one, read it here and let a
/// non-empty env var take precedence.
fn hidden_columns(_prefs: &UiPrefs) -> Vec<ColumnId> {
    columns::parse_hidden(&std::env::var("CCTOP_COLUMNS_HIDE").unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::sync::mpsc::channel;

    pub(super) fn test_app() -> App {
        let (tx, rx) = channel();
        // Keep the receiver alive so sends in tests don't fail.
        std::mem::forget(rx);
        App::with_prefs(Plan::Retail, tx, UiPrefs::default())
    }

    /// The Limits pane answers "is there room to keep working". This adds the
    /// other question nobody is shown — whether the plan was worth buying —
    /// and it must stay quiet until it has enough windows to be worth saying.
    #[test]
    fn the_limits_pane_reports_unused_allowance_once_it_has_enough_windows() {
        use crate::burn::{Log, Sample, key};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = test_app();
        app.quota = Quota {
            fetched: true,
            claude: vec![crate::quota::ProfileQuota {
                profile: "default".into(),
                source: crate::config::AccountSource::Directory,
                status: crate::quota::ProviderStatus::Ok(crate::quota::ProviderQuota {
                    plan: Some("max_20x".into()),
                    windows: vec![crate::quota::Window {
                        label: "7d",
                        pct: 20,
                        duration: Some(Duration::from_secs(7 * 24 * 3600)),
                        resets_at: None,
                    }],
                    limit_reached: false,
                }),
            }],
            codex: Vec::new(),
        };

        let screen_of = |app: &mut App| {
            let mut terminal = Terminal::new(TestBackend::new(160, 40)).expect("backend");
            terminal
                .draw(|frame| {
                    render::draw(frame, app);
                })
                .expect("draw");
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };

        // One completed window is one quiet week, not a pattern, so nothing is
        // claimed from it.
        let mut log = Log::default();
        let k = key("claude", "default", "7d");
        let week = 7 * 24 * 3600;
        let push = |log: &mut Log, w: i64, peak: u32| {
            let start = 1_000_000 + w * week;
            for i in 0..10 {
                log.record(
                    k.clone(),
                    Sample {
                        at: start + i * (week / 10),
                        pct: (peak as i64 * (i + 1) / 10) as u32,
                        resets_at: Some(start + week),
                        plan: Some("max_20x".into()),
                    },
                );
            }
        };
        push(&mut log, 0, 60);
        push(&mut log, 1, 50); // opens the second window, completing the first
        app.burn = log.clone();
        assert!(
            !screen_of(&mut app).contains("unused"),
            "one completed window is not a pattern"
        );

        // A third window completes the second, and now there is something to
        // say: 40% and 50% unused, averaging 45%.
        push(&mut log, 2, 10);
        app.burn = log;
        let screen = screen_of(&mut app);
        assert!(
            screen.contains("~45% of 7d unused"),
            "the unused share never reached the pane:\n{screen}"
        );
    }

    /// The report overlay draws the text the command prints, and says so while
    /// it is still being built — the wait is a full re-parse of every
    /// transcript, and an empty box would read as a broken one.
    #[test]
    fn the_report_overlay_draws_its_text_and_says_when_it_is_working() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let screen_of = |app: &mut App| {
            let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("backend");
            terminal
                .draw(|frame| {
                    render::draw(frame, app);
                })
                .expect("draw");
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };

        let mut app = test_app();
        app.mode = Mode::Insight;
        app.insight_kind = "optimize";
        app.insight = None;
        let waiting = screen_of(&mut app);
        assert!(
            waiting.contains("This is the slow one"),
            "an empty overlay must explain itself: {waiting}"
        );

        app.insight = Some("  fix    something worth doing\n  habit  something else\n".into());
        let drawn = screen_of(&mut app);
        assert!(drawn.contains("something worth doing"), "{drawn}");
        assert!(drawn.contains("esc close"), "the way out is on the frame");

        // Scrolling past the end must not empty the frame: the clamp is what
        // stops a held-down key from leaving a box with nothing in it and no
        // sign it is still open.
        app.insight_scroll = u16::MAX;
        let scrolled = screen_of(&mut app);
        assert!(scrolled.contains("something else"), "{scrolled}");
    }

    /// A page of older turns goes *before* the window already shown — and an
    /// answer meant for a view the user has since left is dropped, not folded
    /// into whatever is open now.
    #[test]
    fn an_earlier_chat_page_prepends_and_a_stray_answer_is_dropped() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "/repo")];
        app.refilter();
        app.selected = 0;
        app.open_conversation();
        let key = app.chat.as_ref().expect("the view opened").session.key();

        let turn = |seq: usize| crate::serve::chat::Turn {
            seq,
            role: "assistant".into(),
            kind: "message".into(),
            ts: String::new(),
            text: format!("turn {seq}"),
            clipped: false,
            tools: Vec::new(),
        };
        let page = |seqs: &[usize], earlier: usize| {
            Box::new(crate::serve::chat::Conversation {
                supported: true,
                turns: seqs.iter().map(|s| turn(*s)).collect(),
                earlier,
                note: None,
            })
        };

        // The latest window arrives first, then the page `u` asked for —
        // which belongs in front of it, not after.
        app.got_chat(key.clone(), None, Ok(page(&[3, 4, 5], 7)));
        app.got_chat(key.clone(), Some(3), Ok(page(&[0, 1, 2], 0)));
        let conv = app
            .chat
            .as_ref()
            .and_then(|v| v.conversation.as_ref())
            .expect("the conversation arrived");
        assert_eq!(
            conv.turns.iter().map(|t| t.seq).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5]
        );
        assert_eq!(conv.earlier, 0);

        // An answer addressed to another row's key — a fetch still in flight
        // from a view since closed — must not land in this one.
        app.got_chat("other".into(), None, Ok(page(&[9], 0)));
        assert_eq!(
            app.chat
                .as_ref()
                .and_then(|v| v.conversation.as_ref())
                .map(|c| c.turns.len()),
            Some(6)
        );

        // And an error is shown rather than an empty box pretending to load.
        app.got_chat(key, None, Err("no such session".into()));
        assert_eq!(
            app.chat.as_ref().and_then(|v| v.error.as_deref()),
            Some("no such session")
        );
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

    pub(super) fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    pub(super) fn session(id: &str, running: bool, label: &str) -> Session {
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
    fn the_root_pid_excludes_ghost_and_child_processes() {
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

        assert_eq!(session.root_pid(), Some(3));
    }

    #[test]
    fn age_filter_roundtrips_through_prefs_keys() {
        for f in [AgeFilter::Day, AgeFilter::Week, AgeFilter::Month] {
            assert_eq!(AgeFilter::parse(f.key()), Some(f));
        }
        assert_eq!(AgeFilter::parse("nope"), None);
    }

    /// An empty table before the first load means "still looking", and the
    /// table draws a different thing for each.
    #[test]
    fn sessions_are_not_reported_empty_before_the_first_load() {
        let app = test_app();
        assert!(!app.loaded);
        assert!(app.sessions.is_empty());
    }

    /// Rebinding from the panel: Enter on a keybind, then the new key, lands
    /// in the file and works on the dashboard at once — and the panel draws
    /// what it wrote.
    #[test]
    fn a_key_bound_in_the_panel_is_saved_and_works() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("config.toml");
        let mut app = test_app();
        app.settings_file = Some(file.clone());

        app.on_key(key(KeyCode::Char(',')));
        assert_eq!(app.mode, Mode::Settings);
        let help = crate::settings::SETTINGS.len()
            + crate::settings::BINDINGS
                .iter()
                .position(|b| b.0 == "help")
                .expect("help is bindable");
        app.settings_cursor = help;
        app.on_key(key(KeyCode::Enter));
        assert!(app.settings_capture);
        app.on_key(key(KeyCode::Char('x')));
        assert!(!app.settings_capture);
        let text = std::fs::read_to_string(&file).expect("written");
        assert!(text.contains("help = \"x\""), "{text}");

        let (cols, rows) = (100u16, 50u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        terminal
            .draw(|frame| {
                render::draw(frame, &mut app);
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let screen: String = (0..rows)
            .flat_map(|y| (0..cols).map(move |x| (x, y)))
            .map(|at| buffer[at].symbol().to_string())
            .collect();
        assert!(screen.contains("help"), "the cursor's row scrolled away");

        app.on_key(key(KeyCode::Esc));
        app.on_key(key(KeyCode::Char('?')));
        assert_eq!(app.mode, Mode::List, "the old key was moved away");
        app.on_key(key(KeyCode::Char('x')));
        assert_eq!(app.mode, Mode::Help);

        // Backspace on the row puts it back.
        app.mode = Mode::Settings;
        app.on_key(key(KeyCode::Backspace));
        assert_eq!(app.settings.key_for("help"), "?");

        // A toggle flips in place.
        app.settings_cursor = 1;
        assert_eq!(crate::settings::SETTINGS[1].0, "notify");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.settings.notify, Some(true));
        assert!(
            app.notify.enabled,
            "the running cctop did not follow the file"
        );
    }
}
