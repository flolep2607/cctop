//! Switching a running dashboard to the cctop just installed under it, without
//! touching an agent.
//!
//! `cctop --update` replaces the file; the cctop already running goes on as
//! the version it started as, and only a quit and a fresh start brought the
//! new one in. Nothing in the dashboard needs that restart to be by hand: the
//! agents live in the daemon, and a cctop that starts reattaches every one of
//! them as a tab (see `restore_running_tabs`). So once a new binary is noticed
//! the footer offers `V`, and `V` is that quit and start done in place — the
//! UI shuts down as it does on quit, and `main` execs the new binary in this
//! terminal, which comes up on the tab this one was on.
//!
//! ponytail: what lives only in this process does not cross — a filter, a
//! scroll position, an open menu, a share tunnel the dashboard started.
//! Agents, tabs and their arrangement do, because they are the daemon's.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crate::App;

/// How often the loop looks for a new binary: a stat, so cheap, but nobody
/// is waiting on it to the second either.
const EVERY: Duration = Duration::from_secs(5);

/// What the switch was asked for: the new binary, and the tab to come back
/// on. Read by `main` once `run` has returned — see [`requested`].
static REQUESTED: Mutex<Option<(PathBuf, usize)>> = Mutex::new(None);

/// The switch `run` ended for, if it did: the binary to exec, and the tab the
/// new cctop should show.
pub fn requested() -> Option<(PathBuf, usize)> {
    REQUESTED.lock().ok()?.take()
}

/// Where the look for a new binary stands.
#[derive(Default)]
pub struct Installed {
    /// The new binary and its version, once both are known.
    pub found: Option<(PathBuf, String)>,
    /// The version being asked of a new binary, off the loop: `--version` is
    /// a process start, which the loop does not wait on.
    asking: Option<(PathBuf, Receiver<Option<String>>)>,
    looked: Option<Instant>,
}

impl App {
    /// Look for a binary installed over this one, when due. Returns whether
    /// the footer has something new to say.
    pub(super) fn tick_installed(&mut self) -> bool {
        if let Some((exe, rx)) = &self.installed.asking {
            let Ok(version) = rx.try_recv() else {
                return false;
            };
            let exe = exe.clone();
            self.installed.asking = None;
            // The same version again — a reinstall — is nothing to switch to.
            return match version.filter(|v| v != cctop_core::update::current_version()) {
                Some(version) => {
                    self.installed.found = Some((exe, version));
                    true
                }
                None => false,
            };
        }
        if self.installed.found.is_some()
            || self.installed.looked.is_some_and(|at| at.elapsed() < EVERY)
        {
            return false;
        }
        self.installed.looked = Some(Instant::now());
        if let Some(exe) = cctop_core::update::replaced() {
            let (tx, rx) = std::sync::mpsc::channel();
            let asked = exe.clone();
            std::thread::spawn(move || {
                let _ = tx.send(cctop_core::update::version_of(&asked));
            });
            self.installed.asking = Some((exe, rx));
        }
        false
    }

    /// `V`: quit into the new binary, if there is one to quit into.
    pub(super) fn switch_to_installed(&mut self) {
        let Some((exe, version)) = self.installed.found.clone() else {
            self.set_status("No newer cctop installed under this one");
            return;
        };
        // The agent this cctop hosts dies with it: that is what `cctop
        // claude` means. A switch must not be the thing that ends it.
        if self.hosted.is_some() {
            self.set_status(format!(
                "v{version} is installed — this cctop hosts an agent, so quit it and start cctop again"
            ));
            return;
        }
        if let Ok(mut slot) = REQUESTED.lock() {
            *slot = Some((exe, self.tab));
        }
        self.should_quit = true;
    }

    /// Put a switched-to cctop on the tab the old one was on, once its tabs
    /// are restored. Tabs come back in the arrangement they were left in, so
    /// the index is the same tab.
    pub(super) fn land_after_switch(&mut self) {
        let Some(tab) = std::env::var(cctop_core::update::SWITCHED_TAB)
            .ok()
            .and_then(|t| t.parse::<usize>().ok())
        else {
            return;
        };
        if tab <= self.tabs.len() {
            self.go_to_tab(tab);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new(
            cctop_core::pricing::Plan::Retail,
            std::sync::mpsc::channel().0,
        )
    }

    /// One test, because [`REQUESTED`] is one slot: with nothing installed `V`
    /// says so; a cctop hosting its agent will not switch, since the agent
    /// would end with it; otherwise it quits and leaves `main` where to go.
    #[test]
    fn v_switches_only_to_something_and_never_over_a_hosted_agent() {
        let mut nothing = app();
        nothing.switch_to_installed();
        assert!(!nothing.should_quit);
        assert!(requested().is_none());

        let new = (PathBuf::from("/usr/local/bin/cctop"), "9.9.9".to_string());
        let mut hosting = app();
        hosting.installed.found = Some(new.clone());
        hosting.hosted = Some((1, "claude".into()));
        hosting.switch_to_installed();
        assert!(!hosting.should_quit);
        assert!(requested().is_none());

        let mut free = app();
        free.installed.found = Some(new.clone());
        free.switch_to_installed();
        assert!(free.should_quit);
        assert_eq!(requested(), Some((new.0, 0)));
        assert!(requested().is_none(), "taken once");
    }
}
