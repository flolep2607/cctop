# Extension API reference

Cursor exposes extension APIs under `vscode.cursor` for programmatic configuration. Use these APIs from VS Code extensions to register MCP servers and plugin paths without editing config files.

## Type definitions

Copy this `declare module` block into your extension project for type checking:

```typescript
declare module "vscode" {
  export namespace cursor {
    export namespace mcp {
      export interface StdioServerConfig {
        name: string;
        server: {
          command: string;
          args: string[];
          env: Record<string, string>;
        };
      }

      export interface RemoteServerConfig {
        name: string;
        server: {
          url: string;
          /**
           * Optional HTTP headers to include with every request to this server
           * (e.g. for authentication).
           */
          headers?: Record<string, string>;
        };
      }

      export type ExtMCPServerConfig = StdioServerConfig | RemoteServerConfig;

      /**
       * Register an MCP server that Cursor can communicate with.
       * Supports HTTP(S) (SSE/streamable HTTP) and local stdio processes.
       */
      export const registerServer: (config: ExtMCPServerConfig) => void;
      export const unregisterServer: (serverName: string) => void;
    }

    export namespace plugins {
      /**
       * Register a directory as a plugin source. Cursor discovers and loads
       * any valid plugins in this directory.
       */
      export const registerPath: (path: string) => void;
      export const unregisterPath: (path: string) => void;
    }
  }
}
```

## MCP servers

Register and manage MCP servers at runtime. This is useful for enterprise environments, onboarding tools, and automated setup workflows where editing `mcp.json` isn't practical.

### `vscode.cursor.mcp.registerServer`

Registers an MCP server.

**Signature:**

```typescript
vscode.cursor.mcp.registerServer(config: ExtMCPServerConfig): void
```

**Parameters:**

- `config: ExtMCPServerConfig` - The server configuration object

### `vscode.cursor.mcp.unregisterServer`

Unregisters a previously registered MCP server.

**Signature:**

```typescript
vscode.cursor.mcp.unregisterServer(serverName: string): void
```

**Parameters:**

- `serverName: string` - The name of the server to unregister

### Configuration types

#### HTTP/SSE server

For servers running on HTTP or Server-Sent Events:

```typescript
interface RemoteServerConfig {
  name: string;
  server: {
    url: string;
    headers?: Record<string, string>;
  };
}
```

**Properties:**

- `name`: Unique identifier for the server
- `server.url`: The HTTP endpoint URL
- `server.headers` (optional): HTTP headers for authentication or other purposes

#### Stdio server

For local servers communicating via standard input/output:

```typescript
interface StdioServerConfig {
  name: string;
  server: {
    command: string;
    args: string[];
    env: Record<string, string>;
  };
}
```

**Properties:**

- `name`: Unique identifier for the server
- `server.command`: The executable command
- `server.args`: Command line arguments
- `server.env`: Environment variables

### MCP examples

#### HTTP/SSE server

Register a remote MCP server with authentication:

```typescript
vscode.cursor.mcp.registerServer({
  name: "my-remote-server",
  server: {
    url: "https://api.example.com/mcp",
    headers: {
      Authorization: "Bearer your-token-here",
      "X-API-Key": "your-api-key",
    },
  },
});
```

#### Stdio server

Register a local MCP server:

```typescript
vscode.cursor.mcp.registerServer({
  name: "my-local-server",
  server: {
    command: "python",
    args: ["-m", "my_mcp_server"],
    env: {
      API_KEY: "your-api-key",
      DEBUG: "true",
    },
  },
});
```

#### Node.js server

Register a Node.js-based MCP server:

```typescript
vscode.cursor.mcp.registerServer({
  name: "nodejs-server",
  server: {
    command: "npx",
    args: ["-y", "@company/mcp-server"],
    env: {
      NODE_ENV: "production",
      CONFIG_PATH: "/path/to/config",
    },
  },
});
```

#### Unregister a server

```typescript
vscode.cursor.mcp.unregisterServer("my-remote-server");
```

#### Conditional registration

```typescript
if (!isServerRegistered("my-server")) {
  vscode.cursor.mcp.registerServer({
    name: "my-server",
    server: {
      url: "https://api.example.com/mcp",
    },
  });
}
```

## Plugin paths

Register additional plugin directories at runtime. Extensions can use this API to tell Cursor about plugin locations without requiring users to manually copy files to `~/.cursor/plugins/local/`.

A `.cursor-plugin/plugin.json` manifest is optional. Without one, Cursor uses [automatic folder-based discovery](https://cursor.com/docs/reference/plugins.md#cursor-plugin-component-discovery) and picks up components from default locations: `rules/`, `skills/`, `agents/`, `commands/`, `mcp.json`, and `hooks/hooks.json`. For example, to inject skills you can register a directory that contains a `skills/` subfolder; no manifest needed.

```text
my-extension/cursor-plugins/team-tools/
├── skills/
│   └── deploy-helper/
│       └── SKILL.md
└── rules/
    └── coding-standards.mdc
```

For the full manifest schema and all component formats, see the [Plugins guide](https://cursor.com/docs/plugins.md#creating-plugins) and the [Plugins reference](https://cursor.com/docs/reference/plugins.md).

### `vscode.cursor.plugins.registerPath`

Registers a directory path as a plugin source. Cursor loads any valid plugins found in the directory.

**Signature:**

```typescript
vscode.cursor.plugins.registerPath(path: string): void
```

**Parameters:**

- `path: string` - Absolute filesystem path to a directory containing plugins

### `vscode.cursor.plugins.unregisterPath`

Removes a previously registered plugin path.

**Signature:**

```typescript
vscode.cursor.plugins.unregisterPath(path: string): void
```

**Parameters:**

- `path: string` - The path to unregister

### Plugin path examples

#### Register a bundled plugin directory

An extension can bundle plugins and register them on activation:

```typescript
import * as vscode from "vscode";
import * as path from "path";

export function activate(context: vscode.ExtensionContext) {
  const pluginsDir = path.join(context.extensionPath, "cursor-plugins");
  vscode.cursor.plugins.registerPath(pluginsDir);

  context.subscriptions.push({
    dispose: () => vscode.cursor.plugins.unregisterPath(pluginsDir),
  });
}
```

#### Register a workspace-relative path

Point Cursor at a shared plugin directory in a monorepo:

```typescript
const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
if (workspaceRoot) {
  vscode.cursor.plugins.registerPath(
    path.join(workspaceRoot, ".cursor-plugins")
  );
}
```

#### Unregister a plugin path

```typescript
vscode.cursor.plugins.unregisterPath("/path/to/plugins");
```


---

## Sitemap

[Overview of all docs pages](/llms.txt)
