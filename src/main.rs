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

use clap::Parser;
use std::io::IsTerminal;

fn main() -> anyhow::Result<()> {
    // `cctop run <agent> …` is handled before clap so the agent's own flags are
    // never mistaken for cctop's.
    #[cfg(unix)]
    {
        let argv: Vec<String> = std::env::args().collect();
        if argv.get(1).is_some_and(|a| a == "run") {
            std::process::exit(shim::run(&argv[2..])?);
        }
    }

    let args = cli::Args::parse();

    if args.update {
        return update::run(false);
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

    // Load whatever pricing is already cached so the first frame isn't zeroed
    // while the network fetch is still in flight.
    pricing::load_cached_pricing();
    ui::run(&args)
}
