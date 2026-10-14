//! A terminal that outlives the socket it was opened on, for a while.
//!
//! A sibling's tab reaches a terminal here through Cloudflare's edge, and the
//! edge drops a WebSocket now and then without a word: it restarts,
//! cloudflared moves to another connection, a laptop changes network. Before
//! this, the pty went with the socket — the `mux open` on it was killed, and
//! the tab's reconnect started a fresh one, which loaded every session to find
//! the one asked for and redrew the whole screen. For a *new* agent's tab it
//! was worse: opening its route again would start a second agent, so it was
//! not reconnected at all.
//!
//! Now a terminal whose socket ends without a Close is *parked* for [`GRACE`]:
//! its pty and program stay, and what the program writes goes on into a
//! [`Ring`] of the last [`RING`] bytes. Every byte the pty has written is
//! counted, and the tab counts every byte it has been handed, so a tab that
//! comes back says how far it got and is sent the rest — from the ring, on the
//! same pty, with nothing started again and nothing redrawn that it already
//! had.
//!
//! The way back is a [`Term`]'s *token*, `<id>.<secret>`, handed to the tab
//! in the `Cctop-Resume` header of the answer that opened it. A resume is the
//! same signed request to the same route — it passes every check an open does
//! — carrying `Cctop-Resume: <token> <offset>`. The id finds the terminal; the
//! secret, 32 random bytes compared in constant time, is what proves the
//! asker is the tab that opened it; and the route must be the one it was
//! opened on. The token dies with its terminal: closed, ended, or parked past
//! [`GRACE`].
//!
//! A resume that cannot be honoured — the token unknown or expired, or the
//! offset no longer in the ring — is answered `410 Gone` and nothing more. The
//! tab decides what that means: a session's tab opens afresh, which redraws;
//! a new agent's tab must not, and says where the agent went.
//!
//! Only a tab that asks for it is parked (`Cctop-Resume: new` on the open).
//! One older than this reconnects by opening afresh, so a parked pty would be
//! a second rmux client hanging on for two minutes with nobody to come back
//! for it.
//!
//! The terminal moves between connection threads rather than staying with the
//! one that opened it: the thread whose socket dropped keeps reading the pty
//! while it waits, and a resume *claims* the [`Term`] from it — sent over a
//! channel, with a byte on a socket pair to wake its `poll` — and pumps it on
//! its own connection. So a terminal holds one of the server's connection
//! slots, whether parked or not, and never two for longer than the handover.
//!
//! ponytail: a pty whose program ends while it is parked is ended there and
//! then, and its last words with it; the tab comes back to `410` and opens
//! afresh, which says the same thing again.

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::process::Child;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// How long a terminal whose socket dropped waits for its tab to come back.
///
/// The same two minutes the tab spends trying to get back (`RECONNECT_FOR` in
/// `cctop_core::remote_open`): long enough to ride out a tunnel reconnecting,
/// and no pty held for anyone who is not still trying.
pub(crate) const GRACE: Duration = Duration::from_secs(120);

/// How much of a terminal's output is kept for a tab that comes back.
///
/// A redraw of a full screen is tens of kilobytes; two minutes of an agent
/// streaming output is rarely more than a few hundred. Past this the gap is
/// filled by a fresh open's redraw instead, which costs less than holding
/// more for every open terminal. Small under test, so overflowing it is
/// cheap.
pub(crate) const RING: usize = if cfg!(test) {
    64 * 1024
} else {
    2 * 1024 * 1024
};

/// How long a resume waits for the terminal's current thread to hand it over.
/// That thread answers within one `poll` — unless it is stuck writing to a
/// socket nobody reads, which its write timeout (`open::WRITE_TIMEOUT`) ends
/// sooner than this.
const CLAIM_WAIT: Duration = Duration::from_secs(15);

/// The last [`RING`] bytes a terminal wrote, and how many it has written.
pub(crate) struct Ring {
    buf: VecDeque<u8>,
    /// Bytes written since the terminal opened: the offset just past `buf`.
    end: u64,
    cap: usize,
}

impl Ring {
    pub(crate) fn new(cap: usize) -> Ring {
        Ring {
            buf: VecDeque::new(),
            end: 0,
            cap,
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.end += bytes.len() as u64;
        self.buf.extend(bytes);
        let over = self.buf.len().saturating_sub(self.cap);
        self.buf.drain(..over);
    }

    /// Everything written from `offset` on, or `None` when that is no longer
    /// held — or was never written, which no honest tab asks for.
    pub(crate) fn since(&self, offset: u64) -> Option<Vec<u8>> {
        let start = self.end - self.buf.len() as u64;
        if offset < start || offset > self.end {
            return None;
        }
        Some(
            self.buf
                .range((offset - start) as usize..)
                .copied()
                .collect(),
        )
    }

    #[cfg(test)]
    pub(crate) fn end(&self) -> u64 {
        self.end
    }
}

/// What the registry holds for a terminal that may be resumed: enough to
/// check a token against, and to ask for the terminal.
struct Entry {
    route: String,
    secret: [u8; 32],
    claims: Sender<Sender<Term>>,
    wake: UnixStream,
}

/// Every resumable terminal on this server, by id.
static TERMINALS: LazyLock<Mutex<HashMap<String, Entry>>> = LazyLock::new(Default::default);

fn terminals() -> std::sync::MutexGuard<'static, HashMap<String, Entry>> {
    TERMINALS.lock().unwrap_or_else(|e| e.into_inner())
}

/// One terminal: the program, its pty, and what it has said.
///
/// Dropped, it ends: the program is killed and the token forgotten. The
/// program is an rmux client (or a shell started for this terminal), so
/// ending it detaches, and the agent in rmux carries on.
pub(crate) struct Term {
    pub(crate) child: Child,
    pub(crate) master: File,
    pub(crate) ring: Ring,
    /// `None` for a terminal its tab cannot come back to.
    token: Option<String>,
    id: String,
    claims: Receiver<Sender<Term>>,
    /// Readable when a resume has sent a claim; `None` with `token`.
    wake: Option<UnixStream>,
}

impl Term {
    /// A terminal for `child` on `master`. With a `route`, it can be resumed
    /// there, and [`Term::token`] is how.
    pub(crate) fn new(child: Child, master: File, route: Option<&str>) -> Term {
        let (claims_tx, claims) = channel();
        let mut term = Term {
            child,
            master,
            ring: Ring::new(RING),
            token: None,
            id: String::new(),
            claims,
            wake: None,
        };
        let Some(route) = route else {
            return term;
        };
        let Ok((wake, wake_tx)) = UnixStream::pair() else {
            return term;
        };
        let _ = wake.set_nonblocking(true);
        let id = hex(&random::<8>());
        let secret = random::<32>();
        term.token = Some(format!("{id}.{}", hex(&secret)));
        term.id = id.clone();
        term.wake = Some(wake);
        terminals().insert(
            id,
            Entry {
                route: route.to_string(),
                secret,
                claims: claims_tx,
                wake: wake_tx,
            },
        );
        term
    }

    /// The token a tab resumes this terminal with, when it can be.
    pub(crate) fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// The fd that turns readable when a resume wants this terminal: `-1`,
    /// which `poll` skips, for one that cannot be resumed.
    pub(crate) fn wake_fd(&self) -> i32 {
        self.wake.as_ref().map_or(-1, |w| w.as_raw_fd())
    }

    /// The newest resume waiting for this terminal, once [`Term::wake_fd`]
    /// polled readable. Older ones that raced it are dropped, and their
    /// askers told the terminal is gone.
    pub(crate) fn take_claim(&mut self) -> Option<Sender<Term>> {
        if let Some(mut wake) = self.wake.as_ref() {
            let mut drain = [0u8; 64];
            while matches!(wake.read(&mut drain), Ok(n) if n > 0) {}
        }
        let mut newest = None;
        while let Ok(reply) = self.claims.try_recv() {
            newest = Some(reply);
        }
        newest
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        if !self.id.is_empty() {
            terminals().remove(&self.id);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Why a resume found nothing to resume.
pub(crate) const GONE: &str = "that terminal has ended, or waited too long for you";

/// The terminal `token` names, on `route`, taken from whichever thread holds
/// it now.
pub(crate) fn claim(token: &str, route: &str) -> Result<Term, &'static str> {
    let (id, secret) = token.split_once('.').ok_or(GONE)?;
    let secret = unhex32(secret).ok_or(GONE)?;
    let (claims, wake) = {
        let map = terminals();
        let entry = map.get(id).ok_or(GONE)?;
        // The route is not a secret; the secret is compared in constant
        // time, so a guess learns nothing from how long it took to refuse.
        if entry.route != route || !constant_time_eq(&entry.secret, &secret) {
            return Err(GONE);
        }
        (
            entry.claims.clone(),
            entry.wake.try_clone().map_err(|_| GONE)?,
        )
    };
    let (reply, back) = channel();
    claims.send(reply).map_err(|_| GONE)?;
    let _ = (&wake).write_all(&[1]);
    back.recv_timeout(CLAIM_WAIT).map_err(|_| GONE)
}

/// Hold a terminal whose socket dropped: keep reading its pty into the ring
/// until a resume claims it, its program ends, or `grace` runs out — then
/// hand it over, or end it.
pub(crate) fn park(mut term: Term, grace: Duration) {
    if term.token.is_none() {
        return;
    }
    let deadline = Instant::now() + grace;
    let mut buf = [0u8; 16 * 1024];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        let mut fds = [
            libc::pollfd {
                fd: term.master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: term.wake_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let ms = left.as_millis().clamp(1, i32::MAX as u128) as i32;
        // SAFETY: two valid pollfds on fds the terminal owns for this call.
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, ms) } < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        // A claim first: a pty with more to say would otherwise keep a tab
        // that has come back waiting.
        if fds[1].revents != 0
            && let Some(reply) = term.take_claim()
        {
            // Refused only when the asker gave up waiting; the terminal is
            // then dropped with the send, as it would be at the deadline.
            let _ = reply.send(term);
            return;
        }
        if fds[0].revents != 0 {
            match (&term.master).read(&mut buf) {
                Ok(n) if n > 0 => term.ring.push(&buf[..n]),
                _ => return,
            }
        }
    }
}

fn random<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(&cctop_core::util::random_bytes(N));
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Equal, in a time that depends only on the length.
fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let diff = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(diff) == 0
}

/// Whether the terminal behind `token` can still be resumed.
#[cfg(test)]
pub(crate) fn registered(token: &str) -> bool {
    token
        .split_once('.')
        .is_some_and(|(id, _)| terminals().contains_key(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_keeps_the_tail_and_counts_everything() {
        let mut ring = Ring::new(8);
        ring.push(b"hello");
        assert_eq!(ring.since(0).as_deref(), Some(&b"hello"[..]));
        assert_eq!(ring.since(5).as_deref(), Some(&b""[..]));
        assert_eq!(ring.since(6), None, "never written");
        ring.push(b" world");
        assert_eq!(ring.end(), 11);
        // Three bytes have fallen out of an eight-byte ring.
        assert_eq!(ring.since(2), None);
        assert_eq!(ring.since(3).as_deref(), Some(&b"lo world"[..]));
        assert_eq!(ring.since(9).as_deref(), Some(&b"ld"[..]));
    }

    fn shell(script: &str) -> (Child, File) {
        let argv = ["sh".to_string(), "-c".to_string(), script.to_string()];
        cctop_core::shim::spawn_pty(&argv, (80, 24), &[]).expect("pty")
    }

    /// Parked, a terminal keeps reading, and a claim with its token takes it
    /// — output and all. A wrong secret, another route, or nonsense takes
    /// nothing, and leaves it parked.
    #[test]
    fn a_parked_terminal_is_claimed_only_with_its_token_on_its_route() {
        let (child, master) = shell("echo parked; read line");
        let term = Term::new(child, master, Some("/api/term/claude/abc"));
        let token = term.token().expect("token").to_string();
        let parker = std::thread::spawn(move || park(term, GRACE));
        let (id, secret) = token.split_once('.').unwrap();
        let mut wrong = secret.to_string().into_bytes();
        wrong[0] = if wrong[0] == b'0' { b'1' } else { b'0' };
        let wrong = format!("{id}.{}", String::from_utf8(wrong).unwrap());
        for (bad, route) in [
            (wrong.as_str(), "/api/term/claude/abc"),
            (token.as_str(), "/api/term/claude/other"),
            ("nonsense", "/api/term/claude/abc"),
            (&format!("{id}.short"), "/api/term/claude/abc"),
        ] {
            assert_eq!(claim(bad, route).err(), Some(GONE), "{bad} {route}");
        }
        assert!(registered(&token));
        let claimed = cctop_core::test_wait::eventually("parked output", || {
            // Claimed, then parked again on this thread until it has spoken.
            let term = claim(&token, "/api/term/claude/abc").ok()?;
            let said = String::from_utf8_lossy(&term.ring.since(0)?).into_owned();
            match said.contains("parked") {
                true => Some(term),
                false => {
                    std::thread::spawn(move || park(term, GRACE));
                    None
                }
            }
        });
        parker.join().unwrap();
        drop(claimed);
        assert!(!registered(&token), "ended, it is forgotten");
        assert_eq!(claim(&token, "/api/term/claude/abc").err(), Some(GONE));
    }

    /// Past its grace a parked terminal ends, and its token with it.
    #[test]
    fn a_terminal_parked_past_its_grace_is_gone() {
        let (child, master) = shell("read line");
        let term = Term::new(child, master, Some("/api/term/claude/abc"));
        let token = term.token().expect("token").to_string();
        park(term, Duration::from_millis(1));
        assert!(!registered(&token));
        assert_eq!(claim(&token, "/api/term/claude/abc").err(), Some(GONE));
    }

    /// A terminal opened without asking to be resumable has no token and is
    /// not parked.
    #[test]
    fn a_terminal_nobody_will_come_back_for_is_not_kept() {
        let (child, master) = shell("read line");
        let term = Term::new(child, master, None);
        assert_eq!(term.token(), None);
        let started = Instant::now();
        park(term, GRACE);
        assert!(started.elapsed() < GRACE / 4);
    }
}
