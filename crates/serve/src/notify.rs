//! The `--notify` webhook: a POST when a session starts needing a person.
//!
//! The page's Notify button covers a browser that is open; this covers the
//! ones that are not — a phone screen that is off, a chat webhook, a pager.
//! The trigger is the same edge the terminal bell uses, and is core's
//! ([`cctop_core::notify::crossings`]): a session *crossing into* waiting or
//! asking, never the level. A snapshot that finds a session already waiting is
//! not news — it is the state of the world when the server started — so the
//! first snapshot fires nothing, and a session that goes on sitting there
//! fires once, not once per refresh.
//!
//! Delivery is a spawned thread per event with a short deadline, because the
//! publish path it hangs off is the refresh loop: a webhook that never answers
//! must cost one stderr line and nothing else — not a stall, not a line per
//! refresh for as long as it stays dead.

use cctop_core::notify;
use cctop_core::session::Session;

/// Where `--notify` points, plus the link a payload's `url` is built on.
pub struct Webhook {
    /// The URL the POST goes to.
    target: String,
    /// The link this serve hands out — `origin/?t=token`, or the bare origin
    /// where no token belongs on it. The session link is built off it by
    /// [`cctop_core::notify::session_payload`], as the dashboard's webhook
    /// builds its own, so the two send one document; see `session_link` there
    /// for why the handed-out link and not an origin and token of its own.
    link_base: String,
}

impl Webhook {
    pub fn new(target: String, link_base: String) -> Webhook {
        Webhook { target, link_base }
    }

    /// The sessions in `after` that just started needing someone, as
    /// `(event, session)` pairs.
    ///
    /// A session that stopped is a crossing to core, but not one a webhook is
    /// sent for — see [`notify::Reason::event`].
    pub fn crossed<'a>(
        &self,
        before: &[Session],
        after: &'a [Session],
    ) -> Vec<(&'static str, &'a Session)> {
        notify::crossings(before, after)
            .into_iter()
            .filter_map(|(reason, session)| Some((reason.event()?, session)))
            .collect()
    }

    /// POST one crossing, on a thread of its own.
    ///
    /// The caller is the publish path, which the refresh runs on: nothing
    /// here may block it, so the send — and its deadline — happen off it.
    pub fn post(&self, event: &'static str, session: &Session) {
        notify::post_session(self.target.clone(), event, session, Some(&self.link_base));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cctop_core::pricing::Provider;
    use cctop_core::session::ActivityState;

    fn hook() -> Webhook {
        Webhook::new(
            "http://127.0.0.1:9/hook".into(),
            "http://127.0.0.1:7777/?t=abc123".into(),
        )
    }

    fn session(id: &str, running: bool, state: ActivityState) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.activity_state = state;
        if running {
            s.process = Some(cctop_core::proc::ProcInfo::default());
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
        let busy = vec![session("a", true, ActivityState::Working)];
        let waiting = vec![session("a", true, ActivityState::WaitingForInput)];
        // The process was gone, then the session reappeared already waiting —
        // nothing was watched working, so nothing crossed.
        assert!(hook.crossed(&stopped, &waiting).is_empty());
        assert!(hook.crossed(&[], &waiting).is_empty());
        // And stopping is the bell's news, not the webhook's.
        assert!(hook.crossed(&busy, &stopped).is_empty());
    }

    /// The link in the body is the handed-out link with the session's path in
    /// it, the same one the dashboard's webhook sends.
    #[test]
    fn the_payload_links_the_session_on_the_handed_out_link() {
        let s = session("s1", true, ActivityState::WaitingForInput);
        let body = notify::session_payload("waiting", &s, Some(&hook().link_base));
        assert_eq!(body["url"], "http://127.0.0.1:7777/session/s1?t=abc123");
        assert_eq!(body["event"], "waiting");
        assert_eq!(body["session_id"], "s1");
    }
}
