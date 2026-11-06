//! Spotting an agent that is going round in circles instead of getting
//! anywhere.
//!
//! A monitor already says when an agent stopped and wants you. The other way an
//! agent wastes an afternoon is by never stopping: the same failing command,
//! run again and again; money going out with nothing written; one edit made
//! and unmade. None of those ever crosses into "waiting", so nothing rang for
//! them. This is the check that does.
//!
//! # How it decides
//!
//! The evidence is worked out once per extraction, in
//! [`SessionData::finalize`](crate::session::SessionData::finalize), from the
//! tool history that is about to be trimmed away — the same place
//! `recent_writes` is distilled, for the same reason: the history is not
//! persisted, the few facts drawn from it are. Thresholds are applied later,
//! by [`Session::looping`](crate::session::Session::looping), against the row
//! as it is *now* — whether the agent is working, how much it spent in the
//! last few minutes — and the settings as they are now, so the table, the
//! bell, Web Push and MCP all read one verdict.
//!
//! Three patterns, each reset by anything that looks like a different outcome:
//!
//! - **The same failing call.** The newest run of identical calls — same tool,
//!   same arguments with whitespace normalised — that all failed, counted back
//!   from the end of the session. Reads, searches and listings in between are
//!   looked past: an agent reading a file between retries is still retrying.
//!   An edit, a call that succeeded, or a *different* failing call ends the run.
//!   A test loop that edits between runs is therefore never counted, which is
//!   the false positive this most needs to avoid: that is an agent working.
//! - **Spending without progress.** At least `stuck_burn`'s dollars spent in its
//!   window of minutes while working, with no edit and no passing test run in
//!   that window. "Passing test" is a shell call whose command names a test
//!   runner and that did not fail — cheap, and wrong only in the safe direction
//!   (a test command that is not one counts as progress, never the reverse).
//! - **An edit made and unmade.** The same file edited back and forth: an edit
//!   whose added lines are exactly an earlier edit's removed lines, and the
//!   other way about. Two such reverts on one file among its recent edits.
//!
//! All three need the session to be working — an agent at its prompt is not
//! looping, however its last turn went.
//!
//! ponytail: the error *text* is not compared. Results are not kept past the
//! tool panel's own parse, so "the same call failed again" stands in for "the
//! same error again" — a flaky command that fails differently each time still
//! counts. Read-only loops (an agent that only ever reads) are not caught at
//! all: telling exploration from being lost is not something a heuristic
//! does well, and a false "looping" on a careful reader is worse than a
//! miss. Remote rows carry none of this; their machine would have to
//! compute it.

use crate::session::{EDIT_TOOLS, ToolDetail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What the tool history says, before any threshold is applied.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    /// The newest run of the same call failing, if it failed at least twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<Repeat>,
    /// The file most often edited back and forth, if any was reverted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle: Option<Cycle>,
    /// When the session last did something that counts as progress: an edit,
    /// or a test run that passed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_progress: Option<String>,
}

/// One call, failing again and again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Repeat {
    pub tool: String,
    /// The call as the tool panel shows it.
    pub call: String,
    pub count: u32,
    /// When the run started and when it last failed.
    pub first: String,
    pub last: String,
}

/// One file, edited and un-edited.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cycle {
    pub file: String,
    pub reverts: u32,
    pub last: String,
}

/// The verdict: why the session looks stuck, and what shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Looping {
    /// One line for the table and the notification.
    pub reason: String,
    /// The calls or edits behind it, for the Info panel.
    pub evidence: Vec<String>,
}

/// The thresholds, from the settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    pub enabled: bool,
    /// Identical failing calls in a row before it counts.
    pub repeat: u32,
    /// Dollars spent within `burn_minutes` with no progress.
    pub burn_dollars: f64,
    pub burn_minutes: i64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            repeat: 3,
            burn_dollars: 2.0,
            burn_minutes: 10,
        }
    }
}

/// Reverts of one file before it counts.
const CYCLES: u32 = 2;

/// How many of a file's newest edits are compared for reverts.
const CYCLE_WINDOW: usize = 20;

/// Parse `stuck_burn` — `"$2 / 10m"`, `"2/10"`, `"$1.50/15m"` — into dollars
/// and minutes. `None` for anything else.
pub fn parse_burn(text: &str) -> Option<(f64, i64)> {
    let (money, time) = text.split_once('/')?;
    let dollars: f64 = money.trim().trim_start_matches('$').trim().parse().ok()?;
    let time = time.trim();
    let digits = time.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == ' ');
    let unit = time[digits.len()..].trim();
    if !matches!(unit, "" | "m" | "min" | "mins" | "minutes") {
        return None;
    }
    let minutes: i64 = digits.trim().parse().ok()?;
    (dollars > 0.0 && minutes > 0).then_some((dollars, minutes))
}

/// Tools that only look: walked past, never counted, never a reset.
fn read_only(tool: &str) -> bool {
    let tool = tool.to_ascii_lowercase();
    [
        "read", "grep", "glob", "search", "list", "ls", "fetch", "view", "todo", "task", "plan",
        "find", "cat",
    ]
    .iter()
    .any(|word| tool.contains(word))
}

fn is_edit(tool: &str) -> bool {
    EDIT_TOOLS.contains(&tool) || tool.contains("patch")
}

fn is_shell(tool: &str) -> bool {
    matches!(
        tool.to_ascii_lowercase().as_str(),
        "bash"
            | "shell"
            | "exec_command"
            | "local_shell"
            | "run_terminal_cmd"
            | "run_shell_command"
    )
}

/// Whether a shell command runs tests.
fn runs_tests(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    [
        "cargo test",
        "cargo nextest",
        "npm test",
        "npm run test",
        "pnpm test",
        "yarn test",
        "pytest",
        "go test",
        "jest",
        "vitest",
        "rspec",
        "mvn test",
        "gradle test",
        "make test",
    ]
    .iter()
    .any(|runner| command.contains(runner))
}

/// The call's arguments, the same whatever their spacing.
fn normalised(detail: &ToolDetail) -> String {
    detail
        .full
        .as_deref()
        .unwrap_or(&detail.d)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Work the evidence out of a session's tool history.
pub fn evidence(details: &HashMap<String, Vec<ToolDetail>>) -> Evidence {
    // The main session only: a subagent's calls are interleaved into the same
    // log, and its retries are its own business.
    let mut calls: Vec<(&str, &ToolDetail)> = details
        .iter()
        .flat_map(|(tool, list)| list.iter().map(move |d| (tool.as_str(), d)))
        .filter(|(_, d)| d.origin.is_none())
        .collect();
    calls.sort_by(|a, b| a.1.ts.cmp(&b.1.ts));
    Evidence {
        repeat: repeat(&calls),
        cycle: cycle(&calls),
        last_progress: calls
            .iter()
            .rev()
            .find(|(tool, d)| {
                is_edit(tool) || (is_shell(tool) && !d.failed && runs_tests(&normalised(d)))
            })
            .map(|(_, d)| d.ts.clone()),
    }
}

/// The newest run of one failing call, counted back from the end.
fn repeat(calls: &[(&str, &ToolDetail)]) -> Option<Repeat> {
    let mut run: Option<(String, Repeat)> = None;
    for (tool, detail) in calls.iter().rev() {
        if read_only(tool) {
            continue;
        }
        if is_edit(tool) || !detail.failed {
            break;
        }
        let key = format!("{tool}\u{1f}{}", normalised(detail));
        match &mut run {
            None => {
                run = Some((
                    key,
                    Repeat {
                        tool: tool.to_string(),
                        call: detail.d.clone(),
                        count: 1,
                        first: detail.ts.clone(),
                        last: detail.ts.clone(),
                    },
                ));
            }
            Some((same, found)) if *same == key => {
                found.count += 1;
                found.first = detail.ts.clone();
            }
            Some(_) => break,
        }
    }
    run.map(|(_, found)| found).filter(|r| r.count >= 2)
}

/// The lines an edit took out and put in, from its diff.
fn sides(detail: &ToolDetail) -> Option<(Vec<&str>, Vec<&str>)> {
    let hunks = &detail.delta.as_ref()?.hunks;
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for line in hunks {
        if line.starts_with("---") || line.starts_with("+++") {
            continue;
        }
        if let Some(text) = line.strip_prefix('-') {
            removed.push(text);
        } else if let Some(text) = line.strip_prefix('+') {
            added.push(text);
        }
    }
    (!removed.is_empty() || !added.is_empty()).then_some((removed, added))
}

/// The file reverted most among its recent edits, if any was reverted at all.
fn cycle(calls: &[(&str, &ToolDetail)]) -> Option<Cycle> {
    let mut by_file: HashMap<&str, Vec<&ToolDetail>> = HashMap::new();
    for (tool, detail) in calls {
        if !is_edit(tool) {
            continue;
        }
        let file = detail
            .paths
            .first()
            .map(String::as_str)
            .unwrap_or(detail.d.as_str())
            .trim();
        if !file.is_empty() {
            by_file.entry(file).or_default().push(detail);
        }
    }
    let mut best: Option<Cycle> = None;
    for (file, edits) in by_file {
        let recent = &edits[edits.len().saturating_sub(CYCLE_WINDOW)..];
        let mut reverts = 0;
        let mut last = String::new();
        for (i, later) in recent.iter().enumerate() {
            let Some((removed, added)) = sides(later) else {
                continue;
            };
            let undoes = recent[..i]
                .iter()
                .filter_map(|e| sides(e))
                .any(|(r, a)| r == added && a == removed);
            if undoes {
                reverts += 1;
                last = later.ts.clone();
            }
        }
        if reverts > 0 && best.as_ref().is_none_or(|b| reverts > b.reverts) {
            best = Some(Cycle {
                file: file.to_string(),
                reverts,
                last,
            });
        }
    }
    best
}

/// Apply the thresholds: whether a working session with this evidence, and
/// these dollars per minute of late, looks stuck as of `now_minute`.
///
/// `minutes` is the session's recent spend by unix minute.
pub fn verdict(
    evidence: &Evidence,
    minutes: &[(i64, f64)],
    working: bool,
    now_minute: i64,
    config: &Config,
) -> Option<Looping> {
    if !config.enabled || !working {
        return None;
    }
    if let Some(r) = evidence
        .repeat
        .as_ref()
        .filter(|r| r.count >= config.repeat)
    {
        return Some(Looping {
            reason: format!("same failing {} ×{}", short(&r.call, 40), r.count),
            evidence: vec![
                format!("{} failed {} times in a row", r.tool, r.count),
                format!("call: {}", short(&r.call, 200)),
                format!("from {} to {}", r.first, r.last),
            ],
        });
    }
    if let Some(c) = evidence.cycle.as_ref().filter(|c| c.reverts >= CYCLES) {
        return Some(Looping {
            reason: format!(
                "{} edited back and forth ×{}",
                short(&c.file, 40),
                c.reverts
            ),
            evidence: vec![
                format!("{} reverts of {}", c.reverts, c.file),
                format!("last at {}", c.last),
            ],
        });
    }
    let from = now_minute - config.burn_minutes + 1;
    let spent: f64 = minutes
        .iter()
        .filter(|(m, _)| *m >= from)
        .map(|(_, c)| c)
        .sum();
    let progressed = evidence
        .last_progress
        .as_deref()
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .is_some_and(|ts| ts.timestamp() / 60 >= from);
    if spent >= config.burn_dollars && !progressed {
        return Some(Looping {
            reason: format!("${spent:.2} in {} min, no edits", config.burn_minutes),
            evidence: vec![
                format!(
                    "${spent:.2} spent in the last {} minutes",
                    config.burn_minutes
                ),
                match &evidence.last_progress {
                    Some(ts) => format!("last edit or passing test at {ts}"),
                    None => "no edit or passing test yet".to_string(),
                },
            ],
        });
    }
    None
}

fn short(text: &str, cells: usize) -> String {
    let line = text.lines().next().unwrap_or_default();
    crate::util::clip(line, cells)
}

/// The settings' thresholds, read at most every few seconds.
///
/// Asked for every running row on every refresh, which is far too often to
/// read `config.toml` each time; a stale threshold for a few seconds after
/// the file changed costs nothing.
pub fn config() -> Config {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<(Instant, Config)>> = Mutex::new(None);
    const FRESH: Duration = Duration::from_secs(5);
    if crate::under_test() {
        return Config::default();
    }
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, config)) = *cache
        && at.elapsed() < FRESH
    {
        return config;
    }
    let config = crate::settings::Settings::load().stuck();
    *cache = Some((Instant::now(), config));
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Delta;

    fn call(ts: &str, d: &str, failed: bool) -> ToolDetail {
        ToolDetail {
            d: d.into(),
            ts: ts.into(),
            failed,
            ..Default::default()
        }
    }

    fn edit(ts: &str, file: &str, removed: &[&str], added: &[&str]) -> ToolDetail {
        let mut hunks: Vec<String> = removed.iter().map(|l| format!("-{l}")).collect();
        hunks.extend(added.iter().map(|l| format!("+{l}")));
        ToolDetail {
            d: file.into(),
            ts: ts.into(),
            delta: Some(Delta {
                added: added.len() as u32,
                removed: removed.len() as u32,
                hunks,
            }),
            ..Default::default()
        }
    }

    fn history(entries: Vec<(&str, ToolDetail)>) -> HashMap<String, Vec<ToolDetail>> {
        let mut out: HashMap<String, Vec<ToolDetail>> = HashMap::new();
        for (tool, d) in entries {
            out.entry(tool.into()).or_default().push(d);
        }
        out
    }

    fn ts(minute: u32) -> String {
        format!("2026-11-06T10:{minute:02}:00Z")
    }

    /// 2026-11-06T10:30Z, in unix minutes.
    const NOW: i64 = 29_899_350;

    fn stuck(e: &Evidence) -> Option<Looping> {
        verdict(e, &[], true, NOW, &Config::default())
    }

    /// Claude's Bash failing the same way four times, with reads between,
    /// is a loop; the reads do not break it.
    #[test]
    fn the_same_failing_command_is_a_loop() {
        let e = evidence(&history(vec![
            ("Bash", call(&ts(1), "cargo test", true)),
            ("Read", call(&ts(2), "src/lib.rs", false)),
            ("Bash", call(&ts(3), "cargo  test", true)),
            ("Grep", call(&ts(4), "fn main", false)),
            ("Bash", call(&ts(5), "cargo test", true)),
            ("Bash", call(&ts(6), "cargo test", true)),
        ]));
        assert_eq!(e.repeat.as_ref().map(|r| r.count), Some(4));
        let looping = stuck(&e).expect("looping");
        assert!(
            looping.reason.contains("cargo test ×4"),
            "{}",
            looping.reason
        );
    }

    /// Codex's shell tool counts the same way.
    #[test]
    fn a_codex_shell_loop_is_a_loop_too() {
        let e = evidence(&history(vec![
            ("exec_command", call(&ts(1), "npm run build", true)),
            ("exec_command", call(&ts(2), "npm run build", true)),
            ("exec_command", call(&ts(3), "npm run build", true)),
        ]));
        assert!(stuck(&e).is_some());
    }

    /// Editing between test runs is working on it, not looping — the false
    /// positive this must never raise.
    #[test]
    fn a_test_fix_loop_that_keeps_editing_is_not_a_loop() {
        let e = evidence(&history(vec![
            ("Bash", call(&ts(1), "cargo test", true)),
            ("Edit", edit(&ts(2), "src/a.rs", &["x"], &["y"])),
            ("Bash", call(&ts(3), "cargo test", true)),
            ("Edit", edit(&ts(4), "src/a.rs", &["y"], &["z"])),
            ("Bash", call(&ts(5), "cargo test", true)),
        ]));
        assert_eq!(e.repeat, None, "the last edit ends the run at one");
        assert_eq!(stuck(&e), None);
    }

    /// A single failure, a different failing command, a success: none of
    /// them is a run.
    #[test]
    fn a_different_outcome_resets_the_run() {
        let one = evidence(&history(vec![("Bash", call(&ts(1), "make", true))]));
        assert_eq!(one.repeat, None);
        let different = evidence(&history(vec![
            ("Bash", call(&ts(1), "make", true)),
            ("Bash", call(&ts(2), "make", true)),
            ("Bash", call(&ts(3), "make check", true)),
        ]));
        assert_eq!(
            different.repeat.map(|r| r.count),
            None,
            "one of the last kind"
        );
        let passed = evidence(&history(vec![
            ("Bash", call(&ts(1), "make", true)),
            ("Bash", call(&ts(2), "make", true)),
            ("Bash", call(&ts(3), "make", true)),
            ("Bash", call(&ts(4), "make", false)),
        ]));
        assert_eq!(passed.repeat, None);
    }

    /// Hours of reading is not looping.
    #[test]
    fn long_reading_is_not_a_loop() {
        let reads: Vec<_> = (0..50)
            .map(|i| ("Read", call(&ts(i % 60), &format!("f{i}.rs"), false)))
            .collect();
        assert_eq!(stuck(&evidence(&history(reads))), None);
    }

    /// One edit made and unmade twice on one file is a loop.
    #[test]
    fn an_edit_made_and_unmade_is_a_loop() {
        let e = evidence(&history(vec![
            ("Edit", edit(&ts(1), "src/a.rs", &["old"], &["new"])),
            ("Edit", edit(&ts(2), "src/a.rs", &["new"], &["old"])),
            ("Edit", edit(&ts(3), "src/a.rs", &["old"], &["new"])),
        ]));
        assert_eq!(e.cycle.as_ref().map(|c| c.reverts), Some(2));
        let looping = stuck(&e).expect("looping");
        assert!(
            looping.reason.contains("back and forth"),
            "{}",
            looping.reason
        );
    }

    /// Money out with no edit and no passing test in the window is a loop; an
    /// edit inside the window is progress.
    #[test]
    fn spending_with_nothing_written_is_a_loop() {
        let minutes: Vec<(i64, f64)> = (0..10).map(|i| (NOW - i, 0.3)).collect();
        let none = Evidence::default();
        let looping = verdict(&none, &minutes, true, NOW, &Config::default()).expect("burn");
        assert!(
            looping.reason.starts_with("$3.00 in 10 min"),
            "{}",
            looping.reason
        );

        let edited = Evidence {
            last_progress: Some(ts(28)),
            ..Default::default()
        };
        assert_eq!(
            verdict(&edited, &minutes, true, NOW, &Config::default()),
            None
        );

        let cheap: Vec<(i64, f64)> = (0..10).map(|i| (NOW - i, 0.05)).collect();
        assert_eq!(verdict(&none, &cheap, true, NOW, &Config::default()), None);
    }

    /// An agent at its prompt is not looping, and the switch turns it off.
    #[test]
    fn only_a_working_session_with_detection_on_is_looping() {
        let e = Evidence {
            repeat: Some(Repeat {
                tool: "Bash".into(),
                call: "make".into(),
                count: 5,
                first: ts(1),
                last: ts(5),
            }),
            ..Default::default()
        };
        assert!(verdict(&e, &[], true, NOW, &Config::default()).is_some());
        assert!(verdict(&e, &[], false, NOW, &Config::default()).is_none());
        let off = Config {
            enabled: false,
            ..Config::default()
        };
        assert!(verdict(&e, &[], true, NOW, &off).is_none());
    }

    #[test]
    fn the_burn_setting_reads_as_written() {
        assert_eq!(parse_burn("$2 / 10m"), Some((2.0, 10)));
        assert_eq!(parse_burn("1.5/15"), Some((1.5, 15)));
        assert_eq!(parse_burn("$1 / 5 min"), Some((1.0, 5)));
        assert_eq!(parse_burn("nonsense"), None);
        assert_eq!(parse_burn("$0 / 10m"), None);
    }

    /// A passing test run is progress; a failing one is not.
    #[test]
    fn a_passing_test_run_is_progress() {
        let e = evidence(&history(vec![
            ("Bash", call(&ts(1), "cargo test -p x", false)),
            ("Bash", call(&ts(2), "cargo test -p y", true)),
        ]));
        assert_eq!(e.last_progress.as_deref(), Some(ts(1).as_str()));
    }
}
