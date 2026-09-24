//! `GET /metrics` — the table, as something Prometheus can scrape.
//!
//! The dashboard answers "what is happening now"; a time-series database is
//! what answers "what happened on Tuesday", and the person who has one already
//! would rather point it at cctop than learn another page. So this renders the
//! snapshot every other route serves in the text exposition format, written by
//! hand: the format is a line per sample and a pair of comments per family, and
//! a client crate would be a registry, a global and a dependency tree bought to
//! produce exactly that.
//!
//! # It says nothing `/api/sessions` does not
//!
//! Every figure here is one the session document already carries — the row's
//! state, its recorded cost and tokens, its tool counts, its context window —
//! summed. It sits behind the same gate as every other route, and either of the
//! run's tokens opens it, because the read-only link already reads all of this.
//! Titles, prompts, paths beyond a project's own directory name, and anything
//! from the transcript's text are absent on purpose: a metrics store keeps what
//! it is sent for months and shows it to whoever has a Grafana login.
//!
//! # Cardinality is bounded by construction
//!
//! A label's values become series, and series are what a Prometheus pays for.
//! So the labels are ones with a small, closed set of values — provider, state,
//! model, token kind — with one exception: the context-window gauges carry a
//! session and a project, because a window's fill only means anything for one
//! session. Those series exist **only for live sessions**, which there are a
//! handful of at a time; a session that ends drops out of the next scrape, and
//! Prometheus marks its series stale rather than accumulating one per session
//! ever run. The session label is the id's first eight characters, which is how
//! the table and `/session/<prefix>` already name one.
//!
//! # Gauges, not counters, even for totals
//!
//! Cost and tokens only ever grow for a given session, but these are sums over
//! the sessions in the table, and a session can leave the table — its host goes
//! unreachable, its transcript is deleted. A counter that went down would be
//! read by `rate()` as a reset and turned into a spike of the whole total.
//! A gauge that went down is simply lower, which is the truth.

use crate::pricing::Plan;
use crate::session::{ActivityState, Session, SessionData};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// The media type of the text exposition format, version pinned — Prometheus
/// picks its parser by this header, and an unversioned `text/plain` is read as
/// the same format only by convention.
pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// How much of a session id labels its context series.
///
/// Eight hex characters are what the table shows and what a `/session/` link
/// is usually cut to; among the few live sessions this is ever applied to, a
/// collision is not a practical concern.
const SHORT_ID: usize = 8;

/// Render the exposition for one snapshot.
///
/// `store` is the serve's own extraction cache, read the way the analytics
/// route reads it — stale allowed — so a scrape costs a memory lookup per row
/// once the cache is warm rather than a transcript parse.
pub fn render(
    sessions: &[Session],
    plan: Plan,
    store: &crate::cache::Store,
    unreadable_hosts: usize,
) -> String {
    render_with(sessions, plan, unreadable_hosts, |s| store.session_data(s))
}

/// [`render`] with the extraction supplied, which is what lets the tests build
/// rows without a transcript on disk.
fn render_with(
    sessions: &[Session],
    plan: Plan,
    unreadable_hosts: usize,
    data: impl Fn(&Session) -> SessionData,
) -> String {
    let mut agg = Aggregate::default();
    for s in sessions {
        agg.add(s, &data(s), plan);
    }
    agg.write(unreadable_hosts)
}

/// Which bucket of `cctop_sessions` a row counts toward.
///
/// `idle` is a session with no process behind it — ended, or never seen
/// running. The transcript's own state is only meaningful while something is
/// live to be in it: the last event of a session closed yesterday says
/// "waiting", and counting it as waiting on you would make the gauge useless
/// for the one alert people want from it.
fn state(s: &Session) -> &'static str {
    if !s.is_running() {
        return "idle";
    }
    match s.activity_state {
        ActivityState::Working => "working",
        ActivityState::WaitingForInput => "waiting",
        ActivityState::Asking => "asking",
        ActivityState::ApiError => "error",
    }
}

/// Every value [`state`] can return, so a provider's series for a state that
/// no session is in reads 0 rather than being absent — an absent series makes
/// `cctop_sessions{state="waiting"} > 0` silently unevaluable, not false.
const STATES: [&str; 5] = ["working", "waiting", "asking", "error", "idle"];

/// A model name for a label, never empty — an empty label value is the same as
/// the label being absent, which would merge unnamed models into a different
/// series shape than every other.
fn model_label(model: &str) -> String {
    match model.is_empty() {
        true => "unknown".to_string(),
        false => model.to_string(),
    }
}

/// The last component of a session's working directory — the project name the
/// table shows — rather than the whole path, which would put a home directory
/// into every series.
fn project_label(s: &Session) -> String {
    std::path::Path::new(&s.label_source)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

type CostKey = (&'static str, bool);

#[derive(Default)]
struct Aggregate {
    sessions: BTreeMap<(&'static str, &'static str), u64>,
    /// (provider, included) -> USD.
    cost_total: BTreeMap<CostKey, f64>,
    cost_today: BTreeMap<CostKey, f64>,
    cost_hour: BTreeMap<CostKey, f64>,
    burn: BTreeMap<CostKey, f64>,
    /// (provider, model, included) -> USD.
    model_total: BTreeMap<(&'static str, String, bool), f64>,
    model_today: BTreeMap<(&'static str, String, bool), f64>,
    /// (provider, model, kind) -> tokens.
    tokens: BTreeMap<(&'static str, String, &'static str), u64>,
    tool_calls: BTreeMap<&'static str, u64>,
    /// Only providers whose transcripts record a per-call outcome: a zero for
    /// the others would read as "nothing failed" when the answer is "cannot say".
    tool_errors: BTreeMap<&'static str, u64>,
    context: Vec<Context>,
}

struct Context {
    session: String,
    project: String,
    provider: &'static str,
    used: u64,
    max: u64,
}

impl Aggregate {
    fn add(&mut self, s: &Session, data: &SessionData, plan: Plan) {
        let provider = s.provider.as_str();
        for st in STATES {
            self.sessions.entry((provider, st)).or_default();
        }
        *self.sessions.entry((provider, state(s))).or_default() += 1;

        *self.tool_calls.entry(provider).or_default() += s.tool_count;
        if s.provider.records_tool_outcomes() {
            *self.tool_errors.entry(provider).or_default() += s.tool_errors;
        }

        self.add_tokens(s, data);

        if s.cost_available {
            // Recorded usage priced at retail, bundled or not, as the analytics
            // page reports it: dropping a Max plan's Claude spend would leave
            // the gauge that most wants to show it empty. `included` is what
            // tells an itemised charge from a retail equivalent. The row's
            // figure wins because a remote row's extraction is an empty default.
            let included = plan.includes(s.provider);
            let key = (provider, included);
            *self.cost_total.entry(key).or_default() += s.total_cost.unwrap_or(data.costs.total);
            *self.cost_today.entry(key).or_default() += s.cost_today;
            *self.cost_hour.entry(key).or_default() += s.cost_hour;
            *self.burn.entry(key).or_default() += s.cost_per_min * 60.0;
            self.add_model_costs(s, data, provider, included);
        }

        if s.is_running()
            && let Some(ctx) = s.context
            && ctx.max > 0
        {
            self.context.push(Context {
                session: s.session_id.chars().take(SHORT_ID).collect(),
                project: project_label(s),
                provider,
                used: ctx.used,
                max: ctx.max,
            });
        }
    }

    /// Tokens by model and kind, from the extraction's per-model breakdown
    /// where there is one.
    ///
    /// A remote row arrives without one — the wire carries only the session's
    /// input and output totals — so its whole count is filed under its current
    /// model, with every input kind folded into `input`.
    fn add_tokens(&mut self, s: &Session, data: &SessionData) {
        let provider = s.provider.as_str();
        let mut put = |model: &str, kind: &'static str, n: u64| {
            if n > 0 {
                *self
                    .tokens
                    .entry((provider, model_label(model), kind))
                    .or_default() += n;
            }
        };
        let mut split = |model: &str, t: &crate::session::Tokens| {
            put(model, "input", t.input);
            put(model, "output", t.output);
            put(model, "cache_read", t.cache_read + t.cached_input);
            put(model, "cache_write", t.cache_write_5m + t.cache_write_1h);
        };
        if !data.model_breakdown.is_empty() {
            for m in &data.model_breakdown {
                split(&m.model, &m.tokens);
            }
        } else if data.tokens.all_input() + data.tokens.output > 0 {
            split(&s.model, &data.tokens);
        } else {
            put(&s.model, "input", s.input_tokens);
            put(&s.model, "output", s.output_tokens);
        }
    }

    /// Cost by model, total and today.
    ///
    /// Today is read off the row's day buckets, which the loader copies from
    /// the extraction for a local row. A remote row's buckets carry one
    /// synthetic model, so a remote session is filed whole under the model it
    /// is on — the only per-model fact that crossed the wire.
    fn add_model_costs(
        &mut self,
        s: &Session,
        data: &SessionData,
        provider: &'static str,
        included: bool,
    ) {
        let remote = s.remote.is_some();
        if !remote && !data.model_breakdown.is_empty() {
            for m in &data.model_breakdown {
                *self
                    .model_total
                    .entry((provider, model_label(&m.model), included))
                    .or_default() += m.total;
            }
        } else {
            *self
                .model_total
                .entry((provider, model_label(&s.model), included))
                .or_default() += s.total_cost.unwrap_or(data.costs.total);
        }

        let today = crate::util::local_date_key(&chrono::Utc::now());
        for (day, models) in &s.costs_by_day {
            if day.as_str() < today.as_str() {
                continue;
            }
            for (model, usd) in models {
                let model = match remote {
                    true => model_label(&s.model),
                    false => model_label(model),
                };
                *self
                    .model_today
                    .entry((provider, model, included))
                    .or_default() += usd;
            }
        }
    }

    fn write(&self, unreadable_hosts: usize) -> String {
        let mut out = Exposition::default();

        out.family(
            "cctop_build_info",
            "gauge",
            "The cctop serving these metrics; always 1, the version is the label.",
        );
        out.sample(
            "cctop_build_info",
            &[("version", env!("CARGO_PKG_VERSION"))],
            1.0,
        );

        out.family(
            "cctop_remote_hosts_unreadable",
            "gauge",
            "Hosts named with --host whose sessions could not be read at the last refresh.",
        );
        out.sample(
            "cctop_remote_hosts_unreadable",
            &[],
            unreadable_hosts as f64,
        );

        out.family(
            "cctop_sessions",
            "gauge",
            "Sessions in the table, by provider and state; idle means no live process.",
        );
        for ((provider, state), n) in &self.sessions {
            out.sample(
                "cctop_sessions",
                &[("provider", provider), ("state", state)],
                *n as f64,
            );
        }

        let costs = [
            (
                "cctop_cost_usd",
                "Estimated cost of every session in the table, in USD at retail rates.",
                &self.cost_total,
            ),
            (
                "cctop_cost_today_usd",
                "Estimated cost recorded since local midnight, in USD.",
                &self.cost_today,
            ),
            (
                "cctop_cost_this_hour_usd",
                "Estimated cost recorded in the current local hour, in USD.",
                &self.cost_hour,
            ),
            (
                "cctop_cost_burn_usd_per_hour",
                "Smoothed live spend rate across sessions, in USD per hour.",
                &self.burn,
            ),
        ];
        for (name, help, values) in costs {
            out.family(name, "gauge", help);
            for ((provider, included), usd) in values {
                out.sample(
                    name,
                    &[("provider", provider), ("included", bool_label(*included))],
                    *usd,
                );
            }
        }

        let by_model = [
            (
                "cctop_model_cost_usd",
                "Estimated cost by model, in USD at retail rates.",
                &self.model_total,
            ),
            (
                "cctop_model_cost_today_usd",
                "Estimated cost by model since local midnight, in USD.",
                &self.model_today,
            ),
        ];
        for (name, help, values) in by_model {
            out.family(name, "gauge", help);
            for ((provider, model, included), usd) in values {
                out.sample(
                    name,
                    &[
                        ("provider", provider),
                        ("model", model),
                        ("included", bool_label(*included)),
                    ],
                    *usd,
                );
            }
        }

        out.family(
            "cctop_tokens",
            "gauge",
            "Tokens recorded, by provider, model and kind (input, output, cache_read, cache_write).",
        );
        for ((provider, model, kind), n) in &self.tokens {
            out.sample(
                "cctop_tokens",
                &[("provider", provider), ("model", model), ("kind", kind)],
                *n as f64,
            );
        }

        out.family(
            "cctop_tool_calls",
            "gauge",
            "Tool calls recorded, by provider.",
        );
        for (provider, n) in &self.tool_calls {
            out.sample("cctop_tool_calls", &[("provider", provider)], *n as f64);
        }
        out.family(
            "cctop_tool_call_errors",
            "gauge",
            "Tool calls the transcript reported as failed; only providers that record outcomes.",
        );
        for (provider, n) in &self.tool_errors {
            out.sample(
                "cctop_tool_call_errors",
                &[("provider", provider)],
                *n as f64,
            );
        }

        let windows = [
            (
                "cctop_context_used_tokens",
                "Tokens in a live session's context window.",
                (|c: &Context| c.used as f64) as fn(&Context) -> f64,
            ),
            (
                "cctop_context_max_tokens",
                "Size of a live session's context window, in tokens.",
                |c: &Context| c.max as f64,
            ),
            (
                "cctop_context_fill_ratio",
                "Share of a live session's context window in use, 0 to 1.",
                |c: &Context| c.used as f64 / c.max as f64,
            ),
        ];
        for (name, help, value) in windows {
            out.family(name, "gauge", help);
            for c in &self.context {
                out.sample(
                    name,
                    &[
                        ("session", &c.session),
                        ("project", &c.project),
                        ("provider", c.provider),
                    ],
                    value(c),
                );
            }
        }

        out.text
    }
}

fn bool_label(b: bool) -> &'static str {
    match b {
        true => "true",
        false => "false",
    }
}

/// The text being built, one family at a time.
#[derive(Default)]
struct Exposition {
    text: String,
}

impl Exposition {
    /// The `# HELP` and `# TYPE` pair that opens a family. Written even for a
    /// family with no samples this scrape, so the metric's description is in
    /// the store before its first value is.
    fn family(&mut self, name: &str, kind: &str, help: &str) {
        let _ = writeln!(self.text, "# HELP {name} {}", escape_help(help));
        let _ = writeln!(self.text, "# TYPE {name} {kind}");
    }

    fn sample(&mut self, name: &str, labels: &[(&str, &str)], value: f64) {
        self.text.push_str(name);
        if !labels.is_empty() {
            self.text.push('{');
            for (i, (k, v)) in labels.iter().enumerate() {
                if i > 0 {
                    self.text.push(',');
                }
                let _ = write!(self.text, "{k}=\"{}\"", escape_label(v));
            }
            self.text.push('}');
        }
        let _ = writeln!(self.text, " {}", number(value));
    }
}

/// A label value as the format requires: backslash, double quote and line feed
/// escaped, everything else — including any UTF-8 — as written.
///
/// Model names and directory names are the free text that reaches a label,
/// and a directory is allowed to be called anything; an unescaped quote in one
/// would end the value early and turn the rest of the line into garbage that
/// fails the whole scrape, not just the one series.
fn escape_label(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

/// HELP text escapes backslash and line feed, but not quotes.
fn escape_help(v: &str) -> String {
    v.replace('\\', "\\\\").replace('\n', "\\n")
}

/// A sample value in the spelling Prometheus parses.
///
/// Rust's `Display` writes finite floats as plain decimals, which is accepted,
/// but spells the infinities `inf` — the format wants `+Inf`, and a value it
/// cannot parse fails the whole scrape.
fn number(v: f64) -> String {
    if v.is_nan() {
        "NaN".to_string()
    } else if v.is_infinite() {
        match v > 0.0 {
            true => "+Inf".to_string(),
            false => "-Inf".to_string(),
        }
    } else {
        v.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;
    use crate::session::{ContextUsage, ModelBreakdown, Tokens};
    use std::collections::HashMap;

    /// A parsed sample line: name, labels in order, value.
    type Sample = (String, Vec<(String, String)>, f64);

    fn valid_name(name: &str) -> bool {
        let mut chars = name.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == ':')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
    }

    /// One sample line split into name, labels and value — a strict reading
    /// of the grammar, so anything the renderer gets wrong fails here the way
    /// it would fail a scrape.
    fn parse_sample(line: &str) -> (String, Vec<(String, String)>, String) {
        let (name, rest) = match line.find('{') {
            Some(i) => (&line[..i], &line[i..]),
            None => {
                let (n, v) = line.split_once(' ').expect("name and value");
                return (n.to_string(), Vec::new(), v.to_string());
            }
        };
        let mut labels = Vec::new();
        let mut chars = rest[1..].chars().peekable();
        loop {
            let key: String = chars.by_ref().take_while(|&c| c != '=').collect();
            assert!(valid_name(&key), "label name {key:?} in {line:?}");
            assert_eq!(chars.next(), Some('"'), "{line:?}");
            let mut value = String::new();
            loop {
                match chars.next().expect("unterminated label value") {
                    '\\' => match chars.next() {
                        Some('\\') => value.push('\\'),
                        Some('"') => value.push('"'),
                        Some('n') => value.push('\n'),
                        other => panic!("bad escape {other:?} in {line:?}"),
                    },
                    '"' => break,
                    '\n' => panic!("raw newline in {line:?}"),
                    c => value.push(c),
                }
            }
            labels.push((key, value));
            match chars.next() {
                Some(',') => continue,
                Some('}') => break,
                other => panic!("expected , or }} got {other:?} in {line:?}"),
            }
        }
        let tail: String = chars.collect();
        let value = tail.strip_prefix(' ').expect("space before value");
        (name.to_string(), labels, value.to_string())
    }

    /// Every line well-formed, every family introduced by HELP then TYPE once
    /// and before its samples, every value a number the format accepts.
    fn check(text: &str) -> Vec<Sample> {
        let mut typed: Vec<String> = Vec::new();
        let mut samples = Vec::new();
        let mut pending_help: Option<String> = None;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("# HELP ") {
                let name = rest.split(' ').next().unwrap();
                assert!(valid_name(name), "{name}");
                pending_help = Some(name.to_string());
            } else if let Some(rest) = line.strip_prefix("# TYPE ") {
                let (name, kind) = rest.split_once(' ').unwrap();
                assert_eq!(
                    pending_help.take().as_deref(),
                    Some(name),
                    "HELP before TYPE"
                );
                assert!(["gauge", "counter"].contains(&kind), "{kind}");
                assert!(!typed.iter().any(|t| t == name), "{name} typed twice");
                typed.push(name.to_string());
            } else {
                let (name, labels, value) = parse_sample(line);
                assert!(valid_name(&name), "{name}");
                assert_eq!(typed.last(), Some(&name), "{name} outside its family");
                let v = match value.as_str() {
                    "+Inf" => f64::INFINITY,
                    "-Inf" => f64::NEG_INFINITY,
                    "NaN" => f64::NAN,
                    v => v.parse().unwrap_or_else(|_| panic!("value {v:?}")),
                };
                samples.push((name, labels, v));
            }
        }
        assert!(text.ends_with('\n'), "the last line needs its line feed");
        samples
    }

    fn find<'a>(samples: &'a [Sample], name: &str, want: &[(&str, &str)]) -> Option<&'a f64> {
        samples
            .iter()
            .find(|(n, labels, _)| {
                n == name
                    && want
                        .iter()
                        .all(|(k, v)| labels.iter().any(|(lk, lv)| lk == k && lv == v))
            })
            .map(|(_, _, v)| v)
    }

    fn live(provider: Provider, id: &str) -> Session {
        let mut s = Session::new(provider, id.to_string());
        s.inferred_running = true;
        s
    }

    #[test]
    fn an_empty_table_is_still_a_valid_exposition() {
        let text = render_with(&[], Plan::Retail, 0, |_| SessionData::default());
        let samples = check(&text);
        assert!(
            find(
                &samples,
                "cctop_build_info",
                &[("version", env!("CARGO_PKG_VERSION"))]
            )
            .is_some()
        );
        assert_eq!(
            find(&samples, "cctop_remote_hosts_unreadable", &[]),
            Some(&0.0)
        );
    }

    #[test]
    fn label_values_are_escaped() {
        let mut s = live(Provider::Claude, "abcdef0123456789");
        s.label_source = "/home/u/we\"ird\\na\nme".to_string();
        s.model = "model \"x\"".to_string();
        s.input_tokens = 5;
        s.context = Some(ContextUsage {
            used: 50,
            max: 200,
            compacted: false,
        });
        let text = render_with(&[s], Plan::Retail, 0, |_| SessionData::default());
        assert!(text.contains(r#"project="we\"ird\\na\nme""#), "{text}");
        assert!(text.contains(r#"model="model \"x\"""#), "{text}");
        let samples = check(&text);
        assert!(
            find(
                &samples,
                "cctop_context_fill_ratio",
                &[("project", "we\"ird\\na\nme")]
            )
            .is_some_and(|v| (*v - 0.25).abs() < 1e-9)
        );
    }

    #[test]
    fn context_series_are_only_for_live_sessions_and_use_a_short_id() {
        let ctx = Some(ContextUsage {
            used: 10,
            max: 100,
            compacted: false,
        });
        let mut on = live(Provider::Claude, "0123456789abcdef");
        on.label_source = "/src/cctop".to_string();
        on.context = ctx;
        let mut off = Session::new(Provider::Claude, "fedcba9876543210".to_string());
        off.context = ctx;
        let text = render_with(&[on, off], Plan::Retail, 0, |_| SessionData::default());
        let samples = check(&text);
        let windows: Vec<_> = samples
            .iter()
            .filter(|(n, _, _)| n == "cctop_context_used_tokens")
            .collect();
        assert_eq!(windows.len(), 1);
        assert!(
            find(
                &samples,
                "cctop_context_used_tokens",
                &[("session", "01234567"), ("project", "cctop")]
            )
            .is_some()
        );
        assert!(!text.contains("fedcba98"));
    }

    #[test]
    fn states_count_live_rows_and_file_ended_ones_as_idle() {
        let mut waiting = live(Provider::Codex, "a");
        waiting.activity_state = ActivityState::WaitingForInput;
        let mut ended = Session::new(Provider::Codex, "b".to_string());
        ended.activity_state = ActivityState::WaitingForInput;
        let text = render_with(&[waiting, ended], Plan::Retail, 0, |_| {
            SessionData::default()
        });
        let samples = check(&text);
        let at = |state| {
            find(
                &samples,
                "cctop_sessions",
                &[("provider", "codex"), ("state", state)],
            )
        };
        assert_eq!(at("waiting"), Some(&1.0));
        assert_eq!(at("idle"), Some(&1.0));
        // Present at zero rather than absent, so an alert on it evaluates.
        assert_eq!(at("error"), Some(&0.0));
    }

    #[test]
    fn cost_and_tokens_split_by_model_and_honour_the_plan() {
        let mut s = live(Provider::Claude, "s1");
        s.total_cost = None; // a Max plan withholds the row's total
        s.cost_today = 1.5;
        s.cost_per_min = 0.1;
        s.tool_count = 10;
        s.tool_errors = 2;
        let today = crate::util::local_date_key(&chrono::Utc::now());
        s.costs_by_day = HashMap::from([(
            today,
            HashMap::from([("opus".to_string(), 1.0), ("haiku".to_string(), 0.5)]),
        )]);
        let mut data = SessionData::default();
        data.costs.total = 3.0;
        let tokens = |input, output, cache_read| Tokens {
            input,
            output,
            cache_read,
            ..Tokens::default()
        };
        data.model_breakdown = vec![
            ModelBreakdown {
                model: "opus".to_string(),
                tokens: tokens(100, 20, 1000),
                costs: Default::default(),
                total: 2.0,
            },
            ModelBreakdown {
                model: "haiku".to_string(),
                tokens: tokens(50, 5, 0),
                costs: Default::default(),
                total: 1.0,
            },
        ];
        let text = render_with(&[s], Plan::Max, 0, |_| data.clone());
        let samples = check(&text);
        let claude = [("provider", "claude"), ("included", "true")];
        assert_eq!(find(&samples, "cctop_cost_usd", &claude), Some(&3.0));
        assert_eq!(find(&samples, "cctop_cost_today_usd", &claude), Some(&1.5));
        assert!(
            find(&samples, "cctop_cost_burn_usd_per_hour", &claude)
                .is_some_and(|v| (*v - 6.0).abs() < 1e-9)
        );
        assert_eq!(
            find(&samples, "cctop_model_cost_usd", &[("model", "opus")]),
            Some(&2.0)
        );
        assert_eq!(
            find(
                &samples,
                "cctop_model_cost_today_usd",
                &[("model", "haiku")]
            ),
            Some(&0.5)
        );
        assert_eq!(
            find(
                &samples,
                "cctop_tokens",
                &[("model", "opus"), ("kind", "cache_read")]
            ),
            Some(&1000.0)
        );
        assert_eq!(
            find(
                &samples,
                "cctop_tool_call_errors",
                &[("provider", "claude")]
            ),
            Some(&2.0)
        );
    }

    #[test]
    fn a_provider_without_outcomes_reports_calls_but_no_errors() {
        let mut s = live(Provider::Cursor, "c");
        s.tool_count = 4;
        let text = render_with(&[s], Plan::Retail, 0, |_| SessionData::default());
        let samples = check(&text);
        assert_eq!(
            find(&samples, "cctop_tool_calls", &[("provider", "cursor")]),
            Some(&4.0)
        );
        assert!(
            find(
                &samples,
                "cctop_tool_call_errors",
                &[("provider", "cursor")]
            )
            .is_none()
        );
    }

    /// The route through the real gate: `serve_connection` on a socket, so
    /// the token check `/metrics` sits behind is the one every page does and
    /// not a copy that could drift from it.
    fn fetch(target: &str) -> String {
        use super::super::{Shared, Snapshot, quota, search, serve_connection};
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::sync::{Arc, Condvar, Mutex};

        let shared = Shared {
            token: "full".to_string(),
            readonly: "view".to_string(),
            metrics: String::new(),
            actions: true,
            port: 7777,
            plan: Plan::Retail,
            latest: Mutex::new(Arc::new(Snapshot {
                version: 1,
                json: "[]".to_string(),
                sessions: vec![live(Provider::Claude, "0123456789")],
                host_errors: Vec::new(),
            })),
            updated: Condvar::new(),
            store: crate::cache::Store::default(),
            quota: Mutex::new(quota::EMPTY.to_string()),
            topics: Mutex::new(search::Topics::default()),
            notify: None,
            hosts: HashMap::new(),
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        client
            .write_all(format!("GET {target} HTTP/1.1\r\n\r\n").as_bytes())
            .unwrap();
        serve_connection(&shared, &mut server);
        drop(server);
        let mut raw = String::new();
        client.read_to_string(&mut raw).unwrap();
        raw
    }

    #[test]
    fn the_route_honours_the_same_tokens_as_every_page() {
        assert!(fetch("/metrics").starts_with("HTTP/1.1 403"));
        assert!(fetch("/metrics?t=wrong").starts_with("HTTP/1.1 403"));
        for token in ["full", "view"] {
            let raw = fetch(&format!("/metrics?t={token}"));
            assert!(raw.starts_with("HTTP/1.1 200"), "{raw}");
            assert!(raw.contains(CONTENT_TYPE), "{raw}");
            let body = raw.split_once("\r\n\r\n").unwrap().1;
            let samples = check(body);
            assert_eq!(
                find(
                    &samples,
                    "cctop_sessions",
                    &[("provider", "claude"), ("state", "working")]
                ),
                Some(&1.0)
            );
        }
    }

    #[test]
    fn non_finite_values_use_the_formats_spelling() {
        assert_eq!(number(f64::INFINITY), "+Inf");
        assert_eq!(number(f64::NEG_INFINITY), "-Inf");
        assert_eq!(number(f64::NAN), "NaN");
        assert_eq!(number(1e-7), "0.0000001");
    }
}
