//! What every face of cctop reads: the sessions on this machine and how they
//! were found, parsed, priced and cached, the plumbing that reaches the agents
//! behind them — hooks, the shim, rmux, injection, the sandbox — and what the
//! faces show and do to a session: its conversation, report, export, handoff
//! brief, and the actions.
//!
//! The terminal dashboard (`cctop-ui`) and the web server (`cctop-serve`) are
//! built on this, and nothing here reaches up into either. Internal to cctop,
//! published only so that `cargo install cctop` can build: no API is promised
//! between versions.

use std::sync::atomic::{AtomicBool, Ordering};

pub mod access;
/// The things the page and the dashboard do to a session rather than say about
/// it: type at it, answer its prompt, resume it, hand it to another harness.
pub mod actions;
pub mod advise;
pub mod alert;
pub mod alias;
pub mod attach;
pub mod branch;
pub mod burn;
pub mod cache;
pub mod cast;
/// The conversation reader, which the dashboard's reader, the page, `--chat`
/// and the handoff brief all draw from.
pub mod chat;
pub mod clipboard;
pub mod collide;
pub mod config;
pub mod convert;
pub mod elog;
pub mod embed;
/// A whole conversation as markdown: `--export`, and `/api/chat/<id>/markdown`.
pub mod export;
pub mod fingerprint;
pub mod fleet;
/// The brief that hands a session's work to another harness.
pub mod handoff;
pub mod hook;
pub mod inject;
pub mod insight;
pub mod json;
pub mod loader;
pub mod notify;
pub mod opencode;
pub mod paste;
pub mod peek;
pub mod pricing;
pub mod proc;
pub mod quota;
pub mod remote_fs;
/// The per-session postmortem: `--report`, and `/api/report`.
pub mod report;
pub mod rmux;
pub mod sandbox;
pub mod screen;
pub mod session;
pub mod settings;
pub mod shim;
pub mod ssh_config;
pub mod ssh_master;
pub mod sshfs;
#[cfg(any(test, feature = "test-support"))]
pub mod test_wait;
pub mod trace;
pub mod tunnel;
pub mod update;
pub mod util;
pub mod watch;
pub mod yolo;

/// Set by the `cctop` binary before it does anything else.
static THE_BINARY: AtomicBool = AtomicBool::new(false);

/// Say that this process is cctop itself, not a test harness. The first thing
/// the binary's `main` does; nothing else calls it.
pub fn running_as_the_binary() {
    THE_BINARY.store(true, Ordering::Relaxed);
}

/// Whether the guards that keep a test off the real machine apply here: no bell
/// on stdout, no copy to the clipboard, no write over saved preferences, and a
/// runtime directory of the test's own rather than the one live dashboards
/// listen in.
///
/// `cfg(test)` alone answered this while cctop was one crate. It is true only
/// for this crate's own tests now, so the UI's and the server's tests turn on
/// the `test-support` feature instead. But features are unified across a
/// `cargo test` run, so the binary that run builds for `tests/` — and leaves
/// at `target/debug/cctop` — has the feature too, and must still behave as
/// cctop. Hence the second half: the binary says what it is before anything
/// can ask. Without the feature this is `cfg!(test)`, a constant.
pub fn under_test() -> bool {
    cfg!(test) || (cfg!(feature = "test-support") && !THE_BINARY.load(Ordering::Relaxed))
}
