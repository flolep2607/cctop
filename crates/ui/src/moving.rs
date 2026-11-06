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
use cctop_core::migrate::send::{Outcome, Plan};
use cctop_core::migrate::{self, Kind};
use cctop_core::reach::Reach;
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
    pub machines: Vec<Machine>,
    pub cursor: usize,
    /// The machine chosen.
    pub host: Option<Reach>,
    /// What the check found, held through the confirmation.
    pub plan: Option<Box<Plan>>,
    /// The directory, editable in the confirmation.
    pub dir: super::line_edit::LineEdit,
}

/// One place a session could move to: a `--host` over ssh, or a sibling on
/// the Cloudflare account — or one of those that cannot be reached now, and
/// why, listed so it is not a mystery that it is missing.
#[derive(Clone)]
pub struct Machine {
    pub name: String,
    pub reach: Result<Reach, String>,
}

/// Every machine `X` could move a session to: the `--host`s, then the
/// siblings the account's last listing found, with why each one that cannot
/// be reached is not reachable.
fn machines(app: &App) -> Vec<Machine> {
    machines_from(
        &app.remote_hosts,
        cctop_core::peer::known(),
        cctop_core::peer::find,
    )
}

/// [`machines`] from what it is handed, so a test need not stand siblings in
/// on the process-wide listing every other test reads too.
fn machines_from(
    hosts: &[cctop_core::fleet::Host],
    siblings: Vec<(String, Option<String>)>,
    find: impl Fn(&str) -> Option<std::sync::Arc<cctop_core::peer::Peer>>,
) -> Vec<Machine> {
    let mut out: Vec<Machine> = hosts
        .iter()
        .map(|host| Machine {
            name: host.target.clone(),
            reach: Ok(Reach::Ssh(host.clone())),
        })
        .collect();
    for (name, why) in siblings {
        let reach = match (why, find(&name)) {
            (Some(why), _) => Err(why),
            (None, Some(peer)) => Ok(Reach::Peer(peer)),
            (None, None) => Err("no longer on the account".into()),
        };
        out.push(Machine { name, reach });
    }
    out
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
        if self.remote_hosts.is_empty() && cctop_core::peer::known().is_empty() {
            return Some(
                "no other machine to move it to — a session moves to a --host over ssh, \
                 or to another cctop on your Cloudflare account"
                    .into(),
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
        let machines = machines(self);
        if let [
            Machine {
                reach: Ok(reach), ..
            },
        ] = machines.as_slice()
        {
            let reach = reach.clone();
            return self.move_check(session, reach);
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
        let machine = self.moving.machines.get(self.moving.cursor).cloned();
        // One that cannot be reached stays in the list, saying why.
        if let Some(Machine {
            name,
            reach: Err(why),
        }) = &machine
        {
            return self.set_status(format!("{name} cannot take it now: {why}"));
        }
        match (self.moving.session.take(), machine) {
            (
                Some(session),
                Some(Machine {
                    reach: Ok(reach), ..
                }),
            ) => {
                self.mode = Mode::List;
                self.move_check(session, reach);
            }
            _ => self.move_cancel(),
        }
    }

    fn move_check(&mut self, session: Box<Session>, host: Reach) {
        self.set_status(format!("Asking {} where it would land…", host.machine()));
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
                self.set_status(format!("Asking {} about {typed}…", host.machine()));
                let _ = self.tx.send(Request::MoveReplan {
                    plan,
                    dir: typed,
                    host,
                });
            }
            false => {
                self.set_status(format!("Moving it to {}…", host.machine()));
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
pub(super) fn check(session: &Session, host: Reach) -> Event {
    let wire = migrate::wire::to(&host);
    match migrate::send::prepare(session, wire.as_ref()) {
        Ok(plan) => Event::Ready(Box::new(plan)),
        Err(why) => Event::Failed(why),
    }
}

pub(super) fn replan(mut plan: Plan, dir: &str, host: Reach) -> Event {
    let wire = migrate::wire::to(&host);
    match migrate::send::replan(&mut plan, dir, wire.as_ref()) {
        Ok(()) => Event::Ready(Box::new(plan)),
        Err(why) => Event::Failed(why),
    }
}

pub(super) fn run(plan: &Plan, host: Reach, send: &(dyn Fn(Event) + Sync)) -> Event {
    let wire = migrate::wire::to(&host);
    let say = |s: String| send(Event::Step(s));
    match migrate::send::run(plan, wire.as_ref(), &say) {
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

    /// The siblings are offered beside the `--host`s, and one that cannot be
    /// reached stays in the list saying why, rather than being asked.
    #[test]
    fn siblings_are_offered_and_an_unreachable_one_says_why() {
        let peer = std::sync::Arc::new(cctop_core::peer::Peer::fixed(
            "desk",
            "http://127.0.0.1:9",
            "s",
        ));
        let hosts = vec![cctop_core::fleet::Host::parse("procdb").unwrap()];
        let list = machines_from(
            &hosts,
            vec![
                ("desk".into(), None),
                ("laptop".into(), Some("offline".into())),
            ],
            |name| (name == "desk").then(|| peer.clone()),
        );
        let names: Vec<_> = list.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["procdb", "desk", "laptop"]);
        assert!(matches!(list[1].reach, Ok(Reach::Peer(_))));
        assert_eq!(
            list[2].reach.as_ref().err().map(String::as_str),
            Some("offline")
        );

        let mut app = test_app();
        app.moving = Flow {
            session: Some(Box::new(session("a", false, "/tmp"))),
            machines: list,
            cursor: 2,
            ..Flow::default()
        };
        app.mode = Mode::MovePick;
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::MovePick, "still choosing");
        assert!(!app.moving.busy);
        assert!(
            app.status()
                .unwrap()
                .contains("laptop cannot take it now: offline"),
            "{:?}",
            app.status()
        );
    }

    #[test]
    fn enter_moves_it_and_an_edited_directory_is_asked_about_first() {
        let mut app = test_app();
        app.moving.host = Some(Reach::Ssh(
            cctop_core::fleet::Host::parse("desktop").unwrap(),
        ));
        app.got_move(Event::Ready(Box::new(sample())));
        assert_eq!(app.mode, Mode::MoveConfirm);
        assert_eq!(app.moving.dir, "/home/me/code/cctop");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::List);
        assert!(app.moving.busy);
        assert!(app.moving.plan.is_none(), "handed to the worker");

        app.moving = Flow {
            host: Some(Reach::Ssh(
                cctop_core::fleet::Host::parse("desktop").unwrap(),
            )),
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
