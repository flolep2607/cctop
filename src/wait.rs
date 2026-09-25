//! `cctop wait`: block until a session stops working.
//!
//! The dashboard answers "is that agent done yet?" for a person. This answers
//! it for a script, or for another agent — the second agent in a pair that
//! must not start until the first has finished, or a driver that types a
//! prompt with `s` and wants to know when the reply is in. Everything it reads
//! is what the table reads, from the same places, so the two never disagree
//! about what a session is doing:
//!
//! - the transcripts, through the same [`Loader`] — a full walk once to find the
//!   session, then [`Loader::refresh_live`] on a timer, which is the sweep the
//!   dashboard runs five times a second and costs only the live rows;
//! - the agents' own hooks, live: a [`Listener`](crate::hook::Listener) is
//!   bound like any cctop's, so `cctop hook` fans each event out to this
//!   process too, and a finished turn or a held permission prompt is heard the
//!   moment it happens rather than at the next transcript read;
//! - what a cctop already heard and wrote onto the agent's rmux session
//!   ([`rmux::State`]), which is the only way to know about a question that was
//!   asked before this started.
//!
//! Nothing is written and nothing is typed. It is a reader that blocks.
//!
//! ponytail: a Claude Code transcript cannot say a turn is over, so a Claude
//! session that went quiet *before* the wait began, with no rmux record of it,
//! reads as working until its next hook event. The dashboard has the same
//! blind spot on its first frame; the wait simply sits in it longer. Keeping a
//! copy of every hook report on disk would close it, and would put a write on
//! the path of every hook fire, which is the one path that must stay cheap.

use crate::hook::Signal;
use crate::loader::Loader;
use crate::pricing::Plan;
use crate::session::{ActivityState, Session};
use std::time::{Duration, Instant};

/// How often the transcripts are re-read. The hooks are the fast path; this is
/// for the agents that have none, and a second is well inside the time it
/// takes anyone to act on the answer.
const REFRESH_EVERY: Duration = Duration::from_secs(1);

/// How often rmux's record of the session is re-read, for a state some cctop
/// heard and this process did not.
const RMUX_EVERY: Duration = Duration::from_secs(2);

/// How often the loop wakes to drain hook events and check the deadline.
const TICK: Duration = Duration::from_millis(200);

/// What to wait for.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Until {
    /// The turn is over and the prompt is the user's.
    Idle,
    /// The agent is blocked on a question — a permission prompt, an MCP
    /// elicitation — and cannot go on until it is answered.
    Waiting,
    /// It was working, and now it is not: a turn that ended after the wait
    /// began, whichever way it ended.
    Done,
    /// It is not working right now, for any reason. Met at once by a session
    /// that is already stopped.
    AnyStop,
}

impl Until {
    pub fn label(self) -> &'static str {
        match self {
            Until::Idle => "idle",
            Until::Waiting => "waiting",
            Until::Done => "done",
            Until::AnyStop => "any-stop",
        }
    }

    pub fn parse(word: &str) -> Option<Until> {
        [Until::Idle, Until::Waiting, Until::Done, Until::AnyStop]
            .into_iter()
            .find(|until| until.label() == word)
    }

    /// Whether `now` meets this: `Some(true)` it does, `Some(false)` it never
    /// will, `None` keep waiting.
    ///
    /// `worked` is whether the session has been working at any point since the
    /// wait began. It is what separates `done` from `any-stop`: a prompt typed
    /// a moment ago may not have reached the agent yet, and an agent still
    /// sitting at its prompt is not an answer to "has it finished what I just
    /// asked?".
    pub fn met(self, now: Now, worked: bool) -> Option<bool> {
        match (self, now) {
            (_, Now::Working) => None,
            (Until::AnyStop, _) => Some(true),
            (Until::Done, Now::Ended) => Some(true),
            (Until::Done, _) => worked.then_some(true),
            (Until::Idle, Now::Idle) | (Until::Waiting, Now::Asking) => Some(true),
            // Ended, it will never be either.
            (Until::Idle | Until::Waiting, Now::Ended) => Some(false),
            // Stopped the other way — asking when idle was wanted, or in an
            // API error. Either can still turn into the one wanted.
            (Until::Idle | Until::Waiting, _) => None,
        }
    }
}

/// What a session is doing, in the words the wait reports it with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Now {
    Working,
    Idle,
    Asking,
    Error,
    Ended,
}

impl Now {
    pub fn label(self) -> &'static str {
        match self {
            Now::Working => "working",
            Now::Idle => "idle",
            Now::Asking => "waiting",
            Now::Error => "error",
            Now::Ended => "ended",
        }
    }

    /// A row's state, with the agent's own last word on top when there is one.
    ///
    /// The hook outranks the transcript the way it does on the dashboard:
    /// the agent saying its turn is over is the fact the transcript is only
    /// estimating, and a held permission prompt leaves no trace on disk at all.
    fn of(session: &Session, heard: Option<Signal>) -> Now {
        if !session.is_running() {
            return Now::Ended;
        }
        match heard {
            Some(Signal::Ended) => Now::Ended,
            Some(Signal::NeedsInput) => Now::Asking,
            Some(Signal::Idle) => Now::Idle,
            Some(signal) if signal.is_working() => Now::Working,
            _ => match session.activity_state {
                ActivityState::Working => Now::Working,
                ActivityState::WaitingForInput => Now::Idle,
                ActivityState::Asking => Now::Asking,
                ActivityState::ApiError => Now::Error,
            },
        }
    }
}

/// Why a target named no session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unresolved {
    Unknown,
    /// More than one session answers to it, none of them better than the rest.
    /// The ids, so the caller can say which to pick between.
    Ambiguous(Vec<String>),
}

/// Find the session `target` names, in `sessions`, given the rmux tabs.
///
/// Tried in order of how exactly each names one thing:
///
/// 1. A number is a pid — the agent's own, or any process in its tree, which
///    is what `$PPID` inside an agent's shell gives.
/// 2. A tab's name, as the bar shows it, or its rmux session name.
/// 3. A prefix of a session id. Several matches with one of them running is
///    that one: the question is asked about something live, and an id prefix
///    long enough to be typed usually is.
///
/// A number that is no pid is still tried as the other two — uuid prefixes are
/// often all digits.
pub fn resolve(
    sessions: &[Session],
    tabs: &[crate::rmux::Running],
    target: &str,
) -> Result<usize, Unresolved> {
    let target = target.trim();
    if target.is_empty() {
        return Err(Unresolved::Unknown);
    }
    let by_pid = |pid: u32| {
        sessions
            .iter()
            .position(|s| s.root_pid() == Some(pid))
            .or_else(|| {
                sessions.iter().position(|s| {
                    s.process
                        .as_ref()
                        .is_some_and(|p| p.process_list.iter().any(|e| e.pid == pid && !e.ghost))
                })
            })
    };
    if let Ok(pid) = target.parse::<u32>()
        && let Some(found) = by_pid(pid)
    {
        return Ok(found);
    }
    let lower = target.to_lowercase();
    if let Some(found) = tabs
        .iter()
        .filter(|tab| {
            tab.name == target
                || tab
                    .label
                    .as_deref()
                    .is_some_and(|label| label.to_lowercase() == lower)
        })
        .find_map(|tab| tab.pid.and_then(by_pid))
    {
        return Ok(found);
    }
    let matching: Vec<usize> = sessions
        .iter()
        .enumerate()
        .filter(|(_, s)| s.session_id.starts_with(target))
        .map(|(i, _)| i)
        .collect();
    match matching.as_slice() {
        [] => Err(Unresolved::Unknown),
        [one] => Ok(*one),
        several => {
            let live: Vec<usize> = several
                .iter()
                .copied()
                .filter(|&i| sessions[i].is_running())
                .collect();
            match live.as_slice() {
                [one] => Ok(*one),
                _ => Err(Unresolved::Ambiguous(
                    several
                        .iter()
                        .map(|&i| sessions[i].session_id.clone())
                        .collect(),
                )),
            }
        }
    }
}

/// How a wait ended.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub session_id: String,
    pub provider: &'static str,
    pub pid: Option<u32>,
    pub label: String,
    pub state: Now,
    pub until: Until,
    /// `Some(true)` met, `Some(false)` can no longer be met, `None` timed out.
    pub met: Option<bool>,
    pub waited: Duration,
}

impl Outcome {
    /// The exit code a script branches on: 0 met, 1 the session ended first,
    /// 124 the timeout — `timeout(1)`'s own, so a wait reads like one.
    pub fn exit_code(&self) -> i32 {
        match self.met {
            Some(true) => 0,
            Some(false) => 1,
            None => 124,
        }
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "session": self.session_id,
            "provider": self.provider,
            "pid": self.pid,
            "label": self.label,
            "state": self.state.label(),
            "until": self.until.label(),
            "met": self.met == Some(true),
            "timed_out": self.met.is_none(),
            "waited_secs": (self.waited.as_secs_f64() * 10.0).round() / 10.0,
        })
    }

    /// One line for a person.
    pub fn sentence(&self) -> String {
        let secs = self.waited.as_secs();
        let who = format!("{} ({})", short(&self.session_id), self.label);
        match self.met {
            Some(true) => format!("{who} is {} after {secs}s", self.state.label()),
            Some(false) => format!(
                "{who} ended after {secs}s without becoming {}",
                self.until.label()
            ),
            None => format!(
                "{who} is still {} after {secs}s; gave up waiting for {}",
                self.state.label(),
                self.until.label()
            ),
        }
    }
}

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

/// The last thing the agent said about itself, however it reached us.
#[derive(Debug, Clone, Copy)]
struct Heard {
    signal: Signal,
    /// Wall-clock seconds: rmux's record carries no `Instant`, and the two are
    /// compared to pick the newer.
    at: u64,
    provisional: bool,
}

impl Heard {
    /// The signal, if it is still believed and no longer provisional — the
    /// same two tests the dashboard applies before a report reaches a row.
    fn believed(&self, now: u64) -> Option<Signal> {
        let age = Duration::from_secs(now.saturating_sub(self.at));
        let settled = !self.provisional || age >= crate::hook::PERMISSION_GRACE;
        (settled && self.signal.is_current_after(age)).then_some(self.signal)
    }
}

/// Keep `heard` if it is newer than what is already there.
fn hear(slot: &mut Option<Heard>, heard: Heard) {
    if slot.is_none_or(|old| heard.at >= old.at) {
        *slot = Some(heard);
    }
}

/// Wait on `target` until `until` or `timeout` (`None` for no limit).
///
/// `listen` binds a hook socket for the length of the wait. Off for the MCP
/// server: see [`crate::mcp`] for why.
pub fn wait(
    target: &str,
    until: Until,
    timeout: Option<Duration>,
    listen: bool,
) -> Result<Outcome, Unresolved> {
    let started = Instant::now();
    let wall_start = chrono::Utc::now();
    // Bound before the walk, so an event fired while the transcripts are
    // being read is not lost to the gap.
    let listener = listen.then(crate::hook::Listener::start).flatten();
    let mut loader = Loader::new();
    loader.set_hook_claims(crate::hook::load_claims());
    let plan = Plan::Retail;
    let mut sessions = loader.load(plan);
    let mut tabs = crate::rmux::running();
    let at = resolve(&sessions, &tabs, target)?;
    let key = sessions[at].key();
    let id = sessions[at].session_id.clone();
    let mut pid = sessions[at].root_pid();

    let mut heard: Option<Heard> = None;
    let mut worked = false;
    let mut refreshed = Instant::now();
    let mut reread_rmux = Instant::now();
    loop {
        let now_secs = crate::rmux::now_secs();
        // Followed by key, then by pid: an agent that had written nothing yet
        // is a `_pid_` row until its transcript appears, and the row that
        // replaces it has a new key and the same process.
        let current = sessions
            .iter()
            .find(|s| s.key() == key)
            .or_else(|| pid.and_then(|p| sessions.iter().find(|s| s.root_pid() == Some(p))));
        if let Some(recorded) = pid
            .and_then(|p| tabs.iter().find(|tab| tab.pid == Some(p)))
            .and_then(|tab| tab.state)
        {
            hear(
                &mut heard,
                Heard {
                    signal: recorded.signal,
                    at: recorded.at,
                    provisional: false,
                },
            );
        }
        let state = match current {
            Some(session) => Now::of(session, heard.and_then(|h| h.believed(now_secs))),
            None => Now::Ended,
        };
        worked |= state == Now::Working
            || current
                .and_then(|s| crate::util::parse_ts(&s.last_active))
                .is_some_and(|last| last > wall_start);
        let met = until.met(state, worked);
        let timed_out = timeout.is_some_and(|limit| started.elapsed() >= limit);
        if met.is_some() || timed_out {
            let fallback = || sessions.iter().find(|s| s.session_id == id);
            let session = current.or_else(fallback);
            return Ok(Outcome {
                session_id: session.map_or(id.clone(), |s| s.session_id.clone()),
                provider: session.map_or("", |s| s.provider.as_str()),
                pid,
                label: session.map_or_else(String::new, |s| s.display_label().to_string()),
                state,
                until,
                met,
                waited: started.elapsed(),
            });
        }

        std::thread::sleep(TICK);
        if let Some(listener) = &listener {
            for event in listener.drain() {
                let ours = event.session_id == id
                    || current.is_some_and(|s| s.launched_as() == event.session_id)
                    || pid.is_some_and(|p| event.pids.contains(&p));
                if ours {
                    hear(
                        &mut heard,
                        Heard {
                            signal: event.reported.signal,
                            at: crate::rmux::now_secs(),
                            provisional: event.reported.provisional,
                        },
                    );
                }
            }
        }
        if refreshed.elapsed() >= REFRESH_EVERY {
            loader.refresh_live(plan, &mut sessions);
            refreshed = Instant::now();
            if pid.is_none() {
                pid = sessions
                    .iter()
                    .find(|s| s.key() == key)
                    .and_then(Session::root_pid);
            }
        }
        if reread_rmux.elapsed() >= RMUX_EVERY && !tabs.is_empty() {
            tabs = crate::rmux::running();
            reread_rmux = Instant::now();
        }
    }
}

/// Parse a timeout: `90`, `90s`, `10m`, `2h`. `0` is no limit, and parses to
/// zero rather than `None`: clap reads an `Option` field as "the flag may be
/// absent", which this one, with its default, never is.
pub fn parse_timeout(text: &str) -> Result<Duration, String> {
    let text = text.trim();
    let (number, unit) = match text.find(|c: char| !c.is_ascii_digit() && c != '.') {
        Some(at) => text.split_at(at),
        None => (text, "s"),
    };
    let scale = match unit {
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        _ => {
            return Err(format!(
                "'{text}': use a number of seconds, or 30s, 10m, 2h"
            ));
        }
    };
    let value: f64 = number
        .parse()
        .map_err(|_| format!("'{text}': use a number of seconds, or 30s, 10m, 2h"))?;
    let secs = value * scale;
    if !secs.is_finite() || secs < 0.0 || secs > u32::MAX as f64 {
        return Err(format!("'{text}' is not a timeout this can wait"));
    }
    Ok(Duration::from_secs_f64(secs))
}

/// `cctop wait`, from its parsed arguments. Returns the exit code.
pub fn run(args: &crate::cli::WaitArgs) -> i32 {
    let timeout = (!args.timeout.is_zero()).then_some(args.timeout);
    match wait(&args.target, args.until, timeout, true) {
        Ok(outcome) => {
            match args.json {
                true => println!("{}", outcome.json()),
                false => println!("{}", outcome.sentence()),
            }
            outcome.exit_code()
        }
        Err(why) => {
            let text = match &why {
                Unresolved::Unknown => format!(
                    "No session, tab or pid here is '{}'. `cctop -l` lists them.",
                    args.target
                ),
                Unresolved::Ambiguous(ids) => format!(
                    "'{}' could be any of {} sessions: {}",
                    args.target,
                    ids.len(),
                    ids.iter()
                        .map(|id| short(id))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            };
            match args.json {
                true => println!(
                    "{}",
                    serde_json::json!({ "target": args.target, "error": text })
                ),
                false => eprintln!("{text}"),
            }
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    fn live(id: &str, pid: u32, state: ActivityState) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.process = Some(crate::proc::ProcInfo {
            process_list: vec![
                crate::proc::ProcEntry {
                    pid,
                    is_root: true,
                    ghost: false,
                    cpu: 0.0,
                    memory: 0,
                    args: String::new(),
                },
                crate::proc::ProcEntry {
                    pid: pid + 1,
                    is_root: false,
                    ghost: false,
                    cpu: 0.0,
                    memory: 0,
                    args: String::new(),
                },
            ],
            ..Default::default()
        });
        s.activity_state = state;
        s
    }

    fn dead(id: &str) -> Session {
        Session::new(Provider::Claude, id.into())
    }

    fn tab(label: &str, pid: u32) -> crate::rmux::Running {
        crate::rmux::Running {
            name: format!("cctop-{label}"),
            pid: Some(pid),
            cwd: None,
            attached: false,
            activity: None,
            label: Some(label.to_string()),
            profile: None,
            order: None,
            state: None,
            color: None,
        }
    }

    /// Each way of naming a session finds it, most exact first.
    #[test]
    fn a_target_is_a_pid_a_tab_or_an_id_prefix() {
        let sessions = vec![
            live("3f2a-live", 500, ActivityState::Working),
            dead("3f2a-old"),
            dead("9c1d-only"),
            live("12345-digits", 700, ActivityState::Working),
        ];
        let tabs = vec![tab("Reviewer", 500)];

        assert_eq!(resolve(&sessions, &tabs, "500"), Ok(0), "the agent's pid");
        assert_eq!(resolve(&sessions, &tabs, "501"), Ok(0), "a child's pid");
        assert_eq!(resolve(&sessions, &tabs, "reviewer"), Ok(0), "a tab name");
        assert_eq!(resolve(&sessions, &tabs, "cctop-Reviewer"), Ok(0));
        assert_eq!(resolve(&sessions, &tabs, "9c1d"), Ok(2), "an id prefix");
        // Two ids share it and one is live: that one.
        assert_eq!(resolve(&sessions, &tabs, "3f2a"), Ok(0));
        // A number that is no pid is still an id prefix.
        assert_eq!(resolve(&sessions, &tabs, "1234"), Ok(3));
    }

    #[test]
    fn a_target_naming_nothing_or_too_much_says_which() {
        let sessions = vec![dead("aa-1"), dead("aa-2")];
        assert_eq!(resolve(&sessions, &[], "zz"), Err(Unresolved::Unknown));
        assert_eq!(resolve(&sessions, &[], "  "), Err(Unresolved::Unknown));
        assert_eq!(
            resolve(&sessions, &[], "aa"),
            Err(Unresolved::Ambiguous(vec!["aa-1".into(), "aa-2".into()]))
        );
        // A tab whose agent has no row is not a session to wait on.
        assert_eq!(
            resolve(&sessions, &[tab("shell", 99)], "shell"),
            Err(Unresolved::Unknown)
        );
    }

    /// Every condition against every state, including the two that can
    /// never now be met.
    #[test]
    fn each_condition_is_met_by_the_states_it_names() {
        use Now::*;
        use Until::*;
        for until in [Until::Idle, Waiting, Done, AnyStop] {
            assert_eq!(until.met(Working, true), None, "{until:?} met mid-turn");
        }
        assert_eq!(AnyStop.met(Now::Idle, false), Some(true));
        assert_eq!(AnyStop.met(Error, false), Some(true));
        assert_eq!(AnyStop.met(Ended, false), Some(true));

        assert_eq!(Until::Idle.met(Now::Idle, false), Some(true));
        assert_eq!(
            Until::Idle.met(Asking, true),
            None,
            "a question may still end idle"
        );
        assert_eq!(Until::Idle.met(Ended, true), Some(false));

        assert_eq!(Waiting.met(Asking, false), Some(true));
        assert_eq!(Waiting.met(Now::Idle, true), None);
        assert_eq!(Waiting.met(Ended, false), Some(false));

        // Done needs a turn to have happened since the wait began.
        assert_eq!(Done.met(Now::Idle, false), None, "an old stop counted");
        assert_eq!(Done.met(Now::Idle, true), Some(true));
        assert_eq!(Done.met(Asking, true), Some(true));
        assert_eq!(Done.met(Ended, false), Some(true));
    }

    /// The agent's own word outranks the transcript; a dead process outranks
    /// both.
    #[test]
    fn the_state_is_the_hooks_over_the_transcript() {
        let working = live("a", 1, ActivityState::Working);
        assert_eq!(Now::of(&working, None), Now::Working);
        assert_eq!(Now::of(&working, Some(Signal::Idle)), Now::Idle);
        assert_eq!(Now::of(&working, Some(Signal::NeedsInput)), Now::Asking);
        let idle = live("a", 1, ActivityState::WaitingForInput);
        assert_eq!(Now::of(&idle, Some(Signal::Busy)), Now::Working);
        assert_eq!(Now::of(&idle, Some(Signal::Ended)), Now::Ended);
        assert_eq!(Now::of(&dead("a"), Some(Signal::Busy)), Now::Ended);
    }

    /// A provisional permission prompt is not a question until its grace is
    /// up, and a stale working claim is not believed.
    #[test]
    fn a_report_is_believed_on_the_dashboards_terms() {
        let asked = Heard {
            signal: Signal::NeedsInput,
            at: 1000,
            provisional: true,
        };
        assert_eq!(asked.believed(1001), None);
        assert_eq!(asked.believed(1010), Some(Signal::NeedsInput));
        let busy = Heard {
            signal: Signal::Busy,
            at: 1000,
            provisional: false,
        };
        assert_eq!(busy.believed(1001), Some(Signal::Busy));
        assert_eq!(busy.believed(1000 + 3600), None);

        let mut slot = None;
        hear(&mut slot, busy);
        hear(&mut slot, Heard { at: 900, ..asked });
        assert_eq!(
            slot.map(|h| h.signal),
            Some(Signal::Busy),
            "an older report won"
        );
    }

    #[test]
    fn a_timeout_takes_a_unit() {
        assert_eq!(parse_timeout("90"), Ok(Duration::from_secs(90)));
        assert_eq!(parse_timeout("30s"), Ok(Duration::from_secs(30)));
        assert_eq!(parse_timeout("10m"), Ok(Duration::from_secs(600)));
        assert_eq!(parse_timeout("2h"), Ok(Duration::from_secs(7200)));
        assert_eq!(parse_timeout("1.5m"), Ok(Duration::from_secs(90)));
        assert_eq!(parse_timeout("0"), Ok(Duration::ZERO), "0 is no limit");
        assert!(parse_timeout("10d").is_err());
        assert!(parse_timeout("soon").is_err());
        assert!(parse_timeout("").is_err());
    }

    #[test]
    fn the_exit_code_says_how_it_ended() {
        let outcome = |met| Outcome {
            session_id: "abcdef123456".into(),
            provider: "claude",
            pid: Some(1),
            label: "proj".into(),
            state: Now::Idle,
            until: Until::AnyStop,
            met,
            waited: Duration::from_secs(3),
        };
        assert_eq!(outcome(Some(true)).exit_code(), 0);
        assert_eq!(outcome(Some(false)).exit_code(), 1);
        assert_eq!(outcome(None).exit_code(), 124);
        let json = outcome(Some(true)).json();
        assert_eq!(json["state"], "idle");
        assert_eq!(json["met"], true);
        assert_eq!(json["timed_out"], false);
        assert!(outcome(None).sentence().contains("gave up"));
    }
}
