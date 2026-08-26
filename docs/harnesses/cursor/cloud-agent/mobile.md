# Cursor for iOS

### Beta

Cursor for iOS is in beta. Features may change before general availability.

Cursor for iOS is a native mobile app for controlling agents running [in the cloud](https://cursor.com/docs/cloud-agent.md) and on your local computer. Start agents, follow their work in real time, and review and merge their pull requests from your iPhone or iPad. It runs on the same backend as [cursor.com/agents](https://cursor.com/agents) and the desktop Agents Window, so the agents you start on mobile show up everywhere you work.

[https://apps.apple.com/app/cursor/id6767085653](https://apps.apple.com/app/cursor/id6767085653)

The app runs on iPhone with iOS 26.0 or later and iPad with iPadOS 26.0 or later, in English.
Android is planned.

Cursor for iOS is available on the Start, Pro, Pro+, Ultra, Teams, and Enterprise plans. Learn
more about [what's included](https://cursor.com/help/account-and-billing/pricing.md).

## Getting started

### Download the app

Get [Cursor from the App Store](https://apps.apple.com/app/cursor/id6767085653) on your
iPhone or iPad.

### Open Cursor and sign in

Sign in with your Cursor account. If your organization requires SSO, you'll
sign in through it.

### Choose a repository

Pick the repository and branch you want Cursor to work in.

### Start directing agents

Send a task and supervise the agent as it works.

Once you're set up, put an agent to work. For example:

- Fix a bug or respond to an incident while you're away from your desk.
- Review and merge a teammate's pull request from your phone.
- Kick off a refactor or a new feature, then check back when the agent is done.
- Ask an agent to investigate a failing CI check.

## What you can do

You get the full cloud agent workflow from your pocket, not a stripped-down chat box. The same machines, models, and review tools you use on the web come with you.

- **Run agents on cloud machines.** Pick a worker for each run: a Cloud machine, a [Self-Hosted Pool](https://cursor.com/docs/cloud-agent/self-hosted-guides/pool.md), or one of [My Machines](https://cursor.com/docs/cloud-agent/self-hosted-guides/my-machines.md). Agents work in full development environments, so they install dependencies, run tests, and verify their changes.
- **Use any model.** Choose any model available for cloud agents. Every run uses the model's maximum supported context window.
- **Let agents run long.** Start a task, lock your phone, and check back later. Agents keep working in the cloud whether or not your device stays connected.
- **Follow the work live.** Watch the chat stream as the agent codes, send follow-ups to a running agent, and tap a subagent card to read its child transcript.
- **Review and merge pull requests.** Read full diffs, commits, deployments, approvals, comments, and checks. Add or change reviewers, then ask an agent to resolve review comments or fix a failing check. You can also merge with squash, mark ready, update the branch, toggle auto-merge, publish, or close.
- **Use your commands and automations.** Your slash commands, skills, and automations work the same on mobile as they do locally, in the CLI, and on the web.
- **Enter Design Mode.** Attach photos, camera shots, or files, then point, click, and draw on images or front-end components to give agents visual direction.
- **Use your voice.** Dictate instructions to agents for hands-free edits with live transcription.
- **Connect MCP tools.** Choose the [MCP](https://cursor.com/docs/mcp.md) servers a run should use at launch, including the Slack MCP, so agents reach the same external tools they use on the web.
- **Stay in the loop.** Get a push notification when an agent finishes a turn, and track up to eight agents at once with Live Activities on the lock screen and Dynamic Island.

The app is cache-first. It reads from local data so the inbox and conversations open fast, then syncs once your connection returns.

## Use the iPad layout

The iPad app uses the larger screen to keep more of an agent's work visible:

- Keep chats in the sidebar while you follow several agents.
- Open a review next to a chat.
- Read file diffs at full width.
- Attach a screenshot and tap a point to leave visual direction, or draw on the image with Apple Pencil.

## What lives on the web

The app focuses on directing and reviewing agents. It isn't an IDE, and it isn't an admin console. A few things stay on [cursor.com/agents](https://cursor.com/agents) or the [Cursor Dashboard](https://cursor.com/dashboard):

- **Editor, terminal, and file browser.** On mobile you see changed files in the diff view, not a full workspace.
- **Secrets and environments.** Configure [cloud agent environments](https://cursor.com/docs/cloud-agent/setup.md) and secrets on the web. Agents on mobile use what's already set up.
- **MCP server management.** Pick servers per run on mobile; add and manage them on the web.
- **Source control setup.** Connect or reconnect [GitHub](https://cursor.com/docs/integrations/github.md) and [GitLab](https://cursor.com/docs/integrations/gitlab.md) from the dashboard.
- **Automations, rules, and skills config.** Manage these on the web. Agents pick up whatever the repo already contains.
- **Admin, billing, and usage.** Web only.

## Move between devices

Agents follow you across surfaces:

- **Desktop or web to mobile.** Agents you start anywhere appear in the mobile inbox automatically. From a local IDE session, push your work to a cloud agent first with Move to Cloud, or keep it on your computer and direct it from your phone with [Remote Control](https://cursor.com/docs/cloud-agent/mobile.md#remote-control).
- **Mobile to desktop.** Any agent you start on mobile shows up in the desktop Cloud Agents panel and at [cursor.com/agents](https://cursor.com/agents). Open it and keep going.
- **Direct your computer from your phone.** Hand a session running on your computer to the cloud with [Remote Control](https://cursor.com/docs/cloud-agent/mobile.md#remote-control), then keep directing it from your phone. The agent loop runs in the cloud while terminal commands, file edits, and tests run on your computer.

Agents started on mobile are tagged with `source: iosApp` so you can tell where they came from.

## Remote Control

Remote Control lets you take an agent you're running on your computer and keep directing it from your phone. The agent loop moves to the cloud while its tools keep running on your machine, so it reads your files, runs your tests, and uses your local setup the same way it did on your desktop.

Remote Control and its settings are only available in the [Agents Window](https://cursor.com/docs/agent/agents-window.md).

### Before you start

- **Use Cursor 3.9.8 or later.** Remote Control requires Cursor client version 3.9.8 or later on your computer. Older clients won't show the Remote Control setting under **Settings > Agents** or the `/remote-control` command.
- **Use a supported account.** Remote Control is available on Pro, Pro+, Ultra, Teams, and Enterprise plans for users with Cloud Agents access. See [what's included](https://cursor.com/help/account-and-billing/pricing.md).
- **Enable Remote Control in Cursor.** In the Agents Window, turn it on under **Settings > Agents** before handing off a session.
- **Enable it for your team.** On Teams and Enterprise plans, an admin must enable Remote Control from [Cursor Dashboard → Cloud Agents → Self-Hosted](https://cursor.com/dashboard/cloud-agents#self-hosted) before members can use it.
- **Allow cloud data storage.** Remote Control isn't available when your privacy settings disable cloud data storage.
- **Use a Git-backed workspace.** The workspace must have a Git remote. Local and Remote SSH workspaces are supported.
- **Keep your computer available.** Your computer must stay awake and online because tool calls run on it. You can turn on **Keep this computer awake** under **Settings > Agents** to prevent sleep while the computer is plugged in.

### Hand off a session

### Run /remote-control

In the agent's input on your computer, run `/remote-control`, then send your
next message. Cursor hands the session to a worker on your machine and makes
it controllable from your other devices.

### Open the agent on your phone

The session shows up in the Cursor app inbox alongside your other desktop
agents. Tap in to pick up where you left off.

### Keep directing it

Send follow-ups, watch the work stream live, and review the results from
your phone. Your computer keeps running the work.

### How your code stays on your machine

Remote Control automatically manages a worker on your computer, with no separate setup required. The agent loop runs in Cursor's cloud, and every tool call (terminal commands, file edits, tests, and git) runs on your computer.

- Your repository, secrets, credentials, and build caches stay on your machine. Only tool results and the context the model needs cross to Cursor.
- Only you can control your agents. Cursor ties each session to your account and your machine, and rejects requests for agents you don't own.
- Cursor sends the conversation state and model context needed to continue the session when you enable Remote Control for the agent.

For the trust boundaries that apply when tool calls run on your computer, see [Security and network](https://cursor.com/docs/cloud-agent/security-network.md).

### Current limitations

- **Git-backed workspaces only.** Remote Control requires a workspace with a Git remote.
- **Your computer must stay available.** Tool calls can't run while your computer is asleep or offline.

### Team controls

Team and Enterprise admins control Remote Control access from [Cursor Dashboard → Cloud Agents → Self-Hosted](https://cursor.com/dashboard/cloud-agents#self-hosted). When an admin enables Remote Control, Cursor also enables the team's self-hosted worker access. When it's off, members can't enable Remote Control or hand local sessions to their other devices.

## Availability

You can use the app on any plan that includes cloud agents: Pro, Pro+, Ultra, Teams, and Enterprise. If your organization requires SSO, you'll sign in through it first.

For HIPAA BAA details, including Eligible Services and implementation requirements, see [HIPAA Business Associate Agreements](https://cursor.com/docs/enterprise/baa.md).

Cursor for iOS relies on [Cloud Agents](https://cursor.com/docs/cloud-agent.md), which need cloud data storage to run. If you're on Privacy Mode (Legacy), switch to Privacy Mode before using the app. You won't be able to start agents on mobile until you do. We never train on your code and only retain code for running the agent. [Learn more about Privacy mode](https://www.cursor.com/privacy-overview).

When you try to use Cloud Agents, Cursor prompts you to opt in to Privacy Mode. Tap **Update**, then confirm **Switch to Privacy Mode** in the dialog. Your code still won't be used for training, but you can't switch back to Legacy Privacy Mode afterward. See [Security and network](https://cursor.com/docs/cloud-agent/security-network.md) for how Cloud Agents store and retain data.

## Related pages

- [Cloud agents overview](https://cursor.com/docs/cloud-agent.md)
- [Cloud agent capabilities](https://cursor.com/docs/cloud-agent/capabilities.md)
- [Cloud agent setup](https://cursor.com/docs/cloud-agent/setup.md)
- [Security and network](https://cursor.com/docs/cloud-agent/security-network.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
