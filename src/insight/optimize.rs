//! `cctop optimize` — what a session spent and did not get back.
//!
//! Every figure on the table is a measurement. This is the one place cctop
//! offers a *judgement*, which is a different kind of claim and has to be made
//! carefully:
//!
//! - A saving is labelled **measured** only when the transcript recorded the
//!   tokens involved. Everything else says **estimated** and means it.
//! - Counts derived from the tool history are floors, because that history is
//!   capped per tool. A finding never claims a total it cannot see.
//! - Nothing here scolds. A finding that reads as a telling-off will be
//!   dismissed on tone by somebody it was right about, and the `note` class
//!   exists so a thing worth knowing can be said without implying it is wrong.
//!
//! It writes nothing. Applying fixes — and grading them against later usage —
//! is a separate feature and a much larger commitment than reading.

use super::{Analysis, Task, plural, substantive};
use std::collections::HashMap;

/// How actionable a finding is.
///
/// The split matters more than the wording: a `Fix` is a thing to go and do, a
/// `Habit` is only ever the user's to change, and a `Note` is not a criticism
/// at all. Ranking them together without the distinction produces a list where
/// the top item cannot be acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Fix,
    Habit,
    Note,
}

impl Class {
    fn as_str(&self) -> &'static str {
        match self {
            Class::Fix => "fix",
            Class::Habit => "habit",
            Class::Note => "note",
        }
    }
}

/// Whether a saving was counted or modelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// The transcript recorded the tokens this cost.
    Measured,
    /// Derived from the session's own averages.
    Estimated,
}

impl Basis {
    fn as_str(&self) -> &'static str {
        match self {
            Basis::Measured => "measured",
            Basis::Estimated => "estimated",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub class: Class,
    pub title: String,
    /// What to do about it, in the user's own terms. Empty for a `Note`, which
    /// by definition is not asking for anything.
    pub remedy: String,
    pub tokens: u64,
    pub usd: f64,
    pub basis: Basis,
    /// Sessions this was seen in, for the detail line.
    pub sessions: usize,
    /// True where the underlying counts were capped and the real figure is
    /// larger.
    pub floor: bool,
}

/// Dollars per token, taken from what these sessions actually paid.
///
/// Rather than a published rate: the point of a dollar figure here is to say
/// what *this* usage would have saved, and a session on a bundled plan or a
/// free model should not be priced as though it were retail. Returns `None`
/// when nothing in view reported both tokens and a cost, which is the honest
/// answer for a corpus cctop cannot price.
fn usd_per_token(analyses: &[&Analysis]) -> Option<f64> {
    let (cost, tokens) = analyses
        .iter()
        .filter(|a| a.cost_available)
        .fold((0.0, 0u64), |(c, t), a| (c + a.cost, t + a.input_total));
    (tokens > 0 && cost > 0.0).then(|| cost / tokens as f64)
}

/// Reads into generated or vendored directories.
fn junk_reads(analyses: &[&Analysis], rate: Option<f64>) -> Option<Finding> {
    let hit: Vec<&&Analysis> = analyses.iter().filter(|a| a.junk_reads > 0).collect();
    let calls: u64 = hit.iter().map(|a| a.junk_reads).sum();
    if calls == 0 {
        return None;
    }
    let tokens: u64 = hit.iter().map(|a| a.junk_tokens).sum();
    Some(Finding {
        class: Class::Fix,
        title: format!(
            "{} into generated or vendored directories",
            plural(calls as usize, "read")
        ),
        remedy: "Name them in .claude/settings.json under permissions.deny, or \
                 in the ignore file your harness reads, so the agent stops \
                 being offered them."
            .into(),
        tokens,
        usd: rate.map(|r| tokens as f64 * r).unwrap_or(0.0),
        // The tokens are what the window actually grew by; the dollars are
        // those tokens at this corpus's own average rate.
        basis: Basis::Measured,
        sessions: hit.len(),
        floor: hit.iter().any(|a| a.truncated),
    })
}

/// The same file read twice inside one session.
fn rereads(analyses: &[&Analysis], rate: Option<f64>) -> Option<Finding> {
    let hit: Vec<&&Analysis> = analyses.iter().filter(|a| a.rereads > 0).collect();
    let calls: u64 = hit.iter().map(|a| a.rereads).sum();
    if calls < 3 {
        return None;
    }
    let tokens: u64 = hit.iter().map(|a| a.reread_tokens).sum();
    Some(Finding {
        class: Class::Habit,
        title: format!(
            "{} re-read inside a session that had already read them",
            plural(calls as usize, "file")
        ),
        remedy: "Usually a context window that lost the file to a compaction. \
                 Putting the file's role in CLAUDE.md, or splitting the work \
                 into shorter sessions, costs less than re-reading it."
            .into(),
        tokens,
        usd: rate.map(|r| tokens as f64 * r).unwrap_or(0.0),
        basis: Basis::Measured,
        sessions: hit.len(),
        floor: hit.iter().any(|a| a.truncated),
    })
}

/// One file read from scratch by many separate sessions.
///
/// The distinction that makes this worth reporting: a path read once in each of
/// six sessions is a piece of context the agent needs every time and is told
/// nowhere, which is a note in CLAUDE.md. The same number of reads spread over
/// six different files is just work.
fn shared_rereads(analyses: &[&Analysis]) -> Option<Finding> {
    let mut across: HashMap<&str, usize> = HashMap::new();
    for a in analyses {
        for path in &a.read_paths {
            *across.entry(path.as_str()).or_default() += 1;
        }
    }
    // Five separate sessions is the point where it stops looking like a file
    // two related pieces of work happened to share.
    let mut repeated: Vec<(&str, usize)> = across.into_iter().filter(|(_, n)| *n >= 5).collect();
    if repeated.is_empty() {
        return None;
    }
    repeated.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let worst = repeated
        .iter()
        .take(3)
        .map(|(p, n)| format!("{} ({n}×)", short_path(p)))
        .collect::<Vec<_>>()
        .join(", ");
    Some(Finding {
        class: Class::Habit,
        title: format!(
            "{} read from scratch by five or more sessions",
            plural(repeated.len(), "file")
        ),
        remedy: format!(
            "Most often {worst}. A file every session has to go and find is one              the agent is not being told about — a line in CLAUDE.md saying what              it is for costs less than reading it each time."
        ),
        tokens: 0,
        usd: 0.0,
        basis: Basis::Estimated,
        sessions: analyses.len(),
        floor: analyses.iter().any(|a| a.truncated),
    })
}

/// The tail of a path, which is what identifies a file to a person.
fn short_path(p: &str) -> String {
    let cleaned = p.replace('\\', "/");
    let parts: Vec<&str> = cleaned.rsplit('/').take(2).collect();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}

/// Sessions that read far more than they wrote.
///
/// Exploration is supposed to look like this, so it is excluded — the finding
/// is about sessions that set out to change something and spent their budget
/// looking for it.
fn read_heavy(analyses: &[&Analysis]) -> Option<Finding> {
    let hit: Vec<&&Analysis> = analyses
        .iter()
        .filter(|a| {
            !matches!(a.task, Task::Exploration | Task::Conversation)
                && a.wrote() > 0
                && a.reads >= a.wrote() * 10
        })
        .collect();
    if hit.is_empty() {
        return None;
    }
    let cost: f64 = hit
        .iter()
        .filter(|a| a.cost_available)
        .map(|a| a.cost)
        .sum();
    Some(Finding {
        class: Class::Habit,
        title: format!(
            "{} read ten times more than edited",
            plural(hit.len(), "session")
        ),
        remedy: "The agent is hunting for context it could have been given. A \
                 pointer in CLAUDE.md to where the relevant code lives is the \
                 usual fix."
            .into(),
        tokens: 0,
        // A share of the session, not the session: the reading was not all
        // wasted, so claiming the whole cost would be a fabrication.
        usd: cost * 0.25,
        basis: Basis::Estimated,
        sessions: hit.len(),
        floor: hit.iter().any(|a| a.truncated),
    })
}

/// Tool calls the transcript reported as failed.
fn failing_calls(analyses: &[&Analysis]) -> Option<Finding> {
    let hit: Vec<&&Analysis> = analyses
        .iter()
        .filter(|a| a.records_outcomes && a.calls >= 20 && a.errors * 10 >= a.calls)
        .collect();
    if hit.is_empty() {
        return None;
    }
    let errors: u64 = hit.iter().map(|a| a.errors).sum();
    let calls: u64 = hit.iter().map(|a| a.calls).sum();
    let cost: f64 = hit
        .iter()
        .filter(|a| a.cost_available)
        .map(|a| a.cost)
        .sum();
    Some(Finding {
        class: Class::Habit,
        title: format!(
            "{}% of tool calls failed across {}",
            errors * 100 / calls.max(1),
            plural(hit.len(), "session")
        ),
        remedy: "A retried call is billed every time. The usual causes are a \
                 command the agent cannot run, a path that does not exist, and \
                 a permission it was never granted — `cctop doctor` covers the \
                 last one."
            .into(),
        tokens: 0,
        // Every failed call was paid for, so its share of the session is the
        // part that bought nothing.
        usd: match calls {
            0 => 0.0,
            _ => cost * errors as f64 / calls as f64,
        },
        basis: Basis::Estimated,
        sessions: hit.len(),
        floor: hit.iter().any(|a| a.truncated),
    })
}

/// Sessions that cost something and changed no file.
///
/// Deliberately a `Note`. A session that edited nothing may have been asked a
/// question, and answering it was the point — cctop cannot tell that apart from
/// a session that went nowhere, so it says what it saw rather than what it
/// suspects.
fn spent_without_editing(analyses: &[&Analysis]) -> Option<Finding> {
    let hit: Vec<&&Analysis> = analyses
        .iter()
        .filter(|a| {
            a.cost_available
                && a.cost >= 0.50
                && a.wrote() == 0
                && !matches!(
                    a.task,
                    Task::Conversation | Task::Planning | Task::Exploration
                )
        })
        .collect();
    if hit.is_empty() {
        return None;
    }
    let cost: f64 = hit.iter().map(|a| a.cost).sum();
    // Named, because "two sessions somewhere" is not something anyone can look
    // into, and the project is the part that jogs a memory of what it was.
    let mut where_: Vec<&str> = hit
        .iter()
        .map(|a| a.label.as_str())
        .filter(|l| !l.is_empty())
        .collect();
    where_.sort_unstable();
    where_.dedup();
    let where_ = match where_.is_empty() {
        true => String::new(),
        false => format!(
            "Mostly in {}. ",
            where_
                .iter()
                .take(3)
                .copied()
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    Some(Finding {
        class: Class::Note,
        title: format!(
            "{} spent over $0.50 and edited no file",
            plural(hit.len(), "session")
        ),
        remedy: format!(
            "{where_}Not necessarily wasted — a question answered well changes              no file. Worth a look only if you expected these to ship something."
        ),
        tokens: 0,
        usd: cost,
        basis: Basis::Measured,
        sessions: hit.len(),
        floor: false,
    })
}

/// Every finding, worst first.
pub fn findings(analyses: &[&Analysis]) -> Vec<Finding> {
    let live: Vec<&Analysis> = analyses
        .iter()
        .copied()
        .filter(|a| substantive(a))
        .collect();
    let rate = usd_per_token(&live);
    let mut out: Vec<Finding> = [
        junk_reads(&live, rate),
        rereads(&live, rate),
        shared_rereads(&live),
        read_heavy(&live),
        failing_calls(&live),
        spent_without_editing(&live),
    ]
    .into_iter()
    .flatten()
    .collect();
    // Class first, then cost. Ranking on cost alone put a `note` at the top,
    // and a note names money that was *spent*, not money that could be saved —
    // so the largest number in the list belonged to the one row nobody could
    // act on. Sorting by what a reader can do about it is the honest order.
    out.sort_by(|a, b| {
        a.class.cmp(&b.class).then(
            b.usd
                .partial_cmp(&a.usd)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    out
}

/// Where the session budget went, by kind of work.
pub fn by_task(analyses: &[&Analysis]) -> Vec<(Task, usize, f64)> {
    let mut acc: HashMap<Task, (usize, f64)> = HashMap::new();
    for a in analyses {
        let e = acc.entry(a.task).or_default();
        e.0 += 1;
        if a.cost_available {
            e.1 += a.cost;
        }
    }
    let mut out: Vec<(Task, usize, f64)> = Task::ALL
        .iter()
        .filter_map(|t| acc.get(t).map(|(n, c)| (*t, *n, *c)))
        .collect();
    out.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    out
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
    if live.is_empty() {
        return "No sessions with recorded tool calls, so there is nothing to read.\n".into();
    }

    let found = findings(analyses);
    // Deliberately only the actionable classes. A `note` carries what a set of
    // sessions cost, which is an observation and not a saving; adding it here
    // would advertise a number nobody could ever recover.
    let recoverable: f64 = found
        .iter()
        .filter(|f| f.class != Class::Note)
        .map(|f| f.usd)
        .sum();
    out.push('\n');
    let _ = writeln!(
        out,
        "  {} sessions  ·  {}  ·  about {} looks recoverable",
        live.len(),
        plural(found.len(), "finding"),
        crate::util::adaptive_usd(recoverable)
    );
    out.push('\n');

    if found.is_empty() {
        let _ = writeln!(
            out,
            "  Nothing worth reporting. That is a real answer, not an empty one."
        );
    }
    for f in &found {
        let amount = match f.usd > 0.0 {
            true => crate::util::adaptive_usd(f.usd),
            false => "—".to_string(),
        };
        let _ = writeln!(
            out,
            "  {:<6} {:<52} {:>9}  {}",
            f.class.as_str(),
            ellipsise(&f.title, 52),
            amount,
            // A note's figure is what was spent, not what could be saved, so it
            // must not read as a saving that was measured.
            match f.class {
                Class::Note => "observed",
                _ => f.basis.as_str(),
            }
        );
        if !f.remedy.is_empty() {
            for line in textwrap(&f.remedy, 66) {
                let _ = writeln!(out, "         {line}");
            }
        }
        if f.floor {
            let _ = writeln!(
                out,
                "         (a floor: the tool history is capped per session)"
            );
        }
        out.push('\n');
    }

    let _ = writeln!(out, "  Where the money went");
    out.push('\n');
    for (task, n, cost) in by_task(&live) {
        let _ = writeln!(
            out,
            "  {:<14} {:>4} sessions  {:>9}",
            task.as_str(),
            n,
            crate::util::adaptive_usd(cost)
        );
    }
    out.push('\n');
    let _ = writeln!(
        out,
        "  Costs are estimates — see `cctop --help` and docs/costs.md."
    );
    let _ = writeln!(
        out,
        "  A `measured` saving was counted from recorded tokens; an"
    );
    let _ = writeln!(
        out,
        "  `estimated` one was derived from this corpus's own averages."
    );
    out.push('\n');
    out
}

/// Wrap to `width`, breaking on spaces only.
fn textwrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// The findings as JSON, for scripting.
pub fn as_json(analyses: &[&Analysis]) -> String {
    let live: Vec<&Analysis> = analyses
        .iter()
        .copied()
        .filter(|a| substantive(a))
        .collect();
    let found = findings(analyses);
    let doc = serde_json::json!({
        "sessions": live.len(),
        "findings": found.iter().map(|f| serde_json::json!({
            "class": f.class.as_str(),
            "title": f.title,
            "remedy": f.remedy,
            "tokens": f.tokens,
            "usd": f.usd,
            "basis": f.basis.as_str(),
            "sessions": f.sessions,
            "floor": f.floor,
        })).collect::<Vec<_>>(),
        "by_task": by_task(&live).into_iter().map(|(t, n, c)| serde_json::json!({
            "task": t.as_str(),
            "sessions": n,
            "usd": c,
        })).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}

/// Cut to `width`, marking that something was cut.
fn ellipsise(s: &str, width: usize) -> String {
    match s.chars().count() > width {
        false => s.to_string(),
        true => s.chars().take(width - 1).collect::<String>() + "\u{2026}",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insight::Task;
    use crate::pricing::Provider;

    fn session(cost: f64, edits: u64, task: Task) -> Analysis {
        Analysis {
            provider: Provider::Claude,
            label: "repo".into(),
            model: "claude-opus-5".into(),
            cost,
            cost_available: true,
            task,
            calls: 40,
            errors: 0,
            records_outcomes: true,
            edits,
            bash_writes: 0,
            reads: 0,
            files_edited: edits,
            files_one_shot: edits,
            read_paths: Default::default(),
            junk_reads: 0,
            junk_tokens: 0,
            reread_tokens: 0,
            rereads: 0,
            cache_read: 0,
            input_total: 0,
            truncated: false,
        }
    }

    /// A note records what a set of sessions *spent*. Counting it as a saving
    /// put $216 of ordinary work at the top of a list of things to fix, and
    /// made the headline advertise money nobody could ever recover.
    #[test]
    fn an_observation_is_not_a_saving() {
        let sessions = [
            session(20.0, 0, Task::Coding),
            session(20.0, 0, Task::Coding),
        ];
        let refs: Vec<&Analysis> = sessions.iter().collect();
        let found = findings(&refs);

        let note = found
            .iter()
            .find(|f| f.class == Class::Note)
            .expect("sessions that spent and edited nothing");
        assert!(note.usd > 0.0, "the note still reports what was spent");

        let recoverable: f64 = found
            .iter()
            .filter(|f| f.class != Class::Note)
            .map(|f| f.usd)
            .sum();
        assert_eq!(recoverable, 0.0, "and none of it counts as recoverable");
    }

    /// Findings are ordered by what a reader can do about them. Ranking on cost
    /// alone put the one unactionable row first.
    #[test]
    fn actionable_findings_outrank_observations() {
        let mut sessions = [
            session(50.0, 0, Task::Coding),
            session(50.0, 0, Task::Coding),
        ];
        sessions[0].junk_reads = 4;
        sessions[0].junk_tokens = 8000;
        sessions[0].input_total = 100_000;
        let refs: Vec<&Analysis> = sessions.iter().collect();
        let found = findings(&refs);

        let classes: Vec<Class> = found.iter().map(|f| f.class).collect();
        let note_at = classes.iter().position(|c| *c == Class::Note);
        let fix_at = classes.iter().position(|c| *c == Class::Fix);
        assert!(fix_at.is_some() && note_at.is_some());
        assert!(fix_at < note_at, "a fix comes before an observation");
    }

    /// Exploration is supposed to read without writing, so it must not be
    /// reported as a session that read too much.
    #[test]
    fn exploration_is_not_reported_as_reading_too_much() {
        let mut explore = session(5.0, 0, Task::Exploration);
        explore.reads = 500;
        explore.edits = 1;
        explore.files_edited = 1;
        let refs = vec![&explore];
        assert!(
            !findings(&refs)
                .iter()
                .any(|f| f.title.contains("ten times")),
            "exploring is what exploration is for"
        );
    }
}
