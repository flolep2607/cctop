# OpenTelemetry Export Wire Reference

### Beta

OpenTelemetry Export is in beta, as is the companion [setup page](https://cursor.com/docs/enterprise/opentelemetry-export.md). This wire surface may change before general availability.

Companion to [OpenTelemetry Export](https://cursor.com/docs/enterprise/opentelemetry-export.md). Full wire surface: every metric, log event, attribute, enum, and presence rule.

The surface is additive. Tolerate unknown attributes, events, and enum values. Renames and removals get explicit notice.

## Transport and scope

- OTLP/HTTP binary protobuf (`application/x-protobuf`), `POST`
- Endpoints: `<base>/v1/metrics` and `<base>/v1/logs`
- Scope: `cursor.telemetry` / `0.1.0`

## Resource attributes

One resource per (team, user, surface, entrypoint, surface version) grouping.

| Attribute           | Type   | Presence | Values / notes                                                                                                           |
| ------------------- | ------ | -------- | ------------------------------------------------------------------------------------------------------------------------ |
| `service.name`      | string | Always   | Constant `cursor`                                                                                                        |
| `service.version`   | string | Optional | Client version when source is desktop/CLI; usually absent on `cloud_agent` / `bugbot`                                    |
| `cursor.team.id`    | int    | Always   | Your team id                                                                                                             |
| `cursor.surface`    | string | Always   | `unspecified` \| `desktop` \| `cli` \| `cloud_agent` \| `bugbot`                                                         |
| `cursor.entrypoint` | string | Always   | `unspecified` \| `desktop` \| `cli` \| `web` \| `mobile` \| `sdk_ts` \| `sdk_py` \| `api` \| `automation` \| `github_pr` |
| `cursor.user.id`    | int    | Optional | Opaque team-scoped user id when the source has one. Often absent on cloud agent. Do not require presence.                |

## Families

Family ids match the toggles in Team Settings. All default on for a new destination.

| Family id              | Signals        | Covers                                                                                                |
| ---------------------- | -------------- | ----------------------------------------------------------------------------------------------------- |
| `model_usage`          | metrics + logs | `token.usage`, `cost.usage`; `api.request`, `api.error`, `api.correction`                             |
| `tool_calls`           | metrics        | `tool.calls`                                                                                          |
| `skills_hooks_plugins` | logs           | `skill.activated`, `hook.execution_complete`, `plugin.installed`                                      |
| `cloud_agents`         | logs           | `cloud_agent.pull_request`, `cloud_agent.setup`, `cloud_agent.artifact`, `cloud_agent.mcp_auth_error` |

## Metrics

All metrics are monotonic **delta** sums. Metric datapoints carry no correlation IDs; those appear on logs only.

Consume metrics as sums of deltas per series. A series is the resource, the metric name, and the exact datapoint attribute set. Windows for the same series can overlap across flushes.

### `cursor.token.usage`

Unit `{token}`. Family `model_usage`.

| Attribute             | Type   | Presence | Values / notes                                                                                                                                                                                                     |
| --------------------- | ------ | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `cursor.token.type`   | string | Always   | `input` \| `output` \| `cache_read` \| `cache_creation`                                                                                                                                                            |
| `cursor.model.name`   | string | Optional | Requested public model after routed-intent collapse (`auto:` to `Auto`, `thinking:` to `Thinking`, `pro:` to `Pro`, `premium:` to `Premium`; else pass-through). Absent on bugbot or when the source had no model. |
| `cursor.api.status`   | string | Optional | `success` \| `errored` \| `aborted`                                                                                                                                                                                |
| `cursor.api.billable` | bool   | Optional |                                                                                                                                                                                                                    |

### `cursor.tool.calls`

Unit `{call}`. Family `tool_calls`. Value `1` per completed tool call.

| Attribute                | Type   | Presence | Values / notes                                                     |
| ------------------------ | ------ | -------- | ------------------------------------------------------------------ |
| `cursor.tool.kind`       | string | Always   | `builtin` \| `mcp`                                                 |
| `cursor.tool.name`       | string | Always   | Builtin id (e.g. `read`, `shell`) or customer MCP tool name (open) |
| `cursor.tool.status`     | string | Always   | `success` \| `failure` \| `aborted` (MCP never reports `aborted`)  |
| `cursor.mcp.server.name` | string | MCP only | Customer-defined server display name (open)                        |

### `cursor.cost.usage`

Unit `USD` (double). Family `model_usage`. Best-effort estimated cost at event time, **not an invoice**. Subject to `cursor.api.correction`. For BYOK, this is the Cursor Token Rate only, not provider spend.

| Attribute           | Type   | Presence | Values / notes                       |
| ------------------- | ------ | -------- | ------------------------------------ |
| `cursor.model.name` | string | Optional | Same collapse rules as `token.usage` |

## Log events

Severities: INFO=9, WARN=13, ERROR=17.

### Common log attributes

| Attribute                | Type   | Presence | Notes                                                                                                                                                                           |
| ------------------------ | ------ | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cursor.event.id`        | string | Always   | **Dedupe key.** Opaque. Deterministic across retries, worker restarts, and Cursor Kafka replay. Prefix `customer-telemetry:v1:...` is stable; treat the whole string as opaque. |
| `cursor.source_event.id` | string | Always   | Opaque internal source identity. Several signals may share one value.                                                                                                           |
| `cursor.request.id`      | string | Optional | On `api.request`, `api.error`, `skill.activated`, `hook.execution_complete`, `plugin.installed`. Never on `api.correction` or `cloud_agent.*`.                                  |
| `cursor.conversation.id` | string | Optional | IDE/CLI: composer UUID. Cloud agent: customer-visible `bc-...` agent id. Join key for session reconstruction across api, skill/hook, and cloud\_agent logs.                     |
| `cursor.usage_event.id`  | string | Optional | `api.request` / `api.error` / `api.correction` only. Request-grain key against Cursor usage and billing exports.                                                                |

### `cursor.api.request`

INFO, body `api_request`. Family `model_usage`.

| Attribute                                  | Type   | Presence | Notes |
| ------------------------------------------ | ------ | -------- | ----- |
| `cursor.api.request.input_tokens`          | int    | Always   |       |
| `cursor.api.request.output_tokens`         | int    | Always   |       |
| `cursor.api.request.cache_read_tokens`     | int    | Always   |       |
| `cursor.api.request.cache_creation_tokens` | int    | Always   |       |
| `cursor.model.name`                        | string | Optional |       |
| `cursor.api.billable`                      | bool   | Optional |       |

### `cursor.api.error`

ERROR, body `api_error`. Family `model_usage`. No raw error messages. Low-cardinality kind and status attributes are planned; don't depend on them yet.

| Attribute             | Type   | Presence | Notes |
| --------------------- | ------ | -------- | ----- |
| `cursor.model.name`   | string | Optional |       |
| `cursor.api.billable` | bool   | Optional |       |

### `cursor.api.correction`

WARN, body `api_correction_<kind>`. Family `model_usage`. Billing finalization: the usage event was retroactively **not** billed. Join on `cursor.usage_event.id` and drop the whole group for billing. Deliberately carries no `cursor.model.name`.

| Attribute                    | Type   | Presence | Values                                                      |
| ---------------------------- | ------ | -------- | ----------------------------------------------------------- |
| `cursor.api.correction.kind` | string | Always   | `not_billed_errored` \| `not_billed_aborted_before_timeout` |

### `cursor.skill.activated`

INFO, body `skill_activated`. Family `skills_hooks_plugins`.

| Attribute              | Type   | Presence | Values / notes                                                              |
| ---------------------- | ------ | -------- | --------------------------------------------------------------------------- |
| `cursor.skill.name`    | string | Always   | Customer-authored (open)                                                    |
| `cursor.skill.trigger` | string | Always   | `agent_read` \| `manually_attached` \| `skill_name_in_prompt`               |
| `cursor.skill.source`  | string | Always   | `unspecified` \| `workspace` \| `user` \| `builtin` \| `plugin` \| `claude` |
| `cursor.plugin.name`   | string | Optional | When the skill came from a plugin                                           |

### `cursor.hook.execution_complete`

INFO (ERROR for `failed` / `timeout`), body `hook_execution_complete`. Family `skills_hooks_plugins`.

| Attribute                 | Type   | Presence | Values / notes                                                                                                                                                                             |
| ------------------------- | ------ | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `cursor.hook.name`        | string | Always   | Customer-configured (open)                                                                                                                                                                 |
| `cursor.hook.type`        | string | Always   | `pre_tool_use` \| `post_tool_use` \| `post_tool_use_failure` \| `before_submit_prompt` \| `after_agent_response` \| `after_agent_thought` \| `stop` \| `subagent_start` \| `subagent_stop` |
| `cursor.hook.outcome`     | string | Always   | `success` \| `blocked` \| `failed` \| `timeout`                                                                                                                                            |
| `cursor.hook.duration_ms` | int    | Always   |                                                                                                                                                                                            |
| `cursor.plugin.name`      | string | Optional | When the hook came from a plugin                                                                                                                                                           |

### `cursor.plugin.installed`

INFO, body `plugin_installed`. Family `skills_hooks_plugins`. No `conversation.id` (install is not conversation-scoped).

| Attribute             | Type   | Presence | Values / notes                                     |
| --------------------- | ------ | -------- | -------------------------------------------------- |
| `cursor.plugin.name`  | string | Always   | Open                                               |
| `cursor.plugin.scope` | string | Always   | `unspecified` \| `public` \| `private_marketplace` |

### `cursor.cloud_agent.pull_request`

INFO (`opened`) / WARN (`creation_failed`), body `cloud_agent_pull_request_<kind>`. Family `cloud_agents`. `conversation.id` = `bc-...`.

| Attribute                                | Type   | Presence      | Values / notes                |
| ---------------------------------------- | ------ | ------------- | ----------------------------- |
| `cursor.cloud_agent.pull_request.kind`   | string | Always        | `opened` \| `creation_failed` |
| `cursor.cloud_agent.pull_request.number` | int    | `opened` only |                               |
| `cursor.cloud_agent.pull_request.draft`  | bool   | `opened` only |                               |

`creation_failed` is live. `opened` may be sparse while the producer rolls out.

### `cursor.cloud_agent.setup`

INFO (`started` / `completed`) / ERROR (`failed`), body `cloud_agent_setup_<kind>`. Family `cloud_agents`. `conversation.id` = `bc-...`.

| Attribute                              | Type   | Presence                    | Values / notes                                  |
| -------------------------------------- | ------ | --------------------------- | ----------------------------------------------- |
| `cursor.cloud_agent.setup.kind`        | string | Always                      | `started` \| `completed` \| `failed`            |
| `cursor.cloud_agent.setup.duration_ms` | int    | Terminal kinds when present | `completed` / `failed`                          |
| `cursor.cloud_agent.setup.reason`      | string | `failed` only               | Open vocabulary (e.g. `install_command_failed`) |

### `cursor.cloud_agent.artifact`

INFO, body `cloud_agent_artifact_created`. Family `cloud_agents`. `conversation.id` = `bc-...`.

| Attribute                                  | Type   | Presence | Values / notes |
| ------------------------------------------ | ------ | -------- | -------------- |
| `cursor.cloud_agent.artifact.file_name`    | string | Always   | Open           |
| `cursor.cloud_agent.artifact.content_type` | string | Optional | MIME           |

### `cursor.cloud_agent.mcp_auth_error`

ERROR, body `cloud_agent_mcp_auth_error`. Family `cloud_agents`. `conversation.id` = `bc-...`.

An MCP server you connected rejected the run's credentials. That server's tool calls failed while the run continued. ERROR because only you can fix the integration; alert on this to catch Automations and Cloud Agents silently losing an MCP server.

| Attribute                | Type   | Presence | Values / notes                                                                                                               |
| ------------------------ | ------ | -------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `cursor.mcp.server.name` | string | Always   | Customer-defined server display name (open), e.g. `github`. Same value space as the `cursor.tool.calls` datapoint attribute. |

## Identity and joins

- **Dedupe logs** on `cursor.event.id`.
- **Session reconstruction:** group logs by `cursor.conversation.id` (composer UUID or `bc-...`).
- **Billing reconcile grain:** `cursor.usage_event.id` across `api.request` / `api.error` / `api.correction`.
- Metrics do not carry these ids. Use `api.request` logs for per-conversation token totals.

See [Joining sessions](https://cursor.com/docs/enterprise/opentelemetry-export.md#joining-sessions) on the setup page for recipes.

## Delivery semantics

- **Logs** are at-least-once. Transient failures recover automatically for about **7 days**; dedupe on `event.id`. Terminal rejections (persistent 4xx, bad payloads) are **not** replayed.
- **Metrics** are at-most-once. Failed metric requests are not retried or replayed.
- **No ordering guarantee.** Corrections can arrive after the requests they amend; order by record timestamp.
- **OTLP partial success** is honored. Rejected items are not re-sent.
- No backfill from before destination activation. Source retention upstream of export is also about **7 days** (separate from the delivery retry window).


---

## Sitemap

[Overview of all docs pages](/llms.txt)
