//! Background work at background priority.
//!
//! cctop is a monitor: the agents it watches and the builds they run are the
//! machine's work, and re-reading transcripts every couple of seconds is not.
//! On a busy box the refresh should be the thing that waits. So the threads
//! that do it — the dashboard's worker, the sibling poller, `cctop serve`'s
//! refresher — drop to nice 10 and the idle I/O class as they start.
//!
//! Per thread rather than per process, and that is the whole design. Linux
//! keeps both priorities per thread, and a process started from a thread
//! inherits that thread's. The UI thread starts the mux daemon, and the daemon
//! starts every agent: lowering the process would lower every agent launched
//! from it, and an unprivileged process can never raise them back
//! (`RLIMIT_NICE` is 0 on most systems). The UI thread stays where it is, and
//! so does keyboard input.
//!
//! For the same reason a lowered thread must not start anything that outlives
//! its work and carries someone's typing — an ssh ControlMaster is the one
//! there is. The dashboard's worker hands those jobs to a thread it starts
//! before lowering itself.
//!
//! Best effort: a kernel or container that refuses either call leaves the
//! thread as it was, which is how cctop ran before this.

/// How far below normal a background thread runs. Ten is what `nice` gives
/// with no argument: under contention such a thread gets a small share of a
/// core, and on an idle machine all of one.
const NICE: i32 = 10;

/// Drop the calling thread to background CPU and I/O priority.
///
/// A no-op under test, where fork-heavy tests would otherwise run their
/// children at nice 10 and turn a loaded machine's slowness into timeouts.
pub fn background() {
    if crate::under_test() {
        return;
    }
    lower_this_thread();
}

fn lower_this_thread() {
    // SAFETY: plain syscalls on the calling thread's own id, no pointers.
    unsafe {
        let tid = libc::gettid();
        libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, NICE);
        libc::syscall(
            libc::SYS_ioprio_set,
            IOPRIO_WHO_PROCESS,
            tid,
            IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT,
        );
    }
}

// From `linux/ioprio.h`, which libc does not carry. `WHO_PROCESS` names a
// thread id despite the name — the I/O class is per thread, like nice.
const IOPRIO_WHO_PROCESS: libc::c_int = 1;
const IOPRIO_CLASS_IDLE: libc::c_int = 3;
const IOPRIO_CLASS_SHIFT: libc::c_int = 13;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_calling_thread_is_lowered() {
        let tid_of = || unsafe { libc::gettid() };
        let nice_of =
            |tid: libc::pid_t| unsafe { libc::getpriority(libc::PRIO_PROCESS, tid as libc::id_t) };
        let class_of = |tid: libc::pid_t| unsafe {
            libc::syscall(libc::SYS_ioprio_get, IOPRIO_WHO_PROCESS, tid) as libc::c_int
                >> IOPRIO_CLASS_SHIFT
        };
        let here = tid_of();
        let before = nice_of(here);
        let (nice, class) = std::thread::spawn(move || {
            lower_this_thread();
            let tid = tid_of();
            (nice_of(tid), class_of(tid))
        })
        .join()
        .unwrap();
        // An absolute nice, so a cctop already started under `nice -n 15`
        // stays there: going back up to 10 is the raise it may not make.
        assert_eq!(nice, before.max(NICE));
        assert_eq!(class, IOPRIO_CLASS_IDLE);
        assert_eq!(nice_of(here), before, "the spawning thread is untouched");
    }
}
