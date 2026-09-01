# Notion

Connect Cursor to Notion so you can delegate specs, tasks, and comments to agents. Because the integration is built on the Cursor SDK, every agent uses the same runtime, harness, and models that power Cursor.

Setup, chat, @mentions, and task assignment happen in Notion. Cursor runs the agent in a secure, sandboxed [Cloud Agent](https://cursor.com/docs/cloud-agent.md) VM, so it keeps working in the background while you handle other work.

See Notion's full docs [here](https://www.notion.com/help/connect-cursor-to-notion).

Cursor in Notion is in beta and is available to all Cursor users with Notion Business or Enterprise plans.

## Get started

### Requirements

Before you connect Cursor in Notion, make sure you have:

- GitHub connected to your Cursor account, used for repository access and pull requests
- A Cursor User API Key, which you create during setup

Cursor usage in Notion is billed through your existing account and is available at no extra cost.

### Installation

1. In Notion, go to Agents in the sidebar → New Agent → Cursor.
2. Pick a starting point for your agent:

- Start with a default template: Templates for common tasks (like Code Q\&A and Bug Triage) help you customize Cursor for specific workflows.
- Start from scratch: Set up your own instructions, triggers, and connections with the standard Cursor harness.

3. Create or take your Cursor API key from your [dashboard](https://cursor.com/dashboard/api) and enter it into Notion.
4. Add the pages, databases, and Notion spaces your agent needs, such as your engineering task database and spec pages.
5. Save the agent and start working.

Cursor is now connected and your agent can write code and open pull requests from tasks or agent chat.

### Permissions

1. The connection is tied to your personal API key. This means Cursor access and activity are linked to your individual Cursor account, not the whole workspace. Learn more →
2. Repos, environment, and connections come from your Cursor agent setup. Configure these in your Cursor dashboard.
3. Each Cursor agent can only access content you have shared in Notion. Permissions are set per agent in Notion and are not inherited from whoever starts a run.

## How to use

Bring Cursor to your task boards, spec docs, and other context that already live in Notion.

If you try to use Cursor in a workspace where it does not have permissions, you should set a pop up to give it access.

### Mention the agent

Start from a spec doc or page. Leave a comment and @-mention Cursor. Ask it to build the spec, refine the plan, or answer a question.

Cursor replies in comments. Click **View chat** to open the full conversation.

The first time you mention the agent on a page, approve page access when prompted.

Example prompts:

- "Make a plan for this spec. Wait for approval."
- "Review this doc and flag anything unclear."
- "Turn this into a checklist of concrete tasks and then open PRs for each."

### Assign a task

On a task database or board, assign a task to Cursor. The agent will:

- Update task status as it works
- Post progress as comments
- Link to any pages or PRs it creates

## Troubleshooting

### The agent can't access a page or database

- Confirm the page or database is listed in **Tools & access**, not only referenced in instructions.
- Confirm the page isn't restricted to a group the agent doesn't belong to.
- If the page is private, share it with the agent directly.

### GitHub connection fails or the agent can't push

- Confirm the token has **Contents: read/write** and **Pull requests: read/write** scopes.
- Check whether SSO or org approval is required in your GitHub organization.
- Confirm the default repository in your setup is correct.
- Regenerate the token if it expired, then reconnect.

### The agent didn't fire when I @mentioned it

- Confirm the **agent mentioned** trigger is on.
- Mention the agent in a **comment** rather than in page body text. Comments are most reliable.
- Confirm the agent has access to the page you mentioned it on.

## FAQ

### How does billing work?

Cursor usage in Notion is billed through your existing Cursor account, the same way [Cloud Agent](https://cursor.com/docs/cloud-agent.md) usage is billed. It's available to all Cursor users at no extra cost. You'll also need a Notion Business or Enterprise plan.

### Where can I view Cursor activity?

Results appear in Notion on the page or task where the agent ran. You can also find your chat sessions in the agent view in Notion, or in Cursor.

### Do Cursor runs kicked off from Notion respect zero data retention (ZDR)?

Yes. Agent runs respect your existing privacy settings in Cursor.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
