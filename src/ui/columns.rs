//! Session-table column definitions: rendering, sorting, and tooltips.

use crate::session::Session;
use crate::util;
use chrono::{DateTime, Utc};
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
    TokenTotal,
    TokenRate,
    Model,
    Harness,
    Project,
}

pub struct Column {
    pub id: ColumnId,
    pub key: &'static str,
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
        key: "status",
        label: " ",
        width: Some(1),
        right_align: false,
        desc: "Running status: ● running (brighter = more recent), ○ stopped",
    },
    Column {
        id: ColumnId::Last,
        key: "active",
        label: "LAST",
        width: Some(5),
        right_align: true,
        desc: "Time since last activity",
    },
    Column {
        id: ColumnId::Duration,
        key: "duration",
        label: "DUR",
        width: Some(6),
        right_align: true,
        desc: "Session duration (first to last activity)",
    },
    Column {
        id: ColumnId::Cost,
        key: "cost",
        label: "$",
        width: Some(9),
        right_align: true,
        desc: "Estimated cost from per-token API pricing (LiteLLM).\nFlat-rate plans (Max, Pro, Team) bill differently,\nso this may not match your invoice.",
    },
    Column {
        id: ColumnId::CostHour,
        key: "cost_hour",
        label: "$/1H",
        width: Some(7),
        right_align: true,
        desc: "Estimated cost in the current local clock hour",
    },
    Column {
        id: ColumnId::CostToday,
        key: "cost_today",
        label: "$/24H",
        width: Some(7),
        right_align: true,
        desc: "Estimated cost since midnight (local time)",
    },
    Column {
        id: ColumnId::Context,
        key: "ctx",
        label: "CTX%",
        width: Some(6),
        right_align: true,
        desc: "Context window used, as a share of the auto-compact threshold",
    },
    Column {
        id: ColumnId::Cpu,
        key: "cpu",
        label: "CPU%",
        width: Some(5),
        right_align: true,
        desc: "CPU usage across the session's process tree",
    },
    Column {
        id: ColumnId::Memory,
        key: "mem",
        label: "MEM",
        width: Some(6),
        right_align: true,
        desc: "Resident memory across the session's process tree",
    },
    Column {
        id: ColumnId::Tools,
        key: "tools",
        label: "TOOLS",
        width: Some(6),
        right_align: true,
        desc: "Total tool invocations in the session",
    },
    Column {
        id: ColumnId::TokenTotal,
        key: "tokens",
        label: "TOKENS",
        width: Some(8),
        right_align: true,
        desc: "Total input and output tokens used by the session",
    },
    Column {
        id: ColumnId::TokenRate,
        key: "tok_rate",
        label: "TOK/m",
        width: Some(7),
        right_align: true,
        desc: "Token rate per minute (exponential moving average)",
    },
    Column {
        id: ColumnId::Model,
        key: "model",
        label: "MODEL",
        width: Some(14),
        right_align: false,
        desc: "Model used by the session",
    },
    Column {
        id: ColumnId::Harness,
        key: "harness",
        label: "HARNESS",
        width: Some(10),
        right_align: false,
        desc: "Where the agent is hosted, such as Cursor or a terminal CLI",
    },
    Column {
        id: ColumnId::Project,
        key: "project",
        label: "PROJECT",
        width: None,
        right_align: false,
        desc: "Session title if renamed, otherwise the working directory",
    },
];

pub fn column(id: ColumnId) -> &'static Column {
    COLUMNS.iter().find(|c| c.id == id).unwrap_or(&COLUMNS[0])
}

pub fn column_by_key(key: &str) -> Option<&'static Column> {
    COLUMNS.iter().find(|c| c.key == key)
}

/// Seconds since a session last did anything.
fn age_secs(s: &Session, now: &DateTime<Utc>) -> Option<i64> {
    util::parse_ts(&s.last_active).map(|d| (now.timestamp() - d.timestamp()).max(0))
}

/// Cell text for one column. Empty means "nothing worth showing".
pub fn render_cell(id: ColumnId, s: &Session, now: &DateTime<Utc>) -> String {
    match id {
        ColumnId::Status => if s.is_running() { "●" } else { "○" }.into(),
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
        ColumnId::Project => s.display_label().to_string(),
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
    fn every_column_has_a_tooltip_and_key() {
        for c in COLUMNS {
            assert!(!c.desc.is_empty(), "{} has no description", c.key);
            assert!(!c.key.is_empty());
        }
        // Exactly one flexible column, or layout breaks.
        assert_eq!(COLUMNS.iter().filter(|c| c.width.is_none()).count(), 1);
    }

    #[test]
    fn column_keys_are_unique() {
        let mut keys: Vec<&str> = COLUMNS.iter().map(|c| c.key).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "duplicate column key");
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
