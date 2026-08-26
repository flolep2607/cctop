//! The per-row action menu: everything you can do to the selected session, in
//! one list.
//!
//! Every action here already had a key. The keys are good ones and they stay —
//! this is a second route to the same code, not a replacement for it. What it
//! adds is that you no longer have to know `O` exists to find the handoff, and
//! that a row which *cannot* be handed off says so on the spot rather than
//! after you press the key.
//!
//! That last part is the reason this is a list of items rather than a menu that
//! only shows what applies. A remote row can do almost none of this, and an
//! entry that quietly vanishes teaches nobody why — it reads as a menu that
//! changes shape at random. So every entry is always present, and the ones that
//! cannot run carry the reason cctop would have printed anyway. The refusals
//! are not written twice: they come from the same
//! [`remote_refusal`](super::App::remote_refusal) and the same subagent checks
//! the key handler uses, because a menu that disagreed with the keyboard about
//! what is possible would be worse than no menu.
//!
//! ponytail: no submenus and no scrolling. Nine entries fit any terminal cctop
//! will draw a table in, and a menu that needed either would be a sign the
//! actions wanted grouping rather than nesting.

use super::App;

/// What a menu entry does, mapped onto the method the key already calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Resume,
    Attach,
    Send,
    Handoff,
    Expand,
    Mark,
    Terminate,
    Delete,
}

pub struct Item {
    pub action: Action,
    pub label: &'static str,
    /// The key that does this from the table, shown so the menu teaches the
    /// shortcut rather than hiding it.
    pub key: &'static str,
    /// Why this cannot be done to the selected row. `None` means it can.
    pub blocked: Option<String>,
    /// Draw a rule above this entry. The two destructive actions sit below one,
    /// so neither is next to a cursor that just arrived from somewhere safe.
    pub rule: bool,
}

impl Item {
    pub fn enabled(&self) -> bool {
        self.blocked.is_none()
    }
}

/// The menu for whatever row is selected, or empty when none is.
///
/// Built fresh on each draw rather than cached: the answers change as the
/// session does — an agent that exits between two frames stops being
/// attachable — and a menu describing the row as it was a second ago is the
/// one thing this must not be.
pub fn items(app: &App) -> Vec<Item> {
    let Some(session) = app.selected_session() else {
        return Vec::new();
    };

    // The three facts every entry is decided by. Read once: `on_subagent` and
    // `selected_session` both walk the visible rows.
    let remote = App::remote_refusal(session);
    let subagent = app.on_subagent();
    let running = session.is_running();
    let deleting = app.deleting.contains(&session.key());
    let has_pid = session.root_pid().is_some();

    // A remote row's transcript, process and pty are all on the other machine,
    // so everything that reaches into this filesystem is refused with the host
    // named — the same answer, from the same place, as pressing the key.
    let far = |also: Option<String>| remote.clone().or(also);

    vec![
        Item {
            action: Action::Resume,
            label: "Resume in a tab",
            key: "R",
            blocked: far(None),
            rule: false,
        },
        Item {
            action: Action::Attach,
            label: "Attach to it",
            key: "a",
            blocked: far(subagent.then(|| "a subagent has no terminal of its own".to_string())),
            rule: false,
        },
        Item {
            action: Action::Send,
            label: "Type into it",
            key: "s",
            blocked: far(match () {
                _ if subagent => Some("a subagent cannot be typed into on its own".into()),
                _ if !has_pid => Some("no local process to type into".into()),
                _ => None,
            }),
            rule: false,
        },
        Item {
            action: Action::Handoff,
            label: "Hand off to another agent",
            key: "O",
            blocked: far(subagent.then(|| "hand off the session, not a subagent".to_string())),
            rule: false,
        },
        Item {
            action: Action::Expand,
            label: "Show its subagents",
            key: "e",
            // Works on a remote row: expanding reads the row already in hand
            // rather than anything on this filesystem.
            blocked: None,
            rule: false,
        },
        Item {
            action: Action::Mark,
            label: "Mark for a batch action",
            key: "space",
            blocked: None,
            rule: false,
        },
        Item {
            action: Action::Terminate,
            label: "Terminate the agent",
            key: "ctrl+k",
            blocked: far(match () {
                _ if !running => Some("it is not running".into()),
                _ if !has_pid => Some("no local process to signal".into()),
                _ => None,
            }),
            rule: true,
        },
        Item {
            action: Action::Delete,
            label: "Delete the transcript",
            key: "d",
            blocked: far(match () {
                _ if subagent => Some("a subagent cannot be deleted on its own".into()),
                _ if deleting => Some("deletion is already in progress".into()),
                _ if running => Some("it is still running".into()),
                _ => None,
            }),
            rule: false,
        },
    ]
}

/// Move `cursor` by `delta`, skipping entries that cannot run.
///
/// Blocked entries are drawn but never landed on: they are there to explain,
/// and stopping the cursor on one would mean Enter did nothing with no way to
/// tell that from a menu that had failed. Returns the cursor unchanged when
/// every entry is blocked, which a remote row very nearly manages.
pub fn step(items: &[Item], cursor: usize, delta: isize) -> usize {
    let n = items.len();
    if n == 0 {
        return 0;
    }
    for hop in 1..=n {
        let next = (cursor as isize + delta * hop as isize).rem_euclid(n as isize) as usize;
        if items[next].enabled() {
            return next;
        }
    }
    cursor
}

/// The first entry that can actually run, which is where the cursor opens.
pub fn first_enabled(items: &[Item]) -> usize {
    items.iter().position(Item::enabled).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(blocked: Option<&str>) -> Item {
        Item {
            action: Action::Mark,
            label: "x",
            key: "x",
            blocked: blocked.map(str::to_string),
            rule: false,
        }
    }

    #[test]
    fn stepping_skips_over_blocked_entries() {
        let items = [item(None), item(Some("no")), item(Some("no")), item(None)];
        assert_eq!(step(&items, 0, 1), 3, "past the two blocked entries");
        assert_eq!(step(&items, 3, 1), 0, "and wraps");
        assert_eq!(step(&items, 0, -1), 3, "backwards too");
    }

    #[test]
    fn a_menu_with_nothing_runnable_leaves_the_cursor_alone() {
        // A remote row very nearly manages this. Moving the cursor onto an
        // entry that cannot run would make Enter look broken.
        let items = [item(Some("no")), item(Some("no"))];
        assert_eq!(step(&items, 0, 1), 0);
        assert_eq!(first_enabled(&items), 0);
    }

    #[test]
    fn the_cursor_opens_on_the_first_thing_that_works() {
        let items = [item(Some("no")), item(None), item(None)];
        assert_eq!(first_enabled(&items), 1);
    }

    #[test]
    fn stepping_an_empty_menu_is_not_a_panic() {
        assert_eq!(step(&[], 0, 1), 0);
        assert_eq!(first_enabled(&[]), 0);
    }
}
