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
//! Terminal output is compressed when both ends agree: `permessage-deflate`
//! (RFC 7692), negotiated in the handshake — see [`accept_deflate`]. A peer
//! older than this offers or accepts nothing, and every frame stays as it was.
//!
//! With context takeover — the deflate window carried from one message to the
//! next — rather than without. A terminal's messages are one `read` of a pty
//! each, often a few hundred bytes, and what makes them compress is what they
//! share with the ones before: the same escape sequences, the same status
//! line redrawn every tenth of a second, a screen repainted with what it
//! already held. Each alone has little to find in itself. Measured on Claude
//! Code's screen, the window is most of the saving: its working spinner,
//! repainted three hundred times, goes over the wire at 4% of its size with
//! the window and 36% without (the pull request that brought this in has the
//! rest). It costs no memory that a fresh window would not: the compressor
//! and decompressor are held for the connection either way, a few hundred
//! kilobytes per open terminal.
//!
//! Only the server compresses. What a tab sends is keystrokes and resizes, a
//! few bytes each, which deflate would make no smaller; RFC 7692 lets either
//! end leave any message uncompressed, so the tab never sets the bit, and the
//! server still reads one that does.
//!
//! What a compressed message decompresses to is held to the same
//! [`MAX_MESSAGE`] as any other, counted while it is inflated, so a few
//! kilobytes that would inflate to gigabytes are refused at the cap rather
//! than after.

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
    /// An opcode newer than this file. Never a continuation, which
    /// [`Decoder`] folds into the message it continues and returns under the
    /// opcode it began with; so neither end reads this as terminal bytes, and
    /// both pass it over rather than type or draw what it might not be.
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
    frame_with(0, op, payload, from_client)
}

/// [`frame`] with the RSV bits in `rsv` set: RSV1 (0x40) marks a compressed
/// message.
fn frame_with(rsv: u8, op: Op, payload: &[u8], from_client: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 14);
    out.push(0x80 | rsv | op.code());
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

/// The extension's name, and what a tab offers in `Sec-WebSocket-Extensions`.
pub const DEFLATE: &str = "permessage-deflate";

/// RSV1: the frame begins a compressed message.
const RSV1: u8 = 0x40;

/// The four bytes a sync flush ends with, which RFC 7692 has the sender drop
/// and the receiver put back.
const FLUSH_TAIL: [u8; 4] = [0, 0, 0xff, 0xff];

/// Below this, a message goes uncompressed: deflate's own framing would eat
/// what it saved on a keystroke's echo.
const DEFLATE_FROM: usize = 32;

/// The server's answer to a tab's `Sec-WebSocket-Extensions`, when one of its
/// offers is a `permessage-deflate` this end can honour: the header value to
/// answer with, and the compressor to send with.
///
/// Every parameter RFC 7692 defines is understood. One this end cannot honour
/// declines that offer rather than the connection — a smaller server window
/// than deflate's 32 KiB, which miniz cannot make — and an unknown or
/// repeated parameter is not an offer at all.
pub fn accept_deflate(offers: &str) -> Option<(String, Deflater)> {
    offers.split(',').find_map(|offer| {
        let mut parts = offer.split(';').map(str::trim);
        if !parts.next()?.eq_ignore_ascii_case(DEFLATE) {
            return None;
        }
        let mut answer = DEFLATE.to_string();
        let mut reset = false;
        let mut seen = Vec::new();
        for param in parts {
            let (name, value) = match param.split_once('=') {
                Some((n, v)) => (n.trim(), Some(v.trim().trim_matches('"'))),
                None => (param, None),
            };
            let name = name.to_ascii_lowercase();
            if seen.contains(&name) {
                return None;
            }
            match (name.as_str(), value) {
                ("server_no_context_takeover", None) => {
                    reset = true;
                    answer.push_str("; server_no_context_takeover");
                }
                // The tab's window is the tab's to keep or not: the server
                // inflates with the largest either way.
                ("client_no_context_takeover", None) => {}
                ("server_max_window_bits", Some("15")) => {
                    answer.push_str("; server_max_window_bits=15");
                }
                ("client_max_window_bits", None) => {}
                ("client_max_window_bits", Some(bits))
                    if bits.parse::<u8>().is_ok_and(|b| (8..=15).contains(&b)) => {}
                _ => return None,
            }
            seen.push(name);
        }
        Some((answer, Deflater::new(reset)))
    })
}

/// Whether a server's `Sec-WebSocket-Extensions` answer agreed to deflate,
/// so its messages may arrive compressed.
pub fn deflate_agreed(answer: &str) -> bool {
    answer.split(',').any(|ext| {
        ext.split(';')
            .next()
            .is_some_and(|name| name.trim().eq_ignore_ascii_case(DEFLATE))
    })
}

/// The sending half of `permessage-deflate`: one deflate stream across the
/// connection's messages, or a fresh one for each when the tab asked for
/// `server_no_context_takeover`.
pub struct Deflater {
    stream: flate2::Compress,
    reset: bool,
}

impl Deflater {
    fn new(reset: bool) -> Deflater {
        Deflater {
            stream: flate2::Compress::new(flate2::Compression::default(), false),
            reset,
        }
    }

    /// A server's binary frame of `payload`, compressed when that is worth it.
    pub fn binary(&mut self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < DEFLATE_FROM {
            return frame(Op::Binary, payload, false);
        }
        match self.compress(payload) {
            Some(packed) => frame_with(RSV1, Op::Binary, &packed, false),
            None => frame(Op::Binary, payload, false),
        }
    }

    /// `payload` deflated and sync-flushed, without the flush's tail. `None`
    /// when deflate failed, which leaves the stream unusable: it is reset,
    /// and the message goes uncompressed, which RFC 7692 allows any message.
    fn compress(&mut self, payload: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(payload.len() / 2 + 64);
        let start = self.stream.total_in();
        loop {
            let used = (self.stream.total_in() - start) as usize;
            if out.capacity() - out.len() < 64 {
                out.reserve(out.capacity().max(1024));
            }
            let ok = self
                .stream
                .compress_vec(&payload[used..], &mut out, flate2::FlushCompress::Sync)
                .is_ok();
            if !ok {
                self.stream.reset();
                return None;
            }
            // All of it taken, and room to spare: the flush is complete.
            let used = (self.stream.total_in() - start) as usize;
            if used == payload.len() && out.len() < out.capacity() {
                break;
            }
        }
        if self.reset {
            self.stream.reset();
        }
        if !out.ends_with(&FLUSH_TAIL) {
            self.stream.reset();
            return None;
        }
        out.truncate(out.len() - FLUSH_TAIL.len());
        Some(out)
    }
}

/// Messages out of a byte stream that arrives in whatever pieces the socket
/// hands over.
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
    /// A message begun by a frame without FIN: its kind, whether it is
    /// compressed, and what has arrived of it so far.
    partial: Option<(Op, bool, Vec<u8>)>,
    /// The connection's inflate stream, once deflate is agreed. A message
    /// marked compressed without it is a protocol error.
    inflate: Option<flate2::Decompress>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Read compressed messages from here on: deflate was agreed.
    pub fn inflating(mut self) -> Decoder {
        self.inflate = Some(flate2::Decompress::new(false));
        self
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
            let Some((flags, code, payload)) = self.next_frame()? else {
                return Ok(None);
            };
            let (fin, rsv) = (flags & 0x80 != 0, flags & 0x70);
            let op = Op::from_code(code);
            // RSV1 only on the first frame of a data message, and only once
            // deflate is agreed; no extension here gives RSV2 or RSV3 a
            // meaning.
            let compressed = rsv & RSV1 != 0;
            if rsv & !RSV1 != 0
                || (compressed && (code & 0x8 != 0 || code == CONTINUATION))
                || (compressed && self.inflate.is_none())
            {
                return Err("a frame with reserved bits nobody agreed to".into());
            }
            if code & 0x8 != 0 {
                if !fin {
                    return Err("a control frame cannot be fragmented".into());
                }
                return Ok(Some((op, payload)));
            }
            let whole = match (code, self.partial.take()) {
                (CONTINUATION, None) => {
                    return Err("a continuation frame with nothing to continue".into());
                }
                (CONTINUATION, Some((op, compressed, mut so_far))) => {
                    so_far.extend_from_slice(&payload);
                    match fin {
                        true => (op, compressed, so_far),
                        false => {
                            self.partial = Some((op, compressed, so_far));
                            continue;
                        }
                    }
                }
                (_, Some(_)) => {
                    return Err("a new message began before the last one ended".into());
                }
                (_, None) if fin => (op, compressed, payload),
                (_, None) => {
                    self.partial = Some((op, compressed, payload));
                    continue;
                }
            };
            return match whole {
                (op, true, packed) => Ok(Some((op, self.inflate(packed)?))),
                (op, false, payload) => Ok(Some((op, payload))),
            };
        }
    }

    /// A compressed message's payload, inflated — refused once it passes
    /// [`MAX_MESSAGE`], before the rest of it is inflated.
    fn inflate(&mut self, mut packed: Vec<u8>) -> Result<Vec<u8>, String> {
        let stream = self
            .inflate
            .as_mut()
            .ok_or("a compressed message nobody agreed to")?;
        packed.extend_from_slice(&FLUSH_TAIL);
        let cap = MAX_MESSAGE as usize;
        let mut out = Vec::with_capacity((packed.len() * 4).min(cap));
        let start = stream.total_in();
        loop {
            let used = (stream.total_in() - start) as usize;
            if out.len() == out.capacity() {
                if out.len() > cap {
                    return Err(format!("a message inflating past {cap} bytes"));
                }
                // One past the cap, so a message that reaches it is seen to.
                out.reserve((out.capacity().max(1024)).min(cap + 1 - out.len()));
            }
            let before = (stream.total_in(), stream.total_out());
            stream
                .decompress_vec(&packed[used..], &mut out, flate2::FlushDecompress::Sync)
                .map_err(|e| format!("a compressed message that does not inflate: {e}"))?;
            if out.len() > cap {
                return Err(format!("a message inflating past {cap} bytes"));
            }
            let used = (stream.total_in() - start) as usize;
            let stuck = (stream.total_in(), stream.total_out()) == before;
            // All of it taken, and the output not cut short by room — or
            // nothing more coming out of nothing more going in.
            if used == packed.len() && (out.len() < out.capacity() || stuck) {
                return Ok(out);
            }
            if stuck {
                return Err("a compressed message that does not inflate".into());
            }
        }
    }

    /// One frame off the front of the buffer — its FIN and RSV bits, opcode
    /// and unmasked payload — or `None` until all of it has arrived.
    ///
    /// The size is checked against what the frame may still add, before its
    /// payload is waited for: a data frame against what is left of
    /// [`MAX_MESSAGE`] after the fragments already held, a control frame
    /// against [`MAX_CONTROL`].
    fn next_frame(&mut self) -> Result<Option<(u8, u8, Vec<u8>)>, String> {
        let b = &self.buf;
        if b.len() < 2 {
            return Ok(None);
        }
        let b0 = b[0];
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
        let held = self.partial.as_ref().map_or(0, |(_, _, p)| p.len() as u64);
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
        Ok(Some((b0 & 0xF0, code, payload)))
    }
}

/// The header a terminal's resume rides on, both ways.
///
/// Asked with [`RESUME_NEW`] on an open, it says the tab can come back; the
/// server answers the upgrade with a token in it. A tab that lost its socket
/// asks again with `<token> <offset>`, the offset being how many bytes of
/// the terminal's output it has been handed, and is answered with the rest
/// on the same pty — or `410 Gone` when that cannot be done. A cctop older
/// than this neither sends nor reads it, and the terminal is opened afresh
/// each time, as it always was. See the server's `resume` module.
pub const RESUME_HEADER: &str = "Cctop-Resume";

/// [`RESUME_HEADER`]'s value on an open that may be resumed later.
pub const RESUME_NEW: &str = "new";

/// The text frame a resumed tab sends to have the far program draw its
/// screen again — after the tab wrote over it to say it was reconnecting.
/// A server older than resume never sees it: only a resumed socket sends it.
///
/// Tabs no longer send it — they paint back the one row they wrote over from
/// their own shadow of the screen, since a same-size SIGWINCH reached the far
/// rmux client only sometimes — but a server still answers it, for the tabs
/// of cctops that do.
pub const REDRAW: &str = r#"{"redraw":true}"#;

/// Whether a text frame is [`REDRAW`].
pub fn is_redraw(text: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(text)
        .is_ok_and(|v| v.get("redraw") == Some(&serde_json::Value::Bool(true)))
}

/// The environment variable that turns on [`trace`].
pub const TRACE_ENV: &str = "CCTOP_TRACE_WS";

/// Past this size the trace starts again, the old one kept beside it as
/// `.old`: enough for days of drops, never a disk filled by one forgotten
/// variable.
const TRACE_CAP: u64 = 1024 * 1024;

/// Where [`trace`] writes: `cctop/ws-trace.log` in the runtime directory.
pub fn trace_path() -> std::path::PathBuf {
    crate::config::runtime_base()
        .join("cctop")
        .join("ws-trace.log")
}

/// With `CCTOP_TRACE_WS=1`, one line about a terminal socket — a drop, its
/// reason and timing, how getting back went — appended to [`trace_path`].
///
/// Both ends write it, the tab's bridge and the server's terminal, each line
/// marked with its `side` and process, so the next "it keeps dropping" can be
/// read off the file on either machine rather than guessed from a screen.
/// The file existing turns it on too: a tab's bridge is started by the mux
/// daemon, whose environment is whatever it was started with, and `touch` on
/// the path reaches every process at once. Failing to write is never the
/// terminal's problem.
pub fn trace(side: &str, what: std::fmt::Arguments) {
    static ON: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
        std::env::var(TRACE_ENV).is_ok_and(|v| !v.is_empty() && v != "0")
    });
    // Tests drop and resume sockets by the dozen, into the real runtime
    // directory's trace when the machine running them has one.
    if crate::under_test() {
        return;
    }
    let path = trace_path();
    if *ON || path.exists() {
        trace_to(&path, side, what);
    }
}

/// Private, both the file and the directory made for it: the runtime
/// directory falls back to `~/.cache` where there is no `$XDG_RUNTIME_DIR`
/// (ssh, `sudo -u`), and a trace names every sibling and terminal route.
fn trace_to(path: &std::path::Path, side: &str, what: std::fmt::Arguments) {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    if let Some(dir) = path.parent() {
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir);
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > TRACE_CAP) {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    let line = format!(
        "{} {side}[{}] {what}\n",
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z"),
        std::process::id(),
    );
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
    {
        // `mode` reaches only a file this call made, and the docs have
        // people make it with `touch`.
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        let _ = file.write_all(line.as_bytes());
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

    /// Each trace is one line, stamped and marked with its side, appended;
    /// past the cap the file starts again with the old one kept beside it.
    #[test]
    fn a_trace_is_one_marked_line_and_never_grows_without_bound() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cctop").join("ws-trace.log");
        trace_to(&path, "bridge", format_args!("drop: the connection ended"));
        trace_to(&path, "serve", format_args!("parked"));
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(
            lines[0].contains(" bridge[") && lines[0].ends_with("] drop: the connection ended")
        );
        assert!(lines[1].contains(" serve[") && lines[1].ends_with("] parked"));
        std::fs::write(&path, vec![b'x'; TRACE_CAP as usize + 1]).unwrap();
        trace_to(&path, "bridge", format_args!("after"));
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .ends_with("] after\n")
        );
        assert!(path.with_extension("log.old").exists());
        let mode = |p: &std::path::Path| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(p).unwrap().permissions().mode() & 0o777
        };
        assert_eq!(mode(&path), 0o600, "after rotation");
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        // One made world-readable by hand is made private on the next line.
        let touched = dir.path().join("touched.log");
        std::fs::write(&touched, b"").unwrap();
        std::fs::set_permissions(
            &touched,
            std::os::unix::fs::PermissionsExt::from_mode(0o644),
        )
        .unwrap();
        trace_to(&touched, "bridge", format_args!("x"));
        assert_eq!(mode(&touched), 0o600);
    }

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

    /// RFC 7692 section 7.2.3's own frames: "Hello" compressed, then again
    /// with the window kept, which the second one leans on.
    #[test]
    fn the_rfcs_compressed_hellos_inflate() {
        let mut d = Decoder::default().inflating();
        d.push(&[0xc1, 0x07, 0xf2, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00]);
        assert_eq!(d.frame().unwrap(), Some((Op::Text, b"Hello".to_vec())));
        d.push(&[0xc1, 0x05, 0xf2, 0x00, 0x11, 0x00, 0x00]);
        assert_eq!(d.frame().unwrap(), Some((Op::Text, b"Hello".to_vec())));
        // The same, split across a non-final frame and its continuation:
        // RSV1 is on the first, never the continuation.
        let mut d = Decoder::default().inflating();
        d.push(&[
            0x41, 0x03, 0xf2, 0x48, 0xcd, 0x80, 0x04, 0xc9, 0xc9, 0x07, 0x00,
        ]);
        assert_eq!(d.frame().unwrap(), Some((Op::Text, b"Hello".to_vec())));
    }

    /// What the server deflates, the tab inflates, message after message on
    /// one window — or a fresh one each, when that was asked for — and
    /// however the bytes arrive. A keystroke's echo goes uncompressed.
    #[test]
    fn deflated_messages_round_trip_with_and_without_the_window() {
        let screen: Vec<Vec<u8>> = (0..40)
            .map(|i| {
                format!("\x1b[{i};1H\x1b[38;5;153m line {i}: the same old status\x1b[39m\x1b[K")
                    .into_bytes()
            })
            .collect();
        for offer in [DEFLATE, "permessage-deflate; server_no_context_takeover"] {
            let (_, mut deflater) = accept_deflate(offer).expect("accepted");
            let mut wire = Vec::new();
            for message in &screen {
                wire.extend(deflater.binary(message));
            }
            wire.extend(deflater.binary(b"x"));
            let mut d = Decoder::default().inflating();
            let mut got = Vec::new();
            for chunk in wire.chunks(7) {
                d.push(chunk);
                while let Some((op, payload)) = d.frame().unwrap() {
                    assert_eq!(op, Op::Binary);
                    got.push(payload);
                }
            }
            let mut want = screen.clone();
            want.push(b"x".to_vec());
            assert_eq!(got, want, "{offer}");
        }
        let (_, mut deflater) = accept_deflate(DEFLATE).unwrap();
        assert_eq!(
            deflater.binary(b"x")[0],
            0x82,
            "a keystroke is not compressed"
        );
        assert_eq!(deflater.binary(&screen[0])[0], 0xc2, "a line of screen is");
    }

    /// A compressed frame on a connection that agreed to none, RSV1 on a
    /// control frame or a continuation, and RSV2 or RSV3 at all, are each a
    /// protocol error.
    #[test]
    fn reserved_bits_nobody_agreed_to_are_refused() {
        let refused = |wire: &[u8], inflating: bool| {
            let mut d = Decoder::default();
            if inflating {
                d = d.inflating();
            }
            d.push(wire);
            d.frame().is_err()
        };
        let hello = [0xc1, 0x07, 0xf2, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00];
        assert!(refused(&hello, false));
        assert!(!refused(&hello, true));
        assert!(refused(&[0xc9, 0x00], true), "a compressed ping");
        assert!(refused(&[0xa2, 0x00], true), "RSV2");
        assert!(refused(&[0x92, 0x00], true), "RSV3");
        assert!(
            refused(&[[0x02, 0x01, b'a'], [0xc0, 0x01, b'b']].concat(), true),
            "RSV1 on a continuation"
        );
    }

    /// A few kilobytes that would inflate to more than a message may be are
    /// refused as they pass the cap.
    #[test]
    fn a_compressed_message_is_capped_by_what_it_inflates_to() {
        let zeros = vec![0u8; MAX_MESSAGE as usize + 1024];
        let mut c = flate2::Compress::new(flate2::Compression::best(), false);
        let mut packed = Vec::with_capacity(64 * 1024);
        c.compress_vec(&zeros, &mut packed, flate2::FlushCompress::Sync)
            .unwrap();
        assert!(packed.len() < 64 * 1024, "{}", packed.len());
        packed.truncate(packed.len() - FLUSH_TAIL.len());
        let mut d = Decoder::default().inflating();
        d.push(&frame_with(RSV1, Op::Binary, &packed, false));
        let why = d.frame().unwrap_err();
        assert!(why.contains("inflating past"), "{why}");
        // Just under the cap is a message.
        let under = vec![0u8; MAX_MESSAGE as usize];
        let (_, mut deflater) = accept_deflate(DEFLATE).unwrap();
        let mut d = Decoder::default().inflating();
        d.push(&deflater.binary(&under));
        assert_eq!(d.frame().unwrap().map(|(_, p)| p.len()), Some(under.len()));
    }

    /// The server takes the first offer it can honour, and says back only
    /// what it agreed to; an offer it cannot honour is declined, not
    /// misread.
    #[test]
    fn a_deflate_offer_is_taken_only_as_far_as_it_can_be_honoured() {
        let answer = |offers: &str| accept_deflate(offers).map(|(a, _)| a);
        assert_eq!(answer(DEFLATE).as_deref(), Some(DEFLATE));
        assert_eq!(
            answer("permessage-deflate; client_max_window_bits").as_deref(),
            Some(DEFLATE)
        );
        assert_eq!(
            answer("permessage-deflate; server_no_context_takeover; client_no_context_takeover")
                .as_deref(),
            Some("permessage-deflate; server_no_context_takeover")
        );
        assert_eq!(
            answer("permessage-deflate; server_max_window_bits=15").as_deref(),
            Some("permessage-deflate; server_max_window_bits=15")
        );
        // A smaller window than miniz makes: that offer declined, the next
        // one taken.
        assert_eq!(
            answer("permessage-deflate; server_max_window_bits=10, permessage-deflate").as_deref(),
            Some(DEFLATE)
        );
        for declined in [
            "",
            "x-webkit-deflate-frame",
            "permessage-deflate; server_max_window_bits=10",
            "permessage-deflate; client_max_window_bits=99",
            "permessage-deflate; mystery",
            "permessage-deflate; server_no_context_takeover; server_no_context_takeover",
        ] {
            assert_eq!(answer(declined), None, "{declined}");
        }
        assert!(deflate_agreed(
            "permessage-deflate; server_no_context_takeover"
        ));
        assert!(!deflate_agreed(""));
        assert!(!deflate_agreed("x-webkit-deflate-frame"));
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
        // A redraw is not a resize, nor a resize a redraw.
        assert_eq!(parse_resize(REDRAW.as_bytes()), None);
        assert!(is_redraw(REDRAW.as_bytes()));
        assert!(!is_redraw(resize_message(120, 40).as_bytes()));
        assert!(!is_redraw(br#"{"redraw":1}"#));
    }
}
