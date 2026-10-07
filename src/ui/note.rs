//! The footer's user-written line: a static note, or a command's first line.
//!
//! The command runs on a thread of its own, like the tunnel opening: a user
//! command may hang or take a second, and the dashboard has to keep drawing.
//! Its answer is folded into the footer on the next tick — see
//! [`super::runloop`] — so a stale badge is the worst anything gets wrong.

use super::*;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// What one command refresh costs before it is given up on. Bounded by
/// `timeout`, so a hung command is a blank badge rather than a thread that
/// never finishes.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a result is shown before the command is asked again.
const REFRESH: Duration = Duration::from_secs(30);

/// A pending command, and when it was asked. `None` until the first run and
/// between runs and their answers.
struct Pending {
    rx: Receiver<Option<String>>,
    asked: Instant,
}

#[derive(Default)]
pub struct Extra {
    /// What the settings file says now; read on every reload.
    note: Option<String>,
    command: Option<String>,
    /// The command's last answer, kept between runs so the footer does not
    /// blink blank every thirty seconds.
    live: Option<String>,
    pending: Option<Pending>,
    last_run: Option<Instant>,
}

impl Extra {
    /// Point the extra at what the file says now. A command that changes
    /// forgets its last answer: showing yesterday's answer to a different
    /// command would be worse than a blank line for one interval.
    pub fn apply(&mut self, note: Option<&str>, command: Option<&str>) {
        let note = nonempty(note);
        let command = nonempty(command);
        if command != self.command.as_deref() {
            self.live = None;
            self.pending = None;
            self.last_run = None;
        }
        self.note = note.map(str::to_string);
        self.command = command.map(str::to_string);
    }

    /// The line to draw, if there is one: the command's answer while it is
    /// live, else the static note. The command wins because it is the one
    /// that can change under you — hiding it behind a static note would
    /// mean it never shows at all.
    pub fn text(&self) -> Option<&str> {
        self.live.as_deref().or(self.note.as_deref())
    }

    /// Collect a finished command, and ask again when the answer has gone
    /// old. Returns whether the footer changed.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        if let Some(pending) = &self.pending {
            match pending.rx.try_recv() {
                Ok(answer) => {
                    changed = self.live != answer;
                    self.live = answer;
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
                // The thread went without answering, which it has no path
                // to do. Blank rather than stale.
                Err(TryRecvError::Disconnected) => {
                    changed = self.live.is_some();
                    self.live = None;
                    self.pending = None;
                }
            }
        }
        let due = self.last_run.is_none_or(|at| at.elapsed() >= REFRESH);
        if due
            && self.pending.is_none()
            && let Some(command) = self.command.clone()
        {
            self.last_run = Some(Instant::now());
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(run(&command));
            });
            self.pending = Some(Pending {
                rx,
                asked: Instant::now(),
            });
        }
        // A command stuck past its deadline gives up its pending slot, so
        // the next tick asks again rather than waiting for a thread that
        // will never answer.
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.asked.elapsed() > COMMAND_TIMEOUT + REFRESH)
        {
            self.pending = None;
        }
        changed
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// The first line of the command's output, blanked when it printed nothing
/// or failed. `timeout` bounds a hung command; the static error text a
/// missing shell would print is deliberately also just nothing on screen.
fn run(command: &str) -> Option<String> {
    let out = std::process::Command::new("timeout")
        .arg(format!("{}s", COMMAND_TIMEOUT.as_secs()))
        .args(["sh", "-c", command])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .next()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_static_note_shows_until_replaced() {
        let mut extra = Extra::default();
        assert_eq!(extra.text(), None);
        extra.apply(Some("on call"), None);
        assert_eq!(extra.text(), Some("on call"));
        extra.apply(None, None);
        assert_eq!(extra.text(), None);
    }

    #[test]
    fn a_blank_command_is_no_command() {
        let mut extra = Extra::default();
        extra.apply(Some("note"), Some("  "));
        assert_eq!(extra.text(), Some("note"));
        // And no command means tick never spawns one.
        assert!(!extra.tick());
        assert!(extra.pending.is_none());
    }

    #[test]
    fn the_command_answer_replaces_the_note() {
        let mut extra = Extra::default();
        extra.apply(Some("note"), Some("printf 'live\\n'"));
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(100));
            if extra.tick() && extra.text() == Some("live") {
                break;
            }
        }
        assert_eq!(extra.text(), Some("live"));
    }
}
