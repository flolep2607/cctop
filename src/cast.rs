//! Recording a pane to an asciinema v2 `.cast` file.
//!
//! The format is a JSON header line and then one JSON array per event:
//! `[seconds, "o", text]` for output and `[seconds, "r", "COLSxROWS"]` for a
//! resize. It is fed the pane's raw pty bytes, tapped in
//! [`Attach::pump`](crate::attach::Attach::pump) before vt100 sees them, rather
//! than anything re-rendered from the parsed screen: a player is itself a
//! terminal emulator, so the agent's own escape sequences replayed at their own
//! times reproduce the pane exactly, where a diff of screens would be cctop's
//! reading of them.
//!
//! Where the files go is [`dir`]; why there is on it.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Where recordings are written: `$XDG_DATA_HOME/cctop/casts`, which is
/// `~/.local/share/cctop/casts` by default.
///
/// The data directory, because a recording is something a user made and means
/// to keep. The cache directory is what `--clear-cache` empties, and a cast lost
/// to a cache sweep is lost for good. The session's working directory was the
/// other candidate and is the wrong one: it is usually a repository, so the file
/// would turn up in `git status` and in the very agent's view of the project it
/// recorded — and a pane onto a shell or someone else's session has no cwd
/// cctop knows at all.
pub fn dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| crate::config::HOME.join(".local").join("share"))
        .join("cctop")
        .join("casts")
}

/// A cast file for `label`, created and empty: `<label>-<stamp>.cast` in
/// [`dir`], numbered when two recordings start in the same second.
///
/// Created here rather than merely named, `create_new`, so two panes recording
/// at once can never be handed the same path and interleave into one file.
pub fn create(label: &str) -> io::Result<(PathBuf, std::fs::File)> {
    let dir = dir();
    std::fs::create_dir_all(&dir)?;
    let stem = file_stem(label);
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    for n in 1..=99u32 {
        let name = match n {
            1 => format!("{stem}-{stamp}.cast"),
            n => format!("{stem}-{stamp}-{n}.cast"),
        };
        let path = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "too many recordings started in the same second",
    ))
}

/// The tab's label as something safe in a file name. A label is whatever the
/// user renamed the tab to, so it can hold a `/` — which would be a directory —
/// or spaces, which make the path the toast hands out awkward to paste.
fn file_stem(label: &str) -> String {
    let stem: String = label
        .chars()
        .map(|c| match c.is_alphanumeric() || c == '-' || c == '_' {
            true => c,
            false => '-',
        })
        .collect();
    let stem = stem.trim_matches('-');
    match stem.is_empty() {
        true => "pane".to_string(),
        false => stem.chars().take(40).collect(),
    }
}

/// A recording in progress, writing to `out`.
///
/// Generic over the writer so the tests can read back what was written without
/// a file; the pane's one is a buffered file.
pub struct Cast<W: Write> {
    out: W,
    /// Time zero for every event's timestamp.
    started: Instant,
    /// The end of the last read, when it stopped partway through a UTF-8
    /// sequence.
    ///
    /// A cast's output is a JSON *string*, so it has to be valid UTF-8, and a
    /// pty read is cut wherever the kernel's buffer was — straight through a
    /// `│` or an emoji as often as not. Lossily converting each read on its own
    /// would turn every such cut into two replacement characters on replay;
    /// held back here, the sequence is completed by the next read and written
    /// whole.
    tail: Vec<u8>,
    /// The first write that failed, after which nothing more is attempted: a
    /// full disk mid-recording must not become an error on every frame of a
    /// pane that is otherwise fine. Reported when the recording is stopped.
    failed: Option<io::Error>,
}

impl<W: Write> Cast<W> {
    /// Start a recording `cols`×`rows` wide, writing the header now.
    ///
    /// `title` is what a player shows; the tab's label is the obvious one.
    pub fn start(mut out: W, cols: u16, rows: u16, title: &str, now: Instant) -> io::Result<Self> {
        let mut header = serde_json::json!({
            "version": 2,
            "width": cols,
            "height": rows,
            "timestamp": chrono::Utc::now().timestamp(),
            "title": title,
        });
        // What the agent was told it is drawing for, which is cctop's own
        // `TERM`: the shim passes its environment down unchanged.
        if let Ok(term) = std::env::var("TERM") {
            header["env"] = serde_json::json!({ "TERM": term });
        }
        writeln!(out, "{header}")?;
        Ok(Cast {
            out,
            started: now,
            tail: Vec::new(),
            failed: None,
        })
    }

    /// Record what the pty said at `now`.
    pub fn output(&mut self, bytes: &[u8], now: Instant) {
        let text = decode(&mut self.tail, bytes);
        if !text.is_empty() {
            self.event(now, "o", &text);
        }
    }

    /// Record that the pane is now `cols`×`rows`.
    pub fn resize(&mut self, cols: u16, rows: u16, now: Instant) {
        self.event(now, "r", &format!("{cols}x{rows}"));
    }

    /// Push what has been written so far to the file.
    ///
    /// Called once per batch of output rather than per event, so a recording
    /// can be played while it is still being made, and a cctop that is killed
    /// loses at most the last frame, without a syscall per pty read.
    pub fn flush(&mut self) {
        if self.failed.is_none()
            && let Err(e) = self.out.flush()
        {
            self.failed = Some(e);
        }
    }

    /// End the recording: write out whatever half-sequence was still held
    /// back, since no read is coming to complete it, and flush.
    ///
    /// Idempotent, so [`Drop`] can call it after an explicit stop has.
    pub fn finish(&mut self) -> io::Result<()> {
        if !self.tail.is_empty() {
            let text = String::from_utf8_lossy(&std::mem::take(&mut self.tail)).into_owned();
            let now = Instant::now();
            self.event(now, "o", &text);
        }
        self.flush();
        match self.failed.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn event(&mut self, now: Instant, kind: &str, data: &str) {
        if self.failed.is_some() {
            return;
        }
        // Microseconds, which is what asciinema itself writes: finer is noise
        // in a file that grows by one of these per pty read.
        let at = now.saturating_duration_since(self.started).as_micros() as f64 / 1e6;
        let line = serde_json::json!([at, kind, data]);
        if let Err(e) = writeln!(self.out, "{line}") {
            self.failed = Some(e);
        }
    }
}

impl<W: Write> Drop for Cast<W> {
    /// A pane can go without anyone stopping its recording — its agent exits,
    /// its tab is restarted underneath it, cctop quits — and the file must
    /// still end on a complete line rather than on whatever the buffer held.
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// `bytes`, after whatever `tail` held from the last read, as valid UTF-8 —
/// leaving in `tail` an incomplete sequence at the very end for the next read
/// to finish.
///
/// Only a sequence cut off *at the end* is held. Bytes that are invalid in the
/// middle can never become valid, so they are replaced at once; holding them
/// would stall everything after them.
fn decode(tail: &mut Vec<u8>, bytes: &[u8]) -> String {
    tail.extend_from_slice(bytes);
    let mut text = String::new();
    let mut rest: &[u8] = tail;
    loop {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                text.push_str(valid);
                rest = &[];
                break;
            }
            Err(e) => {
                let (valid, bad) = rest.split_at(e.valid_up_to());
                text.push_str(&String::from_utf8_lossy(valid));
                match e.error_len() {
                    Some(n) => {
                        text.push(char::REPLACEMENT_CHARACTER);
                        rest = &bad[n..];
                    }
                    None => {
                        rest = bad;
                        break;
                    }
                }
            }
        }
    }
    let held = rest.to_vec();
    *tail = held;
    text
}

/// A pane's recording: the cast and the file it is going to.
pub struct Recording {
    pub path: PathBuf,
    cast: Cast<io::BufWriter<std::fs::File>>,
}

impl Recording {
    /// Start recording a pane labelled `label` that is `cols`×`rows`, beginning
    /// with `screen` — what the pane shows right now.
    ///
    /// The screen is written as the first event because a recording starts
    /// partway through a session: without it the cast would open on a blank
    /// terminal and fill in only as the agent happened to repaint, which for
    /// one sitting at its prompt is never.
    pub fn start(label: &str, cols: u16, rows: u16, screen: &[u8]) -> io::Result<Recording> {
        let (path, file) = create(label)?;
        let now = Instant::now();
        let mut cast = Cast::start(io::BufWriter::new(file), cols, rows, label, now)?;
        cast.output(screen, now);
        cast.flush();
        Ok(Recording { path, cast })
    }

    pub fn output(&mut self, bytes: &[u8]) {
        self.cast.output(bytes, Instant::now());
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cast.resize(cols, rows, Instant::now());
    }

    pub fn flush(&mut self) {
        self.cast.flush();
    }

    /// Stop, handing back where the file is and whether all of it made it.
    pub fn stop(mut self) -> (PathBuf, io::Result<()>) {
        let finished = self.cast.finish();
        (self.path, finished)
    }
}

/// What to tell the user about a recording that has just ended.
pub fn stopped_message(path: &Path, finished: &io::Result<()>) -> String {
    // Home as `~`, so the whole path fits in a toast and still pastes into a
    // shell as it is.
    let shown = match path.strip_prefix(&*crate::config::HOME) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    };
    match finished {
        Ok(()) => format!("Recording saved to {shown}"),
        Err(e) => format!("Could not finish the recording at {shown}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn lines(out: &[u8]) -> Vec<serde_json::Value> {
        std::str::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn the_header_says_the_version_and_the_panes_size() {
        let mut out = Vec::new();
        {
            let _cast = Cast::start(&mut out, 120, 40, "claude", Instant::now()).unwrap();
        }
        let header = &lines(&out)[0];
        assert_eq!(header["version"], 2);
        assert_eq!(header["width"], 120);
        assert_eq!(header["height"], 40);
        assert_eq!(header["title"], "claude");
        assert!(header["timestamp"].as_i64().unwrap() > 0);
    }

    #[test]
    fn events_are_timed_from_the_start_and_resizes_are_their_own_kind() {
        let t0 = Instant::now();
        let mut out = Vec::new();
        {
            let mut cast = Cast::start(&mut out, 80, 24, "sh", t0).unwrap();
            cast.output(b"hello", t0);
            cast.resize(100, 30, t0 + Duration::from_millis(1500));
            cast.output(b"\x1b[2Jworld", t0 + Duration::from_micros(2_250_001));
        }
        let events = &lines(&out)[1..];
        assert_eq!(events[0], serde_json::json!([0.0, "o", "hello"]));
        assert_eq!(events[1], serde_json::json!([1.5, "r", "100x30"]));
        assert_eq!(
            events[2],
            serde_json::json!([2.250001, "o", "\u{1b}[2Jworld"])
        );
    }

    #[test]
    fn a_character_cut_between_two_reads_is_written_whole() {
        let t0 = Instant::now();
        let mut out = Vec::new();
        let box_drawing = "─│".as_bytes();
        {
            let mut cast = Cast::start(&mut out, 80, 24, "sh", t0).unwrap();
            // `─` is three bytes; the read ends after the first of them.
            cast.output(&box_drawing[..1], t0);
            cast.output(&box_drawing[1..4], t0);
            cast.output(&box_drawing[4..], t0);
        }
        let events = &lines(&out)[1..];
        // The lone lead byte wrote nothing; the next read completed it.
        assert_eq!(events.len(), 2);
        assert_eq!(events[0][2], "─");
        assert_eq!(events[1][2], "│");
    }

    #[test]
    fn a_four_byte_character_can_be_cut_anywhere() {
        let crab = "a🦀b".as_bytes();
        for cut in 0..crab.len() {
            let mut tail = Vec::new();
            let mut text = decode(&mut tail, &crab[..cut]);
            text.push_str(&decode(&mut tail, &crab[cut..]));
            assert_eq!(text, "a🦀b", "cut at {cut}");
            assert!(tail.is_empty());
        }
    }

    #[test]
    fn bytes_that_are_not_utf8_are_replaced_without_holding_up_the_rest() {
        let mut tail = Vec::new();
        assert_eq!(decode(&mut tail, b"a\xffb\xe2\x94"), "a\u{fffd}b");
        // The cut-off sequence at the end is held, not replaced.
        assert_eq!(tail, b"\xe2\x94");
        assert_eq!(decode(&mut tail, b"\x80"), "─");
    }

    #[test]
    fn a_recording_stopped_mid_character_still_ends_on_a_whole_line() {
        let t0 = Instant::now();
        let mut out = Vec::new();
        {
            let mut cast = Cast::start(&mut out, 80, 24, "sh", t0).unwrap();
            cast.output(b"ok\xe2", t0);
            cast.finish().unwrap();
        }
        let events = &lines(&out)[1..];
        assert_eq!(events[0][2], "ok");
        assert_eq!(events[1][2], "\u{fffd}");
    }

    #[test]
    fn a_label_becomes_a_file_name() {
        assert_eq!(file_stem("claude"), "claude");
        assert_eq!(file_stem("api / web +1"), "api---web--1");
        assert_eq!(file_stem("///"), "pane");
    }
}
