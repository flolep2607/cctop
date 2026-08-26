# Cursor Router

Cursor Router is the model routing system behind **Auto**. Not every request needs frontier-level intelligence, so the router sends each request to the model that fits the task: simple requests go to fast, efficient models while complex work goes to the most capable ones.

Cursor Router is currently only available on Teams and Enterprise plans.

## How it works

When you select **Auto** and choose **Balance** or **Intelligence** in the model picker, Cursor Router runs a classifier on each agent request and routes it based on task type and complexity. The router picks the most cost-effective model that still produces comparable quality for that request.

Cursor Router is data-driven and managed by Cursor. You can't hand-pick which model handles a request, and the model pool changes over time as new models ship. You steer routing by choosing an optimization mode.

On Enterprise plans, Cursor Router respects your team's model access controls. If a model is blocked for your team, the router routes to an allowed model instead. Blocking too many models reduces routing quality and can disable the router. To create cost savings, the router needs a powerful yet cost-efficient model to use when it isn't calling other frontier models, so enabling [Cursor Grok 4.5](https://cursor.com/docs/models/grok-4-5.md) is a requirement for the router to work.

## Optimization modes

Open the model picker, select **Auto**, and pick a mode under **Optimize For**:

- **Cost**: Uses the previous Auto routing logic. It optimizes token spend.
- **Balance**: Optimizes for intelligence, speed, and cost.
- **Intelligence**: Routes to the most capable models for harder tasks, at a lower cost than running a single frontier model.

Balance and Intelligence use your usage limits faster than Cost. You can switch modes at any time.

## Pricing

All Auto modes bill at the list price of the model each request is routed to. Third-party models also incur the Cursor Token Rate.

Until September 7, 2026, Enterprise Auto Cost pricing is set per million tokens, regardless of which model is used ($1.25/1M input and cache write, $0.25/1M cache read, $6.00/1M output).

## Team settings

Admins configure Cursor Router from the [team dashboard](https://cursor.com/docs/account/teams/dashboard.md).

- **Enable Cursor Router**: Turn routing on or off. When enabled, team members using Auto are routed by Cursor Router. Enterprise teams must enable the router manually as it's off by default. On Enterprise plans, the router can also be configured per [organization group](https://cursor.com/docs/enterprise/organization-groups.md).
- **Routing preferences**: Choose which optimization modes team members can select from Auto. You can disable up to 2 modes.
- **Underlying model**: Display which model Auto routed to at the start of each response, or keep it hidden. Hidden is the default and recommended, so results are judged on their own merit rather than by model name. Applies to Balance and Intelligence modes.
- **Impose Auto**: Make Auto the default model for everyone on the team. **Soft** defaults each new chat to Auto; members can still switch models. **Hard** locks the model picker to Auto. Both are off by default.

## Use Router through the SDK

The [TypeScript SDK](https://cursor.com/docs/sdk/typescript.md#cursor-router) and [Python SDK](https://cursor.com/docs/sdk/python.md#cursor-router) expose Cursor Router as model id `auto-smart` with parameter `optimize_for` (`cost`, `balanced`, or `intelligence`). Call `Cursor.models.list()` to confirm Router is available for the API key's team before you hard-code a selection.

### TypeScript

```typescript
import { Agent } from "@cursor/sdk";

await using agent = await Agent.create({
  apiKey: process.env.CURSOR_API_KEY!,
  model: {
    id: "auto-smart",
    params: [{ id: "optimize_for", value: "balanced" }],
  },
  local: { cwd: process.cwd() },
});
```

### Python

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
    ...
```

The SDK runs Cursor agent workflows. It is not a standalone chat-completions or raw inference API. See [Cursor Router in the TypeScript SDK](https://cursor.com/docs/sdk/typescript.md#cursor-router) or [Python SDK](https://cursor.com/docs/sdk/python.md#cursor-router) for catalog discovery, per-run mode overrides, and troubleshooting.

## Related

- [Cursor Router help](https://cursor.com/help/models-and-usage/cursor-router.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
