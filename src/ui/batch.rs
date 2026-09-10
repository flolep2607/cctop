//! Marked rows, and the actions that run over all of them at once.
//!
//! A batch is not a loop over the single-row action: it has to be refused as a
//! whole before anything is destroyed — one running session blocks a batch
//! delete — and the marks have to outlive the worker's replies, so a row stays
//! on screen until the deletion it was marked for is confirmed. That bookkeeping
//! is what earns this its own file.

use super::*;

impl App {
    /// Open the terminate confirmation for the selected session, explaining
    /// itself when there is nothing this cctop can signal.
    pub(super) fn confirm_terminate(&mut self) {
        // Kept here rather than at the key, because this is now the only way in:
        // `k` moves the cursor, and Ctrl+K is what asks. A subagent has no
        // process of its own to signal — stopping it means stopping its parent,
        // which is not what the cursor is pointing at.
        if self.on_subagent() {
            self.set_status("A subagent cannot be stopped on its own");
            return;
        }
        if let Some(why) = self.selected_session().and_then(App::remote_refusal) {
            self.set_status(why);
            return;
        }
        match self.selected_session() {
            Some(s) if s.root_pid().is_some() => self.mode = Mode::KillConfirm,
            Some(s) if s.is_running() => self.mode = Mode::KillBlocked,
            Some(_) => self.set_status("Selected session is not running"),
            None => {}
        }
    }

    /// Toggle whether the selected session is marked for a batch action.
    pub(super) fn toggle_mark(&mut self) {
        let Some(s) = self.selected_session() else {
            return;
        };
        let key = s.key();
        if !self.marked.remove(&key) {
            self.marked.insert(key);
        }
        self.needs_redraw = true;
    }

    /// The session keys currently marked, in table order for a stable listing.
    pub(super) fn marked_sessions(&self) -> Vec<&Session> {
        self.visible
            .iter()
            .filter_map(|row| match row {
                // Child rows would list their parent a second time.
                Row::Session(i) => self.sessions.get(*i),
                Row::Subagent { .. } => None,
            })
            .filter(|s| self.marked.contains(&s.key()))
            .collect()
    }

    /// True when every marked session is ready for the given batch action.
    pub(super) fn batch_ok(&self, kind: BatchKind) -> bool {
        self.marked_sessions().iter().all(|s| match kind {
            BatchKind::Delete => !s.is_running(),
            BatchKind::Kill => s.root_pid().is_some(),
        })
    }

    pub(super) fn unmark_all(&mut self) {
        if self.marked.is_empty() {
            return;
        }
        self.marked.clear();
        self.needs_redraw = true;
    }

    /// Enter the batch-confirm modal if there's anything to do.
    pub(super) fn batch(&mut self, kind: BatchKind) {
        if self.marked_sessions().is_empty() {
            self.set_status("No sessions marked — press Space to mark");
            return;
        }
        self.batch = kind;
        self.mode = if self.batch_ok(kind) {
            Mode::BatchConfirm
        } else {
            match kind {
                BatchKind::Delete => Mode::BatchDeleteBlocked,
                BatchKind::Kill => Mode::BatchKillBlocked,
            }
        };
        self.needs_redraw = true;
    }

    /// Confirm and run the pending batch action over all marked sessions.
    pub(super) fn batch_execute(&mut self) {
        let kind = self.batch;
        let marked: Vec<Session> = self.marked_sessions().into_iter().cloned().collect();
        let mut requested = 0;
        let mut acted_on: Vec<String> = Vec::new();
        let mut failed = 0;
        for s in &marked {
            let key = s.key();
            // Marking spans machines because the table does; acting does not.
            if s.remote.is_some() {
                failed += 1;
                continue;
            }
            match kind {
                BatchKind::Delete => {
                    if self.tx.send(Request::Delete(Box::new(s.clone()))).is_ok() {
                        self.deleting.insert(key.clone());
                        requested += 1;
                        acted_on.push(key);
                    } else {
                        failed += 1;
                    }
                }
                BatchKind::Kill => match s.root_pid() {
                    Some(pid) => {
                        self.tx
                            .send(Request::Terminate {
                                session_key: key.clone(),
                                pid,
                            })
                            .ok();
                        acted_on.push(key);
                    }
                    None => failed += 1,
                },
            }
        }
        for key in &acted_on {
            self.marked.remove(key);
        }
        self.set_status(match kind {
            BatchKind::Delete => {
                if failed == 0 {
                    format!("Deleting {requested} session(s)…")
                } else {
                    format!("Deleting {requested} session(s), {failed} failed to start")
                }
            }
            BatchKind::Kill => format!(
                "Kill sent to {} session(s){}",
                acted_on.len(),
                if failed > 0 {
                    format!(" ({} skipped)", failed)
                } else {
                    String::new()
                }
            ),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{session, test_app};
    #[test]
    fn batch_delete_keeps_marked_sessions_visible_until_worker_confirms() {
        let mut app = test_app();
        app.sessions = vec![
            session("a", false, "/x"),
            session("b", false, "/x"),
            session("c", false, "/x"),
        ];
        app.refilter();
        // Mark a and c by identity, not by row: the three fixtures are created in
        // the same instant, so which row each lands on depends on how the clock
        // happened to tick. Selecting by index made this assert the sort order,
        // and it failed on hosts where those timestamps came out equal or out of
        // creation order.
        let row = |app: &App, id: &str| {
            app.visible
                .iter()
                .position(|&r| app.sessions[r.session()].session_id == id)
                .expect("fixture is visible")
        };
        app.selected = row(&app, "a");
        app.toggle_mark();
        app.selected = row(&app, "c");
        app.toggle_mark();
        assert_eq!(app.marked.len(), 2);
        assert_eq!(app.marked_sessions().len(), 2);

        app.batch(BatchKind::Delete);
        assert_eq!(app.mode, Mode::BatchConfirm);
        app.batch_execute();
        assert_eq!(app.sessions.len(), 3);
        assert_eq!(app.deleting.len(), 2);
        assert!(app.marked.is_empty());
    }

    #[test]
    fn batch_delete_refuses_when_marked_session_is_running() {
        let mut app = test_app();
        app.sessions = vec![session("a", false, "/x"), session("b", true, "/x")];
        app.refilter();
        app.selected = 0;
        app.toggle_mark();
        app.selected = 1;
        app.toggle_mark();
        app.batch(BatchKind::Delete);
        assert_eq!(app.mode, Mode::BatchDeleteBlocked);
    }
}
