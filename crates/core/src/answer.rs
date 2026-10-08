//! Answering the permission prompt a session is holding.
//!
//! One of the page's actions (`serve::actions`), and the one thing
//! [`crate::yolo`] does on its own, with no page and no server — so it sits
//! below both, with the result types every action shares.

use crate::session::Session;
use serde::Serialize;

/// What happened, in the shape the page renders.
#[derive(Debug, Serialize)]
pub struct Done {
    /// One sentence for the reader, whether it worked or not.
    pub message: String,
    /// The rmux session an action started or found, when there is one, so the
    /// answer can tell someone at a terminal where their agent went.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rmux: Option<String>,
}

/// Every failure is a sentence and a status, because the page shows the sentence
/// and the browser needs the status.
pub type Failed = (u16, String);

pub fn done(message: impl Into<String>) -> Result<Done, Failed> {
    Ok(Done {
        message: message.into(),
        rmux: None,
    })
}

/// Press the key that answers the prompt this session is holding.
///
/// Not a line of text — the prior version of this sent the words "yes" and
/// "no" to a permission prompt, and Claude Code reads Enter in one as picking
/// the highlighted option — which is "Yes". So "No" allowed the tool. Each
/// harness is pressed the key its own menu names, and only when the row says
/// it is asking: a `1` typed into a composer is a stray character in someone's
/// next prompt, not an answer to anything.
///
/// Deny is Esc everywhere, because it is the one key both menus bind to "no"
/// whatever else they list — the number of the "No" option moves as the menu
/// grows ("always allow", "switch to auto mode"). Allow is the first option,
/// which is "Yes" in both and the only one that never widens what is allowed.
pub fn answer(session: &Session, choice: &str) -> Result<Done, Failed> {
    use crate::inject::Key;
    local(session)?;
    let allow = match choice {
        "allow" => true,
        "deny" => false,
        _ => return Err((400, "an answer is `allow` or `deny`".into())),
    };
    if session.activity_state != crate::session::ActivityState::Asking {
        return Err((409, "this session is not asking anything right now".into()));
    }
    // A question with choices is not a permission prompt: `1` there picks the
    // first answer and Esc throws the question away. Refused here as well as
    // never offered by the page, since a page from before this knew no better.
    if session.asking_question {
        return Err((
            409,
            "this is a question with choices, not a permission prompt — answer it in its terminal"
                .into(),
        ));
    }
    // ponytail: Claude Code and Codex only, the two whose menus were driven and
    // checked. Gemini and OpenCode report prompts too, but pressing a guessed
    // key at a menu that means something else by it is how "No" came to allow.
    let key = match (session.provider, allow) {
        (crate::pricing::Provider::Claude, true) => Key::Char('1'),
        (crate::pricing::Provider::Codex, true) => Key::Char('y'),
        (crate::pricing::Provider::Claude | crate::pricing::Provider::Codex, false) => Key::Escape,
        (other, _) => {
            return Err((
                409,
                format!(
                    "cctop does not know how {} answers a prompt — answer it in its terminal",
                    other.as_str()
                ),
            ));
        }
    };
    let Some(pid) = session.root_pid() else {
        return Err((
            409,
            "nothing is running this session — resume it first".into(),
        ));
    };
    match crate::inject::press(pid, key) {
        Ok(()) => {
            // Every other cctop watching this session hears about the answer
            // over the hook socket and stops asking — the agent's own next
            // event is a while coming, and a denied turn sends none at all.
            crate::hook::announce_answer(&session.session_id, allow);
            done(if allow { "Allowed" } else { "Denied" })
        }
        Err(why) => Err((409, why)),
    }
}

/// Refuse a row that came from another machine.
///
/// Every action below signals a pid, opens a directory or writes a file, and all
/// three are about *this* filesystem. See [`crate::session::Remote`].
pub fn local(session: &Session) -> Result<(), Failed> {
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
