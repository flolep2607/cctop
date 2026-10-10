//! Session-table column definitions: rendering, sorting, and tooltips.

use cctop_core::hook::Permission;
use cctop_core::session::{ActivityState, Session, Subagent, SubagentStatus};
use cctop_core::util;
use chrono::{DateTime, Utc};
use std::borrow::Cow;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnId {
    Status,
    Last,
    Duration,
    Cost,
    CostHour,
    CostToday,
    Context,
    Cpu,
    Memory,
    Tools,
    Errors,
    TokenTotal,
    TokenRate,
    Model,
    Harness,
    Permission,
    Conflict,
    Host,
    User,
    Profile,
    Branch,
    Project,
}

pub struct Column {
    pub id: ColumnId,
    pub label: &'static str,
    /// `None` for the flexible column that absorbs leftover width.
    pub width: Option<u16>,
    pub right_align: bool,
    /// What survives a narrow terminal. Higher stays; the lowest goes first.
    ///
    /// The ranking is "what identifies a row" over "what measures it": a row
    /// with no project and no status is unreadable, while a row without TOK/m
    /// is merely less informative. See [`visible_columns`].
    pub priority: u8,
    pub desc: &'static str,
}

/// Smallest usable width for the flexible column, matching `table::column_widths`.
const MIN_FLEX: u16 = 8;

/// Columns shown in the table, in display order. Also the sortable set.
pub const COLUMNS: &[Column] = &[
    Column {
        id: ColumnId::Status,
        label: " ",
        // Two wide so a subagent's dot can sit one cell in from its parent's.
        // The tree glyph that says "child" lives out in the Project column, and
        // reading the far side of the row to find out what the near side is
        // describing is the confusion this indent removes.
        width: Some(2),
        priority: 95,
        right_align: false,
        desc: "Status: ● working (green = fresh, greyer = idle), amber ● awaiting input, red ● API error, ○ stopped; indented one cell for a subagent",
    },
    Column {
        id: ColumnId::Last,
        label: "LAST",
        width: Some(5),
        priority: 90,
        right_align: true,
        desc: "Time since last activity",
    },
    Column {
        id: ColumnId::Duration,
        label: "DUR",
        width: Some(6),
        priority: 55,
        right_align: true,
        desc: "Session duration (first to last activity)",
    },
    Column {
        id: ColumnId::Cost,
        label: "$",
        width: Some(9),
        priority: 85,
        right_align: true,
        desc: "Estimated cost from per-token API pricing (LiteLLM).\nFlat-rate plans (Max, Pro, Team) bill differently,\nso this may not match your invoice.",
    },
    Column {
        id: ColumnId::CostHour,
        label: "$/1H",
        width: Some(7),
        priority: 40,
        right_align: true,
        desc: "Estimated cost in the last 60 minutes, rolling",
    },
    Column {
        id: ColumnId::CostToday,
        label: "$/24H",
        width: Some(7),
        priority: 35,
        right_align: true,
        desc: "Estimated cost since midnight (local time)",
    },
    Column {
        id: ColumnId::Context,
        label: "CTX%",
        // Two cells of bar plus `>100%` after it: seven cells is the whole
        // answer, and one more — a third cell of bar — would push DUR, the
        // next column by priority, out of the idle frame, whose rows are then
        // 119 cells wide against the 118 it has (see
        // `the_idle_view_keeps_duration_at_120`).
        width: Some(7),
        priority: 60,
        right_align: true,
        desc: "Context window used, as a share of the auto-compact threshold",
    },
    Column {
        id: ColumnId::Cpu,
        label: "CPU%",
        width: Some(5),
        priority: 50,
        right_align: true,
        desc: "CPU usage across the session's process tree",
    },
    Column {
        id: ColumnId::Memory,
        label: "MEM",
        width: Some(6),
        priority: 45,
        right_align: true,
        desc: "Resident memory across the session's process tree",
    },
    Column {
        id: ColumnId::Tools,
        label: "TOOLS",
        width: Some(6),
        priority: 30,
        right_align: true,
        desc: "Total tool invocations in the session",
    },
    Column {
        id: ColumnId::Errors,
        label: "ERR%",
        width: Some(5),
        // Above TOOLS, which it qualifies: a session's call count says how busy
        // it has been, and this says how much of that was work.
        priority: 32,
        right_align: true,
        desc: "Share of this session's tool calls the transcript reported as failed.\nA session stuck retrying is the one spending money without moving.\n─ where the harness records no per-call outcome (Cursor, Pi, Windsurf),\nand for a session that has made no calls yet.",
    },
    Column {
        id: ColumnId::TokenTotal,
        label: "TOKENS",
        width: Some(8),
        priority: 25,
        right_align: true,
        desc: "Total input and output tokens used by the session",
    },
    Column {
        id: ColumnId::TokenRate,
        label: "TOK/m",
        width: Some(7),
        priority: 20,
        right_align: true,
        desc: "Token rate per minute (exponential moving average)",
    },
    Column {
        id: ColumnId::Model,
        label: "MODEL",
        width: Some(14),
        priority: 70,
        right_align: false,
        desc: "Model used by the session",
    },
    Column {
        id: ColumnId::Harness,
        label: "HARNESS",
        width: Some(10),
        priority: 65,
        right_align: false,
        desc: "Where the agent is hosted, such as Cursor or a terminal CLI",
    },
    Column {
        id: ColumnId::Permission,
        label: "PERM",
        width: Some(6),
        // Above the measurements but below what names a row: it is a safety
        // fact, and the whole point is that it stays visible when the window
        // narrows and the numbers start dropping off.
        priority: 72,
        right_align: false,
        desc: "How much the session asks before it acts, as its own hooks reported it:\n● BYPASS — or yolo, cctop allowing every prompt it raises (the row menu\nswitches it), ◐ ask or edits, ○ plan (cannot act), each shape filling by\nhow little it asks. ─ when the session has no cctop hooks and cannot say.",
    },
    Column {
        id: ColumnId::Conflict,
        label: "!",
        width: Some(1),
        // Above PERM, and for a stronger version of the same reason: a
        // permission mode is a standing setting, while this is a fault
        // happening now. One cell is a cheap thing to keep on a narrow screen.
        priority: 74,
        right_align: false,
        desc: "Another running agent is working the same ground:\n⚠ it has written a file this session also wrote,\n· it is in the same repository.\nWorktrees count as separate repositories, which is the point of them.",
    },
    Column {
        id: ColumnId::Host,
        label: "HOST",
        width: Some(10),
        // Just under BRANCH. Which machine a row is on is part of naming it —
        // two checkouts of one repository on two boxes are otherwise the same
        // row twice — but the branch identifies it more sharply.
        priority: 67,
        right_align: false,
        desc: "Machine the session is on, for rows read from another over ssh (--host).\nlocal for this one. Hidden unless a --host is configured.\n↑ that machine runs an older cctop; Enter on the row offers to update it.",
    },
    Column {
        id: ColumnId::User,
        label: "USER",
        width: Some(9),
        // Alongside HOST, and for the same reason: with other users' sessions
        // on screen, whose a row is part of naming it — two people in the same
        // repository are otherwise the same row twice.
        priority: 67,
        right_align: false,
        desc: "User whose home the session was read from, when cctop is watching\nevery user (running as root, or $CCTOP_ALL_USERS). Shown only while\nmore than one user's sessions are in view. Filter with user:<name>.",
    },
    Column {
        id: ColumnId::Profile,
        label: "PROFILE",
        width: Some(9),
        // Just under USER, and for the neighbouring reason: USER tells two
        // people's rows apart, this tells one person's two logins apart. A
        // personal session and a work one in the same checkout are otherwise
        // identical rows with different bills.
        priority: 66,
        right_align: false,
        desc: "Claude profile the session was read from — the $CLAUDE_CONFIG_DIR\nit belongs to, named by its directory (~/.claude is `default`).\nEach is a separate account with its own subscription and limits.\nBlank for harnesses that have no such concept.\nHidden unless you have more than one.",
    },
    Column {
        id: ColumnId::Branch,
        label: "BRANCH",
        width: Some(12),
        priority: 68,
        right_align: false,
        desc: "Git branch checked out in the session's working directory,\nor @<commit> when HEAD is detached",
    },
    Column {
        id: ColumnId::Project,
        label: "PROJECT",
        width: None,
        priority: 100,
        right_align: false,
        desc: "Session title if renamed, otherwise the working directory",
    },
];

/// Stable configuration name for a column, used by `$CCTOP_COLUMNS_HIDE` and
/// by the persisted preferences. Kept separate from `label`, which is what the
/// header shows and is free to change with the layout.
pub fn key(id: ColumnId) -> &'static str {
    match id {
        ColumnId::Status => "status",
        ColumnId::Last => "active",
        ColumnId::Duration => "duration",
        ColumnId::Cost => "cost",
        ColumnId::CostHour => "cost_hour",
        ColumnId::CostToday => "cost_today",
        ColumnId::Context => "ctx",
        ColumnId::Cpu => "cpu",
        ColumnId::Memory => "mem",
        ColumnId::Tools => "tools",
        ColumnId::Errors => "errors",
        ColumnId::TokenTotal => "tokens",
        ColumnId::TokenRate => "tok_rate",
        ColumnId::Model => "model",
        ColumnId::Harness => "harness",
        ColumnId::Permission => "perm",
        ColumnId::Conflict => "conflict",
        ColumnId::Host => "host",
        ColumnId::User => "user",
        ColumnId::Profile => "profile",
        ColumnId::Branch => "branch",
        ColumnId::Project => "project",
    }
}

/// Columns the user has hidden by hand: a comma-separated list of the keys
/// above, e.g. `tok_rate,mem`. Unknown names are ignored rather than refused.
pub fn parse_hidden(list: &str) -> Vec<ColumnId> {
    list.split(',')
        .filter_map(|name| {
            let name = name.trim();
            COLUMNS
                .iter()
                .find(|c| key(c.id).eq_ignore_ascii_case(name))
        })
        // The flexible column is the row's identity and has no fixed width to
        // reclaim, so hiding it would only break the layout.
        .filter(|c| c.width.is_some())
        .map(|c| c.id)
        .collect()
}

/// How many users' sessions are in `sessions`, this user counting as one.
///
/// What decides whether USER is drawn and whether the tree gets a user level.
/// Asked of the data rather than of [`cctop_core::config::OTHER_HOMES`], because a
/// root that reads eight homes, of which only one holds any sessions, has one
/// user on screen — and a column naming that one on every row answers nothing.
pub fn users_in_view<'a>(sessions: impl IntoIterator<Item = &'a Session>) -> usize {
    // Two slots rather than a Vec: this is asked per frame by the table, and the
    // answer is never more than two, so a heap for it would be one allocation a
    // frame to hold a pair of borrowed names.
    let mut seen: [Option<&str>; 2] = [None, None];
    let mut n = 0;
    for s in sessions {
        let owner = s.owner.as_deref();
        if !seen[..n].contains(&owner) {
            seen[n] = owner;
            n += 1;
            // Two is the whole question; counting on is a walk for nothing.
            if n > 1 {
                break;
            }
        }
    }
    n
}

/// [`visible_columns`] with the columns the data itself has nothing to put in.
///
/// That is the USER column while there is one user in view, and it is asked of
/// the sessions rather than folded into `hidden` first, because the table asks
/// on every frame: it used to build a `Vec` of hidden columns per frame to
/// carry a fact that is one column being in or out.
///
/// Separate from the list the user and `$CCTOP_COLUMNS_HIDE` control, because
/// it changes as sessions come and go: another user's first session, landing
/// mid-run, has to bring USER in without anyone asking.
pub fn visible_columns_among(
    total: u16,
    hidden: &[ColumnId],
    keep: &[ColumnId],
    sessions: &[Session],
) -> Vec<&'static Column> {
    let one_user = users_in_view(sessions) < 2;
    layout(total, hidden, keep, |c| one_user && c.id == ColumnId::User)
}

/// The columns to draw in `total` cells of width, widest-first casualties last.
///
/// Every fixed column costs its width plus a gutter whether or not it fits, so
/// past a certain narrowness the ones on the right are simply cut off by the
/// terminal — which is how MODEL, HARNESS, BRANCH and PROJECT used to vanish
/// together and leave rows no one could tell apart. Dropping by priority
/// instead means the columns that name a row are the last to go.
///
/// `keep` is dropped last whatever its priority, for a view that exists to
/// show one figure: the idle view is sorted by memory, and MEM is among the
/// first columns a narrow table gives up. A column the user hid stays hidden —
/// `keep` outranks width, not their choice.
///
/// This is the plain form, with nothing ruled out by the data; the table asks
/// [`visible_columns_among`], and the tests ask for this.
#[cfg(test)]
pub fn visible_columns(total: u16, hidden: &[ColumnId], keep: &[ColumnId]) -> Vec<&'static Column> {
    layout(total, hidden, keep, |_| false)
}

/// [`visible_columns`]'s rule, with the columns the data itself rules out
/// dropped through `drop` as well.
fn layout(
    total: u16,
    hidden: &[ColumnId],
    keep: &[ColumnId],
    drop: impl Fn(&Column) -> bool,
) -> Vec<&'static Column> {
    let mut cols: Vec<&'static Column> = COLUMNS
        .iter()
        .filter(|c| !hidden.contains(&c.id) && !drop(c))
        .collect();

    // Drop the least important column until the rest fit, keeping display order.
    while cols.len() > 1 && required_width(&cols) > total {
        let victim = cols
            .iter()
            .enumerate()
            // Later columns lose ties, so the drop order stays predictable.
            .min_by_key(|(i, c)| (keep.contains(&c.id), c.priority, std::cmp::Reverse(*i)))
            .map(|(i, _)| i);
        match victim {
            Some(i) => cols.remove(i),
            None => break,
        };
    }
    cols
}

/// Cells needed to show `cols` without clipping: fixed widths, single-space
/// gutters, and a usable minimum for the flexible column.
fn required_width(cols: &[&'static Column]) -> u16 {
    let fixed: u16 = cols.iter().map(|c| c.width.unwrap_or(MIN_FLEX)).sum();
    fixed + cols.len().saturating_sub(1) as u16
}

/// Seconds since a session last did anything.
fn age_secs(s: &Session, now: &DateTime<Utc>) -> Option<i64> {
    util::parse_ts(&s.last_active).map(|d| (now.timestamp() - d.timestamp()).max(0))
}

/// Cells of shade in the bar in front of a context percentage.
///
/// Two, because that is what fits: the tallest cell this can print is a full
/// bar and `>100%` — seven cells, which is the width of the column. A third
/// cell of bar needs eight, and the idle frame — 118 cells to spend, memory
/// kept — is 119 wide with DUR in it at eight, so DUR would be the column
/// that goes (see `the_idle_view_keeps_duration_at_120`). Two cells still
/// give three shapes, empty, half and full; the number after them is the
/// exact figure, and the colour of the same cell is graded from that number.
const CONTEXT_BAR: usize = 2;

/// A context percentage as a bar of shade in front of the number it counts.
///
/// `pct` is `percent_to_compact()` — the share of the auto-compact threshold
/// that the cell's colour and the context panel are read from — so the bar
/// cannot disagree with either. Above the threshold the bar is full and the
/// `>` before the number says the rest; the `COMPCT` cell covers the other case.
///
/// Fixed-width blocks, no emoji, and no reliance on the palette: in the mono
/// theme every colour in the application is `Reset`, so the pressure a row is
/// under has to be readable from the shape alone, which `▓` against `░` is.
fn context_cell(pct: f64) -> String {
    let rounded = pct.round() as i64;
    let used = rounded.clamp(0, 100) as f64 / 100.0 * CONTEXT_BAR as f64;
    let filled = (used.round() as usize).min(CONTEXT_BAR);
    let number = if rounded > 100 {
        ">100%".to_string()
    } else {
        format!("{rounded}%")
    };
    format!(
        "{}{}{number}",
        "▓".repeat(filled),
        "░".repeat(CONTEXT_BAR - filled),
    )
}

/// A row's ahead/behind counts as the tail of its branch cell.
///
/// `None` covers both ends of the question: a session with no checkout or no
/// upstream has nothing to say, and so does one that is exactly in step — a
/// mark that reads `↑0 ↓0` on every row in a clean working list is noise. The
/// counts come from [`cctop_core::branch::ahead_behind_of`], which measures a
/// checkout against its own upstream rather than this machine's at the same
/// path, so remote and stopped-sandboxed rows have no mark either.
pub fn branch_mark(s: &Session) -> Option<String> {
    let (ahead, behind) = cctop_core::branch::ahead_behind_of(s)?;
    match (ahead, behind) {
        (0, 0) => None,
        (a, 0) => Some(format!("↑{a}")),
        (0, b) => Some(format!("↓{b}")),
        (a, b) => Some(format!("↑{a} ↓{b}")),
    }
}

/// Cell text for one column. Empty means "nothing worth showing".
///
/// Borrowed where the text is already on the session — the harness, the
/// profile, the label, and every fixed glyph — because this is asked per cell
/// per visible row per frame, and an owned `String` per cell is an allocation
/// per cell for text that already existed.
pub fn render_cell<'a>(id: ColumnId, s: &'a Session, now: &DateTime<Utc>) -> Cow<'a, str> {
    use Cow::{Borrowed, Owned};
    match id {
        ColumnId::Status => match s.activity_state {
            // A ring rather than a disc, so the one row that is actually
            // blocking an agent is findable without relying on colour alone.
            ActivityState::Asking => Borrowed("◉"),
            ActivityState::WaitingForInput | ActivityState::ApiError => Borrowed("●"),
            ActivityState::Working if s.is_running() => Borrowed("●"),
            ActivityState::Working => Borrowed("○"),
        },
        ColumnId::Last => Owned(util::relative_age(&s.last_active, now)),
        ColumnId::Duration => Owned(util::session_duration(&s.started_at, &s.last_active)),
        // A free session's cost is a statement, not a figure: lowercase and
        // italic (see `table::session_row`), which distinguishes it from the dash that
        // means no cost could be read at all.
        ColumnId::Cost => match s.total_cost {
            _ if !s.cost_available => Borrowed("─"),
            _ if s.cost_is_free => Borrowed("free"),
            Some(c) => Owned(util::compact_usd(c)),
            None => Borrowed("incl"),
        },
        ColumnId::CostHour => {
            if !s.cost_available {
                Borrowed("─")
            } else if s.cost_is_free {
                Borrowed("free")
            } else if s.total_cost.is_none() {
                Borrowed("incl")
            } else if s.cost_hour > 0.0 {
                Owned(util::compact_usd(s.cost_hour))
            } else {
                Borrowed("─")
            }
        }
        ColumnId::CostToday => {
            if !s.cost_available {
                Borrowed("─")
            } else if s.cost_is_free {
                Borrowed("free")
            } else if s.total_cost.is_none() {
                Borrowed("incl")
            } else if s.cost_today > 0.0 {
                Owned(util::compact_usd(s.cost_today))
            } else {
                Borrowed("─")
            }
        }
        ColumnId::Context => match &s.context {
            None => Borrowed("─"),
            // Only while something is there to finish it. A session that
            // compacted and stopped keeps its last measured percentage, which is
            // what the context panel breaks down for the same session.
            Some(_) if s.is_compacting() => Borrowed("COMPCT"),
            Some(c) => Owned(context_cell(c.percent_to_compact())),
        },
        ColumnId::Cpu => match &s.process {
            Some(p) => Owned(format!("{:.1}", p.cpu)),
            None => Borrowed("─"),
        },
        ColumnId::Memory => match &s.process {
            Some(p) => Owned(util::compact_bytes(p.memory)),
            None => Borrowed(""),
        },
        ColumnId::Tools => match s.tool_count > 0 {
            true => Owned(s.tool_count.to_string()),
            false => Borrowed(""),
        },
        // Zero is drawn as a dash rather than as `0%`: a clean session is the
        // norm, and a column of noughts is a column nobody reads.
        ColumnId::Errors => match s.error_rate() {
            None | Some(0.0) => Borrowed("─"),
            Some(rate) => Owned(format!("{}%", (rate * 100.0).round() as i64)),
        },
        ColumnId::TokenTotal => {
            let total = s.input_tokens + s.output_tokens;
            match total > 0 {
                true => Owned(util::compact_tokens(total)),
                false => Borrowed(""),
            }
        }
        ColumnId::TokenRate => match s.tokens_per_min > 0.0 {
            true => Owned(util::compact_tokens(s.tokens_per_min.round() as u64)),
            false => Borrowed(""),
        },
        ColumnId::Model => Owned(util::short_model(&s.model)),
        ColumnId::Harness => match s.harness.is_empty() {
            true => Borrowed("─"),
            false => Borrowed(s.harness.as_str()),
        },
        // One cell, three shapes, so the column's width is its own: a word per
        // mode (`yolo`, `edits`) spent a word's worth of cells on a table whose
        // numbers do not have them to spare. The shapes are the Status
        // column's family — a disc, a disc half drawn, an empty ring — so how
        // much this session asks reads as ink before it reads as a hue, which
        // is the only kind of legibility the mono palette has.
        // A session with no hooks cannot report this, and "─" is the honest
        // answer: not "it asks about everything", which would be a guess about
        // the one column whose whole job is not to guess.
        // YOLO outranks whatever mode the harness reports: whatever the agent
        // would have asked, cctop is answering yes — and it draws as the same
        // full circle BYPASS does, because in effect it is the same thing.
        // The colour rule is unchanged: the unrestricted pair are still the hot
        // rows, and it is the shape that now says which kind of unrestricted.
        ColumnId::Permission => Borrowed(match (&s.yolo, s.permission) {
            (Some(_), _) | (None, Some(Permission::Bypass)) => "●",
            (None, Some(Permission::Ask | Permission::AcceptEdits)) => "◐",
            (None, Some(Permission::Plan)) => "○",
            (None, None) => "─",
        }),
        // Blank rather than a dash for the ordinary case. This column is a
        // warning light, and a light that is on in every row is off.
        ColumnId::Conflict => match s.conflict {
            Some(cctop_core::collide::Overlap::File) => Borrowed("⚠"),
            Some(cctop_core::collide::Overlap::Directory) => Borrowed("·"),
            None => Borrowed(""),
        },
        // The marker leads rather than trails: the column is ten cells wide
        // and a host name long enough to be cut would take a trailing one
        // with it. Only a host that is behind is marked — it is the one with
        // something to do, from the row's menu.
        ColumnId::Host => match &s.remote {
            Some(r) if matches!(r.skew, Some(cctop_core::fleet::Skew::Older(_))) => {
                Owned(format!("↑{}", r.host))
            }
            Some(r) => Borrowed(r.host.as_str()),
            // Running here, working there: the arrows say both at once, and
            // trail because the host is the part worth keeping when cut.
            None => match &s.sandbox {
                Some(sandbox) => Owned(format!("{}⇄", cctop_core::sandbox::host_of(sandbox))),
                None => Borrowed("local"),
            },
        },
        // Named for your own rows too. The column is only drawn once a second
        // user is in view (see `users_in_view`), and there a blank reads as
        // "nobody" rather than "you" — root's own rows most of all.
        ColumnId::User => Borrowed(cctop_core::config::user_label(s.owner.as_deref())),
        // Blank rather than a dash for a provider with no profiles: the column
        // is about Claude's config directories, and every other harness is not
        // missing one so much as not having the idea.
        ColumnId::Profile => Borrowed(s.profile.as_deref().unwrap_or_default()),
        // The branch, then how far it has diverged from its upstream. The mark
        // trails where Host's leads: a host name is the part worth keeping when
        // the cell is cut, while a mark that is cut away costs the row nothing
        // it cannot still read from the name (see `table::session_row`).
        ColumnId::Branch => match cctop_core::branch::branch_of(s) {
            Some(branch) => match branch_mark(s) {
                Some(mark) => Owned(format!("{branch} {mark}")),
                None => Owned(branch),
            },
            None => Borrowed("─"),
        },
        ColumnId::Project => Borrowed(s.display_label()),
    }
}

/// One cell of a subagent's row, under the same columns as its parent.
///
/// A subagent is not a session and most columns have no answer for it: it runs
/// inside the parent's process, so it has no CPU or memory of its own, and its
/// branch and project are the parent's. Those read as `─` rather than repeating
/// the parent's figure down every child row, which would look like the cost of
/// the session had multiplied.
///
/// `last` is the tree glyph plus the agent's type and description, because the
/// Project column is where the eye already looks for what a row *is*.
pub fn render_subagent_cell(
    id: ColumnId,
    sub: &Subagent,
    last: bool,
    now: &DateTime<Utc>,
) -> String {
    match id {
        // Indented into the second cell of the column, so the left edge alone
        // says child-of-the-row-above without hunting for the tree glyph.
        ColumnId::Status => match sub.status {
            SubagentStatus::Running => " ●".into(),
            SubagentStatus::Done => " ○".into(),
        },
        ColumnId::Last => match &sub.last_active {
            Some(ts) => util::relative_age(ts, now),
            None => "─".into(),
        },
        ColumnId::Duration => {
            if sub.duration_ms > 0 {
                util::compact_duration(sub.duration_ms)
            } else {
                "─".into()
            }
        }
        ColumnId::Cost => {
            if sub.cost > 0.0 {
                util::compact_usd(sub.cost)
            } else {
                "─".into()
            }
        }
        // The same bar a session's row gets: one column, one reading, and a
        // child's window is measured against the same threshold as its parent's.
        ColumnId::Context => match &sub.context {
            Some(c) => context_cell(c.percent_to_compact()),
            None => "─".into(),
        },
        ColumnId::Tools => {
            if sub.tool_count > 0 {
                sub.tool_count.to_string()
            } else {
                "─".into()
            }
        }
        ColumnId::Model => util::short_model(&sub.model),
        ColumnId::Project => {
            let branch = if last { "└─" } else { "├─" };
            let what = if sub.description.is_empty() {
                sub.agent_type.clone()
            } else {
                format!("{}: {}", sub.agent_type, sub.description)
            };
            format!("{branch} {what}")
        }
        // Belongs to the parent, or is not measured per subagent. Left blank
        // rather than dashed: a dozen `─` down a child row is noise the eye has
        // to step over to reach the columns that do say something.
        ColumnId::CostHour
        | ColumnId::CostToday
        | ColumnId::Cpu
        | ColumnId::Memory
        // Counted against the parent, whose figure already includes them.
        | ColumnId::Errors
        // A subagent is on whichever machine its parent is.
        | ColumnId::Host
        // ...and belongs to whoever owns its parent.
        | ColumnId::User
        // ...and runs under whichever login started its parent.
        | ColumnId::Profile
        | ColumnId::TokenTotal
        | ColumnId::TokenRate
        | ColumnId::Harness
        // A subagent runs under whatever its parent was started with, so
        // repeating it down the children would be the same fact four times.
        | ColumnId::Permission
        // A subagent edits through its parent's process, in the parent's
        // directory: the collision is the parent's and is already on its row.
        | ColumnId::Conflict
        | ColumnId::Branch => String::new(),
    }
}

/// Sort order for the permission column: looser is greater, so a descending
/// sort puts the sessions asking least at the top. An unhooked session sorts
/// below every known mode rather than among them — it is an absence, not a
/// setting.
fn permission_rank(s: &cctop_core::session::Session) -> u8 {
    use cctop_core::hook::Permission;
    if s.yolo.is_some() {
        return 5;
    }
    match s.permission {
        None => 0,
        Some(Permission::Plan) => 1,
        Some(Permission::Ask) => 2,
        Some(Permission::AcceptEdits) => 3,
        Some(Permission::Bypass) => 4,
    }
}

/// Sort order for the conflict column: a shared file outranks a shared
/// repository, which outranks a session nobody is racing.
fn conflict_rank(s: &cctop_core::session::Session) -> u8 {
    match s.conflict {
        None => 0,
        Some(cctop_core::collide::Overlap::Directory) => 1,
        Some(cctop_core::collide::Overlap::File) => 2,
    }
}

/// Sort key for the host column, putting this machine before every other.
fn host_key(s: &cctop_core::session::Session) -> (bool, &str) {
    match (&s.remote, &s.sandbox) {
        (Some(r), _) => (true, r.host.as_str()),
        // Beside that host's own rows: what it is working on is the same
        // machine's, wherever the agent happens to run.
        (None, Some(sandbox)) => (true, cctop_core::sandbox::host_of(sandbox)),
        (None, None) => (false, ""),
    }
}

/// `a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())` without the strings.
///
/// Case folding is per byte and length-preserving, so the folded bytes order
/// two labels exactly as the folded strings did: a label that is not ASCII has
/// its bytes left alone, and those still sort by value, and after the letters
/// rather than among them.
fn cmp_folded(a: &str, b: &str) -> Ordering {
    a.bytes()
        .map(|b| b.to_ascii_lowercase())
        .cmp(b.bytes().map(|b| b.to_ascii_lowercase()))
}
/// Ordering for a column, ascending. `Session` has no total order of its own,
/// so every comparison funnels through here.
pub fn compare(id: ColumnId, a: &Session, b: &Session, now: &DateTime<Utc>) -> Ordering {
    let num = |x: f64, y: f64| x.partial_cmp(&y).unwrap_or(Ordering::Equal);
    match id {
        ColumnId::Status => a.is_running().cmp(&b.is_running()),
        // Newest-first reads as "ascending" for an age column.
        ColumnId::Last => b.last_active.cmp(&a.last_active),
        ColumnId::Duration => {
            let span = |s: &Session| {
                let start = util::parse_ts(&s.started_at)
                    .map(|d| d.timestamp())
                    .unwrap_or(0);
                let end = util::parse_ts(&s.last_active)
                    .map(|d| d.timestamp())
                    .unwrap_or(start);
                end - start
            };
            span(a).cmp(&span(b))
        }
        // Bundled sessions sort below any priced one.
        ColumnId::Cost => num(a.total_cost.unwrap_or(-1.0), b.total_cost.unwrap_or(-1.0)),
        ColumnId::CostHour => num(a.cost_hour, b.cost_hour),
        ColumnId::CostToday => num(a.cost_today, b.cost_today),
        ColumnId::Context => {
            // A compacting session is the most urgent thing on screen — but only
            // while it is running, or every session that ever ended on a
            // compaction would sit above the live ones forever.
            let rank = |s: &Session| match &s.context {
                _ if s.is_compacting() => f64::INFINITY,
                Some(c) => c.percent_to_compact(),
                None => -1.0,
            };
            num(rank(a), rank(b))
        }
        ColumnId::Cpu => num(
            a.process.as_ref().map(|p| p.cpu as f64).unwrap_or(0.0),
            b.process.as_ref().map(|p| p.cpu as f64).unwrap_or(0.0),
        ),
        ColumnId::Memory => a
            .process
            .as_ref()
            .map(|p| p.memory)
            .unwrap_or(0)
            .cmp(&b.process.as_ref().map(|p| p.memory).unwrap_or(0)),
        ColumnId::Tools => a.tool_count.cmp(&b.tool_count),
        // A harness that cannot report outcomes sorts below every one that can,
        // rather than among the clean sessions it is not known to be one of.
        ColumnId::Errors => num(
            a.error_rate().unwrap_or(-1.0),
            b.error_rate().unwrap_or(-1.0),
        ),
        ColumnId::TokenTotal => {
            (a.input_tokens + a.output_tokens).cmp(&(b.input_tokens + b.output_tokens))
        }
        ColumnId::TokenRate => num(a.tokens_per_min, b.tokens_per_min),
        ColumnId::Model => a.model.cmp(&b.model),
        ColumnId::Harness => a.harness.cmp(&b.harness),
        // Loosest first when descending, which is the order worth looking at.
        ColumnId::Permission => permission_rank(a).cmp(&permission_rank(b)),
        // Worst first when descending, which is the only order worth asking for.
        ColumnId::Conflict => conflict_rank(a).cmp(&conflict_rank(b)),
        // Local rows sort together, and first: they are the ones you can act on.
        ColumnId::Host => host_key(a).cmp(&host_key(b)),
        // Your own rows sort together, and first, for the same reason: they are
        // the ones you can act on without stepping into someone else's session.
        ColumnId::User => a.owner.cmp(&b.owner),
        ColumnId::Profile => a.profile.cmp(&b.profile),
        // Sessions outside a repository sort together, below every branch.
        ColumnId::Branch => cctop_core::branch::cmp_branch(a, b),
        ColumnId::Project => cmp_folded(a.display_label(), b.display_label()),
    }
    // Stable tiebreak so rows never swap places between identical refreshes.
    .then_with(|| age_secs(a, now).cmp(&age_secs(b, now)))
    .then_with(|| a.session_id.cmp(&b.session_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cctop_core::pricing::Provider;

    fn session(id: &str) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.started_at = "2026-01-01T00:00:00Z".into();
        s.last_active = "2026-01-01T00:00:00Z".into();
        s
    }

    /// PROFILE says which login a row belongs to, and says nothing where there
    /// is no such thing — which is every harness but Claude Code.
    #[test]
    fn the_profile_cell_names_the_login_or_stays_blank() {
        let now = chrono::Utc::now();

        let mut work = session("a");
        work.profile = Some("work".into());
        assert_eq!(render_cell(ColumnId::Profile, &work, &now), "work");

        // Codex holds accounts of its own, under `$CODEX_HOME`, so the cell
        // names them the same way — the column is not Claude's alone.
        let mut codex = Session::new(Provider::Codex, "b".into());
        codex.started_at = "2026-01-01T00:00:00Z".into();
        codex.last_active = codex.started_at.clone();
        codex.profile = Some("work".into());
        assert_eq!(render_cell(ColumnId::Profile, &codex, &now), "work");

        // A harness with no such variable is not a session missing a profile. A
        // dash would read as "unknown", which is a different and wronger claim.
        let mut cursor = Session::new(Provider::Cursor, "c".into());
        cursor.started_at = "2026-01-01T00:00:00Z".into();
        cursor.last_active = cursor.started_at.clone();
        assert_eq!(render_cell(ColumnId::Profile, &cursor, &now), "");
    }

    #[test]
    fn profiles_sort_together() {
        let (mut a, mut b) = (session("a"), session("b"));
        a.profile = Some("work".into());
        b.profile = Some("default".into());
        let now = chrono::Utc::now();
        assert_eq!(compare(ColumnId::Profile, &a, &b, &now), Ordering::Greater);
    }

    #[test]
    fn every_column_has_a_tooltip() {
        for c in COLUMNS {
            assert!(!c.desc.is_empty(), "{} has no description", c.label);
        }
        // Exactly one flexible column, or layout breaks.
        assert_eq!(COLUMNS.iter().filter(|c| c.width.is_none()).count(), 1);
    }

    /// A wide terminal loses nothing, and a narrow one keeps the columns that
    /// say *which session this is* rather than how it is doing.
    #[test]
    fn columns_drop_by_priority_as_width_shrinks() {
        assert_eq!(visible_columns(200, &[], &[]).len(), COLUMNS.len());

        let ids =
            |w| -> Vec<ColumnId> { visible_columns(w, &[], &[]).iter().map(|c| c.id).collect() };
        let narrow = ids(90);
        assert!(narrow.len() < COLUMNS.len(), "90 cells must drop something");
        for keep in [ColumnId::Status, ColumnId::Last, ColumnId::Project] {
            assert!(narrow.contains(&keep), "{keep:?} must survive 90 cells");
        }
        assert!(!narrow.contains(&ColumnId::TokenRate), "TOK/m goes first");

        // Dropping is monotonic: nothing reappears as the terminal narrows.
        let wider = ids(120);
        assert!(narrow.iter().all(|id| wider.contains(id)));

        // Even absurdly narrow, a row still says which session it is, and the
        // one-cell status dot rides along for as long as it fits.
        assert_eq!(ids(12), vec![ColumnId::Status, ColumnId::Project]);
        assert_eq!(ids(8), vec![ColumnId::Project]);
    }

    /// The frame cctop's own dashboard is drawn in: a 120-cell screen inside a
    /// bordered panel, so the columns share 118. Everything that row of the
    /// dashboard shows today still fits — with CTX% at eight cells the numbers
    /// land on the flexible column's floor exactly — and a ninth would push
    /// CPU%, the lowest-priority measurement on screen, out of the frame. This
    /// is the width CTX% is sized against rather than by taste.
    /// The dashboard frame is 118 cells wide — the Sessions panel's inner
    /// width on a 120-cell screen — and CTX% has to fit inside that without
    /// costing a column the frame shows on purpose. The drop rule keeps CPU%
    /// next to go, so it is the one to watch: widening CTX% is only free while
    /// CPU% is still there.
    #[test]
    fn the_dashboard_columns_at_120_still_include_cpu() {
        let shown = visible_columns_among(118, &[], &[], &[session("a")]);
        let ids: Vec<ColumnId> = shown.iter().map(|c| c.id).collect();
        for still_there in [
            ColumnId::Status,
            ColumnId::Last,
            ColumnId::Cost,
            ColumnId::Context,
            ColumnId::Cpu,
            ColumnId::Model,
            ColumnId::Harness,
            ColumnId::Permission,
            ColumnId::Branch,
            ColumnId::Project,
        ] {
            assert!(
                ids.contains(&still_there),
                "{still_there:?} must still fit 118 cells: {ids:?}"
            );
        }
        // One user in view, so USER is out by the data's own rule and the
        // frame keeps every column that is its own choice to show.
        assert!(!ids.contains(&ColumnId::User));
    }

    /// The same 118 cells from the idle frame, which is the tighter of the
    /// two: memory is kept there and duration is what the drop rule takes
    /// first among what is left. This is what sizes CTX% — its bar is two
    /// cells because a third would make the frame 119 wide with DUR in it, and
    /// a column of a view should not pay for a bar on another.
    #[test]
    fn the_idle_view_keeps_duration_at_120() {
        let shown = visible_columns_among(118, &[], &[ColumnId::Memory], &[session("a")]);
        let ids: Vec<ColumnId> = shown.iter().map(|c| c.id).collect();
        for still_there in [
            ColumnId::Status,
            ColumnId::Last,
            ColumnId::Duration,
            ColumnId::Cost,
            ColumnId::Context,
            ColumnId::Memory,
            ColumnId::Model,
            ColumnId::Harness,
            ColumnId::Permission,
            ColumnId::Branch,
            ColumnId::Project,
        ] {
            assert!(
                ids.contains(&still_there),
                "{still_there:?} must still fit 118 cells: {ids:?}"
            );
        }
        // CPU% is the first to go in either frame; it goes here too.
        assert!(!ids.contains(&ColumnId::Cpu));
    }

    #[test]
    fn explicit_hidden_columns_win_over_automatic_dropping() {
        let hidden = parse_hidden("cpu, mem,nonsense");
        assert_eq!(hidden, vec![ColumnId::Cpu, ColumnId::Memory]);
        // Hidden at any width, including one where everything else fits.
        let ids: Vec<ColumnId> = visible_columns(500, &hidden, &[])
            .iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids.len(), COLUMNS.len() - 2);
        assert!(!ids.contains(&ColumnId::Cpu));
        // The flexible column can't be hidden: it has no width to give back.
        assert!(parse_hidden("project").is_empty());
    }

    /// USER comes and goes with the data: one user in view hides it, a second
    /// user's session brings it in, and your own rows then carry your name.
    #[test]
    fn the_user_column_shows_only_with_two_users_in_view() {
        let mine = session("a");
        let mut theirs = session("b");
        theirs.owner = Some("winshen".into());

        let shown = |hidden: &[ColumnId], sessions: &[Session]| {
            visible_columns_among(500, hidden, &[], sessions)
                .iter()
                .any(|c| c.id == ColumnId::User)
        };
        let only_mine = [mine.clone(), mine.clone()];
        assert_eq!(users_in_view(&only_mine), 1);
        assert!(!shown(&[], &only_mine), "one user in view hides USER");
        // Somebody else's sessions alone are still one user.
        assert!(!shown(&[], std::slice::from_ref(&theirs)));

        let both = [mine.clone(), theirs.clone()];
        assert_eq!(users_in_view(&both), 2);
        assert!(shown(&[], &both), "a second user brings USER in");
        // An explicit hide still wins.
        assert!(!shown(&[ColumnId::User], &both));

        let now = Utc::now();
        assert_eq!(render_cell(ColumnId::User, &theirs, &now), "winshen");
        assert_eq!(
            render_cell(ColumnId::User, &mine, &now),
            cctop_core::config::MY_USER.as_str()
        );
    }

    #[test]
    fn bundled_cost_sorts_below_priced() {
        let now = Utc::now();
        let mut free = session("a");
        free.total_cost = None;
        let mut paid = session("b");
        paid.total_cost = Some(0.0);
        assert_eq!(compare(ColumnId::Cost, &free, &paid, &now), Ordering::Less);
    }

    #[test]
    fn free_usage_is_labeled_in_every_cost_column() {
        let mut s = session("free");
        s.cost_is_free = true;
        s.total_cost = Some(0.0);

        let now = chrono::Utc::now();
        assert_eq!(render_cell(ColumnId::Cost, &s, &now), "free");
        assert_eq!(render_cell(ColumnId::CostHour, &s, &now), "free");
        assert_eq!(render_cell(ColumnId::CostToday, &s, &now), "free");
    }

    #[test]
    fn compacting_sessions_sort_highest_on_context() {
        let now = Utc::now();
        let mut a = session("a");
        a.inferred_running = true;
        a.context = Some(cctop_core::session::ContextUsage {
            used: 10,
            max: 200_000,
            compacted: true,
        });
        let mut b = session("b");
        b.context = Some(cctop_core::session::ContextUsage {
            used: 199_000,
            max: 200_000,
            compacted: false,
        });
        assert_eq!(compare(ColumnId::Context, &a, &b, &now), Ordering::Greater);
    }

    #[test]
    fn ordering_is_total_and_stable() {
        let now = Utc::now();
        let a = session("a");
        let b = session("b");
        // Identical except for id: the tiebreak must still order them.
        assert_eq!(compare(ColumnId::Cpu, &a, &b, &now), Ordering::Less);
        assert_eq!(
            compare(ColumnId::Cpu, &a, &a.clone(), &now),
            Ordering::Equal
        );
    }

    #[test]
    fn bundled_plan_shows_incl_not_a_dash() {
        let now = Utc::now();
        let mut s = session("a");
        s.total_cost = None;
        assert_eq!(render_cell(ColumnId::Cost, &s, &now), "incl");
        assert_eq!(render_cell(ColumnId::CostHour, &s, &now), "incl");
    }

    #[test]
    fn unsupported_cursor_cost_shows_unavailable() {
        let now = Utc::now();
        let mut s = Session::new(Provider::Cursor, "cursor".into());
        s.cost_available = false;
        s.total_cost = None;
        assert_eq!(render_cell(ColumnId::Cost, &s, &now), "─");
        assert_eq!(render_cell(ColumnId::CostHour, &s, &now), "─");
        assert_eq!(render_cell(ColumnId::CostToday, &s, &now), "─");
    }

    #[test]
    fn token_total_combines_input_and_output() {
        let now = Utc::now();
        let mut s = session("a");
        s.input_tokens = 12_000;
        s.output_tokens = 345;
        assert_eq!(render_cell(ColumnId::TokenTotal, &s, &now), "12.3K");
    }

    /// The row says where the work is, and stops claiming a branch for a
    /// mount that went away with the agent.
    #[test]
    fn a_sandboxed_row_names_its_host_and_no_stale_branch() {
        let mut s =
            cctop_core::session::Session::new(cctop_core::pricing::Provider::Claude, "s".into());
        s.sandbox = Some("procdb:/home/f/x".into());
        // The checkout this test runs in has a branch; a stopped sandboxed
        // row at the same path must not report it.
        s.label_source = env!("CARGO_MANIFEST_DIR").into();
        let cell = render_cell(ColumnId::Host, &s, &chrono::Utc::now());
        assert_eq!(cell, "procdb⇄");
        assert_eq!(cctop_core::branch::branch_of(&s), None);
    }

    #[test]
    fn a_session_outside_a_repository_shows_no_branch() {
        let now = Utc::now();
        let mut s = session("a");
        s.label_source = "/nonexistent/definitely/not/a/repo".into();
        assert_eq!(render_cell(ColumnId::Branch, &s, &now), "─");
    }

    /// Folding in place has to order labels exactly as folding them into two
    /// new strings did. The interesting pairs are the ones where case folding
    /// is not what a reader would guess: a non-ASCII label, whose bytes folding
    /// leaves alone and which therefore sorts after every ASCII letter however
    /// the letter before it was capitalised, and a label that differs from
    /// another only in case.
    #[test]
    fn sorting_by_project_agrees_with_folding_both_labels() {
        let labels = [
            "", "a", "A", "ab", "AB", "aB", "Ab", "cctop", "CCTOP", "cctop-ui", "cctop UI",
            "Zebra", "zebra", "é", "école", "École", "Ω", "ωmega", "über", "0", "9", "_leading",
            "ß", "SS", "ﬀ",
        ];
        for a in labels {
            for b in labels {
                assert_eq!(
                    cmp_folded(a, b),
                    a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()),
                    "{a:?} against {b:?}"
                );
            }
        }
    }

    #[test]
    fn context_cell_flags_compaction_and_overflow() {
        let now = Utc::now();
        let mut s = session("a");
        s.inferred_running = true;
        s.context = Some(cctop_core::session::ContextUsage {
            used: 0,
            max: 200_000,
            compacted: true,
        });
        assert_eq!(render_cell(ColumnId::Context, &s, &now), "COMPCT");
        s.context = Some(cctop_core::session::ContextUsage {
            used: 400_000,
            max: 200_000,
            compacted: false,
        });
        assert_eq!(render_cell(ColumnId::Context, &s, &now), "▓▓>100%");
    }

    /// The bar and the number are one reading: the bar is the same
    /// `percent_to_compact()` the cell's colour is graded from, scaled to the
    /// two cells the column has for it, with the figure beside it rounded.
    /// Seven cells wide is the column because two of bar plus `>100%` is
    /// exactly seven — the tallest thing this cell can print.
    #[test]
    fn the_context_bar_is_the_percentage_it_shows() {
        let now = Utc::now();
        let threshold = *cctop_core::config::COMPACT_THRESHOLD;
        assert!(
            (0.0..1.0).contains(&threshold),
            "a fraction of the window: {threshold}"
        );
        let at = |fraction: f64| {
            let mut s = session("ctx");
            s.context = Some(cctop_core::session::ContextUsage {
                used: (1_000_000.0 * fraction * threshold) as u64,
                max: 1_000_000,
                compacted: false,
            });
            render_cell(ColumnId::Context, &s, &now).into_owned()
        };
        assert_eq!(at(0.10), "░░10%", "barely warmed up");
        assert_eq!(at(0.60), "▓░60%", "one of two filled, not none");
        assert_eq!(at(1.0), "▓▓100%", "at the threshold the bar is full");
        // Past it the bar has nowhere left to grow, and the number says so.
        assert_eq!(at(1.4), "▓▓>100%");

        // Nothing to measure, and a compaction still running, are unchanged.
        let mut s = session("ctx");
        assert_eq!(render_cell(ColumnId::Context, &s, &now), "─");
        s.context = Some(cctop_core::session::ContextUsage {
            used: 1,
            max: 2,
            compacted: true,
        });
        s.inferred_running = true;
        assert_eq!(render_cell(ColumnId::Context, &s, &now), "COMPCT");

        // And the column holds the tallest cell it can print.
        let column = COLUMNS
            .iter()
            .find(|c| c.id == ColumnId::Context)
            .expect("context column");
        assert_eq!(column.width, Some(7));
        assert!(util::cells(&at(1.4)) <= 7, "the widest cell fits");
    }

    /// Three shapes and a dash for the permission column: `●` for a session
    /// that asks about nothing, `◐` for one that asks as it goes, `○` for the
    /// plan that needs approving. YOLO draws as BYPASS does — in effect the
    /// same thing — and which of them get painted red is `table::cell_color`'s
    /// rule, unchanged.
    #[test]
    fn the_permission_column_is_three_shapes_and_a_dash() {
        let now = Utc::now();
        let mut s = session("perm");
        let cell = |s: &cctop_core::session::Session| {
            render_cell(ColumnId::Permission, s, &now).into_owned()
        };
        assert_eq!(cell(&s), "─", "no hooks reported");
        s.permission = Some(cctop_core::hook::Permission::Plan);
        assert_eq!(cell(&s), "○");
        s.permission = Some(cctop_core::hook::Permission::Ask);
        assert_eq!(cell(&s), "◐");
        s.permission = Some(cctop_core::hook::Permission::AcceptEdits);
        assert_eq!(cell(&s), "◐");
        s.permission = Some(cctop_core::hook::Permission::Bypass);
        assert_eq!(cell(&s), "●");
        // YOLO outranks the configured mode.
        s.permission = Some(cctop_core::hook::Permission::Plan);
        s.yolo = Some(std::sync::Arc::new(cctop_core::yolo::Entry {
            since: now.to_rfc3339(),
            allowed: Vec::new(),
            agent: None,
        }));
        assert_eq!(cell(&s), "●");

        // Every one of them is a fixed width cell the column has room for.
        let column = COLUMNS
            .iter()
            .find(|c| c.id == ColumnId::Permission)
            .expect("permission column");
        for glyph in ["─", "○", "◐", "●"] {
            assert_eq!(util::cells(glyph), 1, "{glyph} must not depend on the font");
        }
        assert!(
            column.width.unwrap() > 1,
            "and the column has more than one"
        );
    }

    /// A transcript that ends on a compaction never changes again, so a session
    /// that compacted and then stopped would claim to be compacting for as long
    /// as cctop listed it — and being pinned to the top of CTX% by that claim,
    /// it would push every live session off the screen.
    #[test]
    fn a_stopped_session_that_compacted_no_longer_claims_to_be_compacting() {
        let now = Utc::now();
        let mut stopped = session("a");
        stopped.context = Some(cctop_core::session::ContextUsage {
            used: 100_000,
            max: 200_000,
            compacted: true,
        });
        assert_eq!(render_cell(ColumnId::Context, &stopped, &now), "▓░60%");

        let mut live = session("b");
        live.inferred_running = true;
        live.context = Some(cctop_core::session::ContextUsage {
            used: 199_000,
            max: 200_000,
            compacted: false,
        });
        assert_eq!(
            compare(ColumnId::Context, &stopped, &live, &now),
            Ordering::Less
        );
    }
}
