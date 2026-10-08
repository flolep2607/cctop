//! Waiting in tests for something to become true, rather than for a guess at
//! how long it takes.
//!
//! A test that sleeps a fixed time is wrong both ways: a short guess goes red
//! the moment the machine is busy, and a long one is paid by every run on an
//! idle one. The suite used to be full of both, along with about fifteen copies
//! of a hand-rolled poll loop, most of which slept *before* their first look,
//! so even a condition that already held cost a full interval.
//!
//! So every wait in the tests goes through [`wait_until`]: look first, then
//! look again every [`POLL`], and give up only after a `timeout` that bounds a
//! *failure*. A passing test never reaches the timeout, so it can be generous
//! ([`PATIENCE`]) at no cost, and a generous one is what keeps a loaded
//! machine from turning the suite red.
//!
//! Compiled for this crate's tests and for any crate that turns on
//! `test-support` (the UI's, the server's and the binary's tests, including
//! `tests/`), so there is one copy rather than one per module.

use std::time::{Duration, Instant};

/// How often a wait looks again. Short, because what is being waited on is
/// usually a thread or a child a few milliseconds away, and a look is cheap.
pub const POLL: Duration = Duration::from_millis(5);

/// How often a wait looks again when looking spawns a process (`rmux`, `ps`):
/// often enough that a pass is not held up, rarely enough that the asking is
/// not itself the load on a busy machine.
pub const ASK_AGAIN: Duration = Duration::from_millis(50);

/// How long a wait holds out before calling it a failure. Twice what the old
/// loops gave (5 s): a pass never waits for it, and a box compiling on every
/// core can stall a thread for seconds.
pub const PATIENCE: Duration = Duration::from_secs(10);

/// Poll `f` until it yields, checking first and then every [`POLL`], for at
/// most `timeout`. `None` means it never did.
pub fn wait_until<T>(timeout: Duration, f: impl FnMut() -> Option<T>) -> Option<T> {
    wait_every(POLL, timeout, f)
}

/// [`wait_until`] at an interval of the caller's choosing, for a look that is
/// itself expensive — one that spawns a process, say, where a 5 ms poll would
/// spend a core on asking.
pub fn wait_every<T>(
    every: Duration,
    timeout: Duration,
    mut f: impl FnMut() -> Option<T>,
) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(found) = f() {
            return Some(found);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(every);
    }
}

/// [`wait_every`] at [`ASK_AGAIN`] with [`PATIENCE`]: the wait for an answer
/// only a subprocess can give.
pub fn wait_asking<T>(f: impl FnMut() -> Option<T>) -> Option<T> {
    wait_every(ASK_AGAIN, PATIENCE, f)
}

/// [`wait_until`] with [`PATIENCE`], for a condition that is all a test needs:
/// whether it came true.
pub fn waits_for(mut f: impl FnMut() -> bool) -> bool {
    wait_until(PATIENCE, || f().then_some(())).is_some()
}

/// Wait for `f` to yield, and fail the test naming `what` never happened.
/// Saying what was awaited is the point: a bare timeout says nothing about
/// which of a test's several waits it was.
#[track_caller]
pub fn eventually<T>(what: &str, f: impl FnMut() -> Option<T>) -> T {
    match wait_until(PATIENCE, f) {
        Some(found) => found,
        None => panic!("waited {PATIENCE:?} for {what}, and it never happened"),
    }
}

/// [`eventually`] for a plain condition.
#[track_caller]
pub fn eventually_true(what: &str, mut f: impl FnMut() -> bool) {
    eventually(what, || f().then_some(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_condition_that_already_holds_costs_no_sleep() {
        let started = Instant::now();
        assert_eq!(wait_until(PATIENCE, || Some(7)), Some(7));
        // Not a margin on scheduling: anything near PATIENCE would mean the
        // first look came after a sleep.
        assert!(started.elapsed() < PATIENCE / 2);
    }

    #[test]
    fn a_condition_that_never_holds_gives_up() {
        let mut looks = 0;
        assert_eq!(
            wait_until::<()>(Duration::from_millis(30), || {
                looks += 1;
                None
            }),
            None
        );
        assert!(looks > 1, "it looked again before giving up");
    }
}
