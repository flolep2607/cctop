//! What a subscription window was paid for and did not use.
//!
//! A rate-limit window is use-it-or-lose-it. Claude gives an account five hours
//! and seven days; Codex gives it five hours. When one resets, whatever was not
//! spent is gone — nobody is billed for it and nobody mentions it, so the most
//! expensive thing about a subscription is invisible: the weeks that used a
//! third of it.
//!
//! [`crate::quota`] answers *how full is the window now*, which is what decides
//! whether there is room to keep working. It cannot answer what a window that
//! has already reset came to, because the provider reports only the current
//! figure and the reset destroys the evidence. So this writes the readings down
//! as they go past, and everything here is derived from that log.
//!
//! **The honest limit, which the display must carry rather than bury:** cctop
//! samples only while it is running. A window whose busiest hours happened with
//! cctop closed has a recorded peak below its real one, so usage is understated
//! and the unused share is *overstated*. What this reports is therefore a
//! ceiling on what was wasted, not a measurement of it, and
//! [`Window::coverage`] is what says how much to trust a given figure.

use crate::config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// A single reading of one window.
///
/// Stored per provider, per profile, per window label. The tuple is what the
/// provider said and when cctop heard it — no derived figures, so a change to
/// how burn is calculated re-reads history rather than invalidating it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sample {
    /// Unix seconds when this reading was taken.
    pub at: i64,
    /// Percent of the window consumed, as the provider reported it.
    pub pct: u32,
    /// When the window resets, which is how a reset is detected at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<i64>,
    /// The plan this account was on. Carried per sample because a plan change
    /// makes the history either side of it incomparable, and averaging across
    /// one would invent a figure describing no plan that ever existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
}

/// The log, keyed by `provider/profile/window`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Log {
    #[serde(default)]
    pub series: HashMap<String, Vec<Sample>>,
}

/// How long to keep samples.
///
/// A rolling year makes month-over-month possible, which is the comparison that
/// makes a burn figure mean anything — one week at 40% unused is a quiet week,
/// and every week at 40% unused is a subscription tier.
const RETAIN_SECS: i64 = 365 * 24 * 60 * 60;

/// Cap per series, so a machine left running for a year cannot grow the file
/// without bound. At the poller's five-minute interval this is about three
/// weeks of continuous running per window, and old samples are thinned rather
/// than dropped — see [`Log::record`].
const MAX_SAMPLES: usize = 6000;

pub fn key(provider: &str, profile: &str, window: &str) -> String {
    format!("{provider}/{profile}/{window}")
}

impl Log {
    pub fn load() -> Self {
        Self::load_from(&config::BURN_LOG_FILE)
    }

    fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(&*config::CACHE_DIR);
        self.save_to(&config::BURN_LOG_FILE);
    }

    /// Written through a temporary file, because the poller writes this every
    /// few minutes and a machine that loses power mid-write should lose the
    /// last reading rather than the year.
    fn save_to(&self, path: &Path) {
        let Ok(text) = serde_json::to_string(self) else {
            return;
        };
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// Add a reading, unless it says exactly what the last one said.
    ///
    /// Returns whether anything was stored. An idle account reports the same
    /// percentage against the same reset for hours, and writing each of those
    /// would fill the log with the periods that carry no information while
    /// crowding out the ones that do. What matters is every *change*, plus the
    /// reading immediately before a reset — and the reset moves `resets_at`, so
    /// a repeat is only ever dropped when both figures are unchanged.
    pub fn record(&mut self, key: String, sample: Sample) -> bool {
        let series = self.series.entry(key).or_default();
        if let Some(last) = series.last()
            && last.pct == sample.pct
            && last.resets_at == sample.resets_at
        {
            return false;
        }
        series.push(sample);
        prune(series);
        true
    }

    pub fn windows(&self, key: &str) -> Vec<Window> {
        self.series.get(key).map(|s| windows(s)).unwrap_or_default()
    }
}

/// Drop what is too old, then thin what is too dense.
///
/// Thinning keeps every other sample from the oldest half, which preserves the
/// shape of old history at half the resolution rather than truncating it to a
/// cliff. The recent half is what a live chart draws and is left alone.
///
/// Age is measured against the newest sample in the series, not against the
/// clock. Against the clock, a log restored from a backup — or read on a
/// machine whose clock is wrong, which is the same machine whose `resets_at`
/// arithmetic is already suspect — deletes itself on the first write. Relative
/// to its own newest entry, a year of history is a year of history whenever it
/// is read.
fn prune(series: &mut Vec<Sample>) {
    let Some(newest) = series.iter().map(|s| s.at).max() else {
        return;
    };
    let cutoff = newest - RETAIN_SECS;
    series.retain(|s| s.at >= cutoff);
    if series.len() <= MAX_SAMPLES {
        return;
    }
    let half = series.len() / 2;
    let mut thinned: Vec<Sample> = series[..half].iter().step_by(2).cloned().collect();
    thinned.extend_from_slice(&series[half..]);
    *series = thinned;
}

/// One window that has been through a reset, reconstructed from the samples
/// that were taken while it was open.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    /// When the window ended, as a Unix timestamp.
    pub ended_at: i64,
    /// The highest percentage seen before it reset. A floor on the true peak:
    /// see the module docs.
    pub peak: u32,
    /// Samples taken while this window was open.
    pub samples: usize,
    /// Seconds between the first and last sample of this window.
    pub observed_secs: i64,
    /// Nominal length of the window, when it can be derived.
    pub length_secs: Option<i64>,
    pub plan: Option<String>,
}

impl Window {
    /// Share of the window cctop was watching, `None` where the window's length
    /// is unknown and the fraction would have no denominator.
    ///
    /// This is the number that says how much the rest of the row is worth. A
    /// window observed for a tenth of its life has a peak that means very
    /// little, and reporting its unused share as though it were measured would
    /// be the dishonest part of this whole feature.
    pub fn coverage(&self) -> Option<f64> {
        let length = self.length_secs.filter(|l| *l > 0)?;
        Some((self.observed_secs as f64 / length as f64).clamp(0.0, 1.0))
    }

    /// Percentage of the allowance that went unused, as an upper bound.
    pub fn unused(&self) -> u32 {
        100u32.saturating_sub(self.peak)
    }

    /// Whether coverage is thin enough that the figure should be read as
    /// "at least this much was used" rather than as a measurement.
    ///
    /// Two thirds is a judgement, not a derivation: below it, a window has
    /// enough unobserved time to hide an entire working session.
    pub fn thin(&self) -> bool {
        self.coverage().is_none_or(|c| c < 0.66)
    }
}

/// Split a series into the windows it covers, newest last.
///
/// A reset is visible without being announced: `resets_at` moves forward, or —
/// when a provider does not report one — the percentage falls, which a window
/// that only accumulates cannot otherwise do. Both are needed. The reset time
/// alone misses a provider that omits it, and a falling percentage alone would
/// read a provider's own correction as a new window.
fn windows(series: &[Sample]) -> Vec<Window> {
    let mut out = Vec::new();
    let mut current: Vec<&Sample> = Vec::new();

    for sample in series {
        let boundary = match current.last() {
            None => false,
            Some(prev) => match (prev.resets_at, sample.resets_at) {
                (Some(a), Some(b)) => b > a,
                _ => sample.pct < prev.pct,
            },
        };
        if boundary {
            out.extend(finish(&current));
            current.clear();
        }
        current.push(sample);
    }
    // The window still open is deliberately not reported: it has not been
    // forfeited yet, and calling its unused share "wasted" while there is still
    // time to spend it would be wrong in the direction that annoys people.
    out
}

fn finish(samples: &[&Sample]) -> Option<Window> {
    let first = samples.first()?;
    let last = samples.last()?;
    // The window's length, taken from the reset time it was counting down to.
    // Providers report the reset but not the start, so the length has to come
    // from the pair — and is unavailable when the reset was never reported.
    let length_secs = last.resets_at.map(|reset| reset - first.at);
    Some(Window {
        ended_at: last.resets_at.unwrap_or(last.at),
        peak: samples.iter().map(|s| s.pct).max().unwrap_or(0),
        samples: samples.len(),
        observed_secs: last.at - first.at,
        length_secs: length_secs.filter(|l| *l > 0),
        plan: last.plan.clone(),
    })
}

/// The average unused share across completed windows, and how many of them fed
/// it.
///
/// Windows with thin coverage are excluded rather than averaged in: a window
/// cctop barely saw would drag the figure toward "you used none of it", which
/// is exactly the wrong direction for a number meant to prompt a downgrade.
/// Returns `None` when nothing is left, which is an honest answer and not a
/// zero.
pub fn average_unused(windows: &[Window]) -> Option<(u32, usize)> {
    let usable: Vec<&Window> = windows.iter().filter(|w| !w.thin()).collect();
    if usable.is_empty() {
        return None;
    }
    let total: u32 = usable.iter().map(|w| w.unused()).sum();
    Some((total / usable.len() as u32, usable.len()))
}

// ---------------------------------------------------------------------------
// `cctop burn`
// ---------------------------------------------------------------------------

/// Eight levels, so a series of percentages can be drawn in one line of text.
const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A sparkline of percentages, drawn against a fixed 0–100 scale.
///
/// Fixed rather than scaled to the data, which is the whole point: a week that
/// peaked at 12% must look like a week that peaked at 12%, not like a full bar
/// because it was the busiest week on record. An auto-scaled chart of unused
/// allowance would say the opposite of what happened.
pub fn spark(pcts: &[u32]) -> String {
    pcts.iter()
        .map(|p| BLOCKS[((*p).min(100) as usize * (BLOCKS.len() - 1)) / 100])
        .collect()
}

/// The short figure for the Limits pane, or `None` when there is nothing
/// trustworthy to say yet.
///
/// Deliberately silent rather than provisional. This sits on a line somebody
/// reads to decide whether there is room to keep working, and a burn figure
/// from two thinly-observed windows would be noise in the middle of it.
pub fn suffix(log: &Log, provider: &str, profile: &str, window: &str) -> Option<String> {
    let (unused, n) = average_unused(&log.windows(&key(provider, profile, window)))?;
    // Two completed windows is not a pattern. One quiet week is a quiet week.
    (n >= 2).then(|| format!("~{unused}% of {window} unused"))
}

pub const HELP: &str = "\
cctop burn — what your subscription windows were paid for and did not use

USAGE:
  cctop burn [--json]

A rate-limit window is use-it-or-lose-it: when it resets, whatever was not
spent is gone. The provider reports only how full the window is now, so cctop
writes those readings down as it sees them and reconstructs what each completed
window came to.

It only sees them while it is running. A window whose busiest hours happened
with cctop closed has a recorded peak below its real one, so the unused share
is an upper bound rather than a measurement — every row says how much of its
window was actually observed, and thinly-observed windows are left out of the
averages.

OPTIONS:
  --json      Machine-readable.
  -h, --help  This.
";

pub fn run(argv: &[String]) -> i32 {
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return 0;
    }
    let json = argv.iter().any(|a| a == "--json");
    if let Some(bad) = argv.iter().find(|a| *a != "--json") {
        eprintln!("cctop burn: unexpected argument `{bad}`; see --help");
        return 2;
    }

    let log = Log::load();
    match json {
        true => println!("{}", as_json(&log)),
        false => print!("{}", report(&log)),
    }
    0
}

/// Every series, oldest window first, with its plan and its own key.
fn series_in_order(log: &Log) -> Vec<(&String, Vec<Window>)> {
    let mut keys: Vec<&String> = log.series.keys().collect();
    keys.sort();
    keys.into_iter()
        .map(|k| (k, log.windows(k)))
        .filter(|(_, w)| !w.is_empty())
        .collect()
}

pub fn report(log: &Log) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let series = series_in_order(log);

    if series.is_empty() {
        return "No completed windows recorded yet.\n\n\
                cctop writes down each account's rate-limit reading while it \
                runs, and\na window has to reset before there is anything to \
                say about it — so this\nfills in after a few hours of Codex or \
                a few days of Claude.\n"
            .into();
    }

    out.push('\n');
    for (key, windows) in &series {
        let plan = windows
            .iter()
            .rev()
            .find_map(|w| w.plan.clone())
            .map(|p| format!("  ({p})"))
            .unwrap_or_default();
        let _ = writeln!(out, "  {key}{plan}");

        let usable: Vec<&Window> = windows.iter().filter(|w| !w.thin()).collect();
        let peaks: Vec<u32> = windows.iter().map(|w| w.peak).collect();
        let _ = writeln!(
            out,
            "  {}  peak of each window, oldest first",
            spark(&peaks)
        );

        match average_unused(windows) {
            Some((unused, n)) => {
                let _ = writeln!(
                    out,
                    "  {unused}% went unused on average, across {} well-observed {}",
                    n,
                    match n {
                        1 => "window",
                        _ => "windows",
                    }
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "  no average: none of these {} windows was observed for long enough",
                    windows.len()
                );
            }
        }
        if usable.len() < windows.len() {
            let _ = writeln!(
                out,
                "  {} of {} left out — cctop was not running for enough of them",
                windows.len() - usable.len(),
                windows.len()
            );
        }
        out.push('\n');
    }

    out.push_str(
        "  An upper bound, not a measurement. cctop samples only while it is\n  \
         running, so a window it half-watched looks quieter than it was and its\n  \
         unused share reads high. Percentages are also not dollars: how a\n  \
         provider maps tokens onto a percentage is undocumented, so a share of\n  \
         a plan's price derived from one would be an illustration.\n",
    );
    out
}

fn as_json(log: &Log) -> String {
    let doc: Vec<serde_json::Value> = series_in_order(log)
        .into_iter()
        .map(|(key, windows)| {
            let avg = average_unused(&windows);
            serde_json::json!({
                "key": key,
                "average_unused_pct": avg.map(|(u, _)| u),
                "windows_averaged": avg.map(|(_, n)| n),
                "windows": windows.iter().map(|w| serde_json::json!({
                    "ended_at": w.ended_at,
                    "peak_pct": w.peak,
                    "unused_pct": w.unused(),
                    "samples": w.samples,
                    "observed_secs": w.observed_secs,
                    "length_secs": w.length_secs,
                    "coverage": w.coverage(),
                    "thinly_observed": w.thin(),
                    "plan": w.plan,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "series": doc,
        "caveat": "Unused shares are upper bounds. cctop samples only while it \
                   is running, so a partly-observed window understates usage and \
                   overstates what was left. Percentages are not dollars.",
    }))
    .unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(at: i64, pct: u32, resets_at: i64) -> Sample {
        Sample {
            at,
            pct,
            resets_at: Some(resets_at),
            plan: None,
        }
    }

    /// An idle account repeats itself for hours. Storing every repeat fills the
    /// log with the periods that carry no information and crowds out the ones
    /// that do.
    #[test]
    fn a_reading_that_says_nothing_new_is_not_stored() {
        let mut log = Log::default();
        let k = key("claude", "default", "7d");
        assert!(log.record(k.clone(), sample(100, 40, 1000)));
        assert!(!log.record(k.clone(), sample(400, 40, 1000)));
        assert!(log.record(k.clone(), sample(700, 41, 1000)));
        assert_eq!(log.series[&k].len(), 2);

        // The same percentage against a *new* window is new information: it is
        // how a reset is seen at all.
        assert!(log.record(k.clone(), sample(1100, 41, 2000)));
        assert_eq!(log.series[&k].len(), 3);
    }

    /// The peak of a window is only recoverable from the samples taken before
    /// it reset — the provider reports the current figure and nothing else.
    #[test]
    fn a_completed_window_reports_the_highest_reading_it_saw() {
        let mut log = Log::default();
        let k = key("claude", "default", "7d");
        for s in [
            sample(0, 10, 1000),
            sample(300, 55, 1000),
            sample(600, 48, 1000), // a provider correction, not a reset
            sample(900, 62, 1000),
            // resets_at moves: a new window
            sample(1000, 5, 2000),
        ] {
            log.record(k.clone(), s);
        }
        let windows = log.windows(&k);
        assert_eq!(windows.len(), 1, "only the completed window is reported");
        assert_eq!(windows[0].peak, 62);
        assert_eq!(windows[0].unused(), 38);
    }

    /// The window still running has not been forfeited. Reporting its unused
    /// share as waste while there is still time to spend it would be wrong.
    #[test]
    fn the_open_window_is_not_reported_as_wasted() {
        let mut log = Log::default();
        let k = key("codex", "default", "5h");
        log.record(k.clone(), sample(0, 5, 1000));
        log.record(k.clone(), sample(300, 20, 1000));
        assert!(log.windows(&k).is_empty());
    }

    /// cctop only sees a window while it is running, so a barely-observed
    /// window understates usage and overstates what was left unused. Averaging
    /// those in would push the headline toward "you used none of it", which is
    /// the wrong direction for a figure that might prompt a downgrade.
    #[test]
    fn a_barely_observed_window_is_not_averaged_in() {
        let long = Window {
            ended_at: 10_000,
            peak: 90,
            samples: 20,
            observed_secs: 900,
            length_secs: Some(1000),
            plan: None,
        };
        let glimpsed = Window {
            ended_at: 20_000,
            peak: 5,
            samples: 2,
            observed_secs: 50,
            length_secs: Some(1000),
            plan: None,
        };
        assert!(!long.thin());
        assert!(glimpsed.thin());

        assert_eq!(
            average_unused(&[long.clone(), glimpsed.clone()]),
            Some((10, 1))
        );
        // And with nothing well-observed, no figure at all rather than a zero.
        assert_eq!(average_unused(&[glimpsed]), None);
    }

    /// A window whose length was never reported has no denominator, so its
    /// coverage is unknown — and unknown coverage is thin coverage.
    #[test]
    fn a_window_of_unknown_length_is_treated_as_thin() {
        let w = Window {
            ended_at: 0,
            peak: 50,
            samples: 5,
            observed_secs: 100,
            length_secs: None,
            plan: None,
        };
        assert_eq!(w.coverage(), None);
        assert!(w.thin());
    }
}
