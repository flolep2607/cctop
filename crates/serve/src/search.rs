//! `/api/search` — the dashboard's two search tiers, over the wire.
//!
//! The same search the TUI's `s` runs, not a third implementation of one: the
//! literal tier is [`crate::session::search`]'s byte scan over the transcripts,
//! and the topical tier is the same [`crate::embed`] index, on the same terms —
//! same floor, same limit, same gate on when a query is worth widening. The
//! constants below are copies of `src/ui/worker.rs`'s, which are private there;
//! the two must agree or the page and the terminal answer the same question
//! differently, so they are pinned here by the same assertions and comments.
//!
//! Everything runs on the request thread, like the report route: a search is
//! asked for, not polled, and a slow one is a slow answer rather than a stalled
//! refresh. Whatever can go wrong in the topical half — no model fetched, an
//! index that will not read — degrades to literal hits alone, because a search
//! that needs the index to answer at all is a search that breaks when it is
//! absent.

use crate::session::Session;
use crate::session::search::{Query, Target};
use serde::Serialize;
use std::sync::Mutex;

/// The shortest query worth a transcript scan.
///
/// Under three characters a query is a keystroke rather than a question, and
/// the honest answer to one is "nearly every session" — which the bound below
/// would then truncate into noise.
pub const MIN_CHARS: usize = 3;

/// The most hits one answer carries.
///
/// A search is for finding *the* session; past this many it is a listing, and
/// the table already is one.
const LIMIT: usize = 25;

/// A word count at which a query is a question, not a keyword — see
/// [`worth_widening`]. Mirrors `TOPICAL_WORDS` in `src/ui/worker.rs`.
const TOPICAL_WORDS: usize = 4;

/// How close a chunk has to be to count as being about the query.
///
/// Mirrors `TOPICAL_FLOOR` in `src/ui/worker.rs`: cosine over mean-pooled
/// static vectors puts a right answer around 0.35–0.5 and unrelated text
/// around 0.05 on a real corpus, so this sits below the answers and well above
/// the noise.
const TOPICAL_FLOOR: f32 = 0.22;

const _: () = assert!(TOPICAL_FLOOR > 0.10 && TOPICAL_FLOOR < 0.35);

/// At most this many sessions the topical tier may add — mirrors
/// `TOPICAL_LIMIT` in `src/ui/worker.rs`, so a vague query widens the answer
/// rather than replacing it with everything on the machine.
const TOPICAL_LIMIT: usize = 10;

/// Whether a query is worth going to the index for. The same rule as the
/// TUI's: an empty literal result means the words are not in any transcript,
/// and a sentence-length query is asking about a subject whether or not one
/// of its words happened to appear.
fn worth_widening(no_hits: bool, needle: &str) -> bool {
    no_hits || needle.split_whitespace().count() >= TOPICAL_WORDS
}

/// The topical tier's state, built at most once per server.
///
/// The model is a 30 MB read and the index a megabyte; both are loaded lazily
/// on the first widening query and then kept, since a serve outlives the
/// dozens of searches a session of looking produces. `absent` records that
/// the model was looked for and not found, so a machine without one does not
/// re-check the filesystem on every query.
#[derive(Default)]
pub struct Topics {
    model: Option<crate::embed::Model>,
    index: Option<crate::embed::index::Index>,
    absent: bool,
}

/// One entry in the answer's `hits` list.
#[derive(Serialize)]
pub struct Hit {
    /// [`Session::key`], so the page can address the row.
    pub key: String,
    pub session_id: String,
    /// The text around the match — or, for a topical-only hit, the head of
    /// the chunk that matched, prefixed `~NN%` exactly as the TUI shows it.
    pub snippet: String,
    /// The topical score, when the index found this. `null` for a literal
    /// hit: a substring match is true or absent, it has no confidence.
    pub score: Option<f32>,
}

/// Run both tiers over `sessions` for `needle`, already lowercased.
///
/// `topics` is the server's lazily-built topical state; a poisoned or
/// contended lock is a reason to skip the second tier, not the first.
pub fn run(topics: &Mutex<Topics>, sessions: &[Session], needle: &str) -> Vec<Hit> {
    let targets: Vec<Target> = sessions.iter().map(Target::of).collect();

    // The literal tier. One `Query` parsed once and offered to every
    // transcript, exactly as the worker's `scan` does.
    let query = Query::parse(needle);
    let mut hits: Vec<Hit> = targets
        .iter()
        .filter_map(|target| {
            crate::session::search::find_query(target, &query).map(|found| Hit {
                key: target.key.clone(),
                session_id: target.session_id.clone(),
                snippet: found.snippet,
                score: None,
            })
        })
        .collect();

    if worth_widening(hits.is_empty(), needle)
        && let Ok(mut topics) = topics.lock()
    {
        widen(&mut topics, needle, &targets, &mut hits);
    }
    hits.truncate(LIMIT);
    hits
}

/// Add sessions that are *about* the query to the literal hits.
///
/// The mirror of `topical` in `src/ui/worker.rs`, kept identical in substance:
/// same lazily-loaded model and index, same refresh-and-save, same floor and
/// limit, and the same rule that a session the literal tier found keeps the
/// snippet showing its words — the more precise answer.
fn widen(topics: &mut Topics, needle: &str, targets: &[Target], hits: &mut Vec<Hit>) {
    if topics.absent {
        return;
    }
    if topics.model.is_none() {
        if !crate::embed::fetch::present() {
            topics.absent = true;
            return;
        }
        match crate::embed::Model::load(&crate::embed::fetch::model_dir()) {
            Ok(m) => topics.model = Some(m),
            Err(_) => {
                // A model that will not load is the same as no model here:
                // the search must keep working, and `--fetch-search-model`
                // verifies the load, so this is a damaged cache, not a state
                // worth reporting on a search route.
                topics.absent = true;
                return;
            }
        }
    }
    let Some(model) = topics.model.as_ref() else {
        return;
    };

    let index = topics.index.get_or_insert_with(|| {
        crate::embed::index::Index::load(&crate::config::EMBEDDING_INDEX_FILE).unwrap_or_default()
    });
    if index.refresh(model, targets) > 0 {
        // Best effort: an index that cannot be written is rebuilt next time,
        // which costs a second, and is not worth failing a search over.
        let _ = index.save(&crate::config::EMBEDDING_INDEX_FILE);
    }

    let query = model.embed(&crate::embed::topic_of(needle));
    for (key, snippet, score) in index.search(&query, TOPICAL_FLOOR, TOPICAL_LIMIT) {
        if hits.iter().any(|h| h.key == key) {
            continue;
        }
        hits.push(Hit {
            session_id: targets
                .iter()
                .find(|t| t.key == key)
                .map(|t| t.session_id.clone())
                .unwrap_or_default(),
            key,
            snippet: format!("~{:.0}% {snippet}", score * 100.0),
            score: Some(score),
        });
    }
}
