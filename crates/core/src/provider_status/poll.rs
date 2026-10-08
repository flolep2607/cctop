//! The loop that asks the status pages, and how often.
//!
//! One loop for every process that watches: the dashboard runs it for its
//! footer, and a standalone `cctop serve` runs it for the web dashboard. A
//! serve the dashboard started does not — it is handed the dashboard's answers
//! instead, so one process never asks the vendor twice.
//!
//! The pacing is [`Schedule`], pure and clock-injected so its tests need
//! neither a network nor a sleep; [`spawn_poller`] is the thread around it.

use super::{Page, PageStatus};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Gap between checks of a page that answered.
///
/// The vendor posts an update every few minutes at best, so polling faster
/// would only buy requests; opening the TUI's panel rechecks at once for
/// anyone who cannot wait.
pub const INTERVAL: Duration = Duration::from_secs(120);

/// Gap after a page could not be reached. Longer, because the likeliest reason
/// is that this machine is offline, when every request is wasted.
pub const RETRY: Duration = Duration::from_secs(300);

/// How often the poller wakes to see whether a page is due or a recheck was
/// asked for.
pub const TICK: Duration = Duration::from_secs(2);

/// When each page is next due.
#[derive(Debug, Default)]
pub struct Schedule {
    /// `None` until a page is first asked, so the first pass asks every page.
    due: [Option<Instant>; Page::ALL.len()],
}

impl Schedule {
    /// Ask every page that is due — or every page, when `forced` — and hand
    /// each answer to `answer` as it arrives. Returns `false` once `answer`
    /// does, which is how a poller learns nobody is listening any more.
    ///
    /// The clock is read after each fetch rather than once per pass: a fetch
    /// can block for its whole timeout, and the gap is from the answer, not
    /// from the question.
    pub fn run(
        &mut self,
        forced: bool,
        now: &mut impl FnMut() -> Instant,
        fetch: &mut impl FnMut(Page) -> PageStatus,
        answer: &mut impl FnMut(Page, PageStatus) -> bool,
    ) -> bool {
        for (i, page) in Page::ALL.into_iter().enumerate() {
            if !forced && self.due[i].is_some_and(|due| now() < due) {
                continue;
            }
            let status = fetch(page);
            self.due[i] = Some(
                now()
                    + match status {
                        PageStatus::Unavailable(_) => RETRY,
                        _ => INTERVAL,
                    },
            );
            if !answer(page, status) {
                return false;
            }
        }
        true
    }
}

/// Poll each page on a thread of its own, handing every answer to `answer`
/// until it returns `false`.
///
/// Per page, so a slow or unreachable one does not hold back the one that
/// answered. Never on a drawing or request thread: [`super::fetch`] can block
/// for its whole timeout, which is exactly what it does when the network is
/// down. `recheck` asks for an immediate pass — a flag rather than a channel,
/// because the poller has nothing else to hear and a set it misses is picked
/// up on its next tick.
pub fn spawn_poller(
    recheck: Arc<AtomicBool>,
    mut answer: impl FnMut(Page, PageStatus) -> bool + Send + 'static,
) {
    std::thread::spawn(move || {
        let mut schedule = Schedule::default();
        loop {
            let forced = recheck.swap(false, Ordering::Relaxed);
            if !schedule.run(forced, &mut Instant::now, &mut super::fetch, &mut answer) {
                return;
            }
            std::thread::sleep(TICK);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::{fixtures, parse};
    use super::*;
    use std::cell::Cell;

    /// A clock the test moves, and a fetch that records who it was asked for.
    struct Rig {
        start: Instant,
        offset: Cell<Duration>,
        asked: Vec<Page>,
    }

    impl Rig {
        fn new() -> Rig {
            Rig {
                start: Instant::now(),
                offset: Cell::new(Duration::ZERO),
                asked: Vec::new(),
            }
        }

        /// One pass at `at` past the start, answering with `reply`; returns
        /// the pages asked during it.
        fn pass(
            &mut self,
            schedule: &mut Schedule,
            at: Duration,
            forced: bool,
            reply: impl Fn(Page) -> PageStatus,
        ) -> Vec<Page> {
            self.offset.set(at);
            let (start, offset) = (self.start, &self.offset);
            let mut asked = Vec::new();
            schedule.run(
                forced,
                &mut || start + offset.get(),
                &mut |page| {
                    asked.push(page);
                    reply(page)
                },
                &mut |_, _| true,
            );
            self.asked.extend(&asked);
            asked
        }
    }

    fn healthy(_: Page) -> PageStatus {
        parse(fixtures::OPERATIONAL)
    }

    #[test]
    fn the_first_pass_asks_every_page() {
        let mut rig = Rig::new();
        let mut s = Schedule::default();
        assert_eq!(
            rig.pass(&mut s, Duration::ZERO, false, healthy),
            Page::ALL.to_vec()
        );
    }

    #[test]
    fn a_page_that_answered_is_not_asked_again_before_the_interval() {
        let mut rig = Rig::new();
        let mut s = Schedule::default();
        rig.pass(&mut s, Duration::ZERO, false, healthy);
        // Every tick until the interval is up asks nothing.
        let mut at = TICK;
        while at < INTERVAL {
            assert!(rig.pass(&mut s, at, false, healthy).is_empty(), "{at:?}");
            at += TICK;
        }
        assert_eq!(
            rig.pass(&mut s, INTERVAL, false, healthy),
            Page::ALL.to_vec()
        );
    }

    #[test]
    fn an_unreachable_page_waits_the_longer_retry() {
        let mut rig = Rig::new();
        let mut s = Schedule::default();
        // Anthropic answers; OpenAI cannot be reached.
        let mixed = |page| match page {
            Page::Anthropic => parse(fixtures::OPERATIONAL),
            Page::OpenAi => PageStatus::Unavailable("connection refused".into()),
        };
        rig.pass(&mut s, Duration::ZERO, false, mixed);
        // The page that answered goes on at its interval meanwhile.
        let mut at = TICK;
        while at < RETRY {
            let asked = rig.pass(&mut s, at, false, mixed);
            assert!(!asked.contains(&Page::OpenAi), "{at:?}");
            at += TICK;
        }
        assert_eq!(
            rig.asked.iter().filter(|p| **p == Page::Anthropic).count(),
            1 + (RETRY.as_secs() / INTERVAL.as_secs()) as usize,
        );
        assert!(
            rig.pass(&mut s, RETRY, false, mixed)
                .contains(&Page::OpenAi)
        );
    }

    #[test]
    fn a_recheck_asks_every_page_at_once() {
        let mut rig = Rig::new();
        let mut s = Schedule::default();
        rig.pass(&mut s, Duration::ZERO, false, healthy);
        assert_eq!(rig.pass(&mut s, TICK, true, healthy), Page::ALL.to_vec());
        // And restarts the interval from the recheck.
        assert!(rig.pass(&mut s, INTERVAL, false, healthy).is_empty());
        assert_eq!(rig.asked.len(), 4);
    }

    #[test]
    fn a_listener_that_hangs_up_stops_the_pass() {
        let mut s = Schedule::default();
        let mut asked = 0;
        let going = s.run(
            false,
            &mut Instant::now,
            &mut |_| {
                asked += 1;
                PageStatus::Pending
            },
            &mut |_, _| false,
        );
        assert!(!going);
        assert_eq!(asked, 1, "nobody listening: the second page is not asked");
    }
}
