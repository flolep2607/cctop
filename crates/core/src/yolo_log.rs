//! The lasting record of what YOLO allowed: `yolo-log.jsonl`, and `cctop yolo
//! log` to read it.
//!
//! [`crate::yolo`] keeps a short list per session in the runtime directory for
//! the page to show, and drops it with the permission — when the session ends,
//! when YOLO goes off, at reboot. That answers "what is it doing", not "what
//! did it run while I wasn't looking", which is asked afterwards. So every
//! allow, from either answer path, and every switch on and off, is also
//! appended here: one JSON object per line, under the data directory
//! (`~/.local/share/cctop`), which is neither a cache nor the runtime
//! directory and survives all of those.
//!
//! # How it is written
//!
//! One `write` of one finished line to a file opened `O_APPEND`, with no lock.
//! Appends to a regular file are atomic per write, so the hook, the owner
//! pressing keys and a switch from the page can share it without a torn line,
//! and none of them ever waits on another — the hook least of all, which writes
//! after its verdict is already sent and on the agent's clock. A write that
//! fails (a read-only home, a full disk) is dropped and changes nothing else.
//!
//! The file is created owner-only (0600), and the directory it is in loses any
//! permission for others. It is bounded the way [`crate::elog`] is: past
//! [`MAX_BYTES`] it is renamed once to `yolo-log.old.jsonl`, so the record
//! is at most two files.
//!
//! # What it never holds
//!
//! A tool input as it arrived. `detail` — the command, path, URL or pattern —
//! goes through [`clean`], which runs [`redact`] over it, strips control
//! characters and caps it. **Redaction is best effort**: it knows the common
//! ways a secret appears on a command line, and a secret in any other shape
//! goes through. Treat the file as sensitive, which is why only its owner can
//! read it.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const FILE: &str = "yolo-log.jsonl";
pub const OLD: &str = "yolo-log.old.jsonl";

/// Past this the file is rotated, once. A line is a few hundred bytes, so this
/// is tens of thousands of allows — weeks of a busy YOLO session — in each of
/// the two files.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// The longest `detail` kept, in characters. A whole command, heredoc and all,
/// in the common case; bounded because a tool input can be the size of a file.
pub const MAX_DETAIL: usize = 1000;

/// What a line records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Event {
    /// A permission prompt was allowed (or, with `ok: false`, a key press to
    /// allow one failed).
    #[default]
    Allow,
    /// YOLO was switched on.
    On,
    /// YOLO was switched off.
    Off,
    /// The switch was dropped because its session ended or its process went.
    Ended,
}

/// Which way an allow got in: see [`crate::yolo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    /// `cctop yolo-hook` answered Claude Code's `PermissionRequest`.
    Hook,
    /// cctop pressed the key a person clicking Allow presses.
    Key,
}

/// Where a switch was thrown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Tui,
    Web,
}

/// The tool a permission prompt was for, and what it was asked to touch, as the
/// observer hook saw it in the payload. Already [`clean`]ed where it is made,
/// so it is safe to carry over the hook socket and to write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

/// One line of the file. Every field but `at`, `event` and `session` is
/// optional, and absent rather than null when unknown — `tool` above all,
/// which is never guessed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// RFC 3339, UTC.
    pub at: String,
    pub event: Event,
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// `claude` or `codex`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<Via>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// What was allowed, through [`clean`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
    /// Only on a key press that failed, as `false`; absent means it went in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    /// Why a press failed, or why a switch ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Origin>,
    /// The agent process the switch was for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

impl Line {
    /// An allow, with what was allowed: the call when the payload had one,
    /// else the display line the row showed, with no tool.
    pub fn allow(session: &str, via: Via, call: Option<Call>, shown: Option<&str>) -> Line {
        let (tool, detail, truncated) = match call {
            Some(call) => {
                let (detail, cut) = match call.detail {
                    Some(d) => {
                        let (d, cut) = clean(&d);
                        (Some(d), cut)
                    }
                    None => (None, false),
                };
                (Some(clean(&call.tool).0), detail, call.truncated || cut)
            }
            None => match shown {
                Some(shown) => {
                    let (d, cut) = clean(shown);
                    (None, Some(d), cut)
                }
                None => (None, None, false),
            },
        };
        Line {
            at: now(),
            event: Event::Allow,
            session: session.to_string(),
            via: Some(via),
            tool,
            detail,
            truncated,
            ..Line::default()
        }
    }
}

/// Where the file lives.
#[derive(Debug, Clone)]
pub struct Log {
    dir: PathBuf,
}

impl Default for Log {
    /// `~/.local/share/cctop` — or, under test, a directory of the test
    /// process's own, as [`crate::config::CACHE_DIR`] is: a test that switches
    /// YOLO on must never add lines to the developer's real record.
    fn default() -> Log {
        #[cfg(any(test, feature = "test-support"))]
        if crate::under_test() {
            return Log::at(
                std::env::temp_dir()
                    .join(format!("cctop-data-{}", std::process::id()))
                    .join("cctop"),
            );
        }
        Log::at(crate::config::data_base().join("cctop"))
    }
}

impl Log {
    pub fn at(dir: impl Into<PathBuf>) -> Log {
        Log { dir: dir.into() }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE)
    }

    pub fn old_path(&self) -> PathBuf {
        self.dir.join(OLD)
    }

    /// Append one line. Errors are the caller's to drop: nothing that writes
    /// here may behave differently because the write failed.
    pub fn append(&self, line: &Line) -> std::io::Result<()> {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        // An existing directory — the casts live beside this — keeps its
        // owner and group bits, and loses whatever it gave everyone else.
        if let Ok(meta) = std::fs::metadata(&self.dir) {
            let mode = meta.permissions().mode();
            if mode & 0o007 != 0 {
                let _ = std::fs::set_permissions(
                    &self.dir,
                    std::fs::Permissions::from_mode(mode & !0o007),
                );
            }
        }
        let path = self.path();
        // ponytail: two writers that both see the file over the cap both
        // rename, and the second can move a just-started file over the old
        // one, losing it. The window is one stat wide, once per four MB.
        if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
            let _ = std::fs::rename(&path, self.old_path());
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&path)?;
        let mut bytes = serde_json::to_vec(line).map_err(std::io::Error::other)?;
        bytes.push(b'\n');
        file.write_all(&bytes)
    }

    /// Every line in both files, oldest first. A line that does not parse is
    /// skipped rather than fatal: the file is append-only text, and a reader
    /// should show what it can.
    pub fn read(&self) -> Vec<Line> {
        [self.old_path(), self.path()]
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .flat_map(|text| {
                text.lines()
                    .filter_map(|l| serde_json::from_str::<Line>(l).ok())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

/// Write `line` to the default log, dropping any error. See [`Log::append`].
pub fn record(line: &Line) {
    let _ = Log::default().append(line);
}

/// Names that say a value is a secret, as a regex alternation. Matched
/// case-insensitively, anywhere in a variable's, flag's or header's name.
const SECRET_NAME: &str = "token|secret|password|passwd|pass|key|auth|credential|cookie";

/// The words a secret's name contains, for deciding whether the name rules
/// are worth compiling. `pass` covers `password` and `passwd`.
const SECRET_WORDS: &[&str] = &[
    "token",
    "secret",
    "pass",
    "key",
    "auth",
    "credential",
    "cookie",
];

/// One redaction: a pattern, what replaces its match, and the lowercase
/// substrings without one of which it cannot match.
///
/// The triggers are there for the hook's clock. Compiling every rule costs
/// about 13 ms in a release build — on each `PermissionRequest` the observer
/// reports and each allow `yolo-hook` records, inside a 250 ms deadline — and
/// almost no command has a secret in it. So a rule is compiled the first time
/// its trigger turns up in a process, and a plain `cargo test` compiles none.
struct Rule {
    triggers: &'static [&'static str],
    pattern: String,
    re: std::sync::OnceLock<regex::Regex>,
    with: &'static str,
}

impl Rule {
    fn new(
        triggers: &'static [&'static str],
        pattern: impl Into<String>,
        with: &'static str,
    ) -> Rule {
        Rule {
            triggers,
            pattern: pattern.into(),
            re: std::sync::OnceLock::new(),
            with,
        }
    }

    fn apply(&self, text: String, lower: &str) -> String {
        if !self.triggers.iter().any(|t| lower.contains(t)) {
            return text;
        }
        let re = self
            .re
            .get_or_init(|| regex::Regex::new(&self.pattern).expect("a redaction rule compiles"));
        match re.is_match(&text) {
            true => re.replace_all(&text, self.with).into_owned(),
            false => text,
        }
    }
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let value = r#"(?:"[^"]*"|'[^']*'|[^\s'"]+)"#;
    vec![
        // `scheme://user:password@host`: the whole userinfo.
        Rule::new(
            &["://"],
            r"(?i)([a-z][a-z0-9+.-]*://)[^/\s:@]+:[^/\s@]+@",
            "${1}[redacted]@",
        ),
        // A header whose name says secret — `Authorization: …`, `X-Api-Key:
        // …`, `Cookie: …` — keeping the scheme word of an auth header. A value
        // already `[redacted]` is matched as one, or the scheme word would be
        // taken for the value on a second pass.
        Rule::new(
            SECRET_WORDS,
            format!(
                r#"(?i)\b([a-z0-9-]*(?:{SECRET_NAME})[a-z0-9-]*\s*:\s*)((?:bearer|basic|digest|token)\s+)?(?:\[redacted\]|[^\s'"\[]+)"#
            ),
            "${1}${2}[redacted]",
        ),
        // A bearer token anywhere else.
        Rule::new(
            &["bearer"],
            r#"(?i)\b(bearer\s+)[^\s'"\[]+"#,
            "${1}[redacted]",
        ),
        // `NAME=value`, for a name that says secret.
        Rule::new(
            SECRET_WORDS,
            format!(r"(?i)\b([a-z0-9_]*(?:{SECRET_NAME})[a-z0-9_]*)={value}"),
            "${1}=[redacted]",
        ),
        // `--flag=value` and `--flag value`. The spaced form never takes the
        // next flag as the value.
        Rule::new(
            SECRET_WORDS,
            format!(r"(?i)(--?[a-z0-9][a-z0-9_-]*(?:{SECRET_NAME})[a-z0-9_-]*)={value}"),
            "${1}=[redacted]",
        ),
        Rule::new(
            SECRET_WORDS,
            format!(
                r#"(?i)(--?[a-z0-9][a-z0-9_-]*(?:{SECRET_NAME})[a-z0-9_-]*\s+)(?:"[^"]*"|'[^']*'|[^\s'"-][^\s'"]*)"#
            ),
            "${1}[redacted]",
        ),
        // Shapes tokens are issued in.
        Rule::new(&["sk-"], r"\bsk-[A-Za-z0-9_-]{8,}", "[redacted]"),
        Rule::new(
            &["ghp_", "gho_", "ghu_", "ghs_", "ghr_"],
            r"\bgh[pousr]_[A-Za-z0-9]{16,}",
            "[redacted]",
        ),
        Rule::new(
            &["github_pat_"],
            r"\bgithub_pat_[A-Za-z0-9_]{16,}",
            "[redacted]",
        ),
        Rule::new(&["xox"], r"\bxox[abposr]-[A-Za-z0-9-]{8,}", "[redacted]"),
        Rule::new(&["akia"], r"\bAKIA[0-9A-Z]{16}", "[redacted]"),
        Rule::new(&["glpat-"], r"\bglpat-[A-Za-z0-9_-]{16,}", "[redacted]"),
    ]
});

/// The shortest run of base64 or hex characters taken for a secret.
const LONG_RUN: usize = 32;

/// Whether `c` belongs in a base64 or hex run. `/` is left out on purpose:
/// with it every long path is a run, and a path is the commonest detail there
/// is. A base64 secret with slashes in it is still caught a segment at a time
/// when its segments are long, which is the best this can do without a
/// tokenizer.
fn in_run(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '_' | '=' | '-')
}

/// Every run of [`LONG_RUN`] or more [`in_run`] characters with both a letter
/// and a digit in it, replaced. By hand rather than by regex, since it is the
/// one rule with no trigger and so would be compiled on every call.
fn redact_runs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        let mixed =
            run.chars().any(|c| c.is_ascii_digit()) && run.chars().any(|c| c.is_ascii_alphabetic());
        match run.chars().count() >= LONG_RUN && mixed {
            true => out.push_str("[redacted]"),
            false => out.push_str(run),
        }
        run.clear();
    };
    for c in text.chars() {
        match in_run(c) {
            true => run.push(c),
            false => {
                flush(&mut run, &mut out);
                out.push(c);
            }
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Replace whatever looks like a secret in `text` with `[redacted]`.
///
/// Best effort, and the doc comment of this module says so to whoever reads
/// the file: a value under a variable, flag or header whose name says it is a
/// secret (`token`, `secret`, `password`, `passwd`, `pass`, `key`, `auth`,
/// `credential`, `cookie`, in any case), an `Authorization:` or `Bearer` value,
/// the `user:password@` of a URL, the shapes common tokens are issued in, and
/// any run of 32 or more base64 or hex characters with both letters and
/// digits in it.
///
/// Names are matched, not words in paths: `src/monkey.rs` is left alone,
/// because nothing is assigned to it. But a *name* containing `key` is a
/// secret's name to this, so `--monkey=1` loses its value: over-redacting a
/// harmless flag costs a line of context, under-redacting costs a secret.
pub fn redact(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let out = RULES
        .iter()
        .fold(text.to_string(), |out, rule| rule.apply(out, &lower));
    redact_runs(&out)
}

/// What may be written as a `detail`: [`redact`]ed, with control characters
/// other than newline and tab dropped — a newline is part of a multi-line
/// command, and JSON escapes it — and capped at [`MAX_DETAIL`] characters.
/// Returns whether it was cut.
///
/// Redaction comes before the cap, so a secret straddling the cap is caught
/// whole rather than cut into a piece no rule recognises.
pub fn clean(text: &str) -> (String, bool) {
    let redacted = redact(text);
    let kept: String = redacted
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    match kept.chars().count() > MAX_DETAIL {
        true => (kept.chars().take(MAX_DETAIL).collect(), true),
        false => (kept, false),
    }
}

/// The call a permission prompt's payload names: its tool, and the whole of
/// the one input field that says the most about it — the same fields, in the
/// same order, as the one-line [`crate::hook`] summary, without cutting at the
/// first line. `None` when the payload names no tool.
pub fn call_of(body: &serde_json::Value) -> Option<Call> {
    let tool = body
        .get("tool_name")
        .and_then(|v| v.as_str())
        .filter(|t| !t.is_empty())?;
    let input = body.get("tool_input");
    let detail = ["command", "file_path", "path", "url", "pattern", "query"]
        .iter()
        .find_map(|key| input?.get(key)?.as_str().filter(|v| !v.is_empty()))
        .map(clean);
    Some(Call {
        tool: clean(tool).0,
        truncated: detail.as_ref().is_some_and(|(_, cut)| *cut),
        detail: detail.map(|(d, _)| d),
    })
}

// ---------------------------------------------------------------------------
// cctop yolo log
// ---------------------------------------------------------------------------

const HELP: &str = "\
cctop yolo — what YOLO mode allowed

Usage:
  cctop yolo log [--session <id>] [--since <when>] [-n <count>] [--json] [-f]

Every prompt YOLO allowed, by the hook or by a key press, and every time it was
switched on or off, is appended to ~/.local/share/cctop/yolo-log.jsonl (rotated
once to yolo-log.old.jsonl past 4 MB). `log` prints both files, oldest first.

  --session <id>   only sessions whose id starts with this
  --since <when>   only entries since a duration ago (30m, 2h, 7d, 1w) or a
                   date (2026-10-01, or a full RFC 3339 time)
  -n <count>       only the last <count> entries
  --json           the raw JSON lines, for a pipe
  -f, --follow     keep printing entries as they are written

Commands and paths are redacted on a best-effort basis before they are written:
treat the file as sensitive.
";

/// `cctop yolo …`. Only `log` so far; anything else, or nothing, is the help.
pub fn run(argv: &[String]) -> i32 {
    match argv.first().map(String::as_str) {
        Some("log") => run_log(&Log::default(), &argv[1..], &mut std::io::stdout()),
        Some("-h" | "--help" | "help") | None => {
            print!("{HELP}");
            0
        }
        Some(other) => {
            eprintln!("cctop yolo: no command {other:?} (try `cctop yolo log`)");
            2
        }
    }
}

struct Filter {
    session: Option<String>,
    since: Option<chrono::DateTime<chrono::Utc>>,
    tail: Option<usize>,
    json: bool,
    follow: bool,
}

impl Filter {
    fn keeps(&self, line: &Line) -> bool {
        self.session
            .as_deref()
            .is_none_or(|s| line.session.starts_with(s))
            && self.since.is_none_or(|since| {
                chrono::DateTime::parse_from_rfc3339(&line.at)
                    .is_ok_and(|at| at.with_timezone(&chrono::Utc) >= since)
            })
    }
}

fn parse_args(argv: &[String]) -> Result<Filter, String> {
    let mut filter = Filter {
        session: None,
        since: None,
        tail: None,
        json: false,
        follow: false,
    };
    let mut args = argv.iter();
    while let Some(arg) = args.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = || {
            inline
                .clone()
                .or_else(|| args.next().cloned())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match name {
            "--session" => filter.session = Some(value()?),
            "--since" => {
                let v = value()?;
                filter.since = Some(since(&v, chrono::Utc::now()).ok_or_else(|| {
                    format!("--since {v:?}: not a duration (30m, 2h, 7d) or a date")
                })?);
            }
            "-n" => {
                let v = value()?;
                filter.tail = Some(v.parse().map_err(|_| format!("-n {v:?}: not a count"))?);
            }
            "--json" => filter.json = true,
            "-f" | "--follow" => filter.follow = true,
            other => return Err(format!("unknown argument {other} (--help lists them)")),
        }
    }
    Ok(filter)
}

/// A `--since`: a duration back from `now`, or a date or time.
fn since(word: &str, now: chrono::DateTime<chrono::Utc>) -> Option<chrono::DateTime<chrono::Utc>> {
    let split = word
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(word.len());
    let (count, unit) = word.split_at(split);
    if let Ok(count) = count.parse::<i64>() {
        let secs = match unit {
            "s" => Some(1),
            "m" => Some(60),
            "h" => Some(3600),
            "d" => Some(86_400),
            "w" => Some(7 * 86_400),
            _ => None,
        };
        if let Some(secs) = secs {
            return now.checked_sub_signed(chrono::Duration::seconds(count.checked_mul(secs)?));
        }
    }
    if let Ok(at) = chrono::DateTime::parse_from_rfc3339(word) {
        return Some(at.with_timezone(&chrono::Utc));
    }
    let day = chrono::NaiveDate::parse_from_str(word, "%Y-%m-%d").ok()?;
    let local = day.and_hms_opt(0, 0, 0)?.and_local_timezone(chrono::Local);
    Some(local.earliest()?.with_timezone(&chrono::Utc))
}

/// One entry for a person: local time, the session's first eight characters,
/// what happened, and what it was about.
fn render(line: &Line) -> String {
    let at = chrono::DateTime::parse_from_rfc3339(&line.at)
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| line.at.clone());
    let session: String = line.session.chars().take(8).collect();
    let place = line
        .cwd
        .as_deref()
        .and_then(|c| Path::new(c).file_name())
        .map(|n| format!(" {}", n.to_string_lossy()))
        .unwrap_or_default();
    let what = match line.event {
        Event::Allow => {
            let via = match line.via {
                Some(Via::Hook) => "hook",
                Some(Via::Key) => "key",
                None => "?",
            };
            let failed = match line.ok {
                Some(false) => " FAILED",
                _ => "",
            };
            let tool = line
                .tool
                .as_deref()
                .map(|t| format!("{t}: "))
                .unwrap_or_default();
            // On one line: a newline in a heredoc is shown, not followed.
            let detail = line.detail.as_deref().unwrap_or("").replace('\n', " ⏎ ");
            let cut = if line.truncated { " …" } else { "" };
            let why = line
                .why
                .as_deref()
                .map(|w| format!(" ({w})"))
                .unwrap_or_default();
            format!("allow/{via}{failed} {tool}{detail}{cut}{why}")
        }
        Event::On | Event::Off => {
            let word = if line.event == Event::On { "on" } else { "off" };
            let from = match line.from {
                Some(Origin::Tui) => " from the TUI",
                Some(Origin::Web) => " from the web",
                None => "",
            };
            let pid = line.pid.map(|p| format!(" pid {p}")).unwrap_or_default();
            format!("YOLO {word}{from}{pid}")
        }
        Event::Ended => format!(
            "YOLO ended: {}",
            line.why.as_deref().unwrap_or("the session ended")
        ),
    };
    let harness = line
        .harness
        .as_deref()
        .map(|h| format!(" [{h}]"))
        .unwrap_or_default();
    format!("{at} {session}{place}{harness} {what}")
}

fn show(line: &Line, filter: &Filter, out: &mut dyn Write) -> std::io::Result<()> {
    match filter.json {
        true => writeln!(
            out,
            "{}",
            serde_json::to_string(line).map_err(std::io::Error::other)?
        ),
        false => writeln!(out, "{}", render(line)),
    }
}

fn run_log(log: &Log, argv: &[String], out: &mut dyn Write) -> i32 {
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        let _ = write!(out, "{HELP}");
        return 0;
    }
    let filter = match parse_args(argv) {
        Ok(f) => f,
        Err(why) => {
            eprintln!("cctop yolo log: {why}");
            return 2;
        }
    };
    let mut lines: Vec<Line> = log.read().into_iter().filter(|l| filter.keeps(l)).collect();
    if let Some(n) = filter.tail {
        lines = lines.split_off(lines.len().saturating_sub(n));
    }
    if lines.is_empty() && !filter.follow && !log.path().exists() && !log.old_path().exists() {
        eprintln!(
            "cctop yolo log: {} — nothing yet. YOLO has not allowed anything on this machine.",
            log.path().display()
        );
        return 0;
    }
    for line in &lines {
        if show(line, &filter, out).is_err() {
            return 0;
        }
    }
    if !filter.follow {
        return 0;
    }
    follow(log, &filter, out)
}

/// `-f`: print whatever lands, as `cctop log -f` does — a poll, because the
/// file may not exist yet and may be rotated under the reader.
fn follow(log: &Log, filter: &Filter, out: &mut dyn Write) -> i32 {
    use std::io::{Read, Seek, SeekFrom};
    let path = log.path();
    let mut at = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(400));
        let Ok(mut f) = std::fs::File::open(&path) else {
            continue;
        };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < at {
            at = 0;
        }
        if f.seek(SeekFrom::Start(at)).is_err() {
            continue;
        }
        let mut buf = String::new();
        if f.read_to_string(&mut buf).is_err() {
            continue;
        }
        // Only up to the last newline: anything after it is mid-write.
        let complete = buf.rfind('\n').map_or(0, |i| i + 1);
        at += complete as u64;
        for text in buf[..complete].lines() {
            if let Ok(line) = serde_json::from_str::<Line>(text)
                && filter.keeps(&line)
                && show(&line, filter, out).is_err()
            {
                return 0;
            }
        }
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> Log {
        let dir = std::env::temp_dir().join(format!(
            "cctop-yolo-log-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Log::at(dir.join("cctop"))
    }

    /// Every rule, on made-up values only.
    #[test]
    fn secrets_are_redacted() {
        for (raw, want) in [
            ("TOKEN=dummy make deploy", "TOKEN=[redacted] make deploy"),
            (
                "export GITHUB_TOKEN='dummy value' && gh pr list",
                "export GITHUB_TOKEN=[redacted] && gh pr list",
            ),
            ("DB_PASSWORD=\"x y\" psql", "DB_PASSWORD=[redacted] psql"),
            (
                "aws_secret_access_key=dummy",
                "aws_secret_access_key=[redacted]",
            ),
            (
                "mysql --password=dummy db",
                "mysql --password=[redacted] db",
            ),
            (
                "tool --api-key dummy --verbose",
                "tool --api-key [redacted] --verbose",
            ),
            ("tool --auth-token -v", "tool --auth-token -v"),
            (
                "curl -H 'Authorization: Bearer dummy' https://example.invalid",
                "curl -H 'Authorization: Bearer [redacted]' https://example.invalid",
            ),
            ("Authorization: dummy", "Authorization: [redacted]"),
            ("echo bearer dummy", "echo bearer [redacted]"),
            (
                "curl -H 'X-Api-Key: dummy' x",
                "curl -H 'X-Api-Key: [redacted]' x",
            ),
            (
                "git clone https://u:p@example.invalid/r.git",
                "git clone https://[redacted]@example.invalid/r.git",
            ),
            ("echo sk-test0000000000000000", "echo [redacted]"),
            ("echo ghp_000000000000000000000000", "echo [redacted]"),
            ("echo gho_000000000000000000000000", "echo [redacted]"),
            ("echo ghs_000000000000000000000000", "echo [redacted]"),
            ("echo github_pat_0000000000000000_x", "echo [redacted]"),
            ("echo xoxb-0000-0000-dummy", "echo [redacted]"),
            ("echo AKIA0000000000000000", "echo [redacted]"),
            ("echo glpat-00000000000000000000", "echo [redacted]"),
            ("echo 0123456789abcdef0123456789abcdef01", "echo [redacted]"),
            (
                "echo ZHVtbXkgZHVtbXkgZHVtbXkgZHVtbXkgZHVt0==",
                "echo [redacted]",
            ),
        ] {
            assert_eq!(redact(raw), want, "{raw}");
        }
    }

    /// What has no secret in it comes through as it went in.
    #[test]
    fn ordinary_commands_survive() {
        for raw in [
            "cargo test",
            "ls -la",
            "src/monkey.rs",
            "rm -rf build",
            "git log --oneline -20",
            "cat /home/someone/projects/a-long-directory-name/another_one/file.rs",
            "cargo test yolo::tests::the_hooks_allow_is_listed_and_never_waits_long",
            "https://example.invalid/path?q=1",
            "grep -rn 'fn main' src",
        ] {
            assert_eq!(redact(raw), raw);
        }
    }

    /// Redacting what was already redacted changes nothing, so a detail that
    /// was cleaned in the hook can be cleaned again when it is written.
    #[test]
    fn redaction_is_idempotent() {
        let once =
            redact("TOKEN=dummy curl -H 'Authorization: Bearer dummy' https://u:p@x.invalid");
        assert_eq!(redact(&once), once);
    }

    /// A multi-line command is kept whole, control characters other than the
    /// newline and tab are dropped, and an overlong one is cut and says so.
    #[test]
    fn clean_keeps_lines_and_caps() {
        let (text, cut) = clean("cat <<EOF\nline\tone\x1b[31m\nEOF");
        assert_eq!(text, "cat <<EOF\nline\tone[31m\nEOF");
        assert!(!cut);
        let (text, cut) = clean(&"word ".repeat(MAX_DETAIL));
        assert_eq!(text.chars().count(), MAX_DETAIL);
        assert!(cut);
    }

    #[test]
    fn a_call_is_its_tool_and_whole_detail() {
        let body = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "TOKEN=dummy make\nsecond line"},
        });
        assert_eq!(
            call_of(&body),
            Some(Call {
                tool: "Bash".into(),
                detail: Some("TOKEN=[redacted] make\nsecond line".into()),
                truncated: false,
            })
        );
        assert_eq!(call_of(&serde_json::json!({ "tool_input": {} })), None);
        let bare =
            call_of(&serde_json::json!({ "tool_name": "mcp__x__y", "tool_input": {"a": 1} }));
        assert_eq!(bare.unwrap().detail, None);
    }

    /// One line per append, the file 0600 and the directory closed to others.
    #[test]
    fn lines_are_appended_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let log = scratch("mode");
        let call = Call {
            tool: "Bash".into(),
            detail: Some("TOKEN=dummy ls".into()),
            truncated: false,
        };
        log.append(&Line::allow("s1", Via::Hook, Some(call), None))
            .unwrap();
        log.append(&Line::allow("s1", Via::Key, None, Some("Bash: ls")))
            .unwrap();
        let mode = std::fs::metadata(log.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let dir = std::fs::metadata(&log.dir).unwrap().permissions().mode();
        assert_eq!(dir & 0o007, 0);
        let lines = log.read();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].tool.as_deref(), Some("Bash"));
        assert_eq!(lines[0].detail.as_deref(), Some("TOKEN=[redacted] ls"));
        assert_eq!(
            lines[1].tool, None,
            "a tool was invented for a display line"
        );
        assert_eq!(lines[1].detail.as_deref(), Some("Bash: ls"));
        assert_eq!(lines[1].via, Some(Via::Key));
        let raw = std::fs::read_to_string(log.path()).unwrap();
        assert!(!raw.contains("dummy"), "{raw}");
        let _ = std::fs::remove_dir_all(log.dir.parent().unwrap());
    }

    /// A directory others could read is closed to them on the first write.
    #[test]
    fn an_open_directory_is_closed() {
        use std::os::unix::fs::PermissionsExt;
        let log = scratch("open-dir");
        std::fs::create_dir_all(&log.dir).unwrap();
        std::fs::set_permissions(&log.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        log.append(&Line::allow("s", Via::Key, None, None)).unwrap();
        let mode = std::fs::metadata(&log.dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o750);
        let _ = std::fs::remove_dir_all(log.dir.parent().unwrap());
    }

    /// Past the cap the file moves to `.old` once, and the reader still sees
    /// both, oldest first.
    #[test]
    fn the_file_rotates_once_past_the_cap() {
        let log = scratch("rotate");
        std::fs::create_dir_all(&log.dir).unwrap();
        let mut first = Line::allow("old", Via::Hook, None, Some("x"));
        first.at = "2000-01-01T00:00:00Z".into();
        let mut filler = serde_json::to_string(&first).unwrap();
        filler.push('\n');
        let body = filler.repeat((MAX_BYTES as usize / filler.len()) + 1);
        std::fs::write(log.path(), body).unwrap();
        log.append(&Line::allow("new", Via::Hook, None, Some("y")))
            .unwrap();
        assert!(log.old_path().exists());
        let now = std::fs::read_to_string(log.path()).unwrap();
        assert_eq!(now.lines().count(), 1);
        let all = log.read();
        assert_eq!(all.first().unwrap().session, "old");
        assert_eq!(all.last().unwrap().session, "new");
        let _ = std::fs::remove_dir_all(log.dir.parent().unwrap());
    }

    /// A directory the log cannot be written in is an error to the caller,
    /// not a panic.
    #[test]
    fn an_unwritable_directory_is_only_an_error() {
        let log = Log::at("/proc/cctop-cannot-be-here");
        assert!(
            log.append(&Line::allow("s", Via::Hook, None, None))
                .is_err()
        );
    }

    #[test]
    fn a_test_never_writes_the_real_log() {
        let real = crate::config::data_base().join("cctop");
        let log = Log::default();
        assert!(!log.path().starts_with(&real), "{}", log.path().display());
        assert!(log.path().starts_with(std::env::temp_dir()));
    }

    #[test]
    fn since_reads_durations_and_dates() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-08T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let ago = |w| (now - since(w, now).unwrap()).num_seconds();
        assert_eq!(ago("90s"), 90);
        assert_eq!(ago("30m"), 1800);
        assert_eq!(ago("2h"), 7200);
        assert_eq!(ago("7d"), 7 * 86_400);
        assert_eq!(ago("1w"), 7 * 86_400);
        assert_eq!(
            since("2026-10-01T00:00:00Z", now).unwrap().to_rfc3339(),
            "2026-10-01T00:00:00+00:00"
        );
        assert!(since("2026-10-01", now).is_some());
        assert!(since("soon", now).is_none());
        assert!(since("5y", now).is_none());
    }

    /// `--json --session` over both files, the rotated one included.
    #[test]
    fn the_reader_filters_across_both_files() {
        let log = scratch("reader");
        std::fs::create_dir_all(&log.dir).unwrap();
        let line = |session: &str, at: &str, detail: &str| {
            let mut l = Line::allow(session, Via::Hook, None, Some(detail));
            l.at = at.into();
            serde_json::to_string(&l).unwrap() + "\n"
        };
        std::fs::write(
            log.old_path(),
            line("abc-1", "2026-01-01T00:00:00Z", "old one")
                + &line("zzz-1", "2026-01-01T00:00:01Z", "other"),
        )
        .unwrap();
        std::fs::write(
            log.path(),
            line("abc-2", "2026-02-01T00:00:00Z", "new one") + "not json\n",
        )
        .unwrap();
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        let mut out = Vec::new();
        assert_eq!(
            run_log(&log, &argv(&["--json", "--session", "abc"]), &mut out),
            0
        );
        let got: Vec<Line> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let details: Vec<_> = got.iter().map(|l| l.detail.clone().unwrap()).collect();
        assert_eq!(details, ["old one", "new one"]);

        let mut out = Vec::new();
        run_log(&log, &argv(&["--json", "-n", "1"]), &mut out);
        assert!(String::from_utf8(out).unwrap().contains("new one"));

        let mut out = Vec::new();
        run_log(
            &log,
            &argv(&["--json", "--since=2026-01-15T00:00:00Z"]),
            &mut out,
        );
        assert_eq!(String::from_utf8(out).unwrap().lines().count(), 1);

        let mut out = Vec::new();
        run_log(&log, &argv(&["--session", "zzz"]), &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("zzz-1") && text.contains("allow/hook other"),
            "{text}"
        );

        assert_eq!(run_log(&log, &argv(&["--bogus"]), &mut Vec::new()), 2);
        assert_eq!(run_log(&log, &argv(&["-n", "x"]), &mut Vec::new()), 2);
        let _ = std::fs::remove_dir_all(log.dir.parent().unwrap());
    }

    #[test]
    fn rendering_reads_as_a_sentence() {
        let mut l = Line::allow(
            "0123456789",
            Via::Key,
            Some(Call {
                tool: "Bash".into(),
                detail: Some("cat <<EOF\nx\nEOF".into()),
                truncated: true,
            }),
            None,
        );
        l.cwd = Some("/w/proj".into());
        l.harness = Some("codex".into());
        let text = render(&l);
        assert!(
            text.ends_with("01234567 proj [codex] allow/key Bash: cat <<EOF ⏎ x ⏎ EOF …"),
            "{text}"
        );
        l.ok = Some(false);
        l.why = Some("no pty".into());
        assert!(render(&l).contains("allow/key FAILED Bash:"));
        assert!(render(&l).ends_with("(no pty)"));
        let on = Line {
            event: Event::On,
            from: Some(Origin::Web),
            pid: Some(42),
            ..l.clone()
        };
        assert!(render(&on).ends_with("YOLO on from the web pid 42"));
    }
}
