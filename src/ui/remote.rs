//! Rows that belong to another machine, and the conflicts they cause.
//!
//! A remote row is read-only — there is no process here to kill, no transcript
//! here to delete — so the refusals have to be uniform wherever they are
//! reached from, and they survive a local walk that knows nothing about them.
//! Two agents editing one file is the same problem seen from the other end, so
//! the clash reporting lives with it.

use super::*;
use crate::loader::Stats;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

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
        for (host, rows) in &self.remotes {
            let skew = self.skew_of(host);
            self.sessions.extend(rows.iter().cloned().map(|mut s| {
                if let Some(r) = s.remote.as_mut() {
                    r.skew = skew.clone();
                }
                s
            }));
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

    /// How `host`'s cctop stands against this one, if it has said.
    pub fn skew_of(&self, host: &str) -> Option<crate::fleet::Skew> {
        crate::fleet::skew(
            self.remote_versions.get(host)?,
            crate::update::current_version(),
        )
    }

    /// Take a host's answer to `--version`, and say so once if it differs.
    ///
    /// A server left on an old cctop while the laptop moved on is exactly the
    /// thing nobody notices: the rows keep arriving, just without whatever the
    /// newer build would have sent. So a mismatch is said out loud, once per
    /// host per run, and then left to the HOST column's marker and the Info
    /// panel — which stay true for as long as it does.
    pub fn got_remote_version(&mut self, host: String, probe: crate::fleet::Probe) {
        use crate::fleet::Skew;
        self.remote_versions.insert(host.clone(), probe);
        let Some(skew) = self.skew_of(&host) else {
            return;
        };
        if !self.remote_skew_told.insert(host.clone()) {
            return;
        }
        let local = crate::update::current_version();
        self.set_status(match skew {
            Skew::Older(v) => format!(
                "{host} runs cctop {v}, older than this {local} — Enter on one of its rows \
                 offers to update it"
            ),
            Skew::Newer(v) => format!(
                "{host} runs cctop {v}, newer than this {local} — `cctop --update` here \
                 to catch up"
            ),
            Skew::Missing => format!(
                "{host} has no cctop where ssh looks — install it there, or name the binary \
                 with --host {host}:/path/to/cctop"
            ),
        });
    }

    /// Why the selected row's machine cannot be offered `--update`, naming
    /// it; `None` when it can.
    ///
    /// Only a host known to be *behind* is offered one. Level is nothing to
    /// do, ahead is this machine's problem, and unknown is a guess — running a
    /// self-replacing binary on someone's server on a guess is not a thing to
    /// put one keypress away.
    pub fn remote_update_refusal(&self, host: &str) -> Option<String> {
        use crate::fleet::Skew;
        if self.remote_updating.contains(host) {
            return Some(format!("an update of {host} is already under way"));
        }
        match self.skew_of(host) {
            Some(Skew::Older(_)) => None,
            Some(Skew::Newer(v)) => Some(format!(
                "{host} runs {v}, newer than this cctop — update this one instead"
            )),
            Some(Skew::Missing) => Some(format!("{host} has no cctop to update")),
            None => Some(match self.remote_versions.get(host) {
                Some(crate::fleet::Probe::Version(v)) => format!("{host} already runs {v}"),
                _ => format!("{host}'s cctop version is not known yet"),
            }),
        }
    }

    /// Ask before running `cctop --update` on the selected row's machine.
    pub(super) fn confirm_remote_update(&mut self) {
        let Some(host) = self
            .selected_session()
            .and_then(|s| s.remote.as_ref())
            .map(|r| r.host.clone())
        else {
            return;
        };
        if let Some(why) = self.remote_update_refusal(&host) {
            self.set_status(why);
            return;
        }
        self.remote_update = Some(host);
        self.mode = Mode::RemoteUpdateConfirm;
    }

    /// The `Host` behind a name, command and all — what the confirmation shows
    /// and what the update runs.
    pub fn remote_host(&self, target: &str) -> Option<&crate::fleet::Host> {
        self.remote_hosts.iter().find(|h| h.target == target)
    }

    /// The answer to the confirmation. Anything but `y` is a no.
    pub(super) fn on_key_remote_update(&mut self, key: KeyEvent) {
        self.mode = Mode::List;
        let Some(target) = self.remote_update.take() else {
            return;
        };
        if key.code != KeyCode::Char('y') {
            return;
        }
        let Some(host) = self.remote_host(&target).cloned() else {
            return;
        };
        self.remote_updating.insert(target.clone());
        let _ = self.tx.send(Request::UpdateRemote(host));
        self.set_status(format!("Updating cctop on {target}…"));
    }

    /// How a remote `--update` went.
    ///
    /// A root-owned install is said as such, with the command to run: cctop
    /// will run a self-replacing binary on another machine when asked, but it
    /// will not type a password into sudo there, and a bare "permission
    /// denied" would leave the user to work out which half refused.
    pub fn remote_updated(
        &mut self,
        host: String,
        result: Result<String, crate::fleet::UpdateFailure>,
    ) {
        use crate::fleet::UpdateFailure;
        self.remote_updating.remove(&host);
        self.set_status(match result {
            Ok(said) => format!("{host}: {said}"),
            Err(UpdateFailure::NeedsRoot(manual)) => format!(
                "Could not update {host}: its cctop is in a directory only root can write. \
                 Run it yourself: {manual}"
            ),
            Err(UpdateFailure::Other(why)) => format!("Could not update {host}: {why}"),
        });
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
            ..Default::default()
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

    /// A server left behind the laptop: said once, marked on its rows, and
    /// updated only through a confirmation that names the command.
    #[test]
    fn an_older_remote_is_said_once_marked_and_offered_an_update() {
        use crate::fleet::{Host, Probe, UpdateFailure};
        let (tx, rx) = channel();
        let mut app = App::new(Plan::Retail, tx);
        let host = Host::parse("box").expect("a host");
        app.remote_hosts = vec![host.clone()];
        let mut s = session("away", true, "/srv/work");
        s.remote = Some(crate::session::Remote {
            host: "box".into(),
            ..Default::default()
        });
        app.remotes.insert("box".into(), vec![s]);
        app.merge_remotes();

        // Before it has said, nothing is claimed and nothing is offered.
        let refusal = app.remote_update_refusal("box").expect("not yet");
        assert!(refusal.contains("not known"), "{refusal}");

        app.got_remote_version("box".into(), Probe::Version("0.0.1".into()));
        app.merge_remotes();
        let said: Vec<String> = app.toasts.iter().map(|t| t.text.clone()).collect();
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("box runs cctop 0.0.1"), "{}", said[0]);

        // A reconnect answers the same; the toast is not said twice.
        app.got_remote_version("box".into(), Probe::Version("0.0.1".into()));
        assert_eq!(app.toasts.iter().count(), 1);

        // The marker rides on the row into the HOST cell.
        app.selected = 0;
        let row = app.selected_session().expect("the row").clone();
        let cell = columns::render_cell(ColumnId::Host, &row, &chrono::Utc::now());
        assert_eq!(cell, "↑box");

        // The menu offers it, and choosing it stops at the confirmation.
        let items = menu::items(&app);
        let entry = items
            .iter()
            .find(|i| i.action == menu::Action::UpdateRemote)
            .expect("an update entry");
        assert!(entry.enabled(), "{:?}", entry.blocked);
        app.confirm_remote_update();
        assert_eq!(app.mode, Mode::RemoteUpdateConfirm);

        // Anything but y is a no, and sends nothing.
        app.on_key_remote_update(KeyEvent::from(KeyCode::Char('n')));
        assert_eq!(app.mode, Mode::List);
        assert!(rx.try_recv().is_err(), "a no must not reach the worker");

        app.confirm_remote_update();
        app.on_key_remote_update(KeyEvent::from(KeyCode::Char('y')));
        let sent = std::iter::from_fn(|| rx.try_recv().ok())
            .find_map(|r| match r {
                Request::UpdateRemote(h) => Some(h),
                _ => None,
            })
            .expect("the update went to the worker");
        assert_eq!(sent, host);
        // While it runs, a second is not offered.
        assert!(app.remote_update_refusal("box").is_some());

        // A root-owned binary is said as such, with the command to run by hand.
        app.remote_updated(
            "box".into(),
            Err(UpdateFailure::NeedsRoot(host.sudo_update_command())),
        );
        let last = app.toasts.iter().next().expect("a toast").text.clone();
        assert!(last.contains("only root can write"), "{last}");
        assert!(last.contains("ssh -t box sudo cctop --update"), "{last}");
    }

    /// A remote that is ahead means this machine is the stale one.
    #[test]
    fn a_newer_remote_points_at_this_machine() {
        let mut app = test_app();
        app.got_remote_version("box".into(), crate::fleet::Probe::Version("999.0.0".into()));
        let said = app.toasts.iter().next().expect("a toast").text.clone();
        assert!(said.contains("cctop --update` here"), "{said}");
        let why = app.remote_update_refusal("box").expect("refused");
        assert!(why.contains("update this one"), "{why}");

        // And a host with no cctop at all is told how to point at one.
        app.got_remote_version("bare".into(), crate::fleet::Probe::Missing);
        let said = app.toasts.iter().next().expect("a toast").text.clone();
        assert!(said.contains("--host bare:/path/to/cctop"), "{said}");
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
            ..Default::default()
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
                // Read joins Expand and Mark on a remote row: it reads over the
                // same ssh channel the row arrived by, never the local path.
                menu::Action::Read | menu::Action::Expand | menu::Action::Mark => {
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
