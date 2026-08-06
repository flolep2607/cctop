//! Workspace tabs: the dashboard, plus a terminal for every agent you open.
//!
//! Nothing here is new machinery. [`shim::host`](crate::shim::host) already puts
//! an agent on a pty cctop owns, and [`attach`](crate::attach) already turns that
//! pty into a screen that can be drawn into any rectangle and resized to it. A
//! tab is a list of those screens; a split is that list drawn side by side
//! instead of one at a time.
//!
//! The dashboard is not in this list — it is tab zero and always there, so
//! [`App::tab`](super::App::tab) is `0` for the session table and `1..=len` for
//! these.

use std::path::Path;

/// One terminal on screen.
pub struct Pane {
    /// The agent this pane started. Dropping it kills the agent, which is why a
    /// pane merely *looking at* someone else's session leaves this `None`: `a`
    /// on a session row must not make cctop responsible for its life.
    hosted: Option<crate::shim::Hosted>,
    /// The agent's pid. A second request to view the same agent finds this pane
    /// rather than opening a duplicate onto one terminal.
    pub pid: u32,
    pub label: String,
    pub view: crate::attach::Attach,
}

impl Pane {
    /// Start `argv` on a pty of cctop's and open a pane onto it.
    pub fn launch(argv: &[String], cwd: Option<&Path>) -> anyhow::Result<Pane> {
        let hosted = crate::shim::host(argv, cwd)?;
        // The shim binds and serves the socket before returning, so there is
        // something to connect to even though the agent has drawn nothing yet.
        let view = crate::attach::attach(hosted.pid).ok_or_else(|| {
            anyhow::anyhow!(
                "{} started but its terminal could not be opened",
                hosted.label
            )
        })?;
        Ok(Pane {
            pid: hosted.pid,
            label: hosted.label.clone(),
            view,
            hosted: Some(hosted),
        })
    }

    /// Open a pane onto an agent something else is responsible for.
    pub fn view_of(pid: u32, label: String) -> Option<Pane> {
        Some(Pane {
            hosted: None,
            pid,
            label,
            view: crate::attach::attach(pid)?,
        })
    }

    /// Whether the agent behind this pane has gone.
    fn finished(&mut self) -> bool {
        match self.hosted.as_mut() {
            Some(hosted) => hosted.finished().is_some(),
            // Nothing here owns the process, so the connection is the only
            // evidence there is: a shim that exited closes it.
            None => self.view.closed(),
        }
    }
}

/// One workspace tab: the panes shown together, and which of them has the
/// keyboard.
pub struct Tab {
    pub panes: Vec<Pane>,
    pub focus: usize,
    /// Panes stacked top to bottom rather than laid out left to right.
    ///
    /// One direction for the whole tab. A split *tree* would let every pane
    /// divide differently, and nothing has needed that yet — this covers the
    /// side-by-side and the stacked case with a bool.
    pub stacked: bool,
}

impl Tab {
    pub fn new(pane: Pane) -> Tab {
        Tab {
            panes: vec![pane],
            focus: 0,
            stacked: false,
        }
    }

    /// What the tab bar calls this tab.
    pub fn title(&self) -> String {
        match self.panes.len() {
            0 | 1 => self
                .panes
                .first()
                .map(|p| p.label.clone())
                .unwrap_or_default(),
            n => format!("{} +{}", self.panes[0].label, n - 1),
        }
    }

    pub fn focused_mut(&mut self) -> Option<&mut Pane> {
        self.panes.get_mut(self.focus)
    }

    /// Move the keyboard to the next pane, wrapping.
    pub fn cycle_focus(&mut self) {
        if !self.panes.is_empty() {
            self.focus = (self.focus + 1) % self.panes.len();
        }
    }

    /// Fold in whatever the agents have drawn. True when anything changed.
    ///
    /// Every pane is pumped, not just the focused one: the shim's output has to
    /// be read whether or not it is on screen, and a background pane whose
    /// buffer filled up would stall the agent behind it.
    pub fn pump(&mut self) -> bool {
        self.panes.iter_mut().fold(false, |changed, pane| {
            // Not `||`: that short-circuits, and every pane must be drained.
            pane.view.pump() | changed
        })
    }

    /// Drop the panes whose agents have exited. True once nothing is left.
    pub fn reap(&mut self) -> bool {
        self.panes.retain_mut(|pane| !pane.finished());
        self.focus = self.focus.min(self.panes.len().saturating_sub(1));
        self.panes.is_empty()
    }
}

/// The agents a new pane can be started with: the ones cctop already knows how
/// to alias, filtered to what is actually installed, plus the login shell.
///
/// The shell earns its place — half of what a tab is wanted for next to an agent
/// is `git diff`, and a tab that can only hold an agent would send you back out
/// to another window for it.
pub fn harnesses() -> Vec<Vec<String>> {
    let mut found: Vec<Vec<String>> = crate::alias::AGENTS
        .split_whitespace()
        .filter(|agent| crate::shim::is_command(agent))
        .map(|agent| vec![agent.to_string()])
        .collect();
    if let Some(shell) = std::env::var("SHELL").ok().filter(|s| !s.is_empty()) {
        found.push(vec![shell]);
    }
    found
}

/// How a command picked from the launcher is named on screen: the command as
/// typed, minus any path, which matches what [`shim::host`](crate::shim::host)
/// calls it once it is running.
pub fn label_of(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| arg.rsplit('/').next().unwrap_or(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The launcher must never offer something that isn't there — picking it
    /// would open a tab onto an immediate "command not found".
    #[test]
    fn the_launcher_only_offers_commands_that_exist() {
        for argv in harnesses() {
            assert!(
                crate::shim::is_command(&argv[0]),
                "offered a command that is not installed: {argv:?}"
            );
        }
    }

    #[test]
    fn a_command_is_labelled_by_its_name_not_its_path() {
        assert_eq!(label_of(&["/usr/bin/claude".into()]), "claude");
        assert_eq!(
            label_of(&["codex".into(), "--full-auto".into()]),
            "codex --full-auto"
        );
    }
}
