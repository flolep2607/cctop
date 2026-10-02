//! `cctop compare` — how each model behaved on the work you actually gave it.
//!
//! The honesty problem here is larger than the arithmetic, and it does not go
//! away with more data: **this is observational**. You did not give two models
//! the same work. You gave the expensive one the problems you expected to be
//! hard, and a table that ignores that will report the expensive model as worse
//! while measuring nothing but your own routing.
//!
//! Nothing can fix that from a transcript. Two things make it visible instead:
//! the caveat is printed under every table, and the same figures are broken
//! down by kind of work, because most of "this model is worse" turns out to be
//! "this model was given the debugging".

use super::{Analysis, Task, plural, substantive};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Row {
    pub model: String,
    pub sessions: usize,
    pub cost: f64,
    /// Whether `cost` is a price. False both where the provider records no
    /// usage and where every session priced to zero, because a model missing
    /// from the price table bills `$0.00` and reads as free when it is not.
    pub cost_available: bool,
    pub calls: u64,
    pub edits: u64,
    pub files_edited: u64,
    pub files_one_shot: u64,
    pub files_reworked: u64,
    /// Files this model's subagents wrote for it on another model — see
    /// [`super::Slice`]. Counted in what its cost and time are spread over,
    /// never in its 1-shot or rework rates, which are about its own writing.
    pub files_delegated: u64,
    pub cache_read: u64,
    pub input_total: u64,
    pub active_ms: u64,
    pub truncated: bool,
}

impl Row {
    /// Share of edited files that took one contiguous attempt.
    ///
    /// `None` below a floor of edited files, because a model credited with two
    /// files can only ever report 0%, 50% or 100% and none of those is a rate.
    pub fn one_shot(&self) -> Option<f64> {
        (self.files_edited >= 5)
            .then(|| self.files_one_shot as f64 * 100.0 / self.files_edited as f64)
    }

    /// Share of edited files a fix session reopened within a day — see
    /// [`super::mark_rework`]. The same floor as [`Row::one_shot`], for the
    /// same reason.
    pub fn reworked(&self) -> Option<f64> {
        (self.files_edited >= 5)
            .then(|| self.files_reworked as f64 * 100.0 / self.files_edited as f64)
    }

    /// Cost of each file the model actually changed.
    ///
    /// The figure this table exists for. Cost per call rewards a model that
    /// makes many cheap calls and gets nowhere; cost per edited file is what
    /// the work was for.
    pub fn per_edit(&self) -> Option<f64> {
        (self.cost_available && self.files_produced() > 0)
            .then(|| self.cost / self.files_produced() as f64)
    }

    /// Files this model wrote or had written for it: what its spend bought.
    pub fn files_produced(&self) -> u64 {
        self.files_edited + self.files_delegated
    }

    pub fn per_call(&self) -> Option<f64> {
        (self.cost_available && self.calls > 0).then(|| self.cost / self.calls as f64)
    }

    /// Agent time spent for each file changed, in milliseconds.
    ///
    /// The other half of what a file cost. A model that is free and takes two
    /// days is not cheaper than one that costs ten dollars and takes five
    /// minutes, and a table of dollars alone says it is.
    pub fn time_per_edit(&self) -> Option<i64> {
        (self.files_produced() > 0 && self.active_ms > 0)
            .then(|| (self.active_ms / self.files_produced()) as i64)
    }

    /// Dollars plus time, per file, with time priced at `rate` per hour.
    ///
    /// The bool says the figure is a floor: the model has no price, so only
    /// its time is in it. That is still worth showing — a free model that
    /// took three hours a file has cost at least those three hours — but it
    /// must not be ranked as if the dollars were known to be zero.
    pub fn all_in(&self, rate: f64) -> Option<(f64, bool)> {
        if self.files_produced() == 0 {
            return None;
        }
        let hours = self.active_ms as f64 / 3_600_000.0;
        let dollars = if self.cost_available { self.cost } else { 0.0 };
        Some((
            (dollars + hours * rate) / self.files_produced() as f64,
            !self.cost_available,
        ))
    }

    pub fn cache_hit(&self) -> Option<f64> {
        (self.input_total > 0).then(|| self.cache_read as f64 * 100.0 / self.input_total as f64)
    }
}

fn fold<'a>(analyses: impl Iterator<Item = &'a Analysis>, rate: Option<f64>) -> Vec<Row> {
    let mut acc: HashMap<String, Row> = HashMap::new();
    for a in analyses {
        // Each agent in the session goes to its own model's row. Session-wide
        // figures the transcript cannot split by agent — the cache, the cap —
        // go with the main agent, and a model is counted once per session
        // however many of its subagents ran.
        let mut seen: Vec<&str> = Vec::new();
        let slices = a.agent_slices();
        for (k, s) in slices.iter().enumerate() {
            if s.model.is_empty() {
                continue;
            }
            let row = acc.entry(s.model.clone()).or_insert_with(|| Row {
                model: s.model.clone(),
                ..Default::default()
            });
            if !seen.contains(&s.model.as_str()) {
                seen.push(&s.model);
                row.sessions += 1;
            }
            row.calls += s.calls;
            row.edits += s.edits;
            row.files_edited += s.files_edited;
            row.files_one_shot += s.files_one_shot;
            row.files_reworked += s.files_reworked;
            row.files_delegated += s.delegated;
            // A parent's time is the whole session's, subagents included, so
            // one on the parent's own model would add the same minutes to the
            // same row twice. On another model they are that model's minutes
            // as well — wall time shared, like the files, not split.
            if k == 0 || s.model != slices[0].model {
                row.active_ms += s.active_ms;
            }
            row.truncated |= a.truncated;
            if k == 0 {
                row.cache_read += a.cache_read;
                row.input_total += a.input_total;
            }
            if a.cost_available {
                row.cost += s.cost;
            }
        }
    }
    let mut out: Vec<Row> = acc
        .into_values()
        .map(|mut r| {
            r.cost_available = r.cost > 0.0;
            r
        })
        .collect();
    out.sort_by(|a, b| order(a, b, rate));
    out
}

/// With a rate, cheapest all-in cost per file first, because that is the
/// question the rate was given to answer. A floor ranks by its figure like any
/// other: putting every known total ahead of it listed `$50` above `≥$2`, and
/// the `≥` already says which numbers could rise. Without, biggest spend
/// first. Either way ties fall to the busier model and then the name, so
/// models nothing distinguishes — every unpriced one, say — do not come out
/// in hash order.
fn order(a: &Row, b: &Row, rate: Option<f64>) -> std::cmp::Ordering {
    let primary = match rate {
        Some(rate) => {
            let key = |r: &Row| r.all_in(rate).map_or(f64::INFINITY, |(v, _)| v);
            key(a).total_cmp(&key(b))
        }
        None => b.cost.total_cmp(&a.cost),
    };
    primary
        .then(b.sessions.cmp(&a.sessions))
        .then(a.model.cmp(&b.model))
}

pub fn rows(analyses: &[&Analysis], rate: Option<f64>) -> Vec<Row> {
    fold(analyses.iter().copied().filter(|a| substantive(a)), rate)
}

/// One table per kind of work that more than one model did.
fn by_task(live: &[&Analysis], rate: Option<f64>) -> Vec<(Task, Vec<Row>)> {
    Task::ALL
        .iter()
        .filter_map(|&task| {
            let sub = fold(live.iter().copied().filter(|a| a.task == task), rate);
            (sub.len() >= 2).then_some((task, sub))
        })
        .collect()
}

fn pct(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.0}%")).unwrap_or_else(|| "—".into())
}

fn money(v: Option<f64>) -> String {
    v.map(crate::util::adaptive_usd)
        .unwrap_or_else(|| "—".into())
}

/// Column layout, sized once for every table in the report so they line up.
struct Layout {
    model: usize,
    rate: Option<f64>,
}

impl Layout {
    /// Wide enough for the longest name, within reason. Cutting names at a
    /// fixed 28 turned `canopywave/moonshotai/kimi-k2.5` into a model that
    /// does not exist.
    fn new<'a>(rows: impl Iterator<Item = &'a Row>, rate: Option<f64>) -> Self {
        let longest = rows.map(|r| r.model.chars().count()).max().unwrap_or(0);
        Layout {
            model: longest.clamp(12, 44),
            rate,
        }
    }

    fn header(&self, out: &mut String) {
        use std::fmt::Write as _;
        let _ = write!(
            out,
            "  {:<w$} {:>8} {:>8} {:>7} {:>8} {:>9} {:>9} {:>6}",
            "model",
            "sessions",
            "files",
            "1-shot",
            "reworked",
            "$/file",
            "time/file",
            "cache",
            w = self.model
        );
        if let Some(rate) = self.rate {
            let _ = write!(out, " {:>11}", format!("@${rate:.0}/h"));
        }
        out.push('\n');
    }

    fn line(&self, out: &mut String, r: &Row) {
        use std::fmt::Write as _;
        let model: String = r.model.chars().take(self.model).collect();
        let _ = write!(
            out,
            "  {:<w$} {:>8} {:>8} {:>7} {:>8} {:>9} {:>9} {:>6}",
            model,
            r.sessions,
            match r.files_delegated {
                0 => r.files_edited.to_string(),
                d => format!("{}+{d}", r.files_edited),
            },
            pct(r.one_shot()),
            pct(r.reworked()),
            money(r.per_edit()),
            r.time_per_edit()
                .map(crate::util::compact_duration)
                .unwrap_or_else(|| "—".into()),
            pct(r.cache_hit()),
            w = self.model
        );
        if let Some(rate) = self.rate {
            let all_in = match r.all_in(rate) {
                Some((v, true)) => format!("≥{}", crate::util::adaptive_usd(v)),
                Some((v, false)) => crate::util::adaptive_usd(v),
                None => "—".into(),
            };
            let _ = write!(out, " {all_in:>11}");
        }
        out.push('\n');
    }
}

/// The report as text, with time unpriced.
///
/// Built as a string rather than printed, so the same words reach the terminal
/// and the TUI overlay without a second implementation of the layout.
pub fn report(analyses: &[&Analysis]) -> String {
    report_at(analyses, None)
}

/// The report, with an extra column pricing agent time at `rate` dollars an
/// hour when one is given.
///
/// There is no default rate. What an hour of waiting is worth is the reader's
/// number, not cctop's, and a table that quietly assumed one would rank
/// models by a guess.
pub fn report_at(analyses: &[&Analysis], rate: Option<f64>) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let live: Vec<&Analysis> = analyses
        .iter()
        .copied()
        .filter(|a| substantive(a))
        .collect();
    let table = rows(analyses, rate);
    if table.is_empty() {
        return "No sessions with recorded tool calls, so there is nothing to compare.\n".into();
    }
    let tasks = by_task(&live, rate);
    let layout = Layout::new(table.iter(), rate);

    out.push('\n');
    let active: u64 = table.iter().map(|r| r.active_ms).sum();
    let _ = writeln!(
        out,
        "  {} across {}, {} of agent time",
        plural(live.len(), "session"),
        plural(table.len(), "model"),
        crate::util::compact_duration(active as i64),
    );
    out.push('\n');
    layout.header(&mut out);
    for r in &table {
        layout.line(&mut out, r);
    }
    out.push('\n');

    // The same figures per kind of work, which is where most of an apparent
    // difference between two models turns out to live.
    for (task, sub) in &tasks {
        let _ = writeln!(out, "  {}", task.as_str());
        for r in sub {
            layout.line(&mut out, r);
        }
        out.push('\n');
    }

    let mut say = |lines: &[&str]| {
        for l in lines {
            let _ = writeln!(out, "  {l}");
        }
        out.push('\n');
    };
    say(&[
        "1-shot is the share of files edited without a retry: no failed",
        "command or edit by the same agent between two edits of the file.",
        "reworked is the share a fix session edited again within a day.",
    ]);
    if table.iter().any(|r| r.files_delegated > 0) {
        say(&[
            "Subagents are credited to their own model. files 4+9 is four",
            "files a model wrote and nine its subagents on another model wrote",
            "for it: its $/file and time/file are spread over all thirteen,",
            "because briefing them and using what came back is its work too.",
            "Its 1-shot and reworked rates are its own four alone.",
        ]);
    }
    say(&[
        "time/file is the agent's working time per file changed: the gaps",
        "between its tool calls, less any pause of over five minutes that",
        "was not a tool still running.",
    ]);
    match rate {
        Some(_) => say(&[
            "The last column is dollars plus that time at the rate given, per",
            "file. ≥ marks a model with no price, so only its time is counted.",
        ]),
        None => say(&[
            "A cheap model that takes hours is not cheap. --rate 60 prices",
            "agent time at $60 an hour and ranks by the total per file.",
        ]),
    }
    if table.iter().any(|r| !r.cost_available) {
        say(&[
            "— under $/file is no price: the harness recorded none, the model",
            "is missing from the price table, or it is free.",
        ]);
    }
    say(&[
        "Observational, not an experiment: these models were not given",
        "the same work, so a difference here may be your routing rather",
        "than the model. The per-task tables above are the closest this",
        "can get to comparing like with like.",
    ]);
    say(&[
        "One agent that switched models mid-way is credited to the one",
        "that cost it the most — the transcript records which model billed",
        "a request, not which one asked for a given tool call.",
    ]);
    if table.iter().any(|r| r.truncated) {
        say(&["Some counts are floors: the per-session tool history is capped."]);
    }
    out
}

/// The table as JSON, for scripting.
pub fn as_json(analyses: &[&Analysis], rate: Option<f64>) -> String {
    let row = |r: &Row| {
        let all_in = rate.and_then(|rate| r.all_in(rate));
        serde_json::json!({
            "model": r.model,
            "sessions": r.sessions,
            "usd": r.cost_available.then_some(r.cost),
            "calls": r.calls,
            "edits": r.edits,
            "files_edited": r.files_edited,
            "files_delegated": r.files_delegated,
            "one_shot_pct": r.one_shot(),
            "files_reworked": r.files_reworked,
            "reworked_pct": r.reworked(),
            "usd_per_file": r.per_edit(),
            "usd_per_call": r.per_call(),
            "active_ms": r.active_ms,
            "active_ms_per_file": r.time_per_edit(),
            "all_in_usd_per_file": all_in.map(|(v, _)| v),
            "all_in_is_floor": all_in.map(|(_, floor)| floor),
            "cache_hit_pct": r.cache_hit(),
            "counts_are_floors": r.truncated,
        })
    };
    let live: Vec<&Analysis> = analyses
        .iter()
        .copied()
        .filter(|a| substantive(a))
        .collect();
    let by_task: Vec<serde_json::Value> = by_task(&live, rate)
        .iter()
        .map(|(task, sub)| {
            serde_json::json!({
                "task": task.as_str(),
                "models": sub.iter().map(row).collect::<Vec<_>>(),
            })
        })
        .collect();
    let doc = serde_json::json!({
        "sessions": live.len(),
        "usd_per_hour": rate,
        "models": rows(analyses, rate).iter().map(row).collect::<Vec<_>>(),
        "by_task": by_task,
        "caveat": "Observational. These models were not given the same work, so \
                   a difference may be routing rather than the model. Subagents are \
                   credited to their own model; an agent that switched models is \
                   credited to the one that cost it the most. files_delegated \
                   count towards a parent's usd_per_file and time, not its rates.",
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    fn session(model: &str, cost: f64, files: u64, active_ms: u64) -> Analysis {
        Analysis {
            provider: Provider::Claude,
            label: "repo".into(),
            model: model.into(),
            cost,
            cost_available: true,
            task: Task::Coding,
            calls: 40,
            errors: 0,
            records_outcomes: true,
            edits: files,
            bash_writes: 0,
            reads: 0,
            files_edited: files,
            files_one_shot: files,
            read_paths: Default::default(),
            junk_reads: 0,
            junk_tokens: 0,
            reread_tokens: 0,
            rereads: 0,
            cache_read: 0,
            input_total: 0,
            active_ms,
            edited: Default::default(),
            slices: Vec::new(),
            files_reworked: 0,
            truncated: false,
        }
    }

    const HOUR: u64 = 3_600_000;

    /// The case this column exists for: free and slow against paid and fast.
    /// Dollars alone rank the free model first; an hour priced at $60 does not.
    #[test]
    fn time_can_outweigh_price() {
        let sessions = [
            session("fast", 10.0, 10, HOUR / 12),
            session("slow", 0.5, 10, 48 * HOUR),
        ];
        let refs: Vec<&Analysis> = sessions.iter().collect();

        let by_spend = rows(&refs, None);
        assert_eq!(by_spend[0].model, "fast", "biggest spend first");

        let all_in = rows(&refs, Some(60.0));
        assert_eq!(all_in[0].model, "fast", "five minutes beats two days");
        let (fast, floor) = all_in[0].all_in(60.0).unwrap();
        assert!(!floor);
        assert!((fast - 1.5).abs() < 1e-9, "($10 + 1/12 h × $60) / 10 files");
    }

    /// A model the price table does not know bills `$0.00`, and printing that
    /// as its cost per file calls it free.
    #[test]
    fn an_unpriced_model_is_not_free() {
        let sessions = [
            session("known", 4.0, 5, HOUR),
            session("unknown", 0.0, 5, HOUR),
        ];
        let refs: Vec<&Analysis> = sessions.iter().collect();
        let table = rows(&refs, Some(60.0));
        let unknown = table.iter().find(|r| r.model == "unknown").unwrap();
        assert_eq!(unknown.per_edit(), None);
        assert_eq!(
            unknown.all_in(60.0),
            Some((12.0, true)),
            "time only, a floor"
        );
        assert_eq!(
            table[0].model, "unknown",
            "a floor ranks by its figure: ≥$12 is below $12.80"
        );

        let text = report_at(&refs, Some(60.0));
        assert!(text.contains("≥$12"), "{text}");
        assert!(!text.contains("$0.00"), "{text}");
    }

    /// The model column used to cut names at 28, which printed a model that
    /// does not exist.
    #[test]
    fn long_model_names_are_not_cut() {
        let name = "canopywave/moonshotai/kimi-k2.5";
        let sessions = [session(name, 1.0, 5, HOUR), session("other", 1.0, 5, HOUR)];
        let refs: Vec<&Analysis> = sessions.iter().collect();
        assert!(report(&refs).contains(name));
    }
}
