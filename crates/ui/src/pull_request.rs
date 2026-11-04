//! `p`: land the selected session's work as a pull request.
//!
//! What is checked, pushed and opened, and why each refusal is a refusal, is
//! [`cctop_core::pull_request`]'s to say. This is the conversation around it:
//! the refusals that need nothing but the row, the worker's steps on the status
//! line as they happen, the question about uncommitted work, the title box, and
//! the URL on the clipboard at the end.
//!
//! # Uncommitted work is asked about, not refused
//!
//! An agent's checkout nearly always has something loose in it — a scratch
//! note, a lockfile it regenerated, the edit it was halfway through when it was
//! told to stop — and most of the time none of it belongs in the pull request.
//! Refusing until the checkout is clean would send the person off to tidy up
//! files they had already decided to leave out. Committing them would put in
//! the pull request what nobody chose to.
//!
//! So it asks, with the count, before the title box: the one thing that must
//! not happen is a pull request opened under the impression it holds work it
//! does not, and a question that says how many files are being left behind
//! rules that out without deciding for anyone.

use super::{App, Mode, worker::Request};
use cctop_core::pull_request::Plan;
use cctop_core::session::Session;

/// A title is one line on GitHub's page; this is room for a long one.
pub const TITLE_MAX: usize = 200;

/// Where a pull request for the dashboard stands.
#[derive(Default)]
pub struct Flow {
    /// The worker is checking or pushing; another `p` waits for it.
    pub busy: bool,
    /// What the check found, held through the question and the title box.
    pub plan: Option<Box<Plan>>,
    pub title: super::line_edit::LineEdit,
}

/// What the worker reports, in the order it happens.
pub enum Event {
    /// A step starting, for the status line.
    Step(String),
    /// The checks passed: what would be opened, and the title to offer.
    Ready(Box<Plan>, String),
    /// Pushed, and the pull request is at this URL; `existing` when it was
    /// already open and only the push was new.
    Opened {
        url: String,
        existing: bool,
    },
    Failed(String),
}

impl App {
    /// Why the selected row cannot have a pull request opened from here, said
    /// the same way by `p` and the row menu.
    pub(super) fn pull_request_refusal(&self, session: &Session) -> Option<String> {
        if let Some(why) = App::remote_refusal(session) {
            return Some(why);
        }
        if let Some(owner) = &session.owner {
            return Some(format!(
                "{owner}'s session — cctop never pushes another user's work"
            ));
        }
        if self.on_subagent() {
            return Some("open it for the session, not a subagent".into());
        }
        if session.sandbox.is_some() && !session.is_running() {
            return Some("its sandbox has stopped, so its checkout is not mounted here".into());
        }
        if session.work_dir().is_none() {
            return Some("its directory is gone from here".into());
        }
        if self.pull_request.busy {
            return Some("a pull request is already on its way".into());
        }
        None
    }

    /// `p`: check the selected session's branch on the worker, which answers
    /// with [`Event`]s.
    pub(super) fn pull_request_selected(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        if let Some(why) = self.pull_request_refusal(session) {
            // As written, the way the row menu says a refusal: some of them
            // start with a session's label or a user's name, neither of which
            // is anyone's to capitalise.
            self.set_status(why);
            return;
        }
        let Some(dir) = session.work_dir() else {
            return;
        };
        let title = session.title.clone();
        self.pull_request = Flow {
            busy: true,
            ..Flow::default()
        };
        let _ = self.tx.send(Request::PullRequestCheck { dir, title });
    }

    pub(super) fn got_pull_request(&mut self, event: Event) {
        match event {
            Event::Step(step) => self.set_status(step),
            Event::Failed(why) => {
                self.pull_request.busy = false;
                self.set_status(format!("No pull request: {why}"));
            }
            Event::Opened { url, existing } => {
                self.pull_request.busy = false;
                super::render::copy_to_clipboard(&url);
                self.set_status(match existing {
                    true => format!("Pushed; it already had a pull request, copied: {url}"),
                    false => format!("Pull request opened, URL copied: {url}"),
                });
            }
            Event::Ready(plan, title) => {
                self.pull_request.busy = false;
                // An open pull request still goes through the box, which then
                // offers the push to it rather than a title: `p` is one
                // unshifted letter, and nothing leaves the machine on a stray
                // keystroke.
                //
                // Only over the bare dashboard: a box that opened over another
                // modal, or under the keys going into a pane, would take
                // keystrokes meant for something else.
                if self.mode != Mode::List || self.tab != 0 {
                    self.set_status(format!(
                        "{} is ready for a pull request — p on the dashboard opens it",
                        plan.branch
                    ));
                    return;
                }
                self.pull_request.title = title.into();
                self.mode = match plan.uncommitted {
                    0 => Mode::PullRequestTitle,
                    _ => Mode::PullRequestUncommitted,
                };
                self.pull_request.plan = Some(plan);
                self.needs_redraw = true;
            }
        }
    }

    /// Enter in the title box: push and open.
    pub(super) fn pull_request_send(&mut self) {
        let title = self.pull_request.title.trim().to_string();
        let existing = self
            .pull_request
            .plan
            .as_ref()
            .is_some_and(|p| p.existing.is_some());
        if title.is_empty() && !existing {
            return;
        }
        self.mode = Mode::List;
        let Some(plan) = self.pull_request.plan.take() else {
            return;
        };
        let body = cctop_core::pull_request::body(&plan);
        self.pull_request.busy = true;
        let _ = self.tx.send(Request::PullRequestOpen { plan, title, body });
    }

    /// Leave the question or the box without opening anything.
    pub(super) fn pull_request_cancel(&mut self) {
        self.mode = Mode::List;
        self.pull_request = Flow::default();
        self.set_status("No pull request opened");
    }
}

/// The worker's half: [`cctop_core::pull_request::prepare`], each step sent as
/// it starts.
pub(super) fn check(
    dir: &std::path::Path,
    title: Option<&str>,
    gh: &str,
    send: &dyn Fn(Event),
) -> Event {
    let mut say = |s: String| send(Event::Step(s));
    match cctop_core::pull_request::prepare(dir, gh, &mut say) {
        Ok(plan) => {
            let title = cctop_core::pull_request::title(title, &plan);
            Event::Ready(Box::new(plan), title)
        }
        Err(why) => Event::Failed(why),
    }
}

/// The worker's other half: [`cctop_core::pull_request::open`].
pub(super) fn open(plan: &Plan, title: &str, body: &str, gh: &str, send: &dyn Fn(Event)) -> Event {
    let mut say = |s: String| send(Event::Step(s));
    match cctop_core::pull_request::open(plan, title, body, gh, &mut say) {
        Ok(url) => Event::Opened {
            url,
            existing: plan.existing.is_some(),
        },
        Err(why) => Event::Failed(why),
    }
}

/// What the title box and the question are drawn from in the snapshot.
#[cfg(test)]
pub(super) fn sample() -> Plan {
    use cctop_core::pull_request::FileStat;
    Plan {
        root: "/home/me/src/cctop/.claude/worktrees/fix-login".into(),
        branch: "fix-login".into(),
        base: "main".into(),
        repo: Some("github.com/me/cctop".into()),
        commits: vec!["Fix the login hint".into(), "Test the named account".into()],
        more_commits: 0,
        files: vec![FileStat {
            path: "src/login.rs".into(),
            added: Some(12),
            removed: Some(3),
        }],
        uncommitted: 3,
        existing: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{key, session, test_app};
    use ratatui::crossterm::event::KeyCode;

    #[test]
    fn a_remote_or_foreign_row_is_refused_before_the_worker_is_asked() {
        let mut app = test_app();
        let mut theirs = session("a", false, "/tmp");
        theirs.owner = Some("winshen".into());
        app.sessions = vec![theirs];
        app.refilter();
        app.pull_request_selected();
        assert!(!app.pull_request.busy);
        assert!(
            app.status().unwrap().contains("winshen"),
            "{:?}",
            app.status()
        );

        app.sessions[0].owner = None;
        app.sessions[0].remote = Some(cctop_core::session::Remote {
            host: "procdb".into(),
            ..Default::default()
        });
        app.refilter();
        app.pull_request_selected();
        assert!(!app.pull_request.busy);
        assert!(
            app.status().unwrap().contains("procdb"),
            "{:?}",
            app.status()
        );
    }

    #[test]
    fn uncommitted_work_is_asked_about_before_the_title() {
        let mut app = test_app();
        app.got_pull_request(Event::Ready(Box::new(sample()), "Fix the login".into()));
        assert_eq!(app.mode, Mode::PullRequestUncommitted);
        app.on_key(key(KeyCode::Char('y')));
        assert_eq!(app.mode, Mode::PullRequestTitle);
        assert_eq!(app.pull_request.title, "Fix the login");

        // Esc in the box leaves without opening anything.
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::List);
        assert!(app.pull_request.plan.is_none());

        // A clean checkout goes straight to the title, and `n` to the
        // question is a no.
        let mut clean = sample();
        clean.uncommitted = 0;
        app.got_pull_request(Event::Ready(Box::new(clean), "t".into()));
        assert_eq!(app.mode, Mode::PullRequestTitle);
        app.mode = Mode::List;
        app.got_pull_request(Event::Ready(Box::new(sample()), "t".into()));
        app.on_key(key(KeyCode::Char('n')));
        assert_eq!(app.mode, Mode::List);
    }

    #[test]
    fn enter_sends_the_edited_title_and_marks_the_flow_busy() {
        let mut app = test_app();
        let mut clean = sample();
        clean.uncommitted = 0;
        app.got_pull_request(Event::Ready(Box::new(clean), "Fix".into()));
        for c in " it".chars() {
            app.on_key(key(KeyCode::Char(c)));
        }
        assert_eq!(app.pull_request.title, "Fix it");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::List);
        assert!(app.pull_request.busy);
        assert!(app.pull_request.plan.is_none(), "handed to the worker");
    }
}
