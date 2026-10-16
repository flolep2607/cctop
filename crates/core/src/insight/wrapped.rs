//! The year in review behind `Y` on the table: one year with the agents,
//! reduced to the handful of figures a recap shows, one card each.
//!
//! Two sources, because neither covers the year alone.
//!
//! The transcripts know everything — what each hour cost and on which model,
//! which files were edited, what kind of work it was — but only for as long as
//! they are kept. Claude Code deletes one after `cleanupPeriodDays`, 30 unless
//! raised, so for most people they reach back a month.
//!
//! Claude Code's prompt history, `history.jsonl`, is kept until it is deleted
//! by hand: every prompt typed, with its time, project and session. It cannot
//! say what anything cost, but it can say when someone was working and on
//! what, for the whole year.
//!
//! So the figures split along that line. When and where — active days, the
//! streak, the clock, the months, sessions, projects — are the union of both,
//! counted in hours with an agent at work, a unit both sources can speak and
//! that a session in both does not count twice. What and how much — spend,
//! models, files, the kind of work — are the transcripts' alone. Where Claude
//! Code's start well after its own history does, the cleanup is why, and
//! [`Wrapped::cleaned_up`] says so rather than showing a month as a year.
//!
//! And every machine cctop reads counts, not only this one. A remote runs
//! `cctop --wrapped YEAR` and answers with its [`Tally`] — the sets and sums,
//! not the finished figures — because the figures do not add: an hour worked
//! on two machines is one hour, and a streak belongs to the union of the days.
//! Hours travel as UTC, so a server that keeps UTC lands on the viewer's clock.
//!
//! ponytail: `stats-cache.json`, behind `/usage`, is also kept and has tokens
//! per model per day, but it starts keeping those when it is first written,
//! which in practice is no earlier than the transcripts. Worth reading the
//! day it reaches further back than they do.

use super::{Analysis, Task};
use crate::session::Session;
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// How many entries a ranked card shows.
const TOP: usize = 5;

/// One year, as the recap's cards read it.
#[derive(Debug, Clone, Default)]
pub struct Wrapped {
    pub year: i32,
    /// The first day of `year` anything was found on, in either source.
    pub since: Option<NaiveDate>,
    /// The first day Claude Code's transcripts cover, and its prompt
    /// history: the two that `cleanupPeriodDays` pulls apart. Other harnesses
    /// keep their transcripts, so theirs say nothing about a cleanup.
    pub claude_since: Option<NaiveDate>,
    pub history_since: Option<NaiveDate>,
    pub sessions: usize,
    pub projects: usize,
    /// Prompts typed into Claude Code — the history records no other harness.
    pub prompts: usize,
    /// What the transcripts' tokens cost at list price, whatever the plan: a
    /// subscriber's figure is what the subscription was worth, not what was
    /// paid.
    pub cost: f64,
    pub tokens: u64,
    pub calls: u64,
    pub files_edited: u64,
    pub files_one_shot: u64,
    /// Distinct clock hours with an agent at work.
    pub hours: usize,
    /// Distinct days with any.
    pub days: usize,
    /// The longest run of consecutive active days, and the day it ended.
    pub streak: (usize, Option<NaiveDate>),
    /// The day with the most active hours.
    pub busiest_day: Option<(NaiveDate, usize)>,
    /// Active hours by month, January first.
    pub by_month: [u32; 12],
    /// Active hours by hour of day, midnight first.
    pub by_hour: [u32; 24],
    /// Ranked by spend, highest first.
    pub models: Vec<(String, f64)>,
    /// Project name and active hours, ranked by hours.
    pub top_projects: Vec<(String, usize)>,
    /// What kind of work, by sessions — see [`Task`].
    pub tasks: Vec<(Task, usize)>,
    /// Slash commands typed, by count.
    pub commands: Vec<(String, usize)>,
    /// The most expensive single session: its title, and its spend this year.
    pub biggest: Option<(String, f64)>,
    /// How many machines answered, this one included.
    pub machines: usize,
    /// The remotes that did not, and why — so a year short by a machine says
    /// so rather than passing for the whole of it.
    pub unreachable: Vec<(String, String)>,
}

impl Wrapped {
    /// The hour of day worked most, as a person's rhythm.
    pub fn peak_hour(&self) -> Option<usize> {
        argmax(&self.by_hour)
    }

    /// The month worked most, `0` for January.
    pub fn peak_month(&self) -> Option<usize> {
        argmax(&self.by_month)
    }

    /// Whether Claude Code's transcripts start well after its history does:
    /// spend from before then was deleted, and is worth a word about keeping
    /// next year's.
    pub fn cleaned_up(&self) -> bool {
        match (self.history_since, self.claude_since) {
            (Some(history), Some(kept)) => (kept - history).num_days() > 14,
            (Some(_), None) => true,
            _ => false,
        }
    }
}

/// One machine's year before it becomes figures: what can be added to another
/// machine's without counting anything twice.
///
/// An hour is hours since the epoch, UTC: the one form two machines in two
/// time zones agree on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tally {
    /// Each hour with an agent at work, with the project it was on.
    pub hours: BTreeSet<(i64, String)>,
    pub sessions: BTreeSet<String>,
    /// The first hour Claude Code's transcripts cover, and its history.
    pub claude_since: Option<i64>,
    pub history_since: Option<i64>,
    pub prompts: usize,
    pub cost: f64,
    pub tokens: u64,
    pub calls: u64,
    pub files_edited: u64,
    pub files_one_shot: u64,
    pub models: BTreeMap<String, f64>,
    /// By [`Task::as_str`], which is what survives the wire.
    pub tasks: BTreeMap<String, usize>,
    pub commands: BTreeMap<String, usize>,
    pub biggest: Option<(String, f64)>,
}

impl Tally {
    /// Fold another machine's year into this one.
    pub fn merge(&mut self, other: Tally) {
        self.hours.extend(other.hours);
        self.sessions.extend(other.sessions);
        self.claude_since = earliest(self.claude_since, other.claude_since);
        self.history_since = earliest(self.history_since, other.history_since);
        self.prompts += other.prompts;
        self.cost += other.cost;
        self.tokens += other.tokens;
        self.calls += other.calls;
        self.files_edited += other.files_edited;
        self.files_one_shot += other.files_one_shot;
        for (k, v) in other.models {
            *self.models.entry(k).or_default() += v;
        }
        for (k, v) in other.tasks {
            *self.tasks.entry(k).or_default() += v;
        }
        for (k, v) in other.commands {
            *self.commands.entry(k).or_default() += v;
        }
        if let Some((_, theirs)) = &other.biggest
            && self.biggest.as_ref().is_none_or(|(_, ours)| theirs > ours)
        {
            self.biggest = other.biggest;
        }
    }
}

fn earliest(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// One line of `history.jsonl`, as far as the recap reads it.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Prompt {
    /// Milliseconds since the epoch.
    pub timestamp: i64,
    #[serde(default)]
    pub project: String,
    #[serde(default, rename = "sessionId")]
    pub session_id: String,
    #[serde(default)]
    pub display: String,
}

/// Every prompt in Claude Code's history. Empty when there is none or it
/// cannot be read: the recap then stands on the transcripts alone.
///
/// A line that does not parse is skipped rather than ending the read — the
/// file is appended to by a running Claude Code, and its last line may be
/// half written.
///
/// ponytail: the default configuration directory only. A second Claude
/// account's history is not read; add it when someone with two wants a
/// recap of both.
pub fn read_history() -> Vec<Prompt> {
    // A test reading the developer's own prompts would pass or fail by
    // whatever they typed this year.
    if crate::under_test() {
        return Vec::new();
    }
    let path = crate::config::CLAUDE_CONFIG_DIR.join("history.jsonl");
    std::fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// This machine's `year`, from `sessions` and their analyses in the same
/// order — which is how [`super::from_store`] returns them — and the prompt
/// history.
///
/// A session counts towards the year if it started in it or worked in it. Its
/// spend is split by the buckets, so one that ran over New Year is charged to
/// each side for its own hours.
///
/// ponytail: the year is this machine's local one here and the viewer's in
/// [`finish`], so a remote a timezone away can lose the hours either side of
/// its own New Year. And tokens, calls and files are a session's whole, not
/// split at the boundary — those are not bucketed by time. Both are off by a
/// few hours' worth, once a year.
pub fn tally(year: i32, sessions: &[Session], analyses: &[Analysis], history: &[Prompt]) -> Tally {
    debug_assert_eq!(sessions.len(), analyses.len());
    let prefix = format!("{year:04}-");
    let mut t = Tally::default();

    for (s, a) in sessions.iter().zip(analyses) {
        let cwd = match a.cwd.is_empty() {
            true => &s.label_source,
            false => &a.cwd,
        };
        let project = project_name(cwd);
        let claude = s.provider == crate::pricing::Provider::Claude;
        let mut spent = 0.0;
        let mut worked = false;
        for (key, by_model) in s.costs_by_hour.iter() {
            if !key.starts_with(&prefix) {
                continue;
            }
            let Some(hour) = local_key_hour(key) else {
                continue;
            };
            worked = true;
            t.hours.insert((hour, project.clone()));
            if claude {
                t.claude_since = earliest(t.claude_since, Some(hour));
            }
            for (model, usd) in by_model {
                spent += usd;
                *t.models.entry(crate::util::short_model(model)).or_default() += usd;
            }
        }

        // A harness that records no usage has no buckets; its start is the
        // one hour it can be placed in.
        let started = [&s.started_at, &s.last_active]
            .into_iter()
            .find_map(|ts| crate::util::parse_ts(ts))
            .filter(|ts| ts.with_timezone(&Local).year() == year);
        if !worked && started.is_none() {
            continue;
        }
        if let Some(ts) = started {
            let hour = ts.timestamp().div_euclid(3600);
            t.hours.insert((hour, project.clone()));
            if claude {
                t.claude_since = earliest(t.claude_since, Some(hour));
            }
        }

        t.sessions.insert(s.session_id.clone());
        t.cost += spent;
        t.tokens += s.input_tokens + s.output_tokens;
        t.calls += a.calls;
        t.files_edited += a.files_edited;
        t.files_one_shot += a.files_one_shot;
        if super::substantive(a) {
            *t.tasks.entry(a.task.as_str().to_string()).or_default() += 1;
        }
        if spent > 0.0 && t.biggest.as_ref().is_none_or(|(_, c)| spent > *c) {
            let title = s.title.clone().filter(|title| !title.trim().is_empty());
            t.biggest = Some((title.unwrap_or(project), spent));
        }
    }

    for p in history {
        let Some(at) = DateTime::from_timestamp_millis(p.timestamp) else {
            continue;
        };
        if at.with_timezone(&Local).year() != year {
            continue;
        }
        let hour = p.timestamp.div_euclid(3_600_000);
        t.hours.insert((hour, project_name(&p.project)));
        t.history_since = earliest(t.history_since, Some(hour));
        if !p.session_id.is_empty() {
            t.sessions.insert(p.session_id.clone());
        }
        t.prompts += 1;
        if let Some(command) = p
            .display
            .split_whitespace()
            .next()
            .filter(|c| is_command(c))
        {
            *t.commands.entry(command.to_string()).or_default() += 1;
        }
    }
    t
}

/// The figures, from every machine's tallies merged into one, on this
/// machine's clock.
pub fn finish(year: i32, t: Tally) -> Wrapped {
    let local =
        |hour: i64| DateTime::from_timestamp(hour * 3600, 0).map(|d| d.with_timezone(&Local));
    let mut w = Wrapped {
        year,
        ..Default::default()
    };
    let mut slots: HashSet<i64> = HashSet::new();
    let mut by_day: HashMap<NaiveDate, usize> = HashMap::new();
    let mut per_project: HashMap<String, usize> = HashMap::new();
    for (hour, project) in t.hours {
        let Some(at) = local(hour).filter(|at| at.year() == year) else {
            continue;
        };
        *per_project.entry(project).or_default() += 1;
        if slots.insert(hour) {
            *by_day.entry(at.date_naive()).or_default() += 1;
            w.by_month[at.month0() as usize] += 1;
            w.by_hour[at.hour() as usize] += 1;
        }
    }

    let mut days: Vec<NaiveDate> = by_day.keys().copied().collect();
    days.sort();
    w.since = days.first().copied();
    w.claude_since = t.claude_since.and_then(local).map(|d| d.date_naive());
    w.history_since = t.history_since.and_then(local).map(|d| d.date_naive());
    w.hours = slots.len();
    w.days = days.len();
    w.streak = streak(&days);
    w.busiest_day = by_day
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)));
    w.sessions = t.sessions.len();
    w.projects = per_project.len();
    w.prompts = t.prompts;
    w.cost = t.cost;
    w.tokens = t.tokens;
    w.calls = t.calls;
    w.files_edited = t.files_edited;
    w.files_one_shot = t.files_one_shot;
    w.biggest = t.biggest;
    w.top_projects = ranked(per_project, |n| *n as f64);
    w.models = ranked(t.models.into_iter().filter(|(_, usd)| *usd > 0.0), |v| *v);
    let tasks = t.tasks.into_iter().filter_map(|(name, n)| {
        Task::ALL
            .into_iter()
            .find(|task| task.as_str() == name)
            .map(|task| (task, n))
    });
    w.tasks = ranked(tasks, |n| *n as f64);
    w.commands = ranked(t.commands, |n| *n as f64);
    w.machines = 1;
    w
}

/// `YYYY-MM-DDTHH` on this machine's clock, as the cost buckets are keyed, as
/// hours since the epoch.
fn local_key_hour(key: &str) -> Option<i64> {
    let naive =
        NaiveDateTime::parse_from_str(&format!("{}:00", key.get(..13)?), "%Y-%m-%dT%H:%M").ok()?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|at| at.timestamp().div_euclid(3600))
}

/// `/clear`, `/model` — and not a path someone pasted, which also starts with
/// a slash.
fn is_command(word: &str) -> bool {
    word.len() > 1
        && word.starts_with('/')
        && word[1..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':'))
}

/// Highest first, ties by name so the same year always draws the same card.
fn ranked<K: Ord, V>(
    items: impl IntoIterator<Item = (K, V)>,
    by: impl Fn(&V) -> f64,
) -> Vec<(K, V)> {
    let mut v: Vec<(K, V)> = items.into_iter().collect();
    v.sort_by(|a, b| by(&b.1).total_cmp(&by(&a.1)).then(a.0.cmp(&b.0)));
    v.truncate(TOP);
    v
}

/// The leaf of the checkout `cwd` belongs to — a worktree counts as its repo.
fn project_name(cwd: &str) -> String {
    let path = super::checkout_path(cwd);
    let leaf = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    match leaf.is_empty() {
        true => "~".to_string(),
        false => leaf.to_string(),
    }
}

/// The longest run of consecutive days in sorted, distinct `days`.
fn streak(days: &[NaiveDate]) -> (usize, Option<NaiveDate>) {
    let mut best = (0, None);
    let mut run = 0;
    for (i, day) in days.iter().enumerate() {
        run = match i > 0 && days[i - 1].succ_opt() == Some(*day) {
            true => run + 1,
            false => 1,
        };
        if run > best.0 {
            best = (run, Some(*day));
        }
    }
    best
}

fn argmax(values: &[u32]) -> Option<usize> {
    values
        .iter()
        .enumerate()
        .filter(|(_, v)| **v > 0)
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(&a.0)))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;
    use std::sync::Arc;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn spent(id: &str, cwd: &str, hours: &[(&str, &str, f64)]) -> (Session, Analysis) {
        let mut s = Session::new(Provider::Claude, id.into());
        s.cost_available = true;
        s.label_source = cwd.into();
        let mut buckets: HashMap<String, HashMap<String, f64>> = HashMap::new();
        for (hour, model, usd) in hours {
            *buckets
                .entry(hour.to_string())
                .or_default()
                .entry(model.to_string())
                .or_default() += usd;
        }
        s.costs_by_hour = Arc::new(buckets);
        let a = super::super::analyse(&s, &Default::default());
        (s, a)
    }

    /// A prompt at a local wall-clock time, as the history records it.
    fn typed(at: &str, project: &str, session: &str, display: &str) -> Prompt {
        let local = NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M").unwrap();
        Prompt {
            timestamp: Local
                .from_local_datetime(&local)
                .unwrap()
                .timestamp_millis(),
            project: project.into(),
            session_id: session.into(),
            display: display.into(),
        }
    }

    fn year(sessions: &[(Session, Analysis)], history: &[Prompt]) -> Wrapped {
        let (s, a): (Vec<Session>, Vec<Analysis>) = sessions.iter().cloned().unzip();
        finish(2026, tally(2026, &s, &a, history))
    }

    #[test]
    fn a_streak_is_the_longest_run_of_consecutive_days() {
        let days = [
            day("2026-01-01"),
            day("2026-01-02"),
            day("2026-01-05"),
            day("2026-01-06"),
            day("2026-01-07"),
            day("2026-02-01"),
        ];
        assert_eq!(streak(&days), (3, Some(day("2026-01-07"))));
        assert_eq!(streak(&[]), (0, None));
    }

    /// The year is cut out of each session's buckets, so spend from the year
    /// before is not charged to this one.
    #[test]
    fn only_the_years_own_spend_is_counted() {
        let w = year(
            &[
                spent(
                    "a",
                    "/src/cctop",
                    &[
                        ("2025-12-31T23", "claude-opus-5", 9.0),
                        ("2026-03-04T22", "claude-opus-5", 2.0),
                        ("2026-03-04T23", "claude-sonnet-5", 5.0),
                    ],
                ),
                spent("b", "/src/web", &[("2026-03-05T22", "claude-opus-5", 1.0)]),
            ],
            &[],
        );

        assert_eq!(w.sessions, 2);
        assert_eq!(w.projects, 2);
        assert_eq!(w.cost, 8.0);
        assert_eq!(w.since, Some(day("2026-03-04")));
        assert_eq!(w.hours, 3);
        assert_eq!(w.streak, (2, Some(day("2026-03-05"))));
        assert_eq!(w.busiest_day, Some((day("2026-03-04"), 2)));
        assert_eq!(w.peak_month(), Some(2));
        assert_eq!(w.peak_hour(), Some(22));
        assert_eq!(w.models[0], ("sonnet-5".to_string(), 5.0));
        assert_eq!(w.top_projects[0], ("cctop".to_string(), 2));
    }

    /// The history reaches back past the transcripts: it extends the when and
    /// where, adds nothing to the spend, and a session in both is one session
    /// and one hour.
    #[test]
    fn the_history_reaches_back_where_the_transcripts_cannot() {
        let history = [
            typed("2026-03-01T09:15", "/src/old", "gone", "/clear"),
            typed("2026-03-01T09:40", "/src/old", "gone", "fix the parser"),
            typed("2026-10-01T10:05", "/src/cctop", "live", "/model opus"),
            typed("2026-10-01T10:20", "/src/cctop", "live", "/clear"),
            typed("2025-12-31T09:00", "/src/old", "last-year", "/clear"),
            typed(
                "2026-10-01T10:30",
                "/src/cctop",
                "live",
                "/home/flo/x.png look",
            ),
        ];
        let w = year(
            &[spent(
                "live",
                "/src/cctop",
                &[("2026-10-01T10", "claude-opus-5", 4.0)],
            )],
            &history,
        );

        assert_eq!(w.since, Some(day("2026-03-01")));
        assert_eq!(w.claude_since, Some(day("2026-10-01")));
        assert_eq!(w.history_since, Some(day("2026-03-01")));
        assert!(w.cleaned_up());
        assert_eq!(w.sessions, 2);
        assert_eq!(w.projects, 2);
        assert_eq!(w.prompts, 5);
        assert_eq!(w.hours, 2);
        assert_eq!(w.cost, 4.0);
        assert_eq!(
            w.commands,
            vec![("/clear".to_string(), 2), ("/model".to_string(), 1)]
        );
    }

    /// Two machines' years: an hour worked on both is one hour, the same repo
    /// on both is one project, and the spend and sessions add up — and it all
    /// survives the JSON a remote sends it as.
    #[test]
    fn machines_merge_without_counting_an_hour_twice() {
        let (s1, a1) = spent(
            "here",
            "/src/cctop",
            &[("2026-05-01T10", "claude-opus-5", 3.0)],
        );
        let (s2, a2) = spent(
            "there",
            "/home/me/cctop",
            &[
                ("2026-05-01T10", "gpt-5.5", 2.0),
                ("2026-05-02T11", "gpt-5.5", 1.0),
            ],
        );
        let mut here = tally(2026, &[s1], &[a1], &[]);
        let there = tally(2026, &[s2], &[a2], &[]);
        let wire: Tally = serde_json::from_str(&serde_json::to_string(&there).unwrap()).unwrap();
        assert_eq!(wire, there);
        here.merge(wire);
        let w = finish(2026, here);

        assert_eq!(w.hours, 2);
        assert_eq!(w.days, 2);
        assert_eq!(w.projects, 1);
        assert_eq!(w.sessions, 2);
        assert_eq!(w.cost, 6.0);
        // A tie, broken by name, so the card is the same on every draw.
        assert_eq!(
            w.models,
            vec![("gpt-5.5".to_string(), 3.0), ("opus-5".to_string(), 3.0)]
        );
        assert_eq!(w.biggest.map(|b| b.1), Some(3.0));
    }

    #[test]
    fn a_session_from_another_year_is_left_out() {
        let w = year(&[spent("a", "/src/x", &[("2025-06-01T10", "m", 3.0)])], &[]);
        assert_eq!(w.sessions, 0);
        assert_eq!(w.since, None);
    }
}
