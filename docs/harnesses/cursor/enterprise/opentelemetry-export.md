# OpenTelemetry Export

### Beta

OpenTelemetry Export is in beta. Features and the wire surface may change before general availability.

OpenTelemetry Export streams Cursor usage data for your team to a collector you run. Cursor sends metrics (tokens, tool calls, best-effort cost) and logs (API requests, errors, corrections, skills, hooks, plugins, and cloud agent lifecycle events) to one team-managed destination. Export runs server-side.

The beta is available on the [Enterprise plan](https://cursor.com/contact-sales?source=docs-opentelemetry-export). Admins configure it in **Team Settings > OpenTelemetry Export**.

The [Wire Reference](https://cursor.com/docs/enterprise/opentelemetry-export/wire.md) documents every metric, log event, and attribute.

## Prerequisites

- An HTTPS endpoint that accepts **OTLP/HTTP protobuf** on `/v1/metrics` and `/v1/logs`. Datadog Agent OTLP ingest, the OpenTelemetry Collector, and ClickHouse/ClickStack all work.
- A bearer token or API key Cursor can send as a request header.
- The endpoint must be reachable from the public internet. Cursor egresses from a fixed set of source IPs.

## Source IPs

Cursor delivers OTLP through a server-side egress proxy. Traffic originates from these static addresses (all `/32`):

| IP address     | CIDR |
| -------------- | ---- |
| 3.218.161.44   | /32  |
| 3.231.18.206   | /32  |
| 35.174.159.35  | /32  |
| 184.73.225.134 | /32  |
| 3.209.66.12    | /32  |
| 52.44.113.131  | /32  |

These IPs don't rotate without advance notice. Use TLS and auth as the primary control. Add IP allowlisting if your network requires it.

## Collector recipes

Cursor pushes to your collector over **OTLP/HTTP binary protobuf**. gRPC and JSON are not supported. Enter the HTTPS base URL in Team Settings without a `/v1` suffix; Cursor appends `/v1/metrics` and `/v1/logs`.

### Minimal OpenTelemetry Collector

```yaml
receivers:
  otlp:
    protocols:
      http:
        endpoint: 0.0.0.0:4318

processors:
  batch:

exporters:
  # Swap for your sink (datadog, clickhouse, logging, etc.)
  logging:
    verbosity: basic

service:
  pipelines:
    metrics:
      receivers: [otlp]
      processors: [batch]
      exporters: [logging]
    logs:
      receivers: [otlp]
      processors: [batch]
      exporters: [logging]
```

Terminate TLS in front of the collector with a load balancer, ingress, or the otelcol TLS settings. Enter `https://otel.example.com` in Cursor, not `https://otel.example.com:4318/v1`. For auth, terminate at the load balancer or configure a static header for Cursor to send, such as `Authorization: Bearer <token>`.

### Datadog Agent (OTLP ingest)

Enable OTLP HTTP ingest and logs in the Agent, then expose the Agent (or a gateway in front of it) over HTTPS:

```yaml
logs_enabled: true

otlp_config:
  receiver:
    protocols:
      http:
        endpoint: 0.0.0.0:4318
  logs:
    enabled: true
```

The env-var equivalents are `DD_OTLP_CONFIG_RECEIVER_PROTOCOLS_HTTP_ENDPOINT=0.0.0.0:4318`, `DD_LOGS_ENABLED=true`, and `DD_OTLP_CONFIG_LOGS_ENABLED=true`. Expose port `4318`, or terminate TLS on 443 and proxy to 4318.

In Cursor, the base URL is the public `https://` endpoint in front of that listener. Add `DD-API-KEY` or site headers only if your gateway expects them; the Agent already has `api_key` configured locally.

See [OTLP ingest in the Datadog Agent](https://docs.datadoghq.com/opentelemetry/setup/otlp_ingest_in_the_agent/) for Agent configuration details.

### Databricks and warehouse-style sinks

For warehouse destinations like Databricks or ClickHouse, run a collector with an OTLP HTTP receiver and the vendor exporter, or forward over HTTP into your ingest pipeline. The Cursor side is the same: an HTTPS base URL serving protobuf on `/v1/metrics` and `/v1/logs`. Consume metrics as sums of deltas and dedupe logs on `cursor.event.id`.

## Enable

In **Team Settings > OpenTelemetry Export**:

1. **Create destination** with the base URL (no `/v1/...`; Cursor appends the paths) and auth headers
2. **Test connection** to check the URL and auth
3. **Enable**. Export starts within about a minute.

Each signal and telemetry family has its own toggle. New families default on unless you turn off `auto_enable_new_families`.

## What Cursor exports

Scope: `cursor.telemetry` `0.1.0`.

Everything below is on by default for a new destination. Turn individual families off in Team Settings.

**Metrics** (delta temporality)

- `cursor.token.usage`: by `cursor.token.type` (`input` / `output` / `cache_read` / `cache_creation`)
- `cursor.tool.calls`: builtin and MCP (`cursor.tool.kind`)
- `cursor.cost.usage`: best-effort USD estimate, not an invoice

**Logs**

- `cursor.api.request`: model call summary
- `cursor.api.error`: error event (no raw messages)
- `cursor.api.correction`: billing finalization; join on `cursor.usage_event.id`
- `cursor.skill.activated`
- `cursor.hook.execution_complete`
- `cursor.plugin.installed`
- `cursor.cloud_agent.setup`: `started` / `completed` / `failed`
- `cursor.cloud_agent.artifact`
- `cursor.cloud_agent.pull_request`: `opened` / `creation_failed`
- `cursor.cloud_agent.mcp_auth_error`: an MCP server rejected the run's credentials

**Families** (admin toggles; all default on)

- `model_usage`: token and cost metrics; api.request / api.error / api.correction
- `tool_calls`: tool.calls metric
- `skills_hooks_plugins`: skill / hook / plugin logs
- `cloud_agents`: cloud\_agent.\* logs

**Useful attributes**

- Resource: `service.name=cursor`, `cursor.team.id`, optional `cursor.user.id`, surface/entrypoint
- Logs: `cursor.event.id` (dedupe), and `cursor.request.id` / `cursor.conversation.id` / `cursor.usage_event.id` when present

## Delivery

- Metrics are **at-most-once**. Delta sums can have brief gaps after a failure.
- Logs are **at-least-once**. Dedupe on `cursor.event.id` for exactly-once views.
- There is no backfill from before the destination existed.
- Editing the endpoint or credentials keeps the destination. Disabling or deleting it drops in-flight data.

## Auth

Cursor stores headers encrypted. To rotate credentials, edit the destination and save. Changes take effect in about 30 seconds.

## Limitations

- **Cost is not billing.** `cursor.cost.usage` is a best-effort estimate. One series covers both included-quota drawdown and on-demand usage. For BYOK it reflects the **Cursor Token Rate** only, not provider spend. Use the Admin and billing APIs for invoices.
- **Disabling or deleting a destination drops in-flight data.** Rotate credentials by editing the destination instead of deleting and re-adding it.
- **Logs can arrive more than once.** Delivery is at-least-once. Dedupe on `cursor.event.id`.
- **No prompt content, no traces, and no historical backfill.** Export starts when you enable the destination.
- **Metric datapoints carry no correlation IDs.** Use log attributes for per-conversation joins. See [Joining sessions](https://cursor.com/docs/enterprise/opentelemetry-export.md#joining-sessions).
- **Metrics are delta-only.** Sum deltas per series. A strict delta-to-cumulative processor may drop end-time-inverted points.

## Joining sessions

Metrics (`cursor.token.usage`, `cursor.tool.calls`, `cursor.cost.usage`) are aggregates. Datapoints carry no `conversation.id`, `request.id`, or `usage_event.id`. This keeps metric cardinality bounded. For session- or request-scoped analysis, use logs.

**What each id means**

- `cursor.conversation.id` is the session key. In the IDE and CLI it's the composer chat UUID. For cloud agents it's the customer-visible `bc-...` agent id. The same value appears on that run's `api.request`, `api.error`, `skill.activated`, `hook.execution_complete`, and `cloud_agent.*` logs when present.
- `cursor.usage_event.id` is the request-grain key on `api.request`, `api.error`, and `api.correction`. Use it to reconcile against Cursor usage and billing exports and to apply corrections.
- `cursor.request.id` is an optional per-call id on most logs. It never appears on `api.correction` or `cloud_agent.*`.
- `cursor.event.id` is a dedupe key only, not a join key across event types.

**Recipe: rank sessions by tokens, then attach skills and tools**

1. Take `cursor.api.request` log rows. Sum `cursor.api.request.input_tokens` and `output_tokens` (and the cache fields if you need them) grouped by `cursor.conversation.id`. This gives per-session token totals, which metrics can't provide.
2. Rank conversations by that sum, or by estimated cost.
3. Left-join other logs on the same `cursor.conversation.id`:
   - `cursor.skill.activated` shows which skills ran
   - `cursor.hook.execution_complete` shows hooks
   - `cursor.cloud_agent.*` shows setup, pull requests, artifacts, and MCP auth failures (cloud agents only)
4. `cursor.tool.calls` is metric-only, so it has no conversation id. Report org-wide tool rates from the metric. Per-session tool attribution is not on the wire yet.

`cursor.cost.usage` is also metric-only. To rank sessions by cost, approximate from `api.request` token totals and your own rates, or pull spend from the Admin and billing APIs and join on `cursor.usage_event.id` where available.

**Recipe: apply a billing correction**

1. Find `cursor.api.correction` logs.
2. Join on `cursor.usage_event.id` to the `api.request` and `api.error` logs sharing that id.
3. Treat the whole group as not billed.

**Caveats**

- Subagents get their own conversation id. Parent rollup is not exported yet.
- Dedupe log rows on `cursor.event.id` before joining if you need exactly-once views.

## Change policy

New metrics and events may appear as coverage expands. `auto_enable_new_families` controls whether they turn on automatically. Renames and removals get explicit notice. The [Wire Reference](https://cursor.com/docs/enterprise/opentelemetry-export/wire.md) documents the full attribute surface.

### The OpenTelemetry Export beta is available on the Enterprise plan

Contact our team to stream Cursor usage into your observability stack.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
