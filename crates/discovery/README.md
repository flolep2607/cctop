# cctop-discovery

The coding agents on this machine — Claude Code, Codex, Cursor, Devin, Gemini
CLI, OpenCode, Pi, Mistral Vibe, Windsurf and the Antigravity CLI — as
[cctop](https://github.com/flolep2607/cctop) finds them: every session, the
process running it, its working directory, its title and what it is doing.

It is the part of cctop that a panel applet, a status bar or a notifier needs,
without cctop's terminal, web server, TLS or multiplexer.

```rust
for agent in cctop_discovery::agents() {
    let Some(pid) = agent.pid else { continue }; // running ones only
    println!("{} {pid} {:?} {:?}", agent.provider.display_name(), agent.state, agent.cwd);
}
```

`cargo run -p cctop-discovery --example agents` prints the running ones.

- One plain blocking call, no async runtime, no daemon, nothing else to
  install: run it on a worker thread.
- `watch_paths()` lists the directories to watch with inotify instead of
  listing on a timer.
- A session open in two terminals is one `Agent` with two `processes`, each
  with its tty and tmux pane.
- `scan()` returns the same list plus an error per agent that could not be
  read, so one broken agent does not hide the others.

Linux only. It reads the agents' own files and `/proc`, and never writes. The
crate docs say which agents can be matched to a process and why the *asking*
state is mostly not visible from files alone.

It is 0.x: `agents`, `scan`, `watch_paths`, the structs they return and the
enums are the interface. A new agent or state is a new enum variant, so it is a
breaking change and comes as a minor bump. Pin the minor you built against. Modules hidden from the docs are
cctop-core's and change without notice.
