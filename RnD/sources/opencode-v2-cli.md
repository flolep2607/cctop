# OpenCode V2: CLI

Fetched from <https://opencode.ai/v2/docs/cli/> on 2026-09-30, for the cctop OpenCode
hook work. Unedited apart from dropping the site's navigation around the
article body.

---

# Intro

Run the CLI in a project to open the full-screen terminal interface:

``` astro-code astro-code-themes github-light github-dark
opencode
```

Pass a directory to work in a different project:

``` astro-code astro-code-themes github-light github-dark
opencode ~/code/my-project
```

Suggested terminals:

  - [Ghostty](https://ghostty.org)
  - [WezTerm](https://wezterm.org)
  - [Alacritty](https://alacritty.org)
  - [Kitty](https://sw.kovidgoyal.net/kitty/)

Truecolor support gives themes their most accurate colors; terminals without it use an approximation of the available palette.

## Automation

Use `opencode run` to submit a prompt without opening the interactive interface. It is designed for scripts, CI jobs, and other workflows that need model output directly in the terminal.

``` astro-code astro-code-themes github-light github-dark
opencode run "Explain this repository"
```

## Mini

Use `opencode mini` to start OpenCode’s minimal interactive interface instead of the full-screen TUI.

``` astro-code astro-code-themes github-light github-dark
opencode mini
```

Run `opencode mini --help` to see its session, model, agent, prompt, and replay options.

## Background service

By default, OpenCode discovers or starts one shared background server for your user account. Every local OpenCode client connects to that server, which owns sessions, configuration, integrations, permissions, and tool execution.

Use `--standalone` to run with a private server, or `--server` to connect to a specific server URL:

``` astro-code astro-code-themes github-light github-dark
opencode --standalone
opencode --server http://localhost:4096
```

To make private servers the default for CLI commands instead of starting the shared service, disable it:

``` astro-code astro-code-themes github-light github-dark
opencode service set disabled true
opencode
```

This stops the running background service. `--server` still connects to an explicitly selected server; `opencode service start` can still start the shared service explicitly. To return to automatic service use, run `opencode service unset disabled`. Pairing requires the shared service and is unavailable while it is disabled.

See [Troubleshooting](/v2/docs/troubleshooting) for shared service diagnostics and the [API reference](/v2/docs/api) for server endpoints.

## Paths

Print a specific local path for use with other tools:

``` astro-code astro-code-themes github-light github-dark
opencode debug paths db
sqlite3 "$(opencode debug paths db)"
```

The optional selector accepts `db`, `home`, `data`, `config`, `cache`, `state`, `tmp`, `bin`, `log`, or `repos` and prints only the path and a newline. The database path respects the release channel and `OPENCODE_DB`; relative database overrides resolve under the data directory, and `:memory:` is printed as-is. This command does not start a server or open the database.

Omit the selector to show all paths with labels:

``` astro-code astro-code-themes github-light github-dark
opencode debug paths
```

## Uninstall

Preview the files and installation that will be removed:

``` astro-code astro-code-themes github-light github-dark
opencode uninstall --dry-run
```

Run `opencode uninstall` to confirm removal. OpenCode stops registered background services and persistent terminals before removing global data, cache, configuration, and state. These directories are shared by OpenCode versions and channels.

To retain configuration and session data:

``` astro-code astro-code-themes github-light github-dark
opencode uninstall --keep-config --keep-data
```

  - `--keep-config` (`-c`) retains configuration files.
  - `--keep-data` (`-d`) retains session data and snapshots. Cache and state are still removed.
  - `--force` (`-f`) skips confirmation; use it for noninteractive removal.
  - Package-manager installations use the detected package manager. Curl installations print the final command to remove the executable manually.
