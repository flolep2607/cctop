# LLM Safety and Controls

AI models can behave unexpectedly. This documentation covers how to control what agents can do, set up safety guardrails, and guide LLM behavior toward desired outcomes.

## Understanding model behavior

LLMs generate text based on probability distributions, not by retrieving facts from a database or executing deterministic logic. They can produce different outputs for the same input, hallucinate facts or code that seems plausible but is wrong, and be influenced by carefully crafted prompts (prompt injection).

You can't rely on LLMs to always make safe decisions. Instead, you combine two approaches: **security controls** that enforce hard boundaries on what agents can do, and **steering mechanisms** that guide LLM behavior toward better outcomes.

For a deeper understanding of how LLMs work, see [How AI Models Work](https://cursor.com/learn/how-ai-models-work.md).

## Two approaches to safety

Cursor provides two complementary approaches to managing AI agent behavior:

**Security controls (deterministic enforcement)**: Hard boundaries that block dangerous operations regardless of what the LLM suggests. These include terminal command restrictions, enforcement hooks that reject operations, approval workflows, and sandboxing. Security controls are your primary defense against harmful agent actions.

**LLM steering (non-deterministic guidance)**: Mechanisms that guide the LLM toward better behavior by shaping its context and available actions. These include Rules that add instructions to prompts, Commands that provide reusable workflows, and integrations that enrich the agent's knowledge. Steering improves agent quality but doesn't guarantee prevention of harmful actions.

Use both approaches together. Security controls provide the safety net. Steering reduces how often agents attempt problematic actions in the first place.

## Security controls

These deterministic controls enforce hard boundaries on what agents can do. They work regardless of what the LLM suggests.

### Terminal command restrictions

By default, Cursor requires your approval before executing any terminal command. This protects against destructive commands (deleting files, dropping databases), commands that expose sensitive data, and commands with unintended side effects.

When an agent wants to run a command, you see a prompt showing the full command. You can approve and run it, deny it, or modify it before running.

#### Auto-approval risks

You can enable auto-approval for terminal commands, but understand the risks. Agents might run destructive commands without your knowledge, commands execute before you can review them, and bugs or prompt injection could cause unintended operations.

#### Run Mode configuration

Enterprise teams can configure Run Mode policies in the team dashboard. In Cursor 3.6 and above, end users choose between **Auto-review** (the default), **Allowlist**, and **Run Everything** modes. **Auto-review** runs allowlisted calls, sandboxes shell commands when it can, and routes the rest through an LLM classifier that returns allow or block based on safety and how well the call matches the user's intent. You can create an allowlist of commands that don't require approval, such as `npm install`, `pip install`, `cargo build`, or `make test`.

The allowlist is best-effort, not a security boundary. Determined agents or prompt injection might bypass it. Always combine allowlists with other security controls like hooks.

See [Run Modes](https://cursor.com/docs/agent/security/run-modes.md#run-mode) and [Agent Security](https://cursor.com/docs/agent/security.md) for details.

### Enforcement hooks

Hooks let you run custom logic at key points in the agent loop.

- Before prompt submission: Scan prompts for sensitive data before they're sent to LLMs. Block submissions that contain API keys or credentials, personal identifiable information (PII), or proprietary information.
- Before file reading: Scan files before agents read them. Redact or block access to configuration files with secrets, PII in databases or logs, or proprietary algorithms.
- After code generation: Scan generated code before it's written to disk. Check for security vulnerabilities (SQL injection, XSS), licensed code that might cause IP issues, or API keys and credentials in code.
- Before terminal execution: Block dangerous commands or route them through approval workflows. For example, block all `git push` commands, require approval for any `sudo` command, or block database `DROP` statements.

#### Example: Blocking git commands

This hook intercepts shell commands and blocks raw git usage, directing users to the GitHub CLI instead:

```bash
#!/bin/bash
input=$(cat)
command=$(echo "$input" | jq -r '.command')

if [[ "$command" =~ git[[:space:]] ]]; then
    cat << EOF
{
  "permission": "deny",
  "userMessage": "Git command blocked. Please use gh tool instead.",
  "agentMessage": "Use 'gh' commands instead of raw git."
}
EOF
fi
```

#### Example: Redacting secrets

This hook scans file contents for GitHub API keys and blocks access if found:

```bash
#!/bin/bash
input=$(cat)
content=$(echo "$input" | jq -r '.content')

if echo "$content" | grep -qE 'gh[ps]_[A-Za-z0-9]{36}'; then
    cat << EOF
{
  "permission": "deny"
}
EOF
    exit 3
fi
```

See [Hooks](https://cursor.com/docs/hooks.md) for complete documentation and more examples.

### Protecting sensitive files

Not all files in your repositories should be accessible to AI. Configuration files, secrets, and sensitive data need protection.

#### .cursorignore

The `.cursorignore` file works like `.gitignore` but controls what Cursor can access. Files matching patterns in `.cursorignore` are excluded from:

- Agent file reading
- Context selection

`.cursorignore` is not a security boundary. It's a convenience feature to exclude files from AI processing, but:

- Users can manually read ignored files
- Agents might find ways to access ignored content
- It doesn't prevent file access, only excludes from indexing

For true security, use file system permissions or encrypt sensitive data.

See [Ignore Files](https://cursor.com/docs/reference/ignore-file.md) for detailed syntax.

#### .cursor directory protection

The `.cursor` directory in repositories contains project-specific settings, rules, and cache files. Enterprise teams can prevent agents from modifying this directory.

When enabled, agents cannot:

- Modify files in `.cursor/`
- Delete the `.cursor/` directory
- Change cursor rules or settings files

Users can still manually edit these files, but agents require approval.

Configure in the [team dashboard](https://cursor.com/docs/account/teams/dashboard.md) under ".cursor Directory Protection" (Enterprise only).

#### Browser origin controls

Enterprise teams can restrict which websites agents can navigate to when using the [browser tool](https://cursor.com/docs/agent/tools/browser.md). Define an allowlist of approved domains—agents attempting to visit other origins are blocked.

Configure in the [team dashboard](https://cursor.com/docs/account/teams/dashboard.md) under "Browser Controls" (Enterprise only).

### Integration with DLP tools

Many enterprises have existing Data Loss Prevention (DLP) tools that scan for sensitive data. You can integrate Cursor with your DLP tools in three ways.

#### Endpoint DLP agents

Most endpoint DLP software can inspect Cursor's network traffic. Configure your DLP to monitor traffic to `*.cursor.sh` domains, scan for sensitive patterns in outbound requests, and block or alert on policy violations.

Network DLP may impact performance. See [Network Configuration](https://cursor.com/docs/enterprise/network-configuration.md) for proxy considerations.

#### Hooks-based DLP

Use Cursor's hooks feature to implement custom DLP logic:

**Before prompt submission:**
Scan prompts for sensitive patterns before sending to LLMs:

```bash
#!/bin/bash
input=$(cat)
prompt=$(echo "$input" | jq -r '.prompt')

# Check for API keys
if echo "$prompt" | grep -qE 'api[_-]?key.*[A-Za-z0-9]{32}'; then
    cat << EOF
{
  "continue": false,
  "userMessage": "Prompt contains what looks like an API key. Remove it and try again."
}
EOF
    exit 1
fi

# Allow if no sensitive data found
cat << EOF
{
  "continue": true
}
EOF
```

**After code generation:**
Scan generated code before it's written to disk:

```bash
#!/bin/bash
input=$(cat)
file_path=$(echo "$input" | jq -r '.file_path')
edits=$(echo "$input" | jq -r '.edits[].new_string')

# Check for hardcoded credentials
if echo "$edits" | grep -qE 'password.*=.*["\047][^"\047]+["\047]'; then
    # Send to your DLP API for analysis
    curl -X POST "https://dlp.yourcompany.com/scan" \
      -H "Content-Type: application/json" \
      -d "{\"content\":\"$edits\",\"file\":\"$file_path\"}"
    
    # Check API response and act accordingly
fi
```

#### Third-party DLP integration

Call your existing DLP vendor's API from hooks:

```bash
#!/bin/bash
input=$(cat)
content=$(echo "$input" | jq -r '.content')

# Send to DLP API
response=$(curl -s -X POST "https://dlp-api.company.com/analyze" \
  -H "Authorization: Bearer $DLP_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"text\":\"$content\"}")

# Parse response
is_allowed=$(echo "$response" | jq -r '.allowed')

if [ "$is_allowed" = "true" ]; then
    cat << EOF
{
  "permission": "allow"
}
EOF
else
    violation=$(echo "$response" | jq -r '.violation_type')
    cat << EOF
{
  "permission": "deny",
  "userMessage": "Content blocked by DLP policy: $violation"
}
EOF
fi
```

This approach gives you centralized DLP policy management across all development tools.

### Approval workflows

You can configure Cursor to ask for approval on every agent action. Users can set their agent to always ask before reading files, editing files, running terminal commands, or making network requests.

However, this approach significantly slows down the development experience. Agents need multiple actions to complete tasks, and requiring approval for each action makes the workflow tedious. Most teams instead choose to use hooks to block dangerous operations automatically.

### Model provider safety

All model providers (OpenAI, Anthropic, Google, SpaceXAI) implement safety systems that filter harmful content. These systems reject prompts requesting harmful information, refuse to generate dangerous code, and filter outputs for safety.

Cursor works with providers to ensure models meet safety standards before deployment to users. Providers continuously evaluate models for safety issues. However, these are not security boundaries. Safety systems can be bypassed or tricked. Always implement your own controls through hooks and access policies.

### Sandboxing considerations

Cursor agents run on your local machine by default. They can read files you can read, write files you can write, execute commands you can execute, and access network resources you can access.

There is no security boundary between agents and your user account. If your account can delete files, agents can delete files (with approval by default).

#### Sandboxing options

If you need stronger isolation, run Cursor in a separate VM using Cloud Agents, use file system permissions to limit what the Cursor process can access, or run Cursor on a dedicated development machine with limited access to production systems.

For most enterprises, the built-in approval requirements and hooks provide sufficient control.

### File system permissions

For further defense, use file system permissions to protect sensitive files:

**Restrict access to secret files:**

```bash
# Make secrets readable only by specific users
chmod 600 .env
chown app-user:app-user .env

# Or use separate directories with restricted access
chmod 700 /etc/app/secrets
```

**Separate sensitive repos:**
Keep highly sensitive code in separate repositories with restricted access. Don't clone these repositories to machines where Cursor runs.

**Encrypted filesystems:**
For very sensitive data, use encrypted filesystems that require explicit mounting. Don't mount these filesystems in directories where Cursor has access.

## LLM steering

Security controls block harmful actions after the LLM suggests them. Steering mechanisms guide the LLM to make better suggestions in the first place. These are non-deterministic. They improve outcomes but don't guarantee prevention.

### Rules

Rules add instructions to the LLM's context window before every request. Use rules to establish coding standards, enforce architectural patterns, set security requirements, or define project-specific conventions.

Rules work at three scopes:

**User rules**: Apply to all projects for a specific user. Use these for personal preferences like code style or preferred libraries.

**Project rules**: Apply to everyone working on a project. Use these for project-specific standards like naming conventions or framework usage.

**Team rules**: Apply to all projects in your organization. Use these for company-wide standards like security requirements or compliance rules.

The LLM sees all applicable rules when generating responses. It will attempt to follow them, but rules are suggestions, not guarantees. Combine rules with enforcement hooks for requirements that must be followed.

See [Rules](https://cursor.com/docs/rules.md) for configuration and examples.

### Commands and workflows

Commands package reusable prompts that agents can invoke with slash commands like `/test` or `/deploy`. Commands help standardize common workflows across your team.

**Workflows**: Create multi-step processes that guide agents through complex tasks. For example, a `/security-review` command might instruct the agent to scan for SQL injection, check for exposed secrets, validate input sanitization, and generate a security report.

**Prompt libraries**: Build a collection of tested prompts for common tasks. This reduces variation in agent behavior and captures institutional knowledge.

Commands are scoped to teams, projects, or users. Team admins can create organization-wide commands that appear for all developers.

See [Commands](https://cursor.com/help/customization/rules.md) for configuration and examples.

### Context enrichment with MCPs

Model Context Protocol (MCP) servers let agents access external data sources. Use MCPs to pull in company documentation, query internal APIs, access knowledge bases, or integrate with development tools.

MCPs enrich the agent's context with information it wouldn't otherwise have. For example, an MCP might provide access to your API specifications, so agents can generate code that correctly calls your internal services.

MCPs are scoped to teams or users. Unlike hooks, MCPs don't enforce policies—they provide information that helps agents make better decisions.

See [MCP Integration](https://cursor.com/docs/mcp.md) for configuration and examples.

### Advanced safety controls for Enterprise

Contact our team to learn about org-wide enforcement and security policies.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
