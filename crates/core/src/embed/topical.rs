//! Which sessions a query is *about*, as opposed to which contain its words.
//!
//! The second tier of search. The first is literal — [`crate::session::search`]
//! reads transcripts for the terms — and it fails in two ways this one answers:
//! the words may simply not be there, and a query long enough to be a sentence
//! is asking about a subject whether or not one of its words happened to
//! appear. See [`worth_widening`] for when it is worth the index at all.
//!
//! # Why this is a module rather than a function in each caller
//!
//! Both surfaces widen: the terminal through `cctop_ui::worker`, the browser
//! through `cctop_serve::search`. This existed in both of them, twice over —
//! the same three constants, the same lazily-loaded model, the same
//! refresh-and-save, the same floor and limit — and the two copies agreed only
//! because a doc comment in each pointed at the other and said so. Nothing
//! failed when they drifted; the two surfaces just answered the same question
//! differently, which is the kind of disagreement nobody reports as a bug.
//!
//! What genuinely differed between them was the shape each collected into, and
//! that stayed with the callers, where it belongs.

use crate::session::search::Target;

/// A word count at which a query is a question, not a keyword.
///
/// Two words is `vast.ai nemotron` — a pair of names, which the literal tier is
/// better at. Four is "where did I price out GPUs", which it cannot answer at
/// all unless those words happen to be in the transcript.
const WORDS: usize = 4;

/// How close a chunk has to be to count as being about the query.
///
/// Cosine over mean-pooled static vectors does not reach the high scores a
/// contextual model would; on a real corpus a right answer sits around 0.35 to
/// 0.5 and unrelated text around 0.05. This sits below the answers and well
/// above the noise, and is a floor rather than a ranking — everything above it
/// is still ordered by score.
const FLOOR: f32 = 0.22;

/// Checked where it cannot drift from the value: a floor at or below the noise
/// admits every session, and one at or above the answers admits none. Both
/// bounds are from scoring a real corpus.
const _: () = assert!(FLOOR > 0.10 && FLOOR < 0.35);

/// At most this many sessions are added by the topical tier, so a vague query
/// widens the table rather than replacing it with everything on the machine.
const LIMIT: usize = 10;

/// Whether a query is worth going to the index for.
///
/// Two ways in, and they are different failures. An empty result means the
/// words are not in any transcript, which is when "what was it about" is the
/// only question left. A long query means it reads as a sentence rather than a
/// keyword, and those ask a topical question even when some literal match did
/// turn up — "where did I price out GPUs" matching a transcript that happens to
/// contain "where" is not an answer.
///
/// A short query that already matched is left alone deliberately: the literal
/// tier is the better answer for `vast.ai nemotron`, and widening it would add
/// vaguer sessions underneath a precise hit.
pub fn worth_widening(no_hits: bool, needle: &str) -> bool {
    no_hits || needle.split_whitespace().count() >= WORDS
}

/// One session the index thinks is about the query.
pub struct Match {
    /// [`Session::key`](crate::session::Session::key), so a caller can match it
    /// back to a row.
    pub key: String,
    /// The head of the chunk that matched, already labelled with the score —
    /// formatted here so the two surfaces cannot come to show it differently.
    pub snippet: String,
    /// The score itself, for a caller that reports confidence separately.
    pub score: f32,
}

/// The topical tier's state, built at most once per process that widens.
///
/// The model is a 30 MB read and the index a megabyte; both are loaded lazily
/// on the first widening query and then kept, since both surfaces outlive the
/// dozens of searches a session of looking produces.
#[derive(Default)]
pub struct Topics {
    model: Option<super::Model>,
    index: Option<super::index::Index>,
    /// Set once the model has been looked for and not found, so a machine
    /// without one does not re-check the filesystem on every keystroke.
    absent: bool,
}

impl Topics {
    /// The sessions `needle` is about, best first.
    ///
    /// Empty is the ordinary answer on a machine with no model, with a damaged
    /// one, or with nothing close enough to the query. The topical tier is an
    /// addition to the search and never a replacement for it, so every failure
    /// here leaves the caller's literal hits exactly as they were.
    pub fn widen(&mut self, needle: &str, targets: &[Target]) -> Vec<Match> {
        // Loaded through a separate step, and the fields touched directly after
        // it, because `index` is borrowed mutably while `model` is still held:
        // disjoint fields the borrow checker can see, where two methods on
        // `self` would be two borrows of the whole.
        self.load_model();
        let Some(model) = self.model.as_ref() else {
            return Vec::new();
        };

        let index = self.index.get_or_insert_with(|| {
            super::index::Index::load(&crate::config::EMBEDDING_INDEX_FILE).unwrap_or_default()
        });
        if index.refresh(model, targets) > 0 {
            // Best effort: an index that cannot be written is rebuilt next time,
            // which costs a second, and is not worth failing a search over.
            //
            // ponytail: the whole file is rewritten, and a *running* session's
            // transcript changes every turn — so searching while an agent works
            // re-embeds that session and rewrites everything else with it. At
            // the size this was measured (271 chunks over 85 sessions, 341 KB)
            // that is 9ms and beneath noticing; it is linear in the corpus, so
            // a machine holding thousands of sessions would want the index
            // appended to rather than replaced. See the ignored measurement in
            // `index.rs` for the numbers and how to take them again.
            let _ = index.save(&crate::config::EMBEDDING_INDEX_FILE);
        }

        let query = model.embed(&super::topic_of(needle));
        index
            .search(&query, FLOOR, LIMIT)
            .into_iter()
            .map(|(key, snippet, score)| Match {
                key,
                snippet: format!("~{:.0}% {snippet}", score * 100.0),
                score,
            })
            .collect()
    }

    /// Load the model on first use, and record it as absent if it will not come.
    ///
    /// A model that will not load is the same as no model here: the search must
    /// keep working, and `--fetch-search-model` verifies the load, so this is a
    /// damaged cache rather than a state worth reporting on a search.
    fn load_model(&mut self) {
        if self.absent || self.model.is_some() {
            return;
        }
        if !super::fetch::present() {
            self.absent = true;
            return;
        }
        match super::Model::load(&super::fetch::model_dir()) {
            Ok(m) => self.model = Some(m),
            Err(_) => self.absent = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two ways in, and the one deliberate way past: a short query that
    /// already found something is being answered by the tier that answered it.
    #[test]
    fn a_query_widens_when_it_found_nothing_or_reads_as_a_sentence() {
        assert!(worth_widening(true, "gpu"));
        assert!(worth_widening(false, "where did I price out GPUs"));
        assert!(!worth_widening(false, "vast.ai nemotron"));
    }

    /// A machine with no model answers the ordinary way — nothing — rather than
    /// failing the search that called it.
    #[test]
    fn a_missing_model_widens_by_nothing() {
        let mut topics = Topics {
            absent: true,
            ..Default::default()
        };
        assert!(topics.widen("anything at all", &[]).is_empty());
    }
}
