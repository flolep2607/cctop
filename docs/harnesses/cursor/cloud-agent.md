# Cloud Agents

Cloud agents use the same [agent fundamentals](https://cursor.com/learn/agents.md) but run in isolated VMs in the cloud with full development environments instead of on your local machine. The development environment is similar to the setup on your laptop: cloned repos, installed dependencies, secrets, startup commands, and network access.

Effective development environments give agents full context on your codebase and organization, so they can test and verify their work.

## Why use Cloud Agents?

You can run as many agents as you want in parallel, and they do not require your local machine to be connected to the internet.

Because they have access to their own virtual machine, cloud agents can build, test, and interact with the changed software. They can also use computers to control the desktop and browser. Cloud agents support [MCP servers](https://cursor.com/docs/mcp.md), giving them access to external tools and data sources like databases, APIs, and third-party services.

Cloud agents can also run in multi-repo environments. Use one when a task spans separate frontend, backend, infrastructure, or shared-library repositories. The agent can inspect the full workspace, make coordinated changes, and open pull requests in the repos it changes. Long-running is not available for multi-repo environments yet.

## How to access

Before anyone can start a cloud agent from a repository, a Cursor account admin needs to connect source control for the account. Set up [GitHub (Cloud and Enterprise Server)](https://cursor.com/docs/integrations/github.md), [GitLab (Cloud and Self-Hosted)](https://cursor.com/docs/integrations/gitlab.md), [Bitbucket Cloud](https://cursor.com/docs/integrations/bitbucket.md), or [Azure DevOps](https://cursor.com/docs/integrations/azure-devops.md).

You can kick off cloud agents from wherever you work:

1. **Cursor for iOS**: Start and manage agents from the [Cursor iOS app](https://cursor.com/docs/cloud-agent/mobile.md)
2. **Cursor Web**: Start and manage agents from [cursor.com/agents](https://cursor.com/agents) on any device
3. **Cursor Desktop**: Select **Cloud** in the dropdown under the agent input
4. **Slack**: Use the @cursor command to kick off an agent
5. **GitHub or Bitbucket**: Comment `@cursor` on a GitHub PR or issue, or on a Bitbucket PR, to kick off an agent
6. **Linear**: Use the @cursor command to kick off an agent
7. **API**: Use the API to kick off an agent

On **Android**, use [cursor.com/agents](https://cursor.com/agents) in Chrome
and tap **Install App** for a Progressive Web App (PWA). See [Cursor for
iOS](https://cursor.com/docs/cloud-agent/mobile.md) for the native iPhone and iPad app and more mobile
options.

### Use Cursor in Slack

Learn more about setting up and using the Slack integration, including
triggering agents and receiving notifications.

## How it works

### Repository provider connection

Cloud agents clone your repo from GitHub, GitLab, Azure DevOps Services, or Bitbucket Cloud and work on a separate branch, then push changes to your repo for handoff.

You need read-write privileges to your repo and any dependent repos or submodules.

### Environments

Agents are only as capable as the environments they run in. An agent that can write code but can't run tests, query services, or reach APIs cannot close the loop on its work.

Not setting up a development environment for your cloud agents is like not giving your engineers a computer. This is why environment setup is the most important step to improve the effectiveness of cloud agents. It lets cloud agents work like engineers do: write code, test and verify work, and ship software.

You can configure environments with agent-led setup, a saved snapshot, or a Dockerfile in `.cursor/environment.json`. See [Cloud agent setup](https://cursor.com/docs/cloud-agent/setup.md) to get started. [Builds](https://cursor.com/docs/cloud-agent/builds.md) prepare each environment in the background so agents start with repositories and dependencies ready.

The Cloud Agents dashboard shows which environment and Build an agent used, along with environment details and version history. On the agent page, hover over the repository name at the top of the page to inspect the environment used for that run. See [Cloud agent setup](https://cursor.com/docs/cloud-agent/setup.md) for configuration details.

### Runtime and environment controls

Cursor manages VM provisioning, isolation, snapshots, startup, artifacts, and capacity for every Cloud Agent. You can add secrets, restrict outbound domains, connect to private networks with Tailscale or a similar client, and use private connectivity for supported source control paths.

See [Cloud Agent security and network](https://cursor.com/docs/cloud-agent/security-network.md) for the full set of environment and network controls. If you're weighing whether to self-host, see [why most teams start with Cursor Cloud](https://cursor.com/docs/cloud-agent/self-hosted.md).

## Models

Cloud Agents use a curated selection of models. You can select the context window size for supported models.

## MCP support

Cloud agents can use [MCP (Model Context Protocol)](https://cursor.com/docs/mcp.md) servers configured for your team. Add and manage MCP servers through the MCP dropdown in [cursor.com/agents](https://cursor.com/agents).

Both HTTP and stdio transports are supported. OAuth is supported for MCP servers that need it. See [Cloud Agent capabilities](https://cursor.com/docs/cloud-agent/capabilities.md) for setup details.

Cloud Agents also include a built-in [Cursor Cloud MCP](https://cursor.com/docs/cloud-agent/capabilities.md#cursor-cloud-mcp) for run diagnostics, including transcripts, run events, environment details, and setup logs.

## Hooks support

Cloud agents run command-based hooks from `.cursor/hooks.json` in your repository. On Enterprise plans, they also run team hooks and enterprise-managed hooks.

This keeps formatters, audit scripts, and policy checks active when work runs in the cloud. Supported hooks include tool and file hooks (`preToolUse`, `beforeShellExecution`, `afterFileEdit`), plus lifecycle hooks (`beforeSubmitPrompt`, `subagentStart` / `subagentStop`, `preCompact`, `afterAgentResponse` / `afterAgentThought`, and `stop`).

Hooks do not run during early exploratory turns in a read-only environment; they start once the agent has a writable environment. Some hooks are IDE-specific (Tab hooks, `workspaceOpen`). User-level hooks from `~/.cursor/hooks.json` are also not available since cloud VMs don't have access to your local home directory.

See [Hooks: Cloud agent support](https://cursor.com/docs/hooks.md#cloud-agent-support) for the full support matrix and details.

## Artifacts and remote desktop control

Cloud agents produce merge-ready PRs with artifacts to demo their changes. You can also control the agent's remote desktop to use the modified software.

- **Artifacts**: Agents produce screenshots, videos, and logs so you can see exactly what changed and how the agent verified its work.
- **Remote desktop control**: Take control of the agent's desktop to test the software yourself in a full development environment without checking out the branch locally. Release control back to the agent for it to keep working.

See [Cloud agent capabilities](https://cursor.com/docs/cloud-agent/capabilities.md) for details on artifacts, computer use, and remote desktop control.

## Share agents with your team

Send an agent's URL to a teammate and they can open the run to see the conversation, the code changes, and the artifacts the agent produced.

Agents are visible to members of the Cursor team they were started under. Cursor also verifies each viewer's repository access: to open a teammate's agent, connect your own source control account under [Integrations](https://cursor.com/dashboard/integrations) and make sure you have access to the repository the agent worked in. Team membership alone doesn't grant access.

Viewing is read-only. To let teammates send follow-up messages and continue the work, a team admin can enable [team follow-ups](https://cursor.com/docs/cloud-agent/settings.md#team-follow-ups).

## Related pages

- Learn more about [Cloud agent capabilities](https://cursor.com/docs/cloud-agent/capabilities.md).
- Learn more about [Cloud agent setup](https://cursor.com/docs/cloud-agent/setup.md).
- Learn more about [Cloud Agent Builds](https://cursor.com/docs/cloud-agent/builds.md).
- Learn more about [Cloud agent security](https://cursor.com/docs/cloud-agent/security-network.md).
- Learn more about [OIDC tokens](https://cursor.com/docs/cloud-agent/identity.md).
- Learn more about [Agent metadata](https://cursor.com/docs/cloud-agent/metadata.md).
- Learn more about [Cloud agent settings](https://cursor.com/docs/cloud-agent/settings.md).

## Billing

Cloud Agents are charged at API pricing for the selected [model](https://cursor.com/docs/models-and-pricing.md#model-pricing). You can select the context window size, and a larger context window can increase token usage and costs. You'll be asked to set a spend limit when you first start using them.

## Troubleshooting

### Agent runs are not starting

- Ensure you're logged in and have connected your GitHub, GitLab, Azure DevOps, or Bitbucket account.
- Check that you have the necessary repository permissions.
- You need to be on a paid Cursor plan.

### My secrets aren't available to the cloud agent

- Ensure you've added secrets in [cursor.com/dashboard/cloud-agents](https://cursor.com/dashboard/cloud-agents)
- Secrets are workspace/team-scoped; make sure you're using the correct account
- Try restarting the cloud agent after adding new secrets

### Can't find the Secrets tab

- If you don't see it, ensure you have the necessary permissions

### Do snapshots copy .env.local files?

Snapshots save your base environment configuration (installed packages, system dependencies, etc.).
If you include `.env.local` files during snapshot creation, they will be saved. However, using the Secrets tab
in Cursor Settings is the recommended approach for managing environment variables.

### A teammate can't open my agent

- The teammate must belong to the same Cursor team as you.
- They need to connect their own source control account under [Integrations](https://cursor.com/dashboard/integrations) and have access to the repository the agent worked in.
- See [Share agents with your team](https://cursor.com/docs/cloud-agent.md#share-agents-with-your-team) for details.

### Slack integration not working

Verify that your workspace admin has installed the Cursor Slack app and that
you have the proper permissions.

## Naming History

Cloud Agents were formerly called Background Agents.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
