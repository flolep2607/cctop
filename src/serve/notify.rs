//! The `--notify` webhook: a POST when a session starts needing a person.
//!
//! The page's Notify button covers a browser that is open; this covers the
//! ones that are not — a phone screen that is off, a chat webhook, a pager.
//! The trigger is the same edge the terminal bell uses (see [`crate::notify`]):
//! a session *crossing into* waiting or asking, never the level. A snapshot
//! that finds a session already waiting is not news — it is the state of the
//! world when the server started — so the first snapshot fires nothing, and a
//! session that goes on sitting there fires once, not once per refresh.
//!
//! Delivery is a spawned thread per event with a short deadline, because the
//! publish path it hangs off is the refresh loop: a webhook that never answers
//! must cost one stderr line and nothing else — not a stall, not a line per
//! refresh for as long as it stays dead.

use crate::session::{ActivityState, Session};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// How long a notification POST is given before it is abandoned.
///
/// A webhook endpoint is someone's chat bridge or push relay; past a few
/// seconds it is down, and the refresh loop must not be made to wait on it.
const POST_TIMEOUT: Duration = Duration::from_secs(5);

/// Where `--notify` points, plus everything a payload's link is built from.
pub struct Webhook {
    /// The URL the POST goes to.
    target: String,
    /// The origin the session link is built on — the tunnel's when there is
    /// one, because a webhook that names `127.0.0.1` is a link that only works
    /// on the machine that sent it, and a webhook is for when you are not at
    /// that machine.
    origin: String,
    /// The full-access token, so the link in a notification can act on the
    /// session and not only look at it. The operator asked for the webhook;
    /// what it hears about needs the buttons.
    token: String,
    /// Set once a failed POST has been said. A dead endpoint is then silent:
    /// it was reported, and repeating the same line on every refresh is the
    /// spam this flag exists to prevent.
    warned: Arc<AtomicBool>,
}

/// What a session was doing at a snapshot, reduced to what the edge sees.
///
/// The same states [`crate::notify`] distinguishes: a session that is not
/// running is stopped whatever its last transcript line said, and an API
/// error is the agent's problem, not yet the user's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Busy,
    Waiting,
    Asking,
    Stopped,
}

fn state_of(session: &Session) -> State {
    if !session.is_running() {
        return State::Stopped;
    }
    match session.activity_state {
        ActivityState::Asking => State::Asking,
        ActivityState::WaitingForInput => State::Waiting,
        ActivityState::Working | ActivityState::ApiError => State::Busy,
    }
}

impl Webhook {
    pub fn new(target: String, origin: String, token: String) -> Webhook {
        Webhook {
            target,
            origin,
            token,
            warned: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The sessions in `after` that just started needing someone, as
    /// `(event, session)` pairs.
    ///
    /// Only a crossing out of `Busy` fires — the `crate::notify` rule, adopted
    /// here for the same reason: a permission prompt answered and the turn
    /// then ending is `Asking` to `Waiting`, which is the same news twice. A
    /// session absent from `before` has no edge at all, which is what makes
    /// the first snapshot silent.
    pub fn crossed<'a>(
        &self,
        before: &[Session],
        after: &'a [Session],
    ) -> Vec<(&'static str, &'a Session)> {
        let was: HashMap<String, State> = before.iter().map(|s| (s.key(), state_of(s))).collect();
        let mut out = Vec::new();
        for session in after {
            let event = match state_of(session) {
                State::Waiting => "waiting",
                State::Asking => "asking",
                State::Busy | State::Stopped => continue,
            };
            if was.get(&session.key()) == Some(&State::Busy) {
                out.push((event, session));
            }
        }
        out
    }

    /// POST one crossing, on a thread of its own.
    ///
    /// The caller is the publish path, which the refresh runs on: nothing
    /// here may block it, so the send — and its deadline — happen off it.
    pub fn post(&self, event: &'static str, session: &Session) {
        let link = format!("{}/session/{}", self.origin, session.session_id);
        let link = match self.token.is_empty() {
            true => link,
            false => format!("{link}?t={}", self.token),
        };
        let body = serde_json::json!({
            "event": event,
            "session_id": session.session_id,
            "project": session.label_source,
            "title": session.title,
            "url": link,
        })
        .to_string();
        let target = self.target.clone();
        let warned = Arc::clone(&self.warned);
        let session_id = session.session_id.clone();
        std::thread::spawn(move || {
            // `build()` yields the Config; the Agent is made from it, as in
            // `quota::agent`.
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(POST_TIMEOUT))
                .build()
                .into();
            let sent = agent
                .post(&target)
                .header("Content-Type", "application/json")
                .send(body.as_str());
            crate::elog::event(
                "notify",
                "post",
                serde_json::json!({
                    "event": event,
                    "session": session_id,
                    "ok": sent.is_ok(),
                    "status": sent.as_ref().map(|r| r.status().as_u16()).unwrap_or(0),
                }),
            );
            if let Err(why) = sent
                && !warned.swap(true, Ordering::Relaxed)
            {
                eprintln!("cctop: notify POST to {target} failed: {why}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    fn hook() -> Webhook {
        Webhook::new(
            "http://127.0.0.1:9/hook".into(),
            "http://127.0.0.1:7777".into(),
            "abc123".into(),
        )
    }

    fn session(id: &str, running: bool, state: ActivityState) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.activity_state = state;
        if running {
            s.process = Some(crate::proc::ProcInfo::default());
        }
        s
    }

    /// The whole point of the edge: one POST on the crossing, silence on every
    /// snapshot after it, however long the session sits there waiting.
    #[test]
    fn only_the_crossing_into_waiting_posts() {
        let hook = hook();
        let busy = vec![session("a", true, ActivityState::Working)];
        let waiting = vec![session("a", true, ActivityState::WaitingForInput)];

        assert!(hook.crossed(&[], &busy).is_empty());
        // A session first seen already waiting is not an edge: it may have
        // been waiting for a day before this server started.
        assert!(hook.crossed(&[], &waiting).is_empty());

        let crossed = hook.crossed(&busy, &waiting);
        assert_eq!(crossed.len(), 1);
        assert_eq!(crossed[0].0, "waiting");
        assert_eq!(crossed[0].1.session_id, "a");

        // Still waiting on the next snapshot: no second edge.
        assert!(hook.crossed(&waiting, &waiting).is_empty());
        // And back to busy, then waiting again, is a fresh crossing.
        assert_eq!(hook.crossed(&waiting, &busy).len(), 0);
        assert_eq!(hook.crossed(&busy, &waiting).len(), 1);
    }

    #[test]
    fn asking_is_its_own_event_but_the_same_turn_does_not_fire_twice() {
        let hook = hook();
        let busy = vec![session("a", true, ActivityState::Working)];
        let asking = vec![session("a", true, ActivityState::Asking)];
        let waiting = vec![session("a", true, ActivityState::WaitingForInput)];

        let crossed = hook.crossed(&busy, &asking);
        assert_eq!(crossed[0].0, "asking");
        // Asking → waiting is one turn reported once.
        assert!(hook.crossed(&asking, &waiting).is_empty());
    }

    #[test]
    fn a_stopped_or_absent_previous_state_is_not_an_edge() {
        let hook = hook();
        let stopped = vec![session("a", false, ActivityState::Working)];
        let waiting = vec![session("a", true, ActivityState::WaitingForInput)];
        // The process was gone, then the session reappeared already waiting —
        // nothing was watched working, so nothing crossed.
        assert!(hook.crossed(&stopped, &waiting).is_empty());
        assert!(hook.crossed(&[], &waiting).is_empty());
    }
}
