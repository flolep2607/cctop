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
    pub cost_available: bool,
    pub calls: u64,
    pub edits: u64,
    pub files_edited: u64,
    pub files_one_shot: u64,
    pub cache_read: u64,
    pub input_total: u64,
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

    /// Cost of each file the model actually changed.
    ///
    /// The figure this table exists for. Cost per call rewards a model that
    /// makes many cheap calls and gets nowhere; cost per edited file is what
    /// the work was for.
    pub fn per_edit(&self) -> Option<f64> {
        (self.cost_available && self.files_edited > 0).then(|| self.cost / self.files_edited as f64)
    }

    pub fn per_call(&self) -> Option<f64> {
        (self.cost_available && self.calls > 0).then(|| self.cost / self.calls as f64)
    }

    pub fn cache_hit(&self) -> Option<f64> {
        (self.input_total > 0).then(|| self.cache_read as f64 * 100.0 / self.input_total as f64)
    }
}

fn fold<'a>(analyses: impl Iterator<Item = &'a Analysis>) -> Vec<Row> {
    let mut acc: HashMap<String, Row> = HashMap::new();
    for a in analyses {
        if a.model.is_empty() {
            continue;
        }
        let row = acc.entry(a.model.clone()).or_insert_with(|| Row {
            model: a.model.clone(),
            ..Default::default()
        });
        row.sessions += 1;
        row.calls += a.calls;
        row.edits += a.edits;
        row.files_edited += a.files_edited;
        row.files_one_shot += a.files_one_shot;
        row.cache_read += a.cache_read;
        row.input_total += a.input_total;
        row.truncated |= a.truncated;
        if a.cost_available {
            row.cost += a.cost;
            row.cost_available = true;
        }
    }
    let mut out: Vec<Row> = acc.into_values().collect();
    out.sort_by(|a, b| {
        b.cost
            .partial_cmp(&a.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

pub fn rows(analyses: &[&Analysis]) -> Vec<Row> {
    fold(analyses.iter().copied().filter(|a| substantive(a)))
}

fn pct(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.0}%")).unwrap_or_else(|| "—".into())
}

fn money(v: Option<f64>) -> String {
    v.map(crate::util::adaptive_usd)
        .unwrap_or_else(|| "—".into())
}

fn header(out: &mut String) {
    use std::fmt::Write as _;
    let _ = writeln!(
        out,
        "  {:<28} {:>8} {:>8} {:>9} {:>9} {:>7}",
        "model", "sessions", "1-shot", "$/file", "$/call", "cache"
    );
}

fn line(out: &mut String, r: &Row) {
    use std::fmt::Write as _;
    let model: String = r.model.chars().take(28).collect();
    let _ = writeln!(
        out,
        "  {:<28} {:>8} {:>8} {:>9} {:>9} {:>7}",
        model,
        r.sessions,
        pct(r.one_shot()),
        money(r.per_edit()),
        money(r.per_call()),
        pct(r.cache_hit()),
    );
}

/// The report as text.
///
/// Built as a string rather than printed, so the same words reach the terminal
/// and the TUI overlay without a second implementation of the layout.
pub fn report(analyses: &[&Analysis]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let live: Vec<&Analysis> = analyses
        .iter()
        .copied()
        .filter(|a| substantive(a))
        .collect();
    let table = rows(analyses);
    if table.is_empty() {
        return "No sessions with recorded tool calls, so there is nothing to compare.\n".into();
    }

    out.push('\n');
    let _ = writeln!(
        out,
        "  {} across {}",
        plural(live.len(), "session"),
        plural(table.len(), "model")
    );
    out.push('\n');
    header(&mut out);
    for r in &table {
        line(&mut out, r);
    }
    out.push('\n');

    // The same figures per kind of work, which is where most of an apparent
    // difference between two models turns out to live.
    for task in Task::ALL {
        let subset: Vec<&Analysis> = live.iter().copied().filter(|a| a.task == task).collect();
        let sub = fold(subset.iter().copied());
        if sub.len() < 2 {
            continue;
        }
        let _ = writeln!(out, "  {}", task.as_str());
        for r in &sub {
            line(&mut out, r);
        }
        out.push('\n');
    }

    let _ = writeln!(
        out,
        "  Observational, not an experiment: these models were not given"
    );
    let _ = writeln!(
        out,
        "  the same work, so a difference here may be your routing rather"
    );
    let _ = writeln!(
        out,
        "  than the model. The per-task tables above are the closest this"
    );
    let _ = writeln!(out, "  can get to comparing like with like.");
    out.push('\n');
    let _ = writeln!(
        out,
        "  A session that used several models is credited entirely to the"
    );
    let _ = writeln!(
        out,
        "  one that cost the most — the transcript records which model"
    );
    let _ = writeln!(
        out,
        "  billed a request, not which one asked for a given tool call."
    );
    if table.iter().any(|r| r.truncated) {
        out.push('\n');
        let _ = writeln!(
            out,
            "  Some counts are floors: the per-session tool history is capped."
        );
    }
    out.push('\n');
    out
}

/// The table as JSON, for scripting.
pub fn as_json(analyses: &[&Analysis]) -> String {
    let row = |r: &Row| {
        serde_json::json!({
            "model": r.model,
            "sessions": r.sessions,
            "usd": r.cost_available.then_some(r.cost),
            "calls": r.calls,
            "edits": r.edits,
            "files_edited": r.files_edited,
            "one_shot_pct": r.one_shot(),
            "usd_per_file": r.per_edit(),
            "usd_per_call": r.per_call(),
            "cache_hit_pct": r.cache_hit(),
            "counts_are_floors": r.truncated,
        })
    };
    let live: Vec<&Analysis> = analyses
        .iter()
        .copied()
        .filter(|a| substantive(a))
        .collect();
    let by_task: Vec<serde_json::Value> = Task::ALL
        .iter()
        .filter_map(|task| {
            let subset: Vec<&Analysis> = live.iter().copied().filter(|a| a.task == *task).collect();
            let sub = fold(subset.iter().copied());
            (sub.len() >= 2).then(|| {
                serde_json::json!({
                    "task": task.as_str(),
                    "models": sub.iter().map(row).collect::<Vec<_>>(),
                })
            })
        })
        .collect();
    let doc = serde_json::json!({
        "sessions": live.len(),
        "models": rows(analyses).iter().map(row).collect::<Vec<_>>(),
        "by_task": by_task,
        "caveat": "Observational. These models were not given the same work, so \
                   a difference may be routing rather than the model. A session \
                   that used several models is credited entirely to the one that \
                   cost the most.",
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}
