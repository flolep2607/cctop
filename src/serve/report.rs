//! One session's postmortem: where the money and the window went.
//!
//! The table answers "what is happening"; this answers "what happened", which
//! is a different question and mostly a retrospective one. The figures it needs
//! are the ones the row deliberately does not carry — every tool call with its
//! arguments, its duration and whether it failed, and the window measured at
//! every request — so it is built from a
//! [`session_data_fresh`](crate::cache::Store::session_data_fresh) parse rather
//! than off the cache. That is a full transcript read per report, which is why
//! this happens on request for one session and never for all of them.
//!
//! The three things it is built to show, in the order they are usually the
//! answer:
//!
//! - **Repeated failures.** Failed calls are grouped by tool *and by argument*,
//!   so an agent that ran the same broken command eleven times appears as one
//!   row saying eleven rather than as eleven rows nobody counts. This is the
//!   single most common way a session spends real money achieving nothing.
//! - **Where the window went.** The breakdown says what is in the context now;
//!   the series says how it got there. A window that climbed steadily is a
//!   conversation; one that jumped is a tool result that will do it again.
//! - **What it cost, split by model.** A cheap model doing the work and an
//!   expensive one doing the thinking produce the same total and very different
//!   conclusions about what to change.
//!
//! Everything here is derived, never re-measured: the report must agree with
//! the table it was opened from, so any figure both show comes from the same
//! field rather than from a second calculation that can drift.

use crate::pricing::Plan;
use crate::session::{CtxPoint, Session, SessionData, Subagent, Tokens};
use crate::util;
use serde::Serialize;
use std::collections::HashMap;

/// How many distinct failure clusters the report lists.
///
/// A session that fails in more than this many distinct ways has a problem the
/// list is not going to isolate, and the tail is a long drizzle of one-offs.
const MAX_FAILURE_CLUSTERS: usize = 12;

/// How many example calls a cluster carries.
///
/// The count is the finding; the examples are only there to make it concrete,
/// and after a few they stop adding anything.
const MAX_FAILURE_SAMPLES: usize = 3;

/// How many tools the activity table lists, busiest first.
const MAX_TOOLS: usize = 20;

/// How many of the slowest individual calls are singled out.
const MAX_SLOWEST: usize = 10;

/// Tools whose duration is a person deciding, not an agent working.
///
/// `AskUserQuestion` sitting at the top of "slowest calls" at fifty seconds is
/// not a finding about the session — it is how long somebody took to read four
/// options. Ranking it alongside a two-minute `cargo build` puts the reader's
/// own thinking time in a list they opened to find out where the *agent's* time
/// went, and buries the real answer under it.
///
/// They stay in the full call log, which is a record of what happened rather
/// than a ranking of what was slow.
///
/// Spelled in the variants the harnesses use, the way
/// [`EDIT_TOOLS`](crate::session::EDIT_TOOLS) is: a name known here but not
/// there would quietly let one back into the list.
const HUMAN_WAIT_TOOLS: &[&str] = &[
    "AskUserQuestion",
    "ask_user_question",
    "ExitPlanMode",
    "exit_plan_mode",
];

/// How many written files are listed.
const MAX_FILES: usize = 40;

/// How many files the diff section covers, most-changed first.
///
/// A session that touched more than this many files is a migration, and the
/// tail of that list is where the interesting edits are not.
const MAX_DIFF_FILES: usize = 40;

/// How many diff lines one file's entry carries.
///
/// Each edit contributes at most [`MAX_DIFF_LINES`](crate::config::MAX_DIFF_LINES);
/// a file edited twenty times would otherwise arrive as twelve hundred lines
/// nobody scrolls through.
const MAX_HUNKS_PER_FILE: usize = 300;

#[derive(Serialize)]
pub struct Report {
    pub session_id: String,
    pub provider: &'static str,
    pub title: Option<String>,
    /// Working directory, spelled with `~` — the report is a page someone may
    /// well put in front of a colleague, and an absolute home path is noise at
    /// best and their login name at worst.
    pub project: Option<String>,
    pub branch: Option<String>,
    /// Which Claude profile the session ran under, when there is one to name.
    pub profile: Option<String>,
    pub model: Option<String>,
    pub models: Vec<String>,
    pub started_at: String,
    pub last_active: String,
    /// Wall time from first record to last, already rendered — the page has no
    /// business knowing this codebase's rounding rules.
    pub duration: String,
    pub running: bool,
    pub state: &'static str,
    pub plan: &'static str,
    /// Set when extraction failed. The rest of the document still renders, with
    /// the zeroes that implies, and this says why they are zeroes.
    pub error: Option<String>,

    pub cost: ReportCost,
    pub tokens: Tokens,
    pub context: Option<ReportContext>,
    pub activity: ReportActivity,
    pub files: Vec<String>,
    /// What the session changed, per file.
    pub diffs: Vec<ReportFileDiff>,
    pub subagents: Vec<Subagent>,
}

/// One file's worth of edits, as a diff.
///
/// The question a report is opened to answer is often "what did it actually
/// do", and a list of file names does not answer it. The transcript records the
/// patch each edit applied, so the report can show the change rather than
/// describe it.
#[derive(Serialize)]
pub struct ReportFileDiff {
    pub file: String,
    pub added: u32,
    pub removed: u32,
    /// How many separate edits landed on this file. A file edited eleven times
    /// is a different story from one edited once, and the hunks alone do not
    /// tell them apart.
    pub edits: usize,
    /// Unified-diff lines, oldest edit first.
    pub hunks: Vec<String>,
    /// The hunks were cut at [`MAX_HUNKS_PER_FILE`], so this is not the whole
    /// change. Said rather than left to be inferred from a diff that stops.
    pub truncated: bool,
}

#[derive(Serialize)]
pub struct ReportCost {
    /// `false` where the transcript records no billable usage, which is not the
    /// same as a session that cost nothing.
    pub available: bool,
    /// `true` when the active plan bundles this provider, in which case `total`
    /// is what it would have cost at retail and is labelled as such.
    pub included: bool,
    pub total: f64,
    pub by_model: Vec<ReportModelCost>,
    /// `YYYY-MM-DD` -> USD, oldest first.
    pub by_day: Vec<(String, f64)>,
    /// `YYYY-MM-DDTHH` -> USD, oldest first.
    pub by_hour: Vec<(String, f64)>,
}

#[derive(Serialize)]
pub struct ReportModelCost {
    pub model: String,
    pub total: f64,
    pub tokens: Tokens,
}

#[derive(Serialize)]
pub struct ReportContext {
    pub used: u64,
    pub max: u64,
    pub percent_to_compact: f64,
    pub compactions: u32,
    /// Present for Claude Code alone; every other harness records per-request
    /// totals without saying what they were made of.
    pub breakdown: Option<ReportBreakdown>,
    /// The window at each request, oldest first.
    pub series: Vec<CtxPoint>,
}

#[derive(Serialize)]
pub struct ReportBreakdown {
    pub total: u64,
    pub startup: u64,
    pub tool_output: u64,
    pub tool_input: u64,
    pub attachments: u64,
    pub user_text: u64,
    pub assistant_text: u64,
    /// Signed, and deliberately so — see [`crate::session::ContextBreakdown::unaccounted`].
    pub unaccounted: i64,
    pub after_compaction: bool,
    pub superseded: bool,
}

#[derive(Serialize)]
pub struct ReportActivity {
    pub tool_count: u64,
    /// Absent, rather than zero, for a harness that records no per-call
    /// outcome: "none failed" and "it does not say" must not render alike.
    pub tool_errors: Option<u64>,
    pub error_rate: Option<f64>,
    pub lines_added: u64,
    pub lines_removed: u64,
    pub tools: Vec<ReportTool>,
    pub failures: Vec<ReportFailure>,
    pub slowest: Vec<ReportCall>,
    /// Every call the extraction still holds, newest first.
    ///
    /// Not every call the session made: the transcript's tool history is
    /// trimmed to [`MAX_SESSION_TOOL_DETAILS`](crate::config::MAX_SESSION_TOOL_DETAILS)
    /// newest, which is why `tool_count` above can be larger. The page says so
    /// rather than presenting this as the whole record — a log that silently
    /// stopped short would be read as a session that stopped doing things.
    pub calls: Vec<ReportCall>,
}

#[derive(Serialize)]
pub struct ReportTool {
    pub name: String,
    pub calls: u64,
    pub failed: u64,
}

/// Failed calls that were the *same* call, counted.
#[derive(Serialize)]
pub struct ReportFailure {
    pub tool: String,
    /// The argument the calls shared, which is what makes them one finding.
    pub detail: String,
    pub count: u64,
    pub samples: Vec<ReportCall>,
}

#[derive(Serialize)]
pub struct ReportCall {
    pub tool: String,
    pub detail: String,
    pub ts: String,
    pub duration_ms: Option<i64>,
    pub failed: bool,
}

/// Assemble the report for one session.
pub fn build(session: &Session, data: &SessionData, plan: Plan) -> Report {
    let included = session.cost_available && plan.includes(session.provider);
    let metrics = &data.metrics;

    Report {
        session_id: session.session_id.clone(),
        provider: session.provider.as_str(),
        title: data
            .custom_title
            .clone()
            .or_else(|| data.title.clone())
            .or_else(|| session.title.clone()),
        project: (!session.label_source.is_empty()).then(|| util::tildify(&session.label_source)),
        branch: crate::ui::columns::branch_of(session),
        profile: session.profile.clone(),
        model: (!session.model.is_empty()).then(|| session.model.clone()),
        models: data.models.clone(),
        started_at: session.started_at.clone(),
        last_active: session.last_active.clone(),
        duration: util::session_duration(&session.started_at, &session.last_active),
        running: session.is_running(),
        state: match session.activity_state {
            crate::session::ActivityState::Working => "working",
            crate::session::ActivityState::WaitingForInput => "waiting",
            crate::session::ActivityState::ApiError => "error",
        },
        plan: plan.as_str(),
        error: data.error.clone(),

        cost: ReportCost {
            available: session.cost_available,
            included,
            total: data.costs.total,
            by_model: data
                .model_breakdown
                .iter()
                .map(|m| ReportModelCost {
                    model: m.model.clone(),
                    total: m.total,
                    tokens: m.tokens.clone(),
                })
                .collect(),
            by_day: sorted_buckets(&data.costs_by_day),
            by_hour: sorted_buckets(&data.costs_by_hour),
        },
        tokens: data.tokens.clone(),
        context: session.context.map(|usage| ReportContext {
            used: usage.used,
            max: usage.max,
            percent_to_compact: usage.percent_to_compact(),
            compactions: data.compactions,
            breakdown: data.context_breakdown.as_ref().map(|b| ReportBreakdown {
                total: b.total,
                startup: b.startup,
                tool_output: b.tool_output,
                tool_input: b.tool_input,
                attachments: b.attachments,
                user_text: b.user_text,
                assistant_text: b.assistant_text,
                unaccounted: b.unaccounted(),
                after_compaction: b.after_compaction,
                superseded: b.superseded,
            }),
            series: data.context_series.clone(),
        }),
        activity: ReportActivity {
            tool_count: metrics.tool_count,
            tool_errors: session
                .provider
                .records_tool_outcomes()
                .then_some(metrics.tool_errors),
            error_rate: session.error_rate(),
            lines_added: metrics.lines_added,
            lines_removed: metrics.lines_removed,
            tools: tools(data),
            failures: failures(data),
            slowest: slowest(data),
            calls: calls(data),
        },
        files: util::abbreviate_paths(&data.recent_writes)
            .into_iter()
            .take(MAX_FILES)
            .collect(),
        diffs: diffs(data),
        subagents: data.subagents.clone(),
    }
}

/// Cost buckets as an oldest-first list.
///
/// A map would arrive at the page in whatever order the JSON object happened to
/// carry, and a chart drawn from it would be in that order too.
fn sorted_buckets(buckets: &HashMap<String, HashMap<String, f64>>) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = buckets
        .iter()
        .map(|(k, models)| (k.clone(), models.values().sum()))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Per-tool call and failure counts, busiest first.
fn tools(data: &SessionData) -> Vec<ReportTool> {
    let mut out: Vec<ReportTool> = data
        .metrics
        .tools
        .iter()
        .map(|(name, &calls)| ReportTool {
            name: name.clone(),
            calls,
            // Counted off the details rather than read from a per-tool total,
            // which nothing records. A tool whose details were trimmed away
            // therefore under-reports here, which is why the headline figure on
            // the page is `tool_errors` and this is the breakdown beneath it.
            failed: data
                .metrics
                .tool_details
                .get(name)
                .map_or(0, |calls| calls.iter().filter(|c| c.failed).count() as u64),
        })
        .collect();
    out.sort_by(|a, b| b.calls.cmp(&a.calls).then(a.name.cmp(&b.name)));
    out.truncate(MAX_TOOLS);
    out
}

/// Failed calls grouped by tool *and argument*, most-repeated first.
///
/// Grouping on the argument is the whole point. Eleven separate failures of
/// `Bash` is a session with a bad afternoon; eleven failures of `Bash` running
/// the identical command is a loop, and only the second is worth anyone's
/// attention. The two look the same in every per-tool count there is.
fn failures(data: &SessionData) -> Vec<ReportFailure> {
    let mut clusters: HashMap<(&str, &str), Vec<ReportCall>> = HashMap::new();
    for (tool, calls) in &data.metrics.tool_details {
        for call in calls.iter().filter(|c| c.failed) {
            // `full` is the untruncated argument where the panel's one-line `d`
            // was shortened; clustering on the short form would merge two long
            // commands that happen to share a prefix.
            let key = call.full.as_deref().unwrap_or(&call.d);
            clusters
                .entry((tool.as_str(), key))
                .or_default()
                .push(ReportCall {
                    tool: tool.clone(),
                    detail: call.d.clone(),
                    ts: call.ts.clone(),
                    duration_ms: call.dur_ms,
                    failed: true,
                });
        }
    }

    let mut out: Vec<ReportFailure> = clusters
        .into_iter()
        .map(|((tool, detail), mut samples)| {
            let count = samples.len() as u64;
            samples.sort_by(|a, b| a.ts.cmp(&b.ts));
            samples.truncate(MAX_FAILURE_SAMPLES);
            ReportFailure {
                tool: tool.to_string(),
                // The cluster key can be the full argument, which is unbounded
                // in a way the page's layout is not; the samples carry the
                // display form and this only has to identify the group.
                detail: util::truncate(detail, 200),
                count,
                samples,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.tool.cmp(&b.tool))
            .then(a.detail.cmp(&b.detail))
    });
    out.truncate(MAX_FAILURE_CLUSTERS);
    out
}

/// Every recorded call, newest first.
///
/// Newest first because a report is usually opened about something that just
/// happened, and the tail of a long session is where it happened.
fn calls(data: &SessionData) -> Vec<ReportCall> {
    let mut out: Vec<ReportCall> = data
        .metrics
        .tool_details
        .iter()
        .flat_map(|(tool, calls)| {
            calls.iter().map(move |call| ReportCall {
                tool: tool.clone(),
                detail: call.d.clone(),
                ts: call.ts.clone(),
                duration_ms: call.dur_ms,
                failed: call.failed,
            })
        })
        .collect();
    // By timestamp, which is the only order that means anything across tools —
    // the per-tool vectors are each chronological and say nothing about each
    // other.
    out.sort_by(|a, b| b.ts.cmp(&a.ts));
    out
}

/// One file's edits, accumulated before they become a [`ReportFileDiff`].
///
/// A named type rather than a tuple in the map: four fields of which two are
/// counts and one is a vector of pairs is exactly the shape nobody can read at
/// the call site six months later.
#[derive(Default)]
struct FileEdits<'a> {
    added: u32,
    removed: u32,
    edits: usize,
    /// `(timestamp, hunks)` per edit, replayed in timestamp order so the diff
    /// reads in the order it was applied.
    parts: Vec<(&'a str, &'a Vec<String>)>,
}

/// What the session changed, per file, most-changed first.
///
/// Grouped by the detail string because that is the file: for `Edit` and
/// `Write` it is the path the call named, and for `apply_patch` it is the path
/// the patch touched. Edits are replayed oldest-first within a file, so the
/// hunks read in the order they were applied.
fn diffs(data: &SessionData) -> Vec<ReportFileDiff> {
    let mut by_file: HashMap<&str, FileEdits<'_>> = HashMap::new();
    for calls in data.metrics.tool_details.values() {
        for call in calls {
            let Some(delta) = &call.delta else { continue };
            if delta.hunks.is_empty() {
                continue;
            }
            let entry = by_file.entry(call.d.as_str()).or_default();
            entry.added += delta.added;
            entry.removed += delta.removed;
            entry.edits += 1;
            entry.parts.push((call.ts.as_str(), &delta.hunks));
        }
    }

    // Abbreviated together rather than each truncated: the identifying part of
    // a path is its tail, and these are frequently sibling files under a
    // worktree whose whole prefix is the same forty characters. This keeps
    // whatever makes each one unique and drops the rest — the same treatment
    // the file list above gets, and for the same reason.
    let paths: Vec<String> = by_file.keys().map(|f| (*f).to_string()).collect();
    let short: HashMap<String, String> = paths
        .iter()
        .cloned()
        .zip(util::abbreviate_paths(&paths))
        .collect();

    let mut out: Vec<ReportFileDiff> = by_file
        .into_iter()
        .map(|(file, mut edits)| {
            edits.parts.sort_by_key(|(ts, _)| *ts);
            let mut hunks: Vec<String> = Vec::new();
            let mut truncated = false;
            for (_, lines) in &edits.parts {
                for line in lines.iter() {
                    if hunks.len() >= MAX_HUNKS_PER_FILE {
                        truncated = true;
                        break;
                    }
                    hunks.push(line.clone());
                }
            }
            ReportFileDiff {
                file: short.get(file).cloned().unwrap_or_else(|| file.to_string()),
                added: edits.added,
                removed: edits.removed,
                edits: edits.edits,
                hunks,
                truncated,
            }
        })
        .collect();
    // Most-changed first: the file a session rewrote is the one worth reading,
    // and a one-line tweak at the top would bury it.
    out.sort_by(|a, b| {
        (b.added + b.removed)
            .cmp(&(a.added + a.removed))
            .then(a.file.cmp(&b.file))
    });
    out.truncate(MAX_DIFF_FILES);
    out
}

/// The individual calls that took longest.
///
/// Wall time, not cost: a call that blocked the session for four minutes is
/// worth seeing whether or not it was billed for, and it usually is not.
///
/// Excludes the tools that block on a person — see [`HUMAN_WAIT_TOOLS`].
fn slowest(data: &SessionData) -> Vec<ReportCall> {
    let mut out: Vec<ReportCall> = data
        .metrics
        .tool_details
        .iter()
        .filter(|(tool, _)| !HUMAN_WAIT_TOOLS.contains(&tool.as_str()))
        .flat_map(|(tool, calls)| {
            calls.iter().filter_map(move |call| {
                call.dur_ms.map(|ms| ReportCall {
                    tool: tool.clone(),
                    detail: call.d.clone(),
                    ts: call.ts.clone(),
                    duration_ms: Some(ms),
                    failed: call.failed,
                })
            })
        })
        .collect();
    out.sort_by_key(|call| std::cmp::Reverse(call.duration_ms));
    out.truncate(MAX_SLOWEST);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Metrics, ToolDetail};

    fn failed_call(detail: &str, ts: &str) -> ToolDetail {
        ToolDetail {
            d: detail.to_string(),
            ts: ts.to_string(),
            failed: true,
            ..Default::default()
        }
    }

    fn data_with(details: HashMap<String, Vec<ToolDetail>>) -> SessionData {
        SessionData {
            metrics: Metrics {
                tool_details: details,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn identical_failures_collapse_into_one_counted_cluster() {
        let data = data_with(HashMap::from([(
            "Bash".to_string(),
            vec![
                failed_call("cargo buidl", "2026-08-19T10:00:00Z"),
                failed_call("cargo buidl", "2026-08-19T10:01:00Z"),
                failed_call("cargo buidl", "2026-08-19T10:02:00Z"),
                failed_call("ls /nope", "2026-08-19T10:03:00Z"),
            ],
        )]));

        let found = failures(&data);
        assert_eq!(found.len(), 2, "two distinct arguments failed");
        // The repeated one leads: a loop is the finding, a one-off is noise.
        assert_eq!(found[0].detail, "cargo buidl");
        assert_eq!(found[0].count, 3);
        assert_eq!(found[1].count, 1);
    }

    #[test]
    fn the_same_argument_to_different_tools_stays_separate() {
        let data = data_with(HashMap::from([
            ("Bash".to_string(), vec![failed_call("x", "1")]),
            ("Read".to_string(), vec![failed_call("x", "2")]),
        ]));
        assert_eq!(failures(&data).len(), 2);
    }

    #[test]
    fn clustering_uses_the_full_argument_where_the_display_form_was_cut() {
        // Two long commands sharing a prefix are one finding if you cluster on
        // the truncated form, and two if you do not. They are two.
        let mut long_a = failed_call("git log --oneline …", "1");
        long_a.full = Some("git log --oneline --since=yesterday -- src/a.rs".into());
        let mut long_b = failed_call("git log --oneline …", "2");
        long_b.full = Some("git log --oneline --since=yesterday -- src/b.rs".into());

        let data = data_with(HashMap::from([("Bash".to_string(), vec![long_a, long_b])]));
        assert_eq!(failures(&data).len(), 2);
    }

    #[test]
    fn successful_calls_are_not_failures() {
        let data = data_with(HashMap::from([(
            "Bash".to_string(),
            vec![ToolDetail {
                d: "cargo test".into(),
                ts: "1".into(),
                ..Default::default()
            }],
        )]));
        assert!(failures(&data).is_empty());
    }

    #[test]
    fn slowest_ranks_by_wall_time_and_ignores_untimed_calls() {
        let timed = |ms: Option<i64>| ToolDetail {
            d: format!("{ms:?}"),
            dur_ms: ms,
            ..Default::default()
        };
        let data = data_with(HashMap::from([(
            "Bash".to_string(),
            vec![timed(Some(10)), timed(None), timed(Some(9_000))],
        )]));

        let ranked = slowest(&data);
        assert_eq!(ranked.len(), 2, "the untimed call cannot be ranked");
        assert_eq!(ranked[0].duration_ms, Some(9_000));
    }

    #[test]
    fn edits_to_one_file_become_one_diff_in_the_order_they_landed() {
        use crate::session::Delta;
        let edit = |file: &str, ts: &str, added, removed, hunk: &str| ToolDetail {
            d: file.to_string(),
            ts: ts.to_string(),
            delta: Some(Delta {
                added,
                removed,
                hunks: vec![hunk.to_string()],
            }),
            ..Default::default()
        };
        let data = data_with(HashMap::from([(
            "Edit".to_string(),
            vec![
                edit("b.rs", "2026-08-19T10:00:00Z", 1, 0, "+one"),
                edit("a.rs", "2026-08-19T10:02:00Z", 5, 2, "+second"),
                edit("a.rs", "2026-08-19T10:01:00Z", 4, 1, "+first"),
            ],
        )]));

        let found = diffs(&data);
        assert_eq!(found.len(), 2);
        // Names stay whole where nothing needs eliding.
        assert!(found.iter().all(|f| f.file.ends_with(".rs")));
        // Most-changed first: the file the session rewrote, not the one it
        // tweaked, is what the reader came for.
        assert_eq!(found[0].file, "a.rs");
        assert_eq!((found[0].added, found[0].removed), (9, 3));
        assert_eq!(found[0].edits, 2);
        // Replayed in the order they were applied, not the order the map held.
        assert_eq!(found[0].hunks, vec!["+first", "+second"]);
        assert!(!found[0].truncated);
    }

    /// Sibling files under one long worktree path are told apart by their
    /// tails, not by forty identical leading characters.
    #[test]
    fn diff_file_names_are_abbreviated_against_each_other() {
        use crate::session::Delta;
        let edit = |file: &str| ToolDetail {
            d: file.to_string(),
            ts: "2026-08-19T10:00:00Z".into(),
            delta: Some(Delta {
                added: 1,
                removed: 0,
                hunks: vec!["+x".into()],
            }),
            ..Default::default()
        };
        let data = data_with(HashMap::from([(
            "Edit".to_string(),
            vec![
                edit("/home/flo/cctop/.claude/worktrees/agent-9/src/ui/theme.rs"),
                edit("/home/flo/cctop/.claude/worktrees/agent-9/src/ui/table.rs"),
            ],
        )]));

        for f in diffs(&data) {
            assert!(
                !f.file.starts_with("/home/flo"),
                "the shared prefix survived: {}",
                f.file
            );
            assert!(f.file.ends_with(".rs"), "{}", f.file);
        }
    }

    #[test]
    fn a_call_with_no_patch_contributes_no_diff() {
        let data = data_with(HashMap::from([(
            "Bash".to_string(),
            vec![ToolDetail {
                d: "ls".into(),
                ..Default::default()
            }],
        )]));
        assert!(diffs(&data).is_empty());
    }

    #[test]
    fn the_call_log_is_newest_first_across_every_tool() {
        let at = |tool: &str, ts: &str| (tool.to_string(), ts.to_string());
        let (bash, read) = (
            at("Bash", "2026-08-19T10:00:00Z"),
            at("Read", "2026-08-19T10:05:00Z"),
        );
        let data = data_with(HashMap::from([
            (
                bash.0,
                vec![ToolDetail {
                    d: "ls".into(),
                    ts: bash.1,
                    ..Default::default()
                }],
            ),
            (
                read.0,
                vec![ToolDetail {
                    d: "a.rs".into(),
                    ts: read.1,
                    ..Default::default()
                }],
            ),
        ]));
        // The per-tool vectors are each chronological and say nothing about
        // each other, so only the timestamp can order the log.
        let log = calls(&data);
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].tool, "Read");
        assert_eq!(log[1].tool, "Bash");
    }

    /// A question the user took a minute to answer is not the session being
    /// slow, and putting it first buries whatever was.
    #[test]
    fn waiting_on_a_person_is_not_a_slow_call() {
        let timed = |ms: i64| ToolDetail {
            d: "x".into(),
            dur_ms: Some(ms),
            ..Default::default()
        };
        let data = data_with(HashMap::from([
            ("AskUserQuestion".to_string(), vec![timed(90_000)]),
            ("ExitPlanMode".to_string(), vec![timed(80_000)]),
            ("Bash".to_string(), vec![timed(4_000)]),
        ]));

        let ranked = slowest(&data);
        assert_eq!(ranked.len(), 1, "only the agent's own work is ranked");
        assert_eq!(ranked[0].tool, "Bash");

        // Still a real call, so the log that records what happened keeps it.
        assert_eq!(calls(&data).len(), 3);
    }

    #[test]
    fn cost_buckets_come_back_oldest_first() {
        let buckets = HashMap::from([
            ("2026-08-19".to_string(), HashMap::from([("m".into(), 2.0)])),
            ("2026-08-17".to_string(), HashMap::from([("m".into(), 1.0)])),
        ]);
        let sorted = sorted_buckets(&buckets);
        assert_eq!(sorted[0].0, "2026-08-17");
        assert_eq!(sorted[1].0, "2026-08-19");
    }
}
