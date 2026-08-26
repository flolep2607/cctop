# MCP

## Overview

The Cursor CLI supports [Model Context Protocol (MCP)](https://cursor.com/docs/mcp.md) servers, allowing you to connect external tools and data sources to `agent`. **MCP in the CLI uses the same configuration as the editor** - any MCP servers you've configured will work with both.

### Learn about MCP

New to MCP? Read the complete guide on configuration, authentication, and
available servers

## CLI commands

Use the `agent mcp` command to manage MCP servers

### List configured servers

View all configured MCP servers and their current status:

```bash
agent mcp list
```

This opens an interactive menu where you can browse, enable, and configure MCP servers at a glance. The list shows:

- Server names and identifiers
- Connection status (connected/disconnected)
- Configuration source (project or global)
- Transport method (stdio, HTTP, SSE)

You can also use the `/mcp list` slash command in interactive mode for the same interface.

### List available tools

View tools provided by a specific MCP server:

```bash
agent mcp list-tools <identifier>
```

This displays:

- Tool names and descriptions
- Required and optional parameters
- Parameter types and constraints

### Login to MCP server

Authenticate with an MCP server configured in your `mcp.json`:

```bash
agent mcp login <identifier>
```

The CLI uses a streamlined login flow with automatic callback handling. The agent gets access to authenticated MCPs immediately after login completes.

### Enable MCP server

Enable an MCP server:

```bash
agent mcp enable <identifier>
```

This adds the server to the local approved list.

### Disable MCP server

Disable an MCP server:

```bash
agent mcp disable <identifier>
```

Disabled servers won't load or prompt for approval.

MCP server names with spaces are supported in CLI commands and `/mcp list-tools`.

## Using MCP with Agent

Once you have MCP servers configured (see the [main MCP guide](https://cursor.com/docs/mcp.md) for setup), `agent` automatically discovers and uses available tools when relevant to your requests.

```bash
# Check what MCP servers are available
agent mcp list

# See what tools a specific server provides
agent mcp list-tools playwright

# Use agent - it automatically uses MCP tools when helpful
agent -p "Navigate to google.com and take a screenshot of the search page"

# Auto-approve all MCP servers (skip approval prompts)
agent --approve-mcps "query my database for recent errors"
```

The CLI follows the same configuration precedence as the editor (project → global → nested), automatically discovering configurations from parent directories.

## MCP vs ACP

Use MCP when you want Cursor to call external tools and services. Use [ACP](https://cursor.com/docs/cli/acp.md) when you want to build a custom client that talks to Cursor CLI itself over protocol messages.

## Related

### MCP Overview

Complete MCP guide: setup, configuration, and authentication

### Available MCP Tools

Browse pre-built MCP servers you can use


---

## Sitemap

[Overview of all docs pages](/llms.txt)
