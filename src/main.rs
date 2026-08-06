mod alias;
mod cache;
mod cli;
mod config;
mod inject;
mod loader;
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
    #[cfg(unix)]
    {
        let argv: Vec<String> = std::env::args().collect();
        let command = match argv.get(1).map(String::as_str) {
            Some("run") => Some(&argv[2..]),
            Some(word) if !word.starts_with('-') && shim::is_command(word) => Some(&argv[1..]),
            _ => None,
        };
        if let Some(command) = command {
            std::process::exit(shim::run(command)?);
        }
    }

    let args = cli::Args::parse();

    if args.update {
        return update::run(false);
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
        anyhow::bail!("the interactive UI needs a TTY; use --list or --json instead");
    }

    alias::install_once(&mut cache::UiPrefs::load());

    // Load whatever pricing is already cached so the first frame isn't zeroed
    // while the network fetch is still in flight.
    pricing::load_cached_pricing();
    ui::run(&args)
}
