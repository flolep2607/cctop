//! Recall: the passages of past sessions that answer a question, for an agent
//! to read mid-task.
//!
//! cctop's search answers a person looking for a *session* — which row, which
//! afternoon. An agent halfway through a task asks something else: "was this
//! already decided, and why", "what did we find last time this broke". It wants
//! the paragraph that says so, from whichever harness it was said in, and it
//! wants it ranked, because it will read the first few and stop.
//!
//! So this returns passages rather than sessions, and finds them in two steps.
//!
//! **Which sessions.** Both of the tiers search already has: the literal scan
//! ([`crate::session::search`]), which finds a name or an error string however
//! far apart its words are, and the topical index ([`crate::embed`]), which
//! finds the conversation that was *about* the question when none of its words
//! were used. Either is enough to make a session a candidate.
//!
//! **Which passages.** Every candidate's conversation is cut into the same
//! chunks the topical index embeds, and each chunk is ranked twice: by how many
//! of the query's words it contains, and by how close its meaning is. The two
//! rankings are fused by reciprocal rank — `Σ 1/(k + rank)` — rather than by
//! adding the scores, because a count of matched words and a cosine are on
//! scales nobody can honestly weigh against each other, while a rank is a rank.
//! A passage near the top of both lists beats one at the top of only one.
//!
//! Nothing here calls a model over the network or writes anything but the
//! topical index cctop already keeps. Without the search model fetched it is
//! the literal half alone, and says so.
//!
//! ponytail: passages come from the harnesses whose transcripts are files the
//! topical index can read (see [`crate::embed::index::chunks_of`]). A session
//! that only the literal scan can read — one kept in a database — still turns
//! up, but as its one matching line rather than a passage.

use crate::embed::index::chunks_of;
use crate::session::Session;
use crate::session::search::{Query, Target};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;

/// How many passages an answer carries when not told.
///
/// An agent reads these inside its own context, where every one costs it
/// tokens; eight is enough to cover a decision and its context without turning
/// the answer into a second transcript.
pub const DEFAULT_LIMIT: usize = 8;

/// At most this many passages from one session.
///
/// The best session usually has several good chunks, and without a cap it
/// would crowd out the one other session that has the actual answer.
const PER_SESSION: usize = 2;

/// The longest passage returned, in characters — about 300 tokens.
const PASSAGE_CHARS: usize = 1200;

/// The most sessions whose passages are read and ranked.
///
/// Literal hits come newest first, so this keeps the recent ones; a question
/// that matches more sessions than this is a common word, and its answer is
/// in the recent ones anyway.
const MAX_CANDIDATES: usize = 40;

/// Same floor and session limit as the dashboard's topical tier, so the agent
/// and the person get the same idea of what a session is about.
const TOPICAL_FLOOR: f32 = 0.22;
const TOPICAL_SESSIONS: usize = 10;

/// The `k` in reciprocal-rank fusion. 60 is the value the method was published
/// with and the one everyone uses; it flattens the difference between first
/// and fifth place enough that a passage strong in both lists wins.
const RRF_K: f32 = 60.0;

/// One passage, and where it came from.
#[derive(Debug, Clone, Serialize)]
pub struct Passage {
    pub session_id: String,
    pub harness: String,
    pub provider: String,
    pub directory: String,
    pub title: Option<String>,
    pub last_active: String,
    /// Which chunk of the session this is, for `read_passage` to open it with
    /// its neighbours. `None` for a session only the literal scan can read.
    pub passage: Option<usize>,
    pub text: String,
    /// `words`, `topic` or `both`: which ranking put this here.
    pub matched: &'static str,
}

/// An answer, with what it could and could not do.
#[derive(Debug, Serialize)]
pub struct Recall {
    pub query: String,
    pub passages: Vec<Passage>,
    /// Whether meaning-based matching took part. False means the search model
    /// was not fetched, and only literal words were matched.
    pub topical: bool,
    pub sessions_matched: usize,
}

/// A chunk of one candidate session, with both of its scores.
struct Scored {
    session: usize,
    chunk: usize,
    text: String,
    words: usize,
    topic: Option<f32>,
}

/// The query's words, as a chunk would contain them.
///
/// Filler stripped by the same rule the topical tier uses, so "how did we
/// decide the cache layout" scores chunks on `decide`, `cache` and `layout`
/// rather than on `how` and `the`.
fn words_of(query: &str) -> Vec<String> {
    crate::embed::topic_of(query)
        .split_whitespace()
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() >= 2)
        .collect()
}

/// How many of `words` start a word somewhere in `text`.
///
/// At the start of a word, not anywhere in one: a plain substring test let
/// "rent" match "current" and "different", which put a table of widget crates
/// second for a question about renting GPUs. A prefix still lets "gpu" match
/// "gpus" and "decide" match "decided".
fn word_hits(text: &str, words: &[String]) -> usize {
    let lower = text.to_lowercase();
    let tokens: Vec<&str> = lower
        .split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '-' || c == '_'))
        .filter(|t| !t.is_empty())
        .collect();
    words
        .iter()
        .filter(|w| tokens.iter().any(|t| t.starts_with(w.as_str())))
        .count()
}

/// How many of a query's words a passage must contain to count as matching
/// on words at all.
///
/// One, for a query of one or two words — a name is a name. Half, past that:
/// "the afternoon I was pricing GPUs to rent" ranked a table of widget crates
/// second because it said "pricing" once, and one word of four is not evidence
/// of anything. A passage below the bar can still come back on its meaning.
fn enough_words(n: usize) -> usize {
    match n {
        0..=2 => 1,
        n => n.div_ceil(2),
    }
}

/// The part of a chunk worth reading: a window around its first matched word,
/// or its opening when nothing literal matched.
fn excerpt(text: &str, words: &[String]) -> String {
    let lower = text.to_lowercase();
    // Byte offsets in `lower` match `text` only for ASCII case changes, so the
    // window is placed by character count, which both spellings share.
    let first = words
        .iter()
        .filter_map(|w| lower.find(w.as_str()))
        .min()
        .map(|byte| lower[..byte].chars().count())
        .unwrap_or(0);
    let start = first.saturating_sub(PASSAGE_CHARS / 5);
    let total = text.chars().count();
    let body: String = text.chars().skip(start).take(PASSAGE_CHARS).collect();
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(body.trim());
    if start + PASSAGE_CHARS < total {
        out.push('…');
    }
    out
}

/// Rank positions, best first, for the chunks `score` says anything about.
fn ranks(scored: &[Scored], score: impl Fn(&Scored) -> Option<f32>) -> HashMap<usize, usize> {
    let mut order: Vec<(usize, f32)> = scored
        .iter()
        .enumerate()
        .filter_map(|(i, s)| score(s).map(|v| (i, v)))
        .collect();
    order.sort_by(|a, b| b.1.total_cmp(&a.1));
    order
        .into_iter()
        .enumerate()
        .map(|(rank, (i, _))| (i, rank))
        .collect()
}

/// The search model and its index, when the model has been fetched.
///
/// Loading is best effort for the same reason as the dashboard's: recall must
/// keep answering with words alone on a machine that never fetched the model,
/// or whose cached copy is damaged.
fn topical(targets: &[Target]) -> Option<(crate::embed::Model, crate::embed::index::Index)> {
    if !crate::embed::fetch::present() {
        return None;
    }
    let model = crate::embed::Model::load(&crate::embed::fetch::model_dir()).ok()?;
    let mut index =
        crate::embed::index::Index::load(&crate::config::EMBEDDING_INDEX_FILE).unwrap_or_default();
    if index.refresh(&model, targets) > 0 {
        let _ = index.save(&crate::config::EMBEDDING_INDEX_FILE);
    }
    Some((model, index))
}

/// The passages of `sessions` that best answer `query`.
///
/// `exclude` drops one session by id — the caller's own, whose conversation
/// contains the question being asked and would otherwise be its first answer.
pub fn recall(sessions: &[Session], query: &str, limit: usize, exclude: Option<&str>) -> Recall {
    let mut pool: Vec<&Session> = sessions
        .iter()
        .filter(|s| exclude != Some(s.session_id.as_str()))
        .collect();
    pool.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    let targets: Vec<Target> = pool.iter().map(|s| Target::of(s)).collect();

    // Which sessions: the literal tier, in parallel, newest first.
    let parsed = Query::parse(query);
    let literal: Vec<(usize, String)> = if parsed.is_empty() {
        Vec::new()
    } else {
        targets
            .par_iter()
            .enumerate()
            .filter_map(|(i, t)| {
                crate::session::search::find_query(t, &parsed).map(|h| (i, h.snippet))
            })
            .collect()
    };
    let topics = topical(&targets);
    let query_vec = topics
        .as_ref()
        .map(|(model, _)| model.embed(&crate::embed::topic_of(query)));
    let mut candidates: Vec<usize> = literal.iter().map(|(i, _)| *i).collect();
    if let (Some((_, index)), Some(qv)) = (&topics, &query_vec) {
        for (key, _, _) in index.search(qv, TOPICAL_FLOOR, TOPICAL_SESSIONS) {
            if let Some(i) = targets.iter().position(|t| t.key == key)
                && !candidates.contains(&i)
            {
                candidates.push(i);
            }
        }
    }
    candidates.sort_unstable();
    candidates.truncate(MAX_CANDIDATES);
    let sessions_matched = candidates.len();

    // Which passages: every candidate's chunks, scored both ways.
    let words = words_of(query);
    let model = topics.as_ref().map(|(m, _)| m);
    let qv = query_vec.as_ref();
    let mut scored: Vec<Scored> = candidates
        .par_iter()
        .flat_map_iter(|&i| {
            let chunks = targets[i]
                .data_file
                .as_deref()
                .map(chunks_of)
                .unwrap_or_default();
            let words = &words;
            chunks.into_iter().enumerate().map(move |(n, text)| Scored {
                session: i,
                chunk: n,
                words: word_hits(&text, words),
                topic: match (model, qv) {
                    (Some(m), Some(qv)) => Some(crate::embed::cosine(qv, &m.embed(&text))),
                    _ => None,
                },
                text,
            })
        })
        .collect();
    let enough = enough_words(words.len());
    scored.retain(|s| s.words >= enough || s.topic.is_some_and(|t| t >= TOPICAL_FLOOR));

    let by_words = ranks(&scored, |s| (s.words >= enough).then_some(s.words as f32));
    let by_topic = ranks(&scored, |s| s.topic.filter(|t| *t >= TOPICAL_FLOOR));
    let fused = |i: usize| -> f32 {
        let part = |r: Option<&usize>| r.map_or(0.0, |r| 1.0 / (RRF_K + *r as f32));
        part(by_words.get(&i)) + part(by_topic.get(&i))
    };
    let mut order: Vec<usize> = (0..scored.len()).collect();
    // Ties — common when only one ranking exists — go to the newer session,
    // which is the lower index because the pool is sorted newest first.
    order.sort_by(|&a, &b| {
        fused(b)
            .total_cmp(&fused(a))
            .then(scored[a].session.cmp(&scored[b].session))
            .then(scored[a].chunk.cmp(&scored[b].chunk))
    });

    let describe = |i: usize, passage: Option<usize>, text: String, matched| {
        let s = pool[i];
        Passage {
            session_id: s.session_id.clone(),
            harness: s.harness.clone(),
            provider: s.provider.as_str().to_string(),
            directory: s.label_source.clone(),
            title: s.title.clone(),
            last_active: s.last_active.clone(),
            passage,
            text,
            matched,
        }
    };
    let mut taken: HashMap<usize, usize> = HashMap::new();
    let mut passages: Vec<Passage> = Vec::new();
    for i in order {
        if passages.len() >= limit {
            break;
        }
        let s = &scored[i];
        let count = taken.entry(s.session).or_default();
        if *count >= PER_SESSION {
            continue;
        }
        *count += 1;
        let matched = match (by_words.contains_key(&i), by_topic.contains_key(&i)) {
            (true, true) => "both",
            (true, false) => "words",
            _ => "topic",
        };
        passages.push(describe(
            s.session,
            Some(s.chunk),
            excerpt(&s.text, &words),
            matched,
        ));
    }

    // A literal hit whose transcript has no readable chunks still answers,
    // as the line it matched on, after every real passage.
    for (i, snippet) in &literal {
        if passages.len() >= limit {
            break;
        }
        if candidates.contains(i) && !scored.iter().any(|s| s.session == *i) {
            passages.push(describe(*i, None, snippet.clone(), "words"));
        }
    }

    Recall {
        query: query.to_string(),
        passages,
        topical: topics.is_some(),
        sessions_matched,
    }
}

/// Passage `n` of one session with `around` neighbours either side, for
/// reading a hit in context.
///
/// `session` may be a prefix of the id, as long as only one session has it.
pub fn read(
    sessions: &[Session],
    session: &str,
    n: usize,
    around: usize,
) -> Result<String, String> {
    let matching: Vec<&Session> = sessions
        .iter()
        .filter(|s| s.session_id.starts_with(session))
        .collect();
    let s = match matching.as_slice() {
        [one] => *one,
        [] => return Err(format!("no session starts with {session}")),
        many => {
            return Err(format!(
                "{} sessions start with {session}; give more of the id",
                many.len()
            ));
        }
    };
    let chunks = s.data_file.as_deref().map(chunks_of).unwrap_or_default();
    if chunks.is_empty() {
        return Err(format!(
            "session {} has no readable conversation",
            s.session_id
        ));
    }
    if n >= chunks.len() {
        return Err(format!(
            "session {} has {} passages; {n} is past the end",
            s.session_id,
            chunks.len()
        ));
    }
    let from = n.saturating_sub(around);
    let to = (n + around).min(chunks.len() - 1);
    let mut out = format!(
        "{} · {} · {} · {}\n",
        s.harness,
        s.label_source,
        s.last_active,
        s.title.as_deref().unwrap_or("untitled")
    );
    for (k, chunk) in chunks.iter().enumerate().take(to + 1).skip(from) {
        out.push_str(&format!("\n[passage {k} of {}]\n{chunk}\n", chunks.len()));
    }
    Ok(out)
}

/// The session the calling process belongs to, when cctop can tell.
///
/// An agent spawns its MCP servers as children, so the server's parent is the
/// agent itself, and cctop already knows which process each session runs in.
/// ponytail: a harness that launches MCP servers through a wrapper process has
/// a parent cctop does not recognise, and then nothing is excluded — the
/// caller's own session can come back among the answers.
pub fn caller(sessions: &[Session]) -> Option<String> {
    let parent = std::os::unix::process::parent_id();
    sessions
        .iter()
        .find(|s| {
            s.process
                .as_ref()
                .is_some_and(|p| p.process_list.iter().any(|e| e.pid == parent))
        })
        .map(|s| s.session_id.clone())
}

/// The text form, for `cctop recall` and the MCP tool alike.
pub fn render(r: &Recall) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if r.passages.is_empty() {
        let _ = writeln!(out, "Nothing in past sessions matches \"{}\".", r.query);
    }
    for (k, p) in r.passages.iter().enumerate() {
        let day = p.last_active.get(..10).unwrap_or(&p.last_active);
        let _ = writeln!(
            out,
            "[{}] {} · {} · {} · {}{}",
            k + 1,
            p.harness,
            tilde(&p.directory),
            day,
            p.title.as_deref().unwrap_or("untitled"),
            match p.passage {
                Some(n) => format!(
                    "  (session {} passage {n}, by {})",
                    short(&p.session_id),
                    p.matched
                ),
                None => format!("  (session {}, one matching line)", short(&p.session_id)),
            }
        );
        for line in p.text.lines() {
            let _ = writeln!(out, "    {line}");
        }
        out.push('\n');
    }
    if !r.topical {
        let _ = writeln!(
            out,
            "Matched on words only: the search model is not fetched (`cctop --fetch-search-model`)."
        );
    }
    out
}

/// A directory with the home prefix folded to `~`, as a person writes it.
fn tilde(dir: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && dir.starts_with(&home) => {
            format!("~{}", &dir[home.len()..])
        }
        _ => dir.to_string(),
    }
}

/// A session id short enough to read and still unique in practice.
fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

pub const HELP: &str = "\
cctop recall — what past sessions said about something, from any agent

USAGE:
  cctop recall QUERY... [--limit N] [--json]
  cctop recall --read SESSION PASSAGE [--around N]

Searches every transcript on this machine — Claude Code, Codex, OpenCode and the
rest — and returns the passages that best answer QUERY: by its words, and by its
meaning when the search model is fetched (`cctop --fetch-search-model`). Agents
get the same thing through `cctop --mcp`; see `cctop --install-mcp`.

OPTIONS:
  --limit N        Passages to return (default 8).
  --read S P       Print passage P of session S (an id or a unique prefix) with
                   its neighbours, to read a hit in context.
  --around N       Neighbours either side for --read (default 1).
  --json           Machine-readable.
  -h, --help       This.

Read-only, apart from the topical index cctop already keeps up to date.
";

/// `cctop recall`.
pub fn run(argv: &[String]) -> i32 {
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return 0;
    }
    let mut limit = DEFAULT_LIMIT;
    let mut json = false;
    let mut read: Option<(String, usize)> = None;
    let mut around = 1;
    let mut words: Vec<String> = Vec::new();
    let mut args = argv.iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--json" => json = true,
            "--limit" => match args.next().and_then(|n| n.parse().ok()) {
                Some(n) if n > 0 => limit = n,
                _ => return usage("--limit needs a number"),
            },
            "--around" => match args.next().and_then(|n| n.parse().ok()) {
                Some(n) => around = n,
                None => return usage("--around needs a number"),
            },
            "--read" => match (args.next(), args.next().and_then(|n| n.parse().ok())) {
                (Some(s), Some(n)) => read = Some((s.clone(), n)),
                _ => return usage("--read needs a session and a passage number"),
            },
            flag if flag.starts_with("--") => return usage(&format!("unknown option {flag}")),
            word => words.push(word.to_string()),
        }
    }

    let mut loader = crate::loader::Loader::new();
    let sessions = loader.load(crate::pricing::Plan::Retail);
    loader.store().save();

    if let Some((session, n)) = read {
        return match self::read(&sessions, &session, n, around) {
            Ok(text) => {
                print!("{text}");
                0
            }
            Err(e) => {
                eprintln!("cctop recall: {e}");
                2
            }
        };
    }
    let query = words.join(" ");
    if query.trim().is_empty() {
        return usage("give something to recall, e.g. `cctop recall why the cache is not shared`");
    }
    let answer = recall(&sessions, &query, limit, None);
    match json {
        true => println!(
            "{}",
            serde_json::to_string_pretty(&answer).unwrap_or_else(|_| "{}".into())
        ),
        false => print!("{}", render(&answer)),
    }
    0
}

fn usage(why: &str) -> i32 {
    eprintln!("cctop recall: {why}; see --help");
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two rankings fused by rank, not by score: a passage that is good on
    /// both beats one that tops only one of them.
    #[test]
    fn a_passage_strong_both_ways_outranks_one_strong_one_way() {
        let chunk = |session, words, topic| Scored {
            session,
            chunk: 0,
            text: String::new(),
            words,
            topic: Some(topic),
        };
        let scored = vec![
            chunk(0, 3, 0.10), // most words, no meaning
            chunk(1, 0, 0.60), // best meaning, no words
            chunk(2, 2, 0.45), // second on both
        ];
        let w = ranks(&scored, |s| (s.words > 0).then_some(s.words as f32));
        let t = ranks(&scored, |s| s.topic.filter(|v| *v >= TOPICAL_FLOOR));
        let fused = |i: usize| {
            let part = |r: Option<&usize>| r.map_or(0.0, |r| 1.0 / (RRF_K + *r as f32));
            part(w.get(&i)) + part(t.get(&i))
        };
        assert!(fused(2) > fused(0));
        assert!(fused(2) > fused(1));
    }

    /// The window lands on the match, so the passage shows why it came back.
    #[test]
    fn an_excerpt_is_centred_on_the_first_match() {
        let text = format!(
            "{}the cache layout was decided here{}",
            "x".repeat(3000),
            "y".repeat(3000)
        );
        let words = vec!["layout".to_string()];
        let out = excerpt(&text, &words);
        assert!(out.contains("cache layout"), "{out}");
        assert!(out.starts_with('…') && out.ends_with('…'));
        assert!(out.chars().count() <= PASSAGE_CHARS + 2);

        let short = excerpt("no match at all", &words);
        assert_eq!(short, "no match at all");
    }

    #[test]
    fn a_long_question_needs_more_than_one_of_its_words() {
        assert_eq!(enough_words(1), 1);
        assert_eq!(enough_words(2), 1);
        assert_eq!(enough_words(3), 2);
        assert_eq!(enough_words(4), 2);
        assert_eq!(enough_words(5), 3);
    }

    /// Filler does not count as a matched word — "how did we" is in everything.
    #[test]
    fn filler_is_not_a_word_worth_matching() {
        let words = words_of("how did we decide the cache layout");
        assert!(words.contains(&"cache".to_string()));
        assert!(!words.contains(&"the".to_string()));
        assert!(!words.contains(&"how".to_string()));
        assert_eq!(word_hits("The Cache LAYOUT", &words), 2);

        // A word inside another word is not that word.
        let rent = words_of("renting GPUs");
        assert_eq!(
            word_hits("the current and different", &["rent".to_string()]),
            0
        );
        assert_eq!(
            word_hits("we priced gpus to rent", &rent),
            1,
            "gpus, by prefix"
        );
    }

    fn session_with(dir: &std::path::Path, id: &str, lines: &[&str]) -> Session {
        let path = dir.join(format!("{id}.jsonl"));
        let body: String = lines
            .iter()
            .map(|text| {
                serde_json::json!({"type": "user", "message": {"role": "user", "content": text}})
                    .to_string()
                    + "\n"
            })
            .collect();
        std::fs::write(&path, body).unwrap();
        let mut s = Session::new(crate::pricing::Provider::Claude, id.into());
        s.data_file = Some(path);
        s.last_active = "2026-10-01T10:00:00Z".into();
        s
    }

    /// End to end on real transcript files: the session that discussed the
    /// thing comes back as a passage, the caller's own session does not, and
    /// a passage can be opened by number.
    #[test]
    fn recall_finds_the_passage_and_skips_the_caller() {
        let dir = std::env::temp_dir().join(format!("cctop-recall-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let long = "filler words about nothing in particular ".repeat(20);
        let sessions = vec![
            session_with(
                &dir,
                "past",
                &[
                    &long,
                    "We chose a sharded cache because the single file grew past 120 MB.",
                ],
            ),
            session_with(&dir, "other", &["Lunch plans and nothing else."]),
            session_with(
                &dir,
                "me",
                &["Why is the cache sharded? I am asking right now."],
            ),
        ];
        let answer = recall(&sessions, "sharded cache", 8, Some("me"));
        assert!(!answer.passages.is_empty());
        assert_eq!(answer.passages[0].session_id, "past");
        assert!(answer.passages[0].text.contains("120 MB"));
        assert!(answer.passages.iter().all(|p| p.session_id != "me"));
        assert!(answer.passages.iter().all(|p| p.session_id != "other"));

        let n = answer.passages[0].passage.unwrap();
        let opened = read(&sessions, "pa", n, 1).unwrap();
        assert!(opened.contains("120 MB"));
        assert!(read(&sessions, "nope", 0, 1).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
