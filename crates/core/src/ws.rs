//! Just enough WebSocket (RFC 6455) to carry one terminal between two cctops.
//!
//! A sibling on the Cloudflare account is reached through its tunnel, and the
//! edge passes exactly two kinds of traffic on: requests, and WebSocket
//! upgrades. A terminal is neither a request nor something the rmux share
//! client could be rebuilt for in a pane (its protocol is an end-to-end
//! encrypted session meant for a browser), so the remote tab speaks the
//! simplest thing the edge carries: binary frames of terminal bytes each way,
//! and a text frame for a resize.
//!
//! Written here rather than pulled in: what is needed is one frame encoder and
//! one incremental decoder, about a hundred lines, against a WebSocket crate's
//! async runtime and handshake machinery that cctop already does by hand in
//! [`crate::peer`].
//!
//! Fragmented messages are reassembled. Neither end here ever sends one, but a
//! proxy between them is allowed to split a message, and a ping may arrive
//! between the pieces; read frame by frame, a continuation would lose which
//! kind of message it continues, and a resize split in two would reach the
//! pty as keystrokes.
//!
//! ponytail: no extensions. Neither end offers `permessage-deflate`, so no
//! frame here is ever compressed.

/// The GUID RFC 6455 fixes for the handshake's accept key.
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// A frame's opcode, as far as a terminal needs to tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Text,
    Binary,
    Close,
    Ping,
    Pong,
    /// A data opcode newer than this file: carried as data. Never a
    /// continuation, which [`Decoder`] folds into the message it continues.
    Other(u8),
}

impl Op {
    fn code(self) -> u8 {
        match self {
            Op::Text => 0x1,
            Op::Binary => 0x2,
            Op::Close => 0x8,
            Op::Ping => 0x9,
            Op::Pong => 0xA,
            Op::Other(code) => code & 0x0F,
        }
    }

    fn from_code(code: u8) -> Op {
        match code {
            0x1 => Op::Text,
            0x2 => Op::Binary,
            0x8 => Op::Close,
            0x9 => Op::Ping,
            0xA => Op::Pong,
            other => Op::Other(other),
        }
    }
}

/// What the server answers a handshake's `Sec-WebSocket-Key` with.
pub fn accept_key(key: &str) -> String {
    let digest = ring::digest::digest(
        &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
        format!("{}{GUID}", key.trim()).as_bytes(),
    );
    crate::util::b64_encode(digest.as_ref())
}

/// A fresh handshake key: sixteen random bytes, base64.
pub fn new_key() -> String {
    crate::util::b64_encode(&random::<16>())
}

fn random<const N: usize>() -> [u8; N] {
    use ring::rand::SecureRandom;
    let mut out = [0u8; N];
    // A failure leaves zeros, which is still a valid key and mask: neither is
    // a secret, both only stop a cache or a proxy confusing two connections.
    let _ = ring::rand::SystemRandom::new().fill(&mut out);
    out
}

/// One frame, finished. A client must mask what it sends and a server must
/// not, so the side is the caller's to say.
pub fn frame(op: Op, payload: &[u8], from_client: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 14);
    out.push(0x80 | op.code());
    let mask_bit = if from_client { 0x80 } else { 0 };
    match payload.len() {
        n if n < 126 => out.push(mask_bit | n as u8),
        n if n <= u16::MAX as usize => {
            out.push(mask_bit | 126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(mask_bit | 127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    match from_client {
        true => {
            let mask = random::<4>();
            out.extend_from_slice(&mask);
            out.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        }
        false => out.extend_from_slice(payload),
    }
    out
}

/// The largest message either end accepts, whole or in fragments. A
/// terminal's messages are a read's worth of output or a keystroke; anything
/// near this is not one, and holding it in memory to find out would be the
/// attack.
const MAX_MESSAGE: u64 = 16 * 1024 * 1024;

/// RFC 6455 caps a control frame's payload at 125 bytes, so nothing larger
/// is buffered to find out it is a ping.
const MAX_CONTROL: u64 = 125;

/// The continuation opcode: more of the message a non-final frame began.
const CONTINUATION: u8 = 0x0;

/// Messages out of a byte stream that arrives in whatever pieces the socket
/// hands over.
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
    /// A message begun by a frame without FIN: its kind, and what has arrived
    /// of it so far.
    partial: Option<(Op, Vec<u8>)>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The next whole message, unmasked and reassembled from its fragments,
    /// or `None` until more has arrived. A control frame between two
    /// fragments is returned as it comes, ahead of the message it interrupts,
    /// so a ping is answered without waiting for the rest.
    ///
    /// `Err` for anything that is not a terminal's or not a WebSocket's: a
    /// message too large, a fragmented control frame, a continuation with
    /// nothing to continue, or a new message started inside another.
    pub fn frame(&mut self) -> Result<Option<(Op, Vec<u8>)>, String> {
        loop {
            let Some((fin, code, payload)) = self.next_frame()? else {
                return Ok(None);
            };
            let op = Op::from_code(code);
            if code & 0x8 != 0 {
                if !fin {
                    return Err("a control frame cannot be fragmented".into());
                }
                return Ok(Some((op, payload)));
            }
            match (code, self.partial.take()) {
                (CONTINUATION, None) => {
                    return Err("a continuation frame with nothing to continue".into());
                }
                (CONTINUATION, Some((op, mut so_far))) => {
                    so_far.extend_from_slice(&payload);
                    match fin {
                        true => return Ok(Some((op, so_far))),
                        false => self.partial = Some((op, so_far)),
                    }
                }
                (_, Some(_)) => {
                    return Err("a new message began before the last one ended".into());
                }
                (_, None) if fin => return Ok(Some((op, payload))),
                (_, None) => self.partial = Some((op, payload)),
            }
        }
    }

    /// One frame off the front of the buffer — its FIN bit, opcode and
    /// unmasked payload — or `None` until all of it has arrived.
    ///
    /// The size is checked against what the frame may still add, before its
    /// payload is waited for: a data frame against what is left of
    /// [`MAX_MESSAGE`] after the fragments already held, a control frame
    /// against [`MAX_CONTROL`].
    fn next_frame(&mut self) -> Result<Option<(bool, u8, Vec<u8>)>, String> {
        let b = &self.buf;
        if b.len() < 2 {
            return Ok(None);
        }
        let fin = b[0] & 0x80 != 0;
        let code = b[0] & 0x0F;
        let masked = b[1] & 0x80 != 0;
        let (len, mut at) = match b[1] & 0x7F {
            126 if b.len() >= 4 => (u16::from_be_bytes([b[2], b[3]]) as u64, 4),
            127 if b.len() >= 10 => {
                let mut eight = [0u8; 8];
                eight.copy_from_slice(&b[2..10]);
                (u64::from_be_bytes(eight), 10)
            }
            126 | 127 => return Ok(None),
            n => (n as u64, 2),
        };
        let held = self.partial.as_ref().map_or(0, |(_, p)| p.len() as u64);
        if code & 0x8 != 0 && len > MAX_CONTROL {
            return Err(format!("a {len}-byte control frame is not a WebSocket's"));
        }
        if code & 0x8 == 0 && held + len > MAX_MESSAGE {
            return Err(format!("a {}-byte message is not a terminal's", held + len));
        }
        let mask = match masked {
            true if b.len() >= at + 4 => {
                let m = [b[at], b[at + 1], b[at + 2], b[at + 3]];
                at += 4;
                Some(m)
            }
            true => return Ok(None),
            false => None,
        };
        let end = at + len as usize;
        if b.len() < end {
            return Ok(None);
        }
        let mut payload = b[at..end].to_vec();
        if let Some(mask) = mask {
            for (i, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[i % 4];
            }
        }
        self.buf.drain(..end);
        Ok(Some((fin, code, payload)))
    }
}

/// A resize, as the text frame carries it: `{"cols":N,"rows":M}`.
pub fn resize_message(cols: u16, rows: u16) -> String {
    serde_json::json!({ "cols": cols, "rows": rows }).to_string()
}

/// Read a resize text frame back. Sizes outside what a terminal can be are
/// refused rather than clamped: they are not a terminal's.
pub fn parse_resize(text: &[u8]) -> Option<(u16, u16)> {
    let v: serde_json::Value = serde_json::from_slice(text).ok()?;
    let dim = |key: &str| {
        v.get(key)
            .and_then(serde_json::Value::as_u64)
            .filter(|n| (1..=4000).contains(n))
            .map(|n| n as u16)
    };
    Some((dim("cols")?, dim("rows")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked example from RFC 6455 section 1.3.
    #[test]
    fn the_accept_key_is_the_rfcs() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    /// Every length encoding, from either side, comes back as it went in —
    /// including when it arrives a byte at a time.
    #[test]
    fn frames_round_trip_whatever_pieces_they_arrive_in() {
        for len in [0usize, 5, 125, 126, 300, 65_535, 65_536, 70_000] {
            let payload: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            for from_client in [true, false] {
                let wire = frame(Op::Binary, &payload, from_client);
                let mut d = Decoder::default();
                let mut got = None;
                for chunk in wire.chunks(if len < 300 { 1 } else { 4096 }) {
                    assert!(got.is_none(), "a frame finished early");
                    d.push(chunk);
                    got = d.frame().expect("a frame");
                }
                assert_eq!(got, Some((Op::Binary, payload.clone())), "{len}");
                assert_eq!(d.frame().unwrap(), None);
            }
        }
        // Two in one read are two.
        let mut d = Decoder::default();
        d.push(&[frame(Op::Text, b"a", true), frame(Op::Close, b"", false)].concat());
        assert_eq!(d.frame().unwrap(), Some((Op::Text, b"a".to_vec())));
        assert_eq!(d.frame().unwrap(), Some((Op::Close, Vec::new())));
    }

    #[test]
    fn a_frame_too_large_to_be_a_terminals_is_refused_before_it_is_read() {
        let mut d = Decoder::default();
        d.push(&[0x82, 127, 0, 0, 0, 1, 0, 0, 0, 0]);
        assert!(d.frame().is_err());
    }

    /// A frame with its first byte set by hand: FIN or not, and any opcode,
    /// including the continuation that [`frame`] never writes.
    fn piece(fin: bool, code: u8, payload: &[u8], from_client: bool) -> Vec<u8> {
        let mut wire = frame(Op::Binary, payload, from_client);
        wire[0] = if fin { 0x80 } else { 0 } | code;
        wire
    }

    /// A message split in three reads back as one, of the kind its first
    /// piece said — however the bytes arrive, and with a ping between pieces
    /// answered ahead of it.
    #[test]
    fn a_fragmented_message_is_reassembled_around_control_frames() {
        for from_client in [true, false] {
            let wire = [
                piece(false, 0x1, b"{\"cols\":", from_client),
                piece(true, 0x9, b"hi", from_client),
                piece(false, CONTINUATION, b"120,\"rows\"", from_client),
                piece(false, CONTINUATION, b"", from_client),
                piece(true, CONTINUATION, b":40}", from_client),
                frame(Op::Binary, b"after", from_client),
            ]
            .concat();
            for chunk in [1, 3, wire.len()] {
                let mut d = Decoder::default();
                let mut got = Vec::new();
                for bytes in wire.chunks(chunk) {
                    d.push(bytes);
                    while let Some(message) = d.frame().unwrap() {
                        got.push(message);
                    }
                }
                assert_eq!(
                    got,
                    [
                        (Op::Ping, b"hi".to_vec()),
                        (Op::Text, resize_message(120, 40).into_bytes()),
                        (Op::Binary, b"after".to_vec()),
                    ],
                    "{chunk}-byte reads"
                );
                assert_eq!(parse_resize(&got[1].1), Some((120, 40)));
            }
        }
    }

    #[test]
    fn fragments_that_break_the_protocol_are_refused() {
        let refused = |wire: Vec<u8>| {
            let mut d = Decoder::default();
            d.push(&wire);
            loop {
                match d.frame() {
                    Err(_) => return true,
                    Ok(None) => return false,
                    Ok(Some(_)) => {}
                }
            }
        };
        // A continuation of nothing.
        assert!(refused(piece(true, CONTINUATION, b"x", true)));
        // A new message inside an unfinished one.
        assert!(refused(
            [piece(false, 0x2, b"a", true), piece(true, 0x2, b"b", true)].concat()
        ));
        // A control frame in pieces, and one past the RFC's 125 bytes.
        assert!(refused(piece(false, 0x9, b"ping", true)));
        assert!(refused(piece(true, 0x9, &[0; 126], true)));
        // The well-formed version of each is not refused.
        assert!(!refused(
            [
                piece(false, 0x2, b"a", true),
                piece(true, CONTINUATION, b"b", true)
            ]
            .concat()
        ));
    }

    /// The cap is on the message, not the frame: fragments that each fit but
    /// together do not are refused at the header of the one that tips it over,
    /// before its payload is waited for.
    #[test]
    fn the_size_cap_holds_across_fragments() {
        let mut d = Decoder::default();
        let half = vec![0u8; (MAX_MESSAGE / 2) as usize];
        d.push(&piece(false, 0x2, &half, false));
        assert_eq!(d.frame().unwrap(), None);
        d.push(&piece(false, CONTINUATION, &half, false));
        assert_eq!(d.frame().unwrap(), None);
        // One more byte's header, without the byte.
        d.push(&[CONTINUATION | 0x80, 1]);
        assert!(d.frame().is_err());
    }

    #[test]
    fn a_resize_reads_back_and_nonsense_does_not() {
        assert_eq!(
            parse_resize(resize_message(120, 40).as_bytes()),
            Some((120, 40))
        );
        for junk in [
            &b"{}"[..],
            b"{\"cols\":0,\"rows\":4}",
            b"{\"cols\":99999,\"rows\":4}",
            b"cols",
        ] {
            assert_eq!(parse_resize(junk), None);
        }
    }
}
