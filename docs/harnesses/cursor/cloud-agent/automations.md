# Automations

Cursor Automations run [cloud agents](https://cursor.com/docs/cloud-agent.md) in the background, either on a schedule or in response to events from GitHub, GitLab, Slack, webhooks, Linear, and more.

Automations can be used to automate tasks like [reviewing recent PR commits for bugs](/marketplace/automations/find-bugs), [performing deep review for vulnerabilities](/marketplace/automations/find-vulnerabilities), [triaging bugs in Slack](/marketplace/automations/fix-slack-bugs), and [summarizing changes to your codebase on a schedule](/marketplace/automations/daily-digest).

## Getting started

Create a new automation in the [Agents Window](https://cursor.com/docs/agent/agents-window.md), at [cursor.com/automations](/automations), with the `/automate` skill from a local agent session, or from a template in the [Cursor Marketplace](/marketplace/automations).

The `/automate` skill lets you describe the workflow you want in plain language. Cursor configures the automation's triggers, instructions, and tools for you.

For any path:

1. Choose a trigger, e.g. every hour or when a pull request is opened.
2. Write a prompt with instructions for the automation.
3. Choose optional tools the agent is able to use, such as Send to Slack, Comment on Pull Request, or tools from MCP.
4. Choose whether the automation needs a repository, multiple repositories, or no repository at all.
5. Save and activate the automation.

The Automations page also includes three Cursor-managed agents:

- [Bugbot](https://cursor.com/docs/bugbot.md) reviews pull requests for bugs and code quality issues.
- [Security Agents](https://cursor.com/docs/security-agents.md) review pull requests and scan codebases for vulnerabilities.
- [PR Routing & Approval](https://cursor.com/docs/approval-agents.md) routes pull requests to reviewers and can approve low-risk changes.

## Billing

Automations create cloud agents and are billed based on cloud agent usage. See [cloud agent pricing](https://cursor.com/docs/models-and-pricing.md#model-pricing) for details.

Automations use each model's maximum supported context window because they run as cloud agents. There is no context-window toggle.

How usage is billed depends on the automation's [permission scope](https://cursor.com/docs/cloud-agent/automations.md#permissions):

- **Team Owned**: Usage is billed to the team's usage pool. Automations execute under a shared team service account, so no individual user's usage is affected.
- **Private**: Usage is billed to the user who created the automation.
- **Team Visible**: Usage is billed to the user who created the automation, the same as Private.

## Triggers

Triggers decide when an automation runs. An automation can have more than one trigger and is run when *any* trigger fires.

For certain triggers like Slack or cron schedules, Cursor defaults to not using a repository. If your automation should make code changes, specify which repository or repositories agents should work in. For source control triggers, specifying a repo or multiple repos is required.

### Scheduled triggers

Scheduled triggers run on a recurring schedule. Choose from preset options or enter a cron expression for precise control.

Scheduled triggers may run with a delay but will not start before the indicated time.

### Source control triggers

Source control triggers respond to pull request and push events from your connected Git provider: GitHub, GitLab, and Bitbucket Cloud. Connect the automation to one repository or a multi-repo environment.

Every connected provider supports the core pull request and push triggers:

- **Draft opened** - When a draft pull request is created.
- **Pull request opened** - When a non-draft PR is created or a draft is marked ready for review.
- **Pull request pushed** - When new commits are pushed to an existing PR.
- **Pull request merged** - When a PR is merged.
- **Push to branch** - When commits are pushed to a specific branch outside a pull request.
- **Comment added** - When someone leaves a top-level comment on a pull request.

GitHub supports the most triggers. GitLab and Bitbucket support the core triggers above, plus a few extras listed in their sections below.

#### GitHub triggers

GitHub is the reference provider and supports every source control trigger. Alongside the core triggers, it adds:

- **Pull request label changed** - When a specific label, or any label, is added to or removed from a pull request.
- **Issue label changed** - When a label is added to or removed from a non-PR issue.
- **CI completed** - When a GitHub check finishes on a pull request or branch.
- **Issue comment** - When a comment is made on a non-PR issue.
- **PR review comment** - When an inline comment is left on a pull request diff.
- **PR review submitted** - When a review is submitted as approved, changes requested, or commented.
- **Review thread updated** - When a review thread on a pull request is marked resolved or unresolved.
- **Workflow run completed** - When a GitHub Actions workflow run finishes on a pull request or branch.

The [Cursor Marketplace](/marketplace/automations) includes templates for [triaging failed GitHub Actions](/marketplace/automations/triage-github-workflow-failures) and [fixing pull request review comments](/marketplace/automations/autofix-pr-review-comments).

#### GitLab triggers

Alongside the core triggers, GitLab adds:

- **Pull request label changed** - When a label is added to or removed from a merge request.
- **Pull request approved** - When a merge request is approved.

#### Bitbucket triggers

Bitbucket support covers Bitbucket Cloud (`bitbucket.org`) only. Bitbucket Server and Data Center are not supported. Alongside the core triggers, it adds:

- **Pull request approved** - When a pull request is approved.

Bitbucket Cloud has no pull request label or inline review-comment triggers.

Pull request triggers don't run on PRs opened from forks. These runs fail with a "Fork pull requests not supported" error because the branch only exists on the fork, and running external code with the repo's permissions isn't safe. The exception is **Pull request merged** triggers, which still run because they start from the merge commit. To work around this, push the branch to the repo itself and open the PR from there.

### Slack triggers

Slack triggers respond to events from the [Cursor Slack integration](https://cursor.com/docs/integrations/slack.md).

Only public Slack channels are visible to Slack triggers at this time.

- **New message in channel** - When a message is sent to a connected Slack channel. Without a message filter, the trigger only fires on top-level channel messages. Add a keyword or regex filter if you want runs from threaded replies as well.
- **Emoji reaction** - When someone reacts to a Slack message with a specific emoji.
- **Channel created** - When a new public Slack channel is created in your workspace.

### Webhook triggers

Webhook triggers create a private HTTP endpoint for your automation. POST to the endpoint to start a run. You can use webhooks to connect automations to internal systems, CI pipelines, monitoring tools, and more.

To retrieve the webhook URL, you must save the automation first, which will then generate a webhook URL to call and an API key for authentication.

### Linear triggers

Linear triggers respond to events from the [Cursor Linear integration](https://cursor.com/docs/integrations/linear.md).

- **Issue created** - When a new issue is created.
- **Status changed** - When an issue's status changes.
- **End of cycle** - When a Linear cycle completes.

### Sentry triggers

Sentry triggers run when error and issue events occur in your Sentry project. Use them to automatically investigate errors, identify root causes, and propose fixes. See the [Investigate Sentry issues](/marketplace/automations/investigate-sentry-issues) marketplace template for a ready-made example.

- **Issue created** - When a new issue is created in Sentry.
- **Issue updated** - When an existing issue changes, such as a status or assignment update.
- **Any issue event** - Matches all issue event types.

### PagerDuty triggers

PagerDuty triggers run on incident events and can be helpful to automatically triage or even resolve incidents.

- **Incident triggered** - When a new incident is created.
- **Incident acknowledged** - When an incident is acknowledged.
- **Incident resolved** - When an incident is resolved.
- **Any incident event** - Matches all incident event types.

## Tools

Cursor Automations can have tools enabled for richer capabilities around GitHub, Slack, memory, MCP, and more. Automations also include the same base set of tools as other cloud agents. See [Cloud agent capabilities](https://cursor.com/docs/cloud-agent/capabilities.md) for details.

### Pull request creation

Repo-backed automations can open pull requests after making code changes requested by the automation prompt. This tool is enabled by default for every automation.

The pull request is opened against the repositories specified for the source control trigger. For other triggers, it uses the repositories specified by the environment.

### Comment on pull request

Posts comments on a target pull request. Supports top-level review comments and inline code comments.

If you enable approvals, the agent can also approve, request changes, and dismiss reviews. Otherwise, it can only post comments.

### Request reviewers

Requests reviewers on a target pull request. The agent can use `git`, memory, and other tools to identify domain experts.

### Send to Slack

Sends messages to a Slack channel. You can target a specific channel or let the agent dynamically choose any channel.

When you allow any channel, Cursor also includes the read access needed for the agent to discover available public channels.

Note that the agent is granted read access to public channels that it can send messages to.

### Read Slack channels

Gives the agent read-only access to list and read messages from public Slack channels.

Use this when the agent needs more context before it replies or opens a pull request.

### MCP server

Connects an [MCP (Model Context Protocol)](https://cursor.com/docs/mcp.md) server so the agent can use external tools and data sources.

Connecting an MCP server gives the agent access to every tool exposed by that server. Only connect servers you trust with the permissions your automation needs.

### Memories

Memories let the agent read and write persistent notes across runs for the same automation. Use this to build agents that remember and improve over time. Each memory is stored as a named entry (`MEMORIES.md` by default) that exists outside the agent's working filesystem.

Memories are enabled by default but can be disabled. Memories can be viewed and edited from the tool configuration UI.

Agents can delete outdated memory files during automation runs. You can also delete memory files from the tool configuration UI.

Memories persist across runs and should be used with caution if your automation handles untrusted input. Inputs may lead to misleading or malicious memories that unintentionally impact future automation runs.

### Computer use

Computer use lets cloud agents kicked off by automations use a computer just like a developer would. That means automations can operate a browser, produce screenshots or recordings, or use your internal services. It is included by default for every automation.

To make sure computer use is effective, ensure that you've configured a development environment for your automation. You can then ask for a demo in your automation instructions when you want the agent to show its work. For example, tell the agent to include a short screen recording after it changes a user-facing flow.

## Automation settings

### Model

You can select which model the cloud agent uses for your automation.

### Repositories

Choose whether the automation needs no repository, one repository, or a multi-repo environment.

The repository setting controls the codebase context for each run:

- **No repository**: The agent does not clone code. Use this for workflows that only need Slack, MCP, webhooks, Linear, or PagerDuty. It cannot edit code or open pull requests.
- **Single repository**: The agent works in one repository and branch. Use this when the automation should read, review, or change code in one codebase.
- **Multi-repo environment**: The agent works across the repositories in an environment. Use this when the task spans multiple codebases.

For certain triggers like Slack or cron schedules, Cursor defaults to not using a repository. If your automation should make code changes, specify which repository or repositories agents should work in.

For source control triggers, specifying a repo or multiple repos is required.

#### Single-repo automations

By default, an automation runs against one repository and branch. This is the right choice when the agent should read, review, or change code in a single codebase.

Source control triggers infer the repository from the pull request. For other triggers, choose the repository and branch in the automation settings.

#### Multi-repo automations

Use a multi-repo environment when an automation needs to work across multiple repositories. Select multiple repos when you configure the environment, or choose an existing one from your [Cloud Agents dashboard](https://cursor.com/dashboard/cloud-agents#environments).

### Permissions

Control who can view and manage the automation. The permission scope also determines how usage is [billed](https://cursor.com/docs/cloud-agent/automations.md#billing).

- **Private**: Only you can manage the automation. Team admins can view and disable the automation.
- **Team Visible**: Only you can manage the automation. Team members can view the automation, and team admins can disable the automation. It still runs with your auth.
- **Team Owned**: Team members can view the automation. Only team admins can manage the automation. It runs with the team's shared automations service account.

Promoting an automation from Private or Team Visible to Team Owned changes the identity it runs as. It stops using your auth and starts using the team's shared automations service account. If the automation uses webhook triggers, regenerate its webhook API key after the scope change. If it uses MCPs or other integrations that rely on personal OAuth credentials, make sure those are configured for the team's service account instead. Only team admins can promote an automation to Team Owned.

### Identity

When an automation acts on external services, it uses the following identities:

- GitHub comments, review approvals, and reviewer requests run as `cursor`.
- Team-scoped automations open pull requests as `cursor`.
- Private automations open pull requests as your GitHub account.
- Slack messages are sent as the Cursor bot.

## Writing prompts

Prompts define what the agent should do. Write them the same way you would write instructions for a cloud agent run.

Tips:

- Be specific about what the agent should check, change, or produce.
- Reference the actions you enabled - you can at-mention tools or informally mention their names.
- Include decision rules for what to do in different cases.
- Set a quality bar for when the agent should open a pull request, comment, or do nothing.
- Describe the output format you want.

## Related

- [Agents Window](https://cursor.com/docs/agent/agents-window.md)
- [Cloud agents overview](https://cursor.com/docs/cloud-agent.md)
- [Cloud agent setup](https://cursor.com/docs/cloud-agent/setup.md)
- [Cloud agent pricing](https://cursor.com/docs/models-and-pricing.md#model-pricing)
- [Skills](https://cursor.com/docs/skills.md)
- [GitHub integration](https://cursor.com/docs/integrations/github.md)
- [GitLab integration](https://cursor.com/docs/integrations/gitlab.md)
- [Slack integration](https://cursor.com/docs/integrations/slack.md)
- [Microsoft Teams integration](https://cursor.com/docs/integrations/microsoft-teams.md)
- [Linear integration](https://cursor.com/docs/integrations/linear.md)
- [Automations help](https://cursor.com/help/ai-features/automations.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
