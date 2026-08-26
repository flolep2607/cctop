# Cloud Agents API

### Public beta

The Cloud Agents API v1 is in public beta. APIs may change before general
availability.

The Cloud Agents API lets you programmatically launch and manage cloud agents that work on your repositories.

- The Cloud Agents API accepts both [Basic and Bearer authentication](https://cursor.com/docs/api.md#authentication). Generate a user API key from [Cursor Dashboard → API Keys](https://cursor.com/dashboard/api), or use a [service account API key](https://cursor.com/docs/account/enterprise/service-accounts.md).
- For details on authentication methods, rate limits, and best practices, see the [API Overview](https://cursor.com/docs/api.md).
- View the full [OpenAPI specification](/docs-static/cloud-agents-openapi.yaml) for detailed schemas and examples.
- Webhooks are coming soon. The legacy [v0 API](https://cursor.com/docs/cloud-agent/api/v0.md) still supports them — see [Webhooks](https://cursor.com/docs/cloud-agent/api/webhooks.md).

### Migrating from v0?

This API splits work into a durable agent plus per-prompt runs, replacing the flatter v0 surface. The legacy [v0 reference](https://cursor.com/docs/cloud-agent/api/v0.md) remains available.

## Endpoints

### Create An Agent

/v1/agents

Create a Cloud Agent and immediately enqueue its initial run. The response returns both the durable `agent` and the initial `run`.

#### Request Body

`prompt` object (required)

The task prompt for the agent, including optional images.

`prompt.text` string (required)

The instruction text for the agent.

`prompt.images` array (optional)

Image inputs for the prompt. Each entry must include either `data` (base64-encoded bytes with a required `mimeType`) or `url` (an http or https URL that Cursor fetches). Maximum 5 images, 15 MB each. Supported MIME types: `image/png`, `image/jpeg`, `image/gif`, `image/webp`.

`model` object (optional)

Model selection. Omit this field to use the configured default. When omitted, Cursor resolves your user default model, then your team default model, then a system default.

`model.id` string (required if `model` provided)

An explicit model ID returned by `GET /v1/models` (for example, `claude-4-sonnet-thinking`).

`model.params` array (optional)

Per-model parameters to apply to the run, such as reasoning effort or context window size. Each item has an `id` and `value`. Use only parameters supported by the selected model — call `GET /v1/models` to discover the valid `id`/`params` combinations.

`name` string (optional)

Display name for the agent. Maximum 100 characters. When omitted, Cursor auto-derives a name from the prompt.

`env` object (optional)

Execution environment target. Use a named `cloud` environment, or route to a self-hosted `pool` or `machine`. Mutually exclusive with explicit `repos` when selecting a named Cursor-hosted environment.

`env.type` string (required if `env` provided)

Execution environment type. `cloud` uses Cursor-hosted VMs; `pool` and `machine` route to self-hosted workers.

`env.name` string (optional)

Named Cursor-hosted environment, self-hosted pool, or self-hosted machine name. For `env.type: "pool"`, this is the pool name (defaults to `default` when omitted). An unknown pool name returns `400` instead of queueing forever.

`repos` array (optional)

Repository configuration. Mutually exclusive with a named cloud environment. Omit both `repos` and `env` to start a no-repo agent. You can also omit `repos` when `env.type` is `pool` to target a [repo-less pool](https://cursor.com/docs/cloud-agent/self-hosted-guides/pool.md#repo-less-pools). Maximum 20 repositories.

`repos[0].url` string (required)

GitHub repository URL (for example, `https://github.com/your-org/your-repo`). Required on every repo entry, including when `prUrl` is provided.

`repos[0].startingRef` string (optional)

Branch name or commit SHA to use as the starting point. Ignored when `prUrl` is provided.

`repos[0].prUrl` string (optional)

GitHub pull request URL. When provided, the agent works on this PR's repository and branches; `startingRef` is ignored. `url` must still be set on the same `repos` entry.

`workOnCurrentBranch` boolean (optional, default: false)

When `false` (the default), Cursor pushes commits to a new auto-generated branch (`cursor/...`) based on `repos[0].startingRef` (or the PR base ref when `prUrl` is set). When `true`, Cursor pushes directly to that starting ref — for a non-PR create, that's the branch you passed in `startingRef`; for a `prUrl` create, that's the PR's head branch. The branch the agent pushed shows up in the agent's `git.branches[]`.

`autoCreatePR` boolean (optional)

Whether Cursor should open a pull request when the run completes.

`skipReviewerRequest` boolean (optional)

Whether to skip requesting the user as a reviewer when Cursor opens a PR. Only applies when `autoCreatePR` is `true`.

`envVars` object (optional)

Session-scoped environment variables for the cloud agent. Values are encrypted at rest, injected into the agent's shell, and deleted with the agent. Maximum 50 entries; names up to 255 bytes (can't start with `CURSOR_`), values up to 4096 bytes. Cannot be combined with a client-supplied `agentId`.

**Beta:** `envVars` is rolling out. If it isn't enabled for your account yet, the field is silently ignored on create rather than failing the request — verify the values are present by inspecting the agent shell on a first run before relying on them in production.

`mcpServers` array (optional)

Inline MCP server definitions available to the agent. Maximum 50 servers. Remote servers support `headers` or OAuth `auth`; stdio servers run inside the cloud VM and can receive `env`. Server names must be unique.

`mcpServers[0].name` string (required)

The MCP server name exposed to the agent.

`mcpServers[0].type` string (optional)

Transport type: `http`, `sse`, or `stdio`. Defaults to `http` for remote servers with `url`, and `stdio` for servers with `command`.

`mcpServers[0].url` string (required for remote MCP)

HTTP or HTTPS URL for a remote MCP server. URLs with username or password are not allowed.

`mcpServers[0].command` string (required for stdio MCP)

Command to start a stdio MCP server inside the cloud agent VM. Use `args` and `env` for arguments and runtime secrets.

`customSubagents` array (optional)

Define custom subagents the main agent can delegate to during the run. Maximum 20 subagents. Each entry requires `name`, `description`, and `prompt`, plus an optional `model` (model ID string, `ModelSelection` object, or `"inherit"`). Names must be unique and cannot collide with built-ins (`explore`, `debug`, `shell`, `computerUse`, etc.).

`mode` string (optional, default: agent)

Initial conversation mode for the agent's first run. `plan` explores and drafts a plan before coding ([Plan mode](https://cursor.com/help/ai-features/plan-mode.md)); `agent` implements changes directly.

`agentId` string (optional)

Client-supplied agent identifier in the form `bc-<uuid>`. Useful for idempotent create flows — re-POSTing the same `agentId` returns `409 agent_id_conflict` rather than creating a duplicate. Cannot be combined with `envVars`; omit `agentId` so the server mints one when you need session secrets.

```bash
curl --request POST \
  --url https://api.cursor.com/v1/agents \
  -u YOUR_API_KEY: \
  --header 'Content-Type: application/json' \
  --data '{
    "prompt": {
      "text": "Add a README with setup instructions"
    },
    "model": {
      "id": "composer-2",
      "params": [
        { "id": "fast", "value": "true" }
      ]
    },
    "repos": [
      {
        "url": "https://github.com/your-org/your-repo",
        "startingRef": "main"
      }
    ],
    "mcpServers": [
      {
        "name": "linear",
        "type": "http",
        "url": "https://mcp.linear.app/sse",
        "headers": {
          "Authorization": "Bearer YOUR_LINEAR_API_KEY"
        }
      },
      {
        "name": "github",
        "type": "stdio",
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-github"],
        "env": {
          "GITHUB_TOKEN": "YOUR_GITHUB_TOKEN"
        }
      }
    ],
    "autoCreatePR": true
  }'
```

Self-hosted pool (including repo-less):

```bash
curl --request POST \
  --url https://api.cursor.com/v1/agents \
  -u YOUR_API_KEY: \
  --header 'Content-Type: application/json' \
  --data '{
    "prompt": {
      "text": "Clone the payments service and add a health check"
    },
    "env": {
      "type": "pool",
      "name": "sandbox"
    }
  }'
```

**Response:**

```json
{
  "agent": {
    "id": "bc-00000000-0000-0000-0000-000000000001",
    "name": "Add README with setup instructions",
    "status": "ACTIVE",
    "env": {
      "type": "cloud"
    },
    "repos": [
      {
        "url": "https://github.com/your-org/your-repo",
        "startingRef": "main"
      }
    ],
    "workOnCurrentBranch": false,
    "autoCreatePR": true,
    "url": "https://cursor.com/agents/bc-00000000-0000-0000-0000-000000000001",
    "createdAt": "2026-04-13T18:30:00.000Z",
    "updatedAt": "2026-04-13T18:30:00.000Z",
    "latestRunId": "run-00000000-0000-0000-0000-000000000001"
  },
  "run": {
    "id": "run-00000000-0000-0000-0000-000000000001",
    "agentId": "bc-00000000-0000-0000-0000-000000000001",
    "status": "CREATING",
    "createdAt": "2026-04-13T18:30:00.000Z",
    "updatedAt": "2026-04-13T18:30:00.000Z"
  }
}
```

### List Agents

/v1/agents

List agents for the authenticated user, newest first.

#### Query Parameters

`limit` number (optional)

Number of agents to return. Default: 20, Max: 100.

`cursor` string (optional)

Pagination cursor from `nextCursor` on the previous response.

`prUrl` string (optional)

Filter agents by GitHub pull request URL.

`includeArchived` boolean (optional, default: true)

Whether to include archived agents in the response.

List items only include the durable identity fields. Call `GET /v1/agents/{id}` to load the full record (`repos`, `workOnCurrentBranch`, `autoCreatePR`, etc.).

`nextCursor` is **omitted** from the response when there are no more pages — it is not returned as `null`. Treat its absence as "no more results".

```bash
curl --request GET \
  --url 'https://api.cursor.com/v1/agents?limit=20' \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "items": [
    {
      "id": "bc-00000000-0000-0000-0000-000000000001",
      "name": "Add README with setup instructions",
      "status": "ACTIVE",
      "env": {
        "type": "cloud"
      },
      "url": "https://cursor.com/agents/bc-00000000-0000-0000-0000-000000000001",
      "createdAt": "2026-04-13T18:30:00.000Z",
      "updatedAt": "2026-04-13T18:45:00.000Z",
      "latestRunId": "run-00000000-0000-0000-0000-000000000001"
    }
  ],
  "nextCursor": "bc-00000000-0000-0000-0000-000000000002"
}
```

### Get An Agent

/v1/agents/

Retrieve durable metadata for an agent. Execution status lives on runs — fetch `latestRunId` and call [Get A Run](https://cursor.com/docs/cloud-agent/api/endpoints.md#get-a-run) to read run state.

#### Path Parameters

`id` string

Unique identifier for the agent (for example, `bc-00000000-0000-0000-0000-000000000001`).

```bash
curl --request GET \
  --url https://api.cursor.com/v1/agents/bc-00000000-0000-0000-0000-000000000001 \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "id": "bc-00000000-0000-0000-0000-000000000001",
  "name": "Add README with setup instructions",
  "status": "ACTIVE",
  "env": {
    "type": "cloud"
  },
  "repos": [
    {
      "url": "https://github.com/your-org/your-repo",
      "startingRef": "main"
    }
  ],
  "workOnCurrentBranch": false,
  "autoCreatePR": true,
  "url": "https://cursor.com/agents/bc-00000000-0000-0000-0000-000000000001",
  "createdAt": "2026-04-13T18:30:00.000Z",
  "updatedAt": "2026-04-13T18:30:00.000Z",
  "latestRunId": "run-00000000-0000-0000-0000-000000000001"
}
```

### Create A Run

/v1/agents//runs

Send a follow-up prompt to an existing active agent. The new run uses the agent's current conversation and workspace state.

Only one run can be active per agent. Calling this while another run is `CREATING` or `RUNNING` returns `409 agent_busy`. Wait for the existing run to terminate, or cancel it.

#### Path Parameters

`id` string

Unique identifier for the agent (for example, `bc-00000000-0000-0000-0000-000000000001`).

#### Request Body

`prompt` object (required)

The follow-up prompt, including optional images.

`prompt.text` string (required)

The follow-up instruction text.

`prompt.images` array (optional)

Image inputs for the follow-up. Each entry must include either `data` (base64-encoded bytes with a required `mimeType`) or `url`. Maximum 5 images, 15 MB each. Supported MIME types: `image/png`, `image/jpeg`, `image/gif`, `image/webp`.

`mcpServers` array (optional)

Inline MCP server definitions for this follow-up run. When provided, these replace any create-time inline MCP servers for this run. Omit to keep the agent's current MCP configuration.

`mode` string (optional)

Conversation mode override for this follow-up run: `agent` or `plan`. Omit to keep the conversation's current mode from prior runs.

```bash
curl --request POST \
  --url https://api.cursor.com/v1/agents/bc-00000000-0000-0000-0000-000000000001/runs \
  -u YOUR_API_KEY: \
  --header 'Content-Type: application/json' \
  --data '{
    "prompt": {
      "text": "Also add troubleshooting steps"
    },
    "mcpServers": [
      {
        "name": "docs",
        "type": "http",
        "url": "https://example.com/mcp"
      }
    ]
  }'
```

**Response:**

```json
{
  "run": {
    "id": "run-00000000-0000-0000-0000-000000000002",
    "agentId": "bc-00000000-0000-0000-0000-000000000001",
    "status": "CREATING",
    "createdAt": "2026-04-13T18:50:00.000Z",
    "updatedAt": "2026-04-13T18:50:00.000Z"
  }
}
```

### List Runs

/v1/agents//runs

List runs for an agent, newest first.

#### Path Parameters

`id` string

Unique identifier for the agent.

#### Query Parameters

`limit` number (optional)

Number of runs to return. Default: 20, Max: 100.

`cursor` string (optional)

Pagination cursor from `nextCursor` on the previous response.

```bash
curl --request GET \
  --url 'https://api.cursor.com/v1/agents/bc-00000000-0000-0000-0000-000000000001/runs?limit=20' \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "items": [
    {
      "id": "run-00000000-0000-0000-0000-000000000002",
      "agentId": "bc-00000000-0000-0000-0000-000000000001",
      "status": "RUNNING",
      "createdAt": "2026-04-13T18:50:00.000Z",
      "updatedAt": "2026-04-13T18:51:00.000Z",
      "git": {
        "branches": [
          {
            "repoUrl": "github.com/your-org/your-repo",
            "branch": "cursor/add-readme-a1b2"
          }
        ]
      }
    }
  ]
}
```

### Get A Run

/v1/agents//runs/

Retrieve status, timestamps, and (for terminal runs) the final result, duration, and pushed branches for a specific run.

#### Path Parameters

`id` string

Unique identifier for the agent.

`runId` string

Unique identifier for the run (for example, `run-00000000-0000-0000-0000-000000000001`).

#### Response Fields

The base run fields (`id`, `agentId`, `status`, `createdAt`, `updatedAt`) are always present. The following are populated as soon as data is available:

`durationMs` integer (terminal runs)

Wall-clock duration of the run in milliseconds, computed once the run reaches `FINISHED`, `ERROR`, `CANCELLED`, or `EXPIRED`.

`result` string (terminal runs)

Final assistant reply text for a terminated run.

`git` object (when a branch has been pushed)

The agent's current pushed branches and pull requests. `git.branches[]` contains `{ repoUrl, branch?, prUrl? }` entries — one per branch the agent has pushed (stacked agents produce multiple).

**Per-agent state, not per-run.** Every run on the same agent returns the same `git` snapshot. Use the agent's `latestRunId` or the SSE stream to attribute work to a specific run.

`repoUrl` is returned without the scheme (for example, `github.com/your-org/your-repo`) — different from request `repos[].url`, which keeps the `https://` prefix.

```bash
curl --request GET \
  --url https://api.cursor.com/v1/agents/bc-00000000-0000-0000-0000-000000000001/runs/run-00000000-0000-0000-0000-000000000001 \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "id": "run-00000000-0000-0000-0000-000000000001",
  "agentId": "bc-00000000-0000-0000-0000-000000000001",
  "status": "FINISHED",
  "createdAt": "2026-04-13T18:30:00.000Z",
  "updatedAt": "2026-04-13T18:45:00.000Z",
  "durationMs": 12357,
  "result": "Added README.md with installation instructions and usage examples.",
  "git": {
    "branches": [
      {
        "repoUrl": "github.com/your-org/your-repo",
        "branch": "cursor/add-readme-a1b2",
        "prUrl": "https://github.com/your-org/your-repo/pull/123"
      }
    ]
  }
}
```

### Stream A Run

/v1/agents//runs//stream

Stream Server-Sent Events (SSE) for one run. The stream is scoped to the requested run and does not replay prior runs.

#### Event types

- `status` — run status update. Payload: `{ runId, status }`.
- `assistant` — assistant text delta. Payload: `{ text }`.
- `thinking` — thinking text delta. Payload: `{ text }`.
- `tool_call` — tool call status update. Payload: `{ callId, name, status, args?, result?, truncated? }`.
- `interaction_update` — optional richer event emitted alongside the simplified events above. Payload matches the `InteractionUpdate` shape consumed by the [TypeScript SDK](https://cursor.com/docs/sdk/typescript.md), with subtypes like `text-delta`, `tool-call-started` / `tool-call-completed`, `step-started` / `step-completed`, and `turn-ended`. If you only need plain text and tool calls, handle the simplified events and ignore `interaction_update`. If you want the full SDK-shape stream, handle `interaction_update` and ignore the simplified events.
- `heartbeat` — keepalive event. Payload: `{}`.
- `result` — terminal run status. Payload: `{ runId, status, text?, durationMs?, git? }`. `text` is the final assistant reply, `durationMs` is the wall-clock run duration in milliseconds, and `git` mirrors `Run.git` (the agent's current pushed branches, not just this run's).
- `error` — stream error. Payload: `{ code, message }`.
- `done` — stream complete. Payload: `{}`.

#### Tool call payloads

`tool_call` events use a stable envelope around tool-specific inputs and outputs:

```typescript
type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

interface ToolCallEventData {
  callId: string;
  name: string;
  status: "running" | "completed";
  args?: JsonValue;
  result?: JsonValue;
  truncated?: {
    args?: true;
    result?: true;
  };
}
```

`callId` identifies one tool invocation across updates. `name` is the public tool name, such as `read_file`, `run_terminal_cmd`, or `mcp`. `args` and `result` are tool-specific JSON values. If `args` or `result` is too large to include in the stream, Cursor omits that field and sets the matching `truncated` flag.

#### Resuming a stream

Most events include an `id` line — an opaque string you should not parse (current format looks like `1713033006000-0`, but treat it as opaque). The leading `status` event has no `id` — it is a sticky framing event that is re-sent at the top of every reconnect.

To resume after a disconnect, reconnect with `Last-Event-ID` set to the most recent received event id. The event id must belong to the requested run; otherwise the request returns `400 invalid_last_event_id`. After a successful resume, expect another `status` event before the resumed range begins.

#### Retention

Stream responses include the `X-Cursor-Stream-Retention-Seconds` header. After the retention window elapses, this endpoint may return `410 stream_expired`. Treat that as a signal to read terminal state via [Get A Run](https://cursor.com/docs/cloud-agent/api/endpoints.md#get-a-run) instead of retrying the stream.

```bash
curl --request GET \
  --url https://api.cursor.com/v1/agents/bc-00000000-0000-0000-0000-000000000001/runs/run-00000000-0000-0000-0000-000000000001/stream \
  -u YOUR_API_KEY: \
  --header 'Accept: text/event-stream'
```

**Example stream:**

```text
event: status
data: {"runId":"run-00000000-0000-0000-0000-000000000001","status":"RUNNING"}

id: 1713033000000-0
event: assistant
data: {"text":"I'll update the README now."}

id: 1713033005000-0
event: tool_call
data: {"callId":"call-1","name":"read_file","status":"running","args":{"path":"README.md"}}

id: 1713033006000-0
event: tool_call
data: {"callId":"call-1","name":"read_file","status":"completed","args":{"path":"README.md"},"result":{"success":{"content":"# Project","totalLines":1,"fileSize":9,"path":"README.md"}}}

id: 1713033010000-0
event: result
data: {"runId":"run-00000000-0000-0000-0000-000000000001","status":"FINISHED","text":"Added README.md with installation instructions.","durationMs":12357,"git":{"branches":[{"repoUrl":"github.com/your-org/your-repo","branch":"cursor/add-readme-a1b2"}]}}

id: 1713033010000-0
event: done
data: {}
```

### Cancel A Run

/v1/agents//runs//cancel

Cancel the active run for an agent. Cancellation is terminal — the run transitions to `CANCELLED` and cannot be resumed. To continue the conversation, create a new run on the same agent.

Cancelling a run that is already in a terminal state, or one that was never active, returns `409 run_not_cancellable`.

#### Path Parameters

`id` string

Unique identifier for the agent.

`runId` string

Unique identifier for the run to cancel.

```bash
curl --request POST \
  --url https://api.cursor.com/v1/agents/bc-00000000-0000-0000-0000-000000000001/runs/run-00000000-0000-0000-0000-000000000001/cancel \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "id": "run-00000000-0000-0000-0000-000000000001"
}
```

### Get Agent Usage

/v1/agents//usage

Retrieve token usage for an agent, broken down per run. The response totals usage across every run on the agent and lists usage for each individual run. Token usage matches the `tokenUsage` reported by the team [usage events](https://cursor.com/docs/account/teams/admin-api.md#get-usage-events-data) endpoint.

#### Path Parameters

`id` string

Unique identifier for the agent (for example, `bc-00000000-0000-0000-0000-000000000001`).

#### Query Parameters

`runId` string (optional)

Scope the response to a single run (for example, `run-00000000-0000-0000-0000-000000000001`). Omit to return usage for every run on the agent. An unknown `runId` returns `404 run_not_found`.

#### Response Fields

`totalUsage` object

Token usage summed across the returned runs. Contains the same fields as each run's `usage` object.

`runs` array

Per-run usage, one entry per run (or a single entry when `runId` is set). Each object contains:

- `id` string - Run identifier (for example, `run-00000000-0000-0000-0000-000000000001`).
- `usageUuid` string (optional) - Internal usage identifier for the run. Omitted when the run has no recorded usage yet.
- `usage` object - Token usage for this run:
  - `inputTokens` number - Input tokens consumed.
  - `outputTokens` number - Output tokens generated.
  - `cacheWriteTokens` number - Tokens written to cache.
  - `cacheReadTokens` number - Tokens read from cache.
  - `totalTokens` number - Sum of the four token counts above.

Runs without any recorded token usage report zeros across all fields. A run that hasn't produced usage yet still appears in `runs` so you can track it over time.

```bash
# All runs on the agent
curl --request GET \
  --url https://api.cursor.com/v1/agents/bc-00000000-0000-0000-0000-000000000001/usage \
  -u YOUR_API_KEY:

# A single run
curl --request GET \
  --url 'https://api.cursor.com/v1/agents/bc-00000000-0000-0000-0000-000000000001/usage?runId=run-00000000-0000-0000-0000-000000000001' \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "totalUsage": {
    "inputTokens": 12480,
    "outputTokens": 3110,
    "cacheWriteTokens": 18200,
    "cacheReadTokens": 42600,
    "totalTokens": 76390
  },
  "runs": [
    {
      "id": "run-00000000-0000-0000-0000-000000000002",
      "usageUuid": "00000000-0000-0000-0000-000000000002",
      "usage": {
        "inputTokens": 6320,
        "outputTokens": 1450,
        "cacheWriteTokens": 7100,
        "cacheReadTokens": 21300,
        "totalTokens": 36170
      }
    },
    {
      "id": "run-00000000-0000-0000-0000-000000000001",
      "usageUuid": "00000000-0000-0000-0000-000000000001",
      "usage": {
        "inputTokens": 6160,
        "outputTokens": 1660,
        "cacheWriteTokens": 11100,
        "cacheReadTokens": 21300,
        "totalTokens": 40220
      }
    }
  ]
}
```

## Artifacts

Artifacts are agent-scoped because the workspace persists across runs.

### List Artifacts

/v1/agents//artifacts

List artifacts produced by an agent. Each artifact's `path` is relative to the workspace's `artifacts/` directory.

Pass the `path` value returned here directly to [Download An Artifact](https://cursor.com/docs/cloud-agent/api/endpoints.md#download-an-artifact). v1 paths are relative; absolute v0 paths (`/opt/cursor/artifacts/...`) are not accepted.

#### Path Parameters

`id` string

Unique identifier for the agent.

```bash
curl --request GET \
  --url https://api.cursor.com/v1/agents/bc-00000000-0000-0000-0000-000000000001/artifacts \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "items": [
    {
      "path": "artifacts/screenshot.png",
      "sizeBytes": 12345,
      "updatedAt": "2026-04-13T18:45:00.000Z"
    }
  ]
}
```

### Download An Artifact

/v1/agents//artifacts/download

Retrieve a temporary 15-minute presigned S3 URL for a specific artifact.

#### Path Parameters

`id` string

Unique identifier for the agent.

#### Query Parameters

`path` string

Relative artifact path returned by [List Artifacts](https://cursor.com/docs/cloud-agent/api/endpoints.md#list-artifacts) (for example, `artifacts/screenshot.png`). Must be under `artifacts/`.

```bash
curl --request GET \
  --url 'https://api.cursor.com/v1/agents/bc-00000000-0000-0000-0000-000000000001/artifacts/download?path=artifacts/screenshot.png' \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "url": "https://cloud-agent-artifacts.s3.us-east-1.amazonaws.com/...",
  "expiresAt": "2026-04-13T19:00:00.000Z"
}
```

## Agent Lifecycle

### Archive An Agent

/v1/agents//archive

Archive an agent. Archived agents remain readable but cannot accept new runs until unarchived. Use this for reversible "soft delete" flows.

Archive is idempotent — re-archiving an already-archived agent returns `200` with no change. You don't need to check current state before calling.

#### Path Parameters

`id` string

Unique identifier for the agent.

```bash
curl --request POST \
  --url https://api.cursor.com/v1/agents/bc-00000000-0000-0000-0000-000000000001/archive \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "id": "bc-00000000-0000-0000-0000-000000000001"
}
```

### Unarchive An Agent

/v1/agents//unarchive

Unarchive an agent so it can accept new runs again.

Unarchive is idempotent — calling it on an already-active agent returns `200` with no change.

#### Path Parameters

`id` string

Unique identifier for the agent.

```bash
curl --request POST \
  --url https://api.cursor.com/v1/agents/bc-00000000-0000-0000-0000-000000000001/unarchive \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "id": "bc-00000000-0000-0000-0000-000000000001"
}
```

### Delete An Agent Permanently

/v1/agents/

Permanently delete an agent. This action is irreversible. Use [Archive](https://cursor.com/docs/cloud-agent/api/endpoints.md#archive-an-agent) for reversible removal.

#### Path Parameters

`id` string

Unique identifier for the agent.

```bash
curl --request DELETE \
  --url https://api.cursor.com/v1/agents/bc-00000000-0000-0000-0000-000000000001 \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "id": "bc-00000000-0000-0000-0000-000000000001"
}
```

## Worker Tokens

### Create A User-Scoped Worker Token

/v1/sub-tokens

Create a one-hour user-scoped token for a self-hosted worker to run as an active team member.

Requires an agent-scoped team service account API key. User-scoped tokens can't mint other user-scoped tokens.

The returned token expires after 1 hour and cannot refresh itself. Mint a new token with the service account API key when you need to refresh a running worker.

#### Request Body

Specify exactly one of the following to identify the target user:

`forUserEmail` string (optional)

Active team member email. Case-insensitive.

`forUserId` integer (optional)

Active team member's numeric Cursor user ID.

By email:

```bash
curl --request POST \
  --url https://api.cursor.com/v1/sub-tokens \
  --header "Authorization: Bearer $CURSOR_SERVICE_ACCOUNT_API_KEY" \
  --header "Content-Type: application/json" \
  --data '{
    "forUserEmail": "alice@company.com"
  }'
```

By user ID:

```bash
curl --request POST \
  --url https://api.cursor.com/v1/sub-tokens \
  --header "Authorization: Bearer $CURSOR_SERVICE_ACCOUNT_API_KEY" \
  --header "Content-Type: application/json" \
  --data '{
    "forUserId": 42
  }'
```

**Response:**

```json
{
  "accessToken": "eyJ...",
  "expiresAt": "2026-04-24T19:00:00.000Z",
  "userId": 42,
  "teamId": 456
}
```

## Fleet Management

Monitor pool worker utilization and build autoscaling against self-hosted Cloud Agent pools. Durable pools stay registered after the last worker disconnects, so you can scale to zero and bring capacity back when [pending requests](https://cursor.com/docs/cloud-agent/api/endpoints.md#list-pending-pool-requests) appear.

The endpoint paths keep the older `private-workers` name; they refer to the same [self-hosted workers](https://cursor.com/docs/cloud-agent/self-hosted-agents.md).

Authenticate with the pool's service account API key via Basic auth or Bearer token. Other API key types are rejected.

### List Workers

/v0/private-workers

List self-hosted pool workers for the authenticated service account's team, newest first.

#### Query Parameters

`status` string (optional, default: `all`)

Filter by worker status. One of `all`, `in_use`, or `idle`.

`scope` string (optional, default: `all`)

Filter by worker scope. One of `all`, `team_pool`, or `personal`.

`limit` integer (optional, default: 50)

Results per page. Range: 1 to 100.

`pageToken` string (optional)

Pagination cursor. Pass the `nextPageToken` from the previous response.

#### Response Fields

`workers` array

Connected workers. Each entry includes:

- `workerId` string — Unique worker identifier. Auto-generated ids are UUIDs; workers started with `CURSOR_AGENT_WORKER_ID` report that custom id instead.
- `isInUse` boolean — Whether the worker currently has an assigned agent.
- `repoOwner`, `repoName` string — Primary repository metadata when the worker registered a git remote. Empty strings for repo-less workers.
- `repoUrl` string (optional) — Primary repository URL. Omitted for repo-less workers.
- `workspaceRootPath` string — Primary workspace path on the worker.
- `connectedAtMs` integer — Connection time in Unix milliseconds.
- `userId` integer — Owning user id. `0` for workers authenticated with a service account key.
- `teamId` integer (optional) — Team id for team pool workers.
- `serviceAccountId` string (optional) — Service account that authenticated the worker.
- `activeBcId` string (optional) — Id of the agent currently running on the worker, when in use.
- `name` string (optional) — Worker display name (`--name`, defaults to the machine hostname).

`totalCount` integer

Total workers matching the filter, across all pages.

`nextPageToken` string (optional)

Pagination cursor for `pageToken`. Omitted when there are no more pages.

```bash
curl --request GET \
  --url "https://api.cursor.com/v0/private-workers?status=idle&scope=team_pool&limit=50" \
  -u "$CURSOR_API_KEY:"
```

**Response:**

```json
{
  "workers": [
    {
      "workerId": "a8574fe8-248e-424a-a078-7584a2b93724",
      "repoOwner": "acme",
      "repoName": "payments-service",
      "repoUrl": "https://github.com/acme/payments-service",
      "workspaceRootPath": "/home/agent/payments-service",
      "connectedAtMs": 1737306880000,
      "userId": 0,
      "teamId": 456,
      "serviceAccountId": "sa_abc123",
      "isInUse": false,
      "name": "gpu-worker-1"
    }
  ],
  "totalCount": 1
}
```

### Get Fleet Summary

/v0/private-workers/summary

Return connected and in-use worker counts for the authenticated user and their team. Use this to trigger scaling decisions when utilization is high.

```bash
curl --request GET \
  --url "https://api.cursor.com/v0/private-workers/summary" \
  -u "$CURSOR_API_KEY:"
```

**Example scaling check:**

```typescript
const summary = await response.json();
const team = summary.teamSummary;
if (team && team.totalConnected > 0) {
  const utilization = team.inUse / team.totalConnected;
  if (utilization >= 0.9) {
    // Scale up: provision additional workers
  }
}
```

### Get Worker By ID

/v0/private-workers/

Retrieve a single self-hosted pool worker by its ID.

#### Path Parameters

`id` string

Unique identifier for the worker (for example, `pw_123`).

```bash
curl --request GET \
  --url "https://api.cursor.com/v0/private-workers/pw_123" \
  -u "$CURSOR_API_KEY:"
```

### List Pools

/v0/private-workers/pools

List durable self-hosted pools for the authenticated service account's team. Pools remain registered after the last worker disconnects, so you can monitor scale-to-zero fleets and decide when to provision capacity.

#### Query Parameters

`scope` string (optional)

Filter by pool list scope. One of `all`, `team_pool`, or `personal`.

`includeStale` boolean (optional, default: false)

When `true`, include pools marked stale after long inactivity.

#### Response Fields

`pools` array

Registered pools. Each entry includes:

- `scope` string — Pool ownership scope (`user` or `team`).
- `ownerId` integer — Owning user or team id for the scope.
- `poolName` string — Pool name (for example, `default` or `gpu`).
- `connectedWorkerCount` integer — Workers currently connected to this pool.
- `inUseWorkerCount` integer — Connected workers that currently have an assigned agent. Idle capacity is `connectedWorkerCount - inUseWorkerCount`.
- `firstSeenAtMs`, `lastSeenAtMs` integer — First and last observation times in Unix milliseconds.
- `isStale` boolean — Whether the pool is marked stale after long inactivity.
- `repoOwner`, `repoName`, `repoUrl` string (optional) — Repository metadata when the pool is tied to a repo. Omitted for [repo-less pools](https://cursor.com/docs/cloud-agent/self-hosted-guides/pool.md#repo-less-pools).

```bash
curl --request GET \
  --url "https://api.cursor.com/v0/private-workers/pools?scope=team_pool&includeStale=false" \
  -u "$CURSOR_API_KEY:"
```

**Response:**

```json
{
  "pools": [
    {
      "scope": "team",
      "ownerId": 456,
      "poolName": "gpu",
      "repoOwner": "acme",
      "repoName": "payments-service",
      "repoUrl": "https://github.com/acme/payments-service",
      "connectedWorkerCount": 2,
      "inUseWorkerCount": 1,
      "firstSeenAtMs": 1737000000000,
      "lastSeenAtMs": 1737306880000,
      "isStale": false
    },
    {
      "scope": "team",
      "ownerId": 456,
      "poolName": "sandbox",
      "connectedWorkerCount": 0,
      "inUseWorkerCount": 0,
      "firstSeenAtMs": 1737100000000,
      "lastSeenAtMs": 1737200000000,
      "isStale": false
    }
  ]
}
```

The `sandbox` entry is repo-less: repo fields are omitted, and the pool stays selectable with zero connected workers.

### Register A Pool

/v0/private-workers/pools

Register a durable pool without starting a worker. Use this to make a pool selectable before any worker connects, for example when an orchestrator provisions capacity on demand. Starting a worker with `--pool` registers the pool implicitly; this endpoint is only needed to create the pool up front.

#### Request Body

`scope` string (required)

Pool ownership scope. One of `user` or `team`.

`poolName` string (required)

Pool name to register (for example, `gpu`).

`repoOwner`, `repoName` string (optional)

Repository metadata when the pool is tied to a repo. Provide both together, or omit both for a repo-less pool.

`repoUrl` string (optional)

Repository URL for display. Requires `repoOwner` and `repoName`.

#### Response Fields

`registered` boolean

Whether the pool was registered.

```bash
curl --request POST \
  --url "https://api.cursor.com/v0/private-workers/pools" \
  -u "$CURSOR_API_KEY:" \
  --header 'Content-Type: application/json' \
  --data '{
    "scope": "team",
    "poolName": "payments-pool",
    "repoOwner": "acme",
    "repoName": "payments-service",
    "repoUrl": "https://github.com/acme/payments-service"
  }'
```

**Response:**

```json
{
  "registered": true
}
```

### Deregister A Pool

/v0/private-workers/pools

Deregister (soft-delete) a durable pool so it no longer appears in pool pickers or [List Pools](https://cursor.com/docs/cloud-agent/api/endpoints.md#list-pools). Workers currently connected to the pool are not affected. Team pools require a team admin; user pools require their owner.

#### Query Parameters

`scope` string (required)

Pool ownership scope. One of `user` or `team`.

`pool_name` string (required)

Pool name to deregister.

`repo_owner` string (optional)

Repository owner when deregistering a repo-scoped pool record.

`repo_name` string (optional)

Repository name when deregistering a repo-scoped pool record. Provide `repo_owner` and `repo_name` together, or omit both for a repo-less pool.

```bash
curl --request DELETE \
  --url "https://api.cursor.com/v0/private-workers/pools?scope=team&pool_name=sandbox" \
  -u "$CURSOR_API_KEY:"
```

**Response:**

```json
{
  "deregistered": true
}
```

### List Pending Pool Requests

/v0/private-workers/pending-requests

List self-hosted pool requests that have not been assigned to a worker yet. Use this endpoint to scale capacity when users are waiting for an available pool worker, or pair it with [Claim A Pending Request](https://cursor.com/docs/cloud-agent/api/endpoints.md#claim-a-pending-request) before starting an ephemeral worker.

This endpoint requires a service account API key. It returns requests for the key's team and excludes My Machines requests. If the key is scoped to specific repositories, pass `repository`; the repository must be in the key's allowed scope.

The response includes a `streamCursor`. Pass it to [Watch Pending Pool Requests](https://cursor.com/docs/cloud-agent/api/endpoints.md#watch-pending-pool-requests) to follow queue changes in real time after this snapshot.

#### Query Parameters

`limit` number (optional)

Number of pending requests to return. Default: 50, Max: 100.

`pageToken` string (optional)

Pagination cursor from the previous response. Page tokens are bound to the `repository` and `pool` filters that issued them.

`repository` string (optional)

Filter by repository URL. Required for repo-scoped service account API keys. Omit for repo-less pending requests.

`pool` string (optional)

Filter by pool name. Exact, case-sensitive match against the request's `pool` label. Omit to list requests for every pool on the team.

#### Response Fields

`requests` array

Pending requests. Each entry includes:

- `id` string — Pending request / agent id (pass to [Claim](https://cursor.com/docs/cloud-agent/api/endpoints.md#claim-a-pending-request) as `id`).
- `userId` integer — Cursor user id that created the request.
- `userEmail` string (optional) — Email of the requesting user, when available. Use it to select user-affine capacity without another lookup.
- `serviceAccountId` string (optional) — Service account associated with the request, when present.
- `repoOwner`, `repoName`, `repoUrl` string (optional) — Repository metadata when the request targets a repo. Omitted for repo-less pool requests.
- `labels` array — Request labels as `{ key, value }` pairs (includes `repo=` and `pool=` when set).
- `createdAtMs` integer — Request creation time in Unix milliseconds.

`nextPageToken` string (optional)

Pagination cursor. Omitted when there are no more pages. To measure queue depth, paginate to completion and count the requests.

`streamCursor` string

Opaque resume position for [Watch Pending Pool Requests](https://cursor.com/docs/cloud-agent/api/endpoints.md#watch-pending-pool-requests). Every page of one logical listing repeats the same `streamCursor`; open the watch from it after you finish paginating. It expires five minutes after the list that issued it.

```bash
curl --request GET \
  --url "https://api.cursor.com/v0/private-workers/pending-requests?limit=50&repository=https%3A%2F%2Fgithub.com%2Facme%2Fpayments-service" \
  -u "$CURSOR_API_KEY:"
```

**Response:**

```json
{
  "requests": [
    {
      "id": "bc-00000000-0000-0000-0000-000000000002",
      "userId": 321,
      "userEmail": "owner@acme.example",
      "serviceAccountId": "sa_abc123",
      "repoOwner": "acme",
      "repoName": "payments-service",
      "repoUrl": "https://github.com/acme/payments-service",
      "labels": [
        { "key": "repo", "value": "acme/payments-service" },
        { "key": "pool", "value": "gpu" },
        { "key": "env", "value": "production" }
      ],
      "createdAtMs": 1737306880000
    }
  ],
  "nextPageToken": "eyJjcmVhdGVkQXRNcyI6MTczNzMwNjg4MDAwMH0=",
  "streamCursor": "djQuZXhhbXBsZS1vcGFxdWUtY3Vyc29y"
}
```

`repoUrl` omits embedded credentials when the original repository URL includes userinfo.

### Watch Pending Pool Requests

/v0/private-workers/pending-requests/stream

Stream pending-request lifecycle events over Server-Sent Events (SSE) so orchestrators can react to queue changes without polling.

This endpoint requires a service account API key. Controllers list-then-watch: call [List Pending Pool Requests](https://cursor.com/docs/cloud-agent/api/endpoints.md#list-pending-pool-requests) to build your view of the queue, keep the response's `streamCursor`, then open the watch from that exact position. Use the same `repository` and `pool` filters for the list and the watch; cursors are bound to the filters that issued them.

#### Query Parameters

`cursor` string (required)

The `streamCursor` from a list response, or the SSE `id:` of the last event you processed. On reconnect, a native `EventSource` resends that id as the `Last-Event-ID` header, which takes precedence over the query parameter.

`repository` string (optional)

Same semantics as [List Pending Pool Requests](https://cursor.com/docs/cloud-agent/api/endpoints.md#list-pending-pool-requests). Required for repo-scoped service account API keys. Pagination parameters are not accepted on the stream.

`pool` string (optional)

Watch only events for this pool. Exact, case-sensitive match against the request's `pool` label. Must match the filter used by the list that issued the cursor. Omit to watch every pool on the team.

#### Events

The watch replays the retained transitions after the cursor, then follows live. Every event's SSE `id:` is the cursor to resume from if the connection drops.

- `created` event — A request entered the queue. Payload: the same request object as [List Pending Pool Requests](https://cursor.com/docs/cloud-agent/api/endpoints.md#list-pending-pool-requests).
- `claimed` event — A worker claimed the request. Payload: `{ id }`.
- `expired` event — The request left the queue without being claimed. Payload: `{ id }`.
- `heartbeat` event — Cursor checkpoint with no state change, sent about every 20 seconds on a quiet stream. Payload: `{}`. Heartbeats advance an idle watch's resume position but do not extend the cursor's lifetime.

#### Cursor lifetime

Every cursor in a watch chain expires **five minutes after the list that issued it**. Heartbeats and reconnects do not extend it. When the cursor expires, or the retained event window no longer covers it, the endpoint returns HTTP `410 Gone` with `{"code": "cursor_expired"}`: re-list and watch from the fresh `streamCursor`. This is routine, not an error path. Re-list proactively on a five-minute timer with jitter instead of riding the `410`, so a fleet of controllers does not synchronize its list calls.

#### Delivery guarantees

Delivery is best-effort, and the list is the source of truth. Events are published after each transition commits, with retries, but a rare failure can drop one, and a dropped event is never redelivered. Between re-lists, treat events as low-latency hints: apply them idempotently (upsert `created` requests, remove `claimed` and `expired` requests by `id`) and let the next list correct any drift. A `claimed` event for a request you never saw is a no-op. Claims stay atomic server-side regardless of your local view.

Do not persist cursors. A service account can hold at most four concurrent streams; use one stream per controller and fan out locally.

```bash
curl --request GET --no-buffer \
  --url "https://api.cursor.com/v0/private-workers/pending-requests/stream?cursor=$STREAM_CURSOR" \
  --header 'Accept: text/event-stream' \
  -u "$CURSOR_API_KEY:"
```

**Example stream:**

```
: connected

event: heartbeat
id: djQuY3Vyc29yLWNoZWNrcG9pbnQ
data: {}

event: created
id: djQuY3Vyc29yLWFmdGVyLWNyZWF0ZWQ
data: {"id":"bc-00000000-0000-0000-0000-000000000002","userId":321,"userEmail":"owner@acme.example","repoOwner":"acme","repoName":"payments-service","repoUrl":"https://github.com/acme/payments-service","labels":[{"key":"pool","value":"gpu"}],"createdAtMs":1737306880000}

event: claimed
id: djQuY3Vyc29yLWFmdGVyLWNsYWltZWQ
data: {"id":"bc-00000000-0000-0000-0000-000000000002"}
```

**The controller loop:**

1. [List pending requests](https://cursor.com/docs/cloud-agent/api/endpoints.md#list-pending-pool-requests) to completion and replace your local view with the result. Keep the response's `streamCursor`.
2. Open the watch with `?cursor=<streamCursor>` and apply events to your local view. Track the latest event `id:` you processed.
3. On disconnect, reconnect with the latest event id as `?cursor=`, or rely on a native `EventSource`, which resends it as `Last-Event-ID` automatically.
4. On HTTP `410 Gone`, go back to step 1 and re-list.

### Claim A Pending Request

/v0/private-workers/claim

Reserve a pending pool request for a specific worker before that worker starts. Controllers use this to atomically assign work across replicas: read [pending requests](https://cursor.com/docs/cloud-agent/api/endpoints.md#list-pending-pool-requests), claim one, then start a worker with a stable worker id that matches the claim.

This endpoint requires a service account API key.

#### Request Body

`id` string (required)

Pending request id. Same value as `id` from [List Pending Pool Requests](https://cursor.com/docs/cloud-agent/api/endpoints.md#list-pending-pool-requests).

`workerId` string (required)

Worker id to reserve for the request. Start the worker with the same id via `CURSOR_AGENT_WORKER_ID` (or the hidden `--worker-id` flag) so the bridge registers the claimed identity.

```bash
curl --request POST \
  --url "https://api.cursor.com/v0/private-workers/claim" \
  -u "$CURSOR_API_KEY:" \
  --header 'Content-Type: application/json' \
  --data '{
    "id": "bc-00000000-0000-0000-0000-000000000002",
    "workerId": "pw_123"
  }'
```

**Response:**

```json
{
  "id": "bc-00000000-0000-0000-0000-000000000002",
  "workerId": "pw_123"
}
```

After a successful claim, start the worker with the reserved id:

```bash
export CURSOR_API_KEY="your-service-account-api-key"
export CURSOR_AGENT_WORKER_ID="pw_123"
agent worker --pool gpu --worker-dir /workspace start
```

## Metadata Endpoints

### API Key Info

/v1/me

Retrieve information about the API key being used for authentication.

#### Response Fields

`apiKeyName` string

Display name of the API key.

`createdAt` string

When the API key was created (ISO 8601).

`userId` integer (user-scoped keys)

Numeric Cursor user ID of the API key's owner. Omitted for service-account / team API keys, which aren't tied to a specific user.

`userEmail` string (user-scoped keys)

Email address of the API key's owner.

`userFirstName`, `userLastName` string (user-scoped keys)

First and last name of the API key's owner, when populated.

```bash
curl --request GET \
  --url https://api.cursor.com/v1/me \
  -u YOUR_API_KEY:
```

**Response (user-scoped key):**

```json
{
  "apiKeyName": "Production API Key",
  "userId": 42,
  "createdAt": "2026-04-13T18:30:00.000Z",
  "userEmail": "developer@example.com",
  "userFirstName": "Alex",
  "userLastName": "Rivera"
}
```

**Response (service-account key):**

```json
{
  "apiKeyName": "Production Service Account",
  "createdAt": "2026-04-13T18:30:00.000Z"
}
```

### List Models

/v1/models

Returns the recommended models you can pass to the `model.id` field on [Create An Agent](https://cursor.com/docs/cloud-agent/api/endpoints.md#create-an-agent), along with the parameters and variants each model accepts. Model parameters use the same `model.params` shape as the [TypeScript SDK ModelSelection](https://cursor.com/docs/sdk/typescript.md#modelselection).

To use the configured default model, omit `model` from the request body entirely. Cursor resolves your user default model, then your team default model, then a system default.

#### Response Fields

Each item in `items` describes one model:

`id` string

Pass this value as `model.id` when creating an agent.

`displayName` string

Human-readable name shown in the Cursor UI.

`description` string (optional)

Short description of the model.

`aliases` array (optional)

Alternate IDs that resolve to the same model (for example, `composer-latest`).

`parameters` array (optional)

Per-model parameter definitions. Each entry has an `id`, optional `displayName`, and a `values` array of permitted `{ value, displayName? }` entries. Use these to populate `model.params` on the create request.

`variants` array (optional)

Concrete `id`+`params` combinations the model accepts. Each entry has a `params` array (which may be empty), a `displayName`, an optional `description`, and an optional `isDefault` flag.

```bash
curl --request GET \
  --url https://api.cursor.com/v1/models \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "items": [
    {
      "id": "composer-2",
      "displayName": "Composer 2",
      "aliases": ["composer-latest", "composer"],
      "parameters": [
        {
          "id": "fast",
          "displayName": "Fast",
          "values": [
            { "value": "false" },
            { "value": "true", "displayName": "Fast" }
          ]
        }
      ],
      "variants": [
        {
          "params": [{ "id": "fast", "value": "true" }],
          "displayName": "Composer 2",
          "isDefault": true
        },
        {
          "params": [{ "id": "fast", "value": "false" }],
          "displayName": "Composer 2"
        }
      ]
    },
    {
      "id": "claude-4.6-sonnet-thinking",
      "displayName": "Claude 4.6 Sonnet (Thinking)",
      "variants": [
        {
          "params": [],
          "displayName": "Claude 4.6 Sonnet (Thinking)",
          "isDefault": true
        }
      ]
    }
  ]
}
```

### List GitHub Repositories

/v1/repositories

List GitHub repositories accessible to the authenticated user through Cursor's GitHub App installation.

**This endpoint has very strict rate limits.**

Limit requests to **1 / user / minute**, and **30 / user / hour.**

This request can take tens of seconds to respond for users with access to many repositories.

Make sure to handle this information not being available gracefully.

```bash
curl --request GET \
  --url https://api.cursor.com/v1/repositories \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "items": [
    {
      "url": "https://github.com/your-org/your-repo"
    }
  ]
}
```


---

## Sitemap

[Overview of all docs pages](/llms.txt)
