//! Events pushed from a live agent into a running cctop.
//!
//! Everything else cctop knows is forensic: it walks transcripts written after
//! the fact and re-derives state from them. That works, but it cannot see the
//! moments that matter most — an agent finishing its turn, an agent blocking on
//! a question, a session beginning or ending — because a transcript records
//! "answered you" and "still thinking" identically, and a session that has
//! ended looks exactly like one that is merely quiet. The agents can say all of
//! it outright: Claude Code and Codex through their hooks, Codex also through
//! its `notify` program.
//!
//! Two halves live here. [`emit`] is `cctop hook`, the command the agent spawns;
//! [`Listener`] is the socket a running UI holds open. The wire between them is
//! one JSON line per event.
//!
//! # The hook must never break the session
//!
//! This is the whole risk of the feature and it shapes every line of [`emit`].
//! An agent does not treat its hooks as observers: Claude Code reads the exit
//! code as a *decision*, where 2 blocks the tool call outright and feeds stderr
//! back to the model, and it reads stdout as content to act on. A monitoring
//! hook that fails loudly would break the coding session it was meant to watch,
//! and it would do it at the worst moment — mid tool call, on someone else's
//! machine, for a feature they only turned on to get a nicer notification.
//!
//! So `cctop hook` has one guarantee it keeps ahead of doing its job: it exits
//! 0, writes nothing to stdout that could be read as a decision, and returns
//! promptly. Not by being careful —
//! by construction. Every fallible step discards its error, the whole exchange
//! runs under a deadline on a thread the process is willing to abandon, and a
//! panic hook turns even an unexpected unwind into a silent success. cctop being
//! absent, stopped, or mid-crash is the *ordinary* case, not an error worth
//! reporting.
//!
//! # The one answer that is not silence
//!
//! Two events answer on stdout at all. `Stop` and `SubagentStop` get the
//! documented no-op `{"continue": true}` (see [`answer`](client::answer)). And, only with
//! `[settings] warn_agents = true`, a file write another live session has
//! recently made to the same file gets the harness's *context* channel —
//! `hookSpecificOutput.additionalContext`, and nothing beside it. That is the
//! one place stdout carries content, and it is shaped so it cannot be a
//! decision: no `permissionDecision`, no `decision`, no `continue`, so the
//! harness proceeds exactly as it would have with no output. It is computed on
//! the same abandonable thread under the same deadline, and any failure on the
//! way — a ledger that will not parse, a lock another hook holds, a deadline
//! that runs out — is the ordinary silence. [`crate::advise`] has the rest.
//!
//! # The one hook that decides, and why it is not this one
//!
//! YOLO answers Claude Code's permission dialog with a decision, and that
//! decision does not come from `cctop hook`. It comes from a second command,
//! `cctop yolo-hook` ([`yolo_hook`]), installed as its own entry for
//! `PermissionRequest` and, matched to `/yolo` alone, `UserPromptExpansion`,
//! so that nothing about this command — its arguments, its settings, the
//! state of any file — can make it decide. That one prints the allow for a
//! session YOLO is on for, in the process it was switched on in, and blocks
//! the expansion of a typed `/yolo` with the switch's answer; it is otherwise
//! exactly as silent as this one, under the same deadline and the same exit-0
//! guarantee. [`crate::yolo`] has the matching rule.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

mod client;
pub(crate) use client::ancestors_of;
mod health;
mod installers;
mod listener;
mod signal;

pub use client::*;
pub use health::*;
pub use installers::*;
pub use listener::*;
pub use signal::*;

#[cfg(test)]
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cctop-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
