mod cli;
mod doctor;
mod mcp;
mod recall;
mod tunnel;
mod wait;
mod why;

// The commands below are dispatched from here by their module names, as they
// were when all of cctop was this one crate.
use cctop_core::{
    alias, attach, burn, cache, elog, embed, hook, insight, loader, pricing, quota, sandbox,
    settings, shim, trace, update,
};
use cctop_serve as serve;
use cctop_ui as ui;

use clap::Parser;
use std::io::IsTerminal;

/// mimalloc, rather than whichever allocator the platform came with.
///
/// Nearly everything cctop does at load is allocate: parsing JSON transcripts,
/// on every core at once. That makes the allocator the hot path rather than a
/// detail of it, and the Linux binaries we ship are static musl builds whose
/// allocator does not hold up under exactly that — many threads, small
/// allocations, all at the same time.
///
/// Measured on a machine with 2020 sessions, the same commit built against
/// glibc instead of musl: discovery took 0.17s where musl took 7.22s, and the
/// whole run 2.8s against 13.5s. Neither number is about parsing.
///
/// Replacing the allocator keeps what musl was chosen for — one static binary
/// that runs on any Linux — instead of trading it away for a glibc build with a
/// floor on how old a distribution may be.
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> anyhow::Result<()> {
    // Before anything, `hook` included: a binary built by `cargo test` carries
    // core's test guards, and this is what keeps them off. See `under_test`.
    cctop_core::running_as_the_binary();
    // The release version, which is this crate's and not cctop-core's: the
    // updater compares it with the newest release, and core's own version is
    // usually older (see `update::current_version`). Before `hook` too, which
    // stamps it on what it reports.
    cctop_core::update::set_version(env!("CARGO_PKG_VERSION"));

    // `cctop run <agent> …` is handled before clap so the agent's own flags are
    // never mistaken for cctop's — `cctop claude --help` must reach claude.
    //
    // A first argument that names an executable is the same thing without the
    // `run`: cctop takes no positionals, so a command is the only thing it can
    // be. Anything else (a typo, a stray word) falls through to clap's usage
    // error rather than being exec'd.
    //
    // The two forms differ in what surrounds the agent. `cctop run claude` is a
    // transparent stand-in for `claude` and hands over the terminal; `cctop
    // claude` starts cctop with the agent attached inside it, which is the point
    // of launching it through cctop at all.
    // `cctop hook` is spawned by the agent itself, many times a session. It is
    // answered before anything else is set up — no config, no pricing, no cache
    // — because the agent is blocked until it returns.
    //
    // Every platform, deliberately. Delivery is a no-op where there are no unix
    // sockets, but the *exit code* is not: `--install-hooks` writes the agent's
    // settings file on any platform, and a `cctop hook` that fell through to
    // clap would exit non-zero, which Claude Code reads as a decision to block
    // the tool call and feed stderr back to the model. Answering here is what
    // keeps the guarantee the hook module is built around — see its docs.
    //
    // `args_os`, because `args` panics on an argument that is not valid Unicode
    // and this runs before any panic hook of cctop's own is installed: a panic
    // here exits 101, which is the one answer the agent reads as "block". A hook
    // invocation's first argument is the bare word `hook`, so comparing the OS
    // string is all this needs, and the guard below covers the rest.
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("hook")) {
        // Installed here rather than left to `emit`, because reading argv is
        // itself what can unwind. `emit` installs the same hook again, which is
        // idempotent and leaves it safe to call on its own.
        std::panic::set_hook(Box::new(|_| std::process::exit(0)));
        let argv: Vec<String> = std::env::args().collect();
        std::process::exit(hook::emit(&argv[2..]));
    }

    // `cctop yolo-hook`, the one hook that may answer a permission prompt, is
    // spawned by Claude Code exactly as `hook` is and answered here for the
    // same reasons, ungated by any setting: whether it says anything is
    // decided inside it, per session — see `hook::yolo_hook`. Its own word
    // rather than an argument to `hook`, so that `cctop hook` cannot be talked
    // into deciding by anything on its command line.
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("yolo-hook")) {
        std::panic::set_hook(Box::new(|_| std::process::exit(0)));
        let argv: Vec<String> = std::env::args().collect();
        std::process::exit(hook::yolo_hook(&argv[2..]));
    }

    // The parts of `cctop sandbox` that the agent runs, as often as `hook` and
    // for the same reason answered before anything is set up: `--sandbox-exec`
    // is every Claude Bash call (and every hook, which it hands straight to
    // bash), `--sandbox-guard` every Claude file tool call, `--sandbox-shell`
    // every command of an agent given cctop as its shell. Flags rather than
    // words because Claude Code splits its shell prefix at the last " -".
    {
        let mut argv = std::env::args_os().skip(1);
        let first = argv.next();
        let rest = || {
            argv.map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        match first.as_deref().and_then(std::ffi::OsStr::to_str) {
            Some("--sandbox-exec") => std::process::exit(sandbox::exec(&rest())),
            Some("--sandbox-guard") => std::process::exit(sandbox::guard(&rest())),
            Some("--sandbox-shell") => std::process::exit(sandbox::shell(&rest())),
            _ => {}
        }
    }

    // `cctop mux`: cctop's own rmux daemon (#197), and the client a tab's
    // pane runs to sit on one of its sessions. After `hook` and the sandbox
    // words, which must answer before anything else, and before clap and any
    // TUI setup: the daemon is a long-lived server, not a command line, and
    // the attach client owns the terminal it is started on.
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("mux")) {
        let argv: Vec<String> = std::env::args().skip(2).collect();
        std::process::exit(cctop_core::mux::main(&argv));
    }

    // Piped into `head`, `jq` or `less` that stops reading early, every
    // `println!` in the non-interactive commands panics on the closed pipe and
    // prints a backtrace hint under output that was otherwise fine — `-l`,
    // `--json`, `log` and `why` all did. Restoring SIGPIPE's default would
    // also fix it, and would also kill the TUI the first time a child's pipe
    // closed under it, so instead exactly that one panic is answered with a
    // quiet exit and every other panic still reaches the usual hook. After
    // `hook`, which installs its own and must never be changed by this.
    {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // A formatted panic carries a `String`, a literal one a `&str`.
            let payload = info.payload();
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied());
            let closed = message.is_some_and(|m| {
                m.starts_with("failed printing to stdout") && m.contains("Broken pipe")
            });
            if closed {
                std::process::exit(0);
            }
            previous(info);
        }));
    }

    // `cctop doctor` is intercepted here for the same reason `run` and `attach`
    // are: cctop takes no positionals, so clap would answer a bare word with a
    // usage error. Before the `is_command` check below, so a stray `doctor`
    // binary on PATH cannot shadow it.
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).map(String::as_str) == Some("doctor") {
            std::process::exit(doctor::run(&argv[2..]));
        }
    }

    // `cctop why` alongside `doctor`, and for the same reason: a bare word, and
    // cctop has no positionals for clap to read one as.
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).map(String::as_str) == Some("why") {
            std::process::exit(why::run(&argv[2..]));
        }
    }

    // `cctop burn` alongside `doctor`, and for the same reason: a bare word,
    // and cctop has no positionals for clap to read one as.
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).map(String::as_str) == Some("burn") {
            std::process::exit(burn::run(&argv[2..]));
        }
    }

    // `cctop log` alongside `burn`, and for the same reason. It only ever
    // reads a file, so it needs nothing set up before it — which is also why
    // it can debug every other command line cctop has.
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).map(String::as_str) == Some("log") {
            std::process::exit(elog::run(&argv[2..]));
        }
    }

    // `cctop yolo log` alongside `log`, and for the same reason: it reads a
    // file and needs nothing set up. Matched on the bare word `yolo` only, so
    // the ungated `yolo-hook` dispatch above is a different word entirely.
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).map(String::as_str) == Some("yolo") {
            std::process::exit(cctop_core::yolo_log::run(&argv[2..]));
        }
    }

    // `cctop optimize`, `compare` and `yield` alongside `doctor`, for the same
    // reason: all are bare words and cctop has no positionals for clap to read
    // one as. Every platform — none asks anything of the operating system.
    {
        let argv: Vec<String> = std::env::args().collect();
        if let Some(word @ ("optimize" | "compare" | "yield")) = argv.get(1).map(String::as_str) {
            std::process::exit(insight::run(word, &argv[2..]));
        }
    }

    // `cctop recall` alongside `doctor`, and for the same reason: a bare word,
    // and its query is positional, which cctop otherwise has none of.
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).map(String::as_str) == Some("recall") {
            std::process::exit(recall::run(&argv[2..]));
        }
    }

    // `cctop wait` alongside `doctor`, and for the same reason: a bare word
    // with a positional of its own. Its flags are clap's, parsed from the rest
    // — a usage error exits 2, which is also what an unknown session exits
    // with, so a script has one code for "you asked about nothing".
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).map(String::as_str) == Some("wait") {
            let args = cli::WaitArgs::parse_from(
                std::iter::once("cctop wait".to_string()).chain(argv[2..].iter().cloned()),
            );
            std::process::exit(wait::run(&args));
        }
    }

    // `cctop as <account> <agent>` alongside `doctor`, and for the same reason.
    // It is how a token account is launched, from the launcher or a shell.
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).map(String::as_str) == Some("as") {
            std::process::exit(quota::run_as(&argv[2..])?);
        }
    }

    // `cctop sandbox <host>:<path> [claude args…]` alongside `as`, and for the
    // same reason: a bare word, with the agent's own arguments after it.
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).map(String::as_str) == Some("sandbox") {
            std::process::exit(sandbox::run_held(&argv[2..])?);
        }
    }

    // `cctop tunnel` alongside `serve`, whose tunnel it connects: a bare word,
    // like the others here, and well below `hook`.
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).map(String::as_str) == Some("tunnel") {
            std::process::exit(tunnel::run(&argv[2..])?);
        }
    }

    // `cctop serve` is intercepted alongside `doctor`, and for the same reason:
    // it is a bare word, and cctop has no positionals for clap to read one as.
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).map(String::as_str) == Some("serve") {
            std::process::exit(serve::run(&argv[2..])?);
        }
    }

    let mut agent: Option<Vec<String>> = None;
    {
        let argv: Vec<String> = std::env::args().collect();
        // `cctop attach` puts a running agent on this terminal directly, with no
        // UI around it. Handled here for the same reason as `run`: it takes a
        // positional, and cctop otherwise has none.
        if argv.get(1).map(String::as_str) == Some("attach") {
            std::process::exit(attach::run_terminal(&argv[2..])?);
        }
        match argv.get(1).map(String::as_str) {
            Some("run") => std::process::exit(shim::run(&argv[2..])?),
            Some(word) if !word.starts_with('-') && shim::is_command(word) => {
                agent = Some(argv[1..].to_vec());
            }
            _ => {}
        }
    }

    // Everything after the agent's name belongs to the agent, so clap must not
    // be shown it; the UI wrapped around it runs on its defaults.
    let args = match agent {
        Some(_) => cli::Args::parse_from(["cctop"]),
        None => cli::Args::parse(),
    };

    // Before anything else measurable happens, and in particular before the
    // caches are touched: a trace that starts after the slow part is no trace.
    if args.trace.is_some() {
        trace::enable();
    }

    if let Some(staged) = &args.install_update {
        return update::install_staged(&staged[0], &staged[1]);
    }
    if args.update {
        update::run(false)?;
        // The new binary does this, not this one: an update that changed how
        // a hook is written is one this process knows nothing about. Without
        // it a CLI-only update would leave the old form firing until the next
        // dashboard start.
        update::repair_hooks_with_new_binary();
        return Ok(());
    }
    if args.repair_hooks {
        for line in hook::repair(None).terminal_lines() {
            println!("{line}");
        }
        return Ok(());
    }

    if args.fetch_search_model {
        embed::fetch::fetch()?;
        return Ok(());
    }

    if args.hooks_status {
        let cwd = std::env::current_dir().ok();
        for (line, problem) in hook::status(cwd.as_deref(), None).lines() {
            eprintln!("{} {line}", if problem { "!" } else { "·" });
        }
        return Ok(());
    }

    if let Some(scope) = args
        .install_hooks
        .as_deref()
        .or(args.remove_hooks.as_deref())
    {
        let installing = args.install_hooks.is_some();
        let cwd = std::env::current_dir().unwrap_or_default();
        let Some(scope) = hook::Scope::parse(scope, &cwd) else {
            anyhow::bail!("unknown scope '{scope}'; use `user` or `project`");
        };
        // One line per harness, including the ones that could not be done:
        // they are separate files, and a `notify` slot that already belongs to
        // somebody else must not undo the four installs that succeeded.
        for line in match installing {
            true => hook::install(&scope),
            false => hook::remove(&scope),
        } {
            eprintln!("{line}");
        }
        eprintln!("Sessions already running keep their old hooks until restarted.");
        return Ok(());
    }

    if args.install_alias || args.remove_alias {
        let changed = if args.install_alias {
            alias::install()
        } else {
            alias::remove()
        };
        match changed.as_slice() {
            [] => eprintln!("No shell startup file needed changing."),
            files => {
                for f in files {
                    eprintln!("Updated {}", f.display());
                }
                eprintln!("Restart your shell, or source the file, to pick it up.");
            }
        }
        return Ok(());
    }

    if let Some(profile) = &args.add_account {
        return quota::add_account(profile);
    }

    if args.clear_cache && cache::clear_session_cache()? {
        eprintln!("Cleared cctop session extraction cache.");
    }

    if let Some(scope) = args.install_mcp.as_deref() {
        if !matches!(scope, "user" | "project") {
            anyhow::bail!("unknown scope '{scope}'; use `user` or `project`");
        }
        for line in mcp::install(scope) {
            eprintln!("{line}");
        }
        eprintln!("Agents already running pick it up when restarted.");
        return Ok(());
    }

    if args.mcp {
        // Nothing may be printed to stdout but JSON-RPC: the transport is the
        // stream, and one stray line of logging desynchronises the client.
        return mcp::serve();
    }

    if let Some(which) = &args.handoff {
        let mut loader = loader::Loader::new();
        let sessions = loader.load(args.plan);
        cli::run_handoff(&sessions, which, &loader)?;
        loader.store().save();
        finish_trace(&args);
        return Ok(());
    }

    // The non-interactive session modes together: each reads one session's own
    // transcript, and none of them wants a cache that might be mid-walk.
    if !args.convert.is_empty() || args.converted {
        let mut loader = loader::Loader::new();
        let sessions = loader.load(args.plan);
        if args.converted {
            cli::run_converted(&sessions, args.remove)?;
        } else {
            let (which, agent) = args.convert.split_at(1);
            cli::run_convert(&sessions, which[0].as_str(), agent[0].as_str())?;
        }
        loader.store().save();
        finish_trace(&args);
        return Ok(());
    }

    if args.list
        || args.json
        || args.statusline
        || args.report.is_some()
        || args.chat.is_some()
        || args.export.is_some()
        || args.access.is_some()
    {
        // Non-interactive modes need pricing before they can print anything, so
        // fetch synchronously. The TUI refreshes it on a background thread.
        pricing::refresh_pricing_blocking();

        let mut loader = loader::Loader::new();
        let sessions = loader.load(args.plan);
        if args.json {
            cli::run_json(&sessions, args.plan, &loader)?;
        } else if args.list {
            cli::run_list(&sessions, args.plan);
        } else if let Some(which) = &args.report {
            cli::run_report(&sessions, which, args.plan, &loader)?;
        } else if let Some(which) = &args.chat {
            cli::run_chat(&sessions, which, args.before, args.agent.as_deref())?;
        } else if let Some(which) = &args.export {
            cli::run_export(&sessions, which, args.tool_output)?;
        } else if let Some(which) = &args.access {
            cli::run_access(&sessions, which, &loader)?;
        } else {
            cli::run_statusline(&sessions);
        }
        loader.store().save();
        finish_trace(&args);
        return Ok(());
    }

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        // With no terminal there is no UI to wrap the agent in, but there is
        // still an agent to run: `claude` is aliased to this, and a pipeline or
        // a script must not be told to use --json instead.
        if let Some(agent) = agent {
            std::process::exit(shim::run(&agent)?);
        }
        anyhow::bail!("the interactive UI needs a TTY; use --list or --json instead");
    }

    // Both streams are known to be a TTY by here, and every non-interactive
    // mode (--list, --json, --update, the alias flags, `run`, `attach`, `hook`)
    // has already returned — so this is the only path that may prompt. CI is
    // excluded even when it hands us a TTY: nobody is there to answer. So is
    // `cctop <agent>`, where someone is waiting on an agent to start and
    // already has, for this run, exactly what the alias would have given them.
    let launching_agent = agent.is_some();
    let mut prefs = cache::UiPrefs::load();
    // Before the pricing fetch, the shim and the UI, because this is the one
    // moment a replacement is free: nothing is open yet, so the new binary can
    // be exec'd in place of this process and the session that follows is simply
    // the new version. Every non-interactive mode has already returned above, so
    // no script and no hook can reach this. It returns when there is nothing to
    // do or nothing worked, and does not return at all when it worked.
    //
    // `cctop claude` is excluded for the reason the alias prompt below is: an
    // agent is being waited on, and a download and a keypress between the
    // command and the agent starting is not what was asked for.
    let auto_update = !args.no_auto_update
        && !launching_agent
        && settings::Settings::load()
            .auto_update
            .unwrap_or(prefs.auto_update);
    update::auto_at_startup(auto_update, &mut prefs);

    // After the update offer: a process that is about to be replaced by a newer
    // one has no business asking a question the new one would have to ask again.
    if !launching_agent && std::env::var_os("CI").is_none() {
        alias::ask_on_first_run(&mut prefs);
    }

    // Load whatever pricing is already cached so the first frame isn't zeroed
    // while the network fetch is still in flight.
    pricing::load_cached_pricing();

    // The palette is chosen here rather than inside `ui::run`, because the
    // agent below starts first and may ask what colour the terminal is before
    // the UI would have chosen one: it would be told dark under a light theme.
    // Safe this early because choosing it reads the terminal's reply off stdin,
    // and nothing has started reading stdin yet — hosting the agent does not.
    ui::choose_palette();

    // Started before the UI so a failure to launch prints as an ordinary error
    // rather than from inside the alternate screen.
    let hosted = agent
        .map(|agent| shim::host(&agent, None, ui::render::pane_size()))
        .transpose()?;

    let code = ui::run(
        args.plan,
        args.delay,
        &args.hosts,
        hosted,
        serve_for_dashboard,
    )?;
    // After the UI is down, so the message is not painted over by the alternate
    // screen being restored.
    finish_trace(&args);
    std::process::exit(code)
}

/// Start the server the dashboard shares its table through.
///
/// Here because this is the one crate with both: the dashboard and the server
/// are built side by side, neither depending on the other, and the dashboard
/// asks for a server through this.
fn serve_for_dashboard(request: ui::ServeRequest) -> anyhow::Result<ui::Served> {
    // Over the account's tunnel the tokens are kept, in the file `cctop serve`
    // keeps them in: a stable hostname with a fresh token each time is still a
    // dead bookmark (decision 3 on #174). Elsewhere a stop still revokes every
    // link, which is what a quick tunnel's throwaway hostname is for anyway.
    let tokens = match request.tunnel && cctop_core::tunnel::account().is_some() {
        true => {
            Some(serve::tokens::load_or_create(&serve::account_token_file(), request.rotate)?.0)
        }
        false => None,
    };
    let serving = serve::start(serve::Options {
        tunnel: request.tunnel,
        tokens,
        plan: request.plan,
        // Fed from the rows this dashboard already has. Two loaders in one
        // process would walk the same disk twice and, worse, could disagree —
        // a page saying one thing while the table beside it says another is
        // the bug nobody thinks to look for.
        scan: false,
        hosts: request.hosts,
        ..Default::default()
    })?;
    Ok(ui::Served {
        local: serving.local.clone(),
        public: serving.public.clone(),
        tunnel_fallback: serving.tunnel_fallback.clone(),
        readonly: serving.readonly.clone(),
        actions: serving.actions,
        // The server moves in with the closure, so dropping what the dashboard
        // holds stops it.
        publish: Box::new(move |sessions, quota, provider_status| {
            serving.publish_table(sessions, quota, provider_status)
        }),
    })
}

/// Write the trace, if one was asked for, and say where it went.
///
/// The path is printed rather than merely returned because the whole point is
/// to hand the file to somebody: a report written somewhere the user has to go
/// looking for is one they will not send.
fn finish_trace(args: &cli::Args) {
    let Some(requested) = args.trace.as_deref() else {
        return;
    };
    let path = match requested {
        "" => trace::default_path(),
        given => std::path::PathBuf::from(given),
    };
    match trace::write_to(&path) {
        Ok(()) => eprintln!("cctop: trace written to {}", path.display()),
        Err(error) => eprintln!(
            "cctop: could not write trace to {}: {error}",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    /// What the dashboard holds of a server it started is a real server's
    /// links: on loopback, and carrying the token the page needs.
    #[test]
    fn the_dashboard_gets_a_real_servers_links() {
        let served = super::serve_for_dashboard(cctop_ui::ServeRequest {
            tunnel: false,
            rotate: false,
            plan: cctop_core::pricing::Plan::Retail,
            hosts: Vec::new(),
        })
        .expect("a loopback server");
        assert!(
            served.local.starts_with("http://127.0.0.1:"),
            "{}",
            served.local
        );
        let token = served.local.split_once("?t=").map(|(_, token)| token);
        // 32 characters, which the dashboard's own tests stand in when they
        // draw the panel's QR code without starting a server.
        assert_eq!(
            token.map(str::len),
            Some(32),
            "no token in {}",
            served.local
        );
        assert!(served.public.is_none());
        assert!(served.actions);
        assert_eq!(served.best(), served.local);
    }
}
