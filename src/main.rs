mod alias;
mod attach;
mod cache;
mod cli;
mod config;
mod hook;
mod inject;
mod loader;
mod notify;
mod pricing;
mod proc;
mod quota;
mod session;
#[cfg(unix)]
mod shim;
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
    #[cfg(unix)]
    let mut agent: Option<Vec<String>> = None;
    #[cfg(unix)]
    {
        let argv: Vec<String> = std::env::args().collect();
        // `cctop attach` puts a running agent on this terminal directly, with no
        // UI around it. Handled here for the same reason as `run`: it takes a
        // positional, and cctop otherwise has none.
        // `cctop hook` is spawned by the agent itself, many times a session.
        // It is answered before anything else is set up — no config, no
        // pricing, no cache — because the agent is blocked until it returns.
        if argv.get(1).map(String::as_str) == Some("hook") {
            std::process::exit(hook::emit(&argv[2..]));
        }
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
        eprintln!(
            "{}",
            match installing {
                true => hook::install(&scope)?,
                false => hook::remove(&scope)?,
            }
        );
        // Codex is configured machine-wide or not at all, so it rides along with
        // the user scope only. A failure there — someone else's notify program
        // already in the slot — is reported rather than fatal: it must not undo
        // the Claude Code half that already succeeded.
        if scope == hook::Scope::User {
            match if installing {
                hook::codex_install()
            } else {
                hook::codex_remove()
            } {
                Ok(what) => eprintln!("{what}"),
                Err(e) => eprintln!("Codex: {e}"),
            }
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

    alias::install_once(&mut cache::UiPrefs::load());

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
