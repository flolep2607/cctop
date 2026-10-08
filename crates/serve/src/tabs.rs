//! `/api/tabs` — the tabs cctop has open, for the browser to reach.
//!
//! A tab in the TUI is an agent in one of cctop's rmux sessions, and rmux
//! keeps the tab bar's arrangement on the sessions themselves — the label, the
//! order, which tab a split belongs to (see [`cctop_core::rmux::Running`]). So this
//! reads the bar from rmux rather than from a TUI: a `cctop serve` with no
//! dashboard anywhere shows the same tabs, in the same order, as the one that
//! opened them.
//!
//! What a tab is *for* in the browser is its terminal, which is an rmux share —
//! the same one a session page frames (see [`super::term`]). Listing is
//! read-only; opening a terminal is an action, and goes through the same guards
//! as every other one.

use cctop_core::rmux::Running;
use cctop_core::session::Session;
use serde::Serialize;

/// How long a tab's window may sit unchanged before the agent in it counts
/// as idle — the same two seconds of silence a pane in the TUI waits for.
const QUIET_SECS: u64 = 2;

/// One tab, as the page draws it.
#[derive(Debug, Serialize)]
pub struct Tab {
    /// The rmux session name: the handle `/api/tab/<name>/terminal` takes.
    pub name: String,
    /// What the tab bar calls it.
    pub label: String,
    pub cwd: Option<String>,
    /// `needs-input`, `working` or `idle`.
    pub state: &'static str,
    /// The tab's colour as the TUI painted it, when it was given one.
    pub color: Option<String>,
    /// The leading session of the split this is a pane of, when it is one —
    /// the same grouping the TUI rebuilds its splits from.
    pub tab: Option<String>,
    /// Whether a terminal client is attached to it somewhere.
    pub attached: bool,
    /// The session the agent is writing, when cctop can tell — for a link to
    /// its page.
    pub session_id: Option<String>,
}

/// The tabs in the TUI's order, matched to sessions where the agent's process
/// is one cctop already attributes.
pub fn build(running: Vec<Running>, sessions: &[Session], now: u64) -> Vec<Tab> {
    running
        .into_iter()
        .map(|r| {
            let session_id = r.pid.and_then(|pid| {
                sessions
                    .iter()
                    .find(|s| s.remote.is_none() && s.root_pid() == Some(pid))
                    .map(|s| s.session_id.clone())
            });
            Tab {
                state: state_of(&r, now),
                label: r.label.clone().unwrap_or_else(|| short_name(&r.name)),
                cwd: r.cwd.as_ref().map(|p| p.to_string_lossy().into_owned()),
                color: r.color.clone(),
                tab: r.tab.clone(),
                attached: r.attached,
                session_id,
                name: r.name,
            }
        })
        .collect()
}

/// What the agent is doing: what it last reported while that can still be
/// true, and otherwise whether its window is still printing.
fn state_of(r: &Running, now: u64) -> &'static str {
    if let Some(state) = r.state.filter(|s| s.is_current(now)) {
        return match state.signal {
            cctop_core::hook::Signal::NeedsInput => "needs-input",
            signal if signal.is_working() => "working",
            _ => "idle",
        };
    }
    match r.activity {
        Some(at) if now.saturating_sub(at) <= QUIET_SECS => "working",
        _ => "idle",
    }
}

/// A session name without the `cctop-` every one of them starts with.
fn short_name(name: &str) -> String {
    name.strip_prefix("cctop-").unwrap_or(name).to_string()
}

/// Whether `name` is one of cctop's live tabs — the only sessions a terminal
/// may be opened on. Without this the route would mint a shell link to any
/// rmux session on the machine, by name.
pub fn is_tab(running: &[Running], name: &str) -> bool {
    running.iter().any(|r| r.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(name: &str, pid: u32, activity: u64) -> Running {
        Running {
            name: name.into(),
            pid: Some(pid),
            cwd: None,
            attached: false,
            activity: Some(activity),
            label: None,
            profile: None,
            order: None,
            state: None,
            color: None,
            tab: None,
            pane: None,
            axis: None,
            window: None,
        }
    }

    /// A tab is matched to the session its agent writes, keeps the bar's
    /// order, and reads as working only while its window is printing.
    #[test]
    fn tabs_keep_their_order_and_find_their_sessions() {
        let mut session = Session::new(cctop_core::pricing::Provider::Claude, "abc".into());
        session.process = Some(cctop_core::proc::ProcInfo {
            pids: 1,
            cpu: 0.0,
            memory: 0,
            command: "claude".into(),
            process_list: vec![cctop_core::proc::ProcEntry {
                pid: 42,
                cpu: 0.0,
                memory: 0,
                args: "claude".into(),
                is_root: true,
                ghost: false,
            }],
        });
        let now = 1_000;
        let tabs = build(
            vec![
                running("cctop-b", 42, now - 1),
                running("cctop-a", 7, now - 60),
            ],
            &[session],
            now,
        );
        assert_eq!(tabs[0].name, "cctop-b", "the bar's order is kept");
        assert_eq!(tabs[0].label, "b");
        assert_eq!(tabs[0].session_id.as_deref(), Some("abc"));
        assert_eq!(tabs[0].state, "working");
        assert_eq!(tabs[1].session_id, None);
        assert_eq!(tabs[1].state, "idle");
    }

    #[test]
    fn only_a_live_tab_can_be_opened() {
        let live = vec![running("cctop-a", 1, 0)];
        assert!(is_tab(&live, "cctop-a"));
        assert!(!is_tab(&live, "work"), "an rmux session cctop did not open");
        assert!(!is_tab(&live, "cctop-gone"));
    }
}
