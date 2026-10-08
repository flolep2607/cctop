//! What every face of cctop reads: the sessions on this machine and how they
//! were found, parsed, priced and cached, and the plumbing that reaches the
//! agents behind them — hooks, the shim, rmux, injection, the sandbox.
//!
//! The terminal dashboard (`cctop-ui`) and the web server (`cctop-serve`) are
//! built on this, and nothing here reaches up into either. Internal to cctop,
//! published only so that `cargo install cctop` can build: no API is promised
//! between versions.

pub mod access;
pub mod advise;
pub mod alert;
pub mod alias;
pub mod answer;
pub mod attach;
pub mod branch;
pub mod burn;
pub mod cache;
pub mod cast;
pub mod clipboard;
pub mod collide;
pub mod config;
pub mod convert;
pub mod elog;
pub mod embed;
pub mod fingerprint;
pub mod fleet;
pub mod hook;
pub mod inject;
pub mod insight;
pub mod json;
pub mod loader;
pub mod notify;
pub mod opencode;
pub mod peek;
pub mod pricing;
pub mod proc;
pub mod quota;
pub mod remote_fs;
pub mod rmux;
pub mod sandbox;
pub mod screen;
pub mod session;
pub mod settings;
pub mod shim;
pub mod ssh_config;
pub mod ssh_master;
pub mod sshfs;
pub mod trace;
pub mod tunnel;
pub mod update;
pub mod util;
pub mod watch;
pub mod yolo;
