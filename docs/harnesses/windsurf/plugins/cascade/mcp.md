# Model Context Protocol (MCP)

Configure MCP servers to extend Cascade with custom tools and services using stdio, HTTP, or SSE transports with admin controls for Teams and Enterprise.

**MCP (Model Context Protocol)** is a protocol that enables LLMs to access custom tools and services.
An MCP client (Cascade, in this case) can make requests to MCP servers to access tools that they provide.
Cascade now natively integrates with MCP, allowing you to bring your own selection of MCP servers for Cascade to use.
See the [official MCP docs](https://modelcontextprotocol.io/) for more information.

<Note>Enterprise users must manually turn this on via settings</Note>

## Adding a new MCP plugin

New MCP plugins can be added by going to the `Settings` > `Tools` > `Windsurf Settings` > `Add Server` section.

If you cannot find your desired MCP plugin, you can add it manually by clicking `View Raw Config` button and editing the raw `mcp_config.json` file.

When you click on an MCP server, simply click `+ Add Server` to expose the server and its tools to Cascade.

<Frame>
  <img />
</Frame>

Cascade supports three [transport types](https://modelcontextprotocol.io/docs/concepts/transports) for MCP
servers: `stdio`,  `Streamable HTTP`, and `SSE`.

Cascade also supports OAuth for each transport type.

For `http` servers, the URL should reflect that of the endpoint and resemble `https://<your-server-url>/mcp`.

<Note>Make sure to press the refresh button after you add a new MCP plugin.</Note>

## mcp\_config.json

The `~/.codeium/mcp_config.json` file is a JSON file that contains a list of servers that Cascade can connect to.

Here’s an example configuration, which sets up a single server for GitHub:

```json theme={null}
{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": [
        "-y",
        "@modelcontextprotocol/server-github"
      ],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "<YOUR_PERSONAL_ACCESS_TOKEN>"
      }
    }
  }
}
```

Be sure to provide the required arguments and environment variables for the servers that you want to use.

See the [official MCP server reference repository](https://github.com/modelcontextprotocol/servers) or [OpenTools](https://opentools.com/) for some example servers.

### Remote HTTP MCPs

It's important to note that for remote HTTP MCPs, the configuration is slightly
different and requires a `serverUrl` or `url` field.

Here's an example configuration for an HTTP server:

```json theme={null}
{
  "mcpServers": {
    "remote-http-mcp": {
      "serverUrl": "<your-server-url>/mcp",
      "headers": {
        "API_KEY": "value"
      }
    }
  }
}
```

### Config Interpolation

The `~/.codeium/mcp_config.json` file handles interpolation of
environment variables in these fields: `command`, `args`, `env`, `serverUrl`, `url`, and
`headers`.

Here’s an example configuration, which uses an `AUTH_TOKEN` environment variable
in `headers`.

```json theme={null}
{
  "mcpServers": {
    "remote-http-mcp": {
      "serverUrl": "<your-server-url>/mcp",
      "headers": {
        "API_KEY": "Bearer ${env:AUTH_TOKEN}"
      }
    }
  }
}
```

## Admin Controls (Teams & Enterprises)

Team admins can toggle MCP access for their team, as well as whitelist approved MCP servers for their team to use:

<Card title="MCP Team Settings" icon="hammer" href="https://windsurf.com/team/settings">
  Configurable MCP settings for your team.
</Card>

<Warning>The above link will only work if you have admin privileges for your team.</Warning>

By default, users within a team will be able to configure their own MCP servers. However, once you whitelist even a single MCP server, **all non-whitelisted servers will be blocked** for your team.

### How Server Matching Works

When you whitelist an MCP server, the system uses **regex pattern matching** with the following rules:

* **Full String Matching**: All patterns are automatically anchored (wrapped with `^(?:pattern)$`) to prevent partial matches
* **Command Field**: Must match exactly or according to your regex pattern
* **Arguments Array**: Each argument is matched individually against its corresponding pattern
* **Array Length**: The number of arguments must match exactly between whitelist and user config
* **Special Characters**: Characters like `$`, `.`, `[`, `]`, `(`, `)` have special regex meaning and should be escaped with `\` if you want literal matching

### Configuration Options

<AccordionGroup>
  <Accordion title="Option 1: Plugin Store Default (Recommended)" description="Leave the Server Config (JSON) field empty to allow the default configuration from the Windsurf MCP Plugin Store.">
    **Admin Whitelist Configuration:**

    * **Server ID**: `github-mcp-server`
    * **Server Config (JSON)**: *(leave empty)*

    ```json theme={null}
    {}
    ```

    **Matching User Config (`mcp_config.json`):**

    ```json theme={null}
    {
      "mcpServers": {
        "github-mcp-server": {
          "command": "docker",
          "args": [
            "run",
            "-i",
            "--rm",
            "-e",
            "GITHUB_PERSONAL_ACCESS_TOKEN",
            "ghcr.io/github/github-mcp-server"
          ],
          "env": {
            "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_your_token_here"
          }
        }
      }
    }
    ```

    This allows users to install the GitHub MCP server with any valid configuration, as long as the server ID matches the plugin store entry.
  </Accordion>

  <Accordion title="Option 2: Exact Match Configuration" description="Provide the exact configuration that users must use. Users must match this configuration exactly.">
    **Admin Whitelist Configuration:**

    * **Server ID**: `github-mcp-server`
    * **Server Config (JSON)**:

    ```json theme={null}
    {
      "command": "docker",
      "args": [
        "run",
        "-i",
        "--rm",
        "-e",
        "GITHUB_PERSONAL_ACCESS_TOKEN",
        "ghcr.io/github/github-mcp-server"
      ],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": ""
      }
    }
    ```

    **Matching User Config (`mcp_config.json`):**

    ```json theme={null}
    {
      "mcpServers": {
        "github-mcp-server": {
          "command": "docker",
          "args": [
            "run",
            "-i",
            "--rm",
            "-e",
            "GITHUB_PERSONAL_ACCESS_TOKEN",
            "ghcr.io/github/github-mcp-server"
          ],
          "env": {
            "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_your_token_here"
          }
        }
      }
    }
    ```

    Users must use this exact configuration - any deviation in command or args will be blocked. The `env` section can have different values.
  </Accordion>

  <Accordion title="Option 3: Flexible Regex Patterns" description="Use regex patterns to allow variations in user configurations while maintaining security controls.">
    **Admin Whitelist Configuration:**

    * **Server ID**: `python-mcp-server`
    * **Server Config (JSON)**:

    ```json theme={null}
    {
      "command": "python3",
      "args": ["/.*\\.py", "--port", "[0-9]+"]
    }
    ```

    **Matching User Config (`mcp_config.json`):**

    ```json theme={null}
    {
      "mcpServers": {
        "python-mcp-server": {
          "command": "python3",
          "args": ["/home/user/my_server.py", "--port", "8080"],
          "env": {
            "PYTHONPATH": "/home/user/mcp"
          }
        }
      }
    }
    ```

    This example allows users flexibility while maintaining security:

    * The regex `/.*\\.py` matches any Python file path like `/home/user/my_server.py`
    * The regex `[0-9]+` matches any numeric port like `8080` or `3000`
    * Users can customize file paths and ports while admins ensure only Python scripts are executed
  </Accordion>
</AccordionGroup>

### Common Regex Patterns

| Pattern         | Matches                   | Example                |
| --------------- | ------------------------- | ---------------------- |
| `.*`            | Any string                | `/home/user/script.py` |
| `[0-9]+`        | Any number                | `8080`, `3000`         |
| `[a-zA-Z0-9_]+` | Alphanumeric + underscore | `api_key_123`          |
| `\\$HOME`       | Literal `$HOME`           | `$HOME` (not expanded) |
| `\\.py`         | Literal `.py`             | `script.py`            |
| `\\[cli\\]`     | Literal `[cli]`           | `mcp[cli]`             |

## Notes

### Admin Configuration Guidelines

* **Environment Variables**: The `env` section is not regex-matched and can be configured freely by users
* **Disabled Tools**: The `disabledTools` array is handled separately and not part of whitelist matching
* **Case Sensitivity**: All matching is case-sensitive
* **Error Handling**: Invalid regex patterns will be logged and result in access denial
* **Testing**: Test your regex patterns carefully - overly restrictive patterns may block legitimate use cases

### Troubleshooting

If users report that their MCP servers aren't working after whitelisting:

1. **Check Exact Matching**: Ensure the whitelist pattern exactly matches the user's configuration
2. **Verify Regex Escaping**: Special characters may need escaping (e.g., `\.` for literal dots)
3. **Review Logs**: Invalid regex patterns are logged with warnings
4. **Test Patterns**: Use a regex tester to verify your patterns work as expected

Remember: Once you whitelist any server, **all other servers are automatically blocked** for your team members.

### General Information

* Since MCP tool calls can invoke code written by arbitrary server implementers, we do not assume liability
  for MCP tool call failures. To reiterate:
* We currently support an MCP server's [tools](https://modelcontextprotocol.io/docs/concepts/tools), [resources](https://modelcontextprotocol.io/docs/concepts/resources), and [prompts](https://modelcontextprotocol.io/docs/concepts/prompts).
