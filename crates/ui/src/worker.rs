//! The worker thread and the messages it exchanges with the UI.
//!
//! Every filesystem walk, transcript parse and process kill happens here rather
//! than on the thread that draws, so a slow scan can never stall a keystroke.
//! The seam is the two enums: the UI only ever knows the worker through a
//! [`Request`] it sends and a [`Response`] it reads back, which is why they and
//! the thread that serves them live together and away from `App`.

use super::*;
use cctop_core::loader::Loader;
use cctop_core::loader::Stats;
use std::sync::Arc;
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
    /// A window of the open conversation view. `host` is the machine to ask
    /// over ssh when the row is remote; `before` pages backwards through a
    /// session longer than one window.
    Chat {
        session: Box<Session>,
        host: Option<cctop_core::fleet::Host>,
        before: Option<usize>,
    },
    /// What the agents' own hooks have said about which processes they run
    /// under. Sent when it changes rather than with each walk: events arrive on
    /// the agents' clock and walks on the UI's, and the worker is where the
    /// [`Loader`] that needs it lives.
    HookClaims(HashMap<String, Vec<u32>>),
    /// Look for `query` inside every listed session's transcript.
    Scan {
        query: String,
        targets: Vec<cctop_core::session::search::Target>,
    },
    /// The git repositories under the home directory, for the launcher's
    /// directory field.
    ///
    /// Sent once per session rather than per keystroke, and answered from the
    /// gentle pool: it walks the home directory, which on a machine with a large
    /// `~/code` is thousands of `stat` calls, and nothing else should wait
    /// behind it.
    Repos,
    /// Run `cctop --update` on a remote machine, which the user has confirmed.
    UpdateRemote(cctop_core::fleet::Host),
    /// Something the launcher's directory field wants to know about an ssh
    /// host: a connection, a listing, the repositories, a directory's state.
    Location(super::location::Ask),
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
    /// The open session's extraction, shared with the cache. It arrives shared
    /// because the store keeps it; it is *taken* shared out of the run loop,
    /// which is the one place that copies — see there.
    Data(String, Arc<SessionData>),
    Quota(Box<Quota>),
    /// Repositories on disk, newest first. Sent when the directory field opens,
    /// and only when the field is open to be helped by them.
    Repos(Vec<std::path::PathBuf>),
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
        snapshot: cctop_core::fleet::Snapshot,
    },
    /// What a host's `cctop --version` said. Sent once per connection by its
    /// poll thread, and again after an update there, which is the one time
    /// the answer is known to have changed.
    RemoteVersion {
        host: String,
        probe: cctop_core::fleet::Probe,
    },
    /// How a confirmed `--update` on a host went.
    RemoteUpdated {
        host: String,
        result: Result<String, cctop_core::fleet::UpdateFailure>,
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
    /// The answer to a [`Request::Location`].
    Location(super::location::Answer),
    /// A conversation read. `key` and `before` echo the request so an answer
    /// for a view that has since moved on is recognised and dropped; the
    /// remote path's document deserialises into the same `Conversation` the
    /// local builder produces, which is why both come back in one arm.
    Chat {
        key: String,
        before: Option<usize>,
        result: Result<Box<cctop_core::chat::Conversation>, String>,
    },
}

/// Remembered scan results, by query and then by session. `None` is a
/// remembered *miss*, which is the answer worth caching most: a miss costs a
/// full read of the transcript, a hit usually stops early.
///
/// Keyed by query first so that a shorter query already answered can be found
/// again in one lookup — which is what lets a query being typed narrow what it
/// has to read. See [`narrowed`].
type ScanCache = HashMap<String, HashMap<String, Option<String>>>;

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
    model: Option<cctop_core::embed::Model>,
    index: Option<cctop_core::embed::index::Index>,
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
    targets: &[cctop_core::session::search::Target],
    hits: &mut HashMap<String, String>,
) {
    if !worth_widening(hits.is_empty(), needle) {
        return;
    }
    if topics.absent {
        return;
    }
    if topics.model.is_none() {
        if !cctop_core::embed::fetch::present() {
            topics.absent = true;
            return;
        }
        match cctop_core::embed::Model::load(&cctop_core::embed::fetch::model_dir()) {
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
        cctop_core::embed::index::Index::load(&cctop_core::config::EMBEDDING_INDEX_FILE)
            .unwrap_or_default()
    });
    if index.refresh(model, targets) > 0 {
        // Best effort: an index that cannot be written is rebuilt next time,
        // which costs a second, and is not worth failing a search over.
        let _ = index.save(&cctop_core::config::EMBEDDING_INDEX_FILE);
    }

    let query = model.embed(&cctop_core::embed::topic_of(needle));
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
    targets: &[cctop_core::session::search::Target],
    needle: &str,
) -> HashMap<String, String> {
    use rayon::prelude::*;
    // Parsed once rather than per session: every target is matched against the
    // same terms, and splitting and folding them again for each would be the
    // only allocation in the parallel hot path.
    let query = cctop_core::session::search::Query::parse(needle);
    let narrower = narrowed(cache, needle);
    let found: Vec<(&cctop_core::session::search::Target, Option<String>)> = targets
        .par_iter()
        .filter(|target| !skippable(&narrower, target))
        .map(|target| {
            let memo = (!target.running)
                .then(|| cache.get(needle).and_then(|seen| seen.get(&target.key)))
                .flatten();
            match memo {
                Some(remembered) => (target, remembered.clone()),
                None => (
                    target,
                    cctop_core::session::search::find_query(target, &query).map(|hit| hit.snippet),
                ),
            }
        })
        .collect();

    let mut hits = HashMap::new();
    for (target, snippet) in found {
        if let Some(snippet) = &snippet {
            hits.insert(target.key.clone(), snippet.clone());
        }
        if !target.running {
            let seen = cache.entry(needle.to_string()).or_default();
            seen.insert(target.key.clone(), snippet);
        }
    }
    if cache.values().map(HashMap::len).sum::<usize>() > MAX_SCAN_CACHE {
        cache.clear();
    }
    hits
}

/// The sessions a shorter query has already cleared, when one has.
///
/// A match for a term contains a match for every prefix of it: a transcript
/// holding `refactor` holds `refacto`. So a session asked about `refacto` and
/// found not to have it cannot have `refactor` either, and reading it again to
/// discover that once per keystroke is the entire cost of typing. Only the
/// sessions that *did* match the longest prefix with an answer are worth
/// reading — which is what turns typing a query into a scan of the handful of
/// sessions that could still match rather than of every session on the machine.
///
/// `None` when nothing shorter has been asked, and then everything is read.
fn narrowed<'c>(cache: &'c ScanCache, needle: &str) -> Option<&'c HashMap<String, Option<String>>> {
    // Longest first, and only on character boundaries so a multi-byte query
    // cannot be cut in half.
    needle
        .char_indices()
        .map(|(at, _)| at)
        .filter(|&at| at > 0)
        .rev()
        .find_map(|at| cache.get(&needle[..at]))
}

/// Whether a session can be passed over without reading it.
///
/// True only on a remembered *miss* for a prefix of this query. Running sessions
/// are never skippable: they are not cached at all, because their transcripts
/// grow, so an answer about one is only ever true of the moment it was taken.
fn skippable(
    narrower: &Option<&HashMap<String, Option<String>>>,
    target: &cctop_core::session::search::Target,
) -> bool {
    if target.running {
        return false;
    }
    match narrower {
        Some(seen) => seen.get(&target.key).is_some_and(Option::is_none),
        None => false,
    }
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
                    let stats = cctop_core::loader::compute_stats(&sessions);
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
                    let stats = cctop_core::loader::compute_stats(&live_rows);
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
                        let _ = tx.send(Response::Data(session.key(), data));
                    });
                }
                // Root reads other users' homes and never writes to them, and a
                // delete is a write: it removes their transcript and, for some
                // harnesses, rewrites a database they own. Refused here rather
                // than at the key, so no path to a delete can get round it.
                Request::Delete(session) if session.owner.is_some() => {
                    let owner = session.owner.as_deref().unwrap_or_default();
                    let result = Err(format!(
                        "{owner}'s session is read-only: cctop never changes another user's files"
                    ));
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
                Request::Delete(session) => {
                    let result = match session.provider {
                        Provider::Claude => cctop_core::session::claude::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Codex => cctop_core::session::codex::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Cursor => cctop_core::session::cursor::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Devin => cctop_core::session::devin::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Gemini => cctop_core::session::gemini::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::OpenCode => cctop_core::session::opencode::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Pi => cctop_core::session::pi::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Windsurf => cctop_core::session::windsurf::delete(&session)
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
                    let result = cctop_core::proc::terminate(pid);
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
                    let result = cctop_core::inject::send_line(pid, &text);
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
                        let found = cctop_core::insight::from_store(&sessions, store);
                        let all: Vec<&cctop_core::insight::Analysis> = found.iter().collect();
                        match which {
                            "optimize" => cctop_core::insight::optimize::report(&all),
                            _ => cctop_core::insight::compare::report(&all),
                        }
                    });
                    if tx.send(Response::Insight(text)).is_err() {
                        break;
                    }
                }
                Request::Chat {
                    session,
                    host,
                    before,
                } => {
                    // Off the request loop, for the same reason as
                    // `Request::Data`: building the conversation re-reads a
                    // whole transcript, and a remote one waits on another
                    // machine entirely. Everything queued behind it on this
                    // thread — refreshes, searches, the next keystroke's work —
                    // would wait with it. The view drops an answer for a
                    // selection it has already left, so a late one is safe.
                    let tx = tx.clone();
                    loader.gently_spawn(move || {
                        let result = match host {
                            // The transcript lives on the far side — ask the
                            // cctop there for the same document the local build
                            // would make, rather than parsing a local file that
                            // happens to share the path.
                            Some(host) => {
                                let marker = before.map(|b| b.to_string());
                                let mut args = vec!["--chat", session.session_id.as_str()];
                                if let Some(marker) = marker.as_deref() {
                                    args.extend(["--before", marker]);
                                }
                                host.run(&args).and_then(|json| {
                                    serde_json::from_str(&json).map_err(|e| {
                                        format!(
                                            "{} returned an unreadable conversation: {e}",
                                            host.target
                                        )
                                    })
                                })
                            }
                            None => Ok(cctop_core::chat::build(&session, before)),
                        };
                        let _ = tx.send(Response::Chat {
                            key: session.key(),
                            before,
                            result: result.map(Box::new),
                        });
                    });
                }
                Request::Scan { query, targets } => {
                    let needle = query.to_ascii_lowercase();
                    let mut hits = loader.gently(|| scan(&mut scans, &targets, &needle));
                    loader.gently(|| topical(&mut topics, &needle, &targets, &mut hits));
                    if tx.send(Response::Scanned { query, hits }).is_err() {
                        break;
                    }
                }
                Request::Repos => {
                    // On the gentle pool rather than inline: the walk is tens of
                    // milliseconds measured, but it is a `stat` per directory and
                    // this loop is what every refresh and every keystroke's work
                    // queues behind.
                    let home = cctop_core::config::HOME.clone();
                    let found = loader.gently(move || super::dirs::repos_under(&home));
                    if tx.send(Response::Repos(found)).is_err() {
                        break;
                    }
                }
                // A thread of its own: the far side downloads a release and
                // swaps its binary, which takes seconds on a good link, and the
                // worker is what answers the keyboard's refresh meanwhile.
                Request::UpdateRemote(host) => {
                    let tx = tx.clone();
                    std::thread::spawn(move || {
                        let result = host.update();
                        let updated = result.is_ok();
                        let _ = tx.send(Response::RemoteUpdated {
                            host: host.target.clone(),
                            result,
                        });
                        if updated {
                            let _ = tx.send(Response::RemoteVersion {
                                host: host.target.clone(),
                                probe: host.probe(),
                            });
                        }
                    });
                }
                // A thread each: a connect can take its whole deadline and a
                // listing its timeout, and a host that is not answering must
                // hold up neither the next keystroke's work nor another host.
                Request::Location(ask) => {
                    let tx = tx.clone();
                    std::thread::spawn(move || {
                        let connect =
                            |host: &str| cctop_core::ssh_master::connect(host).map(|_| ());
                        let answer =
                            super::location::answer(ask, &cctop_core::ssh_master::Ssh, &connect);
                        let _ = tx.send(Response::Location(answer));
                    });
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

    /// A transcript holding `refactor` holds `refacto`, so a query being typed
    /// only has to read the sessions that could still match. This is what makes
    /// typing cheap rather than one full pass over every transcript per letter.
    #[test]
    fn a_query_is_only_read_where_a_shorter_one_still_could_match() {
        use std::io::Write;
        let file = |name: &str, body: &str| {
            let path =
                std::env::temp_dir().join(format!("cctop-narrow-{name}-{}", std::process::id()));
            std::fs::File::create(&path)
                .unwrap()
                .write_all(body.as_bytes())
                .unwrap();
            path
        };
        let target = |name: &str, path: &std::path::Path, running: bool| {
            let mut s = cctop_core::session::Session::new(
                cctop_core::pricing::Provider::Claude,
                name.to_string(),
            );
            s.data_file = Some(path.to_path_buf());
            let mut t = cctop_core::session::search::Target::of(&s);
            t.running = running;
            t
        };

        let yes = file("yes", "{\"t\":\"a refactor landed here\"}\n");
        let no = file("no", "{\"t\":\"nothing relevant at all\"}\n");
        // A live session whose transcript grows, so a miss said about it now is
        // not a miss about it after the next line.
        let live = file("live", "{\"t\":\"nothing relevant at all\"}\n");
        let targets = vec![
            target("yes", &yes, false),
            target("no", &no, false),
            target("live", &live, true),
        ];

        let mut cache = ScanCache::new();
        // The short query clears everything but the session that matched.
        let short = scan(&mut cache, &targets, "refac");
        assert_eq!(short.keys().cloned().collect::<Vec<_>>(), ["claude:yes"]);

        // The long one reads only what the short one could not rule out, and
        // still answers the same.
        let long = scan(&mut cache, &targets, "refactor");
        assert_eq!(long.keys().cloned().collect::<Vec<_>>(), ["claude:yes"]);

        // And a session the prefix cleared is not read again, because its answer
        // is remembered.
        let again = scan(&mut cache, &targets, "refactor");
        assert_eq!(again.keys().cloned().collect::<Vec<_>>(), ["claude:yes"]);

        // A prefix that has never been asked reads everything, so the answer
        // does not depend on the order queries arrive in.
        let mut cold = ScanCache::new();
        assert_eq!(
            scan(&mut cold, &targets, "refactor")
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            ["claude:yes"]
        );
        assert_eq!(scan(&mut cold, &targets, "nomatch").len(), 0);

        // The live session is the exception, and it is the reason a miss about a
        // running transcript is not remembered: its file gains the term, and the
        // next query has to see it rather than trusting the earlier answer.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&live)
            .unwrap()
            .write_all(b"{\"t\":\"and now a refactor landed\"}\n")
            .unwrap();
        let grown = scan(&mut cache, &targets, "refactor");
        assert!(
            grown.contains_key("claude:live"),
            "a running session's transcript grew and was not looked at again"
        );

        for path in [yes, no, live] {
            let _ = std::fs::remove_file(path);
        }
    }

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
