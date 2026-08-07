//! Session-table column definitions: rendering, sorting, and tooltips.

use crate::session::{ActivityState, Session};
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
    TokenTotal,
    TokenRate,
    Model,
    Harness,
    Branch,
    Project,
}

pub struct Column {
    pub id: ColumnId,
    pub label: &'static str,
    /// `None` for the flexible column that absorbs leftover width.
    pub width: Option<u16>,
    pub right_align: bool,
    pub desc: &'static str,
}

/// Columns shown in the table, in display order. Also the sortable set.
pub const COLUMNS: &[Column] = &[
    Column {
        id: ColumnId::Status,
        label: " ",
        width: Some(1),
        right_align: false,
        desc: "Status: ● working (green = fresh, greyer = idle), amber ● awaiting input, red ● API error, ○ stopped",
    },
    Column {
        id: ColumnId::Last,
        label: "LAST",
        width: Some(5),
        right_align: true,
        desc: "Time since last activity",
    },
    Column {
        id: ColumnId::Duration,
        label: "DUR",
        width: Some(6),
        right_align: true,
        desc: "Session duration (first to last activity)",
    },
    Column {
        id: ColumnId::Cost,
        label: "$",
        width: Some(9),
        right_align: true,
        desc: "Estimated cost from per-token API pricing (LiteLLM).\nFlat-rate plans (Max, Pro, Team) bill differently,\nso this may not match your invoice.",
    },
    Column {
        id: ColumnId::CostHour,
        label: "$/1H",
        width: Some(7),
        right_align: true,
        desc: "Estimated cost in the current local clock hour",
    },
    Column {
        id: ColumnId::CostToday,
        label: "$/24H",
        width: Some(7),
        right_align: true,
        desc: "Estimated cost since midnight (local time)",
    },
    Column {
        id: ColumnId::Context,
        label: "CTX%",
        width: Some(6),
        right_align: true,
        desc: "Context window used, as a share of the auto-compact threshold",
    },
    Column {
        id: ColumnId::Cpu,
        label: "CPU%",
        width: Some(5),
        right_align: true,
        desc: "CPU usage across the session's process tree",
    },
    Column {
        id: ColumnId::Memory,
        label: "MEM",
        width: Some(6),
        right_align: true,
        desc: "Resident memory across the session's process tree",
    },
    Column {
        id: ColumnId::Tools,
        label: "TOOLS",
        width: Some(6),
        right_align: true,
        desc: "Total tool invocations in the session",
    },
    Column {
        id: ColumnId::TokenTotal,
        label: "TOKENS",
        width: Some(8),
        right_align: true,
        desc: "Total input and output tokens used by the session",
    },
    Column {
        id: ColumnId::TokenRate,
        label: "TOK/m",
        width: Some(7),
        right_align: true,
        desc: "Token rate per minute (exponential moving average)",
    },
    Column {
        id: ColumnId::Model,
        label: "MODEL",
        width: Some(14),
        right_align: false,
        desc: "Model used by the session",
    },
    Column {
        id: ColumnId::Harness,
        label: "HARNESS",
        width: Some(10),
        right_align: false,
        desc: "Where the agent is hosted, such as Cursor or a terminal CLI",
    },
    Column {
        id: ColumnId::Branch,
        label: "BRANCH",
        width: Some(12),
        right_align: false,
        desc: "Git branch checked out in the session's working directory,\nor @<commit> when HEAD is detached",
    },
    Column {
        id: ColumnId::Project,
        label: "PROJECT",
        width: None,
        right_align: false,
        desc: "Session title if renamed, otherwise the working directory",
    },
];

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
            Some(c) if c.compacting => "COMPCT".into(),
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
        ColumnId::Branch => branch_of(s).unwrap_or_else(|| "─".into()),
        ColumnId::Project => s.display_label().to_string(),
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
            // A compacting session is the most urgent thing on screen.
            let rank = |s: &Session| match &s.context {
                Some(c) if c.compacting => f64::INFINITY,
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
        ColumnId::TokenTotal => {
            (a.input_tokens + a.output_tokens).cmp(&(b.input_tokens + b.output_tokens))
        }
        ColumnId::TokenRate => num(a.tokens_per_min, b.tokens_per_min),
        ColumnId::Model => a.model.cmp(&b.model),
        ColumnId::Harness => a.harness.cmp(&b.harness),
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
        a.context = Some(crate::session::ContextUsage {
            used: 10,
            max: 200_000,
            compacting: true,
        });
        let mut b = session("b");
        b.context = Some(crate::session::ContextUsage {
            used: 199_000,
            max: 200_000,
            compacting: false,
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
        s.context = Some(crate::session::ContextUsage {
            used: 0,
            max: 200_000,
            compacting: true,
        });
        assert_eq!(render_cell(ColumnId::Context, &s, &now), "COMPCT");
        s.context = Some(crate::session::ContextUsage {
            used: 400_000,
            max: 200_000,
            compacting: false,
        });
        assert_eq!(render_cell(ColumnId::Context, &s, &now), ">100%");
    }
}
