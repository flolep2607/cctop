//! Reading the files agents write their sessions to.
//!
//! The per-format line parsers are public so cctop-core can use them, but they
//! are internals, not the interface this crate offers: hidden from the docs,
//! and outside what `cargo-semver-checks` holds to a version.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::Path;

#[doc(hidden)]
pub mod claude_line;
#[doc(hidden)]
pub mod codex_line;
#[doc(hidden)]
pub mod cursor_line;
#[doc(hidden)]
pub mod devin_atif;
#[doc(hidden)]
pub mod lenient;
#[doc(hidden)]
pub mod opencode_line;

/// Lines above this size are almost always base64 image payloads carrying no
/// token, cost, or tool data. Skipping them keeps large transcripts cheap.
pub const MAX_JSONL_LINE_BYTES: usize = 512 * 1024;

/// Stream a `.jsonl` file's non-blank lines, trimmed, to a reader that parses
/// them into its own type.
///
/// The second argument says the line is over [`MAX_JSONL_LINE_BYTES`]: a
/// reader should take only what it cannot do without from such a line, as
/// cctop-core's `for_each_jsonl` does, rather than all of it.
pub fn for_each_line<F: FnMut(&[u8], bool)>(path: &Path, mut f: F) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut buf = Vec::with_capacity(8 * 1024);

    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        let line = buf.trim_ascii();
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_JSONL_LINE_BYTES {
            f(line, true);
            // Don't let one giant line keep its buffer for the rest of the file.
            buf = Vec::with_capacity(8 * 1024);
            continue;
        }
        f(line, false);
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Tokens {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write_5m: u64,
    #[serde(default)]
    pub cache_write_1h: u64,
    // Codex-only
    #[serde(default)]
    pub input_total: u64,
    #[serde(default)]
    pub cached_input: u64,
    #[serde(default)]
    pub reasoning_output: u64,
    #[serde(default)]
    pub total: u64,
}

impl Tokens {
    /// Everything billed as input, across both providers' shapes.
    pub fn all_input(&self) -> u64 {
        self.input + self.cached_input + self.cache_read + self.cache_write_5m + self.cache_write_1h
    }

    /// A prompt whose cached part is counted inside it, not beside it — the
    /// shape Gemini and Devin both report, as Codex does — split into the
    /// uncached remainder and the cached part, so `all_input` rebuilds the
    /// prompt. A cached count above the prompt is clamped, not underflowed.
    pub fn from_prompt(prompt: u64, cached: u64, output: u64) -> Tokens {
        let cached = cached.min(prompt);
        Tokens {
            input: prompt - cached,
            input_total: prompt,
            cached_input: cached,
            output,
            ..Default::default()
        }
    }

    /// Add `other` into this, every field: one sum shared by every reader, so
    /// a field one provider fills is never summed by one copy and dropped by
    /// another.
    ///
    /// Saturating, for the reason the Claude parser gives at its own sums:
    /// these are numbers read out of a file any process on the machine can
    /// write, and one transcript claiming `u64::MAX` tokens would overflow
    /// every debug build and wrap to a nonsense cost in a release one.
    pub fn add(&mut self, other: &Tokens) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write_5m = self.cache_write_5m.saturating_add(other.cache_write_5m);
        self.cache_write_1h = self.cache_write_1h.saturating_add(other.cache_write_1h);
        self.input_total = self.input_total.saturating_add(other.input_total);
        self.cached_input = self.cached_input.saturating_add(other.cached_input);
        self.reasoning_output = self.reasoning_output.saturating_add(other.reasoning_output);
        self.total = self.total.saturating_add(other.total);
    }
}
