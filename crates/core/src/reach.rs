//! How to reach the machine a remote row came from.
//!
//! A remote row arrives one of two ways: from a `--host` this run polls over
//! ssh, or from a sibling cctop on the same Cloudflare account ([`peer`]),
//! found with no host list at all. The row itself only carries the machine's
//! name, so anything that has to go back there for more — the conversation,
//! the report — first asks this module which of the two it is.
//!
//! The ssh host wins when a name is both: it is the one the user named, and
//! the one whose cctop answers every flag the reader asks for.
//!
//! Neither one is a refusal of its own: a machine that is neither has a reason
//! ([`Unreached`]), and that reason is what the user is shown, so it says which
//! of the two things it is not — or that it is a sibling but offline.

use std::fmt;
use std::sync::Arc;

use crate::chat::Conversation;
use crate::fleet::Host;
use crate::peer::{self, Peer};

/// The way back to a remote row's machine.
#[derive(Clone)]
pub enum Reach {
    /// A `--host`: cctop is run there over ssh.
    Ssh(Host),
    /// A sibling on the Cloudflare account: its serve is asked over its
    /// tunnel, signed as this machine.
    Peer(Arc<Peer>),
}

impl fmt::Debug for Reach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reach::Ssh(host) => f.debug_tuple("Ssh").field(&host.target).finish(),
            Reach::Peer(peer) => f.debug_tuple("Peer").field(&peer.machine).finish(),
        }
    }
}

/// Which of the two ways a tab here reaches the machine its agent is on —
/// the part of a [`Reach`] that outlives it.
///
/// A `Reach` holds a live sibling or an ssh spec and is found again from the
/// machine's name; a tab only needs to say which kind it went through, and has
/// to say it after a restart too, so this is what is written on the tab's rmux
/// session ([`rmux::set_transport`](crate::rmux::set_transport)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// A sibling on the Cloudflare account, bridged by `cctop mux reach` or
    /// `mux launch`.
    Peer,
    /// ssh to a `--host`, or a sandbox whose files are on one.
    Ssh,
}

impl Transport {
    /// The word written on the rmux session.
    pub fn as_str(self) -> &'static str {
        match self {
            Transport::Peer => "peer",
            Transport::Ssh => "ssh",
        }
    }

    /// Read back what [`as_str`](Self::as_str) wrote. Anything else is no
    /// transport, which draws as a local tab rather than as a guess.
    pub fn parse(word: &str) -> Option<Transport> {
        match word {
            "peer" => Some(Transport::Peer),
            "ssh" => Some(Transport::Ssh),
            _ => None,
        }
    }
}

/// Why a remote row's machine cannot be asked anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreached {
    /// A sibling on the account whose last poll failed. Asking again would
    /// wait out the same failure, so its reason is the answer.
    Offline { machine: String, why: String },
    /// Neither a `--host` nor a sibling — a spec dropped since the poll, or a
    /// sibling whose tunnel has gone from the account.
    Unknown { machine: String },
}

impl fmt::Display for Unreached {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unreached::Offline { machine, why } => {
                write!(f, "{machine} is not answering: {why}")
            }
            Unreached::Unknown { machine } => write!(
                f,
                "{machine} is neither a --host of this run nor a cctop on its Cloudflare account"
            ),
        }
    }
}

/// How to reach `machine`, given the ssh host this run has by that name.
///
/// The `Host` is passed in rather than looked up because the two callers keep
/// theirs differently — the TUI a list, the server a map — and the peers are
/// the only list both share.
pub fn reach(machine: &str, ssh: Option<&Host>) -> Result<Reach, Unreached> {
    if let Some(host) = ssh {
        return Ok(Reach::Ssh(host.clone()));
    }
    let Some(sibling) = peer::find(machine) else {
        return Err(Unreached::Unknown {
            machine: machine.to_string(),
        });
    };
    if let Some((_, why)) = peer::offline().into_iter().find(|(m, _)| m == machine) {
        return Err(Unreached::Offline {
            machine: machine.to_string(),
            why,
        });
    }
    Ok(Reach::Peer(sibling))
}

/// What [`Reach::via`] says for an ssh host — the one transport that runs a
/// command over there, which a line offering `--update` has to tell apart.
pub const OVER_SSH: &str = "over ssh";

impl Reach {
    /// The machine's name, as the HOST column shows it.
    pub fn machine(&self) -> &str {
        match self {
            Reach::Ssh(host) => &host.target,
            Reach::Peer(peer) => &peer.machine,
        }
    }

    /// Which kind of way this is, for a tab to remember.
    pub fn transport(&self) -> Transport {
        match self {
            Reach::Ssh(_) => Transport::Ssh,
            Reach::Peer(_) => Transport::Peer,
        }
    }

    /// How the answer travels, for a line that says what it is waiting on.
    pub fn via(&self) -> &'static str {
        match self {
            Reach::Ssh(_) => OVER_SSH,
            Reach::Peer(_) => "through its Cloudflare tunnel",
        }
    }

    /// A window of session `id`'s conversation, built by the cctop that holds
    /// the transcript: the same document a local read makes.
    ///
    /// `before` pages backwards and `agent` reads one subagent's own turns, as
    /// for a local read. Over ssh that is `cctop --chat`; from a sibling it is
    /// the `/api/chat/<id>` its own page reads — which also renders the
    /// replies to HTML, a field this side never looks at.
    pub fn chat(
        &self,
        id: &str,
        before: Option<usize>,
        agent: Option<&str>,
    ) -> Result<Conversation, String> {
        let json = match self {
            Reach::Ssh(host) => {
                let marker = before.map(|b| b.to_string());
                let mut args = vec!["--chat", id];
                if let Some(marker) = marker.as_deref() {
                    args.extend(["--before", marker]);
                }
                if let Some(agent) = agent {
                    args.extend(["--agent", agent]);
                }
                host.run(&args)?
            }
            Reach::Peer(peer) => {
                let mut query = Vec::new();
                if let Some(before) = before {
                    query.push(format!("before={before}"));
                }
                if let Some(agent) = agent {
                    query.push(format!("agent={}", encode(agent)));
                }
                let answer =
                    peer.send("GET", &format!("/api/chat/{id}"), &query.join("&"), None)?;
                if answer.status != 200 {
                    return Err(format!("{} {}", peer.machine, peer::refusal(&answer)));
                }
                String::from_utf8(answer.body)
                    .map_err(|_| format!("{}'s answer was not UTF-8", peer.machine))?
            }
        };
        serde_json::from_str(&json).map_err(|e| {
            format!(
                "{} returned an unreadable conversation: {e}",
                self.machine()
            )
        })
    }
}

/// A query value with everything but the unreserved characters escaped. An
/// agent id is one already, so this is the belt to the session's own check.
fn encode(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::peer::fake::{SECRET, sibling};

    /// The registry of siblings is one per process, so each test adds its own
    /// machine under a name nobody else uses rather than replacing the list.
    fn know(peer: Peer) -> Arc<Peer> {
        let peer = Arc::new(peer);
        peer::add_known(peer.clone());
        peer
    }

    #[test]
    fn a_siblings_row_reads_its_conversation_from_the_sibling() {
        let (url, seen) = sibling(
            200,
            r#"{"supported":true,"earlier":0,"turns":[{"seq":0,"role":"user","kind":"message","ts":"","text":"hello from the other machine","html":"<p>hello</p>","tools":[]}]}"#,
        );
        know(Peer::fixed("reach-test-desk", &url, SECRET));

        let reach = reach("reach-test-desk", None).expect("a known sibling is reachable");
        assert!(matches!(reach, Reach::Peer(_)), "{reach:?}");
        let conversation = reach
            .chat("sess-1", Some(4), Some("agent-x"))
            .expect("it answered");
        assert_eq!(conversation.turns[0].text, "hello from the other machine");
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            ["/api/chat/sess-1?before=4&agent=agent%2Dx"],
            "the page's own route, with the page's own parameters"
        );
    }

    #[test]
    fn a_siblings_refusal_is_said_with_its_name() {
        let (url, _) = sibling(404, "no such session");
        know(Peer::fixed("reach-test-gone", &url, SECRET));
        let why = reach("reach-test-gone", None)
            .unwrap()
            .chat("sess-1", None, None)
            .unwrap_err();
        assert_eq!(why, "reach-test-gone answered no such session");
    }

    #[test]
    fn an_offline_sibling_says_so_instead_of_being_asked() {
        let peer = Peer::fixed("reach-test-asleep", "http://127.0.0.1:9", SECRET)
            .failing("offline — its tunnel has no connector");
        know(peer);
        let why = reach("reach-test-asleep", None).unwrap_err();
        assert_eq!(
            why.to_string(),
            "reach-test-asleep is not answering: offline — its tunnel has no connector"
        );
    }

    #[test]
    fn a_machine_that_is_neither_says_which_it_is_not() {
        let why = reach("reach-test-nowhere", None).unwrap_err();
        assert_eq!(
            why,
            Unreached::Unknown {
                machine: "reach-test-nowhere".into()
            }
        );
        assert_eq!(
            why.to_string(),
            "reach-test-nowhere is neither a --host of this run nor a cctop on its Cloudflare account"
        );
    }

    #[test]
    fn an_ssh_host_wins_over_a_sibling_of_the_same_name() {
        know(Peer::fixed("reach-test-both", "http://127.0.0.1:9", SECRET));
        let host = Host::parse("reach-test-both").expect("a bare name is a host");
        let reach = reach("reach-test-both", Some(&host)).unwrap();
        assert!(matches!(reach, Reach::Ssh(_)), "{reach:?}");
        assert_eq!(reach.via(), "over ssh");
    }
}
