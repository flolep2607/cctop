//! Which machine's sessions the table shows: all of them, this one, or one
//! other.
//!
//! With siblings on the account the table is every machine's sessions sorted
//! together, and a machine with a hundred rows beside two with eight hundred
//! each is a machine whose sessions are found by scrolling. The HOST column
//! says where a row is from; it cannot say "show me only these". The selector
//! does, from the table's own title — `[All] [Local] [☁ box]` — clicked, or
//! stepped through with `m`.
//!
//! A filter layer like live-only rather than a view of its own, so the counts
//! in the title, Esc and everything else that works on the table work on it.
//! Not saved with the other filters: the machines on an account come and go,
//! and a cctop that starts showing one machine of several reads as one that has
//! lost the rest.

use super::*;
use ratatui::text::Span;

/// One chip in the selector.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Machine {
    #[default]
    All,
    /// This machine's own sessions.
    Local,
    /// One other machine, by the name its rows carry in [`Remote::host`].
    ///
    /// [`Remote::host`]: cctop_core::session::Remote::host
    Host(String),
}

impl Machine {
    pub fn admits(&self, s: &Session) -> bool {
        match self {
            Machine::All => true,
            Machine::Local => s.remote.is_none(),
            Machine::Host(h) => s.remote.as_ref().is_some_and(|r| r.host == *h),
        }
    }

    /// What the chip says, brackets included.
    pub fn chip(&self) -> String {
        match self {
            Machine::All => "[All]".to_string(),
            Machine::Local => "[Local]".to_string(),
            // U+2601 without the emoji selector: with it, terminals disagree
            // on whether the cloud is one cell or two, and the clicks recorded
            // for every chip after it land a column off in the ones that say
            // two.
            Machine::Host(h) => format!("[☁ {h}]"),
        }
    }
}

impl App {
    /// The chips to draw, in order: everything, this machine, then the others
    /// by name. Empty with no other machine, where the selector would be a
    /// choice between two names for the same rows.
    ///
    /// A machine that is down keeps its chip, as it keeps its last rows. One
    /// picked and since gone from the account keeps it too, so the selector
    /// still says what the table is showing — nothing, from there.
    pub fn machines(&self) -> Vec<Machine> {
        let mut hosts: Vec<&String> = self.remotes.keys().collect();
        if let Machine::Host(h) = &self.machine
            && !self.remotes.contains_key(h)
        {
            hosts.push(h);
        }
        if hosts.is_empty() {
            return Vec::new();
        }
        hosts.sort();
        [Machine::All, Machine::Local]
            .into_iter()
            .chain(hosts.into_iter().cloned().map(Machine::Host))
            .collect()
    }

    /// Show `machine`'s sessions, and say so.
    pub fn pick_machine(&mut self, machine: Machine) {
        if self.machine == machine {
            return;
        }
        self.machine = machine;
        self.refilter();
        let said = match &self.machine {
            Machine::All => "Showing every machine".to_string(),
            Machine::Local => "Showing this machine only".to_string(),
            Machine::Host(h) => format!("Showing {h} only"),
        };
        self.set_status(said);
    }

    /// `m` and `M`: the next or previous chip, wrapping.
    pub fn step_machine(&mut self, forward: bool) {
        let all = self.machines();
        if all.is_empty() {
            self.set_status("No other machine to switch to");
            return;
        }
        let at = all.iter().position(|m| *m == self.machine).unwrap_or(0);
        let next = match forward {
            true => (at + 1) % all.len(),
            false => (at + all.len() - 1) % all.len(),
        };
        self.pick_machine(all[next].clone());
    }

    /// The selector's spans, and where each chip falls from the first column.
    pub(super) fn machine_chips(&self) -> Vec<(Span<'static>, Machine)> {
        self.machines()
            .into_iter()
            .map(|m| {
                let style = match m == self.machine {
                    true => theme::selected(),
                    false => theme::dim(),
                };
                (Span::styled(m.chip(), style), m)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::test_app;
    use cctop_core::pricing::Provider;
    use cctop_core::session::Remote;

    fn on(host: Option<&str>, id: &str) -> Session {
        let mut s = Session::new(Provider::Claude, id.to_string());
        s.remote = host.map(|h| Remote {
            host: h.to_string(),
            ..Default::default()
        });
        s
    }

    fn app() -> App {
        let mut app = test_app();
        app.sessions = vec![on(None, "here")];
        app.remotes.insert(
            "zeta".into(),
            vec![on(Some("zeta"), "z1"), on(Some("zeta"), "z2")],
        );
        app.remotes
            .insert("alpha".into(), vec![on(Some("alpha"), "a1")]);
        app.merge_remotes();
        app
    }

    fn shown(app: &App) -> Vec<String> {
        let mut ids: Vec<String> = app
            .visible
            .iter()
            .filter_map(|r| r.session())
            .map(|i| app.sessions[i].session_id.clone())
            .collect();
        ids.sort();
        ids
    }

    #[test]
    fn no_selector_without_another_machine() {
        let mut app = test_app();
        app.sessions = vec![on(None, "here")];
        assert!(app.machines().is_empty());
        app.step_machine(true);
        assert_eq!(app.machine, Machine::All);
    }

    #[test]
    fn chips_are_all_local_then_hosts_by_name() {
        let chips: Vec<String> = app().machines().iter().map(Machine::chip).collect();
        assert_eq!(chips, ["[All]", "[Local]", "[☁ alpha]", "[☁ zeta]"]);
    }

    #[test]
    fn each_chip_shows_its_machine_and_m_wraps() {
        let mut app = app();
        assert_eq!(shown(&app), ["a1", "here", "z1", "z2"]);
        app.step_machine(true);
        assert_eq!(shown(&app), ["here"]);
        app.step_machine(true);
        assert_eq!(shown(&app), ["a1"]);
        app.step_machine(true);
        assert_eq!(shown(&app), ["z1", "z2"]);
        // The title counts what the chip let through.
        assert_eq!(app.matched, 2);
        app.step_machine(true);
        assert_eq!(app.machine, Machine::All);
        app.step_machine(false);
        assert_eq!(app.machine, Machine::Host("zeta".into()));
    }

    #[test]
    fn a_new_snapshot_keeps_the_pick() {
        let mut app = app();
        app.pick_machine(Machine::Host("alpha".into()));
        app.remotes.insert(
            "alpha".into(),
            vec![on(Some("alpha"), "a1"), on(Some("alpha"), "a2")],
        );
        app.merge_remotes();
        assert_eq!(shown(&app), ["a1", "a2"]);
    }

    #[test]
    fn a_machine_that_left_keeps_its_chip_while_picked() {
        let mut app = app();
        app.pick_machine(Machine::Host("alpha".into()));
        app.remotes.remove("alpha");
        app.merge_remotes();
        assert!(app.machines().contains(&Machine::Host("alpha".into())));
        assert!(shown(&app).is_empty());
    }

    #[test]
    fn esc_puts_every_machine_back() {
        let mut app = app();
        app.pick_machine(Machine::Local);
        assert!(app.has_filter());
        app.clear_one_filter();
        assert_eq!(app.machine, Machine::All);
        assert_eq!(shown(&app).len(), 4);
    }
}
