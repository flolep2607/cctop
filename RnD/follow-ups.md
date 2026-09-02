# Follow-ups

Things found while researching that are not themselves research: real defects,
gaps and inaccuracies noticed in passing. Each one says where it came from, so
a reader can judge how well it was verified.

Nothing here has been acted on unless it says so.

---

## From writing `docs/providers/` (2026-09-02)

Found by reading all seven parsers in `src/session/` closely enough to
document them.

### 1. Codex `apply_patch` loses every file after the first

`distil_recent_writes` recovers only the first file of a multi-file patch. The
`!` collision column is built from recent writes, so a Codex session holding
three files is reported as holding one — and the two it does not report are
exactly the ones another agent could overwrite without warning. That is the
column's whole purpose, so this is a correctness gap rather than a cosmetic one.

The fix is available: the matching `function_call_output` lists the rest.

**Severity:** highest of these four. It makes a safety feature quietly
incomplete.

### 2. `docs/costs.md` overstates what "reported" means

The page says OpenCode's and Pi's costs are read directly from the harness.
True, but incomplete: both harnesses write `cost: 0` for a provider they have
no rates for, and cctop falls back to LiteLLM when it sees a zero cost against
non-zero tokens. As written the page reads as though a reported figure is
always taken at face value.

One sentence fixes it. Left alone during the docs work only to avoid three
agents editing at once.

### 3. Windsurf's parser has never met a Windsurf install

`src/session/windsurf.rs` carries a `ponytail:` saying it was written against
the documented `ItemTable` layout rather than a live database. The settings key
and the bubble field names are informed guesses that fail closed — wrong guesses
produce no rows and no tools rather than wrong ones — but it means the README's
`Tools ✓` for Windsurf may silently be a `─` on a real machine.

The single highest-value unknown across all seven parsers, and it takes one
person with a Windsurf install and `sqlite3` to close.

### 4. Two capability-table cells are conditional, not absolute

Not contradictions — the README table is accurate — but neither cell is
unconditional, and both are now explained on their provider page:

- **OpenCode `Context ✓`** holds only when LiteLLM knows the model. OpenCode
  records no window size of its own, so `context_of` returns `None` without a
  `max_input_tokens` entry and the CTX cell is blank rather than a percentage.
- **Windsurf `Tools ✓`** is provisional, for the reason above.

### 5. `README.md` should link the new directory

The "Two directories of other people's documentation" paragraph is the natural
place, and the capability table's pointer to the cost page could point at
`docs/providers/` instead — that is now where each `─` is explained. Deliberately
not done, since the README's shape is an editorial choice rather than a
mechanical one.

---

## Parser facts worth not re-learning

Not defects — things the parsers already know that cost real effort to
establish, and that a future change could quietly break.

- **Claude writes one assistant record per `tool_use` block**, not one record
  holding several, all sharing a `requestId`. Counting blocks within a record
  therefore answers "1" for every call in a parallel fan-out, so anything
  dividing a turn's tokens by that count is wrong for the whole fan-out.
- **A Claude resume forks the transcript**, so the UUID on a live process's
  command line names a conversation that stopped the moment that process
  started. Matching it literally hands the live agent to the dead row — hence
  `launch_id` and `proc::resumed_key`.
- **Codex's `exec` payload is JavaScript source, not JSON.** Before
  `quote_bare_keys`, roughly two thirds of Codex tool calls rendered with no
  arguments at all. The parser also handles `Promise.all` batches and resolves
  `const patch = "…"` bindings.
- **Gemini's `cached` sits inside `input` rather than adding to it**, and
  `total = input + output + thoughts + tool`. Verified against 605 recorded
  turns; the reasoning is already in a doc comment in `gemini.rs`.
- **`CHARS_PER_TOKEN = 2.75` was fitted against 167 sessions**, not assumed. It
  is the only reason a context breakdown exists at all.
- **OpenCode's connection pooling is not premature optimisation.** Per-session
  opens cost 240,000 context switches and nine seconds of kernel time per walk
  against a 555 MB database.

---

## From building the corpus fingerprint (2026-09-02)

### 6. `watch::roots()` omits Gemini and Windsurf

`src/watch.rs:131` builds its watch list from Claude, Codex, Cursor, Pi,
OpenCode and the two Claude-for-Mac roots. Gemini CLI and Windsurf are not in
it, so a session started in either appears only on the next periodic walk and
never on a create event — the thing the watcher exists to make immediate.

It may be deliberate and undocumented, but it does not read that way, and it was
found the hard way: the fingerprint pass nearly reused this list, which would
have made a changed Gemini transcript invalidate nothing at all. `fingerprint.rs`
therefore keeps its own list and says why, rather than widening this one —
widening it changes watch behaviour, which is a separate decision from making
the cache correct.

**Severity:** small but real, and cheap to fix if it is not deliberate.

### 7. `$CCTOP_SETTLE_MS` is undocumented

It joins the other `CCTOP_*` knobs and should be listed beside them in
`src/cli.rs`'s help.

### 8. `Session` does not derive `Serialize`

Nor do `ProcInfo`, `Remote`, `MacMeta` or `Subagent`. That is what stops the
fingerprint snapshot from being persisted to disk and reused *across* processes,
which is the form that would help `--json` and `--list`. Deriving them is
mechanical; whether the persisted snapshot is worth having is the open question,
since the one-shot paths walk once and exit and nothing currently polls them in
a loop.
