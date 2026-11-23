//! The coding agents on this machine, as cctop finds them.
//!
//! cctop reads the session files of every agent it knows — Claude Code, Codex,
//! Cursor, Devin, Gemini, OpenCode, Pi, Vibe, Windsurf, Antigravity — and
//! matches them to the processes running them. This crate is that part, on its
//! own, so a panel applet, a status bar or a notifier can ask the same question
//! without re-implementing a parser per agent and without linking the
//! terminal, web server, TLS and multiplexer that `cctop-core` carries.
//!
//! # What may change
//!
//! This is 0.x and is being moved out of `cctop-core` in steps (issue #430).
//! So far it holds the vocabulary — [`Provider`], [`Surface`],
//! [`ActivityState`], [`Permission`] — and not yet the parsers or the process
//! match. The names here are expected to stay; variants may be added to any
//! enum, so match them with a wildcard arm. A breaking change takes the next
//! 0.x minor, checked by `cargo-semver-checks` on every release.

mod provider;
mod state;

pub use provider::Provider;
pub use state::{ActivityState, Permission, Surface};
