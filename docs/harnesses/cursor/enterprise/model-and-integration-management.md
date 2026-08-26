# Model and Integration Management

Your team can access multiple AI models and integrate Cursor with various services. This documentation covers how to control which models are available, manage MCP server trust, and set up integrations with tools like Slack, GitHub, and Linear.

## Model access control

Enterprise teams can control which AI models team members can use, [contact sales](https://cursor.com/contact-sales?source=docs-model-controls) to get access. This helps manage costs, ensure appropriate usage, and comply with organizational policies.

Configure model access in two places:

1. **Team Settings → Models** in the [team dashboard](https://cursor.com/docs/account/teams/dashboard.md) (Enterprise only). From Team Settings, open the **Model Providers** section to manage providers, models, defaults, and personal API key (BYOK) controls. This is the team baseline.
2. **Organization → Groups → \[group] → Models**, when you use [Organization Groups](https://cursor.com/docs/enterprise/organization-groups.md#model-access). Use this to widen access for specific cohorts.

You can also manage the team baseline programmatically with the [Admin API model access](https://cursor.com/docs/account/teams/admin-api.md#model-access) routes, or across linked teams with the [Organization API](https://cursor.com/docs/account/organizations/organization-admin-api.md#model-access).

### How team and group model access combine

Cursor reconciles team and Organization Group model settings with a **most-permissive (union)** model. Neither layer fully overrides the other:

- A model is allowed if the **team** or **any of the user's Organization Groups** allows it.
- A group setting cannot make a model more restrictive than what another allowing source already grants. Groups are for widening access, not tightening it below an allow from the team or another group.
- Put your strictest defaults on the **team**. Use Organization Groups only to grant additional models to selected cohorts.

Personal API key (BYOK) controls remain on **Team Settings → Models** only. Organization Group Models settings do not configure BYOK.

See [Organization Groups](https://cursor.com/docs/enterprise/organization-groups.md#how-group-and-team-settings-combine) and [How limits and permissions combine](https://cursor.com/docs/enterprise/organizations.md#how-limits-and-permissions-combine) for the full merge rules across settings.

### How enterprise model rollout works

When new models become available, Cursor doesn't immediately enable them for all enterprise teams.

Instead, Enterprise teams can opt in to new models for their organization.

See [Models](https://cursor.com/docs/models-and-pricing.md) for the current list of available models.

### Auto-review and model access

[Auto-review](https://cursor.com/docs/agent/security/run-modes.md#run-mode) uses a background classifier that runs on [Claude 4.5 Haiku](https://cursor.com/docs/models/claude-4-5-haiku.md) or [GPT-5.4 Mini](https://cursor.com/docs/models/gpt-5-4-mini.md). Blocking all of them disables Auto-review in the IDE, even when team Run Modes includes it. See [Auto-review classifier requirements](https://cursor.com/docs/agent/security/run-modes.md#auto-review-model-requirements).

## Restrict personal API keys (BYOK controls)

Enterprise teams can prevent team members from using their own API keys with third-party providers (OpenAI, Anthropic, Azure, AWS Bedrock) in Cursor. All usage goes through Cursor's included models and usage pool.

Configure this in the [team dashboard](https://cursor.com/docs/account/teams/dashboard.md) under **Team Settings → Models** (Enterprise only).

## MCP server trust management

The Model Context Protocol (MCP) lets you connect external tools and data sources to Cursor. MCP servers can:

- Read files from external systems
- Execute operations on your behalf
- Access databases and APIs
- Integrate with third-party services

MCP servers are designed and implemented by external vendors, not Cursor. We work with partners to provide a [vetted marketplace](/marketplace) of trusted servers, but you should review each server's capabilities and permissions before enabling it for your team.

Because MCP servers have significant capabilities, you need to manage which servers your team can use.

### MCP Allowlist

Enterprise teams can control which MCP servers team members are allowed to use. Configure this in the [team dashboard](https://cursor.com/docs/account/teams/dashboard.md) under "MCP Configuration" (Enterprise only).

Add each approved server as a command or URL entry, then configure its tool controls and network policy. Approving a trusted set of servers and domains is usually enough; apply stricter tool and network controls per server when you need them.

You can also distribute `~/.cursor/permissions.json` through MDM to set the per-user MCP auto-run allowlist from a managed file.

In that file, `mcpAllowlist` must be a JSON array of strings using `server:tool` syntax:

| Entry         | Meaning                                      |
| :------------ | :------------------------------------------- |
| `server:tool` | One specific tool on one specific MCP server |
| `server:*`    | All tools from one MCP server                |
| `*:tool`      | One tool name from any MCP server            |
| `*:*`         | All MCP tools                                |

Cursor resolves the effective MCP allowlist in this order:

1. Team dashboard or other admin-controlled settings
2. `~/.cursor/permissions.json`
3. The MCP allowlist in editor settings and inline **Add to allowlist**

Higher-priority sources replace lower-priority ones. They do not merge.

When an allowlist is active, only servers matching an allowlist entry can run. Servers that don't match are blocked.

Adding a server to the allowlist does not push it to users' machines. Team members still need to configure the server in their own [Cursor settings](https://cursor.com/docs/mcp.md).

To distribute an approved server, add it to a [team marketplace](https://cursor.com/docs/plugins.md#team-marketplaces). Admins can link existing standalone Team MCP servers to the Default marketplace so teammates can install and configure them in the Agent Window, IDE, and CLI.

All allowlist entries support wildcards using `*` to match any sequence of characters.

#### Command-based servers (stdio)

For local MCP servers configured with `command` and `args`, the allowlist matches against the **full command string**: the `command` value and all `args` values joined with spaces.

Given this `mcp.json` config:

```json
{
  "mcpServers": {
    "my-tool": {
      "command": "npx",
      "args": ["-y", "@acme/mcp-tool@latest"]
    }
  }
}
```

The full command string is `npx -y @acme/mcp-tool@latest`. On most systems, the shell resolves `npx` to a full path like `/usr/local/bin/npx` or `/opt/homebrew/bin/npx`, so the actual string becomes `/usr/local/bin/npx -y @acme/mcp-tool@latest`.

Use a leading `*` wildcard to match regardless of the install path:

| Allowlist entry                               | Matches                                                           |
| :-------------------------------------------- | :---------------------------------------------------------------- |
| `*npx -y @acme/mcp-tool@latest`               | `npx` at any path, with these exact arguments                     |
| `/usr/local/bin/npx -y @acme/mcp-tool@latest` | Only this exact path                                              |
| `*npx -y @acme/*`                             | Any `@acme`-scoped MCP package                                    |
| `*python */scripts/mcp-server.py*`            | A Python server at any matching path, with any trailing arguments |

#### URL-based servers (HTTP/SSE)

For remote MCP servers configured with `url`, the allowlist matches against the URL.

Given this `mcp.json` config:

```json
{
  "mcpServers": {
    "acme-tools": {
      "url": "https://mcp.acme.com/sse"
    }
  }
}
```

The allowlist entry matches against the full URL `https://mcp.acme.com/sse`:

| Allowlist entry            | Matches                                 |
| :------------------------- | :-------------------------------------- |
| `https://mcp.acme.com/sse` | This exact URL                          |
| `https://*.acme.com/*`     | Any subdomain and path under `acme.com` |
| `https://mcp.acme.com/*`   | Any path on this host                   |

### Per-server tool controls

Tool controls live in the MCP Configuration section and are set per server, not in a separate auto-run list. For each approved server, restrict which tools can run by listing them in that server's Tools field. Leave the field empty to allow all tools from that server.

### Per-server network controls

Each approved server has its own network policy, so you control what it can reach.

Remote (URL) MCP servers are restricted to the configured URL entry pattern.

Local command-based (`stdio`) servers run in a sandbox with one of these network modes:

| Network mode   | Behavior                                                |
| :------------- | :------------------------------------------------------ |
| **Allow all**  | No egress restrictions.                                 |
| **Allowlist**  | Only listed destinations are reachable.                 |
| **Deny all**   | Run the server locally with no outbound network access. |
| **No sandbox** | Run without command or network sandboxing.              |

## Git repository blocklist

You can prevent Cursor from accessing specific repositories.

Add repository URLs or patterns in the [team dashboard](https://cursor.com/docs/account/teams/dashboard.md) under "Repository Blocklist" (Enterprise only). Cursor will refuse to index or work with blocked repositories.

## Protected Git Scopes

Lock a Git organization, group, or namespace to your Cursor organization so only your teams can use its repositories with [Cloud Agents](https://cursor.com/docs/cloud-agent.md), [automations](https://cursor.com/docs/cloud-agent/automations.md), and [Bugbot](https://cursor.com/docs/bugbot.md). Cursor always verifies that a user can access a repository's connected source before it runs an agent or Bugbot check. Protected Git Scopes adds an organization-level guarantee on top of that per-user check, so enterprises can be confident their code can't be reached through unsanctioned ("shadow IT") Cursor accounts or outside teams, even ones that already have legitimate Git access.

Protect or remove a scope from the [Integrations & MCP](https://cursor.com/dashboard/integrations) tab of your dashboard (Teams and Enterprise). Claiming a scope requires a Cursor team admin who is also a Git provider admin. Works with cloud and self-hosted GitHub and GitLab.

## Integration: Slack

The Slack integration enables Cloud Agents to run directly from Slack. Team members can mention `@cursor` with a prompt and get automated code changes delivered as pull requests.

Cursor requires permissions to read messages, post responses, and access channel metadata. See the [Slack integration documentation](https://cursor.com/docs/integrations/slack.md#permissions) for the full list.

See [Slack integration](https://cursor.com/docs/integrations/slack.md) for detailed setup and usage instructions.

## Integration: GitHub, GHES, and GitLab

Connect Cursor to your version control system to work with Cloud Agents.

Cursor requires read access to repositories and write access to create PRs. You control which repositories the Cursor app can access.

See [GitHub integration](https://cursor.com/docs/integrations/github.md) for setup.

## Integration: Linear

Connect Linear to start Cloud Agents from issues.

Cursor requires read access to issues and write access to update issue status.

See [Linear integration](https://cursor.com/docs/integrations/linear.md) for details.

### Model controls are available on the Enterprise plan

Contact our team to learn about model restrictions and MCP management.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
