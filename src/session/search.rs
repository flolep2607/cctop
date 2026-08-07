//! Full-text search across a session's transcript.
//!
//! The table's `/` filter matches what is already on screen — label, model,
//! harness, project, branch, id — and that is all it can do without reading
//! anything. Finding the session where you discussed a particular function
//! means going into the transcripts, which is what this module is for.
//!
//! Two things shape the implementation. Transcripts are large and numerous, so
//! nothing here parses: matching is a case-insensitive byte scan that stops at
//! the first hit in a session, and a session is abandoned once
//! [`MAX_SCAN_BYTES`] have gone by without one. And "one session" is not "one
//! file" for every provider — Claude spreads subagents across a directory,
//! while OpenCode and Windsurf pack every session of a workspace into one
//! shared database — so the corpus is chosen per provider rather than from
//! `data_file` alone.
//!
//! The text being searched is the transcript as stored, which for the
//! file-backed providers means JSON with its strings escaped. A plain word
//! matches; a phrase containing a quote, a backslash or a newline is escaped on
//! disk and will not.

use super::Session;
use crate::pricing::Provider;
use rusqlite::{Connection, OpenFlags, params};
use std::io::{BufRead, BufReader};
use std::path::Path;

/// How much of one session's transcript is scanned before giving up.
///
/// A miss costs a full read, so this is the ceiling on what one non-matching
/// session can spend. 64 MiB covers essentially every real transcript; the few
/// that exceed it are matched on their first 64 MiB rather than not at all,
/// which is why a truncated scan reports no match instead of an error.
const MAX_SCAN_BYTES: u64 = 64 * 1024 * 1024;

/// Characters of context kept around a hit for the snippet.
const SNIPPET_PAD: usize = 48;

/// The longest snippet handed back to the UI.
const SNIPPET_CHARS: usize = 160;

/// Where a match was found, and the text around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// A cleaned-up window of the matching line, for showing in the UI.
    pub snippet: String,
}

/// One session, reduced to what a scan needs.
///
/// A scan runs over every session in the table, on another thread, so it is
/// handed these rather than `Session` clones — a row carries per-day cost maps,
/// subagents and tool details, none of which a byte scan will ever look at.
#[derive(Debug, Clone)]
pub struct Target {
    /// [`Session::key`], so results can be matched back to rows.
    pub key: String,
    pub provider: Provider,
    pub session_id: String,
    pub data_file: Option<std::path::PathBuf>,
    /// A live session's transcript is still growing, which is what makes its
    /// result unsafe to remember.
    pub running: bool,
}

impl Target {
    pub fn of(session: &Session) -> Target {
        Target {
            key: session.key(),
            provider: session.provider,
            session_id: session.session_id.clone(),
            data_file: session.data_file.clone(),
            running: session.is_running(),
        }
    }
}

/// Find `needle` in a session's transcript.
///
/// `needle` must already be lowercase — callers search many sessions with one
/// query, and lowercasing it once per session would be the only allocation in
/// the hot path.
pub fn find(session: &Target, needle: &str) -> Option<Hit> {
    if needle.is_empty() {
        return None;
    }
    let file = session.data_file.as_deref()?;
    match session.provider {
        // Subagents live in files of their own beside the parent's, and work
        // delegated to one is exactly what someone searches for.
        Provider::Claude => super::transcript_files(file)
            .iter()
            .find_map(|path| scan_file(path, needle)),
        Provider::Codex | Provider::Cursor | Provider::Gemini | Provider::Pi => {
            scan_file(file, needle)
        }
        // One database holds every session, so scanning the file would report a
        // hit for all of them whenever any one matched.
        Provider::OpenCode => scan_opencode(file, &session.session_id, needle),
        Provider::Windsurf => scan_windsurf(file, &session.session_id, needle),
    }
}

/// Scan a text transcript line by line.
fn scan_file(path: &Path, needle: &str) -> Option<Hit> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut read = 0u64;
    loop {
        line.clear();
        // Lossless UTF-8 is not guaranteed for a file something else wrote, and
        // one bad byte must not end the scan; `read_line` errors out on it, so
        // the line is skipped and the scan continues.
        match reader.read_line(&mut line) {
            Ok(0) => return None,
            Ok(n) => read += n as u64,
            Err(_) => {
                read += 1;
                if read >= MAX_SCAN_BYTES {
                    return None;
                }
                continue;
            }
        }
        if let Some(hit) = find_in(&line, needle) {
            return Some(hit);
        }
        if read >= MAX_SCAN_BYTES {
            return None;
        }
    }
}

fn readonly(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

/// Scan the messages OpenCode recorded for one session.
fn scan_opencode(path: &Path, session_id: &str, needle: &str) -> Option<Hit> {
    let db = readonly(path).ok()?;
    // `part` carries the text of a message; `message` carries the envelope, and
    // the tool calls that are worth finding a session by. Both are per-session
    // in this schema, so neither can leak another session's text into this hit.
    for sql in [
        "SELECT data FROM part WHERE session_id = ?1 ORDER BY id",
        "SELECT data FROM message WHERE session_id = ?1 ORDER BY time_created, id",
    ] {
        // The `part` table is absent in older databases, which is a failed
        // prepare rather than an empty result — hence trying each in turn.
        let Ok(mut stmt) = db.prepare(sql) else {
            continue;
        };
        let Ok(rows) = stmt.query_map(params![session_id], |row| row.get::<_, String>(0)) else {
            continue;
        };
        for raw in rows.flatten() {
            if let Some(hit) = find_in(&raw, needle) {
                return Some(hit);
            }
        }
    }
    None
}

/// Scan the Cascade tab holding one Windsurf conversation.
fn scan_windsurf(path: &Path, session_id: &str, needle: &str) -> Option<Hit> {
    let db = readonly(path).ok()?;
    let data = super::windsurf::chat_data(&db)?;
    let tab = super::windsurf::tabs(&data)
        .iter()
        .find(|tab| super::windsurf::tab_id(tab).as_deref() == Some(session_id))?;
    find_in(&tab.to_string(), needle)
}

/// The first occurrence of `needle` in `haystack`, with its surroundings.
///
/// Lowercasing the haystack rather than matching case-insensitively in place
/// keeps this to one pass and one allocation per line. Byte offsets from the
/// lowercase copy are only used to *locate* the hit; the snippet is cut from
/// the original by character, so a multi-byte character never splits.
fn find_in(haystack: &str, needle: &str) -> Option<Hit> {
    let lower = haystack.to_ascii_lowercase();
    let at = lower.find(needle)?;
    // Byte offset -> character offset. ASCII-lowercasing preserves length, so
    // the two strings share offsets.
    let chars_before = haystack[..at].chars().count();
    let start = chars_before.saturating_sub(SNIPPET_PAD);
    let window: String = haystack
        .chars()
        .skip(start)
        .take(SNIPPET_PAD * 2 + needle.chars().count())
        .collect();
    Some(Hit {
        snippet: clean(&window),
    })
}

/// Flatten a raw transcript fragment into one printable line.
///
/// Transcript text arrives with escapes, control characters and long runs of
/// whitespace in it. None of that survives being drawn into a single-line
/// widget, so it is collapsed here rather than at every call site.
fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        // Escapes as they appear inside a JSON string: two characters, not one
        // byte. The whitespace ones become a space; the rest are shown as the
        // character they stand for, so a snippet reads as the text that was
        // written rather than as the encoding it was stored in.
        if c == '\\' {
            match chars.peek() {
                Some('n' | 't' | 'r') => {
                    chars.next();
                    space = true;
                    continue;
                }
                Some(&escaped @ ('"' | '\\' | '/')) => {
                    chars.next();
                    if space && !out.is_empty() {
                        out.push(' ');
                    }
                    space = false;
                    out.push(escaped);
                    continue;
                }
                _ => {}
            }
        }
        if c.is_whitespace() || c.is_control() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(c);
    }
    crate::util::truncate(out.trim(), SNIPPET_CHARS)
}

/// Whether a value is worth showing as a snippet at all.
///
/// Unused today outside tests; kept next to [`clean`] because the two define
/// what a usable snippet is between them.
#[cfg(test)]
fn is_meaningful(snippet: &str) -> bool {
    snippet.chars().any(char::is_alphanumeric)
}

/// Serialise a JSON value the way the scanners see it, for tests.
#[cfg(test)]
fn as_text(value: &serde_json::Value) -> String {
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    /// A session whose only interesting property is the file behind it.
    fn target(provider: Provider, path: &std::path::Path) -> Target {
        let mut s = Session::new(provider, "a".into());
        s.data_file = Some(path.to_path_buf());
        Target::of(&s)
    }

    fn temp(name: &str, body: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("cctop-search-{name}"));
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(body.as_bytes()).expect("write");
        path
    }

    /// The whole point of content search: a word that appears nowhere in the
    /// columns still finds its session.
    #[test]
    fn a_word_only_in_the_transcript_is_found() {
        let path = temp(
            "claude.jsonl",
            &format!(
                "{}\n{}\n",
                as_text(&json!({"type": "user", "text": "unrelated chatter"})),
                as_text(&json!({"type": "user", "text": "please fix the flywheel"})),
            ),
        );
        let s = target(Provider::Claude, &path);

        let hit = find(&s, "flywheel").expect("match");
        assert!(hit.snippet.contains("flywheel"), "{}", hit.snippet);
        assert!(is_meaningful(&hit.snippet));
        assert_eq!(find(&s, "kingfisher"), None);
        let _ = std::fs::remove_file(path);
    }

    /// Case folding happens on the file's side, so a lowercase query finds text
    /// that was written in any case.
    #[test]
    fn matching_ignores_case() {
        let path = temp("case.jsonl", "{\"text\":\"Refactor The Loader\"}\n");
        let s = target(Provider::Codex, &path);
        assert!(find(&s, "refactor the loader").is_some());
        let _ = std::fs::remove_file(path);
    }

    /// A session with no transcript at all must return nothing rather than
    /// panicking or reporting a spurious hit.
    #[test]
    fn a_session_without_a_transcript_matches_nothing() {
        let s = Target::of(&Session::new(Provider::Claude, "a".into()));
        assert_eq!(find(&s, "anything"), None);
    }

    /// An empty query is not "matches everything" here — the caller filters on
    /// metadata for that, and scanning every transcript to answer it would be
    /// the most expensive way to say yes.
    #[test]
    fn an_empty_query_matches_nothing() {
        let path = temp("empty-query.jsonl", "text\n");
        let s = target(Provider::Codex, &path);
        assert_eq!(find(&s, ""), None);
        let _ = std::fs::remove_file(path);
    }

    /// Snippets are drawn into one line of a modal: escapes, newlines and runs
    /// of whitespace all have to come out flattened.
    #[test]
    fn a_snippet_is_a_single_printable_line() {
        assert_eq!(clean("a\\nb\tc   d"), "a b c d");
        assert_eq!(clean("  padded  "), "padded");
        assert_eq!(clean("x\u{7}y"), "x y");
        // A snippet reads as the text that was written, not as the JSON it was
        // stored in.
        assert_eq!(clean(r#"rusqlite = \"0.37\""#), r#"rusqlite = "0.37""#);
        assert_eq!(clean(r"C:\\src"), r"C:\src");
    }

    /// The snippet is cut by character, so a hit next to a multi-byte character
    /// must not slice it in half.
    #[test]
    fn a_snippet_survives_multibyte_neighbours() {
        let hit = find_in("héllo → flywheel ← wörld", "flywheel").expect("match");
        assert!(hit.snippet.contains('→'), "{}", hit.snippet);
        assert!(hit.snippet.contains("flywheel"));
    }

    /// A hit at the very start must not underflow the padding, and one at the
    /// very end must not run off the string.
    #[test]
    fn a_snippet_at_either_edge_is_still_produced() {
        assert!(find_in("flywheel at the start", "flywheel").is_some());
        assert!(find_in("at the end is flywheel", "flywheel").is_some());
    }

    /// Long lines are bounded: the modal gets a snippet, not a paragraph.
    #[test]
    fn a_snippet_is_bounded() {
        let long = format!("{}flywheel{}", "x".repeat(5000), "y".repeat(5000));
        let hit = find_in(&long, "flywheel").expect("match");
        assert!(
            hit.snippet.chars().count() <= SNIPPET_CHARS,
            "{} chars",
            hit.snippet.chars().count()
        );
    }
}
