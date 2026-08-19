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

/// How many written files are listed.
const MAX_FILES: usize = 40;

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
    pub subagents: Vec<Subagent>,
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
        },
        files: util::abbreviate_paths(&data.recent_writes)
            .into_iter()
            .take(MAX_FILES)
            .collect(),
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

/// The individual calls that took longest.
///
/// Wall time, not cost: a call that blocked the session for four minutes is
/// worth seeing whether or not it was billed for, and it usually is not.
fn slowest(data: &SessionData) -> Vec<ReportCall> {
    let mut out: Vec<ReportCall> = data
        .metrics
        .tool_details
        .iter()
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
