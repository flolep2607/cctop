//! The settings page's `[[schedule]]` rows: each schedule, whether it is on,
//! when it fires next, and a way to run it now.
//!
//! A schedule's fields stay in `config.toml`, written by hand (`e` opens it):
//! a prompt and a calendar expression are a paragraph and a grammar, which a
//! row on a page is the wrong place to compose. What the page offers is what
//! changes day to day — switching one off for a holiday, and running one by
//! hand to see what it does. See [`cctop_core::schedule`].

use super::*;
use cctop_core::schedule::Schedule;

/// One schedule as the page shows it.
#[derive(Debug, Clone)]
pub struct ScheduleRow {
    pub schedule: Schedule,
    /// When its timer fires next, as systemd says it; `None` when it is not
    /// installed or there is no systemd to ask.
    pub next: Option<String>,
}

/// systemd's `Fri 2026-11-06 08:00:00 NZDT` as `Fri 11-06 08:00`: the day
/// and the minute are what is read, and the row is narrow.
fn short_time(systemd: &str) -> String {
    let words: Vec<&str> = systemd.split_whitespace().collect();
    match words.as_slice() {
        [day, date, time, ..] if date.len() == 10 && time.len() >= 5 => {
            format!("{day} {} {}", &date[5..], &time[..5])
        }
        _ => systemd.to_string(),
    }
}

/// The schedules in `path`, each with its next run as systemd says it — after
/// the sync, so a timer just installed answers. Never asks systemd under test,
/// which must not reach the developer's own.
pub(super) fn rows(path: &std::path::Path) -> Vec<ScheduleRow> {
    cctop_core::schedule::Schedules::load_from(path)
        .entries
        .into_iter()
        .map(|schedule| {
            let next = match cctop_core::under_test() || !schedule.enabled {
                true => None,
                false => {
                    let mut ctl = |a: &[&str]| cctop_core::schedule::systemctl(a);
                    cctop_core::schedule::timer_times(&schedule, &mut ctl).0
                }
            };
            ScheduleRow { schedule, next }
        })
        .collect()
}

impl App {
    /// Sync the timers if the schedules changed, and read the schedules
    /// again with their next run — on the worker, since every row is a
    /// `systemctl` call and a sync several. The rows arrive with
    /// `Response::Schedules`. Under test, read here, and systemd never asked.
    pub(super) fn refresh_schedules(&mut self) {
        let Some(path) = self.settings_file.clone() else {
            self.schedules.clear();
            return;
        };
        match cctop_core::under_test() {
            true => self.schedules = rows(&path),
            false => {
                let _ = self.tx.send(super::worker::Request::Schedules { path });
            }
        }
    }

    /// The row's description: when, what, where, and when next.
    pub(super) fn schedule_what(&self, i: usize) -> String {
        let Some(row) = self.schedules.get(i) else {
            return String::new();
        };
        let s = &row.schedule;
        let next = match (s.enabled, &row.next) {
            (false, _) => "off".to_string(),
            (true, Some(next)) => format!("next {}", short_time(next)),
            (true, None) => "not installed yet".to_string(),
        };
        format!(
            "{} · {} in {} · {next}",
            s.when,
            s.harness,
            cctop_core::util::tildify(&s.dir)
        )
    }

    /// Enter on a schedule: switch it on or off, in the file, where the timer
    /// follows it at the reload the write causes.
    pub(super) fn schedule_toggle(&mut self, i: usize) {
        let (Some(row), Some(path)) = (self.schedules.get(i), self.settings_file.clone()) else {
            return;
        };
        let name = row.schedule.name.clone();
        let on = !row.schedule.enabled;
        match cctop_core::schedule::set_enabled(&path, &name, on) {
            Ok(()) => self.set_status(format!(
                "Schedule {name} {}",
                match on {
                    true => "on",
                    false => "off",
                }
            )),
            Err(e) => self.set_status(format!("Could not save: {e}")),
        }
        // Forced: two writes inside one millisecond share an mtime.
        self.settings_stamp = 0;
        self.reload_settings();
    }

    /// `n` on a schedule: run it now, as its timer would — a tab of its own,
    /// skipped if its last run is still going.
    ///
    /// On a thread: a run ends by telling subscribed phones, which is a round
    /// trip per browser the page cannot wait on. Its tab appearing in the bar
    /// is the answer; a refusal is in cctop's event log.
    pub(super) fn schedule_run_now(&mut self, i: usize) {
        let Some(row) = self.schedules.get(i) else {
            return;
        };
        let name = row.schedule.name.clone();
        // As the timer would decide: a run still at work is left alone, and one
        // that finished at its prompt is replaced by this one.
        let previous = cctop_core::schedule::previous(&row.schedule.unit());
        if previous == cctop_core::schedule::Previous::Busy {
            return self.set_status(format!("{name} is already running — see its tab"));
        }
        if !cctop_core::under_test() {
            let asked = name.clone();
            std::thread::spawn(move || cctop_core::schedule::run(&asked, true));
        }
        self.set_status(format!(
            "Starting {name} — its tab opens when the agent is up"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::super::settings::Item;

    /// The page lists each schedule, Enter switches one off in the file and
    /// back on, and the line under them opens the file to add one.
    #[test]
    fn schedules_are_on_the_page_and_switch_in_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[[schedule]]\nname = \"deps\"\nwhen = \"Mon..Fri 08:00\"\ndir = \"/tmp\"\n\
             harness = \"claude\"\nprompt = \"Bump the dependencies\"\n",
        )
        .unwrap();
        let mut app = crate::tests::test_app();
        app.settings_file = Some(path.clone());
        app.reload_settings();
        let items = app.settings_items();
        let at = items
            .iter()
            .position(|i| *i == Item::Schedule(0))
            .expect("the schedule's row");
        assert_eq!(items[at + 1], Item::ScheduleAdd);
        assert!(
            app.schedule_what(0)
                .contains("Mon..Fri 08:00 · claude in /tmp")
        );

        app.settings_cursor = at;
        app.settings_activate();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("enabled = false")
        );
        assert!(!app.schedules[0].schedule.enabled);
        assert!(app.schedule_what(0).contains("off"));
        app.settings_activate();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("enabled"));
        assert!(app.schedules[0].schedule.enabled);

        // By hand, under test: said, and nothing started.
        app.schedule_run_now(0);
        assert!(app.status().is_some_and(|s| s.contains("Starting deps")));
    }
}
