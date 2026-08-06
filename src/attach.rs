//! Watching an agent's terminal from inside cctop.
//!
//! The [`shim`](crate::shim) already owns the pty of anything started with
//! `cctop run`, so it can hand out a copy of everything the agent draws. This is
//! the other end: connect, rebuild the screen from the replay, and keep feeding
//! the parser as the agent paints. Keystrokes travel back up the same socket,
//! which is the path [`inject`](crate::inject) has always used.
//!
//! Nothing here is platform-specific except the socket, so the parser and the
//! key encoding compile everywhere; [`attach`] simply never succeeds off unix.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io::Write;

/// A live connection to the shim driving one agent.
pub struct Attach {
    input: Box<dyn Write + Send>,
    /// Filled by the reader thread, drained by [`Attach::pump`]. A buffer rather
    /// than a channel because the UI loop already wakes on a timer; a channel
    /// would add plumbing and arrive no sooner.
    pending: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    /// Screen the agent believes it is drawing on, `(cols, rows)`.
    pub size: (u16, u16),
    /// The agent's screen, rebuilt from its output.
    pub parser: vt100::Parser,
}

impl Attach {
    /// Fold whatever has arrived into the screen. True when it changed.
    pub fn pump(&mut self) -> bool {
        let bytes = {
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *pending)
        };
        if bytes.is_empty() {
            return false;
        }
        self.parser.process(&bytes);
        true
    }

    /// Type a key into the agent. Returns false once the connection is gone.
    pub fn send_key(&mut self, key: KeyEvent) -> bool {
        match encode(key) {
            Some(bytes) => self
                .input
                .write_all(&bytes)
                .and_then(|()| self.input.flush())
                .is_ok(),
            None => true,
        }
    }
}

/// Connect to the shim owning `pid` and start collecting its output.
///
/// `None` when this agent wasn't started by `cctop run` — the socket is the only
/// way in, and there isn't one.
#[cfg(unix)]
pub fn attach(pid: u32) -> Option<Attach> {
    use std::io::{BufRead, BufReader, Read};

    let path = crate::shim::socket_path(pid)?;
    let mut stream = std::os::unix::net::UnixStream::connect(path).ok()?;
    stream.write_all(crate::shim::ATTACH_MAGIC).ok()?;

    // Buffered so the header line can be read without swallowing the replay
    // that follows it in the same chunk.
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut header = String::new();
    reader.read_line(&mut header).ok()?;
    let size = parse_size(&header)?;

    let pending = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let collector = std::sync::Arc::clone(&pending);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            collector
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .extend_from_slice(&buf[..n]);
        }
    });
    Some(Attach {
        input: Box::new(stream),
        pending,
        size,
        // No scrollback: this shows what the agent is showing. Its own history
        // is in the transcript, which the panels already read.
        parser: vt100::Parser::new(size.1, size.0, 0),
    })
}

#[cfg(not(unix))]
pub fn attach(_pid: u32) -> Option<Attach> {
    None
}

/// `cctop-size <cols> <rows>` — the shim's first line to a watcher.
fn parse_size(header: &str) -> Option<(u16, u16)> {
    let mut parts = header.trim().strip_prefix("cctop-size ")?.split(' ');
    let cols = parts.next()?.parse().ok()?;
    let rows = parts.next()?.parse().ok()?;
    (cols > 0 && rows > 0).then_some((cols, rows))
}

/// The bytes a terminal would send for this key.
///
/// Only what an agent reads: text, the editing keys, and the arrows. Anything
/// unmapped is dropped rather than guessed at, since a wrong escape sequence is
/// worse in a live session than a key that did nothing.
fn encode(key: KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let bytes = match key.code {
        // Ctrl-A..Ctrl-Z are the letter with the top three bits cleared, which
        // is also how Ctrl-C reaches the agent as a signal.
        KeyCode::Char(c) if ctrl && c.is_ascii_alphabetic() => {
            vec![c.to_ascii_lowercase() as u8 & 0x1f]
        }
        KeyCode::Char(c) => {
            let mut b = c.to_string().into_bytes();
            if alt {
                b.insert(0, 0x1b);
            }
            b
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        _ => return None,
    };
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_header_is_read_or_refused() {
        assert_eq!(parse_size("cctop-size 120 40\n"), Some((120, 40)));
        assert_eq!(parse_size("cctop-size 0 40\n"), None);
        assert_eq!(parse_size("something else\n"), None);
        assert_eq!(parse_size(""), None);
    }

    /// Every one of these reaches a live agent, so a wrong byte is a wrong
    /// keystroke in someone's session — Ctrl-C especially.
    #[test]
    fn keys_encode_as_a_terminal_would_send_them() {
        let plain = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);

        assert_eq!(encode(plain('a')), Some(b"a".to_vec()));
        assert_eq!(encode(plain('é')), Some("é".as_bytes().to_vec()));
        assert_eq!(encode(ctrl('c')), Some(vec![3]));
        assert_eq!(encode(ctrl('C')), Some(vec![3]));
        // Enter is a carriage return on a pty, not a newline.
        assert_eq!(encode(key(KeyCode::Enter)), Some(b"\r".to_vec()));
        assert_eq!(encode(key(KeyCode::Backspace)), Some(vec![0x7f]));
        assert_eq!(encode(key(KeyCode::Up)), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode(key(KeyCode::F(5))), None);
    }
}
