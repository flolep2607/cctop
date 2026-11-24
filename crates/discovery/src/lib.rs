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
//! [`ActivityState`], [`Permission`] — and the first of the transcript line
//! parsers, under [`session`]; not yet the per-agent readers or the process
//! match. The names here are expected to stay. The enums are exhaustive, so
//! cctop's own matches stay checked, which makes a new variant — a new agent,
//! a new state — a breaking change: it takes the next 0.x minor, as does any
//! other break `cargo-semver-checks` finds on release. Pin the minor you
//! built against.

// Where each agent's files are. Public for cctop-core; hidden because its
// shape is cctop's, not the interface this crate offers.
#[doc(hidden)]
pub mod paths;
mod provider;
pub mod session;
mod state;
#[doc(hidden)]
pub mod time;

pub use provider::Provider;
pub use state::{ActivityState, Permission, Surface};
