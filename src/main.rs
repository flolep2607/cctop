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
mod ui;
mod update;
mod util;
mod watch;

use clap::Parser;
use std::io::IsTerminal;

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
    if !launching_agent && std::env::var_os("CI").is_none() {
        alias::ask_on_first_run(&mut cache::UiPrefs::load());
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

    std::process::exit(ui::run(&args, hosted)?)
}
