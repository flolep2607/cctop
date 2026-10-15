//! Which transport carries a tunnel to the edge: QUIC, HTTP/2 over TCP, or
//! QUIC with a fall back to HTTP/2 — cloudflared's `--protocol` (`quic`,
//! `http2`, `auto`), and the same default.
//!
//! The two carry the same tunnel. Registration, configuration pushes and every
//! request are the same calls; only the framing under them differs (see
//! `http2` for the HTTP/2 side). So switching transport costs a reconnect and
//! nothing else, and the edge takes the new connection in place of the old one
//! with `replace_existing`, as for any reconnect.
//!
//! # When `auto` falls back, and where it parts from cloudflared
//!
//! cloudflared (`supervisor/tunnel.go`, `selectNextProtocol`) falls back to
//! HTTP/2 when QUIC never connects: a dial that times out ("QUIC is broken") or
//! one that keeps failing until its backoff runs out. Once any connection has
//! registered over QUIC it stays on QUIC for good (`HasConnectedWith`).
//!
//! That last rule is the case this module exists for. On a machine whose UDP
//! goes through a NAT that drops it for seconds at a time — WSL2 behind
//! Windows is the one that was measured — QUIC registers fine and then dies of
//! its idle timeout every so often, taking every terminal on it along. TCP
//! rides out the same gaps with retransmits. So here a QUIC connection that
//! *times out* counts against QUIC as a failed dial does, and
//! [`TROUBLE_THRESHOLD`] of them inside [`TROUBLE_WINDOW`] move the tunnel to
//! HTTP/2. A connection the edge closes on purpose is not counted: that is a
//! restart at Cloudflare, not this network.
//!
//! A dial that fails before QUIC has ever registered in this run falls back at
//! once, as cloudflared's does: that is UDP to port 7844 being blocked, and
//! waiting out more attempts would only spend the start budget.
//!
//! # Going back
//!
//! cloudflared tries QUIC again on the next reconnect after a fallback
//! connection has connected. Here the retry waits [`QUIC_RETRY_AFTER`] — a
//! network that just broke QUIC three times is unlikely to have mended within
//! the minute — and then happens at the next reconnect. One failed try puts
//! the tunnel straight back on HTTP/2 for another wait.
//!
//! ponytail: a healthy HTTP/2 connection is never torn down to try QUIC.
//! Moving would cut every stream on it, which is the harm the fallback is
//! there to stop; the retry waits for a reconnect that happens anyway.
//!
//! One [`Selector`] serves every HA connection of a tunnel: what breaks QUIC is
//! this machine's path to the edge, which they all share.

use std::collections::VecDeque;
use std::fmt;
use std::str::FromStr;
use std::time::{Duration, Instant};

use tracing::{info, warn};

/// Which transport a tunnel may use: the `[tunnel] protocol` setting, and
/// `CCTOP_TUNNEL_PROTOCOL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Protocol {
    /// QUIC, falling back to HTTP/2 when QUIC cannot get through or keeps
    /// dropping.
    #[default]
    Auto,
    /// QUIC only (UDP to port 7844).
    Quic,
    /// HTTP/2 over TLS over TCP only (TCP to port 7844).
    Http2,
}

impl Protocol {
    /// The word it is written as, which is cloudflared's `--protocol` word.
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Auto => "auto",
            Protocol::Quic => "quic",
            Protocol::Http2 => "http2",
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Protocol {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Ok(Protocol::Auto),
            "quic" => Ok(Protocol::Quic),
            // cloudflared upgrades its retired `h2mux` to http2, and says so.
            "http2" | "h2" | "h2mux" => Ok(Protocol::Http2),
            other => Err(format!(
                "unknown tunnel protocol {other:?}: use auto, quic or http2"
            )),
        }
    }
}

/// What one connection is dialled over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transport {
    Quic,
    Http2,
}

impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Transport::Quic => "quic",
            Transport::Http2 => "http2",
        })
    }
}

/// What went wrong with QUIC, for [`Selector::quic_trouble`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trouble {
    /// The QUIC handshake did not complete: no answer over UDP, or a refusal.
    Dial,
    /// The handshake completed but registration did not.
    Register,
    /// A registered connection died of its idle timeout: packets stopped
    /// arriving.
    TimedOut,
}

/// How many QUIC troubles inside [`TROUBLE_WINDOW`] move `auto` to HTTP/2.
pub(crate) const TROUBLE_THRESHOLD: usize = 3;

/// How far back a trouble still counts. Long enough that drops every few
/// minutes add up; short enough that one blip a day never does.
pub(crate) const TROUBLE_WINDOW: Duration = Duration::from_secs(30 * 60);

/// How long `auto` stays on HTTP/2 before it tries QUIC again, at the next
/// reconnect after that.
pub(crate) const QUIC_RETRY_AFTER: Duration = Duration::from_secs(30 * 60);

/// Chooses the transport of each dial, and learns from how QUIC fares.
#[derive(Debug)]
pub(crate) struct Selector {
    protocol: Protocol,
    /// When `auto` last moved to HTTP/2; `None` while it is on QUIC.
    fell_back_at: Option<Instant>,
    /// Recent QUIC troubles, oldest first, none older than the window.
    troubles: VecDeque<Instant>,
    /// Whether any connection of this tunnel has registered over QUIC.
    quic_ever_up: bool,
}

impl Selector {
    pub(crate) fn new(protocol: Protocol) -> Selector {
        Selector {
            protocol,
            fell_back_at: None,
            troubles: VecDeque::new(),
            quic_ever_up: false,
        }
    }

    /// The transport to dial next.
    pub(crate) fn transport(&self, now: Instant) -> Transport {
        match self.protocol {
            Protocol::Quic => Transport::Quic,
            Protocol::Http2 => Transport::Http2,
            Protocol::Auto => match self.fell_back_at {
                None => Transport::Quic,
                Some(at) if now.saturating_duration_since(at) >= QUIC_RETRY_AFTER => {
                    Transport::Quic
                }
                Some(_) => Transport::Http2,
            },
        }
    }

    /// Whether `auto` is on HTTP/2 now.
    #[cfg(test)]
    pub(crate) fn fell_back(&self) -> bool {
        self.fell_back_at.is_some()
    }

    /// A connection registered over `transport`.
    pub(crate) fn connected(&mut self, transport: Transport) {
        if transport != Transport::Quic {
            return;
        }
        self.quic_ever_up = true;
        if self.fell_back_at.take().is_some() {
            info!("QUIC registered again; the tunnel is back on QUIC");
            self.troubles.clear();
        }
    }

    /// QUIC failed in the way `trouble` says. Only `auto` acts on it.
    pub(crate) fn quic_trouble(&mut self, trouble: Trouble, now: Instant) {
        if self.protocol != Protocol::Auto {
            return;
        }
        // A retry of QUIC from HTTP/2 that failed: straight back, for another
        // wait. The troubles that caused the first fallback still stand.
        if self.fell_back_at.is_some() {
            self.fell_back_at = Some(now);
            info!(?trouble, "QUIC retry failed; staying on HTTP/2");
            return;
        }
        if trouble == Trouble::Dial && !self.quic_ever_up {
            self.fall_back(
                now,
                "QUIC could not reach the edge (UDP to port 7844 blocked?)",
            );
            return;
        }
        while let Some(&oldest) = self.troubles.front() {
            if now.saturating_duration_since(oldest) > TROUBLE_WINDOW {
                self.troubles.pop_front();
            } else {
                break;
            }
        }
        self.troubles.push_back(now);
        if self.troubles.len() >= TROUBLE_THRESHOLD {
            self.fall_back(now, "QUIC keeps failing on this network");
        }
    }

    fn fall_back(&mut self, now: Instant, why: &str) {
        warn!(
            troubles = self.troubles.len(),
            "{why}; switching the tunnel to HTTP/2 over TCP"
        );
        self.fell_back_at = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, secs: u64) -> Instant {
        start + Duration::from_secs(secs)
    }

    #[test]
    fn the_words_are_cloudflareds() {
        assert_eq!("auto".parse(), Ok(Protocol::Auto));
        assert_eq!(" QUIC ".parse(), Ok(Protocol::Quic));
        assert_eq!("http2".parse(), Ok(Protocol::Http2));
        assert_eq!("h2mux".parse(), Ok(Protocol::Http2));
        assert_eq!("".parse(), Ok(Protocol::Auto));
        assert!("udp".parse::<Protocol>().is_err());
        for p in [Protocol::Auto, Protocol::Quic, Protocol::Http2] {
            assert_eq!(p.to_string().parse(), Ok(p));
        }
        assert_eq!(Protocol::default(), Protocol::Auto);
    }

    #[test]
    fn a_fixed_protocol_never_moves() {
        let t0 = Instant::now();
        for (protocol, transport) in [
            (Protocol::Quic, Transport::Quic),
            (Protocol::Http2, Transport::Http2),
        ] {
            let mut s = Selector::new(protocol);
            for i in 0..10 {
                s.quic_trouble(Trouble::Dial, at(t0, i));
                s.quic_trouble(Trouble::TimedOut, at(t0, i));
            }
            assert_eq!(s.transport(at(t0, 10)), transport);
        }
    }

    #[test]
    fn auto_starts_on_quic() {
        assert_eq!(
            Selector::new(Protocol::Auto).transport(Instant::now()),
            Transport::Quic
        );
    }

    #[test]
    fn a_failed_first_dial_falls_back_at_once() {
        let t0 = Instant::now();
        let mut s = Selector::new(Protocol::Auto);
        s.quic_trouble(Trouble::Dial, t0);
        assert_eq!(s.transport(t0), Transport::Http2);
    }

    #[test]
    fn a_failed_first_registration_is_counted_not_acted_on() {
        // The handshake got through, so UDP works: one registration that went
        // wrong is the edge's business, as it would be over TCP.
        let t0 = Instant::now();
        let mut s = Selector::new(Protocol::Auto);
        s.quic_trouble(Trouble::Register, t0);
        assert_eq!(s.transport(t0), Transport::Quic);
    }

    #[test]
    fn once_quic_has_worked_a_dial_failure_is_only_counted() {
        let t0 = Instant::now();
        let mut s = Selector::new(Protocol::Auto);
        s.connected(Transport::Quic);
        s.quic_trouble(Trouble::Dial, t0);
        assert_eq!(s.transport(t0), Transport::Quic);
        s.quic_trouble(Trouble::Dial, at(t0, 2));
        assert_eq!(s.transport(at(t0, 2)), Transport::Quic);
        s.quic_trouble(Trouble::Dial, at(t0, 6));
        assert_eq!(s.transport(at(t0, 6)), Transport::Http2);
    }

    #[test]
    fn repeated_timeouts_of_a_working_quic_fall_back() {
        // The WSL2 case: registered fine, then dying of silence every so often.
        let t0 = Instant::now();
        let mut s = Selector::new(Protocol::Auto);
        s.connected(Transport::Quic);
        s.quic_trouble(Trouble::TimedOut, t0);
        s.connected(Transport::Quic);
        s.quic_trouble(Trouble::TimedOut, at(t0, 300));
        s.connected(Transport::Quic);
        assert_eq!(s.transport(at(t0, 300)), Transport::Quic);
        s.quic_trouble(Trouble::TimedOut, at(t0, 900));
        assert_eq!(s.transport(at(t0, 900)), Transport::Http2);
    }

    #[test]
    fn troubles_far_apart_never_add_up() {
        let t0 = Instant::now();
        let mut s = Selector::new(Protocol::Auto);
        s.connected(Transport::Quic);
        let spacing = TROUBLE_WINDOW.as_secs() / 2 + 1;
        for i in 0..20 {
            s.quic_trouble(Trouble::TimedOut, at(t0, i * spacing));
            s.connected(Transport::Quic);
        }
        assert_eq!(s.transport(at(t0, 20 * spacing)), Transport::Quic);
    }

    #[test]
    fn quic_is_tried_again_after_the_wait_and_kept_if_it_works() {
        let t0 = Instant::now();
        let mut s = Selector::new(Protocol::Auto);
        s.quic_trouble(Trouble::Dial, t0);
        let before = at(t0, QUIC_RETRY_AFTER.as_secs() - 1);
        assert_eq!(s.transport(before), Transport::Http2);
        // An HTTP/2 connection registering changes nothing.
        s.connected(Transport::Http2);
        assert!(s.fell_back());
        let after = at(t0, QUIC_RETRY_AFTER.as_secs());
        assert_eq!(s.transport(after), Transport::Quic);
        s.connected(Transport::Quic);
        assert!(!s.fell_back());
        assert_eq!(s.transport(after), Transport::Quic);
    }

    #[test]
    fn a_failed_retry_goes_straight_back_for_another_wait() {
        let t0 = Instant::now();
        let mut s = Selector::new(Protocol::Auto);
        s.quic_trouble(Trouble::Dial, t0);
        let retry = at(t0, QUIC_RETRY_AFTER.as_secs() + 5);
        assert_eq!(s.transport(retry), Transport::Quic);
        // Even a kind of trouble that would only be counted on QUIC.
        s.quic_trouble(Trouble::Register, retry);
        assert_eq!(s.transport(retry), Transport::Http2);
        let still = retry + QUIC_RETRY_AFTER - Duration::from_secs(1);
        assert_eq!(s.transport(still), Transport::Http2);
        assert_eq!(s.transport(retry + QUIC_RETRY_AFTER), Transport::Quic);
    }
}
