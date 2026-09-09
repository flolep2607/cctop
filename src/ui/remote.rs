//! Rows that belong to another machine, and the conflicts they cause.
//!
//! A remote row is read-only — there is no process here to kill, no transcript
//! here to delete — so the refusals have to be uniform wherever they are
//! reached from, and they survive a local walk that knows nothing about them.
//! Two agents editing one file is the same problem seen from the other end, so
//! the clash reporting lives with it.

use super::*;
use crate::loader::Stats;

impl App {
    /// The footer's warning that two live agents have written the same file.
    ///
    /// Only the file-level overlaps. A shared repository is worth the cell in
    /// the `!` column and no more — agents share repositories all day and
    /// nothing has gone wrong yet, whereas two of them writing one file means
    /// one has already lost an edit or is about to.
    ///
    /// Unlike the bell's line this does not clear when you select the row: the
    /// bell reports a moment that has passed, and this reports a state that is
    /// still true whether or not you are looking at it.
    pub fn conflict_footer(&self) -> Option<String> {
        use std::collections::BTreeSet;
        let mut agents = 0;
        let mut files: BTreeSet<&str> = BTreeSet::new();
        for c in self.collisions.values() {
            if c.level != crate::collide::Overlap::File {
                continue;
            }
            agents += 1;
            files.extend(c.files.iter().map(String::as_str));
        }
        let first = files.iter().next()?;
        let more = match files.len() {
            1 => String::new(),
            n => format!(" +{} more", n - 1),
        };
        Some(format!(
            "Conflict: ⚠ {}{more} — {agents} agents have written it",
            crate::util::path_tail(first, 2)
        ))
    }

    /// Put the current remote snapshots back into the table.
    ///
    /// Every wholesale replacement of `sessions` — a full walk, a discovery —
    /// drops the remote rows, because the loader only ever knows about this
    /// machine. Rather than teaching the loader about ssh, the rows are
    /// re-appended here and the totals recomputed over both.
    ///
    /// A no-op with no hosts configured, so the ordinary single-machine run
    /// pays nothing for this.
    pub fn merge_remotes(&mut self) {
        if self.remotes.is_empty() {
            return;
        }
        self.sessions.retain(|s| s.remote.is_none());
        for rows in self.remotes.values() {
            self.sessions.extend(rows.iter().cloned());
        }
        self.stats = crate::loader::compute_stats(&self.sessions);
        self.refilter();
    }

    /// Take the worker's totals, unless remote rows mean they are not the whole
    /// picture. The worker only ever sees this machine.
    pub(super) fn adopt_stats(&mut self, stats: Stats) {
        self.stats = match self.remotes.is_empty() {
            true => stats,
            false => crate::loader::compute_stats(&self.sessions),
        };
    }

    /// Whether an action that reaches into this machine can apply to a row.
    ///
    /// Returns the refusal to show, or `None` when the row is local. Every
    /// caller is a path that signals a process, deletes a file, or opens a pty,
    /// and each would otherwise do it to whatever sits at the same path here.
    pub fn remote_refusal(session: &Session) -> Option<String> {
        let r = session.remote.as_ref()?;
        Some(format!(
            "{} is on {} — cctop reads other machines but only acts on this one",
            session.display_label(),
            r.host
        ))
    }

    /// The footer's note that a machine is not answering.
    pub fn remote_footer(&self) -> Option<String> {
        let mut hosts: Vec<&str> = self.remote_errors.keys().map(String::as_str).collect();
        hosts.sort();
        let first = hosts.first()?;
        // One host names its reason, which is nearly always the whole fix
        // ("Permission denied", "command not found"). Several would not fit, so
        // they are counted and the panel is where the rest live.
        Some(match hosts.len() {
            1 => format!("{first}: {}", self.remote_errors[*first]),
            n => format!(
                "{n} hosts unreachable ({first}: {})",
                self.remote_errors[*first]
            ),
        })
    }

    /// What one session collides with, with its peers named the way the table
    /// names them.
    pub fn clash_of(&self, session: &Session) -> Option<panels::Clash> {
        let c = self.collisions.get(&session.key())?;
        let peers = c
            .peers
            .iter()
            .filter_map(|key| self.sessions.iter().find(|s| &s.key() == key))
            .map(|s| s.display_label().to_string())
            .collect();
        Some(panels::Clash {
            level: c.level,
            peers,
            files: c.files.clone(),
        })
    }

    /// Whether the cursor is on a row read from another machine.
    pub fn selected_is_remote(&self) -> bool {
        self.selected_session().is_some_and(|s| s.remote.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{session, test_app};
    use std::sync::mpsc::channel;
    /// Two live agents in one checkout, both having written the same file —
    /// the arrangement the whole warning exists for.
    #[test]
    fn a_contested_file_reaches_the_footer_and_the_info_panel() {
        let repo = std::env::temp_dir().join(format!("cctop-ui-clash-{}", std::process::id()));
        std::fs::create_dir_all(repo.join(".git")).expect("checkout");
        let dir = repo.to_string_lossy().into_owned();
        let contested = crate::collide::normalise("src/ui/mod.rs", &dir);

        let mut app = test_app();
        app.sessions = vec![session("a", true, &dir), session("b", true, &dir)];
        for s in app.sessions.iter_mut() {
            s.recent_writes = vec![contested.clone()];
        }
        app.collisions = crate::collide::apply(&mut app.sessions);

        // On the rows, so the column can colour and sort by it…
        for s in &app.sessions {
            assert_eq!(s.conflict, Some(crate::collide::Overlap::File));
        }
        // …and in the footer, which names the file rather than a count.
        let footer = app.conflict_footer().expect("a warning");
        assert!(footer.contains("ui/mod.rs"), "{footer}");
        assert!(footer.contains("2 agents"), "{footer}");

        // The panel names the peer, which is the part that says what to do.
        let clash = app.clash_of(&app.sessions[0]).expect("a clash");
        assert_eq!(clash.peers, vec![app.sessions[1].display_label()]);
        assert_eq!(clash.files, vec![contested]);

        // A repository shared without a shared file is the quieter finding, and
        // deliberately does not reach the footer.
        app.sessions[1].recent_writes = vec![crate::collide::normalise("other.rs", &dir)];
        app.collisions = crate::collide::apply(&mut app.sessions);
        assert_eq!(
            app.sessions[0].conflict,
            Some(crate::collide::Overlap::Directory)
        );
        assert!(app.conflict_footer().is_none());

        std::fs::remove_dir_all(&repo).ok();
    }

    /// A remote row survives the walk that replaces the table, counts towards
    /// the totals, and refuses every key that would reach into this machine.
    #[test]
    fn remote_rows_outlive_a_walk_and_stay_read_only() {
        let mut app = test_app();
        app.sessions = vec![session("local", true, "/here")];

        let mut away = session("away", true, "/srv/work");
        away.remote = Some(crate::session::Remote {
            host: "box".into(),
            branch: Some("main".into()),
        });
        away.total_cost = Some(3.0);
        app.remotes.insert("box".into(), vec![away]);
        app.merge_remotes();
        assert_eq!(app.sessions.len(), 2);
        assert!(
            (app.stats.spend_claude - 3.0).abs() < 1e-9,
            "totals span hosts"
        );

        // A full walk replaces the table with this machine's rows only. The
        // remote ones have to come back, or a host would blink out every
        // refresh.
        app.sessions = vec![session("local", true, "/here")];
        app.merge_remotes();
        assert_eq!(app.sessions.len(), 2);
        assert_eq!(
            app.sessions.iter().filter(|s| s.remote.is_some()).count(),
            1
        );

        // And nothing here may act on it.
        let remote = app
            .sessions
            .iter()
            .find(|s| s.remote.is_some())
            .expect("the remote row");
        let why = App::remote_refusal(remote).expect("a refusal");
        assert!(why.contains("box"), "the refusal has to name the machine");
        assert!(App::remote_refusal(&app.sessions[0]).is_none());

        // The branch comes from the far side rather than from this filesystem,
        // where the same path may well exist and mean something else.
        assert_eq!(
            crate::ui::columns::branch_of(remote).as_deref(),
            Some("main")
        );

        // A host that stops answering keeps its rows and says so.
        app.remote_errors
            .insert("box".into(), "Permission denied".into());
        let footer = app.remote_footer().expect("a warning");
        assert!(footer.contains("box"), "{footer}");
        assert!(footer.contains("Permission denied"), "{footer}");
    }

    /// With no host configured the column is one repeated word down every row,
    /// so it is hidden — through the user's own mechanism, so the two cannot
    /// disagree about what is on screen.
    #[test]
    fn the_host_column_stays_off_a_single_machine() {
        let ids = |hidden: &[ColumnId]| -> Vec<ColumnId> {
            columns::visible_columns(300, hidden)
                .iter()
                .map(|c| c.id)
                .collect()
        };
        assert!(ids(&[]).contains(&ColumnId::Host));
        assert!(!ids(&[ColumnId::Host]).contains(&ColumnId::Host));
    }

    /// The menu and the keyboard must never disagree about what is possible.
    /// Both ask the same predicates; this pins that they still do.
    #[test]
    fn the_menu_refuses_a_remote_row_the_way_the_keys_do() {
        let mut app = App::new(Plan::Retail, channel().0);
        let mut s = session("a", true, "/repo");
        s.remote = Some(crate::session::Remote {
            host: "devbox".into(),
            branch: None,
        });
        app.sessions = vec![s];
        app.refilter();
        app.selected = 0;

        let items = menu::items(&app);
        assert!(!items.is_empty(), "a selected row has a menu");

        // Everything that reaches into this filesystem is refused, and every
        // refusal names the host — the same answer pressing the key gives.
        for item in &items {
            match item.action {
                menu::Action::Expand | menu::Action::Mark => {
                    assert!(item.enabled(), "{} works on a remote row", item.label);
                }
                _ => {
                    let why = item.blocked.as_deref().unwrap_or("");
                    assert!(
                        why.contains("devbox"),
                        "{} must name the host, said {why:?}",
                        item.label
                    );
                }
            }
        }
        // And the cursor never rests on one of the refusals.
        assert!(items[menu::first_enabled(&items)].enabled());
    }
}
