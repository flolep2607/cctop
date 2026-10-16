//! What an agent says it is doing, in its own words: the OSC 7501 Program
//! Status Protocol.
//!
//! Everything else cctop knows about a session's state is inference — a hook
//! event, a phrase on the screen, a screen that went still, the tail of a
//! transcript. A program that speaks this protocol states it outright:
//! `working`, `blocked` (on a permission, a question, or a sign-in), `done`,
//! `idle`, `error`. Claude Code does from 2.1.295, but only to a terminal that
//! answers its `ESC ] 7501 ; ?` query, and for an agent in a cctop tab that
//! terminal is cctop's rmux. rmux answers it from cctop-rmux 0.10.2 and keeps
//! each pane's report, which it hands out as pane formats — read here.
//!
//! So this is the first thing asked of a pane, ahead of the screen: where it
//! answers, it is right by construction, and a screen recognizer that falls
//! behind a harness's redesign no longer decides what the column says. Where it
//! does not — an agent too old to report, one that has not reported yet, a
//! daemon from before 0.10.2, which expands the formats to nothing — the
//! answer is `None` and the screen is read as before.
//!
//! Spec: <https://www.superlogical.com/rex/docs/build/program-status>.

use crate::hook::Signal;

/// The pane formats that carry the root record, tab-separated, in the order
/// [`parse`] reads them. Appended to a listing's own format, so the status
/// comes with the round trip that listing makes anyway.
pub const FORMAT: &str = "#{pane_program_state}\t#{pane_program_kind}\t#{pane_program_app}";

/// How many fields [`FORMAT`] adds to a listing line.
pub const FIELDS: usize = 3;

/// A pane's reported state, as the protocol names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
}

/// What a `blocked` program is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Permission,
    Question,
    Auth,
}

/// The root record of one pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    pub kind: Option<Kind>,
    /// The program's own identifier, `claude-code` — for whoever wants to
    /// tell agents apart without guessing from argv. Empty when not given.
    pub app: String,
}

/// Read [`FORMAT`]'s fields, or `None` when the pane has no record: an empty
/// state is what both "nothing reported" and "a daemon too old to know the
/// format" expand to, and the two mean the same here.
pub fn parse(fields: &[&str]) -> Option<Status> {
    let [state, kind, app] = fields else {
        return None;
    };
    let state = match state.trim() {
        "idle" => State::Idle,
        "working" => State::Working,
        "done" => State::Done,
        "blocked" => State::Blocked,
        "error" => State::Error,
        // Empty, or a state newer than this cctop: neither is a claim it
        // can act on, so the screen decides.
        _ => return None,
    };
    let kind = match kind.trim() {
        "permission" => Some(Kind::Permission),
        "question" => Some(Kind::Question),
        "auth" => Some(Kind::Auth),
        _ => None,
    };
    Some(Status {
        state,
        kind: kind.filter(|_| state == State::Blocked),
        app: app.trim().to_string(),
    })
}

impl Status {
    /// The state as the signal every other source speaks.
    ///
    /// `done` and `error` are both a turn that is over and a program waiting
    /// on its person: idle, as the transcript would have it. A failure is
    /// still told apart where it matters — the transcript records the API
    /// error, and the row reads that.
    pub fn signal(&self) -> Signal {
        match self.state {
            State::Working => Signal::Busy,
            State::Blocked => Signal::NeedsInput,
            State::Idle | State::Done | State::Error => Signal::Idle,
        }
    }

    /// Whether what it is blocked on is a question with an answer to pick,
    /// rather than a permission to grant.
    pub fn question(&self) -> bool {
        self.kind == Some(Kind::Question)
    }
}

/// The status of the rmux pane `target` (`session:window.pane`), in one
/// listing of its session. `None` when there is no daemon, no such pane, or
/// no record.
pub fn of_pane(target: &str) -> Option<Status> {
    let session = target.split(':').next()?;
    let format = format!("#{{session_name}}:#{{window_index}}.#{{pane_index}}\t{FORMAT}");
    let listing = crate::mux::list_panes(session, &format)?;
    listing.lines().find_map(|line| {
        let fields: Vec<&str> = line.split('\t').collect();
        let (pane, rest) = fields.split_first()?;
        (*pane == target).then(|| parse(rest)).flatten()
    })
}

/// The status of the first pane of rmux session `name` — a tab's agent.
pub fn of_session(name: &str) -> Option<Status> {
    let format = format!("#{{session_name}}\t{FORMAT}");
    let listing = crate::mux::list_panes(name, &format)?;
    listing.lines().find_map(|line| {
        let fields: Vec<&str> = line.split('\t').collect();
        let (session, rest) = fields.split_first()?;
        (*session == name).then(|| parse(rest)).flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five states read as the signals the rest of cctop speaks, and
    /// `kind` is kept only where the protocol gives it a meaning.
    #[test]
    fn each_state_reads_as_its_signal() {
        let read = |fields: &[&str]| parse(fields).map(|s| (s.signal(), s.question()));
        assert_eq!(
            read(&["working", "", "claude-code"]),
            Some((Signal::Busy, false))
        );
        assert_eq!(
            read(&["blocked", "permission", "claude-code"]),
            Some((Signal::NeedsInput, false))
        );
        assert_eq!(
            read(&["blocked", "question", "claude-code"]),
            Some((Signal::NeedsInput, true))
        );
        for done in ["idle", "done", "error"] {
            assert_eq!(read(&[done, "", ""]), Some((Signal::Idle, false)), "{done}");
        }
        // A kind on a state that has none is not carried.
        assert_eq!(parse(&["working", "question", ""]).unwrap().kind, None);
        assert_eq!(
            parse(&["idle", "", "claude-code"]).unwrap().app,
            "claude-code"
        );
    }

    /// No record, a daemon that does not know the formats, and a state from
    /// a newer protocol all say nothing, so the screen is read instead.
    #[test]
    fn nothing_reported_is_none() {
        assert_eq!(parse(&["", "", ""]), None);
        assert_eq!(parse(&["paused", "", "claude-code"]), None);
        assert_eq!(parse(&["working", ""]), None);
    }

    /// End to end on a real daemon: a program asks, as Claude Code does, and
    /// reports only once rmux has answered — so a status read back proves both
    /// that the daemon answers the query and that its report reaches cctop.
    #[test]
    fn a_program_that_asks_is_answered_and_its_report_is_read() {
        let _daemon = crate::mux::TestDaemon::new("program-status");
        let script = r"stty raw -echo
printf '\033]7501;?\033\\'
answer=$(dd bs=1 count=10 2>/dev/null)
case $answer in *7501*) printf '\033]7501;state=blocked:kind=question:app=probe\033\\' ;; esac
sleep 30";
        let argv = ["sh".to_string(), "-c".to_string(), script.to_string()];
        crate::mux::new_session("cctop-p", None, Vec::new(), &argv).expect("session made");
        let status = crate::test_wait::eventually("a report", || of_session("cctop-p"));
        assert_eq!(status.state, State::Blocked);
        assert!(status.question());
        assert_eq!(status.app, "probe");
        let _ = crate::mux::kill_session("cctop-p");
    }
}
