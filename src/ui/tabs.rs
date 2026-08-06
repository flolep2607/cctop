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
use std::time::{Duration, Instant};

/// How long a pane's screen has to sit still before the agent counts as idle.
///
/// An agent that is working repaints constantly — Claude Code's "✻ Baked for
/// 5s" alone ticks every second — so silence is the signal, and it needs no
/// per-harness parsing. Two seconds clears the gap between a spinner's frames
/// without waiting so long that a finished turn goes unnoticed. A blinking
/// cursor does not count: the terminal draws that, not the agent.
///
// ponytail: an agent that redraws nothing while thinking would read as idle.
// None of the four do; if one appears, its transcript's activity state is the
// tiebreak.
const QUIET_IS_IDLE: Duration = Duration::from_secs(2);

/// Why a tab is asking to be looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attention {
    /// The agent has stopped drawing: its turn is over and the prompt is yours.
    Idle,
    /// The agent has explicitly asked something and is blocked on the answer.
    NeedsInput,
}

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
    /// When this pane's screen last changed, which is how idleness is told
    /// without asking the agent or its transcript anything.
    drew_at: Instant,
}

impl Pane {
    /// Whether the agent has gone quiet long enough to count as waiting for you.
    fn idle(&self) -> bool {
        self.drew_at.elapsed() >= QUIET_IS_IDLE
    }
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
            drew_at: Instant::now(),
        })
    }

    /// Open a pane onto an agent something else is responsible for.
    pub fn view_of(pid: u32, label: String) -> Option<Pane> {
        Some(Pane {
            hosted: None,
            pid,
            label,
            view: crate::attach::attach(pid)?,
            drew_at: Instant::now(),
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
            let drew = pane.view.pump();
            if drew {
                pane.drew_at = Instant::now();
            }
            drew | changed
        })
    }

    /// What this tab wants, if anything — the most urgent of its panes.
    ///
    /// `asking` names the pids whose sessions have explicitly asked something.
    /// That comes from the transcript and only exists once one has been written,
    /// so a pane that is blocked on a question its session has not recorded yet
    /// still reads as idle. Idle is the weaker claim of the two, so under-calling
    /// it that way is the right direction to be wrong in.
    ///
    /// The focused pane never asks: you are looking straight at it.
    pub fn attention(&self, focused: bool, asking: &dyn Fn(u32) -> bool) -> Option<Attention> {
        self.panes
            .iter()
            .enumerate()
            .filter(|(i, _)| !(focused && *i == self.focus))
            .filter_map(|(_, pane)| match asking(pane.pid) {
                true => Some(Attention::NeedsInput),
                false => pane.idle().then_some(Attention::Idle),
            })
            // A held question outranks a finished turn: one of them is blocking
            // an agent, the other is only waiting on you when you get to it.
            .max_by_key(|a| matches!(a, Attention::NeedsInput))
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

    /// A pane whose agent is still drawing must not claim to be idle, a held
    /// question must outrank a finished turn, and the tab you are looking at
    /// must stay quiet — blinking the title of the pane in front of you is
    /// noise, and it is the case that fires most often.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_tab_asks_for_attention_only_when_it_has_something_you_cannot_see() {
        let mut kids = Vec::new();
        let mut pane = |script: &str| {
            let (child, pid) = crate::shim::test_session(&["sh", "-c", script], (80, 24));
            kids.push(child);
            (pid, Pane::view_of(pid, "agent".into()).expect("attach"))
        };
        // One agent that keeps painting, one that drew once and went quiet.
        let (busy_pid, busy) = pane("while :; do printf '.'; sleep 0.2; done");
        let (quiet_pid, quiet) = pane("printf 'done'; sleep 30");

        let mut tab = Tab::new(busy);
        tab.panes.push(quiet);
        let nobody = |_: u32| false;

        // Both panes have just been created, so neither has been quiet yet.
        assert_eq!(tab.attention(false, &nobody), None);

        // Past the threshold, only the one that stopped drawing is idle.
        let deadline = Instant::now() + QUIET_IS_IDLE + Duration::from_secs(2);
        while Instant::now() < deadline {
            tab.pump();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(tab.attention(false, &nobody), Some(Attention::Idle));

        // The busy pane holding a question outranks the quiet one being idle.
        assert_eq!(
            tab.attention(false, &|pid| pid == busy_pid),
            Some(Attention::NeedsInput)
        );
        // Focused tab: the focused pane is excluded, so the quiet one is left.
        tab.focus = 0;
        assert_eq!(
            tab.attention(true, &|pid| pid == busy_pid),
            Some(Attention::Idle)
        );
        // Focus the quiet one instead: it is the only pane with anything to
        // report, and you are looking straight at it, so the tab stays quiet.
        tab.focus = 1;
        assert_eq!(tab.attention(true, &nobody), None);

        drop(tab);
        for child in &mut kids {
            let _ = child.kill();
            let _ = child.wait();
        }
        for pid in [busy_pid, quiet_pid] {
            let _ = crate::shim::socket_path(pid).map(std::fs::remove_file);
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
