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

/// Why "today" and "this hour" may not be the viewer's, when that is likely.
///
/// Every calendar bucket cctop draws is cut at the process's local midnight,
/// and a login over ssh does not carry the viewer's timezone across — so on a
/// server set to UTC, a run at 11:47 in Auckland lands on yesterday and the
/// Cost panel says nothing was spent today. Only said when all three hold:
/// the session came in over ssh, `TZ` is not set, and the local clock is UTC.
/// Setting `TZ` at all, `TZ=UTC` included, is a decision and silences it.
pub fn unzoned_over_ssh() -> Option<String> {
    let over_ssh = ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        .any(|v| std::env::var_os(v).is_some_and(|s| !s.is_empty()));
    let tz_set = std::env::var_os("TZ").is_some_and(|s| !s.is_empty());
    let utc = Local::now().offset().local_minus_utc() == 0;
    unzoned(over_ssh, tz_set, utc)
}

fn unzoned(over_ssh: bool, tz_set: bool, utc: bool) -> Option<String> {
    (over_ssh && !tz_set && utc).then(|| {
        "Times are in this server's UTC, not yours — set TZ (e.g. export TZ=Pacific/Auckland) \
         so \"today\" and \"this hour\" are your day"
            .to_string()
    })
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

/// The last `parts` components of a path, prefixed with `…/` when that dropped
/// something.
///
/// A footer naming a contested file needs enough of the path to identify it and
/// none of the prefix, which is the same for everything in the repository. Both
/// separators are honoured because a path can reach cctop from either platform.
pub fn path_tail(path: &str, parts: usize) -> String {
    let components: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    if components.len() <= parts {
        return path.to_string();
    }
    format!("…/{}", components[components.len() - parts..].join("/"))
}

/// Truncate to `width` display cells, appending `…` when it doesn't fit.
///
/// Cells, not chars: a CJK project name is two cells a character, and counting
/// chars let it run past its column and off the edge with no ellipsis at all.
pub fn truncate(s: &str, width: usize) -> String {
    if cells(s) <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    let mut utf8 = [0u8; 4];
    for c in s.chars() {
        let w = cells(c.encode_utf8(&mut utf8));
        // One cell stays free for the ellipsis.
        if used + w >= width {
            break;
        }
        used += w;
        out.push(c);
    }
    out.push('…');
    out
}

/// Terminal cells `s` occupies, by the same measure ratatui lays text out with.
pub fn cells(s: &str) -> usize {
    ratatui::text::Span::raw(s).width()
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

/// The inverse of [`tildify`]: turn a leading `~` back into the home directory.
///
/// Only a leading one, and only when it stands for the whole first component.
/// `~/x` is home; `~other/x` is another user's home, which this does not know
/// how to find and must not silently resolve to the wrong one.
pub fn untildify(path: &str) -> String {
    let home = crate::config::HOME.to_string_lossy();
    if home.is_empty() {
        return path.to_string();
    }
    match path {
        "~" => home.into_owned(),
        rest if rest.starts_with("~/") => format!("{home}{}", &rest[1..]),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Shared helpers that used to be copied from module to module
//
// Each of these had grown a copy per caller, and the copies had drifted: some
// wrote through a fixed temporary name, some counted chars where a terminal
// counts cells, two `--since` parsers disagreed about what a bare date meant.
// ---------------------------------------------------------------------------

/// Replace `path` with `bytes`, so a reader sees the old file or the new one
/// and never half of either.
///
/// Written to a temporary beside the target and renamed over it, which is
/// atomic within one filesystem — hence beside, not in `/tmp`. The parent is
/// created first, because every caller wanted that.
///
/// The temporary's name is unique to this write ([`temp_beside`]), never a
/// fixed `.tmp`: several cctops share most of these files, and two writers on
/// one fixed name `File::create` the same inode, so whichever renames first
/// publishes a file the other is still writing into.
///
/// `private` makes the file owner-only from the moment it exists, rather than
/// created wide and narrowed after: a config holding a token is readable by
/// anyone for as long as that window lasts, however short. Without it the file
/// gets the umask's mode, as `fs::write` would give it.
///
/// No fsync. None of the writers this replaced did one, and what they write is
/// a cache, a state file or a config whose loss on power failure costs a
/// re-read or a re-run, not work — while an fsync on every hook fire is a
/// stall on a slow disk inside someone's coding session.
///
/// A failed write removes its temporary, so a full disk does not also fill up
/// with half-written siblings.
pub fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = temp_beside(path);
    let written = (|| {
        let mut options = std::fs::OpenOptions::new();
        // `create` rather than `create_new`: the name is already unique to
        // this process and write, and a reused pid must not fail to save
        // because a crashed run left its temporary behind.
        options.write(true).create(true).truncate(true);
        if private {
            options.mode(0o600);
        }
        options.open(&tmp)?.write_all(bytes)?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// A temporary name beside `path` that no other writer can already be holding.
///
/// The pid separates processes, which is the direction that matters since each
/// cctop is its own writer; the counter separates threads inside one, which a
/// pid alone would call the same writer.
fn temp_beside(path: &Path) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let seq = WRITES.fetch_add(1, Ordering::Relaxed);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.{seq}.tmp", std::process::id()));
    path.with_file_name(name)
}

/// An exclusive lock on the file at `path`, created if it is not there, or
/// `WouldBlock` once `patience` has gone by with someone else holding it.
///
/// Polled rather than blocked on: the holder may be a cctop that is itself
/// wedged — stopped with ^Z mid-write, say — and every caller would rather go
/// without than wait on that. A zero `patience` is one try. The lock goes with
/// the returned file, or with the process, since the kernel releases it on the
/// last close of the descriptor: which is what makes it safe in a hook that
/// may be abandoned mid-write.
///
/// Owner-only, because the lock files live beside state that is.
pub fn lock_within(path: &Path, patience: std::time::Duration) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path)?;
    let started = std::time::Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
            Err(std::fs::TryLockError::WouldBlock) => {}
        }
        if started.elapsed() >= patience {
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Seconds since the Unix epoch; zero on a clock set before it.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `1 session` / `2 sessions`, because a line that says "1 sessions" reads as
/// one nobody proof-read.
pub fn plural(n: usize, noun: &str) -> String {
    match n {
        1 => format!("1 {noun}"),
        _ => format!("{n} {noun}s"),
    }
}

/// Cells one character occupies, by the same measure as [`cells`].
fn char_cells(c: char) -> usize {
    cells(c.encode_utf8(&mut [0u8; 4]))
}

/// At most `max` cells of `text`, then `…` when something was cut.
///
/// The counterpart of [`truncate`], whose ellipsis fits *inside* the width:
/// this one goes past it, for the places that cap how much is kept rather than
/// how wide a column is — a tool's detail, a quoted prompt, a title. Cells
/// rather than chars for the same reason as there, and a kept part that ends
/// in a space has it trimmed, so the `…` reads as the end of the last word.
///
/// The ellipsis is load-bearing where an agent reads the text as fact: a
/// half-quoted instruction that ends cleanly reads as the whole instruction.
pub fn clip(text: &str, max: usize) -> String {
    if cells(text) <= max {
        return text.to_string();
    }
    let mut used = 0;
    let end = text
        .char_indices()
        .find(|&(_, c)| {
            used += char_cells(c);
            used > max
        })
        .map_or(text.len(), |(i, _)| i);
    format!("{}…", text[..end].trim_end())
}

/// `text` in rows of at most `width` cells, each line of it wrapped on its
/// own, broken on the last space that fits — and inside a word only where it
/// is wider than a whole row.
///
/// Cells, not chars: a CJK line counted in chars came out twice the width it
/// was given and ran over whatever was beside it. Spaces inside a row are kept
/// as typed, and the ones a break lands on are dropped; an empty line stays an
/// empty row.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for raw in text.lines() {
        let mut line = raw;
        while cells(line) > width {
            // The byte where the row is full, and the last space at or before
            // it — a space just past the edge is as good a break as one inside
            // it, since the row is then exactly full.
            let mut used = 0;
            let mut full = line.len();
            let mut space = None;
            for (i, c) in line.char_indices() {
                if c == ' ' {
                    space = Some(i);
                }
                used += char_cells(c);
                if used > width {
                    full = i;
                    break;
                }
            }
            let cut = match space {
                // Nothing to break on, or only a leading space, which would
                // emit an empty row and never make progress: cut the word.
                // A wide character alone on a row narrower than itself still
                // has to go somewhere, or this would never end.
                Some(0) | None => match full {
                    0 => line.chars().next().map_or(line.len(), char::len_utf8),
                    at => at,
                },
                Some(at) => at,
            };
            out.push(line[..cut].to_string());
            line = line[cut..].trim_start();
            if line.is_empty() {
                // Cut exactly at the end, or only spaces were left: an empty
                // row here would be one nobody wrote.
                break;
            }
        }
        if !line.is_empty() || raw.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

/// A span typed by a person, `90s`, `30m`, `2h`, `7d`, `2w` — or `1.5h`.
///
/// `None` for anything else, including a span too long to be a [`Duration`]:
/// `99999999999999w` is a typo to refuse, not a reason to panic, which is what
/// `chrono::Duration::weeks` did with it. A bare number is not a span here;
/// what it would mean depends on the flag, so a caller that has a default unit
/// adds it.
///
/// [`Duration`]: std::time::Duration
pub fn parse_duration(text: &str) -> Option<std::time::Duration> {
    let text = text.trim();
    let split = text.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let (number, unit) = text.split_at(split);
    let scale = match unit {
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3_600.0,
        "d" => 86_400.0,
        "w" => 7.0 * 86_400.0,
        _ => return None,
    };
    let value: f64 = number.parse().ok()?;
    std::time::Duration::try_from_secs_f64(value * scale).ok()
}

/// `--since`, as every command that takes one reads it: a span back from
/// `now` ([`parse_duration`]), an RFC 3339 instant, or a date.
///
/// A bare date is *local* midnight, because that is what a person typing
/// `--since 2026-09-01` means: the start of their 1st of September, not
/// whatever hour of it UTC's midnight falls on. It used to be UTC midnight in
/// `optimize` and local midnight in `yolo log`, so the same flag picked a
/// different set of sessions depending on the command it was given to.
///
/// A span of zero is refused: "since now" selects nothing, and is a slip.
pub fn parse_since(text: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let text = text.trim();
    if let Some(span) = parse_duration(text) {
        if span.is_zero() {
            return None;
        }
        return now.checked_sub_signed(chrono::TimeDelta::from_std(span).ok()?);
    }
    if let Ok(at) = DateTime::parse_from_rfc3339(text) {
        return Some(at.with_timezone(&Utc));
    }
    let day = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?;
    // `earliest`: a midnight a DST change repeats happens twice, and the day
    // starts at the first. One it skips does not happen at all, and the day
    // starts at the hour the clocks jumped to.
    let at = |hour| {
        day.and_hms_opt(hour, 0, 0)?
            .and_local_timezone(Local)
            .earliest()
    };
    Some(at(0).or_else(|| at(1))?.with_timezone(&Utc))
}

#[cfg(test)]
mod shared_tests {
    use super::*;

    /// A write lands whole, leaves no temporary behind, and a private one is
    /// owner-only — checked on the file, since the mode is set at creation.
    #[test]
    fn an_atomic_write_lands_whole_and_private_when_asked() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deeper/config.toml");
        write_atomic(&path, b"token = \"x\"\n", true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"token = \"x\"\n");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "mode was {mode:o}");
        // Replaced, not appended to.
        write_atomic(&path, b"y", true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"y");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["config.toml"], "a temporary was left behind");
    }

    /// Two temporaries for one target never share a name, which is the whole
    /// difference from the fixed `.tmp` the copies used to write through.
    #[test]
    fn temporaries_beside_one_target_are_all_different() {
        let path = Path::new("/x/state.json");
        let (a, b) = (temp_beside(path), temp_beside(path));
        assert_ne!(a, b);
        assert_eq!(a.parent(), path.parent());
        assert!(a.to_string_lossy().starts_with("/x/state.json."));
    }

    /// A failed write says so and leaves nothing where it tried.
    #[test]
    fn a_failed_atomic_write_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("file");
        std::fs::write(&blocker, "").unwrap();
        assert!(write_atomic(&blocker.join("x.json"), b"{}", false).is_err());
    }

    /// The lock keeps a second taker out until the first lets go.
    #[test]
    fn a_held_lock_turns_the_next_taker_away() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.lock");
        let held = lock_within(&path, std::time::Duration::ZERO).unwrap();
        let refused = lock_within(&path, std::time::Duration::from_millis(5)).unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::WouldBlock);
        drop(held);
        assert!(lock_within(&path, std::time::Duration::ZERO).is_ok());
    }

    #[test]
    fn plural_counts_one_and_the_rest() {
        assert_eq!(plural(1, "session"), "1 session");
        assert_eq!(plural(0, "session"), "0 sessions");
        assert_eq!(plural(2, "tab"), "2 tabs");
    }

    /// A CJK string is two cells a character: counted in chars it came out
    /// twice as wide as the room it was given.
    #[test]
    fn clipping_and_wrapping_count_cells_not_chars() {
        // Within the cap, untouched; past it, the ellipsis goes on top.
        assert_eq!(clip("hello", 5), "hello");
        assert_eq!(clip("hello world", 5), "hello…");
        assert_eq!(clip("hello world", 6), "hello…", "the space is trimmed");
        assert_eq!(clip("日本語のテキスト", 6), "日本語…");
        assert_eq!(
            cells(&clip("日本語のテキスト", 7)),
            7,
            "no half a character"
        );

        for row in wrap("日本語のテキストを折り返す", 6) {
            assert!(cells(&row) <= 6, "{row:?} is {} cells", cells(&row));
        }
        assert_eq!(wrap("日本語 テキスト", 7), ["日本語", "テキス", "ト"]);
        assert_eq!(wrap("the quick brown fox", 10), ["the quick", "brown fox"]);
        // A word wider than the row is cut, an empty line stays a row, and a
        // wide character in a one-cell row still makes progress.
        assert_eq!(wrap("abcdefgh", 3), ["abc", "def", "gh"]);
        assert_eq!(wrap("a\n\nb", 5), ["a", "", "b"]);
        assert_eq!(wrap("日本", 1), ["日", "本"]);
    }

    #[test]
    fn durations_read_every_unit_and_refuse_overflow() {
        use std::time::Duration;
        assert_eq!(parse_duration("90s"), Some(Duration::from_secs(90)));
        assert_eq!(parse_duration("30m"), Some(Duration::from_secs(1_800)));
        assert_eq!(parse_duration("2h"), Some(Duration::from_secs(7_200)));
        assert_eq!(parse_duration("7d"), Some(Duration::from_secs(7 * 86_400)));
        assert_eq!(parse_duration("2w"), Some(Duration::from_secs(14 * 86_400)));
        assert_eq!(parse_duration("1.5m"), Some(Duration::from_secs(90)));
        for bad in [
            "",
            "7",
            "d",
            "7y",
            "-3d",
            "soon",
            "1e9s",
            "99999999999999999999w",
        ] {
            assert_eq!(parse_duration(bad), None, "{bad}");
        }
    }

    /// `--since 99999999999999w` panicked in chrono's `Duration::weeks`; it is
    /// a span before the start of time, and refused like any other nonsense.
    #[test]
    fn since_takes_a_span_an_instant_or_a_local_date_and_never_panics() {
        let now = DateTime::parse_from_rfc3339("2026-10-08T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let ago = |w| (now - parse_since(w, now).unwrap()).num_seconds();
        assert_eq!(ago("90s"), 90);
        assert_eq!(ago("7d"), 7 * 86_400);
        assert_eq!(ago("1w"), 7 * 86_400);
        assert_eq!(
            parse_since("2026-10-01T00:00:00Z", now)
                .unwrap()
                .to_rfc3339(),
            "2026-10-01T00:00:00+00:00"
        );
        let local = parse_since("2026-09-01", now)
            .unwrap()
            .with_timezone(&Local);
        assert_eq!(
            (local.date_naive().to_string(), local.hour(), local.minute()),
            ("2026-09-01".to_string(), 0, 0),
            "a date is the person's own midnight"
        );
        for bad in ["", "7", "0d", "-3d", "5y", "yesterday", "99999999999999w"] {
            assert!(parse_since(bad, now).is_none(), "{bad}");
        }
    }

    /// A home of `/home/flo` must not fold `/home/flo2` into `~2`: only a
    /// whole leading component is the home directory.
    #[test]
    fn tildify_folds_only_the_whole_home_component() {
        let home = crate::config::HOME.to_string_lossy().into_owned();
        assert_eq!(tildify(&format!("{home}/x")), "~/x");
        assert_eq!(tildify(&format!("{home}2/x")), format!("{home}2/x"));
    }
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

/// `want` bytes of entropy, from the OS where it will give us any.
///
/// `/dev/urandom` is the real source and the fallback is not as good: hashing
/// the clock and the pid under [`std::hash::RandomState`], whose keys the
/// standard library seeds from the OS once per process. That is enough to stop
/// a value being guessed from across a network and is not enough to stop a
/// local process that already knows when cctop started.
///
/// A short read counts as a failure rather than as fewer bytes: half of
/// something random is not half as good, and silently shortening it is the kind
/// of weakening nobody would find later.
pub fn random_bytes(want: usize) -> Vec<u8> {
    read_random(want).unwrap_or_else(|| hashed_bytes(want))
}

/// `bytes` bytes of entropy as hex, which is twice as many characters.
pub fn random_hex(bytes: usize) -> String {
    random_bytes(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The fallback for a machine whose `/dev/urandom` could not be read, split out
/// so it can be tested without breaking the real source.
fn hashed_bytes(want: usize) -> Vec<u8> {
    use std::hash::{BuildHasher, Hasher, RandomState};

    let mut out = Vec::with_capacity(want);
    for salt in 0.. {
        if out.len() >= want {
            break;
        }
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_i64(now_ms());
        hasher.write_u32(std::process::id());
        hasher.write_u64(salt);
        out.extend_from_slice(&hasher.finish().to_ne_bytes());
    }
    out.truncate(want);
    out
}

/// Read `want` bytes from the system random device, if there is one.
fn read_random(want: usize) -> Option<Vec<u8>> {
    use std::io::Read;

    let mut file = std::fs::File::open("/dev/urandom").ok()?;
    let mut buf = vec![0u8; want];
    file.read_exact(&mut buf).ok()?;
    Some(buf)
}

#[cfg(test)]
mod tests {

    /// Only an ssh login with no timezone of its own on a UTC clock is told;
    /// a TZ set on purpose, a local terminal, or a zoned server is not.
    #[test]
    fn only_an_unzoned_ssh_login_on_utc_is_warned() {
        assert!(super::unzoned(true, false, true).is_some_and(|m| m.contains("TZ")));
        assert!(super::unzoned(true, true, true).is_none());
        assert!(super::unzoned(false, false, true).is_none());
        assert!(super::unzoned(true, false, false).is_none());
    }

    /// Round-trips, tolerates the line wrapping a pasted blob carries, and
    /// refuses prose rather than decoding it into rubbish — which is what makes
    /// it safe to sniff every paste for an image.
    #[test]
    fn base64_decodes_what_it_encodes_and_refuses_prose() {
        for case in [
            &b""[..],
            b"a",
            b"ab",
            b"abc",
            b"\x89PNG\r\n\x1a\n",
            b"\x00\xff\x10",
        ] {
            let text = b64_encode(case);
            assert_eq!(b64_decode(&text).as_deref(), Some(case), "{text}");
        }
        // Wrapped, as `base64` writes it at 76 columns and as a paste of one
        // arrives.
        let text = b64_encode(b"hello world!");
        let wrapped = format!("{}\r\n{}", &text[..8], &text[8..]);
        assert_eq!(b64_decode(&wrapped).as_deref(), Some(&b"hello world!"[..]));
        assert_eq!(b64_decode("what is wrong with this?"), None);
    }
    use super::*;

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
    fn tildify_and_back_are_inverses() {
        let home = crate::config::HOME.to_string_lossy().into_owned();
        assert_eq!(untildify(&tildify(&home)), home);
        let inside = format!("{home}/cctop/src");
        assert_eq!(untildify(&tildify(&inside)), inside);
        // An absolute path elsewhere is left alone by both.
        assert_eq!(untildify("/etc/hosts"), "/etc/hosts");
        assert_eq!(tildify("/etc/hosts"), "/etc/hosts");
        // Another user's home is not this one's, and guessing would resolve a
        // launch into the wrong directory entirely.
        assert_eq!(untildify("~other/x"), "~other/x");
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

    /// The fallback has to be the same shape as the source it stands in for.
    #[test]
    fn the_hashed_fallback_is_the_same_shape_as_the_real_thing() {
        assert_eq!(hashed_bytes(16).len(), 16);
        assert_eq!(hashed_bytes(3).len(), 3);
        assert_ne!(hashed_bytes(16), hashed_bytes(16));
        let hex = random_hex(16);
        assert_eq!(hex.len(), 32);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(random_hex(16), random_hex(16));
    }
}
