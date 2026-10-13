//! The threads that keep what the routes read fresh.
//!
//! Each route answers from the shared snapshot rather than walking the disk on
//! request, so this is where the walking happens: the local refresher, one
//! poller per `--host`, the quota and the providers' status pages. Split out of
//! `lib.rs` so the router reads as a router.

use crate::{FULL_WALK, QUOTA_INTERVAL_SECS, QUOTA_TICK, Remotes, Shared, Snapshot, quota};
use cctop_core::fleet;
use cctop_core::loader::Loader;
use cctop_core::pricing::Plan;
use cctop_core::session::Session;
use cctop_core::watch::Watch;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Poll one host forever, publishing whatever it last said.
///
/// A failed poll keeps the previous rows and records why: the ssh connection
/// dropping has not stopped those agents, and blanking the machine would make
/// the totals look complete when they are not — the same call [`cctop_ui`]
/// makes for the same reason.
pub(crate) fn spawn_host_poller(host: fleet::Host, remotes: Arc<Mutex<Remotes>>) {
    std::thread::spawn(move || {
        loop {
            remember(&remotes, &host.target, host.poll());
            std::thread::sleep(fleet::POLL);
        }
    });
}

/// File one machine's poll: its rows, or why it could not be read beside the
/// rows it last gave.
pub(crate) fn remember(remotes: &Mutex<Remotes>, host: &str, snapshot: fleet::Snapshot) {
    if let Ok(mut remotes) = remotes.lock() {
        match snapshot {
            fleet::Snapshot::Rows(rows) => {
                remotes.errors.remove(host);
                remotes.rows.insert(host.to_string(), rows);
            }
            fleet::Snapshot::Failed(why) => {
                remotes.errors.insert(host.to_string(), why);
            }
        }
    }
}

/// Poll each provider's usage endpoint on the slow cadence they demand,
/// storing the rendered `/api/quota` document each time one answers.
///
/// Each provider is paced by its own last outcome — a throttled one backs off
/// without stalling the other — which is the same arithmetic the dashboard's
/// poller runs (`spawn_quota_poller` in `crates/ui/src/runloop.rs`), minus the burn
/// log: a serve records nothing, it only reports.
pub(crate) fn spawn_quota_poller(shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let mut claude: Vec<cctop_core::quota::ProfileQuota> = Vec::new();
        let mut codex: Vec<cctop_core::quota::ProfileQuota> = Vec::new();
        let (mut claude_due, mut codex_due) = (Instant::now(), Instant::now());
        loop {
            let now = Instant::now();
            let mut changed = false;
            // Each profile is its own account with its own limits, so each is
            // asked separately. They share one due time: the interval exists
            // to be polite to the provider, and a machine with two logins is
            // not entitled to twice the requests.
            if now >= claude_due {
                claude = cctop_core::config::accounts_for(cctop_core::pricing::Provider::Claude)
                    .iter()
                    .map(|profile| cctop_core::quota::ProfileQuota {
                        profile: profile.name.clone(),
                        status: cctop_core::quota::fetch_claude(profile),
                        source: profile.source,
                    })
                    .collect();
                // Paced by whichever account is most throttled, so backing off
                // for one does not keep asking on behalf of another.
                let delay = claude
                    .iter()
                    .map(|q| q.status.retry_delay_secs(QUOTA_INTERVAL_SECS))
                    .max()
                    .unwrap_or(QUOTA_INTERVAL_SECS);
                claude_due = now + Duration::from_secs(delay);
                changed = true;
            }
            if now >= codex_due {
                codex = cctop_core::config::accounts_for(cctop_core::pricing::Provider::Codex)
                    .iter()
                    .map(|profile| cctop_core::quota::ProfileQuota {
                        profile: profile.name.clone(),
                        status: cctop_core::quota::fetch_codex(profile),
                        source: profile.source,
                    })
                    .collect();
                let delay = codex
                    .iter()
                    .map(|q| q.status.retry_delay_secs(QUOTA_INTERVAL_SECS))
                    .max()
                    .unwrap_or(QUOTA_INTERVAL_SECS);
                codex_due = now + Duration::from_secs(delay);
                changed = true;
            }
            if changed && let Ok(mut slot) = shared.quota.lock() {
                *slot = serde_json::to_string(&quota::document(&cctop_core::quota::Quota {
                    fetched: true,
                    claude: claude.clone(),
                    codex: codex.clone(),
                }))
                .unwrap_or_else(|_| quota::EMPTY.to_string());
            }
            std::thread::sleep(QUOTA_TICK);
        }
    });
}

/// Run core's status-page poller into [`Shared::provider_status`].
///
/// The same loop and pacing as the dashboard's footer, so a serve never asks
/// more often than the TUI does — and with no recheck flag anybody can set:
/// the browser re-reads memory on its own timer and cannot make cctop poll.
pub(crate) fn spawn_provider_status_poller(shared: Arc<Shared>) {
    cctop_core::provider_status::spawn_poller(Arc::default(), move |page, status| {
        // A debug build can stand a fixture in for the vendor; the poller must
        // not overwrite it two minutes later.
        #[cfg(feature = "debug")]
        if debug::provider_status_pinned() {
            return true;
        }
        lock_provider_status(&shared).set(page, status);
        true
    });
}

/// The status pages' slot, recovered rather than refused if a writer panicked:
/// it holds plain data, and the worst a half-finished write leaves is one
/// page's answer a poll out of date.
pub(crate) fn lock_provider_status(
    shared: &Shared,
) -> std::sync::MutexGuard<'_, cctop_core::provider_status::Status> {
    shared
        .provider_status
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Walk, refresh, and publish, forever.
///
/// The cadence mirrors the UI's: a light refresh every `delay` that re-reads
/// only rows that can still change, and a full walk when [`Watch`] says a
/// session file appeared or [`FULL_WALK`] passes without it saying anything.
/// A walk on every tick would re-read every provider directory on disk to
/// discover, almost always, nothing.
pub(crate) fn spawn_refresher(
    shared: Arc<Shared>,
    remotes: Arc<Mutex<Remotes>>,
    plan: Plan,
    delay: Duration,
) {
    std::thread::spawn(move || {
        // Blocking, once, before the first snapshot: a dashboard whose first
        // frame shows every cost as zero is worse than one that appears a
        // second later with the truth.
        cctop_core::pricing::refresh_pricing_blocking();

        let mut loader = Loader::new();
        let watch = Watch::start();
        // A hook listener of our own: agents report to every socket in the
        // directory, so a standalone serve sees a permission prompt the moment
        // it goes up whether or not a dashboard process is running.
        let listener = cctop_core::hook::Listener::start();
        let mut reports = cctop_core::hook::Reports::new();
        // Reading an agent's screen needs somewhere to read it from — a shim
        // socket or an rmux pane — which [`Peek`] finds once and reuses. The
        // same opt-in as the TUI's: the phrases are each agent's own UI, not a
        // contract, so they are only looked for when the file says so.
        let mut peek = cctop_core::peek::Peek::new();
        let read_screen = cctop_core::settings::Settings::load().read_screen == Some(true);
        loader.set_hook_claims(reports.claims.clone());
        // The YOLO switch, read on every pass. Only a serve that may act
        // competes to answer for it — see [`cctop_core::yolo`] for why only one
        // cctop on the machine ever presses.
        let mut yolo = cctop_core::yolo::Auto::new(shared.actions);
        let mut rows = loader.load(plan);
        stamp(&mut rows, &reports, &mut peek, read_screen);
        yolo.tick(&mut rows, &reports);
        let mut walked = Instant::now();
        let mut version = 0u64;
        publish(
            &shared,
            &remotes,
            &rows,
            plan,
            loader.store(),
            &mut version,
            None,
        );

        loop {
            std::thread::sleep(delay);

            // Hook events first: a `SessionStart` is a session to walk for now
            // rather than at the next poll, and a moved pid claim belongs in
            // the file the loader resolves pids by.
            let mut appeared = false;
            for event in listener.as_ref().map(|l| l.drain()).unwrap_or_default() {
                let (lifecycle, moved) = reports.observe(&event);
                appeared |= lifecycle;
                if moved {
                    cctop_core::hook::save_claims(&reports.claims);
                    loader.set_hook_claims(reports.claims.clone());
                }
            }
            // A prompt auto mode never answered promotes itself, no event
            // needed — the grace running out is the news.
            reports.promote_matured();
            appeared |= watch.as_ref().is_some_and(|w| {
                w.took_structural_change()
                    || w.awaiting_discovery(|path| {
                        rows.iter().any(|s| s.data_file.as_deref() == Some(path))
                    })
            });
            // With no watcher there is nothing to tell us a session appeared,
            // so the timer is the only trigger and has to carry the whole job.
            if appeared || walked.elapsed() >= FULL_WALK {
                rows = loader.load(plan);
                walked = Instant::now();
            } else {
                loader.refresh_live(plan, &mut rows);
            }
            stamp(&mut rows, &reports, &mut peek, read_screen);
            // After the stamp, which is what says a prompt is up: an answer
            // pressed here is a refresh tick behind the prompt at most.
            yolo.tick(&mut rows, &reports);
            cctop_core::telemetry::watched(&rows);
            publish(
                &shared,
                &remotes,
                &rows,
                plan,
                loader.store(),
                &mut version,
                None,
            );
        }
    });
}

/// Stamp each row with what reports and screens say about it.
///
/// The same [`Session::apply_reports`] the TUI's `App::apply_reports` calls,
/// for the same reason: whatever the rows end up believing is decided once.
/// Rows are rebuilt by the walk and re-derived by the refresh, so the stamp
/// is re-applied every pass — a permission prompt survives the rebuild
/// precisely because the report that raised it lives in `reports`, not the
/// row.
pub(crate) fn stamp(
    rows: &mut [Session],
    reports: &cctop_core::hook::Reports,
    peek: &mut cctop_core::peek::Peek,
    read_screen: bool,
) {
    for session in rows.iter_mut() {
        // A screen read is worth its cost only for a live local agent — a
        // stopped one has no screen, and the transcript already said the rest.
        let screened = match (read_screen, session.is_running()) {
            // `provider`, not `harness`: the column is the display name
            // ("ClaudeCode"), and the recognizer's ids are the lowercase ones.
            (true, true) => session
                .root_pid()
                .and_then(|pid| peek.read(session.provider.as_str(), pid)),
            _ => None,
        };
        session.apply_reports(reports.report(&session.session_id), screened.as_ref());
        reports.stamp_sandbox(session);
    }
}

/// Render one snapshot and wake everyone waiting on it.
/// `loader` is the refresh thread's own, and has to be: the document is built
/// by reading each row's cached extraction, and a fresh loader would have an
/// empty cache and re-parse every transcript on disk once per refresh.
pub(crate) fn publish(
    shared: &Shared,
    remotes: &Mutex<Remotes>,
    local: &[Session],
    plan: Plan,
    store: &cctop_core::cache::Store,
    version: &mut u64,
    usage: Option<&cctop_core::quota::Quota>,
) {
    let (mut sessions, host_errors) = match remotes.lock() {
        Ok(remotes) => {
            let mut merged = local.to_vec();
            for rows in remotes.rows.values() {
                merged.extend(rows.iter().cloned());
            }
            let mut errors: Vec<(String, String)> = remotes
                .errors
                .iter()
                .map(|(h, why)| (h.clone(), why.clone()))
                .collect();
            errors.sort();
            (merged, errors)
        }
        Err(_) => (local.to_vec(), Vec::new()),
    };
    // Newest first, so the page never has to decide what order means. Remote
    // rows arrive on their own schedule and would otherwise land wherever the
    // merge happened to put them.
    sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));

    // The same document `--json` prints and `--host` parses, from the same
    // builder: a browser being shown different figures than the terminal is a
    // bug nobody would think to look for. See [`cctop_core::json::sessions`].
    let document = cctop_core::json::sessions(&sessions, plan, store);
    // Serialised a row at a time and joined, which is byte for byte what the
    // whole array would serialise to — so the dashboard's document is
    // unchanged, and a session page's stream can send its one row of it.
    let rows: Vec<(String, String)> = document
        .iter()
        .filter_map(|row| {
            let value = serde_json::to_value(row).ok()?;
            let id = value.get("session_id")?.as_str()?.to_string();
            Some((id, value.to_string()))
        })
        .collect();
    let json = format!(
        "[{}]",
        rows.iter()
            .map(|(_, r)| r.as_str())
            .collect::<Vec<_>>()
            .join(",")
    );

    // Written through the same lock as the snapshot, so the page's quota panel
    // can never be newer than the rows it sits beside.
    if let Some(usage) = usage
        && let Ok(mut slot) = shared.quota.lock()
    {
        *slot = serde_json::to_string(&quota::document(usage))
            .unwrap_or_else(|_| quota::EMPTY.to_string());
    }

    *version += 1;
    let snapshot = Arc::new(Snapshot {
        version: *version,
        json,
        rows,
        sessions,
        host_errors,
    });
    cctop_core::elog::event(
        "scan",
        "snapshot",
        serde_json::json!({
            "version": snapshot.version,
            "sessions": snapshot.sessions.len(),
            "running": snapshot.sessions.iter().filter(|s| s.is_running()).count(),
            "remote_hosts": snapshot.host_errors.len(),
        }),
    );
    // The crossing is read off the pair of snapshots as they meet, which is the
    // one place both feeds — this server's own refresher and the dashboard's —
    // pass through. A webhook asked for here therefore covers both.
    let mut crossed = Vec::new();
    if let Ok(mut latest) = shared.latest.lock() {
        if let Some(hook) = &shared.notify {
            crossed = hook
                .crossed(&latest.sessions, &snapshot.sessions)
                .into_iter()
                .map(|(event, s)| (event, s.clone()))
                .collect();
        }
        *latest = snapshot;
    }
    shared.updated.notify_all();
    if let Some(hook) = &shared.notify {
        for (event, session) in crossed {
            hook.post(event, &session);
        }
    }
}
