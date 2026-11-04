//! One prompt, typed into every marked session.
//!
//! "Rebase on main and rerun the tests" across five worktrees is five `s`
//! boxes otherwise. This is that box once, delivered per target through the
//! same `Request::SendKeys` the single-row `s` uses — no second way into an
//! agent's keyboard, so whatever `inject` can reach, this reaches, and nothing
//! else.
//!
//! Unlike the batch delete it is not refused as a whole when one row cannot
//! take it: a prompt is not destructive, and the four sessions that can be
//! typed into should not wait on the one that ended. What it owes instead is
//! an account — which got it and which did not and why — and that account
//! cannot be written at Enter, because delivery happens on the worker and can
//! still fail there. So the sends carry a tag back, and the status line is
//! written when the last reply arrives.

use super::*;

/// Why a row cannot be typed into — core's, since an agent's `send_prompt`
/// over MCP asks the same question.
pub(super) use cctop_core::inject::Untypeable;

/// Marked rows the prompt can be typed into, as (name, pid).
pub(super) type Reachable = Vec<(String, u32)>;
/// Marked rows it cannot, as (name, why).
pub(super) type Refused = Vec<(String, Untypeable)>;

/// Which fan-out a worker reply belongs to, and the row it was for.
///
/// The id is what keeps two quick fan-outs apart: the worker answers in
/// order, but a reply to the first arriving after the second has started
/// must not be counted against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SendTag {
    pub id: u64,
    pub label: String,
}

/// A fan-out whose replies are still coming in.
#[derive(Debug, Default)]
pub(crate) struct Fanout {
    id: u64,
    waiting: usize,
    sent: usize,
    /// Every row that did not get it, with why: refused at Enter, then any
    /// the worker could not deliver to.
    missed: Vec<(String, String)>,
}

impl App {
    /// The marked sessions split into those the prompt can be typed into, as
    /// (name, pid), and those it cannot, as (name, why). In table order, so
    /// the box and the status line list them as the table does.
    pub(super) fn send_targets(&self) -> (Reachable, Refused) {
        let mut to = Reachable::new();
        let mut refused = Refused::new();
        for s in self.marked_sessions() {
            let name = s.display_label().to_string();
            match (cctop_core::inject::untypeable(s), s.root_pid()) {
                (None, Some(pid)) => match to.iter().find(|(_, p)| *p == pid) {
                    Some((first, _)) => {
                        refused.push((name, Untypeable::SameTerminal(first.clone())))
                    }
                    None => to.push((name, pid)),
                },
                (Some(why), _) => refused.push((name, why)),
                (None, None) => refused.push((name, Untypeable::NotLive)),
            }
        }
        (to, refused)
    }

    /// Open the send box over every marked session, or say why not.
    ///
    /// Empty rather than prefilled with `continue` like `s`: a prompt worth
    /// sending to five agents at once is a new instruction, not a nudge.
    pub(super) fn send_marked_prompt(&mut self) {
        if self.marked_sessions().is_empty() {
            self.set_status("No sessions marked — press Space to mark");
            return;
        }
        let (to, refused) = self.send_targets();
        if to.is_empty() {
            // Nothing to type into, so no box to type in: say why for each.
            self.set_status(format!(
                "None of the marked sessions can be typed into: {}",
                list_missed(refused.iter().map(|(n, w)| (n.as_str(), w.short())))
            ));
            return;
        }
        self.send_input = Default::default();
        self.send_to_marked = true;
        self.mode = Mode::SendKeys;
    }

    /// Type `text` into every marked session that can take it.
    ///
    /// Who can is decided again here rather than kept from when the box
    /// opened: an agent that ended while the prompt was being written is not
    /// live any more, and a send to its pid would reach nothing — or worse,
    /// whatever has the pid now.
    pub(super) fn send_marked(&mut self, text: String) {
        let (to, refused) = self.send_targets();
        let total = to.len() + refused.len();
        self.fanout_seq += 1;
        let id = self.fanout_seq;
        let mut fanout = Fanout {
            id,
            missed: refused
                .into_iter()
                .map(|(name, why)| (name, why.short()))
                .collect(),
            ..Default::default()
        };
        for (label, pid) in to {
            let tag = Some(SendTag {
                id,
                label: label.clone(),
            });
            let request = Request::SendKeys {
                pid,
                text: text.clone(),
                tag,
            };
            if self.tx.send(request).is_ok() {
                fanout.waiting += 1;
            } else {
                fanout.missed.push((label, "cctop's worker is gone".into()));
            }
        }
        let status = if fanout.waiting == 0 {
            format!(
                "Sent to none of the {total} marked: {}",
                list_missed(fanout.missed.iter().map(|(n, w)| (n.as_str(), w.clone())))
            )
        } else if fanout.missed.is_empty() {
            format!("Sending to {} sessions…", fanout.waiting)
        } else {
            format!(
                "Sending to {} of {total}… not to {}",
                fanout.waiting,
                list_missed(fanout.missed.iter().map(|(n, w)| (n.as_str(), w.clone())))
            )
        };
        self.fanout = (fanout.waiting > 0).then_some(fanout);
        self.set_status(status);
    }

    /// The worker's answer to one `SendKeys`.
    ///
    /// An untagged one is the single-row `s`, said as it always was. A tagged
    /// one is counted into its fan-out, which says the whole of what happened
    /// once the last of them is in.
    pub(super) fn keys_sent(&mut self, tag: Option<SendTag>, result: Result<(), String>) {
        let Some(tag) = tag else {
            return match result {
                Ok(()) => self.set_status("Sent to the session's terminal"),
                Err(error) => self.set_status(error),
            };
        };
        let Some(fanout) = self.fanout.as_mut().filter(|f| f.id == tag.id) else {
            // A fan-out that a newer one replaced; its account is gone.
            return;
        };
        fanout.waiting = fanout.waiting.saturating_sub(1);
        match result {
            Ok(()) => fanout.sent += 1,
            Err(error) => fanout.missed.push((tag.label, error)),
        }
        if fanout.waiting > 0 {
            return;
        }
        let Some(done) = self.fanout.take() else {
            return;
        };
        let total = done.sent + done.missed.len();
        self.set_status(if done.missed.is_empty() {
            format!("Sent to all {} sessions", done.sent)
        } else {
            format!(
                "Sent to {} of {total} — not to {}",
                done.sent,
                list_missed(done.missed.iter().map(|(n, w)| (n.as_str(), w.clone())))
            )
        });
    }
}

/// `a (not live), b (on procdb)`: the rows a prompt did not reach, each with
/// its reason, so nobody has to go looking for which of the five it was.
fn list_missed<'a>(missed: impl Iterator<Item = (&'a str, String)>) -> String {
    missed
        .map(|(name, why)| format!("{name} ({why})"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{session, test_app};
    use std::sync::mpsc::{Receiver, channel};

    /// A live session whose agent is `pid`, as the process scan would leave it.
    fn live(id: &str, label: &str, pid: u32) -> Session {
        let mut s = session(id, true, label);
        s.process.as_mut().unwrap().process_list = vec![cctop_core::proc::ProcEntry {
            pid,
            is_root: true,
            ghost: false,
            cpu: 0.0,
            memory: 0,
            args: String::new(),
        }];
        s
    }

    /// An app whose worker channel the test can read, with `sessions` all
    /// marked.
    fn marked_app(sessions: Vec<Session>) -> (App, Receiver<Request>) {
        let mut app = test_app();
        let (tx, rx) = channel();
        app.tx = tx;
        app.marked = sessions.iter().map(Session::key).collect();
        app.sessions = sessions;
        app.refilter();
        (app, rx)
    }

    /// What the worker was asked to do: (pid, text, label) per send.
    fn sends(rx: &Receiver<Request>) -> Vec<(u32, String, String)> {
        rx.try_iter()
            .filter_map(|r| match r {
                Request::SendKeys {
                    pid,
                    text,
                    tag: Some(tag),
                } => Some((pid, text, tag.label)),
                _ => None,
            })
            .collect()
    }

    fn remote(id: &str, label: &str, host: &str) -> Session {
        let mut s = live(id, label, 999);
        s.remote = Some(cctop_core::session::Remote {
            host: host.into(),
            ..Default::default()
        });
        s
    }

    #[test]
    fn a_prompt_reaches_every_live_marked_session_and_names_the_rest() {
        let (mut app, rx) = marked_app(vec![
            live("a", "api", 101),
            live("b", "web", 102),
            session("c", false, "docs"),
            remote("d", "infra", "procdb"),
        ]);
        app.send_marked_prompt();
        assert_eq!(app.mode, Mode::SendKeys);
        assert!(app.send_to_marked);

        app.send_marked("rebase on main".into());
        let mut got = sends(&rx);
        got.sort();
        assert_eq!(
            got,
            vec![
                (101, "rebase on main".into(), "api".into()),
                (102, "rebase on main".into(), "web".into()),
            ]
        );
        let status = app.status().unwrap_or_default().to_string();
        assert!(status.starts_with("Sending to 2 of 4"), "{status}");
        assert!(status.contains("docs (not live)"), "{status}");
        assert!(status.contains("infra (on procdb)"), "{status}");

        // The account is finished by the worker's replies, one of which failed.
        let tag = |label: &str| {
            Some(SendTag {
                id: app.fanout_seq,
                label: label.into(),
            })
        };
        let (api, web) = (tag("api"), tag("web"));
        app.keys_sent(api, Ok(()));
        assert!(app.fanout.is_some(), "still waiting on web");
        app.keys_sent(web, Err("no way into its terminal".into()));
        assert!(app.fanout.is_none());
        let status = app.status().unwrap_or_default().to_string();
        assert!(status.starts_with("Sent to 1 of 4"), "{status}");
        assert!(
            status.contains("web (no way into its terminal)"),
            "{status}"
        );
        assert!(status.contains("docs (not live)"), "{status}");
        // Marks stay: the next prompt to the same set is one key away.
        assert_eq!(app.marked.len(), 4);
    }

    /// Two rows on one agent process — a transcript the agent moved on from,
    /// still tied to the pid — would have the prompt typed twice into it.
    #[test]
    fn one_terminal_is_typed_into_once() {
        let (mut app, rx) = marked_app(vec![live("a", "api", 7), live("b", "api-old", 7)]);
        app.send_marked("go".into());
        assert_eq!(sends(&rx).len(), 1);
        let status = app.status().unwrap_or_default().to_string();
        assert!(status.contains("(same terminal as"), "{status}");
    }

    #[test]
    fn no_box_opens_when_nothing_marked_can_be_typed_into() {
        let (mut app, rx) = marked_app(vec![
            session("c", false, "docs"),
            remote("d", "infra", "procdb"),
        ]);
        app.send_marked_prompt();
        assert_eq!(app.mode, Mode::List);
        let status = app.status().unwrap_or_default().to_string();
        assert!(status.contains("docs (not live)"), "{status}");
        assert!(status.contains("infra (on procdb)"), "{status}");
        assert!(sends(&rx).is_empty());

        let (mut app, _rx) = marked_app(Vec::new());
        app.send_marked_prompt();
        assert_eq!(app.mode, Mode::List);
    }

    /// The fan-out is the single `s` per row, so it refuses what `s` refuses.
    #[test]
    fn the_fanout_refuses_what_the_single_send_refuses() {
        for s in [
            live("a", "api", 1),
            session("c", false, "docs"),
            remote("d", "infra", "procdb"),
        ] {
            let typeable = cctop_core::inject::untypeable(&s).is_none();
            let mut app = test_app();
            app.sessions = vec![s];
            app.refilter();
            app.selected = 0;
            app.send_prompt();
            assert_eq!(app.mode == Mode::SendKeys, typeable);
            assert!(!app.send_to_marked);
        }
    }

    /// A reply from a fan-out that a newer one replaced is not counted.
    #[test]
    fn a_stale_reply_does_not_finish_a_newer_fanout() {
        let (mut app, _rx) = marked_app(vec![live("a", "api", 1), live("b", "web", 2)]);
        app.send_marked("one".into());
        let first = app.fanout_seq;
        app.send_marked("two".into());
        app.keys_sent(
            Some(SendTag {
                id: first,
                label: "api".into(),
            }),
            Ok(()),
        );
        app.keys_sent(
            Some(SendTag {
                id: first,
                label: "web".into(),
            }),
            Ok(()),
        );
        assert!(app.fanout.is_some(), "the second is still waiting");
    }
}
