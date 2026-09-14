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

/// A query broken into the terms that must *all* appear somewhere in a session.
///
/// Splitting on whitespace rather than matching the phrase is the whole point:
/// the session you are looking for is often the one where two names came up,
/// and they may be hundreds of lines apart. Terms are ANDed across the entire
/// transcript, in any order, at any distance.
///
/// Each term carries the two spellings it can be recognised by and how wrong it
/// is allowed to be. See [`Term`].
#[derive(Debug, Clone)]
pub struct Query {
    terms: Vec<Term>,
}

/// One term, with the tiers it can match through.
///
/// `folded` exists because the separators in a name are the part people drop:
/// `vast.ai` gets typed as `vastai`, `gpt-4` as `gpt4`. Folding both sides to
/// their alphanumerics makes that an exact match rather than a near one, which
/// is both cheaper and more precise than spending the edit-distance budget on
/// punctuation.
///
/// `fuzz` is what is left for real typos — a dropped or doubled letter in a
/// name you half-remember.
#[derive(Debug, Clone)]
struct Term {
    /// Lowercase, exactly as typed.
    text: String,
    /// `text` reduced to its ASCII alphanumerics.
    folded: String,
    /// Edit-distance budget for the fuzzy tier; 0 disables it.
    fuzz: usize,
}

/// How wrong a term of a given length may be.
///
/// Short terms get no budget at all: at distance 1 a three-letter word matches
/// a sizeable share of English, and a filter that matches everything is not a
/// filter. The thresholds are deliberately mean — one edit only once a term is
/// long enough to be distinctive, two once it is long enough that two edits
/// still leave it recognisable.
fn fuzz_for(len: usize) -> usize {
    match len {
        0..=4 => 0,
        5..=7 => 1,
        _ => 2,
    }
}

impl Query {
    /// Split a raw query into terms.
    ///
    /// The input is whatever was typed; each term is lowercased here so the
    /// scanners never have to.
    pub fn parse(raw: &str) -> Query {
        let terms = raw
            .split_whitespace()
            .map(|word| {
                let text = word.to_ascii_lowercase();
                let folded: String = text.chars().filter(char::is_ascii_alphanumeric).collect();
                let fuzz = fuzz_for(folded.len());
                Term { text, folded, fuzz }
            })
            .filter(|t| !t.text.is_empty())
            .collect::<Vec<_>>();
        Query { terms }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
}

/// How much of a query a session has satisfied so far.
///
/// This is the piece that makes "both names, far apart" work: it lives across
/// every line, and for Claude across every file, of one session, so a term
/// found on the first line and a term found on the last still add up to a
/// match. It also bounds the cost — the scan stops the moment the last term is
/// satisfied, so a hit is no more expensive than it has to be, and only a miss
/// pays for the whole transcript, exactly as before.
struct Progress<'q> {
    query: &'q Query,
    /// The snippet each term matched on, once it has.
    found: Vec<Option<String>>,
    /// Terms still unsatisfied.
    remaining: usize,
}

impl<'q> Progress<'q> {
    fn new(query: &'q Query) -> Progress<'q> {
        Progress {
            query,
            found: vec![None; query.terms.len()],
            remaining: query.terms.len(),
        }
    }

    /// Offer one line to every term that is still looking, returning whether
    /// the query is now completely satisfied.
    ///
    /// The lowercase copy, the token boundaries and the folded line are built
    /// at most once per line and only when a tier actually needs them: most
    /// lines satisfy nothing and must not pay for the tiers they never reach.
    fn feed(&mut self, line: &str) -> bool {
        if self.remaining == 0 {
            return true;
        }
        let lower = line.to_ascii_lowercase();
        let mut shape: Option<Shape> = None;

        for i in 0..self.query.terms.len() {
            if self.found[i].is_some() {
                continue;
            }
            let term = &self.query.terms[i];

            // Tier 1: the query as typed. A plain substring scan, and the only
            // tier that sees punctuation, so a quoted path or a version number
            // still matches the way it always did.
            if let Some(at) = lower.find(&term.text) {
                self.found[i] = Some(window(line, at, term.text.len()));
                self.remaining -= 1;
                continue;
            }
            if term.folded.is_empty() {
                continue;
            }
            let shape = shape.get_or_insert_with(|| Shape::of(&lower));

            // Tier 2: separators folded away on both sides.
            if let Some((at, span)) = shape.find_folded(&term.folded) {
                self.found[i] = Some(window(line, at, span));
                self.remaining -= 1;
                continue;
            }

            // Tier 3: a real typo, one word at a time.
            if term.fuzz > 0
                && let Some((at, span)) = shape.find_fuzzy(&lower, &term.folded, term.fuzz)
            {
                self.found[i] = Some(window(line, at, span));
                self.remaining -= 1;
            }
        }
        self.remaining == 0
    }

    /// The hit, if every term was satisfied.
    ///
    /// The per-term snippets are joined rather than reduced to one: with two
    /// names matched in different places, seeing both is what tells you this is
    /// the conversation you meant. They are joined into the single string the
    /// UI already stores, so showing the evidence costs the callers nothing.
    fn finish(self) -> Option<Hit> {
        if self.remaining > 0 {
            return None;
        }
        let mut parts: Vec<String> = self.found.into_iter().flatten().collect();
        parts.dedup();
        Some(Hit {
            snippet: crate::util::truncate(&parts.join(" … "), SNIPPET_CHARS),
        })
    }
}

/// A line reduced to the words in it, and to those words run together.
///
/// Both tiers past the first need the same thing — where the alphanumeric runs
/// are — so it is computed once. `folded` is those runs concatenated, and
/// `at[i]` is the byte offset in the original line of `folded`'s byte `i`,
/// which is what turns a match on the folded text back into a snippet of the
/// real one. Folding keeps ASCII alphanumerics only, so `folded` is one byte
/// per character and the two indexes line up.
///
/// ponytail: non-ASCII words are matched by tier 1 alone. Folding and edit
/// distance are defined here over ASCII, which is what identifiers, model names
/// and package names are written in.
struct Shape {
    folded: String,
    at: Vec<usize>,
    /// Byte ranges of each word, into the lowercase line.
    words: Vec<(usize, usize)>,
}

impl Shape {
    fn of(lower: &str) -> Shape {
        let bytes = lower.as_bytes();
        let mut folded = String::with_capacity(bytes.len());
        let mut at = Vec::with_capacity(bytes.len());
        let mut words = Vec::new();
        let mut start = None;
        for (i, &b) in bytes.iter().enumerate() {
            if b.is_ascii_alphanumeric() {
                folded.push(b as char);
                at.push(i);
                start.get_or_insert(i);
            } else if let Some(s) = start.take() {
                words.push((s, i));
            }
        }
        if let Some(s) = start {
            words.push((s, bytes.len()));
        }
        Shape { folded, at, words }
    }

    /// Where `needle` appears in the folded line, as a range of the original.
    fn find_folded(&self, needle: &str) -> Option<(usize, usize)> {
        let at = self.folded.find(needle)?;
        let start = *self.at.get(at)?;
        // The character after the match, or the end of the last one: a folded
        // match spans the separators it folded away, so the snippet is cut from
        // the real text rather than from the concatenation.
        let end = self
            .at
            .get(at + needle.len())
            .copied()
            .unwrap_or_else(|| self.at.last().map_or(start, |&b| b + 1));
        Some((start, end.saturating_sub(start)))
    }

    /// The first word within `fuzz` edits of `needle`.
    ///
    /// ponytail: a word only qualifies if it starts with the same byte. A
    /// mistyped first letter is therefore not corrected — which is the rare
    /// typo, and skipping the rest is what keeps this affordable over a
    /// transcript with millions of words in it.
    fn find_fuzzy(&self, lower: &str, needle: &str, fuzz: usize) -> Option<(usize, usize)> {
        let first = *needle.as_bytes().first()?;
        for &(s, e) in &self.words {
            let word = &lower[s..e];
            if word.len().abs_diff(needle.len()) > fuzz {
                continue;
            }
            if word.as_bytes().first() != Some(&first) {
                continue;
            }
            if within(word, needle, fuzz) {
                return Some((s, e - s));
            }
        }
        None
    }
}

/// Whether `a` and `b` are within `k` edits of each other.
///
/// The full Levenshtein table is not needed to answer a yes/no question, so
/// this keeps two rows and abandons as soon as every cell in the current one is
/// already over budget — which, for the length-filtered candidates that reach
/// it, is almost immediately.
fn within(a: &str, b: &str, k: usize) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len().abs_diff(b.len()) > k {
        return false;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        let mut best = i;
        for j in 1..=b.len() {
            let sub = prev[j - 1] + usize::from(a[i - 1] != b[j - 1]);
            cur[j] = sub.min(prev[j] + 1).min(cur[j - 1] + 1);
            best = best.min(cur[j]);
        }
        if best > k {
            return false;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()] <= k
}

/// Find every term of `needle` in a session's transcript.
///
/// `needle` is the raw query; see [`Query::parse`]. Callers scanning many
/// sessions with one query should parse it once and use [`find_query`].
pub fn find(session: &Target, needle: &str) -> Option<Hit> {
    find_query(session, &Query::parse(needle))
}

/// Find every term of `query` in a session's transcript.
pub fn find_query(session: &Target, query: &Query) -> Option<Hit> {
    if query.is_empty() {
        return None;
    }
    let file = session.data_file.as_deref()?;
    let mut progress = Progress::new(query);
    match session.provider {
        // Subagents live in files of their own beside the parent's, and work
        // delegated to one is exactly what someone searches for. One `Progress`
        // spans them all: a session is the unit being matched, so a term named
        // in the parent and a term named in a subagent still meet.
        Provider::Claude => {
            for path in super::transcript_files(file) {
                if scan_file(&path, &mut progress) {
                    break;
                }
            }
        }
        Provider::Codex | Provider::Cursor | Provider::Gemini | Provider::Pi => {
            scan_file(file, &mut progress);
        }
        // One database holds every session, so scanning the file would report a
        // hit for all of them whenever any one matched.
        Provider::OpenCode => scan_opencode(file, &session.session_id, &mut progress),
        Provider::Windsurf => scan_windsurf(file, &session.session_id, &mut progress),
    }
    progress.finish()
}

/// Scan a text transcript line by line, returning whether the query is
/// satisfied.
fn scan_file(path: &Path, progress: &mut Progress) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut read = 0u64;
    loop {
        line.clear();
        // Lossless UTF-8 is not guaranteed for a file something else wrote, and
        // one bad byte must not end the scan; `read_line` errors out on it, so
        // the line is skipped and the scan continues.
        match reader.read_line(&mut line) {
            Ok(0) => return false,
            Ok(n) => read += n as u64,
            Err(_) => {
                read += 1;
                if read >= MAX_SCAN_BYTES {
                    return false;
                }
                continue;
            }
        }
        if progress.feed(&line) {
            return true;
        }
        if read >= MAX_SCAN_BYTES {
            return false;
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
fn scan_opencode(path: &Path, session_id: &str, progress: &mut Progress) {
    let Ok(db) = readonly(path) else {
        return;
    };
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
            if progress.feed(&raw) {
                return;
            }
        }
    }
}

/// Scan the Cascade tab holding one Windsurf conversation.
fn scan_windsurf(path: &Path, session_id: &str, progress: &mut Progress) {
    let Ok(db) = readonly(path) else {
        return;
    };
    let Some(data) = super::windsurf::chat_data(&db) else {
        return;
    };
    let tabs = super::windsurf::tabs(&data);
    let Some(tab) = tabs
        .iter()
        .find(|tab| super::windsurf::tab_id(tab).as_deref() == Some(session_id))
    else {
        return;
    };
    progress.feed(&tab.to_string());
}

/// The text around a match, cleaned up for display.
///
/// Byte offsets locate the hit; the snippet is cut from the original by
/// character, so a multi-byte character never splits.
fn window(haystack: &str, at: usize, span: usize) -> String {
    let chars_before = haystack[..at].chars().count();
    let span_chars = haystack[at..(at + span).min(haystack.len())]
        .chars()
        .count();
    let start = chars_before.saturating_sub(SNIPPET_PAD);
    let text: String = haystack
        .chars()
        .skip(start)
        .take(SNIPPET_PAD * 2 + span_chars)
        .collect();
    clean(&text)
}

/// The first occurrence of `needle` in `haystack`, with its surroundings.
///
/// The scanners drive [`Progress`] across a whole transcript; this is the same
/// machinery pointed at a single string, which is the unit the snippet tests
/// are written in.
#[cfg(test)]
fn find_in(haystack: &str, needle: &str) -> Option<Hit> {
    let query = Query::parse(needle);
    let mut progress = Progress::new(&query);
    progress.feed(haystack);
    progress.finish()
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

    /// The case this was built for: two names, neither spelled the way it is
    /// written down, hundreds of lines apart in the same conversation.
    #[test]
    fn two_misspelled_names_far_apart_still_find_their_session() {
        let mut body = String::new();
        body.push_str(&as_text(
            &json!({"type": "user", "text": "spun up a box on vast.ai for the weekend"}),
        ));
        body.push('\n');
        for i in 0..400 {
            body.push_str(&as_text(
                &json!({"type": "user", "text": format!("filler {i}")}),
            ));
            body.push('\n');
        }
        body.push_str(&as_text(
            &json!({"type": "user", "text": "nemotron-70b was the one that fit"}),
        ));
        body.push('\n');
        let path = temp("far-apart.jsonl", &body);
        let s = target(Provider::Claude, &path);

        // `vastai` drops the dot; `nemotrn` drops a letter.
        let hit = find(&s, "vastai nemotrn").expect("both terms");
        assert!(hit.snippet.contains("vast.ai"), "{}", hit.snippet);
        assert!(hit.snippet.contains("nemotron"), "{}", hit.snippet);

        // A term that is in no line fails the whole query, however good the
        // others are.
        assert_eq!(find(&s, "vastai nemotrn kingfisher"), None);
        let _ = std::fs::remove_file(path);
    }

    /// Order is not significance: the terms are ANDed, so asking for them
    /// backwards is the same question.
    #[test]
    fn terms_match_in_any_order() {
        let path = temp("order.jsonl", "{\"text\":\"first alpha then omega\"}\n");
        let s = target(Provider::Codex, &path);
        assert!(find(&s, "alpha omega").is_some());
        assert!(find(&s, "omega alpha").is_some());
        let _ = std::fs::remove_file(path);
    }

    /// Folding is what makes a dropped separator free, in both directions: the
    /// query may be missing the punctuation or carrying it.
    #[test]
    fn separators_do_not_have_to_be_typed() {
        let path = temp(
            "folded.jsonl",
            "{\"text\":\"deployed on vast.ai with gpt-4\"}\n",
        );
        let s = target(Provider::Codex, &path);
        assert!(find(&s, "vastai").is_some());
        assert!(find(&s, "gpt4").is_some());
        // And the other way round, against text that ran the words together.
        let plain = temp("plain.jsonl", "{\"text\":\"deployed on vastai\"}\n");
        let t = target(Provider::Codex, &plain);
        assert!(find(&t, "vast.ai").is_some());
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(plain);
    }

    /// The fuzzy tier is deliberately mean. A short word gets no budget at all,
    /// because at one edit it would match half the language.
    #[test]
    fn short_terms_are_not_fuzzy() {
        assert_eq!(fuzz_for(3), 0);
        assert_eq!(fuzz_for(4), 0);
        assert_eq!(fuzz_for(6), 1);
        assert_eq!(fuzz_for(12), 2);

        let path = temp("short.jsonl", "{\"text\":\"the dog sat\"}\n");
        let s = target(Provider::Codex, &path);
        // `log` is one edit from `dog` and must not match.
        assert_eq!(find(&s, "log"), None);
        let _ = std::fs::remove_file(path);
    }

    /// A mistyped first letter is a documented limit, not an oversight: the
    /// prefilter that skips it is what keeps the fuzzy tier affordable.
    #[test]
    fn a_typo_is_corrected_except_in_the_first_letter() {
        let path = temp("firstletter.jsonl", "{\"text\":\"about nemotron today\"}\n");
        let s = target(Provider::Codex, &path);
        assert!(find(&s, "nemotrn").is_some(), "dropped letter");
        assert!(find(&s, "nemotronn").is_some(), "doubled letter");
        assert_eq!(find(&s, "wemotron"), None, "first letter is not corrected");
        let _ = std::fs::remove_file(path);
    }

    /// Bounded edit distance, checked directly — the yes/no the fuzzy tier asks.
    #[test]
    fn edit_distance_respects_its_budget() {
        assert!(within("nemotron", "nemotrn", 1));
        assert!(!within("nemotron", "nemotrn", 0));
        assert!(within("kitten", "sitting", 3));
        assert!(!within("kitten", "sitting", 2));
        assert!(within("same", "same", 0));
        // Length alone can settle it, without walking the table.
        assert!(!within("a", "abcdefgh", 2));
    }

    /// Every term contributes its own window, so a multi-term hit shows why it
    /// matched rather than only where it first did.
    #[test]
    fn a_snippet_shows_each_term_it_matched() {
        let hit =
            find_in("alpha is here and omega is way over there", "alpha omega").expect("match");
        assert!(hit.snippet.contains("alpha"), "{}", hit.snippet);
        assert!(hit.snippet.contains("omega"), "{}", hit.snippet);
        assert!(hit.snippet.chars().count() <= SNIPPET_CHARS);
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
