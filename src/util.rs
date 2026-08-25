//! Formatting and timestamp helpers shared by the CLI and TUI.

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike, Utc};
use std::path::Path;

/// Parse an ISO-8601 timestamp into a UTC instant.
pub fn parse_ts(value: &str) -> Option<DateTime<Utc>> {
    if value.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// Format epoch milliseconds as RFC-3339; empty string if out of range.
pub fn ms_to_rfc3339(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339())
        .unwrap_or_default()
}

/// Local-time day key, `YYYY-MM-DD`.
pub fn local_date_key(dt: &DateTime<Utc>) -> String {
    let l = dt.with_timezone(&Local);
    format!("{:04}-{:02}-{:02}", l.year(), l.month(), l.day())
}

/// Local-time hour key, `YYYY-MM-DDTHH`.
pub fn local_hour_key(dt: &DateTime<Utc>) -> String {
    let l = dt.with_timezone(&Local);
    format!(
        "{:04}-{:02}-{:02}T{:02}",
        l.year(),
        l.month(),
        l.day(),
        l.hour()
    )
}

/// Local midnight today, as a UTC instant.
pub fn local_midnight_today() -> DateTime<Utc> {
    let now = Local::now();
    Local
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .single()
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

/// Number of days in the current local calendar month.
pub fn days_in_current_month() -> u32 {
    let now = Local::now();
    let (y, m) = (now.year(), now.month());
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let first_next = chrono::NaiveDate::from_ymd_opt(ny, nm, 1).unwrap();
    let first_this = chrono::NaiveDate::from_ymd_opt(y, m, 1).unwrap();
    (first_next - first_this).num_days() as u32
}

/// htop-style relative age: `now`, `5m`, `3h`, `2d`, `4w`, `7mo`, `1y`.
pub fn relative_age(value: &str, now: &DateTime<Utc>) -> String {
    let Some(parsed) = parse_ts(value) else {
        return "n/a".into();
    };
    let secs = (now.timestamp() - parsed.timestamp()).max(0);
    match secs {
        s if s < 60 => "now".into(),
        s if s < 3_600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3_600),
        s if s < 604_800 => format!("{}d", s / 86_400),
        s if s < 2_592_000 => format!("{}w", s / 604_800),
        s if s < 31_536_000 => format!("{}mo", s / 2_592_000),
        s => format!("{}y", s / 31_536_000),
    }
}

/// Compact duration for the DUR column: `45s`, `3m12s`, `2h05m`.
pub fn compact_duration(ms: i64) -> String {
    if ms <= 0 {
        return "—".into();
    }
    let s = (ms as f64 / 1000.0).round() as i64;
    if s < 60 {
        format!("{s}s")
    } else if s < 3_600 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{}h{:02}m", s / 3_600, (s % 3_600) / 60)
    }
}

/// Long-form duration used in the Info panel: `1h 04m 09s`.
pub fn long_duration(ms: i64) -> String {
    let s = (ms.max(0) as f64 / 1000.0).round() as i64;
    let (h, m, sec) = (s / 3_600, (s % 3_600) / 60, s % 60);
    if h > 0 {
        format!("{h}h {m}m {sec}s")
    } else if m > 0 {
        format!("{m}m {sec}s")
    } else {
        format!("{sec}s")
    }
}

/// Wall-clock span between a session's first and last activity.
pub fn session_duration(started: &str, last: &str) -> String {
    let (Some(start), Some(end)) = (
        parse_ts(started),
        parse_ts(last).or_else(|| parse_ts(started)),
    ) else {
        return String::new();
    };
    let secs = (end.timestamp() - start.timestamp()).max(0);
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3_600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3_600),
        s if s < 604_800 => format!("{}d", s / 86_400),
        s => format!("{}w", s / 604_800),
    }
}

/// `1.2K`, `3.4M`, `5.6G`.
pub fn compact_tokens(value: u64) -> String {
    match value {
        v if v >= 1_000_000_000 => format!("{:.1}G", v as f64 / 1e9),
        v if v >= 1_000_000 => format!("{:.1}M", v as f64 / 1e6),
        v if v >= 1_000 => format!("{:.1}K", v as f64 / 1e3),
        v => v.to_string(),
    }
}

/// `512B`, `40K`, `1.5M`, `2.1G`. Empty string for zero.
pub fn compact_bytes(value: u64) -> String {
    const K: u64 = 1024;
    match value {
        0 => String::new(),
        v if v >= K * K * K => format!("{:.1}G", v as f64 / (K * K * K) as f64),
        v if v >= K * K => format!("{:.1}M", v as f64 / (K * K) as f64),
        v if v >= K => format!("{:.0}K", v as f64 / K as f64),
        v => format!("{v}B"),
    }
}

/// Two-decimal dollars for table cells.
pub fn compact_usd(value: f64) -> String {
    format!("${:.2}", normalize_usd_zero(value))
}

/// Sub-cent amounts keep four decimals so small spends stay visible.
pub fn adaptive_usd(value: f64) -> String {
    let value = normalize_usd_zero(value);
    if value > 0.0 && value < 0.01 {
        format!("${value:.4}")
    } else {
        format!("${value:.2}")
    }
}

/// Avoid displaying floating-point noise as a negative zero amount.
fn normalize_usd_zero(value: f64) -> f64 {
    if value == 0.0 || (-0.005..0.0).contains(&value) {
        0.0
    } else {
        value
    }
}

/// Six-decimal dollars, matching the JSON cost breakdown fields.
pub fn money(value: f64) -> String {
    format!("{value:.6}")
}

pub fn token_cost(tokens: u64, rate_per_million: f64) -> f64 {
    tokens as f64 * rate_per_million / 1_000_000.0
}

/// `1234567` -> `1,234,567`.
pub fn with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Round up to a "nice" chart maximum: 1, 1.5, 2, 2.5, 3, 4, 5, 7.5, 10 × 10^n.
pub fn nice_max(val: f64) -> f64 {
    if val <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(val.log10().floor());
    for step in [1.0, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 7.5, 10.0] {
        if step * mag >= val {
            return step * mag;
        }
    }
    10.0 * mag
}

/// Truncate to `width` display cells, appending `…` when it doesn't fit.
pub fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    if width <= 1 {
        return s.chars().take(width).collect();
    }
    let mut out: String = s.chars().take(width - 1).collect();
    out.push('…');
    out
}

/// Strip a leading `$HOME` and replace it with `~`.
pub fn tildify(path: &str) -> String {
    let home = crate::config::HOME.to_string_lossy();
    if !home.is_empty() && path.starts_with(home.as_ref()) {
        let rest = &path[home.len()..];
        if rest.is_empty() {
            return "~".into();
        }
        if rest.starts_with('/') {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

/// Drop routing prefixes, version-date suffixes, and the `claude-` vendor prefix.
///
/// Custom providers name models by the whole route they are reached through
/// (`canopywave/zai/glm-5.1`), which is all prefix and no information once it
/// no longer fits the column — the leaf is the part that identifies the model.
///
/// `gpt-` is deliberately kept: dropping it leaves bare version numbers like
/// `5.5`, which say nothing about which model they are, whereas `claude-opus-5`
/// still reads clearly as `opus-5`.
pub fn short_model(model: &str) -> String {
    let m = model
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(model);
    let m = m.strip_prefix("claude-").unwrap_or(m);
    // Trim a trailing `-YYYYMMDD` release stamp. Model names come straight from
    // transcripts and custom providers can put anything in them, so the split
    // has to respect char boundaries: a byte split panics mid-character. A
    // non-boundary here also means the last 9 bytes hold a multi-byte char,
    // which can never be `-` plus eight digits.
    let idx = m.len().wrapping_sub(9);
    if m.len() > 9 && m.is_char_boundary(idx) {
        let (head, tail) = m.split_at(idx);
        if tail.starts_with('-') && tail[1..].chars().all(|c| c.is_ascii_digit()) {
            return head.to_string();
        }
    }
    m.to_string()
}

/// Split a path into non-empty components, tolerating both separators.
fn path_parts(value: &str) -> Vec<&str> {
    value.split(['/', '\\']).filter(|p| !p.is_empty()).collect()
}

/// Shorten each path to its shortest unique trailing segments.
///
/// Every path starts as just its leaf name and grows one component at a time
/// only while it collides with another path, so unambiguous entries stay short.
pub fn abbreviate_paths(values: &[String]) -> Vec<String> {
    let home = crate::config::HOME.to_string_lossy().to_string();
    let home_parts = path_parts(&home);

    let parts_list: Vec<Vec<&str>> = values
        .iter()
        .map(|v| {
            let parts = path_parts(v);
            if parts.len() >= home_parts.len()
                && home_parts.iter().enumerate().all(|(i, hp)| parts[i] == *hp)
            {
                let rest = parts[home_parts.len()..].to_vec();
                if rest.is_empty() { vec!["~"] } else { rest }
            } else {
                parts
            }
        })
        .collect();

    let mut widths: Vec<usize> = parts_list
        .iter()
        .map(|p| usize::from(!p.is_empty()))
        .collect();

    loop {
        let mut groups: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        for (i, parts) in parts_list.iter().enumerate() {
            if parts.is_empty() {
                continue;
            }
            let label = parts[parts.len() - widths[i]..].join("/");
            groups.entry(label).or_default().push(i);
        }
        let mut changed = false;
        for indices in groups.values() {
            if indices.len() < 2 {
                continue;
            }
            for &idx in indices {
                if widths[idx] < parts_list[idx].len() {
                    widths[idx] += 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    parts_list
        .iter()
        .enumerate()
        .map(|(i, parts)| {
            if parts.is_empty() {
                "unknown".to_string()
            } else {
                parts[parts.len() - widths[i]..].join("/")
            }
        })
        .collect()
}

/// Pretty-print an MCP tool name.
///
/// `mcp__Claude_in_Chrome__tabs_context_mcp` -> `Claude in Chrome: tabs context`.
/// Non-MCP names pass through unchanged.
pub fn pretty_mcp_name(name: &str) -> String {
    let Some(without_prefix) = name.strip_prefix("mcp__") else {
        return name.to_string();
    };
    let Some(sep) = without_prefix.find("__") else {
        return without_prefix.replace('_', " ");
    };
    let server = &without_prefix[..sep];
    let tool = &without_prefix[sep + 2..];

    // Anonymous/dynamic servers are named by UUID; showing it helps nobody.
    if crate::config::is_full_uuid(&server.to_ascii_lowercase()) {
        return tool.replace('_', " ").trim().to_string();
    }

    // Unwrap `plugin_<name>_<name>` into just `<name>`.
    let server_stripped = server
        .strip_prefix("plugin_")
        .and_then(|rest| {
            let mid = rest.find('_')?;
            let (a, b) = (&rest[..mid], &rest[mid + 1..]);
            (a.eq_ignore_ascii_case(b)).then(|| a.to_string())
        })
        .unwrap_or_else(|| server.to_string());

    let server_clean = server_stripped
        .strip_suffix("_mcp")
        .unwrap_or(&server_stripped)
        .replace(['-', '_'], " ")
        .trim()
        .to_string();

    let server_word = server_clean
        .split(' ')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let tool_stripped = tool
        .strip_prefix(&format!("{server_word}_"))
        .unwrap_or(tool);
    let tool_clean = tool_stripped
        .strip_suffix("_mcp")
        .unwrap_or(tool_stripped)
        .replace('_', " ")
        .trim()
        .to_string();

    format!("{server_clean}: {tool_clean}")
}

// ---------------------------------------------------------------------------
// Base64
// ---------------------------------------------------------------------------

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding. Used for OSC 52 clipboard writes.
pub fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        let idx = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        out.push(B64_ALPHABET[idx[0] as usize] as char);
        out.push(B64_ALPHABET[idx[1] as usize] as char);
        out.push(if chunk.len() > 1 {
            B64_ALPHABET[idx[2] as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_ALPHABET[idx[3] as usize] as char
        } else {
            '='
        });
    }
    out
}

/// SHA-1 of `data`.
///
/// Here for exactly one reason: a WebSocket handshake answers the client's
/// `Sec-WebSocket-Key` with `base64(sha1(key + GUID))`, and a browser will not
/// open the socket without it. RFC 6455 fixed the algorithm in 2011 and it
/// cannot be substituted — this is not a security claim about SHA-1, it is the
/// protocol's checksum, and the value is public in both directions.
///
/// Hand-rolled rather than a dependency, the same bargain [`b64_encode`] takes
/// two functions up: the algorithm is forty lines and frozen, and the crate
/// that would replace it is a crate to carry, audit and bump forever.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];

    // The padded message: the data, a 1 bit, zeroes, then the bit length.
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());

    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(word.try_into().expect("chunks_exact(4)"));
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, &word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            // Wrapping throughout: SHA-1 is defined modulo 2^32, and a debug
            // build would otherwise panic on the first block that overflows.
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut out = [0u8; 20];
    for (chunk, word) in out.chunks_exact_mut(4).zip(h) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Decode base64, accepting both the standard and URL-safe alphabets and
/// tolerating missing padding (JWT payloads omit it).
pub fn b64_decode(input: &str) -> Option<Vec<u8>> {
    let mut acc: u32 = 0;
    let mut bits: u8 = 0;
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    for c in input.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Read at most `max_bytes` from the start of a file.
pub fn read_head(path: &Path, max_bytes: usize) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; max_bytes];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Read the last `max_bytes` of a file as lossy UTF-8.
pub fn read_tail(path: &Path, max_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    let want = max_bytes.min(size);
    f.seek(SeekFrom::Start(size - want)).ok()?;
    let mut buf = vec![0u8; want as usize];
    f.read_exact(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Tail windows tried in turn by [`scan_tail_escalating`], in bytes.
///
/// A single JSONL entry can carry a whole file read or a screenshot, so it is
/// routinely hundreds of kilobytes; one of those between EOF and the entry a
/// scan is after pushes it out of any fixed window. Growing 4x at a time keeps
/// the wasted re-scan of the windows that already came up empty to a third of
/// the final read, and 4MB is the point past which a transcript that still has
/// no match almost certainly never will — better to give up than to spend the
/// refresh tick proving it.
const TAIL_STEPS: [u64; 4] = [65_536, 262_144, 1_048_576, 4_194_304];

/// Run `scan` over the tail of a file, widening the window until it hits.
///
/// Each call gets a byte window that normally starts mid-line, so `scan` must
/// tolerate a truncated first line; a window that grows completes that line
/// rather than dropping it.
pub fn scan_tail_escalating<T>(path: &Path, mut scan: impl FnMut(&str) -> Option<T>) -> Option<T> {
    let size = std::fs::metadata(path).ok()?.len();
    for &want in &TAIL_STEPS {
        let hit = scan(&read_tail(path, want)?);
        if hit.is_some() {
            return hit;
        }
        if want >= size {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two vectors FIPS 180-1 publishes, plus the worked example from RFC
    /// 6455 §1.3 — which is the one that matters, because it is the exact
    /// handshake a browser will check.
    #[test]
    fn sha1_matches_the_published_vectors() {
        let hex = |b: [u8; 20]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();

        assert_eq!(
            hex(sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        // Empty input exercises the padding path with no data block at all.
        assert_eq!(hex(sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");

        // RFC 6455: key "dGhlIHNhbXBsZSBub25jZQ==" + the GUID must produce
        // "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=".
        let accept = b64_encode(&sha1(
            b"dGhlIHNhbXBsZSBub25jZQ==258EAFA5-E914-47DA-95CA-C5AB0DC85B11",
        ));
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn compact_formats() {
        assert_eq!(compact_tokens(999), "999");
        assert_eq!(compact_tokens(1_500), "1.5K");
        assert_eq!(compact_tokens(2_400_000), "2.4M");
        assert_eq!(compact_bytes(0), "");
        assert_eq!(compact_bytes(2048), "2K");
        assert_eq!(with_commas(1_234_567), "1,234,567");
    }

    #[test]
    fn usd_does_not_display_negative_zero() {
        assert_eq!(compact_usd(-0.0), "$0.00");
        assert_eq!(adaptive_usd(-0.000_001), "$0.00");
        assert_eq!(adaptive_usd(-0.01), "$-0.01");
    }

    #[test]
    fn durations() {
        assert_eq!(compact_duration(45_000), "45s");
        assert_eq!(compact_duration(192_000), "3m12s");
        assert_eq!(compact_duration(7_500_000), "2h05m");
        assert_eq!(compact_duration(0), "—");
    }

    #[test]
    fn epoch_millis_format_and_reject_out_of_range() {
        assert_eq!(
            parse_ts(&ms_to_rfc3339(1_700_000_000_000)).map(|d| d.timestamp_millis()),
            Some(1_700_000_000_000)
        );
        assert_eq!(ms_to_rfc3339(0), "1970-01-01T00:00:00+00:00");
        assert_eq!(ms_to_rfc3339(i64::MAX), "");
    }

    #[test]
    fn model_shortening() {
        assert_eq!(short_model("claude-opus-4-5-20251101"), "opus-4-5");
        assert_eq!(short_model("claude-opus-5"), "opus-5");
        assert_eq!(short_model("gpt-5.3-codex"), "gpt-5.3-codex");
        assert_eq!(short_model("gpt-5.5"), "gpt-5.5");
    }

    /// Custom providers name a model by the whole route to it. The prefixes are
    /// the gateway's, not the model's, and they push the identifying part out of
    /// the column: `canopywave/z` says nothing, `glm-5.1` says everything.
    #[test]
    fn model_shortening_drops_provider_routes() {
        assert_eq!(short_model("canopywave/zai/glm-5.1"), "glm-5.1");
        assert_eq!(short_model("moonshotai/kimi-k2.6"), "kimi-k2.6");
        assert_eq!(
            short_model("anthropic/claude-opus-4-5-20251101"),
            "opus-4-5"
        );
        assert_eq!(short_model("openai/gpt-5.2"), "gpt-5.2");
        // A trailing slash leaves no leaf; the original still beats an empty cell.
        assert_eq!(short_model("weird/"), "weird/");
    }

    /// Regression: a byte split at `len - 9` panicked whenever it landed inside
    /// a multi-byte character, taking the whole TUI down on every refresh.
    #[test]
    fn model_shortening_survives_non_ascii_names() {
        assert_eq!(short_model("gpt-café-preview"), "gpt-café-preview");
        assert_eq!(short_model("modèle-20251101"), "modèle");
        assert_eq!(short_model("日本語モデル"), "日本語モデル");
    }

    #[test]
    fn mcp_names() {
        assert_eq!(
            pretty_mcp_name("mcp__Claude_in_Chrome__tabs_context_mcp"),
            "Claude in Chrome: tabs context"
        );
        assert_eq!(
            pretty_mcp_name("mcp__github__search_code"),
            "github: search code"
        );
        // Server prefix repeated in the tool name is redundant.
        assert_eq!(pretty_mcp_name("mcp__slack__slack_send"), "slack: send");
        assert_eq!(pretty_mcp_name("Bash"), "Bash");
    }

    #[test]
    fn abbreviation_expands_only_on_collision() {
        let paths = vec![
            "/home/flo/work/api".to_string(),
            "/home/flo/personal/api".to_string(),
            "/home/flo/cctop".to_string(),
        ];
        let out = abbreviate_paths(&paths);
        assert_eq!(out[0], "work/api");
        assert_eq!(out[1], "personal/api");
        assert_eq!(out[2], "cctop"); // no collision, stays a leaf
    }

    #[test]
    fn nice_maxima() {
        assert_eq!(nice_max(0.0), 1.0);
        assert_eq!(nice_max(87.0), 100.0);
        assert_eq!(nice_max(230.0), 250.0);
    }
}
