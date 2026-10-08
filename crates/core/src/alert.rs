//! Alerts on what a session costs and how it is going, beside the bell for
//! when it needs you.
//!
//! [`crate::notify`] answers one question — is it my move? — and every other
//! thing worth being interrupted for is a threshold on a number cctop already
//! draws: a session's cost, its burn rate, the day's spend, the share of its
//! tool calls that fail, how long it has been silent while claiming to work.
//! Each is off until `[settings]` gives it a threshold, because a limit is a
//! decision about money and attention that cctop has no business guessing.
//!
//! The rule is the bell's rule: an edge, never a level. A session over budget
//! is still over budget on the next refresh, so an alert fires on the refresh
//! that sees the crossing and not again until the reading has come back down
//! far enough to cross afresh — [`REARM`] of the threshold, so a burn rate
//! that hovers around its limit is one alert rather than one per refresh. The
//! row's marker is the level: it stays on for as long as the reading is past
//! the threshold, so what the bell said once the table keeps saying.
//!
//! Alerts only. Nothing here stops, pauses or signals a session: an agent over
//! budget may be one refresh away from finishing the job it was paid to do,
//! and killing it would throw that spend away rather than save any.

use crate::session::{ActivityState, Session, SubagentStatus};
use crate::util;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// How far back down a reading has to come before the same alert can fire
/// again, as a share of its threshold.
///
/// A fifth: wide enough that a burn rate wobbling around its limit — it is an
/// average of the last few seconds' spend, and it wobbles — is one crossing,
/// narrow enough that a session which really did calm down and then took off
/// again is told about twice.
pub const REARM: f64 = 0.8;

/// The span the error-loop rule measures over.
///
/// The ERR% column is the whole session's rate, which a loop in its last ten
/// minutes barely moves once it has made a few hundred good calls. So the rule
/// takes the difference of the two counters across this window instead, which
/// needs nothing the transcript does not already record — only a memory of
/// what the counters read a few minutes ago.
///
/// ponytail: fixed rather than a setting; the call count is the knob that
/// matters, and a second one would be a setting nobody could reason about.
pub const ERROR_WINDOW: Duration = Duration::from_secs(10 * 60);

/// The thresholds in force, from `[settings]`. Zero is off, for every rule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rules {
    /// Dollars; a session whose COST passes it.
    pub session_cost: f64,
    /// Dollars an hour; a session whose burn rate passes it.
    pub burn_rate: f64,
    /// Dollars; the day's spend across every session passing it.
    pub daily_spend: f64,
    /// A fraction; the share of tool calls failing across [`ERROR_WINDOW`].
    pub error_rate: f64,
    /// The fewest calls in the window a rate is judged on: two failures out of
    /// three is a bad minute, not a loop.
    pub error_calls: u64,
    /// How long a working session may write nothing.
    pub stall: Duration,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            session_cost: 0.0,
            burn_rate: 0.0,
            daily_spend: 0.0,
            error_rate: 0.0,
            error_calls: 10,
            stall: Duration::ZERO,
        }
    }
}

/// Which alert a row is under.
///
/// Ordered by how much the row's marker should say it: a session failing call
/// after call is wasting the money the budget alerts only count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Cost,
    Burn,
    Stall,
    Errors,
}

impl Kind {
    const ALL: [Kind; 4] = [Kind::Cost, Kind::Burn, Kind::Stall, Kind::Errors];

    /// What the status dot turns into. One cell each, like the dot it stands
    /// in for, and told apart by shape rather than only by colour.
    pub fn glyph(self) -> &'static str {
        match self {
            Kind::Cost | Kind::Burn => "$",
            Kind::Errors => "!",
            // A dotted ring: the dot with nothing inside it.
            Kind::Stall => "◌",
        }
    }

    /// The word a webhook's `kind` carries.
    fn name(self) -> &'static str {
        match self {
            Kind::Cost => "cost",
            Kind::Burn => "burn",
            Kind::Stall => "stall",
            Kind::Errors => "errors",
        }
    }
}

/// What a session's own hooks last said, as far as a stall is concerned.
///
/// The stall rule needs it because a transcript alone cannot tell the three
/// silences apart: a turn that has finished, a tool call that is taking its
/// time, and an agent that has stopped getting anywhere all leave the newest
/// record exactly where it was. Only the hooks say which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hooked {
    /// No hooks report on this session.
    Unknown,
    /// Working, with no tool call open.
    Working,
    /// A tool call has begun and not come back, or the context is being
    /// compacted: both are work that writes nothing until it is done.
    InFlight,
    /// The turn is over or the agent is asking: not working at all.
    Quiet,
}

/// One alert that just fired.
#[derive(Debug, Clone, PartialEq)]
pub struct Fired {
    /// The session it is about; `None` for the day's spend, which is nobody's.
    pub key: Option<String>,
    /// `None` for the day's spend, which has no row to mark.
    pub kind: Option<Kind>,
    /// The sentence the toast, the bell and the webhook all carry.
    pub text: String,
}

impl Fired {
    /// The word a webhook's `kind` carries.
    pub fn kind_name(&self) -> &'static str {
        self.kind.map_or("today", Kind::name)
    }
}

/// Where a reading stands against its threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    /// At or past it.
    Above,
    /// Below it, but not far enough to count as having come back.
    Between,
    /// Far enough below that crossing again would be news again. Also what a
    /// rule that is off, or does not apply, reads.
    Clear,
}

fn level(value: f64, threshold: f64) -> Level {
    if threshold <= 0.0 {
        Level::Clear
    } else if value >= threshold {
        Level::Above
    } else if value < threshold * REARM {
        Level::Clear
    } else {
        Level::Between
    }
}

/// One rule's memory of one reading: whether it is past its threshold.
#[derive(Debug, Clone, Copy, Default)]
struct Latch {
    high: bool,
    /// Whether any reading has been taken yet. The first only says where
    /// things stand — a session first seen over budget did not cross in front
    /// of us, exactly as a session first seen idle does not ring the bell.
    seen: bool,
}

impl Latch {
    /// Take a reading, and say whether it is the crossing.
    fn step(&mut self, level: Level) -> bool {
        if !self.seen {
            self.seen = true;
            self.high = level == Level::Above;
            return false;
        }
        match level {
            Level::Above if !self.high => {
                self.high = true;
                true
            }
            Level::Clear => {
                self.high = false;
                false
            }
            _ => false,
        }
    }
}

/// Everything remembered about one running session.
#[derive(Debug, Default)]
struct Watch {
    latches: [Latch; 4],
    /// `(when, tool calls, failed calls)` each time the counters moved, oldest
    /// first, trimmed to what [`ERROR_WINDOW`] still reaches.
    samples: VecDeque<(Instant, u64, u64)>,
}

impl Watch {
    /// Calls and failures since the start of [`ERROR_WINDOW`], by subtraction.
    ///
    /// Samples are only kept when the counters move, so the counters' value at
    /// any moment is the newest sample at or before it. The window's start is
    /// then the newest sample that old — or the first sighting, for a session
    /// cctop has not been watching that long, which is what makes a window
    /// only ever hold calls this cctop saw happen.
    fn window(&mut self, now: Instant, calls: u64, errors: u64) -> (u64, u64) {
        // A counter that went backwards is a transcript re-read from scratch
        // — a cache cleared, a file rewritten — and nothing before it compares.
        if self
            .samples
            .back()
            .is_some_and(|&(_, c, e)| calls < c || errors < e)
        {
            self.samples.clear();
        }
        if self
            .samples
            .back()
            .is_none_or(|&(_, c, e)| (c, e) != (calls, errors))
        {
            self.samples.push_back((now, calls, errors));
        }
        if let Some(start) = now.checked_sub(ERROR_WINDOW) {
            while self.samples.len() > 1 && self.samples[1].0 <= start {
                self.samples.pop_front();
            }
        }
        let (_, c0, e0) = self.samples[0];
        (calls - c0, errors - e0)
    }
}

/// The alert state machine: every rule's latch, for every running session and
/// for the day.
#[derive(Debug, Default)]
pub struct Alerts {
    watched: HashMap<String, Watch>,
    today: Latch,
    /// The rules the latches were taken against. A threshold changed in the
    /// settings panel re-seeds them, so lowering a budget below what a session
    /// has already spent marks the row but does not ring for a crossing nobody
    /// watched happen.
    rules: Option<Rules>,
}

impl Alerts {
    /// Fold a refresh in, returning the alerts that just fired.
    ///
    /// Runs every refresh whether or not any rule is on, like the bell's
    /// machine: the error window needs the counters' history, and a rule
    /// turned on later should start from what is already known.
    pub fn observe(
        &mut self,
        rules: &Rules,
        sessions: &[Session],
        spend_today: f64,
        hooked: impl Fn(&Session) -> Hooked,
        now: Instant,
        wall: DateTime<Utc>,
    ) -> Vec<Fired> {
        if self.rules.as_ref() != Some(rules) {
            self.rules = Some(*rules);
            self.today.seen = false;
            for watch in self.watched.values_mut() {
                watch.latches = Default::default();
            }
        }
        let mut fired = Vec::new();
        let mut next = HashMap::with_capacity(self.watched.len());
        // Only running sessions: a stopped one cannot cross anything, and the
        // map stays a handful of entries rather than one per transcript ever
        // written.
        for session in sessions.iter().filter(|s| s.is_running()) {
            let key = session.key();
            let mut watch = self.watched.remove(&key).unwrap_or_default();
            let label = session.display_label();
            let (calls, errors) = watch.window(now, session.tool_count, session.tool_errors);
            for kind in Kind::ALL {
                let (level, text) = reading(kind, rules, session, (calls, errors), &hooked, wall);
                if watch.latches[kind as usize].step(level) {
                    fired.push(Fired {
                        key: Some(key.clone()),
                        kind: Some(kind),
                        text: format!("{label} {text}"),
                    });
                }
            }
            next.insert(key, watch);
        }
        self.watched = next;

        if self.today.step(level(spend_today, rules.daily_spend)) {
            fired.push(Fired {
                key: None,
                kind: None,
                text: format!(
                    "Today's spend is {}, past the {} alert",
                    util::compact_usd(spend_today),
                    util::compact_usd(rules.daily_spend)
                ),
            });
        }
        fired
    }

    /// The alert a row should wear, if it is under one: the loudest of those
    /// still past their threshold.
    pub fn marker(&self, key: &str) -> Option<Kind> {
        let watch = self.watched.get(key)?;
        Kind::ALL
            .into_iter()
            .rev()
            .find(|&k| watch.latches[k as usize].high)
    }
}

/// One rule's reading of one session, and the words for it should it fire.
fn reading(
    kind: Kind,
    rules: &Rules,
    s: &Session,
    (calls, errors): (u64, u64),
    hooked: &impl Fn(&Session) -> Hooked,
    wall: DateTime<Utc>,
) -> (Level, String) {
    match kind {
        // A cost the plan includes is not money this session is spending, so
        // neither money rule applies to it.
        Kind::Cost => match s.total_cost {
            Some(cost) => (
                level(cost, rules.session_cost),
                format!(
                    "has cost {}, past the {} alert",
                    util::compact_usd(cost),
                    util::compact_usd(rules.session_cost)
                ),
            ),
            None => (Level::Clear, String::new()),
        },
        Kind::Burn => match s.total_cost {
            Some(_) => {
                let per_hour = s.cost_per_min * 60.0;
                (
                    level(per_hour, rules.burn_rate),
                    format!(
                        "is burning {}/h, past the {}/h alert",
                        util::compact_usd(per_hour),
                        util::compact_usd(rules.burn_rate)
                    ),
                )
            }
            None => (Level::Clear, String::new()),
        },
        Kind::Errors if rules.error_rate <= 0.0 => (Level::Clear, String::new()),
        Kind::Errors => {
            // Judged on a fraction of the window, so it needs enough calls to
            // be one; with fewer, a reading that was past it holds rather than
            // clearing, since a loop between two bursts has not stopped.
            let rate = match calls {
                0 => 0.0,
                n => errors as f64 / n as f64,
            };
            let level = match level(rate, rules.error_rate) {
                Level::Above if calls < rules.error_calls.max(1) => Level::Between,
                Level::Clear if calls > 0 && rate >= rules.error_rate * REARM => Level::Between,
                other => other,
            };
            (
                level,
                format!(
                    "is looping: {errors} of its last {calls} tool calls failed ({}m)",
                    ERROR_WINDOW.as_secs() / 60
                ),
            )
        }
        Kind::Stall => stall(rules.stall, s, hooked(s), wall),
    }
}

/// Whether a session that says it is working has gone silent.
///
/// Three things have to agree before it counts. The row reads as working — not
/// waiting on you, not asking, not an API error, which the red dot already
/// says. The hooks say working too, with no tool call open. And nothing has
/// been written for the threshold, counting a running subagent's transcript,
/// since a parent waiting on its subagent writes nothing of its own.
///
/// The hooks are required, not merely consulted. Without them a finished turn
/// is indistinguishable from work in a transcript for most harnesses — see
/// `state_of` in [`crate::notify`] — and this rule would fire ten minutes after
/// every turn ended.
///
/// ponytail: a tool call that never returns is not alerted. A hung `Bash` and
/// a build that takes half an hour look identical from outside — the hook said
/// the call began and nothing has said it ended — and an alert that fires on
/// every long build teaches people to ignore it.
fn stall(threshold: Duration, s: &Session, hooked: Hooked, wall: DateTime<Utc>) -> (Level, String) {
    if threshold.is_zero() || s.activity_state != ActivityState::Working {
        return (Level::Clear, String::new());
    }
    let working = match hooked {
        Hooked::Quiet => return (Level::Clear, String::new()),
        // Nothing to tell a silence by: hold whatever was last decided.
        Hooked::Unknown | Hooked::InFlight => false,
        Hooked::Working => true,
    };
    let newest = std::iter::once(s.last_active.as_str())
        .chain(
            s.subagents
                .iter()
                .filter(|a| a.status == SubagentStatus::Running)
                .filter_map(|a| a.last_active.as_deref()),
        )
        .filter_map(util::parse_ts)
        .max();
    let Some(newest) = newest else {
        return (Level::Between, String::new());
    };
    let silent = (wall - newest).to_std().unwrap_or_default();
    let level = match (silent >= threshold, working) {
        // Something new was written: whatever the hooks say, it is not stuck.
        (false, _) => Level::Clear,
        (true, true) => Level::Above,
        (true, false) => Level::Between,
    };
    (
        level,
        format!(
            "has written nothing for {}m while working",
            silent.as_secs() / 60
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    fn session(id: &str) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.abbrev_label = id.into();
        s.process = Some(crate::proc::ProcInfo::default());
        s.total_cost = Some(0.0);
        s.last_active = chrono::Utc::now().to_rfc3339();
        s
    }

    /// Drive one session through a series of readings, a refresh a minute
    /// apart, and return how many alerts each refresh fired.
    struct Run {
        alerts: Alerts,
        rules: Rules,
        start: Instant,
        wall: DateTime<Utc>,
        tick: u32,
    }

    impl Run {
        fn new(rules: Rules) -> Run {
            Run {
                alerts: Alerts::default(),
                rules,
                start: Instant::now(),
                wall: chrono::Utc::now(),
                tick: 0,
            }
        }

        fn step(&mut self, sessions: &[Session], today: f64, hooked: Hooked) -> Vec<Fired> {
            self.tick += 1;
            let at = Duration::from_secs(60 * self.tick as u64);
            self.alerts.observe(
                &self.rules,
                sessions,
                today,
                |_| hooked,
                self.start + at,
                self.wall + chrono::Duration::from_std(at).unwrap(),
            )
        }
    }

    fn costing(dollars: f64) -> Vec<Session> {
        let mut s = session("a");
        s.total_cost = Some(dollars);
        vec![s]
    }

    /// The whole point: one alert on the crossing, silence while it stays
    /// over, and again only once it has come back down past [`REARM`] and
    /// crossed afresh — not every time it wobbles across the line.
    #[test]
    fn a_threshold_fires_once_per_crossing_with_hysteresis() {
        let mut run = Run::new(Rules {
            session_cost: 10.0,
            ..Rules::default()
        });
        assert!(run.step(&costing(5.0), 0.0, Hooked::Unknown).is_empty());
        let fired = run.step(&costing(10.5), 0.0, Hooked::Unknown);
        assert_eq!(fired.len(), 1, "{fired:?}");
        assert_eq!(fired[0].kind, Some(Kind::Cost));
        assert!(fired[0].text.contains("$10.50"), "{}", fired[0].text);
        assert!(run.step(&costing(11.0), 0.0, Hooked::Unknown).is_empty());
        // Dipping just under the line and back is the same crossing.
        assert!(run.step(&costing(9.5), 0.0, Hooked::Unknown).is_empty());
        assert!(run.step(&costing(10.1), 0.0, Hooked::Unknown).is_empty());
        assert_eq!(run.alerts.marker("claude:a"), Some(Kind::Cost));
        // Well under it is a fresh start.
        assert!(run.step(&costing(7.0), 0.0, Hooked::Unknown).is_empty());
        assert_eq!(run.alerts.marker("claude:a"), None);
        assert_eq!(run.step(&costing(12.0), 0.0, Hooked::Unknown).len(), 1);
    }

    /// Already over when first seen — cctop just started, or the threshold
    /// was just lowered — marks the row but does not ring: no crossing
    /// happened in front of anybody.
    #[test]
    fn a_session_first_seen_over_budget_is_marked_not_rung() {
        let mut run = Run::new(Rules {
            session_cost: 10.0,
            ..Rules::default()
        });
        assert!(run.step(&costing(20.0), 0.0, Hooked::Unknown).is_empty());
        assert_eq!(run.alerts.marker("claude:a"), Some(Kind::Cost));

        run.rules.session_cost = 15.0;
        assert!(run.step(&costing(20.0), 0.0, Hooked::Unknown).is_empty());
        assert_eq!(run.alerts.marker("claude:a"), Some(Kind::Cost));
    }

    #[test]
    fn a_rule_left_at_zero_never_fires() {
        let mut run = Run::new(Rules::default());
        let mut s = costing(1_000.0);
        s[0].cost_per_min = 100.0;
        s[0].tool_count = 50;
        run.step(&s, 5_000.0, Hooked::Working);
        s[0].tool_count = 100;
        s[0].tool_errors = 50;
        s[0].last_active = "2020-01-01T00:00:00Z".into();
        assert!(run.step(&s, 9_000.0, Hooked::Working).is_empty());
        assert_eq!(run.alerts.marker("claude:a"), None);
    }

    #[test]
    fn a_burn_rate_is_judged_per_hour_and_an_included_plan_is_exempt() {
        let mut run = Run::new(Rules {
            burn_rate: 30.0,
            ..Rules::default()
        });
        let mut s = costing(1.0);
        run.step(&s, 0.0, Hooked::Unknown);
        s[0].cost_per_min = 0.6; // $36/h
        let fired = run.step(&s, 0.0, Hooked::Unknown);
        assert_eq!(fired.len(), 1);
        assert!(fired[0].text.contains("$36.00/h"), "{}", fired[0].text);

        // A subscription that includes the provider is not spending anything.
        let mut run = Run::new(run.rules);
        let mut s = costing(1.0);
        s[0].total_cost = None;
        run.step(&s, 0.0, Hooked::Unknown);
        s[0].cost_per_min = 5.0;
        assert!(run.step(&s, 0.0, Hooked::Unknown).is_empty());
    }

    /// The day's spend has no row, and comes back round at midnight: the reset
    /// to nothing clears it, so the next day's crossing is news again.
    #[test]
    fn the_days_spend_fires_once_and_again_the_next_day() {
        let mut run = Run::new(Rules {
            daily_spend: 50.0,
            ..Rules::default()
        });
        assert!(run.step(&[], 40.0, Hooked::Unknown).is_empty());
        let fired = run.step(&[], 55.0, Hooked::Unknown);
        assert_eq!(fired.len(), 1);
        assert_eq!(
            (fired[0].key.as_deref(), fired[0].kind_name()),
            (None, "today")
        );
        assert!(run.step(&[], 60.0, Hooked::Unknown).is_empty());
        assert!(run.step(&[], 0.0, Hooked::Unknown).is_empty());
        assert_eq!(run.step(&[], 51.0, Hooked::Unknown).len(), 1);
    }

    fn calls(total: u64, failed: u64) -> Vec<Session> {
        let mut s = session("a");
        s.tool_count = total;
        s.tool_errors = failed;
        vec![s]
    }

    /// A loop is judged on the calls in the window, not the session's
    /// lifetime ERR%: two hundred clean calls before it must not hide it.
    #[test]
    fn an_error_loop_is_measured_over_the_recent_window() {
        let mut run = Run::new(Rules {
            error_rate: 0.25,
            error_calls: 10,
            ..Rules::default()
        });
        run.step(&calls(200, 2), 0.0, Hooked::Unknown);
        // Five failures in six: bad, but too few calls to call a loop.
        assert!(run.step(&calls(206, 7), 0.0, Hooked::Unknown).is_empty());
        let fired = run.step(&calls(212, 10), 0.0, Hooked::Unknown);
        assert_eq!(fired.len(), 1, "8 of 12 failing is a loop");
        assert!(
            fired[0].text.contains("8 of its last 12"),
            "{}",
            fired[0].text
        );
        assert!(
            fired[0].text.contains(" failed"),
            "worded as a failure, so the toast reads as one"
        );
        assert_eq!(run.alerts.marker("claude:a"), Some(Kind::Errors));
        assert!(run.step(&calls(220, 14), 0.0, Hooked::Unknown).is_empty());

        // Ten minutes of clean calls later, the window holds none of the
        // failures and the latch has cleared.
        let mut total = 220;
        for _ in 0..11 {
            total += 5;
            run.step(&calls(total, 14), 0.0, Hooked::Unknown);
        }
        assert_eq!(run.alerts.marker("claude:a"), None);
    }

    #[test]
    fn a_counter_that_goes_backwards_starts_the_window_over() {
        let mut w = Watch::default();
        let t = Instant::now();
        assert_eq!(w.window(t, 100, 10), (0, 0));
        assert_eq!(w.window(t + Duration::from_secs(60), 110, 15), (10, 5));
        assert_eq!(w.window(t + Duration::from_secs(120), 5, 1), (0, 0));
        // And a sample older than the window stops counting.
        assert_eq!(
            w.window(t + Duration::from_secs(120) + ERROR_WINDOW, 9, 1),
            (4, 0)
        );
    }

    fn silent_for(minutes: i64) -> Vec<Session> {
        let mut s = session("a");
        s.last_active = (chrono::Utc::now() - chrono::Duration::minutes(minutes)).to_rfc3339();
        vec![s]
    }

    /// A stall fires only when the hooks say the agent is working with no
    /// tool open: a finished turn, a long tool call, or no hooks at all are
    /// silences it cannot tell from being stuck.
    #[test]
    fn a_stall_needs_the_hooks_to_say_working() {
        let rules = Rules {
            stall: Duration::from_secs(10 * 60),
            ..Rules::default()
        };
        for quiet in [Hooked::Unknown, Hooked::InFlight, Hooked::Quiet] {
            let mut run = Run::new(rules);
            run.step(&silent_for(0), 0.0, quiet);
            assert!(
                run.step(&silent_for(30), 0.0, quiet).is_empty(),
                "{quiet:?}"
            );
        }

        let mut run = Run::new(rules);
        assert!(run.step(&silent_for(0), 0.0, Hooked::Working).is_empty());
        assert!(run.step(&silent_for(5), 0.0, Hooked::Working).is_empty());
        // The run's clock moves a minute per step, so `silent_for` counts on
        // top of that; either way it is well past ten.
        let fired = run.step(&silent_for(12), 0.0, Hooked::Working);
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].kind, Some(Kind::Stall));
        assert!(run.step(&silent_for(20), 0.0, Hooked::Working).is_empty());
        assert_eq!(run.alerts.marker("claude:a"), Some(Kind::Stall));

        // A tool opening afterwards neither clears it nor fires again; a new
        // record does clear it.
        assert!(run.step(&silent_for(25), 0.0, Hooked::InFlight).is_empty());
        assert_eq!(run.alerts.marker("claude:a"), Some(Kind::Stall));
        run.step(&silent_for(-10), 0.0, Hooked::Working);
        assert_eq!(run.alerts.marker("claude:a"), None);
    }

    /// A parent waiting on its subagent writes nothing, and is not stuck while
    /// the subagent is still writing.
    #[test]
    fn a_running_subagent_counts_as_the_session_writing() {
        let mut run = Run::new(Rules {
            stall: Duration::from_secs(10 * 60),
            ..Rules::default()
        });
        let mut s = silent_for(30);
        let now = (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339();
        s[0].subagents.push(crate::session::Subagent {
            agent_id: "sub".into(),
            agent_type: "general-purpose".into(),
            description: String::new(),
            model: String::new(),
            started_at: None,
            last_active: Some(now),
            duration_ms: 0,
            status: SubagentStatus::Running,
            cost: 0.0,
            tool_count: 0,
            tool_use_id: None,
            context: None,
            ghost: false,
        });
        run.step(&s, 0.0, Hooked::Working);
        assert!(run.step(&s, 0.0, Hooked::Working).is_empty());
        assert_eq!(run.alerts.marker("claude:a"), None);
    }

    /// The row wears the loudest alert it is under, and a session that stops
    /// takes its alerts with it.
    #[test]
    fn the_marker_is_the_loudest_and_leaves_with_the_session() {
        let mut run = Run::new(Rules {
            session_cost: 1.0,
            error_rate: 0.25,
            error_calls: 2,
            ..Rules::default()
        });
        let mut s = calls(0, 0);
        s[0].total_cost = Some(2.0);
        run.step(&s, 0.0, Hooked::Unknown);
        assert_eq!(run.alerts.marker("claude:a"), Some(Kind::Cost));
        s[0].tool_count = 4;
        s[0].tool_errors = 4;
        assert_eq!(run.step(&s, 0.0, Hooked::Unknown).len(), 1);
        assert_eq!(run.alerts.marker("claude:a"), Some(Kind::Errors));

        s[0].process = None;
        run.step(&s, 0.0, Hooked::Unknown);
        assert_eq!(run.alerts.marker("claude:a"), None);
    }
}
