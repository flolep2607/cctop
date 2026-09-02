# Adding a harness

[← all harnesses](README.md)

There is no trait to implement. A parser is a module in `src/session/` with a
handful of free functions, and the rest of the work is registering it at the
places that match on `Provider` — of which there are more than you would guess,
because most of them exist to keep an unsupported capability reading as
"unavailable" rather than as zero.

The order below is roughly the order in which things start working.

## 1. The provider

`Provider` in [`src/pricing.rs`](../../src/pricing.rs). Add the variant, its
`as_str` (that string is the stable key: it prefixes `Session::key`, and
`fleet.rs` parses it back when reading another machine's rows over ssh), and
`records_tool_outcomes`.

Answer that last one honestly. `false` means the transcript *cannot* say a call
failed, and it is what makes `ERR%` print `─` instead of a reassuring `0%`. A
harness that records outcomes badly is still `true`; a harness that records none
is `false`.

## 2. Where its data lives

[`src/config.rs`](../../src/config.rs). A `LazyLock<PathBuf>` per root, reading
an environment override first and falling back to the platform convention.
Follow the existing spelling: the override names the same directory the harness
itself would take, so a user who has already moved their sessions does not have
to tell cctop twice.

Then a `*_roots()` function through `roots_across_homes`, so `$CCTOP_HOMES` and
a root sweep find the other users' copies too. Anything that reads the single
`LazyLock` directly will silently ignore them.

## 3. The parser

A new `src/session/<name>.rs`. The functions the rest of the code will call:

```rust
pub fn list_sessions() -> Vec<Session>;                    // discovery
pub fn extract(path: &Path) -> SessionData;                // the numbers
pub fn extract_context(session: &Session) -> Option<ContextUsage>;  // optional
pub fn extract_last_tool(session: &Session) -> String;              // optional
pub fn delete(session: &Session) -> std::io::Result<()>;
```

Two of the seven take a session id as well as a path — `opencode::extract(path,
id)` and `windsurf::extract(path, id)` — because their file holds every session
at once. That is the only shape variation.

What each one owes:

- **`list_sessions`** must be cheap. It runs on every full walk, over every
  session that has ever existed on the machine, so it reads a bounded head and
  tail rather than whole files (`extract::read_first_lines`,
  `read_last_lines`), and caches what cannot change in a `STATIC_CACHE`. Pi is
  the one exception, and its page says why.
- **`extract`** may be expensive; it is cached and fanned out across cores. It
  must return `SessionData::error` rather than partial totals when a read
  fails, because the cache refuses to persist an errored extraction and a
  fabricated `$0.00` would be indistinguishable from a free plan.
- **`extract_context`** returns `None` where the harness records no window, and
  that must stay `None` rather than a guess: the denominator sets every
  percentage in the UI and where the auto-compaction marker sits.
- **`delete`** may refuse. Windsurf's returns `Unsupported` with a message
  saying to delete it from the editor.

For JSONL, use `extract::for_each_jsonl` rather than reading lines yourself —
it handles oversized lines, salvaging their `usage` instead of dropping the
turn's cost, and it skips the truncated tail a crashed writer leaves. Summarise
tool arguments through `extract::tool_detail`, and rename your harness's tool
names into the common vocabulary at parse time, the way Codex's
`normalise_exec_tool` does; one activity renderer serves all seven.

Where the harness reports a cost of its own, price it through `FallbackRates`
when it reports zero against non-zero tokens. Every harness that supports
arbitrary providers writes `0` for the ones it has no rates for, and that is not
the same as free.

## 4. Wiring it in

| File | What to add |
|---|---|
| `session/mod.rs` | the `rayon::join` in `list_all`; `effective_mtime_ms`; `Session::resume_argv`; `is_waiting_for_input_event`; `Surface::label` |
| `cache.rs` | `disk_key` (only if the file does not back one session alone), the `extract` dispatch, and the trace span name |
| `loader.rs` | `read_tail`'s three dispatches, plus any provider-specific liveness inference |
| `proc.rs` | `could_be_agent`, the per-provider process test, and how a session id appears on the command line |
| `doctor.rs` | a row in `sources()`, so a missing directory reports as "not installed" rather than as zero sessions |
| `access.rs` | rules files, config files, and the `hook::Harness` mapping if there is one |
| `session/search.rs` | how `/` searches its transcripts — the default line scan, or a query if the store is shared |
| `serve/chat.rs` | a conversation reader, if you want the browser view; Claude and Codex are the only two today, and everything else says so rather than looking empty |
| `cli.rs` | the `--list` grouping and the `--json` account block |
| `hook.rs` | `Harness` and its `configs`, if the harness has hooks to install into |

`ui/table.rs` has a test asserting every provider appears in the empty state.
It will fail until you add yours, which is the intended reminder.

## 5. What to document

- The README's capability table. A `─` there is a claim about the *harness*, not
  about cctop, so it needs to be true.
- [`docs/costs.md`](../costs.md) — the path table and the environment-override
  list.
- [`docs/troubleshooting.md`](../troubleshooting.md) — the same override list,
  under "My sessions are missing".
- A page in this directory, and a row in its index. Say what you could not
  extract and why; that is the half a future reader cannot reconstruct from the
  code, because absent code leaves no trace.

Pricing needs nothing if the harness reports its own costs or its models are in
LiteLLM. A built-in rate table is only worth adding for models LiteLLM lags on.

## 6. Tests

Each parser's `mod tests` writes a fixture into `std::env::temp_dir()`, parses
it, and removes it. Keep to that: it is fast, needs no fixture directory in the
crate, and `cargo publish` has a size limit.

Write the test for the trap, not for the happy path. The existing ones are
almost all regressions — cached input not double-billed, a `$set` patch reading
identically to the whole-file shape, a batched `exec` counting more than one
call — and each names in its doc comment what went wrong.

Two platform rules apply, and both have broken CI before: a fixture filename
containing `|`, `:`, `*` or `?` needs `#[cfg(unix)]`, and a path from
`tempfile::tempdir()` must be canonicalised before being compared against one a
watcher reported, because macOS resolves symlinks. If your parser holds a file
handle — as the SQLite ones do — give the tests a way to close it, or Windows
will refuse to delete the fixture.

Then run the gate the way CI does, from [CLAUDE.md](../../CLAUDE.md).
