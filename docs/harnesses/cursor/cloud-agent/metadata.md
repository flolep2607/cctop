# Agent metadata

### Preview

Agent metadata is in preview and subject to change, including breaking
changes.

Cloud Agents can read key-value metadata about the current run from inside the VM: the agent id, who owns it, who submitted this turn, which model is serving, and which repos are checked out. Hooks and install scripts can read those values too.

Agents call this API with their terminal tools. You don't need to run these requests yourself.

To have an agent read metadata, include this in your prompt:

```text
To read agent metadata, follow the instructions at
https://cursor.com/docs/cloud-agent/metadata
```

This API is local to the agent VM. It is not the caller-owned `metadata` tags you set when creating an agent with the [SDK](https://cursor.com/docs/sdk/typescript.md#agent-metadata) or [Cloud Agents API](https://cursor.com/docs/cloud-agent/api/endpoints.md). Those APIs use Cursor API keys and manage agents from outside the VM.

When something outside the VM needs to verify the agent's identity, have the agent mint an [OIDC token](https://cursor.com/docs/cloud-agent/identity.md) instead. Those JWTs are signed and audience-bound. Metadata is not a credential. It can include the current turn's submitter and serving model, which a token shouldn't carry.

Cursor-managed Cloud Agent VMs serve metadata on the same socket as OIDC tokens. Self-hosted workers do not serve this API yet.

## Read a value

The agent reads keys over the Unix socket at `CURSOR_AGENT_SOCKET`. On Cursor-managed VMs the default is `/run/cursor/api.sock`.

```bash
curl --unix-socket "${CURSOR_AGENT_SOCKET:-/run/cursor/api.sock}" \
  http://cursor-agent/v1/meta-data/agent/id
```

Requests are HTTP over a Unix socket. The hostname in the URL is ignored.

List a prefix to see which keys exist:

```bash
curl --unix-socket "${CURSOR_AGENT_SOCKET:-/run/cursor/api.sock}" \
  http://cursor-agent/v1/meta-data/
```

```text
agent/
owner/
turn/
workspace/
```

Then request a key:

```bash
curl --unix-socket "${CURSOR_AGENT_SOCKET:-/run/cursor/api.sock}" \
  http://cursor-agent/v1/meta-data/owner/user-id
```

### Request

`GET /v1/meta-data[/<path>]` over the Unix socket. No body or extra headers. Trailing slashes are allowed, so a listed `agent/` can be requested as `/v1/meta-data/agent/`.

A missing key returns `404`.

### Response

Successful reads are `text/plain; charset=utf-8`. A key response is the value as text, with nothing else.

| Kind   | Body                                                                                                |
| :----- | :-------------------------------------------------------------------------------------------------- |
| Key    | The value as a string. Keys with several values put one entry per line.                             |
| Prefix | One child per line, sorted. Nested prefixes end with `/`. The listing ends with a trailing newline. |

Error responses are JSON. See [Rate limits and errors](https://cursor.com/docs/cloud-agent/metadata.md#rate-limits-and-errors).

### When keys appear

[Install scripts](https://cursor.com/docs/cloud-agent/setup.md) can read the same socket. A key is present only when it has a value: `turn/` is missing until a coding turn starts, and `workspace/branch-name` is missing until the run records a branch. Owner, team, and repository keys are available from agent creation onward.

If the socket is missing right after boot, retry the connection.

## Keys

Missing keys are omitted from listings and return `404` if requested directly. A listing only includes keys that exist right now.

### `agent/`

| Key             | When present | Description                                                                     |
| :-------------- | :----------- | :------------------------------------------------------------------------------ |
| `agent/id`      | Always       | Cloud Agent id (`bcId`).                                                        |
| `agent/name`    | When known   | Name shown in the dashboard.                                                    |
| `agent/source`  | When known   | How the agent was started, such as `WEBSITE`, `API`, `SLACK`, or `AUTOMATIONS`. |
| `agent/runtime` | Always       | `managed` on Cursor-managed Cloud Agent VMs.                                    |

### `owner/`

| Key                        | When present | Description                                                                                    |
| :------------------------- | :----------- | :--------------------------------------------------------------------------------------------- |
| `owner/user-id`            | When known   | Cursor user id of the agent owner, as a decimal string. Prefer this over email for allowlists. |
| `owner/user-email`         | When known   | Lowercased owner email. Email can change.                                                      |
| `owner/service-account-id` | When known   | Service account id when a service account owns the agent.                                      |
| `owner/team-id`            | When known   | Owning team id, as a decimal string.                                                           |

### `turn/`

`turn/` exists only while a coding turn is active. Between turns those keys are gone. If `turn/` is missing, there is no active turn.

Values under `turn/` always reflect the current turn. Don't cache them across turns.

| Key               | When present  | Description                                                                                                                                                                                             |
| :---------------- | :------------ | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `turn/id`         | During a turn | Id of this coding turn. Different from `agent/id`, which is the Cloud Agent id (`bcId`).                                                                                                                |
| `turn/user-id`    | When known    | Cursor user id of the person who submitted this turn, as a decimal string. On a [team follow-up](https://cursor.com/docs/cloud-agent/settings.md#team-follow-ups) this can differ from `owner/user-id`. |
| `turn/user-email` | When known    | Lowercased email of that person.                                                                                                                                                                        |
| `turn/started-at` | During a turn | Turn start as Unix seconds.                                                                                                                                                                             |
| `turn/model`      | When known    | Model serving this turn. If you selected Auto, this is the model that served, not `Auto`.                                                                                                               |

OIDC tokens don't include who submitted the turn or which model is serving, because a token can outlive the turn. Read those keys from metadata instead.

### `workspace/`

| Key                        | When present          | Description                                                                                                                                                                                                                |
| :------------------------- | :-------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `workspace/repo-url`       | When known            | Primary repository in `host/path` form, such as `github.com/acme/widgets`. Hostname is lowercased, with no scheme, credentials, port, query, or `.git` suffix. On a multi-repo agent, this is only the primary repository. |
| `workspace/repo-urls`      | When the set is known | Every repository in the workspace, same form as `repo-url`. Primary repository first, then the rest sorted, one URL per line. Missing means the set isn't known, not that there is only one repo.                          |
| `workspace/branch-name`    | When known            | Branch on the primary repository.                                                                                                                                                                                          |
| `workspace/environment-id` | When known            | Id of the Cursor environment this run used.                                                                                                                                                                                |
| `workspace/automation-id`  | For automations       | Automation id when `agent/source` is automations.                                                                                                                                                                          |

`workspace/repo-url` is the primary repository. For the full set, read `workspace/repo-urls`.

## Who can read metadata

Any process that can reach the socket can read every key: the agent, code it runs, and hooks. Treat these values as visible to the whole run.

Metadata is not signed. To prove identity to AWS, GCP, Vault, or your own service, have the agent mint an [OIDC token](https://cursor.com/docs/cloud-agent/identity.md) and verify the JWT. Don't forward metadata values as a credential.

## Rate limits and errors

Each agent VM can make **120** metadata requests per minute, in bursts of up to 20. The socket also accepts at most 8 connections at once. That cap is shared with OIDC minting.

Retry `429`, `503`, `500`, `502`, and `504` with backoff. Treat `403` as fatal: this agent isn't allowed to read metadata.

`404` and `405` responses include a `usage` string that restates how to call the API. Rate-limit and saturation errors stay code-only:

```json
{ "error": "not_found", "usage": "GET /v1/meta-data[/<path>] ..." }
```

```json
{ "error": "rate_limited" }
```

| HTTP      | `error`               | When                                                            |
| :-------- | :-------------------- | :-------------------------------------------------------------- |
| 404       | `not_found`           | Unknown or missing key                                          |
| 405       | `method_not_allowed`  | Not `GET`                                                       |
| 429       | `rate_limited`        | Over the per-agent request budget; honor `Retry-After`          |
| 503       | `saturated`           | Too many connections; honor `Retry-After`                       |
| 500       | `host_error`          | Internal error; retry                                           |
| 502 / 504 | `backend_unreachable` | Cursor couldn't return metadata; retry                          |
| Other     | `backend_error`       | Cursor rejected the request. `403` is fatal; `503` is retryable |

## Examples

An agent or hook can compare the turn submitter to the owner. A teammate's follow-up can take a stricter path:

```bash
SOCKET="${CURSOR_AGENT_SOCKET:-/run/cursor/api.sock}"

owner="$(curl -fsS --unix-socket "$SOCKET" \
  http://cursor-agent/v1/meta-data/owner/user-id)"
turn_user="$(curl -fsS --unix-socket "$SOCKET" \
  http://cursor-agent/v1/meta-data/turn/user-id || true)"

if [ -n "$turn_user" ] && [ "$turn_user" != "$owner" ]; then
  echo "follow-up from user $turn_user; owner is $owner"
fi
```

An agent or hook can tag logs with the agent id and the model that served the turn:

```bash
SOCKET="${CURSOR_AGENT_SOCKET:-/run/cursor/api.sock}"

agent_id="$(curl -fsS --unix-socket "$SOCKET" \
  http://cursor-agent/v1/meta-data/agent/id)"
model="$(curl -fsS --unix-socket "$SOCKET" \
  http://cursor-agent/v1/meta-data/turn/model || true)"

echo "cloud_agent_id=$agent_id model=${model:-unknown}"
```

List every repository in the workspace. `repo-urls` is one URL per line:

```bash
curl -fsS --unix-socket "${CURSOR_AGENT_SOCKET:-/run/cursor/api.sock}" \
  http://cursor-agent/v1/meta-data/workspace/repo-urls
```

```text
github.com/acme/widgets
github.com/acme/docs
```

## Related pages

- [OIDC tokens](https://cursor.com/docs/cloud-agent/identity.md) for signed JWTs and cloud federation
- [Secrets & Network](https://cursor.com/docs/cloud-agent/security-network.md) for dashboard secrets and egress controls
- [Cloud agent setup](https://cursor.com/docs/cloud-agent/setup.md) for install scripts that can read this socket
- [Hooks](https://cursor.com/docs/hooks.md) for running this API at tool and conversation boundaries
- [Service accounts](https://cursor.com/docs/account/enterprise/service-accounts.md) when agents run as a team service account


---

## Sitemap

[Overview of all docs pages](/llms.txt)
