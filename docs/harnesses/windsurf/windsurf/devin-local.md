# Devin Local Agent

Use the same agent harness as Devin CLI directly inside Windsurf.

Devin Local is our next-generation agent harness shared with [Devin CLI](https://cli.devin.ai).

It operates on your machine with access to your local files, tools, and environment and is meant to eventually replace Cascade as the primary local agent.

<Note>
  Devin Local is currently in preview and has some [limitations](#limitations) compared to Cascade. Devin Local is not supported in the Jetbrains plugin for Windsurf.
</Note>

## Key improvements

In the time since Cascade first launched, model capabilities have evolved significantly. Devin Local is built from the ground up to efficiently leverage these advancements.

### Token efficiency

The Devin Local agent is significantly more token-efficient, with a greater focus on prompt caching. Most tasks take up to 30% fewer tokens than Cascade to accomplish the same result.

### Subagents

The Devin Local agent can spawn independent [subagents](https://cli.devin.ai/docs/subagents) to handle subtasks — either in the foreground or background. Subagents share tools and codebase context with the parent agent but operate in their own conversation chain.

### Sandboxing

The Devin Local agent supports OS-level sandboxing. When enabled, the sandbox enforces:

* **Filesystem isolation** — writable and readable paths are derived from your permission scopes
* **Network filtering** — domain allowlists and denylists control what the agent can reach

Enterprise admins can enforce sandbox behavior across the organization through [team settings](https://cli.devin.ai/docs/enterprise/team-settings#sandbox-enforcement), including requiring sandbox mode for all users and configuring organization-wide domain filtering rules.

### Quick Review

[Quick Review](/windsurf/quick-review) is a dedicated subagent available with the Devin Local agent to get rapid feedback on changes.

## Switching your agent

In most cases, you can switch your agent to `Devin Local` when starting *new* conversations via the agent selector in the bottom right corner of Windsurf.

<Frame>
  <img />
</Frame>

### Agent settings

If Devin Local doesn't appear in the agent selector, you might need to enable it from `Windsurf Settings`:

1. Open the Command Palette with `Cmd+Shift+P` (macOS) or `Ctrl+Shift+P` (Windows/Linux)
2. Open `Windsurf User Settings`
3. Click the "Agents" tab
4. Toggle the "Devin Local" agent on
5. Restart Windsurf

<Frame>
  <img />
</Frame>

You can also choose to disable Cascade entirely with the `windsurf.cascade.enabled` setting.

## Differences

### Permissions model

Devin Local replaces [auto-execution levels](/windsurf/terminal#auto-execution-levels) with a more fine-grained permissions system to control which actions the agent can take:

* **Deny** rules block actions entirely (highest priority)
* **Ask** rules always prompt for approval
* **Allow** rules auto-approve actions without prompting

Permissions can be scoped to file reads, file writes, command execution, HTTP fetches, and MCP tools. They can be configured at the project, user, or organization level.

### MCP server configuration

With the Devin Local agent, MCP servers are configured via [config files](https://cli.devin.ai/docs/extensibility/mcp/configuration) on your local machine.

The file location is determined by the scope:

| Scope          | Location                      | Shared with team?                  |
| -------------- | ----------------------------- | ---------------------------------- |
| Project        | `.devin/config.json`          | Yes (checked into version control) |
| Local override | `.devin/config.local.json`    | No (gitignored)                    |
| User           | `~/.config/devin/config.json` | No                                 |

## Limitations

The following features are not currently supported with the Devin Local agent:

* **Memories** — The Devin Local agent does not persist memories between sessions. Migrate your critical memories to [skills](/windsurf/cascade/skills).
* **Workflows** — Workflows are not available with the Devin Local agent. Migrate your workflows to [skills](/windsurf/cascade/skills).
* **Codemaps** — The Devin Local agent does not yet read [codemaps](/windsurf/codemaps).
* **Code Lenses** - Currently [code lenses](/command/windsurf-related-features) do not yet trigger the Devin Local agent.
* **Fast Context** - Devin Local uses subagents to explore code, but doesn't have the same fast context UI as Cascade.
* **App Deploys** - The Devin Local agent does not support app deploys.
* **Conversation Sharing** - Conversation sharing is not yet available with the Devin Local agent.

The Devin Local agent does support [rules and AGENTS.md files](https://cli.devin.ai/docs/extensibility/rules) as well as [skills](https://cli.devin.ai/docs/extensibility/skills/overview) for providing persistent context and reusable workflows.

## Enterprise controls

Enterprise admins can configure the Devin Local agent through [team settings](https://windsurf.com/team/settings), including [new controls only available with the Devin Local agent](https://cli.devin.ai/docs/enterprise/team-settings):

* **Sandbox enforcement** - Require sandbox mode for all users and configure organization-wide domain filtering rules
* **Granular permissions** - Control which actions the agent can take with more fine-grained permissions
* **Network enforcement** - Control network access with allowed and denied domains

Additionally, the "Enable Cascade" control can be used to disable the legacy Cascade agent entirely to ensure your team follows the new controls available with Devin CLI.

### Unsupported enterprise controls

The following legacy enterprise controls are not available with the Devin Local agent:

* **Restrict Tool Calls to Workspace** - by default, the Devin Local agent can only read/edit files within the workspace.
  Custom [permissions](https://cli.devin.ai/docs/reference/permissions) are a more flexible replacement that can be used to replicate the same rules.
* **App Deploys** - App deploys are not yet supported with the Devin Local agent.
* **Conversation Sharing** - Conversation sharing is not yet supported with the Devin Local agent.
* **Auto Run Terminal Commands** - The Devin Local agent uses its own [permissions model](https://cli.devin.ai/docs/reference/permissions) instead of auto-execution levels.
* **Attribution Filtering** - Attribution filtering is not yet supported with the Devin Local agent.

## Further reading

* [Devin CLI quickstart](https://cli.devin.ai/docs)
* [Essential commands](https://cli.devin.ai/docs/essential-commands)
* [Extensibility overview](https://cli.devin.ai/docs/extensibility)
* [Team settings](https://cli.devin.ai/docs/enterprise/team-settings)
