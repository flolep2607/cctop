//! `--trace` — where did the time go, in a file you can send someone.
//!
//! `doctor` answers "why is cctop showing the wrong thing"; this answers "why is
//! it slow", which is the question that cannot be answered from the outside. A
//! slow start has half a dozen plausible causes — a huge corpus, a cache too big
//! to load, a pricing fetch stalling on a firewall, a transcript that reparses on
//! every tick — and they are indistinguishable from a stopwatch. Each one is
//! obvious from inside.
//!
//! So the phases time themselves and the totals go to a file. It reports
//! aggregates rather than a line per event: a walk that ran forty times matters
//! as "forty times, 12s total, 900ms worst", and a log with forty entries of it
//! buries that under scrolling. The file is small enough to paste into an issue.
//!
//! It is off unless asked for, and costs one relaxed atomic load when it is —
//! [`span`] is called on paths that run per session per refresh, so "free when
//! off" has to be true rather than nearly true.
//!
//! # What it may contain
//!
//! It is written to be handed to a stranger, so it carries no session titles, no
//! project paths, no queries, and no file names from anyone's transcripts —
//! only counts, byte totals and durations. Paths that are cctop's own are
//! spelled with `~` for the same reason: a home directory names its user.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

static ON: AtomicBool = AtomicBool::new(false);

/// One phase's tally. Totals and a worst case, because the two fail differently:
/// a phase that is slow every time and one that is fast except once need
/// different fixes, and a mean hides both.
#[derive(Default)]
struct Phase {
    calls: u64,
    total: Duration,
    worst: Duration,
}

#[derive(Default)]
struct Recording {
    /// Insertion-ordered so the report reads in the order things happened
    /// rather than however a hash map felt.
    order: Vec<&'static str>,
    phases: HashMap<&'static str, Phase>,
    counters: Vec<(&'static str, u64)>,
    facts: Vec<(String, String)>,
    started: Option<Instant>,
}

static REC: LazyLock<Mutex<Recording>> = LazyLock::new(|| Mutex::new(Recording::default()));

/// Begin recording. Idempotent, and the only thing that turns [`on`] true.
pub fn enable() {
    if let Ok(mut rec) = REC.lock() {
        rec.started = Some(Instant::now());
    }
    ON.store(true, Ordering::Relaxed);
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Time a phase for as long as the returned guard lives.
///
/// `name` is a static label rather than a formatted string so that turning
/// tracing on cannot start allocating inside the loops being measured.
pub fn span(name: &'static str) -> Span {
    Span {
        name,
        started: on().then(Instant::now),
    }
}

pub struct Span {
    name: &'static str,
    /// `None` when tracing is off, which is what makes the guard free.
    started: Option<Instant>,
}

impl Drop for Span {
    fn drop(&mut self) {
        let Some(started) = self.started else { return };
        let elapsed = started.elapsed();
        if let Ok(mut rec) = REC.lock() {
            if !rec.phases.contains_key(self.name) {
                rec.order.push(self.name);
            }
            let phase = rec.phases.entry(self.name).or_default();
            phase.calls += 1;
            phase.total += elapsed;
            phase.worst = phase.worst.max(elapsed);
        }
    }
}

/// Add to a running total — sessions parsed, bytes read, cache hits taken.
///
/// Durations say how long something took; these say how much of it there was,
/// which is what separates "slow parser" from "a great deal to parse".
pub fn add(name: &'static str, n: u64) {
    if !on() {
        return;
    }
    if let Ok(mut rec) = REC.lock() {
        match rec.counters.iter_mut().find(|(k, _)| *k == name) {
            Some((_, v)) => *v += n,
            None => rec.counters.push((name, n)),
        }
    }
}

/// Record something measured once: a version, a file size, a count of homes.
///
/// Recording the same key again replaces it rather than adding a line. These
/// describe the machine, not events on it, and the walk they are gathered in
/// runs repeatedly — so appending would print "sessions found" once per refresh
/// and leave the reader to work out that they are all the same number.
pub fn fact(key: &str, value: impl Into<String>) {
    if !on() {
        return;
    }
    if let Ok(mut rec) = REC.lock() {
        let value = value.into();
        match rec.facts.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value,
            None => rec.facts.push((key.to_string(), value)),
        }
    }
}

/// Spell a path for a stranger's eyes: cctop's own files live under the user's
/// home, and the home directory names the user.
pub fn redact(path: &Path) -> String {
    let shown = path.display().to_string();
    match shown.strip_prefix(&crate::config::HOME.display().to_string()) {
        Some(rest) => format!("~{rest}"),
        None => shown,
    }
}

fn ms(d: Duration) -> String {
    match d.as_secs_f64() {
        s if s >= 1.0 => format!("{s:.2}s"),
        s => format!("{:.0}ms", s * 1000.0),
    }
}

fn bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => format!("{n} B"),
        _ => format!("{v:.1} {}", UNITS[unit]),
    }
}

/// Render the report. Separate from writing it so the shape can be tested
/// without a filesystem.
fn render() -> String {
    let Ok(rec) = REC.lock() else {
        return String::new();
    };
    let mut out = String::new();
    out.push_str(&format!(
        "cctop {} trace\n{} on {} ({} cores)\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
        std::env::consts::OS,
        std::thread::available_parallelism().map_or(0, |n| n.get()),
    ));
    if let Some(started) = rec.started {
        out.push_str(&format!("ran for {}\n", ms(started.elapsed())));
    }

    if !rec.facts.is_empty() {
        out.push_str("\n-- environment --\n");
        let width = rec.facts.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        for (key, value) in &rec.facts {
            out.push_str(&format!("{key:width$}  {value}\n"));
        }
    }

    if !rec.order.is_empty() {
        out.push_str("\n-- phases --\n");
        let width = rec.order.iter().map(|n| n.len()).max().unwrap_or(0);
        out.push_str(&format!(
            "{:width$}  {:>6}  {:>9}  {:>9}\n",
            "phase", "calls", "total", "worst"
        ));
        for name in &rec.order {
            let Some(p) = rec.phases.get(name) else {
                continue;
            };
            out.push_str(&format!(
                "{name:width$}  {:>6}  {:>9}  {:>9}\n",
                p.calls,
                ms(p.total),
                ms(p.worst)
            ));
        }
    }

    if !rec.counters.is_empty() {
        out.push_str("\n-- totals --\n");
        let width = rec.counters.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        for (key, value) in &rec.counters {
            // A counter whose name says it holds bytes is unreadable raw.
            let shown = match key.ends_with("_bytes") {
                true => bytes(*value),
                false => value.to_string(),
            };
            out.push_str(&format!("{key:width$}  {shown}\n"));
        }
    }
    out
}

/// Default destination: beside the caches, named for the process so two cctops
/// tracing at once do not write over each other.
pub fn default_path() -> PathBuf {
    config_dir().join(format!("trace-{}.txt", std::process::id()))
}

fn config_dir() -> PathBuf {
    crate::config::CACHE_DIR.clone()
}

/// Write the report to `path`, returning where it landed.
///
/// Failure is reported rather than swallowed: someone passed `--trace` and is
/// waiting for a file to send, so a silent no-op is the one unhelpful outcome.
pub fn write_to(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, render())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The recording is process-wide and the test harness is threaded, so a test
    /// that turns tracing on would otherwise make a test that asserts it is off
    /// fail at random. Same bargain as `pricing::install_test_table`.
    #[must_use = "hold the guard for as long as the test touches the recording"]
    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The guard has to cost nothing when tracing is off, because it is taken
    /// per session per refresh — the exact loops `--trace` exists to measure.
    #[test]
    fn a_span_records_nothing_while_tracing_is_off() {
        let _guard = exclusive();
        ON.store(false, Ordering::Relaxed);
        let span = span("never-recorded");
        assert!(span.started.is_none(), "an off span must not take a clock");
        add("never-counted", 5);
        fact("never-noted", "x");
    }

    #[test]
    fn durations_read_in_the_unit_that_suits_them() {
        assert_eq!(ms(Duration::from_millis(4)), "4ms");
        assert_eq!(ms(Duration::from_millis(950)), "950ms");
        assert_eq!(ms(Duration::from_millis(1500)), "1.50s");
    }

    #[test]
    fn byte_totals_read_as_sizes() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(2048), "2.0 KB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.0 MB");
    }

    /// The file is meant to be handed to a stranger, and a home directory names
    /// its user.
    #[test]
    fn a_home_path_is_spelled_without_the_user() {
        let inside = crate::config::HOME.join(".cache").join("cctop");
        let shown = redact(&inside);
        assert!(
            shown.starts_with("~/") || shown.starts_with("~\\"),
            "{shown}"
        );
        assert!(!shown.contains(&crate::config::HOME.display().to_string()));
        // A path that is not under the home is left alone rather than mangled.
        let outside = Path::new("/opt/somewhere/else");
        assert_eq!(redact(outside), "/opt/somewhere/else");
    }

    /// The environment section describes the machine, and the walk that gathers
    /// it runs once per refresh — so a long run must not accumulate one copy of
    /// every fact per walk.
    #[test]
    fn a_fact_recorded_twice_is_replaced_rather_than_repeated() {
        let _guard = exclusive();
        enable();
        fact("sessions found", "73");
        fact("sessions found", "74");
        let text = render();
        ON.store(false, Ordering::Relaxed);

        assert_eq!(text.matches("sessions found").count(), 1, "{text}");
        assert!(text.contains("74"), "the newest value wins:\n{text}");
    }

    /// Whatever else it says, the header has to identify the build and machine
    /// — a trace you cannot attribute to a version is a trace you cannot act on.
    #[test]
    fn the_report_names_the_version_and_platform() {
        let _guard = exclusive();
        let text = render();
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
        assert!(text.contains(std::env::consts::OS));
    }
}
