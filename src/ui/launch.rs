//! Starting an agent, resuming one, and attaching to one already running.
//!
//! All three end in the same place — a pane in a tab — and differ only in what
//! they decide first: whether the multiplexer owns the process, whether a
//! session id is being resumed, and whether the user must be asked to install
//! rmux before any of it can happen. Keeping them together is what stops those
//! decisions from being made three slightly different ways.

use super::*;

/// Everything [`App::open_tab`] needs beyond the command itself.
///
/// A struct rather than six more parameters: they had already outgrown a
/// readable call, and at a call site `verb: "Attached to"` says what the
/// fifth positional string never did.
struct NewTab<'a> {
    cwd: Option<std::path::PathBuf>,
    /// The thing being opened, as a status message would name it.
    what: &'a str,
    own: tabs::Own,
    /// What happened, for the status line: resumed, reattached, attached.
    verb: &'a str,
    /// The session this pane resumes, when it resumes one.
    resumed: Option<String>,
    /// What to call the tab, when the command would call it badly.
    ///
    /// A launch is its command — a `claude` tab is called `claude`, which is
    /// both true and short. A *resume* is not: its command carries the session
    /// id, and `claude --resume 4ebf1ab4-2ef8-4fb2-a7d5-d445b5026dc9` is 45
    /// characters of tab bar whose only variable part is a uuid nobody reads.
    /// The caller that knows which session this is passes its name instead.
    label: Option<String>,
    /// The account this pane runs as, when it is one cctop chose — so the pane
    /// border reports that account's limits rather than the default's.
    profile: Option<String>,
}

/// How much of a session's name a resumed tab's label carries.
///
/// The bar elides labels itself once it is crowded, but only then — with two
/// tabs open there is room for a whole title, and a title can be a sentence.
/// This is the point past which a tab name stops identifying the session and
/// starts being its own paragraph.
pub(super) const TAB_LABEL_CHARS: usize = 24;

/// How long a freshly launched agent is given before a handoff brief is typed
/// at it.
///
/// Only reached by a harness that takes no opening prompt on its command line —
/// everything in [`handoff::opening_argv`](crate::handoff::opening_argv) is
/// handed the brief as an argument instead, because no delay is long enough to
/// win that race reliably. Too short and the line is lost; too long and the user
/// is left looking at an idle agent wondering whether the handoff worked.
const HANDOFF_SETTLE: Duration = Duration::from_secs(3);

impl App {
    /// Open the launcher, remembering where the pick should go and which
    /// directory it should start in.
    pub fn launch_prompt(&mut self, into: LaunchInto) {
        if matches!(into, LaunchInto::Split { .. }) && self.active_tab().is_none() {
            self.set_status("Nothing to split — open a tab first");
            return;
        }
        let offer = tabs::choices(&self.open_rmux());
        if offer.is_empty() {
            self.set_status("No agent found in PATH, and $SHELL is not set");
            return;
        }
        self.launch_offer = offer;
        self.launch_into = into;
        self.launch_cursor = 0;
        // A split lands next to an agent already working somewhere; a fresh tab
        // starts where cctop itself was invoked. The selected dashboard row is
        // for inspecting or resuming that session, not an implicit cwd switch.
        self.launch_cwd = match into {
            LaunchInto::Split { .. } => self.launch_cwd.clone(),
            LaunchInto::Tab => self.launch_root.clone(),
        };
        self.mode = Mode::Launch;
    }

    /// Write the selected session's context brief and offer it to a new agent.
    ///
    /// This is the cross-harness counterpart to `R`. Resuming puts the *same*
    /// harness back on the *same* transcript; a handoff carries what the session
    /// was doing across to a different agent entirely, which is the one thing no
    /// harness can do for itself — each one can only read its own transcripts.
    ///
    /// The brief is written before the launcher opens so a failure to write it
    /// is reported instead of starting an agent that then has nothing to read.
    pub(super) fn handoff_selected(&mut self) {
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        // The panels already hold the selected session's extraction; a brief
        // built while the row is still loading, or while a subagent row owns the
        // panels, falls back to the header alone rather than to another
        // session's data.
        let data = match self.panel_key == session.key() {
            true => self.panel_data.as_ref(),
            false => None,
        };
        let brief = crate::handoff::build(&session, data);
        let path = match crate::handoff::write(&brief) {
            Ok(path) => path,
            Err(error) => {
                self.set_status(format!("Could not write the handoff brief: {error}"));
                return;
            }
        };
        self.pending_brief = Some(path);
        self.pending_fork = crate::handoff::forkable(&session).map(std::path::Path::to_path_buf);
        // The receiving agent belongs in the directory the work is in, whatever
        // row the cursor moves to while the launcher is up.
        self.launch_prompt(LaunchInto::Tab);
        // `launch_prompt` bails on its own when nothing can be launched, and
        // leaving a brief pending for a launcher that never opened would attach
        // it to the next unrelated agent instead.
        if self.mode != Mode::Launch {
            self.pending_brief = None;
            self.pending_fork = None;
            return;
        }
        self.set_status(format!(
            "Handing off {} — pick who takes it",
            brief.summary()
        ));
    }

    /// Deliver a brief to the agent it was launched for, once that agent has had
    /// long enough to start reading its keyboard.
    pub(super) fn tick_handoff(&mut self) {
        let Some((pid, line, due)) = self.handoff_send.clone() else {
            return;
        };
        if Instant::now() < due {
            return;
        }
        self.handoff_send = None;
        match crate::inject::send_line(pid, &line) {
            Ok(()) => self.set_status("Handed the brief over"),
            // The brief is on disk either way, so the failure is recoverable by
            // hand — say where it is rather than only that this did not work.
            Err(error) => self.set_status(format!("Could not hand the brief over: {error}")),
        }
    }

    /// Reopen the selected session in a tab of its own.
    ///
    /// This is the one way into a session cctop did not start. `a` shows an
    /// agent's live terminal, but only for the agents cctop hosts — there is no
    /// pty to borrow otherwise. Resuming instead starts a *new* agent and hands
    /// it the transcript, which is what the harnesses themselves offer and works
    /// whether the session ended an hour ago or is running in another window.
    pub(super) fn resume_selected(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let Some(argv) = session.resume_argv() else {
            self.set_status(format!(
                "{} sessions cannot be resumed from a shell",
                session.provider.as_str()
            ));
            return;
        };
        if !crate::shim::is_command(&argv[0]) {
            self.set_status(format!("{} is not installed on this machine", argv[0]));
            return;
        }
        // Two agents appending to one transcript is not something any of the
        // harnesses coordinate, so the running case asks first.
        if session.is_running() {
            self.mode = Mode::ResumeConfirm;
            return;
        }
        self.resume_now();
    }

    /// Resume the selected session, having decided that it should be.
    pub(super) fn resume_now(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let Some(argv) = session.resume_argv() else {
            return;
        };
        // Resumed under the account the transcript lives in, not under
        // whatever the launcher last chose. For Codex this is the difference
        // between resuming and not: a session id under `~/.codex-work` does not
        // exist under `~/.codex`, so `codex resume <id>` would start a blank
        // session and report nothing wrong. The row already knows which account
        // it came from — `Session::profile` is stamped from the transcript path.
        let profile = session
            .profile
            .as_deref()
            .and_then(|name| crate::config::profile_named(session.provider, name));
        let argv = match profile {
            Some(profile) => crate::config::argv_under_profile(argv, profile),
            None => argv,
        };
        // The transcript is full of paths relative to where the agent ran, so a
        // resumed session belongs in the same directory.
        let cwd = session.work_dir();
        let what = format!("{} · {}", session.display_label(), argv[0]);
        // What the tab is called: the agent, then which session it is. The
        // command cannot say the second half without spelling out a uuid, and
        // the uuid is the half nobody reads.
        let label = format!(
            "{} · {}",
            argv[0],
            crate::util::truncate(session.display_label(), TAB_LABEL_CHARS)
        );
        // Named after the session, so resuming it a second time reattaches to
        // the agent already doing it rather than starting a rival.
        let rmux = crate::rmux::name_for_session(session.provider.as_str(), &session.session_id);

        // Already on screen: switch to it. rmux would attach a second client to
        // the same agent, which works but leaves two panes fighting over one
        // window's size for no reason.
        //
        // Asked of `resumed` as well as of `rmux`, because without rmux
        // installed every pane's `rmux` is `None` and the question would answer
        // "no" every time — putting a second agent on one transcript, which is
        // the thing `ResumeConfirm` exists to warn about and which would happen
        // here with no warning at all, the session having already stopped.
        if let Some(at) = self.tabs.iter().position(|tab| {
            // `sessions`, not just the panes: the tab may be one this cctop has
            // no client on — another cctop's, or one it detached from itself —
            // and resuming into a second agent is exactly what this guards.
            tab.sessions().any(|name| name == rmux)
                || tab
                    .panes
                    .iter()
                    .any(|p| p.resumed.as_deref() == Some(&rmux))
        }) {
            self.go_to_tab(at + 1);
            self.set_status(format!("Already open: {what}"));
            return;
        }

        let Some(own) = self.own_preferring_rmux(Deferred::Resume, || rmux.clone()) else {
            return;
        };
        // Reattaching is not resuming: the agent was never gone, so saying
        // "resumed" would misdescribe what just happened.
        let verb = match &own {
            tabs::Own::Tmux(name) if crate::rmux::exists(name) => "Reattached to",
            _ => "Resumed",
        };
        self.open_tab(
            &argv,
            NewTab {
                cwd,
                what: &what,
                own,
                verb,
                resumed: Some(rmux),
                label: Some(label),
                profile: profile.map(|p| p.name.clone()),
            },
        );
    }

    /// Where the agent about to start should live, offering to install rmux if
    /// that is the only reason it would not be rmux-backed.
    ///
    /// `None` means the question is on screen and the caller must stop. The
    /// launch is not held anywhere in the meantime — [`Deferred`] records only
    /// which of the two entry points to run again once there is an answer.
    ///
    /// The silent fallback is kept for every machine where the question cannot
    /// be usefully asked — no package manager, or no way to reach root. rmux is
    /// how this is *better*, not how it works, and such a machine gets exactly
    /// the behaviour cctop had before rather than a complaint about a program
    /// the user never asked for. The offer exists for the machine where the
    /// fallback would instead quietly cost the user a feature one keypress away.
    pub(super) fn own_preferring_rmux(
        &mut self,
        deferred: Deferred,
        name: impl FnOnce() -> String,
    ) -> Option<tabs::Own> {
        if crate::rmux::available() {
            return Some(tabs::Own::Tmux(name()));
        }
        // Asked in this order so that installing rmux in another window still
        // works: `available` above is the live check, and neither a previous
        // "no" nor a running install is consulted until it has said no.
        if self.rmux_declined || self.rmux_installing.is_some() {
            return Some(tabs::Own::Cctop);
        }
        // No package manager to offer means there is nothing to ask about, so
        // this is the plain fallback rather than a refusal: `?` here would
        // return `None`, which the caller reads as "the launch is waiting on an
        // answer" — and no answer would ever come, so the tab never opened.
        let Some(install) = crate::rmux::installer() else {
            return Some(tabs::Own::Cctop);
        };
        self.rmux_install = Some(install);
        self.rmux_deferred = Some(deferred);
        self.mode = Mode::TmuxInstall;
        self.needs_redraw = true;
        None
    }

    /// Answer the rmux offer: run the install in a pane, or give up on rmux for
    /// this run and start the agent on cctop's own pty.
    pub(super) fn rmux_install_answer(&mut self, install: bool) {
        self.mode = Mode::List;
        let Some(offer) = self.rmux_install.take() else {
            return;
        };
        if !install {
            self.rmux_declined = true;
            self.run_deferred_launch();
            return;
        }
        // In a pane, not a subprocess: `sudo` wants a password, and a pane is a
        // pty the user can type it into. It also puts the package manager's
        // output somewhere it can be read, which is the difference between a
        // failed install and a tab that closed for no stated reason.
        match tabs::Pane::launch(&offer.argv, None, tabs::Own::Cctop) {
            Ok(pane) => {
                self.rmux_installing = Some(pane.pid);
                self.tabs.push(tabs::Tab::new(pane));
                self.go_to_tab(self.tabs.len());
                self.set_status(format!("Installing rmux with {}", offer.manager));
            }
            Err(error) => {
                self.set_status(format!("Could not run the install: {error}"));
                self.rmux_declined = true;
                self.run_deferred_launch();
            }
        }
    }

    /// Watch a running install to whichever of its two ends it reaches.
    ///
    /// Called from the poll loop after panes are reaped, so "the pane is gone"
    /// is already true here rather than true one tick later.
    pub(super) fn poll_rmux_install(&mut self) {
        let Some(pid) = self.rmux_installing else {
            return;
        };
        if crate::rmux::available() {
            self.rmux_installing = None;
            self.set_status("rmux installed");
            self.run_deferred_launch();
            return;
        }
        // The pane is gone and rmux is still not here: the install failed, or
        // the user closed it. Either way the launch has waited long enough, and
        // it goes where it would have gone had nothing been offered.
        let open = self
            .tabs
            .iter()
            .flat_map(|tab| tab.panes.iter())
            .any(|pane| pane.pid == pid);
        if !open {
            self.rmux_installing = None;
            self.rmux_declined = true;
            if self.rmux_deferred.is_some() {
                self.set_status("rmux was not installed — starting without it");
            }
            self.run_deferred_launch();
        }
    }

    /// Re-run whichever launch stopped to ask about rmux.
    pub(super) fn run_deferred_launch(&mut self) {
        match self.rmux_deferred.take() {
            Some(Deferred::Resume) => self.resume_now(),
            Some(Deferred::Launch) => self.launch_selected(),
            None => {}
        }
    }

    /// Start `argv` in a new tab, reporting what happened either way.
    ///
    /// `resumed` names the session the tab is going back to, when it is going
    /// back to one — what the next resume of it looks itself up by.
    fn open_tab(&mut self, argv: &[String], tab: NewTab<'_>) {
        let NewTab {
            cwd,
            what,
            own,
            verb,
            resumed,
            label,
            profile,
        } = tab;
        let mut pane = match tabs::Pane::launch(argv, cwd.as_deref(), own) {
            Ok(pane) => pane,
            Err(error) => {
                self.set_status(format!("Could not start {what}: {error}"));
                return;
            }
        };
        pane.resumed = resumed;
        pane.profile = profile;
        if let Some(label) = label {
            pane.label = label;
        }
        // Worth saying once per tab: it changes what quitting cctop means.
        let kept = match pane.outlives_cctop() {
            true => " — it will outlive cctop",
            false => "",
        };
        self.tabs.push(tabs::Tab::new(pane));
        self.go_to_tab(self.tabs.len());
        let where_ = cwd
            .map(|dir| format!(" in {}", crate::util::tildify(&dir.to_string_lossy())))
            .unwrap_or_default();
        self.set_status(format!("{verb} {what}{where_}{kept}"));
    }

    /// The rmux sessions this cctop already has a pane onto.
    pub fn open_rmux(&self) -> Vec<String> {
        self.tabs
            .iter()
            .flat_map(tabs::Tab::sessions)
            .map(str::to_string)
            .collect()
    }

    /// What the launcher is offering.
    pub fn launch_choices(&self) -> &[tabs::Choice] {
        &self.launch_offer
    }

    /// The argv that resumes a new agent onto a copy of the session being
    /// handed over, when that is possible and `argv` is the agent that can read
    /// it.
    ///
    /// The copy lands in the *receiving* account's directory, which is not
    /// always the sending one's: handing a personal session to a work login has
    /// to put the transcript where that login will look for it.
    ///
    /// A failure to copy is reported and answered with `None`, which puts the
    /// launch back on the brief — the handoff still happens, with less of the
    /// conversation in it.
    pub(super) fn fork_pending(&mut self, argv: &[String]) -> Option<Vec<String>> {
        let transcript = self.pending_fork.clone()?;
        if crate::handoff::command_of(argv) != Some("claude") {
            return None;
        }
        let profile = self.chosen_profile(Provider::Claude);
        let config_dir = profile
            .map(|p| p.dir.clone())
            .unwrap_or_else(|| crate::config::CLAUDE_CONFIG_DIR.clone());
        match crate::handoff::fork(&transcript, &config_dir) {
            Ok(id) => {
                let argv = vec!["claude".to_string(), "--resume".to_string(), id];
                Some(match profile {
                    Some(profile) => crate::config::argv_under_profile(argv, profile),
                    None => argv,
                })
            }
            Err(error) => {
                self.set_status(format!("Could not copy the transcript: {error}"));
                None
            }
        }
    }

    /// Whether the launcher's pick is an agent already running somewhere.
    ///
    /// Reattaching lands wherever that agent already is, so a directory typed
    /// for it would be accepted and then ignored — which is worse than the key
    /// not being offered.
    pub(super) fn launch_is_reattach(&self) -> bool {
        matches!(
            self.launch_offer.get(self.launch_cursor),
            Some(tabs::Choice::Waiting(_))
        )
    }

    /// Start the launcher's pick.
    pub fn launch_selected(&mut self) {
        let Some(choice) = self.launch_offer.get(self.launch_cursor).cloned() else {
            return;
        };
        let cwd = self.launch_cwd.clone();
        // Read off the choice rather than the argv below, which by then carries
        // the `env VAR=value` prefix a profile is passed through.
        let starting = match &choice {
            tabs::Choice::Start(argv) => Self::profile_provider(argv),
            tabs::Choice::Waiting(_) => None,
        };
        let (argv, own) = match &choice {
            // Reattaching: the agent chose its own command long ago, and the
            // argv here only names the tab.
            tabs::Choice::Waiting(agent) => (
                vec![choice.label()],
                tabs::Own::TmuxExisting(agent.name.clone()),
            ),
            // A fresh agent has no identity to be idempotent about — two
            // `claude` tabs are two agents — so this takes the next free name
            // rather than a derived one.
            tabs::Choice::Start(argv) => {
                let own = self.own_preferring_rmux(Deferred::Launch, || {
                    crate::rmux::free_name(&tabs::label_of(argv))
                });
                // The offer went up instead. This runs again from the top when
                // it is answered, and the launcher's snapshot is still here to
                // run it from.
                let Some(own) = own else { return };
                (self.with_profile(argv.clone()), own)
            }
        };
        // The offer is a snapshot, and an agent can finish in the time the modal
        // is up. Attaching to a session that has gone spawns a client that exits
        // at once — a tab that flickers and vanishes, where the truth is simply
        // that the agent ended while being looked at.
        if let tabs::Choice::Waiting(agent) = &choice
            && !crate::rmux::exists(&agent.name)
        {
            self.set_status(format!("{} has ended", choice.label()));
            return;
        }

        // A handoff goes to an agent that is starting fresh. Reattaching lands
        // in a conversation already under way, where a "read this and continue"
        // line would interrupt whatever it is doing mid-turn.
        let fresh = matches!(choice, tabs::Choice::Start(_));
        // Claude to Claude the conversation itself is handed over rather than a
        // summary of it, the receiving agent being resumed onto a copy of the
        // transcript. Everything else gets the brief, which is the only form it
        // can read.
        let forked = fresh.then(|| self.fork_pending(&argv)).flatten();
        let carrying_conversation = forked.is_some();
        let argv = forked.unwrap_or(argv);
        // Only where the fork did not happen: an agent that cannot read the
        // transcript, or one whose copy could not be written.
        let brief = match fresh && !carrying_conversation {
            true => self.pending_brief.clone(),
            false => None,
        };
        let line = brief.as_deref().map(crate::handoff::prompt_for);
        // Handed over in the argv wherever the harness takes an opening prompt;
        // `opening_argv` says why that is not the same as typing it.
        let opening = line
            .as_deref()
            .and_then(|line| crate::handoff::opening_argv(&argv, line));
        let argv = &argv;
        let mut pane =
            match tabs::Pane::launch(opening.as_ref().unwrap_or(argv), cwd.as_deref(), own) {
                Ok(pane) => pane,
                Err(error) => {
                    self.set_status(format!("Could not start {}: {error}", tabs::label_of(argv)));
                    return;
                }
            };
        // The profile is only knowable here: it reached the agent as an
        // environment variable, which nothing downstream can read back. A fresh
        // agent takes the account the launcher was showing; one being reattached
        // takes the one it was started under, which the sweep read back off its
        // rmux session.
        pane.profile = match &choice {
            tabs::Choice::Start(_) => self.launch_profile().map(|p| p.name.clone()),
            tabs::Choice::Waiting(agent) => agent.profile.clone(),
        };
        // The tab is named after the agent, not after the brief it was handed:
        // `Pane::launch` names it from the argv it was given, and that argv now
        // ends in a paragraph — or, for a fork, in the uuid of the copy, which
        // is the half of a resume nobody reads.
        if opening.is_some() {
            pane.label = tabs::label_of(argv);
        } else if carrying_conversation {
            pane.label = "claude".to_string();
        }
        let label = pane.label.clone();
        let mut carried = "";
        if carrying_conversation {
            // Both are spent: the copy is what the agent is reading, and the
            // brief that was written alongside it has no reader left.
            self.pending_fork = None;
            self.pending_brief = None;
            carried = " with the conversation";
        }
        if let Some(line) = line {
            self.pending_brief = None;
            match opening.is_some() {
                // Already in the agent's argv — there is nothing left to send,
                // and the agent opens on the brief instead of on an empty prompt.
                true => carried = " with the brief",
                // No prompt argument to use, so it goes the old way: typed once
                // the agent has had long enough to be listening.
                false => {
                    self.handoff_send = Some((pane.pid, line, Instant::now() + HANDOFF_SETTLE));
                }
            }
        }
        let kept = match pane.outlives_cctop() {
            true => " — it will outlive cctop",
            false => "",
        };
        match self.launch_into {
            LaunchInto::Split { stacked } => {
                let Some(tab) = self.active_tab() else { return };
                tab.stacked = stacked;
                tab.panes.push(pane);
                tab.focus = tab.panes.len() - 1;
            }
            LaunchInto::Tab => {
                self.tabs.push(tabs::Tab::new(pane));
                self.go_to_tab(self.tabs.len());
            }
        }
        // Reattaching is not starting, and it does not land in the launcher's
        // directory: the agent has been working somewhere since before any of
        // this and stays there. Saying "Started ... in ~/here" would be wrong
        // twice over.
        self.set_status(match &choice {
            tabs::Choice::Waiting(agent) => {
                let at = agent
                    .cwd
                    .as_ref()
                    .map(|dir| format!(" in {}", crate::util::tildify(&dir.to_string_lossy())))
                    .unwrap_or_default();
                format!("Reattached to {label}{at} — it was never gone")
            }
            tabs::Choice::Start(_) => {
                let where_ = cwd
                    .map(|dir| format!(" in {}", crate::util::tildify(&dir.to_string_lossy())))
                    .unwrap_or_default();
                // Only for a fresh Codex, and only where its hooks are waiting
                // to be trusted. A reattached one is deliberately left out: it
                // has been running since before whatever is installed now, so
                // the answer for it is a restart rather than a keystroke.
                let trust = match starting {
                    Some(Provider::Codex) => self.codex_trust_hint(
                        crate::hook::codex_hooks_installed(self.hook_project().as_deref()),
                    ),
                    _ => "",
                };
                format!("Started {label}{where_}{carried}{kept}{trust}")
            }
        });
    }

    /// Put the selected agent's own terminal on screen, in a tab of its own.
    ///
    /// Two ways in, because there are two ways an agent's terminal can belong to
    /// cctop. A shim holding a pty has a copy of the output to give away; an agent
    /// handed to rmux has none, and is reached by becoming another of its clients
    /// instead. Either way this only *looks* at the agent — closing the pane
    /// detaches from it and never ends it.
    pub(super) fn attach_selected(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let label = format!("{} · {}", session.abbrev_label, session.model);
        let title = session.display_label().to_string();
        // How a resume names this same session. Recorded on the pane below so
        // that `R` afterwards finds the agent already on screen instead of
        // starting a second one on one transcript — `a` and `R` reach the same
        // agent by different routes, and only this makes them agree.
        let resumed = crate::rmux::name_for_session(session.provider.as_str(), &session.session_id);
        let Some(pid) = session.root_pid() else {
            self.set_status("Selected session has no local process");
            return;
        };
        if self.open_view(pid, label.clone()) {
            return;
        }
        // Started by cctop and then handed to rmux. Without this the message
        // below would say cctop did not start an agent cctop started, and send
        // the user to relaunch something that is already running.
        if let Some(name) = crate::rmux::holding(pid) {
            // A second client onto one session leaves the two panes arguing over
            // one window's size, so an agent already on screen is switched to.
            if let Some(at) = self
                .tabs
                .iter()
                .position(|tab| tab.sessions().any(|open| open == name))
            {
                self.go_to_tab(at + 1);
                self.set_status(format!("Already open: {label}"));
                return;
            }
            self.open_tab(
                &[title],
                NewTab {
                    cwd: None,
                    what: &label,
                    own: tabs::Own::TmuxExisting(name),
                    verb: "Attached to",
                    resumed: Some(resumed),
                    // Already a bare agent name, so the command names it right.
                    label: None,
                    // Somebody else started it; its account is whatever they
                    // chose, which the rmux session name does not record.
                    profile: None,
                },
            );
            return;
        }
        self.set_status(
            "Only sessions started by cctop can be attached — start them as `cctop claude`",
        );
    }

    /// Go back to the agent this cctop launched.
    ///
    /// Without this, F12 would be a one-way door: a freshly started agent has
    /// written no transcript yet, so it has no row in the table to press `a` on.
    pub(super) fn attach_hosted(&mut self) {
        let Some((pid, label)) = self.hosted.clone() else {
            self.set_status("No agent was launched by this cctop — start one as `cctop claude`");
            return;
        };
        if !self.open_view(pid, label) {
            self.set_status("The agent's terminal is gone");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::test_app;
    /// Regression: the "already open" guard asked only about `rmux`, which is
    /// `None` on every pane when rmux is not installed — so `R` on a session
    /// already resumed in a tab started a second agent on the one transcript,
    /// and being stopped, it did so without even the confirmation.
    #[cfg(target_os = "linux")]
    #[test]
    fn resuming_a_session_already_in_a_tab_goes_to_that_tab() {
        let (mut child, pid) = crate::shim::test_session(&["sh", "-c", "sleep 30"], (80, 24));
        let mut pane = tabs::Pane::view_of(pid, "claude".into()).expect("attach");
        // What a resumed tab records regardless of who carries the agent. The
        // pane has no rmux, standing in for a machine without it.
        pane.resumed = Some(crate::rmux::name_for_session("claude", "abc"));
        assert!(pane.rmux.is_none());

        let mut app = test_app();
        app.sessions
            .push(crate::session::Session::new(Provider::Claude, "abc".into()));
        app.visible = vec![Row::Session(0)];
        app.selected = 0;
        app.tabs.push(tabs::Tab::new(pane));
        app.tab = 0;

        app.resume_now();

        assert_eq!(
            app.tabs.len(),
            1,
            "a second agent was put on one transcript"
        );
        assert_eq!(app.tab, 1, "the tab already holding it was not shown");
        let (status, _) = app.status.clone().expect("nothing was said");
        assert!(status.contains("Already open"), "{status}");

        let _ = child.kill();
        let _ = child.wait();
        let _ = crate::shim::socket_path(pid).map(std::fs::remove_file);
    }

    /// The three ways the ownership decision can go, since only one of them is
    /// new: rmux present is unchanged, rmux absent and uninstallable is the old
    /// silent fallback, and only rmux absent but installable stops to ask.
    #[test]
    fn ownership_asks_only_when_rmux_could_actually_be_installed() {
        let mut app = test_app();
        let own = app.own_preferring_rmux(Deferred::Launch, || "cctop-x".into());
        match (crate::rmux::available(), crate::rmux::installer()) {
            (true, _) => assert!(matches!(own, Some(tabs::Own::Tmux(_)))),
            (false, Some(_)) => {
                assert!(own.is_none(), "the launch waits for the answer");
                assert_eq!(app.mode, Mode::TmuxInstall);
            }
            (false, None) => assert!(matches!(own, Some(tabs::Own::Cctop))),
        }
    }

    /// One "no" holds for the run. Asking again on the next tab would make
    /// declining cost more than accepting, which is not offering a choice.
    #[test]
    fn a_declined_offer_is_not_made_again() {
        let mut app = test_app();
        app.rmux_declined = true;
        let own = app.own_preferring_rmux(Deferred::Launch, || "cctop-x".into());
        assert!(own.is_some(), "the launch goes ahead without asking");
        assert_ne!(app.mode, Mode::TmuxInstall);
    }

    /// Declining still starts the agent — the offer interrupted a launch, and
    /// saying no to rmux is not saying no to the agent.
    #[test]
    fn declining_the_offer_releases_the_launch() {
        let mut app = test_app();
        app.mode = Mode::TmuxInstall;
        app.rmux_install = Some(crate::rmux::Install {
            manager: "apt",
            argv: vec!["sh".into(), "-c".into(), "apt-get install -y rmux".into()],
        });
        app.rmux_deferred = Some(Deferred::Launch);

        app.rmux_install_answer(false);

        assert!(app.rmux_declined);
        assert!(app.rmux_install.is_none());
        assert!(
            app.rmux_deferred.is_none(),
            "the launch was run, not dropped"
        );
        assert_eq!(app.mode, Mode::List);
    }

    /// The failure that would otherwise be invisible: an install that ends
    /// without rmux — it errored, or the user closed the tab — leaves a launch
    /// waiting on a pane that no longer exists.
    #[test]
    fn an_install_that_ends_without_rmux_releases_the_launch() {
        let mut app = test_app();
        // A pid no pane has, standing in for the install tab having gone.
        app.rmux_installing = Some(u32::MAX);
        app.rmux_deferred = Some(Deferred::Launch);

        app.poll_rmux_install();

        assert!(app.rmux_installing.is_none());
        assert!(app.rmux_deferred.is_none());
    }

    /// was about to start.
    #[test]
    fn the_launcher_keeps_its_cursor_on_screen() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = test_app();
        // More choices than a short terminal can hold at once.
        app.launch_offer = (0..20)
            .map(|i| tabs::Choice::Start(vec![format!("agent-{i}")]))
            .collect();
        app.mode = Mode::Launch;
        let (cols, rows) = (80u16, 14u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");

        // Every choice, including the ones far past the bottom of the window.
        for cursor in 0..app.launch_offer.len() {
            app.launch_cursor = cursor;
            let mut layout = render::Layout::default();
            terminal
                .draw(|frame| layout = render::draw(frame, &mut app))
                .expect("draw");

            let row = layout
                .launch_rows
                .iter()
                .find(|(_, i)| *i == cursor)
                .map(|(row, _)| *row);
            let row = row.unwrap_or_else(|| panic!("choice {cursor} was not drawn"));
            assert!(row < rows, "choice {cursor} drawn off screen at row {row}");

            // Drawn, and drawn as the selection: the highlight is the only thing
            // saying which of twenty agents Enter starts.
            let buffer = terminal.backend().buffer().clone();
            let label = format!("agent-{cursor}");
            let line: String = (0..cols).map(|x| buffer[(x, row)].symbol()).collect();
            assert!(
                line.contains(&label),
                "row {row} is not choice {cursor}: {line:?}"
            );

            // And what is under the list stays under it, never scrolled away.
            let text: String = (0..rows)
                .map(|y| {
                    (0..cols)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect();
            assert!(text.contains("Enter start"), "the keys scrolled off");
            assert!(
                text.contains("this directory") || text.contains(" in "),
                "where it would run scrolled off"
            );
        }
    }
}
