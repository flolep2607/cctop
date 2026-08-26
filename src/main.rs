mod access;
mod alias;
mod attach;
mod cache;
mod cli;
mod collide;
mod config;
mod doctor;
mod fleet;
mod handoff;
mod hook;
mod inject;
mod loader;
mod mcp;
mod notify;
mod pricing;
mod proc;
mod quota;
mod serve;
mod session;
#[cfg(unix)]
mod shim;
// The tabs and their agents are unix-only, but the code that opens them is not
// cfg'd apart — so Windows gets the same surface with nothing behind it rather
// than a cfg on every call site. See `shim_stub` for the bargain.
#[cfg(not(unix))]
#[path = "shim_stub.rs"]
mod shim;
mod tmux;
mod trace;
mod ui;
mod update;
mod util;
mod watch;

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
    if std::env::args().nth(1).as_deref() == Some("hook") {
        let argv: Vec<String> = std::env::args().collect();
        std::process::exit(hook::emit(&argv[2..]));
    }

    // `cctop doctor` is intercepted here for the same reason `run` and `attach`
    // are: cctop takes no positionals, so clap would answer a bare word with a
    // usage error. Before the `is_command` check below, so a stray `doctor`
    // binary on PATH cannot shadow it.
    //
    // Every platform. The checks that are unix-only say so individually; the
    // question "why can cctop not see my sessions" is not unix-only at all.
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).map(String::as_str) == Some("doctor") {
            std::process::exit(doctor::run(&argv[2..]));
        }
    }

    // `cctop serve` is intercepted alongside `doctor`, and for the same reason:
    // it is a bare word, and cctop has no positionals for clap to read one as.
    // Every platform — a socket and a browser are not unix-only.
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).map(String::as_str) == Some("serve") {
            std::process::exit(serve::run(&argv[2..])?);
        }
    }

    #[cfg(unix)]
    let mut agent: Option<Vec<String>> = None;
    #[cfg(unix)]
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

    #[cfg(unix)]
    // Everything after the agent's name belongs to the agent, so clap must not
    // be shown it; the UI wrapped around it runs on its defaults.
    let args = match agent {
        Some(_) => cli::Args::parse_from(["cctop"]),
        None => cli::Args::parse(),
    };
    #[cfg(not(unix))]
    let args = cli::Args::parse();

    // Before anything else measurable happens, and in particular before the
    // caches are touched: a trace that starts after the slow part is no trace.
    if args.trace.is_some() {
        trace::enable();
    }

    if args.update {
        return update::run(false);
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

    if args.clear_cache && cache::clear_session_cache()? {
        eprintln!("Cleared cctop session extraction cache.");
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

    if args.list || args.json {
        // Non-interactive modes need pricing before they can print anything, so
        // fetch synchronously. The TUI refreshes it on a background thread.
        pricing::refresh_pricing_blocking();

        let mut loader = loader::Loader::new();
        let sessions = loader.load(args.plan);
        if args.json {
            cli::run_json(&sessions, args.plan, &loader)?;
        } else {
            cli::run_list(&sessions, args.plan);
        }
        loader.store().save();
        finish_trace(&args);
        return Ok(());
    }

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        // With no terminal there is no UI to wrap the agent in, but there is
        // still an agent to run: `claude` is aliased to this, and a pipeline or
        // a script must not be told to use --json instead.
        #[cfg(unix)]
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
    #[cfg(unix)]
    let launching_agent = agent.is_some();
    #[cfg(not(unix))]
    let launching_agent = false;
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
    let auto_update = !args.no_auto_update && !launching_agent && prefs.auto_update;
    update::auto_at_startup(auto_update, &mut prefs);

    // After the update offer: a process that is about to be replaced by a newer
    // one has no business asking a question the new one would have to ask again.
    if !launching_agent && std::env::var_os("CI").is_none() {
        alias::ask_on_first_run(&mut prefs);
    }

    // Load whatever pricing is already cached so the first frame isn't zeroed
    // while the network fetch is still in flight.
    pricing::load_cached_pricing();

    // Started before the UI so a failure to launch prints as an ordinary error
    // rather than from inside the alternate screen.
    #[cfg(unix)]
    let hosted = agent.map(|agent| shim::host(&agent, None)).transpose()?;
    #[cfg(not(unix))]
    let hosted = None;

    let code = ui::run(&args, hosted)?;
    // After the UI is down, so the message is not painted over by the alternate
    // screen being restored.
    finish_trace(&args);
    std::process::exit(code)
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
