# Run Cursor in your evals

Use the [Cursor SDK](https://cursor.com/docs/sdk/typescript.md) to run Cursor's agent loop inside your own eval harness. The same agent that powers the Cursor IDE, CLI, and web app is scriptable from TypeScript, so you can score Grok 4.6 (and other models we support) on your benchmarks.

Benchmark authors like [Artificial Analysis](https://x.com/ArtificialAnlys/status/2057277363789197561) and [SWE-rebench](https://swe-rebench.com/) already use it to score Cursor on their leaderboards.

## Why the SDK

Eval harnesses need a stable, programmatic interface to an agent: pass a task, get a transcript and final state, score the result. The SDK gives you that without shelling out to the CLI:

- **Real agent loop.** Tool calls, file edits, terminal commands, and reasoning run through the same code path as the product.
- **Grok-first, multi-model.** Grok 4.6 is the default to evaluate, but any model in Cursor's catalog works through the same API.
- **Local or sandboxed cloud runtime.** Run against a working tree on disk for fast iteration, or use Cursor's hosted VMs for isolated, parallel runs.
- **Structured streams and results.** Typed `SDKMessage` events, per-step deltas, and a final `RunResult` with model, duration, and git info.

For the full API surface, see the [Cursor SDK reference](https://cursor.com/docs/sdk/typescript.md).

## Setup

### Install the SDK

```bash
npm install @cursor/sdk
```

### Get an API key

Generate a key from [Cursor Dashboard → API Keys](https://cursor.com/dashboard/api). Service account keys from [Team settings](https://cursor.com/dashboard/team-settings) also work.

```bash
export CURSOR_API_KEY="your-key"
```

### Pick a runtime

| Runtime   | What it does                                                  | When to use                                                                             |
| :-------- | :------------------------------------------------------------ | :-------------------------------------------------------------------------------------- |
| **Local** | Runs the agent against a working tree on disk.                | Reproducible repo-based tasks where you control the checkout.                           |
| **Cloud** | Runs in an isolated Cursor-hosted VM with the repo cloned in. | Parallel runs, untrusted code execution, or harnesses that don't have the repo locally. |

## Evaluating Grok 4.6

A single-task eval against Grok 4.6 on a local working tree:

```typescript
import { Agent } from "@cursor/sdk";

const result = await Agent.prompt(
  "Implement the failing tests in tests/string_utils.test.ts. Do not modify the tests.",
  {
    apiKey: process.env.CURSOR_API_KEY!,
    model: { id: "grok-4.6" },
    local: { cwd: "/path/to/task/checkout" },
  },
);

console.log(result.status);     // "finished" | "error" | "cancelled"
console.log(result.result);     // final assistant text
console.log(result.durationMs); // wall-clock duration
```

`Agent.prompt()` creates an agent, sends one prompt, waits for the run to finish, and disposes. It's the right primitive for stateless eval tasks.

After the run completes, score the working tree with whatever your harness already uses (test runner, judge model, exact-match checker, etc.).

### Streaming events for transcripts

Most harnesses want the full transcript, not the final text. Open a long-lived agent and stream `SDKMessage` events:

```typescript
import { Agent, type SDKMessage } from "@cursor/sdk";

await using agent = await Agent.create({
  apiKey: process.env.CURSOR_API_KEY!,
  model: { id: "grok-4.6" },
  local: { cwd: taskCwd },
});

const run = await agent.send(taskPrompt);
const transcript: SDKMessage[] = [];

for await (const event of run.stream()) {
  transcript.push(event);

  if (event.type === "assistant") {
    for (const block of event.message.content) {
      if (block.type === "text") process.stdout.write(block.text);
    }
  }
  if (event.type === "tool_call" && event.status === "completed") {
    console.log(`[tool] ${event.name}`);
  }
}

const final = await run.wait();
saveTranscript(transcript, final);
```

`run.stream()` yields typed events for assistant text, thinking, tool calls (start and completion), and lifecycle status. Final metadata (model, duration, git info) reads off the `Run` after the stream ends. See [Stream events](https://cursor.com/docs/sdk/typescript.md#stream-events) for the full event schema.

## Evaluating other models

The same harness code evaluates any model in Cursor's catalog. Swap the `id`:

```typescript
const models = ["grok-4.6", "composer-2.5", "gpt-5.6-sol", "claude-opus-5", "gemini-3.1-pro"];

for (const id of models) {
  const result = await Agent.prompt(taskPrompt, {
    apiKey: process.env.CURSOR_API_KEY!,
    model: { id },
    local: { cwd: taskCwd },
  });

  recordScore(id, scoreTask(result));
}
```

The agent loop, tool schema, prompts, and stream shape stay constant across models, so you measure model-level differences instead of harness drift. List supported ids with [`Cursor.models.list()`](https://cursor.com/docs/sdk/typescript.md#cursormodelslist).

## Running tasks in parallel (cloud)

For large eval sets, run each task in an isolated cloud VM. The VM clones the repo, runs the agent, and surfaces git results back to your harness.

```typescript
import { Agent } from "@cursor/sdk";

async function runTask(task: EvalTask) {
  await using agent = await Agent.create({
    apiKey: process.env.CURSOR_API_KEY!,
    model: { id: "grok-4.6" },
    cloud: {
      repos: [{ url: task.repoUrl, startingRef: task.baseRef }],
    },
  });

  const run = await agent.send(task.prompt);
  const result = await run.wait();

  return {
    taskId: task.id,
    status: result.status,
    branch: result.git?.branches[0]?.branch,
    durationMs: result.durationMs,
  };
}

const results = await Promise.all(tasks.map(runTask));
```

Each agent runs in its own VM, so you can parallelize as wide as your rate limits and request pools allow. See [Cloud agents](https://cursor.com/docs/cloud-agent.md) for VM behavior, lifecycle, and artifact handling.

## Per-task configuration

The SDK gives you the knobs eval harnesses usually need:

- **Custom tool sets.** Restrict or extend tools via [MCP servers](https://cursor.com/docs/sdk/typescript.md#mcp-servers) inline on `Agent.create()`.
- **Subagents.** Define named [subagents](https://cursor.com/docs/sdk/typescript.md#subagents) the main agent can spawn during a task.
- **Cancellation and timeouts.** Call `run.cancel()` to enforce wall-clock limits. Status becomes `"cancelled"` and partial output stays readable.
- **Per-step callbacks.** Use the `onStep` and `onDelta` options on `agent.send()` for finer-grained logging.

## Privacy and billing

SDK runs follow the same pricing, request pools, and Privacy Mode rules as runs from the IDE and Cloud Agents. Eval traffic is tagged so it shows up under the SDK label in your team's [usage dashboard](https://cursor.com/dashboard/usage). To keep eval data out of model training, turn on [Privacy Mode](https://cursor.com/help/security-and-privacy/privacy.md) for the account or team running the harness.

## Higher rate limits

### Running a benchmark at scale?

Default API rate limits are tuned for development workloads, not full eval sweeps. If you're benchmarking Cursor on a public leaderboard or running a large internal eval, email [leerob@cursor.com](mailto:leerob@cursor.com) and we'll get you set up with higher limits.

## Next steps

- Browse the full [Cursor SDK reference](https://cursor.com/docs/sdk/typescript.md) for every option, event type, and error class.
- Read about [Grok 4.6](https://cursor.com/docs/models/grok-4-6.md) and the rest of Cursor's [models](https://cursor.com/docs/models-and-pricing.md).
- Explore [Cloud agents](https://cursor.com/docs/cloud-agent.md) for sandboxed, parallel runs.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
