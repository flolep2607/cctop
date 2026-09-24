//! The analytics document behind `/api/analytics`: every session the table
//! knows, with the history its rows summarise.
//!
//! `json_sessions` — the document the dashboard already streams — is built to
//! be re-sent every couple of seconds, so it trims: cost buckets stop at 31
//! days and 48 hours, and the tool history never crosses the wire at all.
//! Analytics exists for the long view, so nothing here is trimmed, and the
//! token buckets sit beside the cost ones: a session on a bundled plan, or
//! under a provider that records no rates, has no dollars to graph but burned
//! tokens all the same.

use crate::pricing::{Plan, Provider};
use crate::session::{ActivityState, Session, Surface};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

/// The whole response: a timestamp, the plan the figures were read under, and
/// one row per session.
#[derive(Serialize)]
pub struct Analytics {
    /// When the document was built, RFC-3339.
    pub generated: String,
    /// `retail`, `max` or `included` — carried beside the rows because `cost`
    /// and `cost_included` are only meaningful relative to it.
    pub plan: &'static str,
    /// Newest first, in the order the caller already sorted.
    pub sessions: Vec<AnalyticsSession>,
}

/// Billed tokens, flattened to the four kinds a chart draws.
#[derive(Serialize)]
pub struct AnalyticsTokens {
    /// Input billed at the full rate — the uncached remainder. On a remote row
    /// it is everything billed as input instead: the wire carries the
    /// all-input total and the output total, not the four-way split.
    pub input: u64,
    /// Output, including reasoning where the provider counts it there.
    pub output: u64,
    /// Served from cache: `cache_read`, plus the `cached_input` Codex and
    /// Gemini name instead. Folded together so the parts sum to `total`.
    pub cache_read: u64,
    /// Both cache-write tiers, `cache_write_5m + cache_write_1h`.
    pub cache_write: u64,
    /// Everything billed: `all_input() + output`, plus `reasoning_output` for
    /// the one provider — Gemini — that counts thinking beside `output`
    /// instead of inside it, so the figure equals what the per-day buckets
    /// chart rather than sitting short of it.
    pub total: u64,
}

/// One session, everything the analytics page draws on.
#[derive(Serialize)]
pub struct AnalyticsSession {
    pub id: String,
    pub provider: &'static str,
    /// The display name for the provider-and-surface pair — "Claude Cowork"
    /// rather than a bare "claude" — so the page never has to re-derive it.
    pub label: &'static str,
    /// `cli`, `editor`, `desktop-code` or `desktop-cowork`.
    pub surface: &'static str,
    /// The newest model the session ran; empty where nothing said one.
    pub model: String,
    /// Every model the transcript recorded — empty on a remote row, whose wire
    /// document sends only the newest under `model`.
    pub models: Vec<String>,
    pub harness: String,
    /// Working directory the session runs in.
    pub project: String,
    /// The row's abbreviated label, or the directory's basename where no
    /// abbreviation pass ran over this set of sessions.
    pub project_name: String,
    pub title: Option<String>,
    /// Login name of the user the session belongs to, when cctop is sweeping
    /// homes other than the reader's own.
    pub user: Option<String>,
    /// Which Claude profile — which `$CLAUDE_CONFIG_DIR` — the session was
    /// read out of. No other provider has the concept.
    pub profile: Option<String>,
    /// The local account's email — or its organisation when no email is
    /// recorded — for the providers that keep a credential on disk, and only
    /// for the reader's own sessions: stamping the local login on another
    /// user's row would be a wrong answer where no answer is the true one.
    /// Same rule as `cli::json_sessions`; a bare string rather than that
    /// route's object because the page uses it directly as a grouping key.
    pub account: Option<String>,
    /// The ssh host the row came from, when it did not come from this machine.
    pub host: Option<String>,
    /// Branch read on the machine the session runs on.
    pub branch: Option<String>,
    pub started: String,
    pub last_active: String,
    /// A process (or a trusted liveness inference) backs the row right now.
    pub running: bool,
    /// `working`, `waiting`, `asking` or `error` — what the newest transcript
    /// event says, independent of `running`.
    pub state: &'static str,
    pub tokens: AnalyticsTokens,
    /// The recorded total in USD, `null` only where the provider records no
    /// billable usage at all (`cost_available: false`).
    ///
    /// A plan-bundled session still emits its figure: what was recorded is the
    /// retail equivalent rather than money spent, and `cost_included` is what
    /// tells the two apart — the page reads the pair together, so withholding
    /// the number would hide the bundled share it exists to report.
    ///
    /// The row's figure wins over the extraction's because the two differ on a
    /// remote session: `data` is an empty default there, while `total_cost`
    /// is the number the far side sent. Where the row carries none — a bundled
    /// local session withholds `total_cost` — the extraction's total is the
    /// recorded figure.
    pub cost: Option<f64>,
    pub cost_available: bool,
    /// The active plan bundles this provider's usage: `cost` is the recorded
    /// retail equivalent, not an itemised charge — the distinction the page
    /// labels `incl` for.
    pub cost_included: bool,
    /// Recorded usage that priced at zero, as with a free model — distinct
    /// from `cost_available: false`, which means nothing was recorded at all.
    pub cost_free: bool,
    pub tools: u64,
    /// Calls the transcript reported as failed, `null` where the provider
    /// records no per-call outcome — a zero there would read as "no failures"
    /// when the honest answer is "cannot say".
    pub tool_errors: Option<u64>,
    /// Tool name -> number of calls.
    pub tool_names: HashMap<String, u64>,
    pub lines_added: u64,
    pub lines_removed: u64,
    /// How many subagents the session spawned — off the row, which a remote
    /// session fills from the wire where `SessionData` stays empty.
    pub subagents: usize,
    /// USD across all subagents. Off the row, for the same reason as `cost`.
    pub subagent_cost: f64,
    /// `YYYY-MM-DD` -> model -> USD, the whole history untrimmed.
    ///
    /// Session-level rather than `SessionData`-level on purpose: a remote row
    /// fills this from the wire document, which is the only way its history
    /// reaches the page.
    pub by_day: HashMap<String, HashMap<String, f64>>,
    /// `YYYY-MM-DDTHH` -> model -> USD.
    pub by_hour: HashMap<String, HashMap<String, f64>>,
    /// `YYYY-MM-DD` -> model -> billed tokens.
    ///
    /// Lives on `SessionData`, which the wire format has no field for, so a
    /// remote session reports an empty map rather than a fabricated one.
    pub tokens_by_day: HashMap<String, HashMap<String, u64>>,
    /// `YYYY-MM-DDTHH` -> model -> billed tokens.
    pub tokens_by_hour: HashMap<String, HashMap<String, u64>>,
    /// Paths the session wrote lately, newest first.
    pub writes: Vec<String>,
}

/// Build the document for the sessions the snapshot currently holds.
///
/// Extractions come from [`Store::session_data`](crate::cache::Store::session_data),
/// the cached read — not `session_data_fresh`. Nothing below wants the fields
/// only a fresh parse restores (the tool history, the context series), and a
/// full re-parse of every transcript per request is precisely the cost the
/// cache exists to avoid.
///
/// A remote row is still emitted: `session_data` returns the empty default
/// for one, because its `data_file` names a path on another machine, but
/// everything the wire carried — the cost buckets, the subagent figures, the
/// identity fields — lives on [`Session`] itself.
pub fn build(sessions: &[Session], plan: Plan, store: &crate::cache::Store) -> Analytics {
    // Each of these is a file read that cannot change inside a request, so it
    // happens once rather than per row.
    let claude_account = crate::quota::claude_account();
    let codex_account = crate::quota::codex_account();

    Analytics {
        generated: crate::util::ms_to_rfc3339(crate::util::now_ms()),
        plan: plan.as_str(),
        sessions: sessions
            .iter()
            .map(|s| row(s, plan, store, &claude_account, &codex_account))
            .collect(),
    }
}

fn row(
    s: &Session,
    plan: Plan,
    store: &crate::cache::Store,
    claude_account: &Option<crate::quota::Account>,
    codex_account: &Option<crate::quota::Account>,
) -> AnalyticsSession {
    let data = store.session_data(s);
    let included = s.cost_available && plan.includes(s.provider);
    let account = match s.provider {
        Provider::Claude if s.owner.is_none() => claude_account.as_ref(),
        Provider::Codex if s.owner.is_none() => codex_account.as_ref(),
        _ => None,
    }
    .and_then(|a| a.email.clone().or_else(|| a.organization.clone()));

    AnalyticsSession {
        id: s.session_id.clone(),
        provider: s.provider.as_str(),
        label: s.surface.label(s.provider),
        surface: match s.surface {
            Surface::Cli => "cli",
            Surface::Editor => "editor",
            Surface::DesktopCode => "desktop-code",
            Surface::DesktopCowork => "desktop-cowork",
        },
        model: s.model.clone(),
        models: data.models.clone(),
        harness: s.harness.clone(),
        project: s.label_source.clone(),
        project_name: if s.abbrev_label.is_empty() {
            Path::new(&s.label_source)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| s.label_source.clone())
        } else {
            s.abbrev_label.clone()
        },
        title: s.title.clone(),
        user: s.owner.clone(),
        profile: s.profile.clone(),
        account,
        host: s.remote.as_ref().map(|r| r.host.clone()),
        branch: crate::ui::columns::branch_of(s),
        started: s.started_at.clone(),
        last_active: s.last_active.clone(),
        running: s.is_running(),
        state: match s.activity_state {
            ActivityState::Working => "working",
            ActivityState::WaitingForInput => "waiting",
            ActivityState::Asking => "asking",
            ActivityState::ApiError => "error",
        },
        tokens: if s.remote.is_some() {
            // `data` is the empty default for a remote row — the transcript is
            // on another machine — but the wire carried the totals the far
            // side measured, and relaying them beats reporting a burn of zero.
            AnalyticsTokens {
                input: s.input_tokens,
                output: s.output_tokens,
                cache_read: 0,
                cache_write: 0,
                total: s.input_tokens + s.output_tokens,
            }
        } else {
            AnalyticsTokens {
                input: data.tokens.input,
                output: data.tokens.output,
                cache_read: data.tokens.cache_read + data.tokens.cached_input,
                cache_write: data.tokens.cache_write_5m + data.tokens.cache_write_1h,
                // Everywhere else `reasoning_output` is an "of which" the output
                // count already holds; Gemini reports `thoughts` disjointly, which
                // is why its bucket adds them too.
                total: data.tokens.all_input()
                    + data.tokens.output
                    + if s.provider == Provider::Gemini {
                        data.tokens.reasoning_output
                    } else {
                        0
                    },
            }
        },
        cost: if s.cost_available {
            Some(s.total_cost.unwrap_or(data.costs.total))
        } else {
            None
        },
        cost_available: s.cost_available,
        cost_included: included,
        cost_free: s.cost_is_free,
        // Off the row rather than `data.metrics`: the loader copies these over
        // for local sessions and the wire carries them for remote ones, either
        // of which leaves `data` looking emptier than the session was.
        tools: s.tool_count,
        tool_errors: s.provider.records_tool_outcomes().then_some(s.tool_errors),
        tool_names: data.metrics.tools.clone(),
        lines_added: data.metrics.lines_added,
        lines_removed: data.metrics.lines_removed,
        subagents: s.subagents.len(),
        subagent_cost: s.subagents_cost,
        by_day: s.costs_by_day.clone(),
        by_hour: s.costs_by_hour.clone(),
        tokens_by_day: data.tokens_by_day.clone(),
        tokens_by_hour: data.tokens_by_hour.clone(),
        writes: s.recent_writes.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Remote;

    /// Sessions with no `data_file` keep the test hermetic: `session_data`
    /// returns the empty default before the cache is ever consulted, so every
    /// figure below came off the row itself.
    fn row_of(s: Session, plan: Plan) -> AnalyticsSession {
        let store = crate::cache::Store::new();
        build(&[s], plan, &store)
            .sessions
            .into_iter()
            .next()
            .expect("one row in, one row out")
    }

    #[test]
    fn a_provider_with_no_billable_usage_reports_null_cost() {
        let mut s = Session::new(Provider::Cursor, "c".into());
        s.cost_available = false;
        s.total_cost = None;

        let row = row_of(s, Plan::Retail);

        assert!(!row.cost_available);
        assert_eq!(row.cost, None);
        assert!(!row.cost_included);
    }

    /// A bundled plan changes how the figure is read, not whether there is
    /// one: `cost` carries the recorded retail equivalent and `cost_included`
    /// says so, while the buckets still chart the session's shape.
    #[test]
    fn a_bundled_session_is_marked_included_and_keeps_its_figures() {
        let mut s = Session::new(Provider::Claude, "x".into());
        // What `annotate` leaves under a bundled plan: gated `None`, so the
        // recorded total comes off the extraction — zero here, the session
        // having no transcript.
        s.total_cost = None;
        s.costs_by_day
            .insert("2026-08-11".into(), HashMap::from([("m".into(), 1.25)]));

        let row = row_of(s, Plan::Max);

        assert!(row.cost_included);
        assert_eq!(row.cost, Some(0.0));
        assert_eq!(row.by_day["2026-08-11"]["m"], 1.25);
    }

    /// `tool_errors` is a number only where the transcript can record a
    /// failure at all: Pi writes calls but never their outcomes.
    #[test]
    fn tool_errors_is_null_only_where_the_harness_cannot_say() {
        let pi = row_of(Session::new(Provider::Pi, "p".into()), Plan::Retail);
        assert_eq!(pi.tool_errors, None);

        let mut claude = Session::new(Provider::Claude, "c".into());
        claude.tool_errors = 3;
        let claude = row_of(claude, Plan::Retail);
        assert_eq!(claude.tool_errors, Some(3));
    }

    /// A remote session's extraction is the empty default — its transcript is
    /// on another machine — but the fields the wire document carried still
    /// arrive: the buckets, the cost, the host it ran on.
    #[test]
    fn a_remote_row_reports_what_the_wire_carried() {
        let mut s = Session::new(Provider::Claude, "r".into());
        s.remote = Some(Remote {
            host: "buildbox".into(),
            branch: Some("main".into()),
            ..Default::default()
        });
        s.total_cost = Some(4.2);
        s.input_tokens = 4_000;
        s.output_tokens = 500;
        s.costs_by_day
            .insert("2026-08-11".into(), HashMap::from([("m".into(), 4.2)]));

        let row = row_of(s, Plan::Retail);

        assert_eq!(row.host.as_deref(), Some("buildbox"));
        assert_eq!(row.branch.as_deref(), Some("main"));
        assert_eq!(row.by_day["2026-08-11"]["m"], 4.2);
        assert_eq!(row.cost, Some(4.2));
        // The token totals the wire carried still report, even though the
        // extraction — the transcript being on the far machine — is empty.
        assert_eq!(row.tokens.total, 4_500);
        // But the buckets need per-event timestamps, which the wire does not
        // carry: empty, not invented.
        assert!(row.tokens_by_day.is_empty());
    }
}
