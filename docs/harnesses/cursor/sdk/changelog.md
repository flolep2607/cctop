# SDK Changelog

The latest features, improvements, and fixes shipping to the Cursor SDK, covering `@cursor/sdk` on npm and `cursor-sdk` on PyPI.

## 1.0.27

- **Restrict the agent's toolset.** `tools` allowlists the built-in tools offered to the model (`[]` means text-only), and `disallowedTools` removes tools while keeping the rest. Both take public names like `"read"` or capability groups like `"shell"` and `"mcp"`, in TypeScript and Python (`tools`, `disallowed_tools`). Local agents only for now, and not persisted across `resume`.
- **Log in from the browser in TypeScript.** `Cursor.auth.login()` opens a browser login, mints an API key, and stores it in `~/.cursor/sdk/auth.json`; `Cursor.auth.status()` and `Cursor.auth.logout()` round it out. After login, `Agent.create()` and the `Cursor.*` reads work without `apiKey` or `CURSOR_API_KEY`.
- **Usage and cost for local agents.** `agent.getUsage()` in TypeScript and `agent.get_usage()` in Python now work for local agents too, returning a per-turn breakdown. Pass a `runId` from a previous result to narrow to one turn.
- **Open PRs as the Cursor GitHub App.** `cloud.openAsCursorGithubApp` in TypeScript and `open_as_cursor_github_app` in Python control PR authorship. Service-account keys default to the app; user keys default to the key's owner.
- **Multi-root local workspaces.** Pass `local.dirs` to load rules, skills, and project context from several folders; `cwd` stays the single primary working directory. Replaces the `cwd` array form, which only ever used the first entry.
- **Clearer Python errors.** Failures that previously surfaced as a bare "internal error" now carry the underlying message and code.
- **Admin command denylists apply to local runs.** Shell commands matching your team's admin denylist are rejected with a policy message before they execute, including on paths that skip approval prompts.

## 1.0.26

- **Warm up a local workspace before the first send.** `platform.prewarmLocalWorkspace(options)` resolves rules, skills, MCP servers, and ignore files ahead of time, so the first `send()` against that workspace starts immediately. It returns a release function to call on shutdown.
- **Control how long workspace scans stay cached.** `configureCursorSdk({ local: { workspaceScanCacheTtlMs } })` sets the cache lifetime for workspace scans, and the `CURSOR_RIPWALK_CACHE_TTL_MS` environment variable sets the same value for hosted deployments. Long-lived servers on stable checkouts can now skip repeated re-scans.
- **Custom tools run without approval prompts.** Host-defined tools passed via `customTools` no longer fail with an interactive-approval error on sandboxed or auto-review local runs. Deny rules and sandbox limits still apply.
- **Signed macOS binaries.** The `@cursor/sdk` macOS platform packages now ship code-signed binaries, so Gatekeeper and endpoint security tools no longer block them.
- **Cleaner Python exception hierarchy.** `PermissionDeniedError`, `BadRequestError`, and `InternalServerError` now inherit directly from `CursorSDKError` instead of `AuthenticationError`, `ConfigurationError`, and `NetworkError`, so `except` blocks catch what their names say.
- **Fixed intermittent startup failures in Python.** Roughly 1 in 64 agent launches failed before reaching the first send. Launches are now reliable.

## 1.0.25

- **Billed usage and cost on demand.** `agent.getUsage()` in TypeScript and `agent.get_usage()` in Python return token usage, billed cost, and a per-run breakdown for cloud agents, and `Agent.getUsage(agentId)` works without a handle. Cost is server-derived, includes discounts, and settles shortly after a run ends. Cloud-only for now; local runs throw a typed configuration error.

## 1.0.24

- **TypeScript and Python now release together.** Starting with 1.0.24, `@cursor/sdk` on npm and `cursor-sdk` on PyPI ship from the same release and share a version number. Python releases no longer trail TypeScript.
- **More reliable long-running streams.** Streaming responses on heavy runs no longer drop mid-stream, which previously surfaced as network errors in Python clients on long turns.

## 1.0.23

- **Per-send environment variables for cloud runs.** Pass `send(prompt, { cloud: { envVars } })` to scope env vars to a single run, including the first send that creates the agent. `Agent.create({ cloud: { envVars } })` still sets agent-scoped defaults.
- **Error details on failed runs.** Local and cloud runs that fail now expose a structured error with `message` and `code` fields, so you can tell what went wrong without parsing logs. `run.wait()` behaves the same as before.
- **Token usage in Python.** Run streams emit typed `usage` messages with per-turn token counts, and cumulative totals are available on `run.usage` and `RunResult.usage`, matching TypeScript from 1.0.22.
- **Sturdier local run history.** Run history on disk now survives interrupted writes, fixing a class of failures where a crashed process left runs that could not be resumed.
- **Fixed streaming stalls on Bun.** Run streams under Bun no longer stall on long responses.

## 1.0.22

- **Token usage on every run.** Local runs emit per-turn `usage` events on `run.stream()` and cumulative totals on `run.wait()`. Cloud runs surface the same usage on their stream and `wait()` results, and totals persist for detached local handles so a process that reattaches still gets them.

## 1.0.21

- **Run agents under Bun.** `agent.send()` now works under Bun with the same behavior as Node. This also fixes fresh Node installs that could miss a required dependency.
- **Friendlier runtime names in Python.** List APIs and `get_run` accept `runtime="cloud"`, `"local"`, and `"auto"`, matching the documented values.

## 1.0.20

- **The SDK imports cleanly under Bun.** Importing `@cursor/sdk` no longer crashes under Bun. Running agents under Bun follows in 1.0.21.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
