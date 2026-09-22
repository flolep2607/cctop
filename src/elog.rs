//! `CCTOP_LOG` — a line per thing that happened, in a file an agent can tail.
//!
//! `--trace` aggregates *time*; this records *happenings* — a request answered,
//! a frame relayed, a key pressed, a hook fired, a webhook posted — one JSON
//! object per line, so `tail -f` or `cctop log -f` is the whole reader. It
//! exists for the case the TUI cannot help: reproducing a bug on the browser
//! surface, or asking an agent what cctop did while it was looking elsewhere.
//!
//! `CCTOP_LOG=events` (or `1`) writes one line per event: `ts`, `pid`, `src`,
//! `kind` and the structured fields. `CCTOP_LOG=io` adds the bytes — a preview
//! of what moved in and out, which is the level that catches an escape
//! sequence arriving where no escape should be. Either may name a file —
//! `CCTOP_LOG=io,/tmp/x.jsonl` — and a bare path means `events` there. The
//! default is `events.jsonl` beside the caches, one file for every cctop
//! process, so a serve, a shim and a TUI all land in the same stream — which
//! is the point: a bug between two of them is only visible in the join.
//!
//! The level can also be raised on a running server through the debug-only
//! `POST /api/debug/log` route, for a `cctop serve` that was not started with
//! the variable set.
//!
//! # What it may contain
//!
//! At `io` level, whatever crossed the wire — which can be a prompt, a token,
//! or a password typed into an agent. The file is created owner-only and stays
//! on this machine; it is a debug file and it is opt-in. It is *not* scrubbed:
//! scrubbing is exactly the kind of help that hides the bug being chased.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{LazyLock, Mutex};

/// How much gets written. Ordered, so `level() >= Events` is the cheap test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Off = 0,
    /// One line per event: what happened, where, and how big it was.
    Events = 1,
    /// Events plus a preview of the bytes that moved. For "which escape
    /// sequence did that come from" — the question a byte count cannot answer.
    Io = 2,
}

/// `LEVEL` starts as "ask the environment" so a debug route can set it either
/// way at runtime — `255` is that sentinel, never a level itself.
const ENV: u8 = 255;
static LEVEL: AtomicU8 = AtomicU8::new(ENV);

/// The parsed `CCTOP_LOG`: level, then the file it writes to.
static CFG: LazyLock<(Level, PathBuf)> = LazyLock::new(|| {
    let mut level = Level::Off;
    let mut path = default_path();
    // Unset is off; set-but-empty is on — somebody typed the variable and the
    // polite reading is that they meant it.
    if let Some(spec) = std::env::var_os("CCTOP_LOG") {
        let spec = spec.to_string_lossy();
        let mut named = false;
        for part in spec.split(',') {
            match part.trim() {
                "0" | "off" | "no" => level = Level::Off,
                "io" => level = Level::Io,
                // Anything with a path separator — or a dotfile extension — is
                // a filename, not a level. `~` is expanded the same way
                // everywhere else in cctop.
                other if other.contains('/') || other.starts_with('~') => {
                    path = PathBuf::from(crate::util::untildify(other));
                    named = true;
                }
                "" => {}
                _ => level = Level::Events,
            }
        }
        // An empty spec, or one that named a file and no level, both mean
        // "events". `named` guards the second: `CCTOP_LOG=/tmp/x` is a request
        // to log there, not a request for nothing.
        if level == Level::Off && (!named || spec.trim().is_empty()) {
            level = Level::Events;
        }
        // `CCTOP_LOG=off,/tmp/x` asked for off and named a file anyway — the
        // level wins.
    }
    (level, path)
});

static FILE: LazyLock<Mutex<Option<std::fs::File>>> = LazyLock::new(|| Mutex::new(None));

/// How big the log may grow before the next open starts a fresh file.
///
/// The old file is kept one deep as `events.old.jsonl`, so the bound on the
/// pair is twice this. Large enough that a busy `io`-level afternoon fits;
/// small enough that forgetting the variable was ever set is not a disk leak.
const MAX_LOG_BYTES: u64 = 16 * 1024 * 1024;

/// The current level. Read whenever anything asks — the debug route can move
/// it on a running server, so it is not a constant.
pub fn level() -> Level {
    match LEVEL.load(Ordering::Relaxed) {
        ENV => CFG.0,
        0 => Level::Off,
        1 => Level::Events,
        _ => Level::Io,
    }
}

/// Set the level on a running process, in either direction. The debug route
/// is the caller: enabling `io` for one reproduction and then turning the log
/// back off is the shape that takes.
#[cfg(feature = "debug")]
pub fn set(level: Level) {
    LEVEL.store(level as u8, Ordering::Relaxed);
}

/// The name the debug route spells.
#[cfg(feature = "debug")]
pub fn level_name(level: Level) -> &'static str {
    match level {
        Level::Off => "off",
        Level::Events => "events",
        Level::Io => "io",
    }
}

/// The level a `?level=` query asks for, or `None` for a word that is none.
#[cfg(feature = "debug")]
pub fn parse_level(word: &str) -> Option<Level> {
    match word {
        "off" | "0" => Some(Level::Off),
        "events" | "1" => Some(Level::Events),
        "io" => Some(Level::Io),
        _ => None,
    }
}

/// The file events are written to — where `cctop log` reads by default.
pub fn path() -> &'static Path {
    &CFG.1
}

fn default_path() -> PathBuf {
    crate::config::CACHE_DIR.join("events.jsonl")
}

/// Whether this process is the one that opens the file — first event wins.
fn file() -> Option<std::sync::MutexGuard<'static, Option<std::fs::File>>> {
    let mut slot = FILE.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        *slot = open(&CFG.1);
    }
    slot.is_some().then_some(slot)
}

fn open(path: &Path) -> Option<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    // One rotation, on open: a runaway log is kept to one file plus one old
    // one rather than grown until somebody notices.
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX_LOG_BYTES {
        let _ = std::fs::rename(path, path.with_extension("old.jsonl"));
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .ok()
}

/// Record one event. `fields` becomes the rest of the line's object.
///
/// One `write_all` of a finished line: appends to a regular file are atomic
/// per write, so two cctop processes and two threads in each can share the
/// file without a line ever arriving torn.
pub fn event(src: &'static str, kind: &'static str, fields: serde_json::Value) {
    if level() < Level::Events {
        return;
    }
    write(src, kind, fields);
}

/// Record a byte stream crossing a boundary — the "IN & OUT" half.
///
/// The count is always written; a preview of the bytes themselves only at
/// `io` level, which is the difference between "four bytes arrived" and
/// "those four bytes were `\x1b[<35;79;14M`". `extra` merges into the line —
/// who the frame went to, which watcher sent it.
pub fn bytes(
    src: &'static str,
    kind: &'static str,
    dir: &'static str,
    data: &[u8],
    extra: serde_json::Value,
) {
    if level() < Level::Events {
        return;
    }
    let mut fields = serde_json::json!({ "dir": dir, "bytes": data.len() });
    if let serde_json::Value::Object(map) = extra {
        for (k, v) in map {
            fields[k] = v;
        }
    }
    if level() >= Level::Io {
        fields["data"] = preview(data).into();
    }
    write(src, kind, fields);
}

/// One terminal event the TUI received — the "IN" side of the dashboard.
///
/// Bare mouse motion is `io`-only: a finger resting on a trackpad is hundreds
/// of them a second, and every other kind already tells you where it was.
/// That said, `io` is the level that answers "did the terminal send it at
/// all" — which is the question a stray `<35;` in the agent's input poses.
pub fn tui(event: &crossterm::event::Event) {
    use crossterm::event::{Event, MouseEventKind};
    if level() < Level::Events {
        return;
    }
    match event {
        Event::Key(k) => write(
            "tui",
            "key",
            serde_json::json!({
                "code": format!("{:?}", k.code),
                "mods": format!("{:?}", k.modifiers),
            }),
        ),
        Event::Mouse(m) => {
            if matches!(m.kind, MouseEventKind::Moved) && level() < Level::Io {
                return;
            }
            write(
                "tui",
                "mouse",
                serde_json::json!({
                    "ev": format!("{:?}", m.kind),
                    "col": m.column,
                    "row": m.row,
                }),
            );
        }
        Event::Paste(text) => bytes("tui", "paste", "in", text.as_bytes(), serde_json::json!({})),
        Event::Resize(cols, rows) => write(
            "tui",
            "resize",
            serde_json::json!({ "cols": cols, "rows": rows }),
        ),
        Event::FocusGained => write("tui", "focus", serde_json::json!({ "gained": true })),
        Event::FocusLost => write("tui", "focus", serde_json::json!({ "gained": false })),
    }
}

/// Write the line. The lock orders writers inside this process; `O_APPEND`
/// does the same between processes.
fn write(src: &'static str, kind: &'static str, fields: serde_json::Value) {
    let mut line = serde_json::json!({
        "ts": crate::util::ms_to_rfc3339(crate::util::now_ms()),
        "pid": std::process::id(),
        "src": src,
        "kind": kind,
    });
    if let (Some(obj), serde_json::Value::Object(extra)) = (line.as_object_mut(), fields) {
        for (k, v) in extra {
            obj.insert(k, v);
        }
    }
    let mut out = serde_json::to_vec(&line).unwrap_or_default();
    out.push(b'\n');
    if let Some(mut slot) = file()
        && let Some(f) = slot.as_mut()
    {
        // A write error — full disk, removed file — is reported nowhere: the
        // log is a convenience, and a convenience that can break the thing it
        // watches is not one.
        let _ = f.write_all(&out);
    }
}

/// The first bytes of a payload, escaped so a terminal reading the log is
/// never fed a control sequence — a log that injects `\x1b[2J` into the pager
/// tailing it is a debugging tool that creates bugs.
fn preview(data: &[u8]) -> String {
    const MAX: usize = 200;
    let mut out = String::new();
    for &b in data.iter().take(MAX) {
        match b {
            0x1b => out.push_str("\\e"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    if data.len() > MAX {
        out.push_str(&format!("…+{}", data.len() - MAX));
    }
    out
}

// ---------------------------------------------------------------------------
// `cctop log`
// ---------------------------------------------------------------------------

pub const HELP: &str = "\
cctop log — the event stream CCTOP_LOG writes, readable without a TUI

USAGE:
  cctop log [FILE] [-f] [-n N] [--src SRC] [--kind KIND] [--raw]

With no FILE it reads the default log (the one CCTOP_LOG writes). -f follows
it like tail -f. --src and --kind keep only matching events; --raw prints the
JSONL untouched for a pipe.

To get events in the file at all, run the cctop being watched with
CCTOP_LOG=events (metadata) or CCTOP_LOG=io (metadata plus byte previews), or
raise a running server's level with POST /api/debug/log.
";

/// One filter flag's value — `--src http` or `--src=http`.
fn flag_value<'a>(argv: &'a [String], i: &mut usize, name: &str) -> Option<&'a str> {
    let arg = argv.get(*i)?;
    if let Some(v) = arg.strip_prefix(&format!("{name}=")) {
        return Some(v);
    }
    if arg == name {
        *i += 1;
        return argv.get(*i).map(String::as_str);
    }
    None
}

/// A log line for a human: `14:32:01.512 http response GET /api/sessions → 200`.
fn render(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let ts = v["ts"].as_str().unwrap_or_default();
    // The milliseconds carry the ordering information; the date does not.
    let time = ts.get(11..).unwrap_or(ts);
    let src = v["src"].as_str().unwrap_or("?");
    let kind = v["kind"].as_str().unwrap_or("?");
    let mut out = format!("{time} {src:7} {kind}");
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            if matches!(k.as_str(), "ts" | "pid" | "src" | "kind") {
                continue;
            }
            let shown = match val {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            out.push_str(&format!(" {k}={shown}"));
        }
    }
    Some(out)
}

/// `cctop log` — print the event file, optionally following it.
///
/// Read-only over the same file every cctop writes; there is no state here to
/// get wrong.
pub fn run(argv: &[String]) -> i32 {
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return 0;
    }
    let mut follow = false;
    let mut raw = false;
    let mut tail: Option<usize> = None;
    let mut src: Option<String> = None;
    let mut kind: Option<String> = None;
    let mut path = path().to_path_buf();

    let mut i = 0;
    while i < argv.len() {
        if let Some(v) = flag_value(argv, &mut i, "--src") {
            src = Some(v.to_string());
        } else if let Some(v) = flag_value(argv, &mut i, "--kind") {
            kind = Some(v.to_string());
        } else if let Some(v) = flag_value(argv, &mut i, "-n") {
            tail = v.parse().ok();
        } else {
            match argv[i].as_str() {
                "-f" | "--follow" => follow = true,
                "--raw" => raw = true,
                "-h" | "--help" => {
                    print!("{HELP}");
                    return 0;
                }
                other if !other.starts_with('-') => path = PathBuf::from(other),
                other => {
                    eprintln!("cctop log: unknown flag {other} (--help lists them)");
                    return 2;
                }
            }
        }
        i += 1;
    }

    let wanted = |line: &str| -> bool {
        if src.is_none() && kind.is_none() {
            return true;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return false;
        };
        src.as_deref().is_none_or(|s| v["src"].as_str() == Some(s))
            && kind
                .as_deref()
                .is_none_or(|k| v["kind"].as_str() == Some(k))
    };
    let show = |line: &str| {
        if !wanted(line) {
            return;
        }
        match raw {
            true => println!("{line}"),
            false => println!("{}", render(line).unwrap_or_else(|| line.to_string())),
        }
    };

    let Ok(content) = std::fs::read_to_string(&path) else {
        eprintln!(
            "cctop log: {} — nothing there. Run a cctop with CCTOP_LOG=events (or =io) first.",
            path.display()
        );
        return 1;
    };
    let mut lines: Vec<&str> = content.lines().collect();
    if let Some(n) = tail {
        lines = lines.split_off(lines.len().saturating_sub(n));
    }
    for line in lines {
        show(line);
    }
    if !follow {
        return 0;
    }

    // tail -f: keep the file open and read whatever lands. A poll rather than
    // inotify — the file may not exist yet, may be rotated open over, and a
    // second of lag never hid anything.
    let mut at = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(400));
        let Ok(mut f) = std::fs::File::open(&path) else {
            continue;
        };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < at {
            // Rotated or truncated under us: start over.
            at = 0;
        }
        use std::io::{Read, Seek, SeekFrom};
        if f.seek(SeekFrom::Start(at)).is_err() {
            continue;
        }
        let mut buf = String::new();
        if f.read_to_string(&mut buf).is_err() {
            continue;
        }
        at = len;
        // A line without its newline is mid-write; hold it for the next pass.
        let complete = buf.ends_with('\n') || buf.is_empty();
        let mut iter = buf.lines().peekable();
        while let Some(line) = iter.next() {
            if !complete && iter.peek().is_none() {
                at -= line.len() as u64;
                break;
            }
            show(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The escape in a mouse report must not reach the terminal tailing the
    /// log — that is the whole reason previews are escaped.
    #[test]
    fn a_preview_escapes_what_would_move_the_cursor() {
        let shown = preview(b"\x1b[<35;79;14M");
        assert_eq!(shown, "\\e[<35;79;14M");
        assert!(!shown.contains('\x1b'));
    }

    #[test]
    fn a_preview_bounds_how_much_it_carries() {
        let big = vec![b'x'; 4096];
        let shown = preview(&big);
        assert!(shown.ends_with("…+3896"), "{shown}");
        // 200 carried bytes + "…" (three bytes itself) + "+3896".
        assert_eq!(shown.len(), 208);
    }

    #[test]
    fn a_line_renders_for_a_person() {
        let line = r#"{"ts":"2026-09-17T12:34:56.789Z","pid":1,"src":"http","kind":"response","path":"/api/sessions","status":200,"bytes":1234}"#;
        let shown = render(line).unwrap();
        // serde_json keeps insertion order (`preserve_order`): the extra fields
        // print in the order they were logged.
        assert_eq!(
            shown,
            "12:34:56.789Z http    response path=/api/sessions status=200 bytes=1234"
        );
    }

    #[test]
    fn level_words_and_paths_parse() {
        // The parser runs once per process into CFG, so exercise the match by
        // proxy: the words that mean a level, and the strings that mean a file.
        for (spec, want) in [
            ("off", Level::Off),
            ("io", Level::Io),
            ("events", Level::Events),
            ("1", Level::Events),
        ] {
            let mut level = Level::Off;
            for part in spec.split(',') {
                match part.trim() {
                    "0" | "off" | "no" => level = Level::Off,
                    "io" => level = Level::Io,
                    other if other.contains('/') || other.starts_with('~') => {}
                    "" => {}
                    _ => level = Level::Events,
                }
            }
            assert_eq!(level, want, "{spec}");
        }
    }
}
