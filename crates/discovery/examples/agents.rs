//! The running coding agents on this machine, one per line.
//!
//! `cargo run -p cctop-discovery --example agents`

fn main() {
    for agent in cctop_discovery::agents() {
        let Some(pid) = agent.pid else { continue };
        println!(
            "{:<15} {:>7}  {:<16} {}  {}",
            agent.provider.display_name(),
            pid,
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
