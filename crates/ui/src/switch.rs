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
//! The footer's update button is the way in, beside `⧉ share`: `↑ v0.39.3`
//! while a release is out, which opens a tab running the update (sudo asks
//! for its password there, in plain sight); `⟳ v0.39.3 · switch` once it is
//! installed, which is `V`. A dashboard left open all day asks for releases
//! again every hour, so the button appears without a restart.
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

/// How often a running dashboard asks whether a release is out. The answer is
/// cached for as long by `update::cached_latest_version`, so asking sooner
/// would only read the same file.
const RECHECK: Duration = Duration::from_secs(60 * 60);

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
    /// When a release was last asked for, and the answer on its way. The
    /// first ask is the loop's own at startup, so this starts counting then.
    checked: Option<Instant>,
    checking: Option<Receiver<Option<String>>>,
    /// The tab the button opened to run the update, by its process: clicked
    /// again while it runs, the button goes back to it rather than starting a
    /// second update.
    pub updating: Option<u32>,
}

/// What the footer's update button says, and what a click on it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCorner {
    /// A release is out: a click opens a tab that installs it.
    Available(String),
    /// The tab installing it is open.
    Updating,
    /// It is installed under this cctop: a click switches to it.
    Installed(String),
}

impl UpdateCorner {
    pub fn label(&self) -> String {
        match self {
            Self::Available(version) => format!("↑ v{version}"),
            Self::Updating => "↑ updating…".to_string(),
            Self::Installed(version) => format!("⟳ v{version} · switch"),
        }
    }
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

    /// Ask again whether a release is out, once an hour, off the loop.
    /// Returns whether the button has something new to say.
    pub(super) fn tick_available(&mut self) -> bool {
        if let Some(rx) = &self.installed.checking {
            let Ok(found) = rx.try_recv() else {
                return false;
            };
            self.installed.checking = None;
            let changed = found != self.update_available;
            self.update_available = found;
            return changed;
        }
        let Some(at) = self.installed.checked else {
            self.installed.checked = Some(Instant::now());
            return false;
        };
        // A build cargo keeps the books on is told nothing — see the startup
        // check in `run`, which this repeats.
        if at.elapsed() < RECHECK || cctop_core::update::built_by_cargo() {
            return false;
        }
        self.installed.checked = Some(Instant::now());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(cctop_core::update::available_update());
        });
        self.installed.checking = Some(rx);
        false
    }

    /// The update button, when there is anything to update to.
    pub fn update_corner(&self) -> Option<UpdateCorner> {
        if let Some((_, version)) = &self.installed.found {
            return Some(UpdateCorner::Installed(version.clone()));
        }
        let version = self.update_available.clone()?;
        Some(match self.update_tab() {
            Some(_) => UpdateCorner::Updating,
            None => UpdateCorner::Available(version),
        })
    }

    /// The tab the button's update runs in, while it is still open.
    fn update_tab(&self) -> Option<usize> {
        self.tab_running(self.installed.updating?)
            .map(|(at, _)| at + 1)
    }

    /// A click on the update button.
    pub(super) fn on_update_corner(&mut self) {
        match self.update_corner() {
            Some(UpdateCorner::Installed(_)) => self.switch_to_installed(),
            Some(UpdateCorner::Updating) => {
                if let Some(tab) = self.update_tab() {
                    self.go_to_tab(tab);
                }
            }
            Some(UpdateCorner::Available(version)) => self.run_update(&version),
            None => {}
        }
    }

    /// Open a tab that runs `cctop --update`, under sudo when this user
    /// cannot write where the binary lives, and holds what it said until
    /// Enter. A tab rather than in the background because sudo wants a
    /// password typed somewhere, and because an update that failed should be
    /// read where it failed.
    fn run_update(&mut self, version: &str) {
        let Ok(exe) = std::env::current_exe() else {
            self.set_status("Could not find cctop's own binary to update");
            return;
        };
        let quoted = cctop_core::sandbox::sh_quote(&exe.to_string_lossy());
        let sudo = match exe.parent().is_some_and(writable) {
            true => "",
            false => "sudo ",
        };
        let argv = vec![
            "sh".to_string(),
            "-c".to_string(),
            update_script(sudo, &quoted),
        ];
        self.open_command_tab(&argv, &format!("the update to v{version}"), "update");
        if let Some(pid) = self
            .tabs
            .last()
            .and_then(|tab| tab.panes.first())
            .map(crate::tabs::Pane::agent)
        {
            self.installed.updating = Some(pid);
        }
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

/// The tab's script. Its messages sit in single quotes, so an apostrophe in
/// one ends the string early and `sh` refuses the whole line — which is how
/// the update button once failed in 51ms without running anything.
fn update_script(sudo: &str, quoted: &str) -> String {
    format!(
        "{sudo}{quoted} --update; s=$?; \
         if [ $s -eq 0 ]; then printf '\\nDone. If a new version went in, the update button now says switch (or press V).\\n'; \
         else printf '\\nThe update exited %s.\\n' \"$s\"; fi; \
         printf 'Press Enter to close this tab.\\n'; read _"
    )
}

/// Whether this user may create files in `dir`, which is what replacing the
/// binary in it takes.
fn writable(dir: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: a valid NUL-terminated path; access only reads it.
    unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_script_parses() {
        let script = update_script("sudo ", "'/usr/local/bin/cctop'");
        let ok = std::process::Command::new("sh")
            .args(["-n", "-c", &script])
            .status()
            .expect("sh");
        assert!(ok.success(), "{script}");
    }

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

    /// The button says what a click will do: install a release that is out,
    /// and once one is installed, switch to it — which wins, since installing
    /// again would only say it is done.
    #[test]
    fn the_button_offers_the_next_step() {
        let mut app = app();
        assert_eq!(app.update_corner(), None);
        app.update_available = Some("9.9.9".into());
        let available = app.update_corner().expect("a release");
        assert_eq!(available, UpdateCorner::Available("9.9.9".into()));
        assert_eq!(available.label(), "↑ v9.9.9");
        app.installed.found = Some((PathBuf::from("/usr/local/bin/cctop"), "9.9.9".into()));
        let installed = app.update_corner().expect("installed");
        assert_eq!(installed.label(), "⟳ v9.9.9 · switch");
        // An update tab that has closed is no longer what the button stands for.
        app.installed.found = None;
        app.installed.updating = Some(u32::MAX);
        assert_eq!(app.update_corner(), Some(available));
    }
}
