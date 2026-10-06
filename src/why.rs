//! `cctop why` — why a session's row says what it says.
//!
//! Every other output is a verdict. The table shows a dot, `-j` shows
//! `"running": false`, and neither says how that was reached — so a row that is
//! wrong is a row with nothing to argue with. This prints the reasoning:
//! every agent process on the machine, the session each was given to, and the
//! rule that gave it.
//!
//! It exists because of one bug worth remembering. `claude --resume X` does not
//! reopen X; it forks, writing a new transcript that records X as the id it was
//! launched from. cctop read the command line literally, so the live agent was
//! attributed to a conversation nobody was in — the running session showed as
//! stopped and its CPU landed on a dead row. Diagnosing that took a process
//! table, `/proc/<pid>/cwd`, and a JSONL field nobody had documented. It should
//! have taken one command.
//!
//! Read-only: it walks sessions and processes and prints. Nothing here can
//! change a session, a file, or a process.

use crate::loader::Loader;
use crate::pricing::Plan;
use crate::session::Session;

pub const HELP: &str = "\
cctop why — why cctop thinks a session is running, or is not

USAGE:
  cctop why [SESSION_ID]

With no argument it explains every process it can see. With one, it explains
that session: whether a process was matched to it, which one, and by which rule
— and, when nothing was, what else claimed the processes nearby.

A session id may be shortened, as long as it is unambiguous.
";

/// The sessions `id` names, or the sentence explaining why it names none or too
/// many.
///
/// A prefix that matches two is refused rather than printed twice: this answers
/// "why does this row say so", and the answer for a prefix of somebody else's
/// session is a confident lie about the wrong one. The list is what makes the
/// refusal actionable — the same bargain [`crate::cli`] makes, since a prefix is
/// ambiguous only relative to sessions the user cannot see from here.
fn resolve<'a>(sessions: &'a [Session], id: &str) -> Result<Vec<&'a Session>, String> {
    let matched: Vec<_> = sessions
        .iter()
        .filter(|s| s.session_id.starts_with(id))
        .collect();
    match matched.as_slice() {
        [] => Err(format!(
            "No session here starts with {id}.\n`cctop -l` lists them; `cctop doctor` \
             says where they are read from."
        )),
        [only] => Ok(vec![*only]),
        many => Err(format!(
            "`{id}` matches {} sessions:\n{}",
            many.len(),
            many.iter()
                .map(|s| format!("  {} ({})", s.session_id, s.provider.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        )),
    }
}

pub fn run(argv: &[String]) -> i32 {
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return 0;
    }
    let wanted = argv.iter().find(|a| !a.starts_with('-'));

    let mut loader = Loader::new();
    // A full walk, because the question is about attribution and attribution is
    // done over every session on the machine — a subset would change the answer.
    let sessions = loader.load(Plan::Retail);
    let attributions = loader.attributions().to_vec();

    let matching: Vec<&Session> = match wanted {
        None => sessions.iter().collect(),
        Some(id) => match resolve(&sessions, id) {
            Ok(matched) => matched,
            Err(why) => {
                println!("{why}");
                return 1;
            }
        },
    };

    println!("{} agent process(es) attributed:", attributions.len());
    if attributions.is_empty() {
        println!("  (none — nothing on this machine looks like a running agent)");
    }
    for a in &attributions {
        println!("\n  pid {} → {}", a.pid, a.key);
        println!("    rule    {}", a.rule);
        if !a.matched.is_empty() {
            println!("    matched {}", a.matched);
        }
        if !a.argv.is_empty() {
            println!("    argv    {}", crate::util::truncate(&a.argv, 100));
        }
    }

    println!();
    for session in matching {
        let key = session.key();
        let mine: Vec<_> = attributions.iter().filter(|a| a.key == key).collect();
        let running = session.is_running();
        println!("{key}");
        println!(
            "  running   {}",
            match (running, session.process.is_some()) {
                (true, true) => "yes, a process was matched to it",
                // Cursor and Cowork have no per-session process; a growing
                // transcript is the only signal there is.
                (true, false) => "yes, inferred from a transcript still growing",
                (false, _) => "no",
            }
        );
        if session.launched_as() != session.session_id {
            // The half that explains most wrong answers.
            println!(
                "  resumed   this transcript was forked from {}, which is the id \
                 its process names",
                session.launched_as()
            );
        }
        println!("  last      {}", session.last_active);
        println!("  cwd       {}", session.label_source);
        for a in &mine {
            println!("  pid {}     {}", a.pid, a.rule);
        }
        if mine.is_empty() && !running {
            // Say what else took the processes in this directory, which is the
            // next question every time.
            let neighbours: Vec<_> = attributions
                .iter()
                .filter(|a| a.matched == session.label_source || a.key != key)
                .take(3)
                .collect();
            match neighbours.is_empty() {
                true => println!("  nothing on this machine looked like its process"),
                false => {
                    println!("  no process was matched to it. What took the others:");
                    for a in neighbours {
                        println!("    pid {} → {} ({})", a.pid, a.key, a.rule);
                    }
                }
            }
        }
        println!();
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    fn sessions() -> Vec<Session> {
        [
            ("11111111-aaaa-4000-8000-000000000001", Provider::Claude),
            ("22222222-bbbb-4000-8000-000000000002", Provider::Codex),
        ]
        .into_iter()
        .map(|(id, provider)| Session::new(provider, id.to_string()))
        .collect()
    }

    /// The help says a prefix is allowed, and it is — as long as it picks out
    /// one session, which is what the whole command is about: a confident
    /// explanation of somebody else's row is worse than no explanation.
    #[test]
    fn a_prefix_that_names_one_session_explains_it() {
        let sessions = sessions();
        let matched = resolve(&sessions, "2222").expect("an unambiguous prefix");
        assert_eq!(matched.len(), 1);
        assert_eq!(
            matched[0].session_id,
            "22222222-bbbb-4000-8000-000000000002"
        );
    }

    #[test]
    fn a_prefix_that_names_several_says_which_ones() {
        let sessions = [("abc-1", Provider::Claude), ("abc-2", Provider::Codex)]
            .into_iter()
            .map(|(id, provider)| Session::new(provider, id.to_string()))
            .collect::<Vec<_>>();
        let why = resolve(&sessions, "abc").expect_err("two sessions share the prefix");
        assert!(why.contains("2 sessions"), "{why}");
        // Both named, with their harness, so the user can lengthen the prefix.
        assert!(why.contains("abc-1 (claude)"), "{why}");
        assert!(why.contains("abc-2 (codex)"), "{why}");
    }

    #[test]
    fn a_prefix_that_names_nothing_says_where_to_look() {
        let why = resolve(&sessions(), "zzz").expect_err("no such session");
        assert!(why.contains("No session here starts with zzz"), "{why}");
        assert!(why.contains("doctor"), "{why}");
    }
}
