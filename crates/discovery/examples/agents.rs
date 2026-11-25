//! The running coding agents on this machine, one line per process, and the
//! agents whose sessions could not be read.
//!
//! `cargo run -p cctop-discovery --example agents`

fn main() {
    let scan = cctop_discovery::scan();
    for agent in &scan.agents {
        // A session open in two terminals is one agent with two processes.
        for process in &agent.processes {
            println!(
                "{:<15} {:>7}  {:<10} {:<6} {:<16} {}  {}",
                agent.provider.display_name(),
                process.pid,
                process
                    .tty
                    .as_deref()
                    .map(|t| t.display().to_string())
                    .unwrap_or_default(),
                process.tmux.as_ref().map(|t| t.pane.as_str()).unwrap_or(""),
                agent.state.map(|s| format!("{s:?}")).unwrap_or_default(),
                agent
                    .cwd
                    .as_deref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                agent.title.as_deref().unwrap_or(""),
            );
        }
    }
    for error in &scan.errors {
        eprintln!("{error}");
    }
}
