//! The worker thread and the messages it exchanges with the UI.
//!
//! Every filesystem walk, transcript parse and process kill happens here rather
//! than on the thread that draws, so a slow scan can never stall a keystroke.
//! The seam is the two enums: the UI only ever knows the worker through a
//! [`Request`] it sends and a [`Response`] it reads back, which is why they and
//! the thread that serves them live together and away from `App`.

use super::*;
use crate::loader::Loader;
use crate::loader::Stats;
use std::sync::mpsc::Receiver;

pub(super) enum Request {
    Refresh,
    /// Update the running sessions without re-walking every provider directory.
    RefreshLive,
    /// Extract full data for one session, to populate the bottom panels.
    Data(Box<Session>),
    Delete(Box<Session>),
    Terminate {
        session_key: String,
        pid: u32,
    },
    /// Type a line into the terminal hosting a live session.
    SendKeys {
        pid: u32,
        text: String,
    },
    /// Build an `optimize` or `compare` report. Slow — it re-parses every
    /// transcript, because the tool calls it reasons about are never cached —
    /// so it goes to the worker like any other walk.
    Insight {
        which: &'static str,
    },
    /// What the agents' own hooks have said about which processes they run
    /// under. Sent when it changes rather than with each walk: events arrive on
    /// the agents' clock and walks on the UI's, and the worker is where the
    /// [`Loader`] that needs it lives.
    HookClaims(HashMap<String, Vec<u32>>),
    /// Look for `query` inside every listed session's transcript.
    Scan {
        query: String,
        targets: Vec<crate::session::search::Target>,
    },
    Shutdown,
}

pub(super) enum Response {
    /// Cheap discovery result, shown before transcript extraction completes.
    Discovered(Vec<Session>),
    /// One row whose transcript has finished loading.
    Annotated(Box<Session>),
    Sessions(Box<(Vec<Session>, Stats)>),
    /// Only the rows that moved during a light refresh, plus recomputed totals.
    /// Shipping these instead of the whole table is the point of the light path:
    /// copying thousands of rows back every couple of seconds is the cost being
    /// avoided.
    LiveRows(Box<(Vec<Session>, Stats)>),
    Data(String, Box<SessionData>),
    Quota(Box<Quota>),
    /// Pricing landed, so cached costs are stale and a reload is due.
    PricingReady,
    /// A newer release exists. Reported once; cctop never updates itself.
    UpdateAvailable(String),
    Terminated {
        session_key: String,
        result: Result<(), String>,
    },
    Deleted {
        session_key: String,
        result: Result<(), String>,
    },
    KeysSent {
        result: Result<(), String>,
    },
    /// One remote machine's snapshot, or why it could not be read.
    Remote {
        host: String,
        snapshot: crate::fleet::Snapshot,
    },
    /// A finished transcript scan: session key -> the text around its match.
    /// The query comes back with it, because the user has usually typed more by
    /// the time a scan over thousands of transcripts lands.
    Scanned {
        query: String,
        hits: HashMap<String, String>,
    },
    /// A finished report, already laid out as text.
    Insight(String),
}

/// Remembered scan results, keyed by session and query. `None` is a remembered
/// *miss*, which is the answer worth caching most: a miss costs a full read of
/// the transcript, a hit usually stops early.
type ScanCache = HashMap<(String, String), Option<String>>;

/// Entries kept before the scan cache is dropped wholesale.
///
/// Reached only by someone who has run many distinct queries over many
/// sessions; forgetting everything then costs one re-scan rather than the
/// bookkeeping an eviction policy would need for a cache this cheap to refill.
const MAX_SCAN_CACHE: usize = 20_000;

/// Search every target's transcript for `needle`, in parallel.
///
/// Running sessions are never cached: their transcripts grow, so today's "not
/// found" is not tomorrow's, and the one case where a stale answer is most
/// visible is the session the user is watching right now.
/// A query with this many words is a sentence rather than a keyword, and is
/// asking a topical question even when the literal search found something.
///
/// Two words is `vast.ai nemotron` — a pair of names, which the literal tier is
/// better at. Four is "where did I price out GPUs", which it cannot answer at
/// all unless those words happen to be in the transcript.
const TOPICAL_WORDS: usize = 4;

/// How close a chunk has to be to count as being about the query.
///
/// Cosine over mean-pooled static vectors does not reach the high scores a
/// contextual model would; on a real corpus a right answer sits around 0.35 to
/// 0.5 and unrelated text around 0.05. This sits below the answers and well
/// above the noise, and is a floor rather than a ranking — everything above it
/// is still ordered by score.
const TOPICAL_FLOOR: f32 = 0.22;

/// Checked where it cannot drift from the value: a floor at or below the noise
/// admits every session, and one at or above the answers admits none. Both
/// bounds are from scoring a real corpus.
const _: () = assert!(TOPICAL_FLOOR > 0.10 && TOPICAL_FLOOR < 0.35);

/// At most this many sessions are added by the topical tier, so a vague query
/// widens the table rather than replacing it with everything on the machine.
const TOPICAL_LIMIT: usize = 10;

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
fn worth_widening(no_hits: bool, needle: &str) -> bool {
    no_hits || needle.split_whitespace().count() >= TOPICAL_WORDS
}

/// The topical tier's state, built at most once per worker.
#[derive(Default)]
struct Topics {
    model: Option<crate::embed::Model>,
    index: Option<crate::embed::index::Index>,
    /// Set once the model has been looked for and not found, so a machine
    /// without one does not re-check the filesystem on every keystroke.
    absent: bool,
}

/// Widen `hits` with sessions that are *about* the query rather than containing
/// it.
///
/// Runs when the literal search found nothing, or when the query reads like a
/// question rather than a keyword. Those are the two cases where "the words are
/// not in the transcript" is the user's problem rather than their intent, and
/// they are also the only cases worth the index: a two-word query that already
/// matched is being answered well by the tier that answered it.
///
/// Anything missing — no model, no readable transcripts — leaves `hits` exactly
/// as the literal search left them. The topical tier is an addition to the
/// search, never a replacement for it, so it can be absent without the search
/// being broken.
fn topical(
    topics: &mut Topics,
    needle: &str,
    targets: &[crate::session::search::Target],
    hits: &mut HashMap<String, String>,
) {
    if !worth_widening(hits.is_empty(), needle) {
        return;
    }
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
                // A model that will not load is the same as no model here: the
                // search must keep working, and `--fetch-search-model` verifies
                // the load, so this is the unusual case of a damaged cache.
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
        // A session the literal search already found keeps the snippet that
        // shows the words it matched; that is the more precise answer, and
        // overwriting it would replace evidence with a guess.
        hits.entry(key)
            .or_insert_with(|| format!("~{:.0}% {snippet}", score * 100.0));
    }
}

fn scan(
    cache: &mut ScanCache,
    targets: &[crate::session::search::Target],
    needle: &str,
) -> HashMap<String, String> {
    use rayon::prelude::*;
    // Parsed once rather than per session: every target is matched against the
    // same terms, and splitting and folding them again for each would be the
    // only allocation in the parallel hot path.
    let query = crate::session::search::Query::parse(needle);
    let found: Vec<(&crate::session::search::Target, Option<String>)> = targets
        .par_iter()
        .map(|target| {
            let memo = (!target.running)
                .then(|| cache.get(&(target.key.clone(), needle.to_string())))
                .flatten();
            match memo {
                Some(remembered) => (target, remembered.clone()),
                None => (
                    target,
                    crate::session::search::find_query(target, &query).map(|hit| hit.snippet),
                ),
            }
        })
        .collect();

    if cache.len() + found.len() > MAX_SCAN_CACHE {
        cache.clear();
    }
    let mut hits = HashMap::new();
    for (target, snippet) in found {
        if !target.running {
            cache.insert((target.key.clone(), needle.to_string()), snippet.clone());
        }
        if let Some(snippet) = snippet {
            hits.insert(target.key.clone(), snippet);
        }
    }
    hits
}

/// Owns the `Loader` and does all filesystem and parsing work off the UI thread,
/// so a slow scan can never stall input or rendering.
pub(super) fn spawn_worker(
    plan: Plan,
    rx: Receiver<Request>,
    tx: Sender<Response>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut loader = Loader::new();
        let mut sent_initial_discovery = false;
        // The last full walk's rows, kept so a light refresh has something to
        // update in place.
        let mut live_rows: Vec<Session> = Vec::new();
        // What earlier transcript scans found, so refining a query re-reads only
        // what it has to. See `scan`.
        let mut scans: ScanCache = HashMap::new();
        // The topical tier's model and index, loaded at most once and only if a
        // query ever asks for them. See `topical`.
        let mut topics = Topics::default();
        while let Ok(req) = rx.recv() {
            match req {
                Request::Refresh => {
                    // Row-by-row publishing exists so the first table isn't
                    // withheld for the slowest transcript. Later refreshes end
                    // with a `Sessions` payload that replaces everything anyway,
                    // so streaming them too only buys a redundant repaint — at the
                    // cost of one message and one table scan per session, per
                    // refresh, forever.
                    let first_load = !sent_initial_discovery;
                    sent_initial_discovery = true;
                    let sessions = loader.load_progressive(
                        plan,
                        // Only the first table has someone waiting for it.
                        first_load,
                        |sessions| {
                            if first_load {
                                let _ = tx.send(Response::Discovered(sessions.to_vec()));
                            }
                        },
                        |session| {
                            if first_load {
                                let _ = tx.send(Response::Annotated(Box::new(session.clone())));
                            }
                        },
                    );
                    let stats = crate::loader::compute_stats(&sessions);
                    // The light path needs its own copy to carry forward; one clone
                    // per full walk replaces one per refresh.
                    live_rows = sessions.clone();
                    if tx
                        .send(Response::Sessions(Box::new((sessions, stats))))
                        .is_err()
                    {
                        break;
                    }
                }
                Request::HookClaims(claims) => {
                    loader.set_hook_claims(claims);
                }
                Request::RefreshLive => {
                    if live_rows.is_empty() {
                        // Nothing walked yet, so there is nothing to update.
                        continue;
                    }
                    let moved = loader.refresh_live(plan, &mut live_rows);
                    let stats = crate::loader::compute_stats(&live_rows);
                    if tx
                        .send(Response::LiveRows(Box::new((moved, stats))))
                        .is_err()
                    {
                        break;
                    }
                }
                Request::Data(session) => {
                    // Off the request loop: a big transcript takes seconds to
                    // re-read, and everything queued behind it on this thread —
                    // refreshes, searches, the next keystroke's work — waits
                    // with it. The UI drops responses for a selection it has
                    // already left, so an answer landing late is safe.
                    let store = loader.store_shared();
                    let tx = tx.clone();
                    loader.gently_spawn(move || {
                        let data = store.session_data_fresh(&session);
                        let _ = tx.send(Response::Data(session.key(), Box::new(data)));
                    });
                }
                Request::Delete(session) => {
                    let result = match session.provider {
                        Provider::Claude => crate::session::claude::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Codex => crate::session::codex::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Cursor => crate::session::cursor::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Devin => crate::session::devin::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Gemini => crate::session::gemini::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::OpenCode => crate::session::opencode::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Pi => {
                            crate::session::pi::delete(&session).map_err(|error| error.to_string())
                        }
                        Provider::Windsurf => crate::session::windsurf::delete(&session)
                            .map_err(|error| error.to_string()),
                    };
                    if result.is_ok() {
                        loader.store().evict(&session);
                    }
                    if tx
                        .send(Response::Deleted {
                            session_key: session.key(),
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Request::Terminate { session_key, pid } => {
                    let result = crate::proc::terminate(pid);
                    if tx
                        .send(Response::Terminated {
                            session_key,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Request::SendKeys { pid, text } => {
                    let result = crate::inject::send_line(pid, &text);
                    if tx.send(Response::KeysSent { result }).is_err() {
                        break;
                    }
                }
                Request::Insight { which } => {
                    // On the gentle pool: nobody is waiting on the table, and a
                    // full re-parse would otherwise take every core away from
                    // the session the user is watching.
                    // Walked first, because the walk needs the loader mutably
                    // and the analysis only needs its store. Warm, so this is
                    // the cheap half.
                    let sessions = loader.load(plan);
                    let store = loader.store();
                    let text = loader.gently(|| {
                        let found = crate::insight::from_store(&sessions, store);
                        let all: Vec<&crate::insight::Analysis> = found.iter().collect();
                        match which {
                            "optimize" => crate::insight::optimize::report(&all),
                            _ => crate::insight::compare::report(&all),
                        }
                    });
                    if tx.send(Response::Insight(text)).is_err() {
                        break;
                    }
                }
                Request::Scan { query, targets } => {
                    let needle = query.to_ascii_lowercase();
                    let mut hits = loader.gently(|| scan(&mut scans, &targets, &needle));
                    loader.gently(|| topical(&mut topics, &needle, &targets, &mut hits));
                    if tx.send(Response::Scanned { query, hits }).is_err() {
                        break;
                    }
                }
                Request::Shutdown => break,
            }
        }
        loader.store().save();
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two doors into the topical tier, and the case that must stay shut.
    #[test]
    fn only_an_empty_or_sentence_like_query_reaches_the_index() {
        // Nothing found: the index is the only thing left to ask.
        assert!(worth_widening(true, "nemotron"));
        assert!(worth_widening(true, "a"));

        // Found something, and the query is a keyword: the literal tier already
        // gave the better answer.
        assert!(!worth_widening(false, "nemotron"));
        assert!(!worth_widening(false, "vast.ai nemotron"));
        assert!(!worth_widening(false, "musl static build"));

        // Found something, but the query is a sentence: it is asking about a
        // subject, and a stray match on "where" is not that.
        assert!(worth_widening(false, "where did I price out GPUs"));
        assert!(worth_widening(
            false,
            "the conversation about running a language model"
        ));
    }
}
