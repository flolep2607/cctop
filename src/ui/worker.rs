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
fn scan(
    cache: &mut ScanCache,
    targets: &[crate::session::search::Target],
    needle: &str,
) -> HashMap<String, String> {
    use rayon::prelude::*;
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
                    crate::session::search::find(target, needle).map(|hit| hit.snippet),
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
                    // The open panels are the one view where staleness shows, so
                    // this path never accepts a backed-off entry.
                    let data = loader.store().session_data_fresh(&session);
                    if tx
                        .send(Response::Data(session.key(), Box::new(data)))
                        .is_err()
                    {
                        break;
                    }
                }
                Request::Delete(session) => {
                    let result = match session.provider {
                        Provider::Claude => crate::session::claude::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Codex => crate::session::codex::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Cursor => crate::session::cursor::delete(&session)
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
                    let hits = loader.gently(|| scan(&mut scans, &targets, &needle));
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
