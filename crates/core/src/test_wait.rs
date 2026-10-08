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
//! The same goes for "the queue is full", which two tests of the hook once
//! read off a second of a filler thread making no progress: [`fill_queue`]
//! asks the kernel instead, and is here so both of them can.
//!
//! Compiled for this crate's tests and for any crate that turns on
//! `test-support` (the UI's, the server's and the binary's tests, including
//! `tests/`), so there is one copy rather than one per module.

use std::path::Path;
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

/// Connect to `path` until its queue has no room left, and hold every
/// connection made, so that the queue stays full until they are dropped.
///
/// Non-blocking, so the connect that finds the queue full is told so —
/// EAGAIN — instead of waiting in it. How many it takes is the kernel's
/// business (a backlog of one takes two), which is why this counts nothing
/// and asks instead. The listener's backlog has to be small — a handful —
/// since every connection is held: one at `SOMAXCONN` would take thousands.
pub fn fill_queue(path: &Path) -> Vec<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;

    let (addr, len) = sockaddr(path);
    let mut held = Vec::new();
    loop {
        // SAFETY: a fresh descriptor, owned from the line below on.
        let raw = unsafe {
            libc::socket(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                0,
            )
        };
        assert!(raw >= 0, "socket");
        // SAFETY: just opened, and owned by nothing else.
        let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) };
        // SAFETY: a fully initialised address and its true length.
        if unsafe { libc::connect(raw, std::ptr::from_ref(&addr).cast(), len) } == 0 {
            held.push(fd);
            assert!(held.len() <= 64, "the queue never filled");
            continue;
        }
        let error = std::io::Error::last_os_error();
        assert_eq!(
            error.raw_os_error(),
            Some(libc::EAGAIN),
            "the queue refused for another reason: {error}"
        );
        return held;
    }
}

/// `path` as the address a unix socket call takes, and its length.
pub fn sockaddr(path: &Path) -> (libc::sockaddr_un, libc::socklen_t) {
    use std::os::unix::ffi::OsStrExt;

    let bytes = path.as_os_str().as_bytes();
    // SAFETY: zeroed is a valid `sockaddr_un` once the family and the path
    // are written, and the length returned covers exactly those bytes.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, byte) in addr.sun_path.iter_mut().zip(bytes) {
        *slot = *byte as libc::c_char;
    }
    let len =
        (std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1) as libc::socklen_t;
    (addr, len)
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
