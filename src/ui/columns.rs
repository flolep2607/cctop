//! Session-table column definitions: rendering, sorting, and tooltips.

use crate::session::{ActivityState, Session, Subagent, SubagentStatus};
use crate::util;
use chrono::{DateTime, Utc};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

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
        desc: "Estimated cost in the current local clock hour",
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
        width: Some(6),
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
        desc: "How much the session asks before it acts, as its own hooks reported it:\nask, edits (writes files unasked), plan (cannot act), BYPASS (asks nothing).\n─ when the session has no cctop hooks installed and so cannot say.",
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

/// The columns to draw in `total` cells of width, widest-first casualties last.
///
/// Every fixed column costs its width plus a gutter whether or not it fits, so
/// past a certain narrowness the ones on the right are simply cut off by the
/// terminal — which is how MODEL, HARNESS, BRANCH and PROJECT used to vanish
/// together and leave rows no one could tell apart. Dropping by priority
/// instead means the columns that name a row are the last to go.
pub fn visible_columns(total: u16, hidden: &[ColumnId]) -> Vec<&'static Column> {
    let mut cols: Vec<&'static Column> =
        COLUMNS.iter().filter(|c| !hidden.contains(&c.id)).collect();

    // Drop the least important column until the rest fit, keeping display order.
    while cols.len() > 1 && required_width(&cols) > total {
        let victim = cols
            .iter()
            .enumerate()
            // Later columns lose ties, so the drop order stays predictable.
            .min_by_key(|(i, c)| (c.priority, std::cmp::Reverse(*i)))
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

/// Cell text for one column. Empty means "nothing worth showing".
pub fn render_cell(id: ColumnId, s: &Session, now: &DateTime<Utc>) -> String {
    match id {
        ColumnId::Status => match s.activity_state {
            ActivityState::WaitingForInput | ActivityState::ApiError => "●".into(),
            ActivityState::Working if s.is_running() => "●".into(),
            ActivityState::Working => "○".into(),
        },
        ColumnId::Last => util::relative_age(&s.last_active, now),
        ColumnId::Duration => util::session_duration(&s.started_at, &s.last_active),
        ColumnId::Cost => match s.total_cost {
            _ if !s.cost_available => "─".into(),
            _ if s.cost_is_free => "FREE".into(),
            Some(c) => util::compact_usd(c),
            None => "incl".into(),
        },
        ColumnId::CostHour => {
            if !s.cost_available {
                "─".into()
            } else if s.cost_is_free {
                "FREE".into()
            } else if s.total_cost.is_none() {
                "incl".into()
            } else if s.cost_hour > 0.0 {
                util::compact_usd(s.cost_hour)
            } else {
                "─".into()
            }
        }
        ColumnId::CostToday => {
            if !s.cost_available {
                "─".into()
            } else if s.cost_is_free {
                "FREE".into()
            } else if s.total_cost.is_none() {
                "incl".into()
            } else if s.cost_today > 0.0 {
                util::compact_usd(s.cost_today)
            } else {
                "─".into()
            }
        }
        ColumnId::Context => match &s.context {
            None => "─".into(),
            // Only while something is there to finish it. A session that
            // compacted and stopped keeps its last measured percentage, which is
            // what the context panel breaks down for the same session.
            Some(_) if s.is_compacting() => "COMPCT".into(),
            Some(c) => {
                let pct = c.percent_to_compact().round() as i64;
                if pct > 100 {
                    ">100%".into()
                } else {
                    format!("{pct}%")
                }
            }
        },
        ColumnId::Cpu => match &s.process {
            Some(p) => format!("{:.1}", p.cpu),
            None => "─".into(),
        },
        ColumnId::Memory => s
            .process
            .as_ref()
            .map(|p| util::compact_bytes(p.memory))
            .unwrap_or_default(),
        ColumnId::Tools => {
            if s.tool_count > 0 {
                s.tool_count.to_string()
            } else {
                String::new()
            }
        }
        // Zero is drawn as a dash rather than as `0%`: a clean session is the
        // norm, and a column of noughts is a column nobody reads.
        ColumnId::Errors => match s.error_rate() {
            None | Some(0.0) => "─".into(),
            Some(rate) => format!("{}%", (rate * 100.0).round() as i64),
        },
        ColumnId::TokenTotal => {
            let total = s.input_tokens + s.output_tokens;
            if total > 0 {
                util::compact_tokens(total)
            } else {
                String::new()
            }
        }
        ColumnId::TokenRate => {
            if s.tokens_per_min > 0.0 {
                util::compact_tokens(s.tokens_per_min.round() as u64)
            } else {
                String::new()
            }
        }
        ColumnId::Model => util::short_model(&s.model),
        ColumnId::Harness => {
            if s.harness.is_empty() {
                "─".into()
            } else {
                s.harness.clone()
            }
        }
        // A session with no hooks cannot report this, and "─" is the honest
        // answer: not "it asks about everything", which would be a guess about
        // the one column whose whole job is not to guess.
        ColumnId::Permission => match s.permission {
            Some(p) => p.label().into(),
            None => "─".into(),
        },
        // Blank rather than a dash for the ordinary case. This column is a
        // warning light, and a light that is on in every row is off.
        ColumnId::Conflict => match s.conflict {
            Some(crate::collide::Overlap::File) => "⚠".into(),
            Some(crate::collide::Overlap::Directory) => "·".into(),
            None => String::new(),
        },
        ColumnId::Branch => branch_of(s).unwrap_or_else(|| "─".into()),
        ColumnId::Project => s.display_label().to_string(),
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
        ColumnId::Context => match &sub.context {
            Some(c) => format!("{}%", c.percent_to_compact().round() as i64),
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

// ---------------------------------------------------------------------------
// Git branch
// ---------------------------------------------------------------------------

/// How long a branch reading is trusted.
///
/// A checkout is a rare event and a name that is a few seconds out of date is
/// harmless; walking the filesystem once per row per frame is not.
const BRANCH_TTL: Duration = Duration::from_secs(15);

/// A branch reading, and when it was taken. `None` is a real answer — the
/// directory is not in a repository — and is cached just as a name is.
type Reading = (Option<String>, Instant);

/// Working directory -> its last reading.
static BRANCHES: LazyLock<Mutex<HashMap<String, Reading>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Branch checked out in a session's working directory, or `None` when it is
/// not in a repository. Cached, so the filter can ask per row per keystroke.
/// Sort order for the permission column: looser is greater, so a descending
/// sort puts the sessions asking least at the top. An unhooked session sorts
/// below every known mode rather than among them — it is an absence, not a
/// setting.
fn permission_rank(s: &crate::session::Session) -> u8 {
    use crate::hook::Permission;
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
fn conflict_rank(s: &crate::session::Session) -> u8 {
    match s.conflict {
        None => 0,
        Some(crate::collide::Overlap::Directory) => 1,
        Some(crate::collide::Overlap::File) => 2,
    }
}

pub fn branch_of(s: &crate::session::Session) -> Option<String> {
    branch(&s.label_source)
}

/// Branch of a working directory, cached.
///
/// `git rev-parse` would be a subprocess per row per frame, and `git2` is a C
/// library and a build step for what is one short file in a documented format.
fn branch(dir: &str) -> Option<String> {
    if dir.is_empty() {
        return None;
    }
    let mut cache = BRANCHES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((branch, at)) = cache.get(dir)
        && at.elapsed() < BRANCH_TTL
    {
        return branch.clone();
    }
    let branch = read_head(Path::new(dir));
    cache.insert(dir.to_string(), (branch.clone(), Instant::now()));
    branch
}

/// Read the branch straight out of the repository's `HEAD`.
///
/// Three shapes have to survive this: an ordinary `.git` directory; a `.git`
/// *file* holding a `gitdir:` pointer, which is what a linked worktree or a
/// submodule has and where the interesting branches usually live; and a HEAD
/// carrying a commit id instead of a ref, as during a rebase or a bisect —
/// reported as a short id, because "not on a branch" is itself worth seeing.
fn read_head(start: &Path) -> Option<String> {
    // A session is often launched from a subdirectory of its repository.
    let git = start
        .ancestors()
        .map(|dir| dir.join(".git"))
        .find(|p| p.exists())?;
    let head = if git.is_dir() {
        git.join("HEAD")
    } else {
        let pointer = std::fs::read_to_string(&git).ok()?;
        let target = PathBuf::from(pointer.trim().strip_prefix("gitdir:")?.trim());
        // Absolute for a worktree, relative to the containing directory for a
        // submodule.
        let target = if target.is_absolute() {
            target
        } else {
            git.parent()?.join(target)
        };
        target.join("HEAD")
    };

    let head = std::fs::read_to_string(head).ok()?;
    let head = head.trim();
    if let Some(name) = head.strip_prefix("ref: refs/heads/") {
        Some(name.to_string())
    } else if let Some(other) = head.strip_prefix("ref: ") {
        // Some other ref namespace; show it whole rather than guess at a name.
        Some(other.to_string())
    } else if head.is_empty() {
        None
    } else {
        // Detached. Take chars, not bytes: a corrupt HEAD must not panic here.
        Some(std::iter::once('@').chain(head.chars().take(7)).collect())
    }
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
        // Sessions outside a repository sort together, below every branch.
        ColumnId::Branch => branch(&a.label_source).cmp(&branch(&b.label_source)),
        ColumnId::Project => a
            .display_label()
            .to_ascii_lowercase()
            .cmp(&b.display_label().to_ascii_lowercase()),
    }
    // Stable tiebreak so rows never swap places between identical refreshes.
    .then_with(|| age_secs(a, now).cmp(&age_secs(b, now)))
    .then_with(|| a.session_id.cmp(&b.session_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    fn session(id: &str) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.started_at = "2026-01-01T00:00:00Z".into();
        s.last_active = "2026-01-01T00:00:00Z".into();
        s
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
        assert_eq!(visible_columns(200, &[]).len(), COLUMNS.len());

        let ids = |w| -> Vec<ColumnId> { visible_columns(w, &[]).iter().map(|c| c.id).collect() };
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

    #[test]
    fn explicit_hidden_columns_win_over_automatic_dropping() {
        let hidden = parse_hidden("cpu, mem,nonsense");
        assert_eq!(hidden, vec![ColumnId::Cpu, ColumnId::Memory]);
        // Hidden at any width, including one where everything else fits.
        let ids: Vec<ColumnId> = visible_columns(500, &hidden).iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), COLUMNS.len() - 2);
        assert!(!ids.contains(&ColumnId::Cpu));
        // The flexible column can't be hidden: it has no width to give back.
        assert!(parse_hidden("project").is_empty());
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
        assert_eq!(render_cell(ColumnId::Cost, &s, &now), "FREE");
        assert_eq!(render_cell(ColumnId::CostHour, &s, &now), "FREE");
        assert_eq!(render_cell(ColumnId::CostToday, &s, &now), "FREE");
    }

    #[test]
    fn compacting_sessions_sort_highest_on_context() {
        let now = Utc::now();
        let mut a = session("a");
        a.inferred_running = true;
        a.context = Some(crate::session::ContextUsage {
            used: 10,
            max: 200_000,
            compacted: true,
        });
        let mut b = session("b");
        b.context = Some(crate::session::ContextUsage {
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

    /// The three HEAD shapes cctop actually meets: a checkout, a linked
    /// worktree pointing elsewhere, and a detached HEAD mid-rebase.
    #[test]
    fn head_is_read_for_a_checkout_a_worktree_and_a_detached_commit() {
        let root = std::env::temp_dir().join(format!("cctop-branch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/feat/idle\n").unwrap();
        assert_eq!(read_head(&repo).as_deref(), Some("feat/idle"));

        // From a subdirectory, since that is where agents are usually started.
        let deep = repo.join("src/ui");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(read_head(&deep).as_deref(), Some("feat/idle"));

        // A linked worktree: `.git` is a file pointing at the real git dir.
        let gitdir = repo.join(".git/worktrees/wt");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/other\n").unwrap();
        let worktree = root.join("wt");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        assert_eq!(read_head(&worktree).as_deref(), Some("other"));

        std::fs::write(repo.join(".git/HEAD"), "0123456789abcdef0123\n").unwrap();
        assert_eq!(read_head(&repo).as_deref(), Some("@0123456"));

        assert_eq!(read_head(&root.join("nowhere")), None);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_session_outside_a_repository_shows_no_branch() {
        let now = Utc::now();
        let mut s = session("a");
        s.label_source = "/nonexistent/definitely/not/a/repo".into();
        assert_eq!(render_cell(ColumnId::Branch, &s, &now), "─");
    }

    #[test]
    fn context_cell_flags_compaction_and_overflow() {
        let now = Utc::now();
        let mut s = session("a");
        s.inferred_running = true;
        s.context = Some(crate::session::ContextUsage {
            used: 0,
            max: 200_000,
            compacted: true,
        });
        assert_eq!(render_cell(ColumnId::Context, &s, &now), "COMPCT");
        s.context = Some(crate::session::ContextUsage {
            used: 400_000,
            max: 200_000,
            compacted: false,
        });
        assert_eq!(render_cell(ColumnId::Context, &s, &now), ">100%");
    }

    /// A transcript that ends on a compaction never changes again, so a session
    /// that compacted and then stopped would claim to be compacting for as long
    /// as cctop listed it — and being pinned to the top of CTX% by that claim,
    /// it would push every live session off the screen.
    #[test]
    fn a_stopped_session_that_compacted_no_longer_claims_to_be_compacting() {
        let now = Utc::now();
        let mut stopped = session("a");
        stopped.context = Some(crate::session::ContextUsage {
            used: 100_000,
            max: 200_000,
            compacted: true,
        });
        assert_eq!(render_cell(ColumnId::Context, &stopped, &now), "60%");

        let mut live = session("b");
        live.inferred_running = true;
        live.context = Some(crate::session::ContextUsage {
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
