# Cursor Python SDK

The `cursor-sdk` package lets you call Cursor's agent from your own Python code. The same agent that runs in the Cursor IDE, CLI, and web app is scriptable from Python with sync and async clients, typed dataclasses, and ordinary iteration for streams and pages. Run the `/sdk` skill inside Cursor to get started.

For the REST API, see the [Cloud Agents API](https://cursor.com/docs/cloud-agent/api/endpoints.md). For other languages, see the [SDK Bridge](https://cursor.com/docs/sdk/bridge.md).

## Overview

The SDK wraps local and cloud runtimes behind one interface. You write the same code regardless of where the agent runs.

| Runtime                   | What it does                                                          | When to use                                                                                                                |
| :------------------------ | :-------------------------------------------------------------------- | :------------------------------------------------------------------------------------------------------------------------- |
| **Local**                 | Runs the agent against local files on disk.                           | Dev scripts and CI checks against a working tree.                                                                          |
| **Cloud (Cursor-hosted)** | Runs in an isolated VM with your repo cloned in. Cursor runs the VMs. | When the caller doesn't have the repo, you want many agents in parallel, or runs need to survive the caller disconnecting. |

Set the runtime by passing `local` or `cloud` to `Agent.create()`.

## Authentication

Set `CURSOR_API_KEY` or pass `api_key` before creating an agent.

The SDK accepts user API keys and service account API keys for both local and cloud runs. Team Admin API keys are not yet supported.

- **User API key** from [Cursor Dashboard -> API Keys](https://cursor.com/dashboard/api)
- **Service account API key** from [Team settings](https://cursor.com/dashboard/team-settings). See [Service accounts](https://cursor.com/docs/account/enterprise/service-accounts.md)

```bash
export CURSOR_API_KEY="your-key"
```

## Usage and billing

SDK runs follow the same pricing, request pools, and Privacy Mode rules as runs from the IDE and Cloud Agents. Spend shows up in your team's [usage dashboard](https://cursor.com/dashboard/usage) under the SDK tag.

To read per-run token counts in code, see [Token usage](https://cursor.com/docs/sdk/python.md#token-usage). To fetch billed usage and dollar cost for an agent's runs, see [`agent.get_usage()`](https://cursor.com/docs/sdk/python.md#agentget_usage).

## Core concepts

| Concept          | Description                                                                                                                      |
| :--------------- | :------------------------------------------------------------------------------------------------------------------------------- |
| **Agent**        | Durable handle that holds conversation state, workspace config, model selection, and settings. Survives across multiple prompts. |
| **Run**          | One prompt submission. Owns its own stream, status, result, conversation, and cancellation.                                      |
| **SDKMessage**   | Typed stream message yielded during a run. Same shape across local and cloud runtimes.                                           |
| **CursorClient** | Explicit client for lifecycle control, custom HTTP options, or multiple workspaces in one process. `Client` is an alias.         |
| **AsyncClient**  | Async-mirror client. Required for all async operations.                                                                          |

## Installation

```bash
pip install cursor-sdk
```

Requires Python 3.10 or later.

## Quick start

```python
import os

from cursor_sdk import Agent, LocalAgentOptions

with Agent.create(
    model="composer-2.5",
    api_key="crsr_key",
    local=LocalAgentOptions(cwd=os.getcwd()),
) as agent:
    print(agent.send("Summarize what this repository does").text())
```

[Stream events](https://cursor.com/docs/sdk/python.md#stream-events) shows how to extract assistant text, handle tool calls, and read run state. For a one-shot prompt (create, run, finish), see [`Agent.prompt()`](https://cursor.com/docs/sdk/python.md#agentprompt).

### Cloud quick start

The Python SDK has native support for Cursor's cloud agents. You can list connected repositories, start an agent against one of them, wait for the run, and review the final result.

```python
from cursor_sdk import Agent, CloudAgentOptions, CloudRepository

with Agent.create(
    model="composer-2.5",
    api_key="crsr_key",
    cloud=CloudAgentOptions(
        repos=[CloudRepository(url="https://github.com/your-org/your-repo", starting_ref="main")],
        auto_create_pr=True,
    ),
) as agent:
    print(agent.send("Add structured logging to the auth middleware").text())
```

Cloud agents started by the SDK are filtered out of the default agent list. To view them in Cursor Web or the Cursor agents window, click **Filter > Source > SDK**.

## Async usage

The async client mirrors the sync surface and is recommended for servers, bots, and concurrent agent orchestration. `AsyncAgent`, `AsyncClient`, `AsyncRun`, and `AsyncCursor` are exported from both `cursor_sdk` and `cursor_sdk.asyncio`.

```python
import asyncio
import os

from cursor_sdk import AsyncClient, LocalAgentOptions

async def main():
    async with await AsyncClient.launch_bridge(workspace=os.getcwd()) as client:
        async with await client.agents.create(
            model="composer-2.5",
            api_key="crsr_key",
            local=LocalAgentOptions(cwd=os.getcwd()),
        ) as agent:
            run = await agent.send("Summarize what this repository does")
            print(await run.text())

asyncio.run(main())
```

There is no global async default client. Instantiate `AsyncClient` explicitly, or use `AsyncClient.launch_bridge(...)` as an async context manager, so each event loop owns its own client. Do not mix sync and async clients in the same code path.

Direct `AsyncAgent` class methods require `client=`. Use
`await client.agents.create(...)` or
`await AsyncAgent.create(..., client=client)`.

| Sync                      | Async                               |
| :------------------------ | :---------------------------------- |
| `CursorClient` / `Client` | `AsyncClient` / `AsyncCursorClient` |
| `Agent`                   | `AsyncAgent`                        |
| `Run`                     | `AsyncRun`                          |
| `Cursor`                  | `AsyncCursor`                       |
| `ListResult`              | `AsyncListResult`                   |
| `DefaultHttpxClient`      | `DefaultAsyncHttpxClient`           |

## Creating agents

`Agent.create()` validates options and returns a handle immediately. Pass either `local` or `cloud` to pick a runtime.

```python
from cursor_sdk import Agent, CloudAgentOptions, CloudRepository, LocalAgentOptions

agent = Agent.create(
    model="composer-2.5",
    local=LocalAgentOptions(cwd="."),
)

cloud_agent = Agent.create(
    model="composer-2.5",
    cloud=CloudAgentOptions(
        repos=[CloudRepository(url="https://github.com/your-org/your-repo", starting_ref="main")],
        auto_create_pr=True,
    ),
)
```

`agent.agent_id` is populated immediately. Local agents get an `agent-<uuid>` ID; cloud agents get a `bc-<uuid>` ID. `agent.model` is a typed `ModelSelection`, so `agent.model.id` and `agent.model.params` work directly.

Cloud agents started by the SDK are filtered out of the default agent list. To
view them in Cursor Web or a Cursor agent window, click **Filter > Source > SDK**.

### No-repo cloud agents

Cloud agents can run on an empty VM with no repository. Pass `cloud` with an empty `repos` list, or omit `repos` entirely. Omitting `cloud` selects the local runtime instead.

```python
from cursor_sdk import Agent, CloudAgentOptions

with Agent.create(cloud=CloudAgentOptions(repos=[])) as agent:
    run = agent.send("Research the top 3 Python testing frameworks and summarize.")
    print(run.wait().result)
```

No-repo agents must be enabled for your account or team. Repository-scoped API keys can't create them; use an unrestricted service account key or a user API key instead.

### Session environment variables

For cloud agents, pass `env_vars` when a run needs short-lived credentials or other values that should live only with that agent.

```python
import os

agent = Agent.create(
    model="composer-2.5",
    cloud=CloudAgentOptions(
        repos=[CloudRepository(url="https://github.com/your-org/your-repo")],
        env_vars={
            "STAGING_API_TOKEN": os.environ["STAGING_API_TOKEN"],
        },
    ),
)
```

These values are encrypted at rest, injected into the cloud agent's shell, and deleted with the agent. `env_vars` can't be used with a caller-supplied `agent_id`; omit `agent_id` and read the server-minted ID from `agent.agent_id`. Variable names can't start with `CURSOR_`.

For values that should only exist during a single run, pass them on `agent.send()` instead. See [Per-run environment variables](https://cursor.com/docs/sdk/python.md#per-run-environment-variables).

### Agent metadata

Attach your own identifiers to a cloud agent when you create it. Metadata can link an agent to a user, tenant, workflow, or ticket in your system, and is read back on `SDKAgentInfo.metadata` from `client.agents.get()` and `client.agents.list()`. These tags are not the in-VM [agent metadata](https://cursor.com/docs/cloud-agent/metadata.md) API, which exposes the current run's id, owner, turn, and workspace from inside the VM.

```python
from cursor_sdk import Agent, CloudAgentOptions, CloudRepository

with Agent.create(
    model="composer-2.5",
    cloud=CloudAgentOptions(
        repos=[CloudRepository(url="https://github.com/your-org/your-repo")],
        metadata={
            "end_user_id": "user-123",
            "ticket_id": "ENG-456",
        },
    ),
) as agent:
    print(agent.agent_id)
```

Metadata is available for cloud agents at creation time. You can attach up to 50 key-value pairs. Keys must be non-empty and no more than 255 characters. Values must be strings no larger than 4096 bytes. Empty string values are allowed, and an empty mapping is treated as no metadata.

If metadata isn't enabled for the API key's account, creating an agent with a
non-empty map returns `403 feature_unavailable`.

### Model parameters

Use `ModelSelection.params` to pass per-model options such as reasoning effort or Cursor Router's `optimize_for`. Parameter IDs and values vary by model. Use [`Cursor.models.list()`](https://cursor.com/docs/sdk/python.md#the-cursor-namespace) to discover supported parameters and preset variants for your account.

```python
from cursor_sdk import Agent, LocalAgentOptions, ModelParameterValue, ModelSelection

agent = Agent.create(
    model=ModelSelection(
        id="composer-2.5",
        params=[ModelParameterValue(id="fast", value="true")],
    ),
    local=LocalAgentOptions(cwd="."),
)
```

Use [`Cursor.models.list()`](https://cursor.com/docs/sdk/python.md#the-cursor-namespace) to discover the parameter IDs and preset variants for a given model. See [Cursor Router](https://cursor.com/docs/sdk/python.md#cursor-router) for the `auto-smart` selection contract.

### Cursor Router

[Cursor Router](https://cursor.com/docs/cursor-router.md) selects a model for each Auto request. In the SDK, Router is the `auto-smart` model with an `optimize_for` parameter. It is available on Teams and Enterprise. Enterprise admins must enable Router for the team before `auto-smart` appears in the catalog.

The Cursor SDK is an agent SDK, not a standalone model-inference or chat-completions API. Router picks models for Cursor agent runs that can reason over a workspace, call tools, run commands, and edit files. Cursor does not currently document a raw Router endpoint for arbitrary model calls.

#### Select Cost, Balance, or Intelligence

Pass `auto-smart` and set `optimize_for` explicitly:

| Product label | SDK value      |
| :------------ | :------------- |
| Cost          | `cost`         |
| Balance       | `balanced`     |
| Intelligence  | `intelligence` |

Use **Balance** in product copy. Use `balanced` only as the SDK wire value.

```python
import os

from cursor_sdk import Agent, LocalAgentOptions, ModelParameterValue, ModelSelection

with Agent.create(
    model=ModelSelection(
        id="auto-smart",
        params=[ModelParameterValue(id="optimize_for", value="balanced")],
    ),
    local=LocalAgentOptions(cwd=os.getcwd()),
) as agent:
    run = agent.send("Find and fix the failing authentication test")
    result = run.wait()

    print(result.status)
```

Always pass `optimize_for`. Do not omit it and do not send a legacy `default` value; discovery through the catalog is the supported contract.

#### Discover Router in the model catalog

`Cursor.models.list()` returns the models, parameter definitions, and preset variants available to the API key's current account and team. Cursor Router appears as `auto-smart` when Router is available. Team administrators can disable Router or restrict which optimization modes members may select.

Treat the catalog as the source of truth before hard-coding a selection:

```python
from cursor_sdk import Cursor, ModelParameterValue, ModelSelection

models = Cursor.models.list()
router = next((model for model in models if model.id == "auto-smart"), None)
optimize_for = next(
    (
        parameter
        for parameter in (router.parameters if router else [])
        if parameter.id == "optimize_for"
    ),
    None,
)

if router is None or optimize_for is None:
    raise RuntimeError(
        "Cursor Router is not available for this API key. "
        "Verify that Router is enabled for the key's team."
    )

requested_mode = "balanced"
allowed_values = {entry.value for entry in optimize_for.values}

if requested_mode not in allowed_values:
    raise RuntimeError(
        f'Router mode "{requested_mode}" is not enabled for this team.'
    )

model = ModelSelection(
    id=router.id,
    params=[ModelParameterValue(id=optimize_for.id, value=requested_mode)],
)
```

#### Switch modes per run

Override the model on `agent.send()` to change Router mode for a run:

```python
from cursor_sdk import ModelParameterValue, ModelSelection, SendOptions

run = agent.send(
    "Handle this complex migration",
    SendOptions(
        model=ModelSelection(
            id="auto-smart",
            params=[ModelParameterValue(id="optimize_for", value="intelligence")],
        ),
    ),
)
```

Per-run model overrides are sticky. Later sends without an override keep using the new selection. See [Per-run model override](https://cursor.com/docs/sdk/python.md#per-run-model-override).

#### Model ids: `auto-smart`, `auto`, and `default`

| Selection                                     | Meaning                                                                                                                                     |
| :-------------------------------------------- | :------------------------------------------------------------------------------------------------------------------------------------------ |
| `auto-smart` with `optimize_for`              | Cursor Router. Use this when you want Cost, Balance, or Intelligence.                                                                       |
| `ModelSelection(id="auto")`                   | Server-selected Auto fallback when a specific model is missing from the catalog. Prefer `auto-smart` when you need an explicit Router mode. |
| Omitting `optimize_for`, or sending `default` | Not a supported Router contract. Always discover allowed values and pass `cost`, `balanced`, or `intelligence`.                             |

#### Billing and routing pool

- All Auto modes bill at the list price of the model each request is routed to.
- The underlying model can change between requests. Prefer a fixed model id when you need reproducible comparisons.
- Enterprise model allowlists shape the routing pool. Blocking required models can disable Router.

For current rates and the routing pool, see [Cursor Router](https://cursor.com/docs/cursor-router.md) and [Auto modes](https://cursor.com/docs/models-and-pricing.md#auto-modes).

#### Troubleshooting missing Router

If `auto-smart` is missing or an optimization mode is rejected:

1. Call `Cursor.models.list()`.
2. Confirm `auto-smart` is in the result.
3. Confirm `optimize_for` includes the value you want (`cost`, `balanced`, or `intelligence`).
4. Confirm Router is enabled for the team tied to the API key.
5. If you belong to multiple teams, confirm the key is operating in the intended team context.
6. Check team model-access policy if Router is unavailable or cannot choose a valid underlying model.

### Raw dictionaries

Typed dataclasses are preferred for application code because IDE autocomplete and type checking work better. The SDK also accepts plain dictionaries for short scripts or externally supplied JSON. Snake-case keys are normalized.

```python
from cursor_sdk import Agent

with Agent.create(
    {
        "api_key": "crsr_key",
        "model": {"id": "composer-2.5"},
        "local": {"cwd": "."},
    }
) as agent:
    ...
```

## Agent

The handle returned by `Agent.create()`, `Agent.resume()`, `client.agents.create()`, and `client.agents.resume()`.

```python
class Agent:
    agent_id: str
    model: ModelSelection | None
    client: CursorClient

    def send(
        self,
        message: str | Mapping[str, Any] | UserMessage,
        options: SendOptions | Mapping[str, Any] | None = None,
        *,
        idempotency_key: str | None = None,
    ) -> Run: ...

    def reload(self) -> None: ...
    def close(self) -> None: ...

    def list_messages(
        self, options: Mapping[str, Any] | None = None
    ) -> list[AgentMessage]: ...
    def list_artifacts(self) -> list[SDKArtifact]: ...
    def download_artifact(self, path: str) -> bytes: ...
    def get_usage(self, *, run_id: str | None = None) -> AgentUsage: ...

    def archive(self, options: Mapping[str, Any] | None = None) -> None: ...
    def unarchive(self, options: Mapping[str, Any] | None = None) -> None: ...
    def delete(self, options: Mapping[str, Any] | None = None) -> None: ...
```

| Member                             | Description                                                                           |
| :--------------------------------- | :------------------------------------------------------------------------------------ |
| `agent_id`                         | Stable agent identifier. `agent-<uuid>` for local, `bc-<uuid>` for cloud.             |
| `model`                            | Current typed model selection. Updates after a successful send with a model override. |
| `send`                             | Start a new run with the given prompt. Returns a `Run` handle.                        |
| `reload`                           | Re-read filesystem config (hooks, project MCP, subagents) without disposing.          |
| `close`                            | Close the agent and release resources.                                                |
| `list_messages`                    | List message history for the agent.                                                   |
| `list_artifacts`                   | List files produced by the agent (cloud only; local returns empty).                   |
| `download_artifact`                | Download a file by path (cloud only; local raises).                                   |
| `get_usage`                        | Fetch billed token usage and dollar cost for the agent.                               |
| `archive` / `unarchive` / `delete` | Manage cloud agent lifecycle.                                                         |

Use a context manager for automatic cleanup:

```python
with Agent.create(model="composer-2.5", local=LocalAgentOptions(cwd=".")) as agent:
    print(agent.send("Explain this repository").text())
```

When you use the sync `Agent.*` or `Cursor.*` helpers without passing `client=`, the SDK starts or reuses a module-level default client. It is closed automatically at process exit, and you can close it explicitly:

```python
from cursor_sdk import close_default_client

close_default_client()
```

### Agent.prompt()

```python
Agent.prompt(
    message: str | Mapping[str, Any] | UserMessage,
    options: AgentOptions | Mapping[str, Any] | None = None,
    *,
    client: CursorClient | None = None,
) -> RunResult
```

One-shot convenience: creates an agent, sends a single prompt, waits for the run to finish, and disposes.

```python
from cursor_sdk import Agent, AgentOptions, LocalAgentOptions

result = Agent.prompt(
    "What does the auth middleware do?",
    AgentOptions(model="composer-2.5", local=LocalAgentOptions(cwd=".")),
)
print(result.result)
```

Async equivalent (assumes you already have an `AsyncClient` open):

```python
from cursor_sdk import AgentOptions, AsyncAgent, LocalAgentOptions

result = await AsyncAgent.prompt(
    "What does the auth middleware do?",
    AgentOptions(model="composer-2.5", local=LocalAgentOptions(cwd=".")),
    client=client,
)
```

## CursorClient

Use `CursorClient` when you want explicit lifecycle control, a custom bridge endpoint, custom HTTP options, or multiple workspaces in one process. `Client` remains available as an alias.

```python
from cursor_sdk import CursorClient, LocalAgentOptions

with CursorClient.launch_bridge(workspace=".") as client:
    with client.agents.create(
        model="composer-2.5",
        api_key="crsr_key",
        local=LocalAgentOptions(cwd="."),
    ) as agent:
        print(agent.send("Summarize what this repository does").text())
```

### Resources

Explicit clients expose resource namespaces:

| Resource       | Sync method examples                                                             | Async method examples                                              |
| :------------- | :------------------------------------------------------------------------------- | :----------------------------------------------------------------- |
| `agents`       | `client.agents.create(...)`, `client.agents.list(...)`, `client.agents.get(...)` | `await client.agents.create(...)`, `await client.agents.list(...)` |
| `models`       | `client.models.list()`                                                           | `await client.models.list()`                                       |
| `repositories` | `client.repositories.list()`                                                     | `await client.repositories.list()`                                 |

Top-level methods such as `client.create_agent(...)` and `client.list_agents(...)` remain available, but resource namespaces are the preferred shape for application code.

### Custom HTTP clients

Both sync and async clients accept a custom `httpx` client for proxies, transports, and other advanced HTTP configuration:

```python
from cursor_sdk import CursorClient, DefaultHttpxClient

with CursorClient.launch_bridge(
    workspace=".",
    http_client=DefaultHttpxClient(proxy="http://proxy.example.com"),
) as client:
    ...
```

```python
from cursor_sdk import AsyncClient, DefaultAsyncHttpxClient

async with await AsyncClient.launch_bridge(
    workspace=".",
    http_client=DefaultAsyncHttpxClient(proxy="http://proxy.example.com"),
) as client:
    ...
```

`DefaultHttpxClient` and `DefaultAsyncHttpxClient` keep the SDK's default timeout and redirect behavior. Plain `httpx.Client` and `httpx.AsyncClient` use httpx defaults instead.

### Configuring timeouts and retries

Both clients expose `with_options(...)`, which returns a shallow copy that shares connection settings and overrides defaults. Use `timeout` for all requests, or set `unary_timeout` and `stream_timeout` separately. `max_retries` controls client retries:

```python
short = client.with_options(timeout=5.0, max_retries=2)
agent = short.agents.create(model="composer-2.5", local=LocalAgentOptions(cwd="."))
```

Async equivalent:

```python
short_async = async_client.with_options(timeout=5.0, max_retries=2)
agent = await short_async.agents.create(model="composer-2.5", local=LocalAgentOptions(cwd="."))
```

## Sending messages

Each `agent.send()` returns a `Run`. Each `await async_agent.send()` returns an `AsyncRun`. The agent retains conversation context across runs; the run is the unit of work for one prompt.

```python
print(agent.send("Find the bug in src/auth.py").text())

# Same agent, full conversation context is preserved.
print(agent.send("Fix it and add a regression test").text())
```

Async equivalent:

```python
run = await agent.send("Find the bug in src/auth.py")
print(await run.text())

run = await agent.send("Fix it and add a regression test")
print(await run.text())
```

To send images alongside text:

```python
run = agent.send(
    {
        "text": "What's in this screenshot?",
        "images": [{"data": base64_png, "mime_type": "image/png"}],
    }
)
```

You can also use helper dataclasses. `SDKImage.from_file(path)` reads from disk and handles base64 encoding for you:

```python
from cursor_sdk import SDKImage, UserMessage

run = agent.send(
    UserMessage(
        text="What's in this screenshot?",
        images=[SDKImage.from_file("screenshot.png")],
    )
)
```

`SDKImage.data_image(base64_data, mime_type)` and `SDKImage.url_image(url)` are also available for callers that already have encoded bytes or a remote URL.

### Run

```python
class Run:
    id: str
    agent_id: str
    status: str  # "running" | "finished" | "error" | "cancelled" | "expired"
    result: str
    model: ModelSelection | None
    duration_ms: int
    git: RunGitInfo | None
    created_at: str | None
    usage: TokenUsage | None  # cumulative; property on the live handle

    def stream(self) -> Iterator[SDKMessage]: ...
    def messages(self) -> Iterator[SDKMessage]: ...
    def events(self) -> Iterator[RunStreamEvent]: ...
    def iter_text(self) -> Iterator[str]: ...
    def text(self) -> str: ...
    def wait(self) -> RunResult: ...
    def cancel(self) -> None: ...
    def conversation(self) -> list[ConversationTurn]: ...
    def conversation_json(self) -> str: ...
    def observe(self, *, after_offset: str | None = None) -> Iterator[RunStreamEvent]: ...

    def supports(self, operation: str) -> bool: ...
    def unsupported_reason(self, operation: str) -> str | None: ...
    def on_did_change_status(
        self, listener: Callable[[str], None]
    ) -> Callable[[], None]: ...
```

`run.stream()` is an alias for `run.messages()`. Iterating `run` directly yields `RunStreamEvent` envelopes, the same as `run.events()`.

`AsyncRun` exposes the same state fields, including `usage`. Methods that do I/O are async: `async for message in run.stream()`, `async for message in run.messages()`, `async for event in run.events()`, `async for text in run.iter_text()`, `await run.text()`, `await run.wait()`, `await run.cancel()`, `await run.conversation()`, `await run.conversation_json()`, and `async for event in run.observe()`.

### Streaming

```python
run = agent.send("Find the bug in src/auth.py")

for message in run.messages():
    if message.type == "assistant":
        for block in message.message.content:
            if block.type == "text":
                print(block.text, end="")
    elif message.type == "thinking":
        print(message.text, end="")
    elif message.type == "tool_call":
        print(f"[tool] {message.name}: {message.status}")
    elif message.type == "status":
        print(f"[status] {message.status}")
    elif message.type == "usage":
        print(f"[usage] turn total={message.usage.total_tokens}")
```

A run stream is consumable once. `run.messages()`, `run.events()`, and `run.iter_text()` all draw from the same underlying stream and advance it. Once the stream completes, the run holds the terminal result (`run.result`, `run.status`, `run.usage`, `run.git`, ...). Call `run.wait()` to drain any remaining events and return the typed `RunResult`.

### Waiting without streaming

```python
result = run.wait()

print(result.status)       # "finished" | "error" | "cancelled" | "expired"
print(result.result)       # final assistant text, if any
print(result.model)        # resolved ModelSelection used for this run
print(result.duration_ms)
print(result.usage)        # cumulative TokenUsage, or None if unavailable
print(result.git)          # RunGitInfo on cloud
```

Async equivalent:

```python
result = await run.wait()
```

### Token usage

Runs report token usage when the runtime provides it. Read the cumulative total from `run.usage` on the live handle (while streaming or after `wait()`), or from `result.usage` on the `RunResult` returned by `run.wait()`. Both hold a `TokenUsage` summed across every turn that reported usage, and both are `None` when no turn did—for example a cancelled run that never finished a turn, a runtime that doesn't surface usage, or a detached cloud snapshot that hasn't reconciled usage yet.

```python
@dataclass(frozen=True)
class TokenUsage:
    input_tokens: int
    output_tokens: int
    cache_read_tokens: int
    cache_write_tokens: int
    total_tokens: int
    reasoning_tokens: int | None = None
```

| Field                | Description                                                                                           |
| :------------------- | :---------------------------------------------------------------------------------------------------- |
| `input_tokens`       | Prompt tokens sent to the model.                                                                      |
| `output_tokens`      | Tokens generated by the model.                                                                        |
| `cache_read_tokens`  | Tokens served from the prompt cache.                                                                  |
| `cache_write_tokens` | Tokens written to the prompt cache.                                                                   |
| `total_tokens`       | `input_tokens + output_tokens + cache_read_tokens + cache_write_tokens`. Excludes `reasoning_tokens`. |
| `reasoning_tokens`   | Reasoning tokens, a subset of `output_tokens`. `None` when the model or runtime didn't report it.     |

```python
result = run.wait()

if result.usage is not None:
    print(f"total: {result.usage.total_tokens}")
    print(f"in: {result.usage.input_tokens}, out: {result.usage.output_tokens}")
    print(
        f"cache read/write: {result.usage.cache_read_tokens}/{result.usage.cache_write_tokens}"
    )
else:
    print("no usage reported for this run")
```

`reasoning_tokens` is already counted inside `output_tokens`, so `total_tokens` leaves it out to avoid double-counting.

For per-turn numbers as they stream, handle the `usage` [stream event](https://cursor.com/docs/sdk/python.md#stream-events) (`SDKUsageMessage`). It fires once at the end of each turn that reported usage and carries that turn's `TokenUsage`. `run.usage` and `result.usage` stay cumulative across the run. After stream turns, the handle prefers those summed totals; otherwise it uses usage from `wait()` or from a `get_run` / `list_runs` snapshot when the bridge supplies it.

```python
for message in run.messages():
    if message.type == "usage":
        print(f"turn used {message.usage.total_tokens} tokens")

# Or after wait / without consuming messages yourself:
result = run.wait()
print(run.usage, result.usage)
```

Async equivalent: `async for message in run.messages()` and `await run.wait()`. `run.usage` is still a sync property on `AsyncRun`.

`TokenUsage` is exported from `cursor_sdk` (plus `to_token_usage` / `sum_token_usage` for advanced callers). Wire JSON is camelCase (`inputTokens`, …); the Python dataclasses use snake\_case.

Token counts are what the runtime reports; they say nothing about cost. For billed usage and the dollar cost of an agent's runs, call [`agent.get_usage()`](https://cursor.com/docs/sdk/python.md#agentget_usage).

### Reading text output

`iter_text()` yields assistant text as it streams. `text()` returns the final terminal text, blocking on `wait()` if the run is still running.

```python
for chunk in run.iter_text():
    print(chunk, end="")

final_text = run.text()
```

Async equivalent:

```python
async for chunk in run.iter_text():
    print(chunk, end="")

final_text = await run.text()
```

### Cancelling a run

```python
run.cancel()
```

Async equivalent:

```python
await run.cancel()
```

`run.cancel()` requests cancellation of an active run. The status moves to `"cancelled"`, the live stream stops, in-flight tool calls stop, and `run.wait()` resolves with `status: "cancelled"`. Partial output (assistant text written so far) stays on the `Run` object.

Cancelling a run that is already terminal (`"finished"`, `"error"`, `"cancelled"`, `"expired"`) raises `UnsupportedRunOperationError`. Guard with `run.status` when in doubt:

```python
if run.status == "running":
    run.cancel()
```

### Reading run state

```python
print(run.id)
print(run.status)  # "running" | "finished" | "error" | "cancelled" | "expired"

stop = run.on_did_change_status(lambda status: print(f"status changed to {status}"))
stop()  # remove the listener

turns = run.conversation()
```

`run.conversation()` returns a typed `list[ConversationTurn]`. Use it to render or persist structured history without subscribing to the live stream. `run.conversation_json()` returns the raw JSON string.

For async runs, use `await run.conversation()` and `await run.conversation_json()`.

### Per-run model override

The `model` you pass to `agent.send()` overrides the agent's selection for that run, then becomes sticky: subsequent sends without an override continue to use the new model. To switch back, pass another `model` override or read the current selection from `agent.model`.

```python
from cursor_sdk import ModelParameterValue, ModelSelection, SendOptions

run = agent.send(
    "Plan the refactor",
    SendOptions(
        model=ModelSelection(
            id="composer-2.5",
            params=[ModelParameterValue(id="fast", value="true")],
        ),
    ),
)
```

`run.model` and `result.model` reflect the selection this run used and are immutable once the run starts.

### Per-run environment variables

Cloud agents can also take environment variables for a single run. Pass `cloud.env_vars` in `SendOptions` and the values are injected into the agent's shell for that run only — when the run finishes, they're removed from the VM and the next run doesn't see them. This is the right shape for credentials that rotate between turns, like a short-lived deploy token you mint right before asking the agent to use it.

```python
from cursor_sdk import CloudSendOptions, SendOptions

run = agent.send(
    "Deploy the preview environment",
    SendOptions(
        cloud=CloudSendOptions(env_vars={"DEPLOY_TOKEN": mint_short_lived_token()}),
    ),
)
```

If a run-scoped variable has the same name as an agent-scoped one from [`env_vars` on `CloudAgentOptions`](https://cursor.com/docs/sdk/python.md#session-environment-variables), the run-scoped value wins for that run, then the agent-scoped value comes back on the next run.

Per-run variables work on the first send too. The SDK passes them along with agent creation, scoped to the initial run, so they aren't persisted on the agent. Like agent-scoped variables, they're encrypted at rest and names can't start with `CURSOR_`.

Per-run environment variables are cloud agents only, and they aren't available for agents running against public repositories. For local agents, the agent process inherits your own environment, so set variables on the process before calling `send()`.

### Conversation mode

Pass `mode="plan"` or `mode="agent"` to control whether a run explores and plans first or implements changes directly. See [Plan mode](https://cursor.com/help/ai-features/plan-mode.md) for what plan mode does in the product.

Set `mode` in `AgentOptions` passed to `Agent.create()` to seed the first run. On follow-up `agent.send()` calls, omit `mode` to keep the conversation's current mode, or pass `mode` to switch for that run only.

```python
from cursor_sdk import Agent, AgentOptions, CloudAgentOptions, CloudRepository, SendOptions

with Agent.create(
    AgentOptions(
        model="composer-2.5",
        mode="plan",
        cloud=CloudAgentOptions(
            repos=[CloudRepository(url="https://github.com/your-org/your-repo")],
        ),
    )
) as agent:
    agent.send("Design the auth refactor").wait()
    agent.send(
        "Looks good, start building",
        SendOptions(mode="agent"),
    ).wait()
```

### Streaming raw deltas

Pass `on_delta` and `on_step` callbacks in `SendOptions` for lower-level updates. Sync callbacks are called inline. Async callbacks may be sync or async; awaitable return values are awaited before the next event is processed.

```python
from cursor_sdk import SendOptions

def on_delta(update):
    if update.type in ("text-delta", "thinking-delta"):
        print(update.text, end="")

run = agent.send(
    "Refactor the utils module",
    SendOptions(on_delta=on_delta, on_step=lambda step: print(f"[step] {step.type}")),
)
run.wait()
```

The concrete update and step subclasses live in `cursor_sdk.events`:

```python
from cursor_sdk.events import TextDeltaUpdate, ToolCallStartedUpdate

if isinstance(update, TextDeltaUpdate):
    print(update.text)
```

They remain importable from `cursor_sdk` for backward compatibility, but new code should import from `cursor_sdk.events`.

### SendOptions

| Property          | Type                                         | Description                                                                                                                                                                                                                              |
| :---------------- | :------------------------------------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `model`           | `str \| ModelSelection \| Mapping[str, Any]` | Per-send model override. If omitted, uses `agent.model`. Sticky after a successful send.                                                                                                                                                 |
| `mode`            | `"agent" \| "plan"`                          | Per-send conversation mode override. If omitted on follow-ups, keeps the conversation's current mode.                                                                                                                                    |
| `mcp_servers`     | `Mapping[str, McpServerConfig]`              | Inline MCP server definitions. Fully replaces creation-time servers for this run.                                                                                                                                                        |
| `cloud.env_vars`  | `Mapping[str, str]`                          | Cloud agents only. [Per-run environment variables](https://cursor.com/docs/sdk/python.md#per-run-environment-variables) injected for this run and removed when it finishes. Overrides agent-scoped `env_vars` by name for this run only. |
| `local.force`     | `bool`                                       | Local agents only. Defaults to `None` (unset). Set `True` to expire a stuck active run before starting this message. Cloud returns `409 agent_busy` server-side, so no equivalent is needed.                                             |
| `idempotency_key` | `str`                                        | Optional client-generated idempotency key for the send.                                                                                                                                                                                  |
| `on_step`         | `Callable[[ConversationStep], Any]`          | Callback after each completed conversation step (text, thinking, or tool batch).                                                                                                                                                         |
| `on_delta`        | `Callable[[InteractionUpdate], Any]`         | Callback per raw `InteractionUpdate`.                                                                                                                                                                                                    |

***

The next three sections are detailed reference for `SDKMessage`, `InteractionUpdate`, and `ConversationTurn`. Skim or skip on a first read; [Resuming agents](https://cursor.com/docs/sdk/python.md#resuming-agents) picks up the narrative.

## Stream events

`run.messages()` yields typed SDK message dataclasses. Discriminate on `message.type`. All messages include `agent_id` and `run_id` when the runtime provides them.

```python
SDKMessage = (
    SDKSystemMessage
    | SDKUserMessageEvent
    | SDKAssistantMessage
    | SDKThinkingMessage
    | SDKToolUseMessage
    | SDKStatusMessage
    | SDKTaskMessage
    | SDKRequestMessage
    | SDKUsageMessage
    | Mapping[str, Any]
)
```

| `type`        | Dataclass             | Key fields                                                                  |
| :------------ | :-------------------- | :-------------------------------------------------------------------------- |
| `"system"`    | `SDKSystemMessage`    | `subtype`, `model`, `tools`                                                 |
| `"user"`      | `SDKUserMessageEvent` | `message.content`                                                           |
| `"assistant"` | `SDKAssistantMessage` | `message.content` with `TextBlock` and `ToolUseBlock` values                |
| `"thinking"`  | `SDKThinkingMessage`  | `text`, `thinking_duration_ms`                                              |
| `"tool_call"` | `SDKToolUseMessage`   | `call_id`, `name`, `status`, `args`, `result`, `truncated`                  |
| `"status"`    | `SDKStatusMessage`    | `status`, `message`                                                         |
| `"task"`      | `SDKTaskMessage`      | `status`, `text`                                                            |
| `"request"`   | `SDKRequestMessage`   | `request_id`                                                                |
| `"usage"`     | `SDKUsageMessage`     | `usage` ([`TokenUsage`](https://cursor.com/docs/sdk/python.md#token-usage)) |

`SDKToolUseMessage` is emitted twice for most tool calls: first with `status="running"` and `args` populated, then again on completion with `status="completed"` (or `"error"`) and `result` populated. `truncated` flags whether the SDK truncated `args` or `result` because the payload was too large.

`SDKUsageMessage` is emitted once at the end of each turn that reported token usage, carrying that turn's [`TokenUsage`](https://cursor.com/docs/sdk/python.md#token-usage). The cumulative total across turns stays on `run.usage` and `result.usage`. See [Token usage](https://cursor.com/docs/sdk/python.md#token-usage).

```python
@dataclass(frozen=True)
class SDKUsageMessage:
    type: Literal["usage"]
    agent_id: str
    run_id: str
    usage: TokenUsage
```

Result data (final text, model, duration, cumulative token usage, git metadata) lives on the `Run` object after the stream completes. Use `run.wait()` to read it, including `result.usage` when the runtime reported it.

> **Tool call schema is not stable.** The `args` and `result` payloads on `tool_call` events reflect each tool's internal shape and can change as tools evolve. Tool names can also be renamed or replaced. Treat `args` and `result` as untyped data and parse defensively. The event envelope (`type`, `call_id`, `name`, `status`) is stable.

`run.events()` yields lower-level `RunStreamEvent` envelopes. Use it when you need offsets, terminal result envelopes, or raw interaction updates:

```python
for event in run.events():
    print(event.kind, event.offset)
```

## Interaction updates

`InteractionUpdate` is the raw delta type passed to the `on_delta` callback on `agent.send()`. Updates are finer-grained than `SDKMessage` events: text streams in token-by-token and tool calls report partial state as args accumulate.

```python
InteractionUpdate = (
    TextDeltaUpdate
    | ThinkingDeltaUpdate
    | ThinkingCompletedUpdate
    | ToolCallStartedUpdate
    | ToolCallCompletedUpdate
    | PartialToolCallUpdate
    | TokenDeltaUpdate
    | StepStartedUpdate
    | StepCompletedUpdate
    | TurnEndedUpdate
    | UserMessageAppendedUpdate
    | SummaryUpdate
    | SummaryStartedUpdate
    | SummaryCompletedUpdate
    | ShellOutputDeltaUpdate
    | UnknownInteractionUpdate
    | Mapping[str, Any]
)
```

`PartialToolCallUpdate` is emitted as the model streams arguments into a tool call before it commits. The same stability disclaimer that applies to `SDKToolUseMessage.args` applies here.

## Conversation types

The structured per-turn view of a run, returned by `run.conversation()`. Each item is a wrapper that carries the turn `type` discriminator alongside the typed payload in `turn`.

```python
@dataclass(frozen=True)
class ConversationTurn:
    type: str  # "agentConversationTurn" | "shellConversationTurn"
    turn: AgentConversationTurn | ShellConversationTurn | Mapping[str, Any]

@dataclass(frozen=True)
class AgentConversationTurn:
    user_message: Mapping[str, Any] | None = None
    steps: Sequence[ConversationStep] = ()

@dataclass(frozen=True)
class ShellConversationTurn:
    shell_command: ShellCommand | None = None
    shell_output: ShellOutput | None = None

ConversationStep = (
    AssistantConversationStep
    | ToolCallConversationStep
    | ThinkingConversationStep
    | Mapping[str, Any]
)
```

Discriminate on `turn.type` and read the payload through `turn.turn`:

```python
for turn in run.conversation():
    if turn.type == "agentConversationTurn":
        for step in turn.turn.steps:
            print(step.type)
    elif turn.type == "shellConversationTurn":
        print(turn.turn.shell_command, turn.turn.shell_output)
```

`run.conversation()` from `on_step` callbacks fires per `ConversationStep`, not per turn. Tool-call conversation steps carry a `Mapping[str, Any]` payload. Treat tool-call payload details as untyped data; see the [stability note](https://cursor.com/docs/sdk/python.md#stream-events) under Stream events.

## Resuming agents

```python
Agent.resume(
    agent_id: str,
    options: AgentOptions | Mapping[str, Any] | None = None,
    *,
    client: CursorClient | None = None,
) -> Agent
```

Use `Agent.resume()` or `client.agents.resume()` to reattach to an existing agent by ID. Common flows: reconnecting to a long-running cloud agent that was kicked off earlier, or continuing a conversation after the local process restarted. Runtime is auto-detected from the ID prefix (`bc-` is cloud, anything else is local).

```python
agent = Agent.resume("bc-abc123")
run = agent.send("Also update the changelog")
run.wait()
```

Async equivalent:

```python
agent = await client.agents.resume("bc-abc123")
run = await agent.send("Also update the changelog")
await run.wait()
```

`agent.model` is `None` on resume unless you pass `model` again. Inline MCP servers are not persisted across resume; they often carry secrets and live in memory only. Pass them again on resume, or use file-based MCP config (`.cursor/mcp.json` plus `local.setting_sources`) for servers that should survive.

### Local persistence

Local agents persist conversation state and run metadata through the bridge, so follow-ups and `Agent.resume()` survive a process restart. The bridge keeps this under a per-workspace state root on disk by default. Cloud agents persist server-side, so resuming a cloud agent from anywhere returns the same conversation.

Local persistence is workspace-scoped. When the bridge runs as a long-lived sidecar or subprocess, give it the same workspace as the agent so local list, get, and resume calls resolve the right agents. Set it once on the client and pass `cwd` to the local list and get calls:

```python
from cursor_sdk import CursorClient

with CursorClient.launch_bridge(workspace="/path/to/repo") as client:
    agents = client.agents.list(runtime="local", cwd="/path/to/repo")
    info = client.agents.get(agents.items[0].agent_id, cwd="/path/to/repo")
```

## Inspecting agents and runs

Use `CursorClient` for list, get, and pagination APIs.

```python
from cursor_sdk import CursorClient

with CursorClient.launch_bridge(workspace=".") as client:
    agents = client.agents.list(runtime="local", cwd=".")

    for agent_info in agents.auto_paging_iter():
        print(agent_info.agent_id)

    info = client.agents.get(agents.items[0].agent_id)
    runs = client.agents.list_runs(info.agent_id)
    run = client.agents.get_run(runs.items[0].id)
```

Async equivalent:

```python
agents = await client.agents.list(runtime="local", cwd=".")

async for agent_info in agents.auto_paging_iter():
    print(agent_info.agent_id)

info = await client.agents.get(agents.items[0].agent_id)
runs = await client.agents.list_runs(info.agent_id)
run = await client.agents.get_run(runs.items[0].id)
```

Use `agent.list_messages()` on an agent handle to read message history. `Agent.messages.list(agent_id)` is a typed-attribute convenience for the same call when you only have an ID.

Use `Agent.get_run(run_id)` or `client.agents.get_run(run_id)` to fetch a run
without an agent handle. Cancel it with
`Agent.cancel_run(run_id, agent_id=...)` or
`client.agents.cancel_run(run_id, agent_id=...)`. The async client methods are
awaitable and use the same arguments.

`AgentMessage` is distinct from a streamed `SDKMessage`:

```python
@dataclass(frozen=True)
class AgentMessage:
    type: str
    uuid: str
    agent_id: str
    message: Any = None
```

List endpoints return `ListResult[T]`. Use `.items` and `.next_cursor` directly, iterate the current page with `for item in page`, or iterate all pages with `.auto_paging_iter()`. Async list endpoints return `AsyncListResult[T]`; `async for item in page` walks the current page, and `async for item in page.auto_paging_iter()` walks every page in the result set.

### SDKAgentInfo

The metadata shape returned by `Agent.list()`, `Agent.get()`, `client.agents.list()`, and `client.agents.get()`.

```python
@dataclass(frozen=True)
class SDKAgentInfo:
    agent_id: str
    name: str
    summary: str
    last_modified: str | None = None
    status: str | None = None  # "running" | "finished" | "error"
    created_at: str | None = None
    archived: bool = False
    runtime: Literal["local", "cloud"] | None = None
    cwd: str = ""
    env: CloudEnvironment | None = None
    repos: Sequence[str] = ()
    metadata: Mapping[str, str] = {}  # from CloudAgentOptions.metadata; empty for local agents
```

### Cloud agent lifecycle

Cloud agents stay in your team's workspace until you archive or delete them. `client.agents.list(runtime="cloud")` hides archived agents by default; pass `include_archived=True` to see them. Filter by `pr_url` to find the agent that opened a specific pull request.

```python
# By ID, no agent handle required:
Agent.archive(agent_id)
Agent.unarchive(agent_id)
Agent.delete(agent_id)

# Through an explicit client:
client.agents.archive(agent_id)
client.agents.unarchive(agent_id)
client.agents.delete(agent_id)

# On an existing agent handle:
agent.archive()
agent.unarchive()
agent.delete()
```

`archive` soft-deletes the agent so the transcript stays readable. `unarchive` restores it. `delete` is permanent; subsequent reads return `NotFoundError`.

Async lifecycle methods use the same names and are awaitable.

### agent.get\_usage()

Fetch billed token usage and dollar cost for an agent's runs. Cloud agents return a per-run breakdown; local agents return a per-turn breakdown. Pass `run_id` to restrict the result to one entry: for cloud agents a `run-<uuid>` run ID, for local agents an ID from a previous `get_usage().runs[].run_id`.

```python
usage = agent.get_usage()

print(f"tokens: {usage.usage.total_tokens}")
if usage.cost is not None:
    print(f"charged: ${usage.cost.charged_cents / 100:.2f}")
for run in usage.runs:
    print(run.run_id, run.usage.total_tokens)
```

```python
@dataclass(frozen=True)
class AgentUsage:
    usage: TokenUsage              # summed across `runs`
    runs: Sequence[RunUsage] = ()
    cost: UsageCost | None = None  # summed across `runs`

@dataclass(frozen=True)
class RunUsage:
    run_id: str
    usage: TokenUsage
    cost: UsageCost | None = None

@dataclass(frozen=True)
class UsageCost:
    raw_cost_cents: float  # undiscounted model token cost; 0 for request-priced usage
    charged_cents: float   # amount charged, discounts and the Cursor Token Rate included
```

Cost includes discounts and can take a moment to settle after a run ends; `cost` is `None` until it does. `charged_cents` is `0.0` for plan-included, BYOK, and credit-grant usage.

This is a different view from [Token usage](https://cursor.com/docs/sdk/python.md#token-usage): `run.usage` is the live token count for one run, while `get_usage()` is the billed record across the agent's runs. On async agents, `await agent.get_usage()` matches. `AgentUsage`, `RunUsage`, and `UsageCost` are exported from `cursor_sdk`.

## The Cursor namespace

Account-level and catalog reads. Sync methods take optional `api_key` and otherwise fall back to `CURSOR_API_KEY`.

```python
from cursor_sdk import Cursor

me = Cursor.me()
models = Cursor.models.list()
repositories = Cursor.repositories.list()
```

Explicit-client equivalent:

```python
me = client.me()
models = client.models.list()
repositories = client.repositories.list()
```

Async equivalent:

```python
from cursor_sdk import AsyncCursor

me = await AsyncCursor.me(client=client)
models = await AsyncCursor.models.list(client=client)
repositories = await AsyncCursor.repositories.list(client=client)
```

`Cursor.me()` returns an `SDKUser` with `api_key_name`, `created_at`, and
optional `user_id`, `user_email`, `user_first_name`, and `user_last_name`
fields.

Use `Cursor.models.list()` to discover valid model IDs and per-model parameters before calling `Agent.create()` or `agent.send()`. Parameters are model-specific. Common examples are reasoning effort and Cursor Router's `optimize_for` on `auto-smart`.

The catalog is account- and team-specific. Cursor Router only appears as `auto-smart` when Router is available for the API key's team. See [Cursor Router](https://cursor.com/docs/sdk/python.md#cursor-router).

```python
models = Cursor.models.list()
composer = next((model for model in models if model.id == "composer-2.5"), None)

print(composer.parameters if composer else [])
# [
#   ModelParameterDefinition(
#       id="fast",
#       display_name="Fast",
#       values=(
#           ModelParameterDefinitionValue(value="false"),
#           ModelParameterDefinitionValue(value="true", display_name="Fast"),
#       ),
#   ),
# ]
```

Preset `variants` on each `SDKModel` already contain valid `params`, so you can copy them into a `ModelSelection`.

Prefer an explicit Router selection (`auto-smart` + `optimize_for`) when a target model is missing and you want Cost, Balance, or Intelligence. Fall back to `ModelSelection(id="auto")` only when you want server-selected Auto without choosing a Router mode. For Cursor Router, always pass `optimize_for` explicitly.

`Cursor.repositories.list()` returns the SCM repositories (GitHub, GitLab, Bitbucket, Azure DevOps, depending on what's connected) available for cloud agents on the calling account or team. Each item exposes a `url`. Use these to populate `CloudAgentOptions.repos`.

## MCP servers

Agents can pick up MCP servers from inline definitions, project/user settings, plugins, and dashboard-managed configuration depending on the runtime.

```python
from cursor_sdk import (
    Agent,
    AgentOptions,
    HttpMcpServerConfig,
    LocalAgentOptions,
    McpAuth,
    StdioMcpServerConfig,
)

agent = Agent.create(
    AgentOptions(
        model="composer-2.5",
        local=LocalAgentOptions(cwd="."),
        mcp_servers={
            "docs": HttpMcpServerConfig(
                url="https://example.com/mcp",
                auth=McpAuth(client_id="client-id", scopes=["read", "write"]),
            ),
            "filesystem": StdioMcpServerConfig(
                command="npx",
                args=["-y", "@modelcontextprotocol/server-filesystem", "."],
            ),
        },
    )
)
```

Flat dictionaries (`{"type": "http", "url": ...}` and `{"type": "stdio", "command": ...}`) are also accepted as a quick-script convenience.

### What gets loaded

**Local agents** load servers from up to five sources, with first-match-wins precedence on conflicting names:

1. `mcp_servers` on `agent.send()`. Fully replaces creation-time servers for that run (not merged).
2. `mcp_servers` on `Agent.create()`. Used when no per-send override is provided.
3. Plugin servers, if `local.setting_sources` includes `"plugins"`.
4. Project servers from `.cursor/mcp.json`, if `local.setting_sources` includes `"project"`.
5. User servers from `~/.cursor/mcp.json`, if `local.setting_sources` includes `"user"`.

Without `local.setting_sources`, only inline servers are loaded. If a local MCP server requires OAuth login, the SDK can reuse a saved login from the Cursor app, but it cannot open a browser to sign you in.

**Cloud agents** load servers from:

1. `mcp_servers` on `agent.send()`. Fully replaces creation-time servers for that run (not merged).
2. `mcp_servers` on `Agent.create()`. Used when no per-send override is provided.
3. Your user and team MCP servers from [cursor.com/agents](https://cursor.com/agents).

If an inline server doesn't include `auth` or `headers` and you've previously authorized that server URL on cursor.com/agents, runs authenticated with a personal API token reuse those OAuth tokens automatically. Service account API keys cannot fall back to user auth as they are not associated with a user.

`local.setting_sources` does not apply to cloud agents.

### Cloud

Cloud agents accept authenticated MCP configs inline too. Cloud MCP supports HTTP and stdio transports. Use HTTP `headers` for static API keys or Bearer tokens. Use HTTP `auth` for OAuth-protected servers. Use stdio `env` when the server runs inside the cloud VM and reads credentials from environment variables.

```python
from cursor_sdk import (
    Agent,
    AgentOptions,
    CloudAgentOptions,
    CloudRepository,
    HttpMcpServerConfig,
    StdioMcpServerConfig,
)

agent = Agent.create(
    AgentOptions(
        model="composer-2.5",
        cloud=CloudAgentOptions(
            repos=[CloudRepository(url="https://github.com/your-org/your-repo")],
        ),
        mcp_servers={
            "linear": HttpMcpServerConfig(
                url="https://mcp.linear.app/mcp",
                headers={"Authorization": "Bearer linear_pat_xxx"},
            ),
            "github": StdioMcpServerConfig(
                command="npx",
                args=["-y", "@modelcontextprotocol/server-github"],
                env={"GITHUB_TOKEN": "ghp_xxx"},
            ),
        },
    )
)
```

- HTTP `headers` and `auth` are handled by Cursor's backend. Sensitive fields are redacted and do not enter the VM.
- Stdio `env` values are passed into the VM because the server runs there. Treat them like any other runtime secret.
- OAuth for MCP servers configured on cursor.com/agents stays per-user, even for team-level servers.

See [MCP](https://cursor.com/docs/mcp.md) for the full config format and [Cloud Agent capabilities](https://cursor.com/docs/cloud-agent/capabilities.md#mcp-tools) for cloud-specific behavior.

## Subagents

Define named subagents that the main agent can spawn via the `Agent` tool. Pass them inline:

```python
from cursor_sdk import Agent, AgentDefinition, AgentOptions, LocalAgentOptions

agent = Agent.create(
    AgentOptions(
        model="composer-2.5",
        local=LocalAgentOptions(cwd="."),
        agents={
            "code-reviewer": AgentDefinition(
                description="Expert code reviewer for quality and security.",
                prompt="Review code for bugs, security issues, and proven approaches.",
                model="inherit",
            ),
            "test-writer": AgentDefinition(
                description="Writes tests for code changes.",
                prompt="Write comprehensive tests for the given code.",
            ),
        },
    )
)
```

Subagents committed to the repo at `.cursor/agents/*.md` (with `name`, `description`, and optional `model` frontmatter) are also picked up. Inline definitions override file-based ones with the same name.

### Nested subagents

Subagents can spawn their own subagents, within a nesting limit. When a subagent uses the `Agent` tool, it reaches the same subagent executor the parent has, so a parent can delegate to a subagent that delegates further. Each level sees the same set of named subagents. The top-level agent and its direct subagents can launch subagents, but a subagent launched by another subagent can't launch further ones.

## Restricting the toolset

`tools` allowlists the built-in tools offered to the model; `disallowed_tools` removes tools and keeps the rest, including tools added to the platform after your SDK version was released. Both are local agents only for now, and neither persists on the agent: pass them again on resume to keep the restriction.

```python
from cursor_sdk import Agent, AgentOptions, LocalAgentOptions

# Read-only agent: only these tools are offered.
reader = Agent.create(
    AgentOptions(
        model="composer-2.5",
        tools=["read", "grep", "glob", "ls"],
        local=LocalAgentOptions(cwd="."),
    )
)

# Everything except shell access.
no_shell = Agent.create(
    AgentOptions(
        model="composer-2.5",
        disallowed_tools=["shell"],
        local=LocalAgentOptions(cwd="."),
    )
)
```

- Omitting `tools` offers the standard toolset for the selected model; `tools=[]` offers no built-in tools, so the model can only respond with text.
- Both fields accept public names (`"read"`, `"edit"`, `"task"`, `"webSearch"`, ...) and the capability groups `"shell"` and `"mcp"`. Unknown names raise `BadRequestError` at creation.
- Deny wins: a tool must be in `tools` (when set) and not in `disallowed_tools` to be offered.
- Disallowing `"mcp"` also removes [custom tools](https://cursor.com/docs/sdk/python.md#custom-tools). Disallowing `"task"` prevents [subagents](https://cursor.com/docs/sdk/python.md#subagents); otherwise subagents keep their own curated toolsets.

## Custom tools

Custom tools let you expose Python functions to local agents without standing up a separate MCP server. Pass them on `LocalAgentOptions.custom_tools`.

```python
from cursor_sdk import Agent, CustomTool, CustomToolContext, LocalAgentOptions

def get_deployment_status(args, context: CustomToolContext):
    service = args["service"]
    return f"Service {service} is healthy."

with Agent.create(
    model="composer-2.5",
    local=LocalAgentOptions(
        cwd=".",
        custom_tools={
            "get_deployment_status": CustomTool(
                description="Look up the current deployment status for a service.",
                input_schema={
                    "type": "object",
                    "properties": {
                        "service": {"type": "string", "description": "Service name"},
                    },
                    "required": ["service"],
                },
                execute=get_deployment_status,
            ),
        },
    ),
) as agent:
    agent.send("Is the checkout service healthy?").wait()
```

`execute` receives the parsed arguments and a `CustomToolContext` with `tool_call_id` when available. It can return a string, a JSON-compatible value, or a mapping with a `content` list. Custom tools are local agents only.

## Hooks

Hooks are file-based only. There is no programmatic hook callback. Hooks are a project policy boundary, not a per-run knob.

- **Local:** Add `.cursor/hooks.json` to the repo passed as `local.cwd`, or add `~/.cursor/hooks.json` for user-level hooks.
- **Cloud:** Commit `.cursor/hooks.json` and its scripts to the repo passed in `cloud.repos`. SDK-created cloud agents load project hooks automatically. On Enterprise plans, they also run team hooks and enterprise-managed hooks.

See [Hooks](https://cursor.com/docs/hooks.md) for the configuration format and [Cloud Agents hooks support](https://cursor.com/docs/cloud-agent.md#hooks-support) for cloud behavior.

## Artifacts

List and download files from the agent's workspace.

```python
@dataclass(frozen=True)
class SDKArtifact:
    path: str
    size_bytes: int = 0
    updated_at: str = ""
```

```python
from pathlib import Path

artifacts = agent.list_artifacts()

for artifact in artifacts:
    print(artifact.path, artifact.size_bytes)

# Download a single artifact to disk.
content = agent.download_artifact(artifacts[0].path)
Path("review.md").write_bytes(content)
```

Async agents expose `await agent.list_artifacts()` and `await agent.download_artifact(path)`.

Artifact support is runtime-dependent. Local SDK agents return an empty list from `list_artifacts()` and raise from `download_artifact()`.

## Resource management

Always close agents when done. The cleanest sync pattern is a context manager:

```python
from cursor_sdk import Agent, LocalAgentOptions

with Agent.create(model="composer-2.5", local=LocalAgentOptions(cwd=".")) as agent:
    agent.send("Summarize the repository").wait()
```

To dispose explicitly:

```python
agent.close()
```

Async agents and clients support async context managers and `await` cleanup:

```python
from cursor_sdk import AsyncClient, LocalAgentOptions

async with await AsyncClient.launch_bridge(workspace=".") as client:
    async with await client.agents.create(
        model="composer-2.5",
        local=LocalAgentOptions(cwd="."),
    ) as agent:
        run = await agent.send("Summarize the repository")
        await run.wait()
```

To dispose explicitly:

```python
await agent.close()
await client.aclose()
```

The module-level sync default client is closed automatically at process exit. Long-running processes can close and reset it explicitly:

```python
from cursor_sdk import close_default_client

close_default_client()
```

## Configuration reference

The Python SDK accepts helper dataclasses and raw dictionaries. Dataclasses use Python `snake_case` fields and are preferred for application code.

### AgentOptions

| Property           | Type                                                 | Default                                                             | Description                                                                                                                                                                           |
| :----------------- | :--------------------------------------------------- | :------------------------------------------------------------------ | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `model`            | `str \| ModelSelection \| Mapping[str, Any]`         | Required for local; cloud falls back to the server-resolved default | Model to use. See [`ModelSelection`](https://cursor.com/docs/sdk/python.md#modelselection).                                                                                           |
| `api_key`          | `str`                                                | `CURSOR_API_KEY` env                                                | User API key or service account key. Team Admin keys are not yet supported.                                                                                                           |
| `name`             | `str`                                                | Auto-generated                                                      | Human-readable agent name surfaced in `client.agents.list()` / `client.agents.get()`.                                                                                                 |
| `local`            | `LocalAgentOptions \| Mapping[str, Any]`             | `None`                                                              | Local agent config. Pass to create a local agent.                                                                                                                                     |
| `cloud`            | `CloudAgentOptions \| Mapping[str, Any]`             | `None`                                                              | Cloud agent config. Pass to create a cloud agent.                                                                                                                                     |
| `mcp_servers`      | `Mapping[str, McpServerConfig]`                      | `None`                                                              | Inline MCP server definitions.                                                                                                                                                        |
| `agents`           | `Mapping[str, AgentDefinition \| Mapping[str, Any]]` | `None`                                                              | Subagent definitions.                                                                                                                                                                 |
| `tools`            | `Sequence[str]`                                      | Default toolset                                                     | Only the listed built-in tools are offered to the model. `[]` means no built-in tools; the model can only respond with text. Local agents only.                                       |
| `disallowed_tools` | `Sequence[str]`                                      | `None`                                                              | Removes the listed built-in tools; everything else stays available. Deny wins when combined with `tools`. Local agents only.                                                          |
| `agent_id`         | `str`                                                | Auto-generated                                                      | Durable agent ID. Pass to keep a stable ID across invocations.                                                                                                                        |
| `idempotency_key`  | `str`                                                | Auto-generated for cloud                                            | Optional client-generated idempotency key. Cloud only.                                                                                                                                |
| `mode`             | `"agent" \| "plan"`                                  | `None`                                                              | Initial conversation mode for the agent's first run. When omitted, the server starts in agent mode. See [Conversation mode](https://cursor.com/docs/sdk/python.md#conversation-mode). |

### LocalAgentOptions

| Property          | Type                                            | Default | Description                                                                                                                         |
| :---------------- | :---------------------------------------------- | :------ | :---------------------------------------------------------------------------------------------------------------------------------- |
| `cwd`             | `str \| os.PathLike`                            | `None`  | Primary working directory. Multi-entry lists are rejected; use `dirs` for multi-root.                                               |
| `dirs`            | `Sequence[str \| os.PathLike]`                  | `None`  | Additional workspace folders for multi-root setups. Merged with `cwd` so rules, skills, and workspace context load from every path. |
| `setting_sources` | `Sequence[SettingSource]`                       | `None`  | Ambient settings layers: `"project"`, `"user"`, `"team"`, `"mdm"`, `"plugins"`, or `"all"`.                                         |
| `sandbox_options` | `SandboxOptions \| Mapping[str, Any]`           | `None`  | Local sandbox options.                                                                                                              |
| `store`           | `LocalAgentStoreConfig \| Mapping[str, Any]`    | `None`  | Local store config passed to the bridge.                                                                                            |
| `auto_review`     | `bool`                                          | `None`  | Route local tool calls through Auto-review when the connected backend supports it.                                                  |
| `custom_tools`    | `Mapping[str, CustomTool \| Mapping[str, Any]]` | `None`  | [Custom tools](https://cursor.com/docs/sdk/python.md#custom-tools) exposed to local agents.                                         |

### CloudAgentOptions

| Property                    | Type                                             | Default                                                | Description                                                                                                                                                                                                                    |
| :-------------------------- | :----------------------------------------------- | :----------------------------------------------------- | :----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `env`                       | `CloudEnvironment \| Mapping[str, Any]`          | `None`                                                 | Execution environment. When omitted, the server uses Cursor-hosted cloud VMs. `pool` and `machine` target self-hosted workers you run.                                                                                         |
| `repos`                     | `Sequence[CloudRepository \| Mapping[str, Any]]` | `None`                                                 | Repositories to clone into the VM. Omit or pass `[]` for a [no-repo agent](https://cursor.com/docs/sdk/python.md#no-repo-cloud-agents) with an empty workspace. Pass `pr_url` on a repo to attach the agent to an existing PR. |
| `work_on_current_branch`    | `bool`                                           | `None`                                                 | Push commits to the existing branch instead of a new one. The server treats an omitted value as `False`.                                                                                                                       |
| `auto_create_pr`            | `bool`                                           | `None`                                                 | Open a PR when the run finishes. The server treats an omitted value as `False`.                                                                                                                                                |
| `open_as_cursor_github_app` | `bool`                                           | `True` for service-account keys, `False` for user keys | Open PRs as the Cursor GitHub App instead of the API key's owner. The resolved value is echoed on create, get, and list.                                                                                                       |
| `skip_reviewer_request`     | `bool`                                           | `None`                                                 | Skip requesting the calling user as a reviewer on the PR. The server treats an omitted value as `False`.                                                                                                                       |
| `env_vars`                  | `Mapping[str, str]`                              | `None`                                                 | Session-scoped environment variables for cloud agents.                                                                                                                                                                         |
| `metadata`                  | `Mapping[str, str]`                              | `None`                                                 | Caller-owned string tags persisted on the cloud agent. See [Agent metadata](https://cursor.com/docs/sdk/python.md#agent-metadata).                                                                                             |

### AgentDefinition

| Property      | Type                                                             | Default    | Description                                                                                      |
| :------------ | :--------------------------------------------------------------- | :--------- | :----------------------------------------------------------------------------------------------- |
| `description` | `str`                                                            | *required* | When to use this subagent. Shown to the parent agent so it knows when to spawn.                  |
| `prompt`      | `str`                                                            | *required* | System prompt for the subagent.                                                                  |
| `model`       | `str \| ModelSelection \| Mapping[str, Any] \| "inherit"`        | `None`     | Model override. `None` and `"inherit"` both use the parent's selection.                          |
| `mcp_servers` | `Sequence[str \| AgentDefinitionMcpServer \| Mapping[str, Any]]` | `None`     | MCP servers available to this subagent. Names reference servers from the parent's `mcp_servers`. |

### CustomTool

```python
@dataclass
class CustomTool:
    execute: Callable[[Mapping[str, Any], CustomToolContext], Any]
    description: str | None = None
    input_schema: Mapping[str, Any] | None = None

class CustomToolContext:
    tool_call_id: str | None = None
```

### ModelSelection

```python
@dataclass(frozen=True)
class ModelSelection:
    id: str
    params: Sequence[ModelParameterValue] = ()

@dataclass(frozen=True)
class ModelParameterValue:
    id: str
    value: str
```

`id` is the model identifier (for example, `"composer-2.5"` or `"auto-smart"`). `params` carries per-model parameters such as reasoning effort or Router's `optimize_for`. Use `Cursor.models.list()` to discover valid IDs, parameter definitions, and preset variants for your account. See [Cursor Router](https://cursor.com/docs/sdk/python.md#cursor-router) for the Router selection contract.

### McpServerConfig

```python
from cursor_sdk.types import McpServerConfig

@dataclass(frozen=True)
class HttpMcpServerConfig:
    url: str
    type: Literal["http", "sse"] | str = "http"
    headers: Mapping[str, str] | None = None
    auth: McpAuth | Mapping[str, Any] | None = None

@dataclass(frozen=True)
class SseMcpServerConfig(HttpMcpServerConfig):
    type: Literal["sse"] = "sse"

@dataclass(frozen=True)
class StdioMcpServerConfig:
    command: str
    args: Sequence[str] | None = None
    env: Mapping[str, str] | None = None
    cwd: str | os.PathLike | None = None  # local only; cloud rejects this field

@dataclass(frozen=True)
class McpAuth:
    client_id: str
    client_secret: str | None = None
    scopes: Sequence[str] = ()
```

For HTTP servers running in the cloud, `headers` and `auth` are handled by Cursor's backend. Sensitive fields are redacted before the VM sees them. For stdio servers in the cloud, `env` values are passed into the VM (treat them like any runtime secret).

### UserMessage

```python
@dataclass(frozen=True)
class UserMessage:
    text: str
    images: Sequence[SDKImage | Mapping[str, Any]] | None = None
```

The structured form of `agent.send()`'s message argument. Use it to send images alongside text.

### SDKImage

```python
@dataclass(frozen=True)
class SDKImage:
    url: str | None = None
    data: str | None = None
    mime_type: str | None = None
    dimension: SDKImageDimension | Mapping[str, Any] | None = None

    @classmethod
    def from_url(cls, url: str, dimension=None) -> SDKImage: ...

    @classmethod
    def from_data(cls, data: bytes | str, mime_type: str, dimension=None) -> SDKImage: ...

    @classmethod
    def url_image(cls, url: str, dimension=None) -> SDKImage: ...

    @classmethod
    def data_image(cls, data: str, mime_type: str, dimension=None) -> SDKImage: ...

    @classmethod
    def from_file(cls, path, *, mime_type=None, dimension=None) -> SDKImage: ...
```

Pass either a remote `url` or base64 `data` with a `mime_type`. `from_data()` accepts bytes or a base64 string. `from_file()` reads a file from disk and base64-encodes it.

### SettingSource

`SettingSource` is available from `cursor_sdk.types`.

```python
from cursor_sdk.types import SettingSource
```

Controls which on-disk settings layers a local agent loads. Cloud agents always load `project`, `team`, and `plugins` and ignore this field.

| Value       | Source                                  |
| :---------- | :-------------------------------------- |
| `"project"` | `.cursor/` in the workspace             |
| `"user"`    | `~/.cursor/`                            |
| `"team"`    | Team settings synced from the dashboard |
| `"mdm"`     | MDM-managed enterprise settings         |
| `"plugins"` | Plugin-provided settings                |
| `"all"`     | Shorthand for all of the above          |

### ListResult

```python
@dataclass(frozen=True)
class ListResult(Generic[T]):
    items: list[T]
    next_cursor: str = ""

    def to_dict(self) -> dict[str, Any]: ...
    def has_next_page(self) -> bool: ...
    def next_page_info(self) -> dict[str, str]: ...
    def get_next_page(self) -> ListResult[T]: ...
    def auto_paging_iter(self) -> Iterator[T]: ...
```

Returned by `client.agents.list()`, `client.agents.list_runs()`, and `Agent.list()`. `next_cursor` is empty when there are no more pages. Async list endpoints return `AsyncListResult[T]` with awaitable equivalents.

## Errors

All SDK errors extend `CursorAgentError`. `CursorSDKError` is the backward-compatible alias root for older callers. Use `is_retryable` and `retry_after` to drive retry logic.

```python
class CursorAgentError(Exception):
    message: str
    code: str | None
    status: int | None
    status_code: int | None
    details: list[Mapping[str, Any]]
    is_retryable: bool
    cause: BaseException | None
    proto_error_code: str | None
    request_id: str | None
    headers: Mapping[str, str]
    retry_after: str | None
```

| Error                          | When                                                                                                                    |
| :----------------------------- | :---------------------------------------------------------------------------------------------------------------------- |
| `AuthenticationError`          | Invalid API key or not logged in.                                                                                       |
| `PermissionDeniedError`        | Authenticated caller does not have permission for the requested operation.                                              |
| `RateLimitError`               | Too many requests or usage limits exceeded.                                                                             |
| `ConfigurationError`           | Invalid model, missing required configuration, or bad request parameters.                                               |
| `AgentBusyError`               | Sending a follow-up while the agent already has a run in `CREATING` or `RUNNING` state (HTTP `409`, code `agent_busy`). |
| `BadRequestError`              | Request is malformed.                                                                                                   |
| `IntegrationNotConnectedError` | Creating a cloud agent for a repo whose SCM provider is not connected.                                                  |
| `NetworkError`                 | Service unavailable or network failure.                                                                                 |
| `APITimeoutError`              | Request timed out.                                                                                                      |
| `InternalServerError`          | Cursor service returned a server error.                                                                                 |
| `NotFoundError`                | Requested resource was not found.                                                                                       |
| `AgentNotFoundError`           | Agent does not exist or isn't visible under the current working directory.                                              |
| `UnsupportedRunOperationError` | Run operation is not supported for the current run state.                                                               |

### Retrying with backoff

`is_retryable` and `retry_after` drive caller-side retry logic. `retry_after` is an HTTP-style string (seconds, or an HTTP date) supplied by the server when it's set.

```python
import time

from cursor_sdk import Agent, AgentOptions, CursorAgentError, LocalAgentOptions, RateLimitError

for attempt in range(3):
    try:
        result = Agent.prompt(
            "Audit the auth middleware for missing input validation",
            AgentOptions(model="composer-2.5", local=LocalAgentOptions(cwd=".")),
        )
        break
    except RateLimitError as err:
        time.sleep(float(err.retry_after) if err.retry_after else 2**attempt)
    except CursorAgentError as err:
        if not err.is_retryable:
            raise
        time.sleep(2**attempt)
```

Every `CursorAgentError` includes `request_id` when the server returned one. Log it whenever you surface an error so support has a handle on the failure.

### IntegrationNotConnectedError

```python
class IntegrationNotConnectedError(ConfigurationError):
    provider: str   # e.g. "github", "gitlab", "azuredevops"
    help_url: str   # dashboard link to reconnect
```

Use `help_url` to point the user at the right reconnect flow. New providers may be added without an SDK release.

### AgentBusyError

Cloud agents allow only one active run at a time. `AgentBusyError` is raised when you call `agent.send()` (or otherwise create a run) while another run on the same agent is still `CREATING` or `RUNNING`.

`is_retryable` is `False`. Retrying immediately will keep failing until the active run reaches a terminal status or you cancel it. Other `409` responses, such as `agent_archived`, raise `ConfigurationError` instead.

Wait for the active run to finish, cancel it with `run.cancel()`, or poll `Agent.list_runs()` before sending again:

```python
from cursor_sdk import Agent, AgentBusyError

agent = Agent.resume("bc-00000000-0000-0000-0000-000000000001")

try:
    agent.send("Also add tests for the auth middleware.")
except AgentBusyError:
    runs = Agent.list_runs(agent.agent_id, {"runtime": "cloud", "limit": 1})
    active = runs.items[0] if runs.items else None
    if active is not None and active.status == "running":
        active.cancel()
    agent.send("Also add tests for the auth middleware.")
```

Local agents do not raise `AgentBusyError`. Pass `local={"force": True}` on `send()` to expire a stuck local run before starting a new one.

### UnsupportedRunOperationError

```python
class UnsupportedRunOperationError(ConfigurationError):
    operation: str
```

Raised when a `Run` operation is not allowed on the current run. The most common case is `run.cancel()` on a run that's already terminal.

`run.supports(operation)` and `run.unsupported_reason(operation)` report SDK-level capability for an operation name (`"stream"`, `"wait"`, `"cancel"`, `"conversation"`) and do not check run state. Read `run.status` to guard state-sensitive calls.

## Troubleshooting

Set `CURSOR_SDK_LOG=debug` (or `info`) to attach a stderr handler to the SDK's own logger. The SDK only configures its own `cursor_sdk` logger, so this won't interfere with the host application's logging setup.

```bash
CURSOR_SDK_LOG=debug python my_script.py
```

The bundled bridge binary is installed as `cursor-sdk-bridge` on PATH alongside the package. Run it directly to confirm the build shipped with your wheel:

```bash
cursor-sdk-bridge --help
```

## Known limitations

- Tool-call payload schemas are intentionally not strongly typed.
- Inline MCP servers are not persisted across `Agent.resume()`. Pass them again on resume if needed.
- Custom tools (`local.custom_tools`) and toolset restrictions (`tools`, `disallowed_tools`) are local agents only. The restrictions don't persist on the agent; pass them again on resume.
- Artifact download is not implemented for local agents.
- `local.setting_sources` (and the file-based MCP and subagent paths it gates) does not apply to cloud agents. Cloud always loads `project`, `team`, and `plugins`.
- Hooks are file-based only (`.cursor/hooks.json`). No programmatic callbacks.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
