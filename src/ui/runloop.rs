//! Process entry, terminal setup, and the loop that turns events into redraws.
//!
//! This is the only part of the UI that owns the terminal itself and the
//! threads feeding it — quota, fleet hosts, the release check. `App` is state
//! plus the methods that change it; nothing in it knows about `crossterm`,
//! panics, or how often a walk is due. Keeping the loop here means the state
//! can be built and driven by a test without a terminal anywhere near it.

use super::worker::{Response, spawn_worker};
use super::*;
use crate::cli::Args;
use ratatui::crossterm::cursor::Show;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, Event,
};
use ratatui::crossterm::execute;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use crate::quota::INTERVAL_SECS as QUOTA_INTERVAL_SECS;

/// How often the poller wakes to see whether any provider is due.
const QUOTA_TICK: Duration = Duration::from_secs(10);

/// Gap between full directory walks.
///
/// Only a walk can notice a session that didn't exist before, and it costs one
/// `stat` per transcript ever recorded — thousands of them, nearly all belonging
/// to sessions that ended long ago. A filesystem watch reports creations and
/// removals as they happen, so this is the safety net for whatever the watch
/// misses (or for when no watch could be established at all) rather than the way
/// new sessions are normally found. `r` still forces one immediately.
const FULL_WALK_INTERVAL: Duration = Duration::from_secs(60);

/// Gap between walks while a created file has yet to become a session.
///
/// Short, because this is the window in which a session the user just started is
/// missing from the table; bounded, because the walk is the expensive one and a
/// file may sit there for a while before the model first answers.
const PENDING_WALK_INTERVAL: Duration = Duration::from_secs(3);

/// Run the UI. `hosted` is an agent cctop launched for this session, which it
/// shows attached and outlives by nothing: when the agent exits, so does cctop,
/// so `cctop claude` gets you back to your shell the way `claude` would.
pub fn run(args: &Args, hosted: Option<crate::shim::Hosted>) -> anyhow::Result<i32> {
    // Before anything draws, and once: the palette is read by every widget and
    // must not change under them mid-run. Before `ratatui::init` too, because
    // `auto` asks the terminal for its background and reads the answer off
    // stdin, which nothing else may be reading yet.
    theme::init_from_env(crate::settings::Settings::load().theme.as_deref());

    let (req_tx, req_rx) = channel::<Request>();
    let (res_tx, res_rx) = channel::<Response>();
    let worker = spawn_worker(args.plan, req_rx, res_tx.clone());

    // Pricing and quota are network-bound; keep both off the UI thread.
    {
        let tx = res_tx.clone();
        std::thread::spawn(move || {
            crate::pricing::refresh_pricing_blocking();
            let _ = tx.send(Response::PricingReady);
        });
    }
    spawn_quota_poller(res_tx.clone());
    let hosts = crate::fleet::Host::collect(&args.hosts);
    for host in &hosts {
        spawn_host_poller(host.clone(), res_tx.clone());
    }

    // One cached check per day, off the UI thread. Only ever reports: replacing
    // the binary stays behind an explicit `--update`. A build output is told
    // nothing at all — the hint's whole call to action is `--update`, which
    // refuses on a file cargo is keeping books on.
    if !crate::update::built_by_cargo() {
        std::thread::spawn(move || {
            if let Some(version) = crate::update::available_update() {
                let _ = res_tx.send(Response::UpdateAvailable(version));
            }
        });
    }

    let mut app = App::new(args.plan, req_tx.clone());
    app.refresh_secs = args.delay;
    // With nothing to put in it, HOST is a column of one repeated word. Hidden
    // through the same mechanism the user has, so `$CCTOP_COLUMNS_HIDE` and this
    // cannot disagree about what is on screen.
    if hosts.is_empty() {
        app.hidden_columns.push(ColumnId::Host);
    }
    // The conversations and serves that reach back to a remote row ask the
    // `Host`, not the row — the row only knows the machine's name.
    app.remote_hosts = hosts;
    // Likewise USER: with only this user's homes in view, every row's owner is
    // the person reading the screen.
    if crate::config::OTHER_HOMES.is_empty() {
        app.hidden_columns.push(ColumnId::User);
    }
    // And PROFILE, which most machines have exactly one of. A column repeating
    // `default` down every row is a column that answers nothing.
    if crate::config::profile_count() <= 1 {
        app.hidden_columns.push(ColumnId::Profile);
    }
    // Ahead of the first walk, so the first table already attributes processes
    // by what the agents said rather than by the guess that stands in when
    // nothing has.
    if !app.hook_pids.is_empty() {
        let _ = req_tx.send(Request::HookClaims(app.hook_pids.clone()));
    }
    let _ = req_tx.send(Request::Refresh);

    // Tabs backed by rmux outlive cctop. Reattach them before the first frame
    // so reopening the dashboard restores the workspace rather than making the
    // user find and reopen every surviving agent through the launcher.
    app.restore_running_tabs();

    // Attach before the first frame: the agent cctop was asked to launch is the
    // reason it is running, so it should be on screen and not behind a keypress.
    // Its own session row appears later, once it has written a transcript.
    let mut hosted = hosted;
    if let Some(hosted) = hosted.as_ref() {
        app.hosted = Some((hosted.pid, hosted.label.clone()));
        app.attach_hosted();
    }

    // `ratatui::init` installs a hook that leaves the alt screen and raw mode on
    // panic, but it knows nothing about the mouse capture enabled below, nor does
    // its restore path make the cursor visible again. Without this, a panic can
    // leave the user's shell receiving mouse escape sequences or with no cursor.
    // Installed after `init` so it runs before ratatui's restore hook.
    let mut terminal = ratatui::init();
    // Light mode has to own the terminal's default colours. A Reset cell
    // (Clear, unstyled help text, a column with no hue of its own) otherwise
    // punches through to whatever the emulator was — dark ink on a dark
    // ground, which is how white mode used to look like nothing.
    if theme::variant() == theme::Variant::Light {
        let _ = execute!(
            std::io::stdout(),
            ratatui::crossterm::style::SetColors(ratatui::crossterm::style::Colors::new(
                ratatui::crossterm::style::Color::Black,
                ratatui::crossterm::style::Color::White,
            )),
        );
    }
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous_hook(info);
    }));
    let _ = execute!(std::io::stdout(), crossterm::style::Print(MOUSE_ON));
    // Bracketed paste, so a paste arrives as one `Event::Paste` instead of as
    // one `Event::Key` per character. Without it there is no way to tell a paste
    // from typing, and the newlines in a pasted message reach the agent as the
    // Enter that submits it — a five-line paste asking five questions.
    let _ = execute!(std::io::stdout(), EnableBracketedPaste);
    // Ask the terminal to tell Shift+Enter apart from Enter, which it otherwise
    // cannot: both are a carriage return, and a terminal has no other way to
    // send the difference. An agent in a pane wants that difference badly —
    // Shift+Enter is how you write a second line of a prompt without submitting
    // the first — and until cctop asks for it, the keypress reaches cctop as a
    // plain Enter and the distinction is lost before any pane could carry it.
    //
    // Only the disambiguating flag, and only where the terminal says it can:
    // the richer flags report key releases and repeats, which would double
    // every keystroke cctop already handles. Terminals without the protocol are
    // left alone, and there Shift+Enter stays what it has always been.
    if crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false) {
        let _ = execute!(
            std::io::stdout(),
            event::PushKeyboardEnhancementFlags(
                event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            )
        );
    }

    // Established before the loop so the first tick already has it; `None` just
    // means discovery falls back to the periodic walk.
    let watch = crate::watch::Watch::start();
    app.listener = crate::hook::Listener::start();
    // A hook naming a cctop that has since been moved or deleted fires nothing
    // at all, so it is repointed here rather than left to look installed while
    // reporting nothing. Anything narrower than that is left for the panel.
    for fixed in crate::hook::repair(app.hook_project().as_deref()) {
        app.set_status(&fixed);
    }
    // What repair deliberately would not touch: an install registering fewer
    // events than this cctop wants, or a settings file that will not parse.
    // Both look installed and quietly deliver less than they should, so they
    // are worth one line on the way in — an install that is simply absent is
    // not, since that is a choice and nagging about it is what makes people
    // stop reading the status line.
    if app
        .hook_status()
        .entries
        .iter()
        .any(|s| s.health.is_problem())
    {
        app.set_status("Agent hooks need attention — press h");
    }

    let result = event_loop(
        &mut app,
        &mut terminal,
        &res_rx,
        &req_tx,
        watch.as_ref(),
        hosted.as_mut(),
    );

    // Before the terminal is restored, so the agent's hangup does not race the
    // screen being handed back. Clearing the tabs only ends the panes; a
    // rmux-backed one is a client, and the agent behind it is left running —
    // which is the point, and so worth saying out loud on the way out.
    let had_tabs = !app.open_rmux().is_empty();
    app.tabs.clear();
    drop(hosted);

    // Asked after the clients are gone, and asked of rmux rather than of the
    // tabs: an agent left running by an earlier cctop is just as reachable as
    // one from this run, and the line below is the only thing that tells anyone
    // they are there at all.
    let left_running = match had_tabs {
        true => crate::rmux::sessions(),
        // Nothing here ever touched rmux, so nothing here is owed an account of
        // what is in it.
        false => Vec::new(),
    };

    restore_terminal();

    // After the restore, so it lands on the terminal the user is handed back
    // rather than inside the alternate screen that is about to be torn down.
    if !left_running.is_empty() {
        println!(
            "{} agent{} still running in rmux; `cctop` then `t` to get back to {}.",
            left_running.len(),
            if left_running.len() == 1 { "" } else { "s" },
            if left_running.len() == 1 {
                "it"
            } else {
                "them"
            },
        );
    }

    let _ = req_tx.send(Request::Shutdown);
    // The worker persists newly extracted transcript data while shutting down.
    // Joining it matters: returning from main immediately would otherwise kill
    // the detached thread mid-save, forcing every launch to parse all sessions
    // from scratch again.
    let _ = worker.join();
    app.save_prefs();
    result
}

/// Undo every terminal mode the TUI may have changed.
///
/// Mouse tracking, asked for by hand rather than through crossterm's
/// `EnableMouseCapture`.
///
/// The difference is `?1003h`, which crossterm turns on and this does not. That
/// is *any-motion* tracking: the terminal reports a bare hover, one sequence per
/// cell the pointer crosses, with no button held. cctop has never used one —
/// `on_mouse` drops movement, and switching tabs on a hover would drag you out
/// of the agent you are typing into — so the whole stream was noise.
///
/// Noise with a cost, though. A hover report is `\x1b[<35;79;14M`, and a reader
/// that takes it across two reads loses the `\x1b[` and keeps the rest, which
/// lands in whatever is being typed into as the literal text `<35;79;14M`. That
/// needs a machine lagging enough to split a read mid-sequence and a pointer
/// moving through it — which is exactly when it was reported. Presses, drags and
/// the wheel are all still asked for, so nothing cctop reads is lost; there is
/// simply no longer a report for every pixel of hover to be torn in half.
///
/// `?1000h` presses and releases, `?1002h` drags, `?1006h` the SGR encoding that
/// can name a column past 223.
///
/// `?1003l` first, turning any-motion *off*: asking only for the modes above
/// leaves one already on exactly as it was, and a program that ran in this
/// terminal before cctop — an agent in fullscreen, a cctop that died — can have
/// left it on. The hover stream it keeps sending is what gets torn.
const MOUSE_ON: &str = "\x1b[?1003l\x1b[?1000h\x1b[?1002h\x1b[?1006h";

/// `ratatui::restore` intentionally only disables raw mode and leaves the
/// alternate screen; it does not restore cursor visibility. Keep this separate
/// so regular exits, input errors, and panics all use the same cleanup path.
fn restore_terminal() {
    // Popped unconditionally: a push that never happened pops nothing, and a
    // terminal left in the protocol would report keys to the user's shell in a
    // form it does not read.
    let _ = execute!(std::io::stdout(), event::PopKeyboardEnhancementFlags);
    let _ = execute!(std::io::stdout(), DisableBracketedPaste);
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    let _ = execute!(std::io::stdout(), Show);
    let _ = execute!(std::io::stdout(), ratatui::crossterm::style::ResetColor);
    ratatui::restore();
    // Send Show once more after leaving the alternate screen. Some terminals
    // scope cursor state to the active screen buffer.
    let _ = execute!(std::io::stdout(), Show);
}

/// Read one machine on a timer until cctop exits.
///
/// A thread rather than a slot in the worker's queue: an ssh round trip can
/// take seconds or hang until its timeout, and the worker is what answers the
/// keyboard's refresh. One wedged host must cost only itself.
fn spawn_host_poller(host: crate::fleet::Host, tx: Sender<Response>) {
    std::thread::spawn(move || {
        loop {
            let snapshot = host.poll();
            if tx
                .send(Response::Remote {
                    host: host.target.clone(),
                    snapshot,
                })
                .is_err()
            {
                // The UI has gone; so should this.
                return;
            }
            std::thread::sleep(crate::fleet::POLL);
        }
    });
}

/// Write every window of every account into the burn log.
///
/// Returns whether anything new was stored. Only `Ok` statuses carry windows;
/// a signed-out or throttled account has nothing to record, and recording a
/// zero for it would look exactly like an account that used none of its
/// allowance.
fn record_burn(log: &mut crate::burn::Log, quota: &Quota) -> bool {
    let at = crate::util::now_ms() / 1000;
    let mut stored = false;
    for (provider, profiles) in [("claude", &quota.claude), ("codex", &quota.codex)] {
        for profile in profiles {
            let crate::quota::ProviderStatus::Ok(q) = &profile.status else {
                continue;
            };
            for window in &q.windows {
                stored |= log.record(
                    crate::burn::key(provider, &profile.profile, window.label),
                    crate::burn::Sample {
                        at,
                        pct: window.pct,
                        resets_at: window.resets_at,
                        plan: q.plan.clone(),
                    },
                );
            }
        }
    }
    stored
}

fn spawn_quota_poller(tx: Sender<Response>) {
    std::thread::spawn(move || {
        let mut quota = Quota::default();
        let (mut claude_due, mut codex_due) = (Instant::now(), Instant::now());
        // Every reading of every window passes through here, which is the only
        // place that is true — so it is where they get written down. A window
        // that resets takes its own history with it, and nothing else in cctop
        // sees a figure before that happens.
        let mut burn = crate::burn::Log::load();

        loop {
            let now = Instant::now();
            let mut changed = false;

            // Each provider is paced by its own last outcome: a throttled one
            // backs off without stalling the other.
            // An account just added is asked about now; the cache answers
            // for the others, so this is one request.
            if now >= claude_due
                || crate::quota::NUDGE.swap(false, std::sync::atomic::Ordering::Relaxed)
            {
                // Each profile is its own account with its own limits, so each
                // is asked separately — and paced separately, by the usage
                // cache, which answers for any account that is not due yet.
                quota.claude = crate::config::accounts_for(Provider::Claude)
                    .iter()
                    .map(|profile| crate::quota::ProfileQuota {
                        profile: profile.name.clone(),
                        status: crate::quota::fetch_claude(profile),
                        source: profile.source,
                    })
                    .collect();
                // Woken for whichever account is due soonest. The cache holds
                // the rest, so a throttled one is not asked early on behalf of
                // another, and a healthy one is not left waiting on it.
                let delay = quota
                    .claude
                    .iter()
                    .map(|q| q.status.retry_delay_secs(QUOTA_INTERVAL_SECS))
                    .min()
                    .unwrap_or(QUOTA_INTERVAL_SECS);
                claude_due = now + Duration::from_secs(delay);
                changed = true;
            }
            if now >= codex_due {
                // Per account for the same reason as Claude's, and paced the
                // same way.
                quota.codex = crate::config::accounts_for(Provider::Codex)
                    .iter()
                    .map(|profile| crate::quota::ProfileQuota {
                        profile: profile.name.clone(),
                        status: crate::quota::fetch_codex(profile),
                        source: profile.source,
                    })
                    .collect();
                let delay = quota
                    .codex
                    .iter()
                    .map(|q| q.status.retry_delay_secs(QUOTA_INTERVAL_SECS))
                    .min()
                    .unwrap_or(QUOTA_INTERVAL_SECS);
                codex_due = now + Duration::from_secs(delay);
                changed = true;
            }

            if changed {
                quota.fetched = true;
                // Only written when a reading actually said something new, so an
                // idle account does not rewrite the file every five minutes for
                // nothing.
                if record_burn(&mut burn, &quota) {
                    burn.save();
                }
                if tx.send(Response::Quota(Box::new(quota.clone()))).is_err() {
                    break;
                }
            }
            std::thread::sleep(QUOTA_TICK);
        }
    });
}

fn event_loop(
    app: &mut App,
    terminal: &mut ratatui::DefaultTerminal,
    res_rx: &Receiver<Response>,
    req_tx: &Sender<Request>,
    watch: Option<&crate::watch::Watch>,
    mut hosted: Option<&mut crate::shim::Hosted>,
) -> anyhow::Result<i32> {
    let mut last_refresh = Instant::now();
    let mut last_full_walk = Instant::now();
    let mut layout = render::Layout::default();
    let mut refresh_in_flight = true;
    let mut last_blink = true;

    loop {
        // Drain everything the workers have produced.
        // Whether any of it changed the table, which is also the question
        // "does the page need telling" — see the feed below the loop.
        let mut annotated_rows_changed = false;
        // Only a refresh can move a session between busy and waiting, so the
        // notifier is fed here rather than once per loop iteration — that would
        // rebuild its map five times a second over rows that hadn't moved.
        let mut rows_changed = false;
        loop {
            match res_rx.try_recv() {
                Ok(Response::Discovered(sessions)) => {
                    app.sessions = sessions;
                    app.loaded = true;
                    app.stats = crate::loader::compute_stats(&app.sessions);
                    app.refilter();
                    app.merge_remotes();
                    rows_changed = true;
                }
                Ok(Response::Annotated(session)) => {
                    // Match on the key's two fields rather than on `key()`: that
                    // formats a String per candidate, so a scan over thousands of
                    // rows allocated thousands of times — per arriving row, on the
                    // thread that also has to answer the keyboard.
                    // `remote.is_none()` is part of the identity, not a
                    // nicety: the worker only ever reports local rows, and a
                    // remote session that happened to share an id would be
                    // overwritten by one from this machine.
                    let found = app.sessions.iter_mut().find(|s| {
                        s.remote.is_none()
                            && s.provider == session.provider
                            && s.session_id == session.session_id
                    });
                    if let Some(existing) = found {
                        *existing = *session;
                    } else {
                        app.sessions.push(*session);
                    }
                    annotated_rows_changed = true;
                }
                Ok(Response::Sessions(payload)) => {
                    let (sessions, stats) = *payload;
                    app.sessions = sessions;
                    app.loaded = true;
                    app.stats = stats;
                    app.merge_remotes();
                    app.push_history();
                    app.refilter();
                    crate::elog::event(
                        "scan",
                        "refresh",
                        serde_json::json!({"sessions": app.sessions.len(), "kind": "full"}),
                    );
                    refresh_in_flight = false;
                    annotated_rows_changed = false;
                    rows_changed = true;
                }
                Ok(Response::LiveRows(payload)) => {
                    let (rows, stats) = *payload;
                    for row in rows {
                        let found = app.sessions.iter_mut().find(|s| {
                            s.remote.is_none()
                                && s.provider == row.provider
                                && s.session_id == row.session_id
                        });
                        match found {
                            Some(existing) => *existing = row,
                            // A session that started since the last full walk.
                            None => app.sessions.push(row),
                        }
                    }
                    app.adopt_stats(stats);
                    app.loaded = true;
                    app.push_history();
                    app.refilter();
                    crate::elog::event(
                        "scan",
                        "refresh",
                        serde_json::json!({"sessions": app.sessions.len(), "kind": "live"}),
                    );
                    refresh_in_flight = false;
                    rows_changed = true;
                }
                Ok(Response::Data(key, data)) => {
                    // Discard results for a session the user has already left.
                    if key == app.panel_key {
                        app.panel_data = Some(*data);
                        app.needs_redraw = true;
                    }
                }
                Ok(Response::Quota(q)) => {
                    // Before the new reading replaces the old: the notifier's
                    // edge is between the two, so it has to see this one first.
                    for text in app.notify.observe_quota(&q) {
                        app.announce_quota_freed(&text);
                    }
                    app.quota = *q;
                    // The poller has just written whatever this reading added,
                    // so this is the one moment the log is known to have moved.
                    app.burn = crate::burn::Log::load();
                    app.needs_redraw = true;
                }
                Ok(Response::UpdateAvailable(version)) => {
                    app.update_available = Some(version);
                    app.needs_redraw = true;
                }
                Ok(Response::PricingReady) => {
                    // Cached costs were computed without rates; recompute them.
                    let _ = req_tx.send(Request::Refresh);
                    refresh_in_flight = true;
                }
                Ok(Response::Terminated {
                    session_key,
                    result,
                }) => match result {
                    Ok(()) => {
                        app.set_status("Termination signal sent");
                        let _ = req_tx.send(Request::Refresh);
                        refresh_in_flight = true;
                    }
                    Err(error) => {
                        app.set_status(format!("Could not stop {session_key}: {error}"));
                    }
                },
                Ok(Response::Deleted {
                    session_key,
                    result,
                }) => {
                    app.deleting.remove(&session_key);
                    match result {
                        Ok(()) => {
                            app.sessions.retain(|session| session.key() != session_key);
                            app.marked.remove(&session_key);
                            app.stats = crate::loader::compute_stats(&app.sessions);
                            app.refilter();
                            app.set_status("Deleted session");
                        }
                        Err(error) => {
                            app.set_status(format!("Could not delete {session_key}: {error}"))
                        }
                    }
                }
                Ok(Response::KeysSent { result }) => match result {
                    Ok(()) => app.set_status("Sent to the session's terminal"),
                    Err(error) => app.set_status(error),
                },
                Ok(Response::Remote { host, snapshot }) => {
                    match snapshot {
                        crate::fleet::Snapshot::Rows(rows) => {
                            app.remote_errors.remove(&host);
                            app.remotes.insert(host, rows);
                        }
                        // The last good snapshot is kept rather than blanked: a
                        // dropped ssh connection has not stopped those agents,
                        // and an empty machine is a stronger claim than a stale
                        // one. The footer says the reading is old.
                        crate::fleet::Snapshot::Failed(why) => {
                            app.remote_errors.insert(host, why);
                        }
                    }
                    app.merge_remotes();
                    rows_changed = true;
                }
                Ok(Response::Scanned { query, hits }) => app.scanned(query, hits),
                Ok(Response::Insight(text)) => {
                    app.insight = Some(text);
                    app.insight_scroll = 0;
                }
                Ok(Response::Chat {
                    key,
                    before,
                    result,
                }) => app.got_chat(key, before, result),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        // A permission prompt whose grace has run out is news, and nothing is
        // going to send an event to say so — the event that would have spared
        // us is the one that did not arrive.
        rows_changed |= app.promote_matured_prompts();
        if annotated_rows_changed || rows_changed {
            // Extraction rebuilds each subagent from its transcript, which
            // cannot know what a hook already reported, so the hook's answer is
            // reapplied to every batch of rows that replaces them. The same
            // goes for the permission mode, which no transcript records at all.
            app.apply_finished_agents();
            app.apply_reports();
            // After the hooks, because a row's liveness is what decides whether
            // it can still race anyone, and cheap enough to redo wholesale:
            // it compares paths already in memory and reads no transcript.
            app.collisions = crate::collide::apply(&mut app.sessions);
        }
        if annotated_rows_changed {
            // A burst can contain hundreds of rows. Recompute and sort once
            // after draining it rather than once per transcript.
            app.stats = crate::loader::compute_stats(&app.sessions);
            app.refilter();
        }
        if rows_changed {
            app.check_bells();
        }
        // The page gets what the table has, and only when it changed. This is
        // also what wakes a browser: its event stream is parked on the version
        // this bumps, so a page updates when the table does rather than on a
        // clock of its own.
        if rows_changed || annotated_rows_changed {
            app.feed_serving();
        }

        app.sync_panel_data();
        app.tick_scan();

        // Every tab, not just the visible one: an agent whose output nobody
        // reads eventually blocks on writing it.
        let mut drawn = false;
        for tab in &mut app.tabs {
            drawn |= tab.pump();
        }
        // An agent that said what it wanted says it once, to the person who
        // just looked: the tab colour is the alarm, this is the message. Here
        // rather than on the keypress that focused the pane — a bell arriving
        // while you are already on it has also been seen, and a tab you switch
        // away from must keep its bell rather than have it cleared by the tick.
        if let Some(note) = app.focused_pane().and_then(tabs::Pane::answer_bell) {
            app.set_status(note);
        }
        let closed = app.tabs.iter_mut().fold(false, |any, tab| tab.reap() | any);
        if closed {
            app.drop_empty_tabs();
        }
        app.pump_add_account();
        // After the reap, so a finished install is seen as finished on the same
        // tick its pane goes away.
        app.poll_rmux_install();
        // And after both, so a tab this cctop has just lost is not immediately
        // re-added by a listing taken before its session went.
        app.sync_shared_tabs();
        if drawn || closed {
            app.needs_redraw = true;
        }

        // Hook events arrive whenever an agent hits one, which is not on any
        // tick of ours, so they are drained here alongside everything else.
        if let Some(events) = app.listener.as_ref().map(crate::hook::Listener::drain) {
            let (changed, lifecycle) = app.apply_hooks(events);
            app.needs_redraw |= changed;
            // A session that has just begun or ended is a row to find or forget
            // now. Waiting for the next poll would leave an agent the user just
            // started missing from the table for as long as the interval, which
            // is precisely the moment they are looking for it.
            if lifecycle && !refresh_in_flight {
                let _ = req_tx.send(Request::Refresh);
                refresh_in_flight = true;
                last_refresh = Instant::now();
            }
        }

        // A brief for a just-launched agent comes due on a timer rather than an
        // event, so the loop is the only thing that can notice.
        app.tick_handoff();
        app.tick_torn();

        // The same is true of a tunnel being registered: the answer arrives on
        // a channel nothing polls but this, and until it does the corner has a
        // spinner to turn.
        app.needs_redraw |= app.tick_share();
        // The insight report's spinner turns on the same terms, and so does
        // the conversation view's.
        app.needs_redraw |= app.insight_loading() || app.chat_loading();

        // A blinking tab is the one thing on screen that changes with no event
        // behind it, so the loop has to ask for the frame itself — but only on
        // the half-cycle it actually flips, not on every poll.
        let phase = app.blink_on();
        if phase != last_blink && app.any_attention() {
            app.needs_redraw = true;
        }
        last_blink = phase;

        // Expire the transient status line.
        if let Some((_, at)) = &app.status
            && at.elapsed() > Duration::from_secs(3)
        {
            app.status = None;
            app.needs_redraw = true;
        }

        // A pasted image's corner preview expires on the same terms, a touch
        // longer — it is the confirmation that *that* image went.
        if let Some(preview) = &app.paste_preview
            && preview.at.elapsed() > Duration::from_secs(5)
        {
            app.paste_preview = None;
            app.needs_redraw = true;
        }

        if app.needs_redraw {
            terminal.draw(|frame| layout = render::draw(frame, app))?;
            app.needs_redraw = false;
        }

        // Wait for input, but never past the next scheduled refresh. The
        // interval is read live so +/- changes apply on the very next poll.
        let refresh_every = Duration::from_secs_f64(app.refresh_secs);
        // Attached, the same wait is what stands between a keystroke and seeing
        // it echoed, so it drops to a frame's worth.
        let idle_wait = match app.tab {
            0 => Duration::from_millis(200),
            _ => Duration::from_millis(16),
        };
        // A spinner that advances five times a second reads as a stutter. While
        // one is turning the loop wakes at its frame rate instead.
        let idle_wait =
            match app.share_opening.is_some() || app.insight_loading() || app.chat_loading() {
                true => idle_wait.min(Duration::from_millis(100)),
                false => idle_wait,
            };
        let wait = refresh_every
            .checked_sub(last_refresh.elapsed())
            .unwrap_or(Duration::ZERO)
            .min(idle_wait);
        if event::poll(wait)? {
            let event = event::read()?;
            crate::elog::tui(&event);
            match event {
                Event::Key(key) => app.on_key(key),
                Event::Paste(text) => app.on_paste(&text),
                Event::Mouse(m) => app.on_mouse(m, &layout),
                Event::Resize(_, _) => app.needs_redraw = true,
                _ => {}
            }
        }

        // The agent cctop was launched to run has finished, so cctop has nothing
        // left to do either: hand its exit code back and get out of the way.
        if let Some(hosted) = hosted.as_mut()
            && let Some(code) = hosted.finished()
        {
            return Ok(code);
        }

        if app.should_quit {
            break;
        }

        // Only one refresh in flight: a scan slower than the interval must not
        // queue up behind itself.
        let refresh_every = Duration::from_secs_f64(app.refresh_secs);
        if last_refresh.elapsed() >= refresh_every && !refresh_in_flight {
            last_refresh = Instant::now();
            refresh_in_flight = true;
            // Walking every provider directory is what scales with the number of
            // sessions ever created, while what the user watches scales with the
            // number running now. So the fast tick updates the running rows and
            // the walk — the only thing that can notice a *new* session — runs on
            // its own slower cadence.
            let watched_change = watch.is_some_and(crate::watch::Watch::took_structural_change);
            // A transcript is created before it is summarizable — the model name
            // only arrives with the first assistant message — so the walk the
            // create earned can find nothing. Keep walking, at a cadence between
            // the fast tick and the safety net, until the file becomes a session.
            let awaiting = !watched_change
                && last_full_walk.elapsed() >= PENDING_WALK_INTERVAL
                && watch.is_some_and(|w| {
                    w.awaiting_discovery(|path| {
                        app.sessions
                            .iter()
                            .any(|s| s.data_file.as_deref() == Some(path))
                    })
                });
            let full_due =
                watched_change || awaiting || last_full_walk.elapsed() >= FULL_WALK_INTERVAL;
            if full_due {
                last_full_walk = Instant::now();
            }
            let _ = req_tx.send(if full_due {
                Request::Refresh
            } else {
                Request::RefreshLive
            });
        }
    }
    // Quit was pressed rather than the agent exiting, so there is no exit code
    // to inherit.
    Ok(0)
}
