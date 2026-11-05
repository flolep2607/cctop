//! `X`: move the selected session to another machine.
//!
//! What moves, how, and every refusal is [`cctop_core::migrate`]'s to say.
//! This is the conversation around it: which machine (when there is more than
//! one to choose), what the far side proposes and the directory to change it,
//! the steps on the status line, and the answer.
//!
//! Nothing is frozen or sent until Enter on the confirmation: the check that
//! fills it in only asks questions, so it can sit open while the person reads.

use super::{App, Mode, worker::Request};
use cctop_core::fleet::Host;
use cctop_core::migrate::send::{Outcome, Plan};
use cctop_core::migrate::{self, Kind};
use cctop_core::session::Session;

/// A path is a line; this is room for a long one.
pub const DIR_MAX: usize = 400;

/// Where a move for the dashboard stands.
#[derive(Default)]
pub struct Flow {
    /// The worker is checking or moving; another `X` waits for it.
    pub busy: bool,
    /// The session being moved, held while a machine is picked.
    pub session: Option<Box<Session>>,
    /// The machines to pick from, and which one is highlighted.
    pub machines: Vec<Host>,
    pub cursor: usize,
    /// The machine chosen.
    pub host: Option<Host>,
    /// What the check found, held through the confirmation.
    pub plan: Option<Box<Plan>>,
    /// The directory, editable in the confirmation.
    pub dir: super::line_edit::LineEdit,
}

/// What the worker reports, in the order it happens.
pub enum Event {
    Step(String),
    /// The check passed: what would happen.
    Ready(Box<Plan>),
    Moved(Box<Outcome>),
    Failed(String),
}

impl App {
    /// Why the selected row cannot move from here, said the same way by `X`
    /// and the row menu.
    pub(super) fn move_refusal(&self, session: &Session) -> Option<String> {
        if self.on_subagent() {
            return Some("move the session, not a subagent".into());
        }
        if let Err(why) = migrate::movable(session) {
            return Some(why);
        }
        if let Some(moved) = migrate::moved_to(session.provider, &session.session_id) {
            return Some(migrate::moved_refusal(&moved));
        }
        if self.remote_hosts.is_empty() {
            return Some(
                "no other machine to move it to — a session moves over ssh to a --host".into(),
            );
        }
        if self.moving.busy {
            return Some("a move is already on its way".into());
        }
        None
    }

    /// `X`: pick the machine, or go straight to the check when there is one.
    pub(super) fn move_selected(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        if let Some(why) = self.move_refusal(session) {
            self.set_status(format!("Cannot move it: {why}"));
            return;
        }
        let session = Box::new(session.clone());
        let machines = self.remote_hosts.clone();
        if machines.len() == 1 {
            let host = machines[0].clone();
            return self.move_check(session, host);
        }
        self.moving = Flow {
            session: Some(session),
            machines,
            ..Flow::default()
        };
        self.mode = Mode::MovePick;
    }

    /// Enter in the machine list.
    pub(super) fn move_pick(&mut self) {
        let host = self.moving.machines.get(self.moving.cursor).cloned();
        match (self.moving.session.take(), host) {
            (Some(session), Some(host)) => {
                self.mode = Mode::List;
                self.move_check(session, host);
            }
            _ => self.move_cancel(),
        }
    }

    fn move_check(&mut self, session: Box<Session>, host: Host) {
        self.set_status(format!("Asking {} where it would land…", host.target));
        self.moving = Flow {
            busy: true,
            host: Some(host.clone()),
            ..Flow::default()
        };
        let _ = self.tx.send(Request::MoveCheck { session, host });
    }

    pub(super) fn got_move(&mut self, event: Event) {
        match event {
            Event::Step(step) => self.set_status(step),
            Event::Failed(why) => {
                self.moving.busy = false;
                self.set_status(format!("Not moved: {why}"));
            }
            Event::Moved(outcome) => {
                self.moving = Flow::default();
                let mut line = format!(
                    "Moved — running on {} as {}, in {}; stopped here",
                    outcome.machine, outcome.receipt.session_id, outcome.receipt.dir
                );
                for note in [&outcome.receipt.note, &outcome.note].into_iter().flatten() {
                    line.push_str(&format!(" ({note})"));
                }
                self.set_status(line);
            }
            Event::Ready(plan) => {
                self.moving.busy = false;
                // Only over the bare dashboard, for the reason `p` gives: a
                // box that opened over another modal would take its keys.
                if self.mode != Mode::List || self.tab != 0 {
                    self.set_status(format!(
                        "{} can take it — X on the dashboard moves it",
                        plan.machine
                    ));
                    return;
                }
                self.moving.dir = plan.placement.dir.clone().into();
                self.moving.plan = Some(plan);
                self.mode = Mode::MoveConfirm;
                self.needs_redraw = true;
            }
        }
    }

    /// Enter on the confirmation: move it — or, when the directory was
    /// edited, ask the far side about that one first.
    pub(super) fn move_confirm(&mut self) {
        let Some(plan) = self.moving.plan.take() else {
            return self.move_cancel();
        };
        let Some(host) = self.moving.host.clone() else {
            return self.move_cancel();
        };
        let typed = self.moving.dir.trim().to_string();
        self.mode = Mode::List;
        self.moving.busy = true;
        match typed != plan.placement.dir && !typed.is_empty() {
            true => {
                self.set_status(format!("Asking {} about {typed}…", host.target));
                let _ = self.tx.send(Request::MoveReplan {
                    plan,
                    dir: typed,
                    host,
                });
            }
            false => {
                self.set_status(format!("Moving it to {}…", host.target));
                let _ = self.tx.send(Request::MoveRun { plan, host });
            }
        }
    }

    /// Leave the list or the confirmation without moving anything.
    pub(super) fn move_cancel(&mut self) {
        self.mode = Mode::List;
        self.moving = Flow::default();
        self.set_status("Nothing moved");
    }
}

/// One line saying how the files would land.
pub(crate) fn landing(plan: &Plan) -> String {
    let p = &plan.placement;
    match p.kind {
        Kind::Existing => format!("into the checkout at {}", p.dir),
        Kind::Worktree => format!("into a new worktree at {}", p.dir),
        Kind::Clone => format!("into a new clone at {}", p.dir),
    }
}

/// The worker's halves.
pub(super) fn check(session: &Session, host: Host) -> Event {
    let wire = migrate::wire::Ssh(host);
    match migrate::send::prepare(session, &wire) {
        Ok(plan) => Event::Ready(Box::new(plan)),
        Err(why) => Event::Failed(why),
    }
}

pub(super) fn replan(mut plan: Plan, dir: &str, host: Host) -> Event {
    let wire = migrate::wire::Ssh(host);
    match migrate::send::replan(&mut plan, dir, &wire) {
        Ok(()) => Event::Ready(Box::new(plan)),
        Err(why) => Event::Failed(why),
    }
}

pub(super) fn run(plan: &Plan, host: Host, send: &(dyn Fn(Event) + Sync)) -> Event {
    let wire = migrate::wire::Ssh(host);
    let say = |s: String| send(Event::Step(s));
    match migrate::send::run(plan, &wire, &say) {
        Ok(outcome) => Event::Moved(Box::new(outcome)),
        Err(why) => Event::Failed(why),
    }
}

/// What the confirmation is drawn from in the snapshot.
#[cfg(test)]
pub(super) fn sample() -> Plan {
    Plan {
        harness: migrate::Harness::Claude,
        session_id: "0b4e8a54-8d39-4f4e-8a39-6b8a3c1c2d01".into(),
        title: "fix the login hint".into(),
        cwd: "/home/me/src/cctop".into(),
        transcript: "/home/me/.claude/projects/-home-me-src-cctop/x.jsonl".into(),
        pid: Some(4242),
        repo: migrate::Repo {
            origin: Some("git@github.com:me/cctop.git".into()),
            key: "github.com/me/cctop".into(),
            name: "cctop".into(),
            head: "abc123".into(),
            branch: Some("fix-login".into()),
            rel_cwd: String::new(),
        },
        placement: migrate::Placement {
            kind: Kind::Existing,
            dir: "/home/me/code/cctop".into(),
            root: None,
            has_head: true,
            note: None,
        },
        machine: "desktop".into(),
        size_hint: 3 * 1024 * 1024,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{key, session, test_app};
    use ratatui::crossterm::event::KeyCode;

    #[test]
    fn with_no_other_machine_x_says_how_to_have_one() {
        let mut app = test_app();
        app.sessions = vec![session("a", false, "/tmp")];
        app.refilter();
        app.move_selected();
        assert!(!app.moving.busy);
        assert_eq!(app.mode, Mode::List);
        assert!(app.status().is_some());
    }

    #[test]
    fn enter_moves_it_and_an_edited_directory_is_asked_about_first() {
        let mut app = test_app();
        app.moving.host = Some(Host::parse("desktop").unwrap());
        app.got_move(Event::Ready(Box::new(sample())));
        assert_eq!(app.mode, Mode::MoveConfirm);
        assert_eq!(app.moving.dir, "/home/me/code/cctop");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::List);
        assert!(app.moving.busy);
        assert!(app.moving.plan.is_none(), "handed to the worker");

        app.moving = Flow {
            host: Some(Host::parse("desktop").unwrap()),
            ..Flow::default()
        };
        app.got_move(Event::Ready(Box::new(sample())));
        for c in "-2".chars() {
            app.on_key(key(KeyCode::Char(c)));
        }
        app.on_key(key(KeyCode::Enter));
        assert!(
            app.status().unwrap().contains("cctop-2"),
            "{:?}",
            app.status()
        );

        // Esc leaves without moving anything.
        app.moving = Flow::default();
        app.got_move(Event::Ready(Box::new(sample())));
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::List);
        assert!(app.moving.plan.is_none());
    }
}
