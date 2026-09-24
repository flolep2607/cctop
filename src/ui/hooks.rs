//! The integration panel: which agents are hooked up, and where.
//!
//! Installing a hook edits a settings file that belongs to another program, so
//! this is deliberately separate from [`signals`](super::signals), which reads
//! what those hooks send. The panel answers a different question — not "what is
//! this agent doing" but "would it be able to tell me at all" — and it is the
//! only place that writes to another harness's configuration.

use super::*;

impl App {
    /// Open the integration panel, reading the current state off disk.
    pub fn open_hooks(&mut self) {
        self.hooks = Some(self.hook_status());
        self.mode = Mode::Hooks;
        self.needs_redraw = true;
    }

    /// The integration's state, scoped to whichever project the cursor is on.
    pub(super) fn hook_status(&self) -> crate::hook::Report {
        let mut report =
            crate::hook::status(self.hook_project().as_deref(), self.listener.as_ref());
        // Codex's entry carries a reminder to go and trust its hooks, which is
        // advice until it is done and noise afterwards. Only the events can tell
        // which — see [`App::codex_hooks_heard`] — so the panel is where the
        // reminder is dropped rather than where it is written.
        if self.codex_hooks_heard() {
            for entry in &mut report.entries {
                if entry.harness == crate::hook::Harness::Codex {
                    entry.note = None;
                }
            }
        }
        report
    }

    /// Whether Codex's hooks have ever fired into this cctop.
    ///
    /// Codex is the one harness whose hooks sit there inert until a person has
    /// reviewed and trusted them, and where that trust is recorded is not
    /// something Codex documents — so an install that reads as complete on disk
    /// may be delivering nothing at all.
    ///
    /// What answers it is the permission mode. `notify`, Codex's other channel,
    /// carries no such field, and a hook carries it on every event worth having
    /// — so a Codex row that knows how much it asks before it acts is a Codex
    /// whose hooks are firing. Rows from another machine are excluded: their
    /// mode arrived over `--host` from a cctop where the hooks work, which says
    /// nothing about this one.
    pub(super) fn codex_hooks_heard(&self) -> bool {
        self.sessions.iter().any(|session| {
            session.provider == Provider::Codex
                && session.remote.is_none()
                && session.permission.is_some()
        })
    }

    /// The line a Codex being started needs, when it needs one.
    ///
    /// The moment of starting one is when this is worth saying and cheap to act
    /// on: there is a fresh prompt on screen, `/hooks` costs a keystroke there,
    /// and the alternative is a session that runs for an hour reporting only
    /// that its turns ended. Said only when the hooks are installed and have
    /// never been heard from — an install nobody asked for is not something to
    /// nag about, and one that is already working needs nothing.
    pub(super) fn codex_trust_hint(&self, installed: bool) -> &'static str {
        match installed && !self.codex_hooks_heard() {
            true => " — run /hooks in it and trust cctop's to see it here",
            false => "",
        }
    }

    /// The project a `project`-scoped install would write into: the directory
    /// of the selected session, when it has one on this machine.
    pub fn hook_project(&self) -> Option<std::path::PathBuf> {
        self.selected_session()
            .map(|s| std::path::PathBuf::from(&s.label_source))
            .filter(|dir| dir.is_dir())
    }

    /// Install or remove from the panel, and show what happened.
    ///
    /// Every harness at once. The status line gets a count rather than five
    /// paths — the panel underneath is redrawn from disk immediately below, and
    /// that is where the detail belongs.
    pub fn set_hooks(&mut self, scope: crate::hook::Scope, install: bool) {
        let done = match install {
            true => crate::hook::install(&scope),
            false => crate::hook::remove(&scope),
        };
        self.set_status(format!(
            "{} {} agents ({})",
            match install {
                true => "Asked",
                false => "Stopped",
            },
            done.len(),
            scope.label()
        ));
        self.hooks = Some(self.hook_status());
    }

    /// What the agents have actually said, newest state per session, as
    /// `(project, state)` pairs for the panel.
    ///
    /// The project rather than the session id: an id names nothing to a reader,
    /// and this list is the answer to "is the thing I just installed working",
    /// which needs a name you recognise.
    pub fn reporting(&self) -> Vec<(String, &'static str)> {
        let mut rows: Vec<(String, &'static str)> = self
            .hooked
            .values()
            .filter(|r| r.is_current())
            .map(|r| {
                let name = std::path::Path::new(&r.cwd)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "—".into());
                (name, r.signal.label())
            })
            .collect();
        rows.sort();
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{session, test_app};
    /// Codex's hooks are written to disk and then sit there doing nothing until
    /// a person has trusted them, and where that trust is recorded is not
    /// something Codex documents — so the only way to know is whether anything
    /// has arrived.
    ///
    /// The permission mode is what answers it: `notify`, Codex's other channel,
    /// carries no such field, so a Codex row that knows its mode is a Codex
    /// whose hooks are firing. Said when a Codex is *started*, which is the
    /// moment `/hooks` is a keystroke away rather than a thing to remember.
    #[test]
    fn a_codex_whose_hooks_are_untrusted_is_told_so_when_it_starts() {
        let mut app = test_app();
        let mut codex = session("c", true, "proj");
        codex.provider = Provider::Codex;
        app.sessions = vec![codex];

        assert!(!app.codex_hooks_heard(), "nothing has reported yet");
        assert!(
            !app.codex_trust_hint(true).is_empty(),
            "a written-but-silent install is exactly the case worth saying"
        );
        assert!(
            app.codex_trust_hint(false).is_empty(),
            "an install nobody asked for is not something to nag about"
        );

        // One hook event carries the mode, which nothing but a hook does.
        app.sessions[0].permission = Some(crate::hook::Permission::Ask);
        assert!(app.codex_hooks_heard());
        assert!(
            app.codex_trust_hint(true).is_empty(),
            "the reminder outlived its purpose"
        );

        // A row from another machine says nothing about this one's hooks: its
        // mode came over `--host` from a cctop where they work.
        app.sessions[0].remote = Some(crate::session::Remote {
            host: "elsewhere".into(),
            branch: None,
            ..Default::default()
        });
        assert!(!app.codex_hooks_heard());

        // And a Claude Code row reporting its mode is not Codex's answer.
        app.sessions[0].remote = None;
        app.sessions[0].provider = Provider::Claude;
        assert!(!app.codex_hooks_heard());
    }
}
