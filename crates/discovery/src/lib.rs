//! The coding agents on this machine, as cctop finds them.
//!
//! cctop reads the session files of every agent it knows — Claude Code, Codex,
//! Cursor, Devin, Gemini CLI, OpenCode, Pi, Mistral Vibe, Windsurf and the
//! Antigravity CLI — and matches them to the processes running them. This
//! crate is that part, on its own, so a panel applet, a status bar or a
//! notifier can ask the same question without re-implementing a parser per
//! agent and without linking the terminal, web server, TLS and multiplexer
//! that `cctop-core` carries.
//!
//! ```no_run
//! for agent in cctop_discovery::agents() {
//!     let Some(pid) = agent.pid else { continue }; // running ones only
//!     println!(
//!         "{} {} pid {pid} {:?} in {:?}: {}",
//!         agent.provider.display_name(),
//!         agent.session_id,
//!         agent.state,
//!         agent.cwd,
//!         agent.title.as_deref().unwrap_or("(untitled)"),
//!     );
//! }
//! ```
//!
//! `examples/agents.rs` is the same, runnable, with a line per process and the
//! errors: `cargo run -p cctop-discovery --example agents`.
//!
//! # What it can and cannot say
//!
//! Everything comes from files the agents write and from `/proc`. It is Linux
//! only, reads and never writes, and opens another program's database
//! read-only.
//!
//! - **Which sessions exist** — every agent above, past and present.
//! - **Which are running** — Claude Code, Codex, OpenCode, Pi, Devin and Vibe
//!   are matched to their process. Cursor, Windsurf, Gemini CLI and Antigravity
//!   are not, so their `pid` is `None` even while they work.
//! - **What a running one is doing** — working, waiting for you, or stopped on
//!   an API error, from the newest records of its transcript. *Asking* (held on
//!   a permission prompt) mostly is not visible this way: the prompt leaves no
//!   record, so it reads as work in progress. cctop learns it from the agents'
//!   hooks, live, which this crate does not listen to.
//!
//! # Using it from a long-running program
//!
//! - **No runtime, no daemon.** [`agents`] is a plain blocking call: it reads
//!   files and `/proc` and returns. Run it on a worker thread. Nothing has to be
//!   installed or running — not cctop, not a hook.
//! - **When to call it.** [`watch_paths`] lists the directories the agents
//!   write to, and how deep under each the sessions are, for inotify. A write
//!   there is a reason to list again. A process starting or exiting writes
//!   nothing, so keep a slow timer as well, or a pidfd per process.
//! - **One session, several processes.** The same session can be open in two
//!   terminals — `claude --resume <id>` twice. It is still one [`Agent`], so
//!   `session_id` is a key that is unique in a listing; its
//!   [`processes`](Agent::processes) has one entry per terminal, and
//!   [`pid`](Agent::pid) is the first of them.
//! - **Finding the window.** Each [`AgentProcess`] carries its terminal
//!   (`/dev/pts/N`) and, when it was started in tmux, the pane id and the
//!   server's socket.
//! - **One broken agent.** [`scan`] is [`agents`] with the errors kept: an agent
//!   whose directory cannot be read, or whose files made the reader panic, is
//!   one [`ScanError`], and every other agent is listed as usual.
//!
//! # What may change
//!
//! This is 0.x. [`agents`], [`scan`], [`watch_paths`], the structs they return
//! and the enums are the interface, and are meant to stay. The structs are
//! `#[non_exhaustive]`, so a new field is not a breaking change. The enums are
//! exhaustive, so cctop's own matches stay checked, which makes a new variant
//! — a new agent, a new state — a breaking change: it takes the next 0.x
//! minor, as does any other break `cargo-semver-checks` finds on release. Pin
//! the minor you built against.
//!
//! The modules hidden from these docs are cctop-core's, public only because
//! core is another crate. Their shape follows cctop's needs and changes without
//! notice; do not build on them.

mod agents;
// Hidden, like every module below: public for cctop-core, which is another
// crate, and shaped by cctop's needs rather than by this crate's interface.
#[doc(hidden)]
pub mod mounts;
#[doc(hidden)]
pub mod paths;
#[doc(hidden)]
pub mod process;
mod provider;
#[doc(hidden)]
pub mod session;
mod state;
#[doc(hidden)]
pub mod time;
mod watch;
mod window;

pub use agents::{Agent, Scan, ScanError, agents, scan};
pub use provider::Provider;
pub use state::{ActivityState, Permission, Surface};
pub use watch::{WatchPath, watch_paths};
pub use window::{AgentProcess, TmuxPane};
