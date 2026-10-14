//! Rows that belong to another machine, and the conflicts they cause.
//!
//! A remote row is read-only — there is no process here to kill, no transcript
//! here to delete — so the refusals have to be uniform wherever they are
//! reached from, and they survive a local walk that knows nothing about them.
//! Two agents editing one file is the same problem seen from the other end, so
//! the clash reporting lives with it.

use super::*;
use cctop_core::loader::Stats;
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
            if c.level != cctop_core::collide::Overlap::File {
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
            cctop_core::util::path_tail(first, 2)
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
        self.stats = cctop_core::loader::compute_stats(&self.sessions);
        self.refilter();
    }

    /// Take the worker's totals, unless remote rows mean they are not the whole
    /// picture. The worker only ever sees this machine.
    pub(super) fn adopt_stats(&mut self, stats: Stats) {
        self.stats = match self.remotes.is_empty() {
            true => stats,
            false => cctop_core::loader::compute_stats(&self.sessions),
        };
    }

    /// Whether an action that reaches into this machine can apply to a row.
    ///
    /// Returns the refusal to show, or `None` when the row is local. Every
    /// caller is a path that signals a process, deletes a file, or edits one,
    /// and each would otherwise do it to whatever sits at the same path here.
    /// Opening the row is the exception, and goes to its machine instead —
    /// see [`open_remote_selected`](Self::open_remote_selected) — so the
    /// refusal points there.
    pub fn remote_refusal(session: &Session) -> Option<String> {
        let r = session.remote.as_ref()?;
        Some(format!(
            "{} is on {} — R opens it there in a tab; nothing else acts on another machine",
            session.display_label(),
            r.host
        ))
    }

    /// Open the selected remote row in a tab here, working on its machine.
    ///
    /// The tab runs a transport to that machine — a sibling's terminal socket,
    /// or ssh — and the cctop there attaches to the session, resumes it, or
    /// says why not and is a shell in its project: see
    /// [`cctop_core::remote_open`]. Inside this machine's rmux like every other
    /// tab, so the tab outlives a restart of this cctop just as theirs do; the
    /// session itself is in the far machine's rmux and outlives the tab.
    pub(super) fn open_remote_selected(&mut self) {
        use cctop_core::remote_open;
        if self.on_subagent() {
            self.set_status("Open the session, not one of its subagents");
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        let Some(remote) = session.remote.as_ref() else {
            return;
        };
        let host = remote.host.clone();
        let reach = match self.reach_of(&host) {
            Ok(reach) => reach,
            Err(why) => {
                self.set_status(format!("Cannot open it there: {why}"));
                return;
            }
        };
        let provider = session.provider.as_str();
        let exe = std::env::current_exe().unwrap_or_else(|_| "cctop".into());
        let argv = match remote_open::argv(
            &reach,
            &exe,
            provider,
            &session.session_id,
            &session.label_source,
        ) {
            Ok(argv) => argv,
            Err(why) => {
                self.set_status(format!("Not opening it: {why}"));
                return;
            }
        };
        let shown = session.display_label().to_string();
        let name = remote_open::tab_name(&host, provider, &session.session_id);
        // Already open: switch to it, as a resume does — asked of the panes'
        // own record too, for a tab whose rmux name this cctop has no client
        // on yet.
        if let Some(at) = self.tabs.iter().position(|tab| {
            tab.sessions().any(|open| open == name)
                || tab
                    .panes
                    .iter()
                    .any(|p| p.resumed.as_deref() == Some(&name))
        }) {
            self.go_to_tab(at + 1);
            self.set_status(format!("Already open: {shown} on {host}"));
            return;
        }
        // The machine first: the tab bar is where a tab on another machine
        // has to look like one, before anything is typed into it.
        let label = format!(
            "{host} · {}",
            cctop_core::util::truncate(&shown, launch::TAB_LABEL_CHARS)
        );
        let what = format!("{shown} on {host}");
        self.open_remote_tab(&argv, &what, name, label);
    }

    /// Every machine the table reads, by name, and whether it is answering —
    /// the footer's presence dots. A dot, not a sentence: a footer line that
    /// spelled out why crowded out everything beside it, and the reason is
    /// said where it is needed, when one of that machine's rows is opened.
    pub fn remote_presence(&self) -> Vec<(String, bool)> {
        let mut hosts: Vec<(String, bool)> = self
            .remotes
            .keys()
            .chain(self.remote_errors.keys())
            .map(|host| (host.clone(), !self.remote_errors.contains_key(host)))
            .collect();
        hosts.sort();
        hosts.dedup();
        hosts
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
    pub fn skew_of(&self, host: &str) -> Option<cctop_core::fleet::Skew> {
        cctop_core::fleet::skew(
            self.remote_versions.get(host)?,
            cctop_core::update::current_version(),
        )
    }

    /// Take a host's answer to `--version`, and say so once if it differs.
    ///
    /// A server left on an old cctop while the laptop moved on is exactly the
    /// thing nobody notices: the rows keep arriving, just without whatever the
    /// newer build would have sent. So a mismatch is said out loud, once per
    /// host per run, and then left to the HOST column's marker and the Info
    /// panel — which stay true for as long as it does.
    pub fn got_remote_version(&mut self, host: String, probe: cctop_core::fleet::Probe) {
        use cctop_core::fleet::Skew;
        self.remote_versions.insert(host.clone(), probe);
        let Some(skew) = self.skew_of(&host) else {
            return;
        };
        if !self.remote_skew_told.insert(host.clone()) {
            return;
        }
        let local = cctop_core::update::current_version();
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
        use cctop_core::fleet::Skew;
        if self.remote_updating.contains(host) {
            return Some(format!("an update of {host} is already under way"));
        }
        match self.skew_of(host) {
            Some(Skew::Older(_)) => None,
            Some(Skew::Newer(v)) => Some(format!(
                "{host} runs {v}, newer than this cctop — update this one instead"
            )),
            Some(Skew::Missing) => Some(format!("{host} has no cctop to update")),
            None => Some(
                match (self.remote_versions.get(host), self.reach_of(host)) {
                    (Some(cctop_core::fleet::Probe::Version(v)), _) => {
                        format!("{host} already runs {v}")
                    }
                    // Only an ssh host is ever asked its version, and `--update`
                    // is a command run over ssh — so for anything else "not known
                    // yet" would be a wait with no end.
                    (_, Ok(cctop_core::reach::Reach::Ssh(_))) => {
                        format!("{host}'s cctop version is not known yet")
                    }
                    (_, Ok(cctop_core::reach::Reach::Peer(_))) => format!(
                        "{host} is reached through its Cloudflare tunnel, not ssh — run cctop --update there"
                    ),
                    (_, Err(why)) => why.to_string(),
                },
            ),
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
    pub fn remote_host(&self, target: &str) -> Option<&cctop_core::fleet::Host> {
        self.remote_hosts.iter().find(|h| h.target == target)
    }

    /// The way back to a remote row's machine: its `--host`, or a sibling on
    /// the Cloudflare account — see [`cctop_core::reach`]. Every feature that
    /// goes back to the machine for more asks this rather than `remote_hosts`
    /// alone, which knows nothing of the siblings.
    pub fn reach_of(
        &self,
        machine: &str,
    ) -> Result<cctop_core::reach::Reach, cctop_core::reach::Unreached> {
        cctop_core::reach::reach(machine, self.remote_host(machine))
    }

    /// How a remote row's machine is read, for the lines that say so. A
    /// machine with no way back any more was most likely a `--host`.
    pub fn remote_via(&self, machine: &str) -> &'static str {
        match self.reach_of(machine) {
            Ok(reach) => reach.via(),
            Err(cctop_core::reach::Unreached::Offline { .. }) => "through its Cloudflare tunnel",
            Err(cctop_core::reach::Unreached::Unknown { .. }) => "over ssh",
        }
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
        result: Result<String, cctop_core::fleet::UpdateFailure>,
    ) {
        use cctop_core::fleet::UpdateFailure;
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
    use crate::tests::{session, test_app};
    use std::sync::mpsc::channel;
    /// Two live agents in one checkout, both having written the same file —
    /// the arrangement the whole warning exists for.
    #[test]
    fn a_contested_file_reaches_the_footer_and_the_info_panel() {
        let repo = std::env::temp_dir().join(format!("cctop-ui-clash-{}", std::process::id()));
        std::fs::create_dir_all(repo.join(".git")).expect("checkout");
        let dir = repo.to_string_lossy().into_owned();
        let contested = cctop_core::collide::normalise("src/ui/mod.rs", &dir);

        let mut app = test_app();
        app.sessions = vec![session("a", true, &dir), session("b", true, &dir)];
        for s in app.sessions.iter_mut() {
            s.recent_writes = vec![contested.clone()];
        }
        app.collisions = cctop_core::collide::apply(&mut app.sessions);

        // On the rows, so the column can colour and sort by it…
        for s in &app.sessions {
            assert_eq!(s.conflict, Some(cctop_core::collide::Overlap::File));
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
        app.sessions[1].recent_writes = vec![cctop_core::collide::normalise("other.rs", &dir)];
        app.collisions = cctop_core::collide::apply(&mut app.sessions);
        assert_eq!(
            app.sessions[0].conflict,
            Some(cctop_core::collide::Overlap::Directory)
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
        away.remote = Some(cctop_core::session::Remote {
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
            cctop_core::branch::branch_of(remote).as_deref(),
            Some("main")
        );

        // A host that stops answering keeps its rows, and its dot goes red.
        app.remote_errors
            .insert("box".into(), "Permission denied".into());
        assert_eq!(app.remote_presence(), [("box".to_string(), false)]);
    }

    /// One dot per machine: green while it answers, red once it does not.
    #[test]
    fn the_footer_has_a_dot_per_machine() {
        let mut app = test_app();
        assert!(app.remote_presence().is_empty());

        for (host, dir) in [("box", "/srv/work"), ("bench", "/srv/other")] {
            let mut row = session(host, true, dir);
            row.remote = Some(cctop_core::session::Remote {
                host: host.into(),
                ..Default::default()
            });
            app.remotes.insert(host.into(), vec![row]);
        }
        app.remote_errors.insert("box".into(), "timeout".into());
        // A machine that has never answered is still listed, as down.
        app.remote_errors.insert("asleep".into(), "offline".into());
        assert_eq!(
            app.remote_presence(),
            [
                ("asleep".to_string(), false),
                ("bench".to_string(), true),
                ("box".to_string(), false),
            ]
        );
    }

    /// A server left behind the laptop: said once, marked on its rows, and
    /// updated only through a confirmation that names the command.
    #[test]
    fn an_older_remote_is_said_once_marked_and_offered_an_update() {
        use cctop_core::fleet::{Host, Probe, UpdateFailure};
        let (tx, rx) = channel();
        let mut app = App::new(Plan::Retail, tx);
        let host = Host::parse("box").expect("a host");
        app.remote_hosts = vec![host.clone()];
        let mut s = session("away", true, "/srv/work");
        s.remote = Some(cctop_core::session::Remote {
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
        app.got_remote_version(
            "box".into(),
            cctop_core::fleet::Probe::Version("999.0.0".into()),
        );
        let said = app.toasts.iter().next().expect("a toast").text.clone();
        assert!(said.contains("cctop --update` here"), "{said}");
        let why = app.remote_update_refusal("box").expect("refused");
        assert!(why.contains("update this one"), "{why}");

        // And a host with no cctop at all is told how to point at one.
        app.got_remote_version("bare".into(), cctop_core::fleet::Probe::Missing);
        let said = app.toasts.iter().next().expect("a toast").text.clone();
        assert!(said.contains("--host bare:/path/to/cctop"), "{said}");
    }

    /// With no host configured the column is one repeated word down every row,
    /// so it is hidden — through the user's own mechanism, so the two cannot
    /// disagree about what is on screen.
    #[test]
    fn the_host_column_stays_off_a_single_machine() {
        let ids = |hidden: &[ColumnId]| -> Vec<ColumnId> {
            columns::visible_columns(300, hidden, &[])
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
        s.remote = Some(cctop_core::session::Remote {
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
                // Resume and Attach open it on its own machine.
                menu::Action::Read
                | menu::Action::Expand
                | menu::Action::Mark
                | menu::Action::Resume
                | menu::Action::Attach => {
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

    /// `R` on a remote row opens it on its machine when this cctop can reach
    /// that machine, says so plainly when it cannot, refuses an id that did
    /// not come from a harness, and goes to the tab already open on it.
    #[test]
    fn a_remote_row_opens_on_its_machine_or_says_why_not() {
        let mut app = test_app();
        let mut s = session("away", false, "/srv/work");
        s.remote = Some(cctop_core::session::Remote {
            host: "box".into(),
            ..Default::default()
        });
        app.sessions = vec![s];
        app.refilter();
        app.selected = 0;

        // Not a sibling and not a --host: nothing to reach it by.
        app.resume_selected();
        let said = app.status().expect("a reason").to_owned();
        assert!(said.contains("box is neither a --host"), "{said}");

        // A --host, but an id that is not one: refused before ssh is built.
        app.remote_hosts = vec![cctop_core::fleet::Host::parse("box").expect("host")];
        app.sessions[0].session_id = "-oProxyCommand=x".into();
        app.attach_selected();
        let said = app.status().expect("a reason").to_owned();
        assert!(said.contains("Not opening it"), "{said}");

        // Already open: switched to, not opened twice.
        app.sessions[0].session_id = "abc".into();
        let (mut child, pid) = cctop_core::shim::test_session(&["sh", "-c", "sleep 30"], (80, 24));
        let mut pane = tabs::Pane::view_of(pid, "ssh".into()).expect("attach");
        pane.resumed = Some(cctop_core::remote_open::tab_name("box", "claude", "abc"));
        app.tabs.push(tabs::Tab::new(pane));
        app.tab = 0;
        app.resume_selected();
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.tab, 1);
        let said = app.status().expect("said").to_owned();
        assert!(
            said.contains("Already open") && said.contains("box"),
            "{said}"
        );
        let _ = child.kill();
        let _ = child.wait();
        let _ = cctop_core::shim::socket_path(pid).map(std::fs::remove_file);
    }

    /// The launcher opened on a remote row from an ssh host offers that row's
    /// project there; on a local row it stays where cctop was started.
    #[test]
    fn the_launcher_on_a_remote_row_starts_in_its_project_there() {
        let mut app = test_app();
        app.remote_hosts = vec![cctop_core::fleet::Host::parse("box").expect("host")];
        let mut s = session("away", false, "/srv/work");
        s.remote = Some(cctop_core::session::Remote {
            host: "box".into(),
            ..Default::default()
        });
        app.sessions = vec![s, session("here", false, "/tmp")];
        app.refilter();
        let remote_at = app
            .visible
            .iter()
            .position(|r| matches!(r, Row::Session(0)))
            .expect("the remote row");
        app.selected = remote_at;
        app.launch_prompt(LaunchInto::Tab);
        let target = app.launch_remote.clone().expect("prefilled");
        assert_eq!(target.spelled(), "box:/srv/work");

        app.mode = Mode::List;
        app.selected = 1 - remote_at;
        app.launch_prompt(LaunchInto::Tab);
        assert!(app.launch_remote.is_none());
    }

    /// A row from a sibling on the Cloudflare account — no `--host` anywhere —
    /// opens its conversation, read from the sibling, instead of being refused
    /// as a machine this run is not connected to.
    #[test]
    fn a_siblings_row_opens_its_conversation() {
        use cctop_core::peer::{Peer, add_known, fake};
        let (url, seen) = fake::sibling(
            200,
            r#"{"supported":true,"earlier":0,"turns":[{"seq":0,"role":"user","kind":"message","ts":"","text":"library suggestions","tools":[]}]}"#,
        );
        add_known(std::sync::Arc::new(Peer::fixed(
            "ui-test-computer36",
            &url,
            fake::SECRET,
        )));
        let (tx, rx) = channel();
        let mut app = App::new(Plan::Retail, tx);
        let mut s = session("away", true, "/home/me/work");
        s.remote = Some(cctop_core::session::Remote {
            host: "ui-test-computer36".into(),
            ..Default::default()
        });
        app.sessions = vec![s];
        app.refilter();
        app.selected = 0;
        app.open_conversation();
        assert_eq!(app.mode, Mode::Conversation, "{:?}", app.status());

        // The worker's half: what the view asked for, asked of the sibling.
        let (session, reach, before, agent) = std::iter::from_fn(|| rx.try_recv().ok())
            .find_map(|r| match r {
                Request::Chat {
                    session,
                    reach,
                    before,
                    agent,
                } => Some((session, reach, before, agent)),
                _ => None,
            })
            .expect("the read went to the worker");
        let reach = reach.expect("a remote row is read where it lives");
        assert_eq!(reach.via(), "through its Cloudflare tunnel");
        let conversation = reach
            .chat(&session.session_id, before, agent.as_deref())
            .expect("the sibling answered");
        assert_eq!(conversation.turns[0].text, "library suggestions");
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            [format!("/api/chat/{}", session.session_id)]
        );
        app.got_chat(session.key(), None, None, Ok(Box::new(conversation)));
        assert!(
            app.chat
                .as_ref()
                .and_then(|v| v.conversation.as_ref())
                .is_some()
        );
    }

    /// A row whose machine is neither says which of the two it is not, and a
    /// sibling that is down says that instead.
    #[test]
    fn an_unreachable_row_says_why() {
        use cctop_core::peer::{Peer, add_known};
        add_known(std::sync::Arc::new(
            Peer::fixed("ui-test-asleep", "http://127.0.0.1:9", "made-up")
                .failing("offline — its tunnel has no connector"),
        ));
        let mut app = test_app();
        let row = |host: &str| {
            let mut s = session(host, true, "/home/me/work");
            s.remote = Some(cctop_core::session::Remote {
                host: host.into(),
                ..Default::default()
            });
            s
        };
        for (machine, expected) in [
            (
                "ui-test-gone",
                "ui-test-gone is neither a --host of this run nor a cctop on its Cloudflare account",
            ),
            (
                "ui-test-asleep",
                "ui-test-asleep is not answering: offline — its tunnel has no connector",
            ),
        ] {
            app.sessions = vec![row(machine)];
            app.refilter();
            app.selected = 0;
            app.open_conversation();
            assert_eq!(app.mode, Mode::List);
            assert!(app.chat.is_none());
            let said = app.status().expect("a reason").to_string();
            assert!(said.ends_with(expected), "{said}");
            assert!(!said.contains("not connected"), "{said}");
        }
        // The same reason is why `--update` is not offered there.
        assert_eq!(
            app.remote_update_refusal("ui-test-gone").as_deref(),
            Some(
                "ui-test-gone is neither a --host of this run nor a cctop on its Cloudflare account"
            )
        );
    }
}
