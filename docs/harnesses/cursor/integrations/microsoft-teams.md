# Microsoft Teams

With Cursor's integration for Microsoft Teams, you can use [Cloud Agents](https://cursor.com/docs/cloud-agent.md) to work on tasks directly from Microsoft Teams by mentioning `@Cursor` with a prompt.

## Get started

### Installation

1. Go to [Cursor integrations](https://www.cursor.com/dashboard/integrations)

2. Click *Connect* next to Microsoft Teams or go to the [Microsoft Marketplace listing](https://marketplace.microsoft.com/en-us/product/WA200010720)

3. Install the Cursor app in your Microsoft Teams workspace

4. After installing in Microsoft Teams, you'll be redirected back to Cursor to finalize setup

   1. Connect GitHub, GitLab, Azure DevOps, or Bitbucket, if you haven't connected a repository provider yet
   2. Enable usage-based pricing
   3. Confirm privacy settings

5. Start using Cloud Agents in Microsoft Teams by mentioning `@Cursor`

## How to use

Mention `@Cursor` and give your prompt. Cursor automatically picks the right repository, model, base branch, or named [cloud agent environment](https://cursor.com/docs/cloud-agent/setup.md) based on your message, the thread context, and your recent agent activity.

To use a specific repository, include its name in your message:

- `@Cursor in cursor-app, fix the login bug`
- `@Cursor fix the auth issue in backend-api`

To use a specific model, mention it in your message:

- `@Cursor with opus, fix the login bug`
- `@Cursor use gpt-5.2 to refactor the auth module`

To use a named environment, mention it in your message or add `env=<name>`:

- `@Cursor use the Platform environment to update the shared API`
- `@Cursor env=Platform update the shared API`
- `@Cursor env="Platform Services" update the shared API`

### Commands

Run `@Cursor help` for an up-to-date command list.

| Command                      | Description                                                                               |
| :--------------------------- | :---------------------------------------------------------------------------------------- |
| `@Cursor [prompt]`           | Start a Cloud Agent. In channel threads with existing agents, adds follow-up instructions |
| `@Cursor help`               | Show setup and usage help                                                                 |
| `@Cursor unlink`             | Disconnect your Cursor account from Microsoft Teams                                       |
| `@Cursor disconnect`         | Disconnect your Cursor account from Microsoft Teams                                       |
| `@Cursor [options] [prompt]` | Set the repository, environment, branch, or model for a run                               |

#### Options

Customize Cloud Agent behavior with these options:

| Option                | Description                                                                         | Natural language example       | Inline example            |
| :-------------------- | :---------------------------------------------------------------------------------- | :----------------------------- | :------------------------ |
| `repo`                | Use a specific repository                                                           | `in acme/web-app`              | `repo=acme/web-app`       |
| `env` / `environment` | Use a named [cloud agent environment](https://cursor.com/docs/cloud-agent/setup.md) | `use the Platform environment` | `env="Platform Services"` |
| `branch`              | Use a specific base branch                                                          | `work from the main branch`    | `branch=main`             |
| `model`               | Use a specific model                                                                | `with opus`                    | `model=opus`              |

#### Syntax formats

Natural:

```bash
@Cursor with opus, fix the login bug in backend-api
```

Inline:

```bash
@Cursor env="Platform Services" branch=dev model=opus Fix the login bug
```

#### Option precedence

When combining options:

- **Explicit values** override defaults
- **Inline options** override model and repository values inferred from your message
- **Dashboard settings** apply when no value is specified or inferred
- **`env`** takes precedence over `repo` when both are present

The bot parses options from anywhere in the message, allowing natural command writing.

#### Using thread context

Cloud Agents understand and use context from existing Microsoft Teams discussions. This is useful when your team discusses an issue and you want the agent to make the code change based on that conversation.

Cloud Agents read the relevant thread or chat context when invoked,
understanding and acting on your team's discussion.

#### Follow-up instructions

In channel threads, reply in the agent's thread with another `@Cursor` mention to add follow-up instructions.

In personal chats and group chats, continue the conversation from Cursor using *Open in Web* or *Open in Desktop*.

### Status updates & handoff

When Cloud Agent starts, Microsoft Teams shows a launch card with the selected repository, model, and branch. The card includes options to *Open in Web*, *Open in Desktop*, and *Switch repository*.

When Cloud Agent completes, you get a Microsoft Teams notification with the result. If the agent opened a pull request, the card includes an option to view the PR.

### Managing agents

Manage Cloud Agents using actions on the Microsoft Teams cards.

Available options:

- **Add follow-up**: Add instructions to an existing agent from a channel thread
- **Switch repository**: Relaunch the same request against a different repository
- **Delete**: Stop and archive the Cloud Agent
- **Open in Web**: Continue in the web interface
- **Open in Desktop**: Continue in Cursor
- **Update settings**: Manage your Cloud Agent defaults
- **Give feedback**: Send feedback about agent performance

## Configuration

Manage default settings and privacy options from [Dashboard -> Cloud Agents](https://www.cursor.com/dashboard/cloud-agents).

### Settings

#### Default model

Used when no model is specified in your message. See [settings](https://www.cursor.com/dashboard/cloud-agents) for available options.

#### Repository selection

Cursor automatically selects the right repository based on:

1. **Your message content**: repository names or keywords in your prompt
2. **Recent agent activity**: repositories you've used recently
3. **Default repository**: fallback when no match is found

To use a specific repository, include its name in your message. For example: `@Cursor in mobile-app, fix the login bug`.

#### Base branch

Starting branch for Cloud Agent. Leave blank to use the repository's default branch, often `main`.

### Routing behavior

Cursor evaluates your Microsoft Teams message in this order:

1. **Explicit options**: `repo`, `env`, `branch`, and `model` values in your prompt
2. **Your message content**: repository names, model names, or branch names in your prompt
3. **Recent agent activity**: repositories you've used recently
4. **Default repository**: fallback when no match is found

### Privacy

Cloud Agents support Privacy Mode.

Read more about [Privacy Mode](https://www.cursor.com/privacy-overview) or manage your [privacy settings](https://www.cursor.com/dashboard/cloud-agents).

Privacy Mode (Legacy) is not supported. Cloud Agents require temporary
code storage while running.

#### Display agent summary

Display agent summaries and diff images. They may contain file paths or code snippets. You can turn this on or off.

## Permissions

Cursor requests these Microsoft Teams permissions for Cloud Agents to work in your workspace:

| Permission                   | Description                                                         |
| :--------------------------- | :------------------------------------------------------------------ |
| `identity`                   | Identifies the Microsoft Teams user starting or managing an agent   |
| `messageTeamMembers`         | Sends direct messages for setup, account linking, and notifications |
| `ChannelMessage.Read.Group`  | Reads channel messages and replies for thread context               |
| `ChatMessage.Read.Chat`      | Reads personal and group chat messages for conversation context     |
| `ChannelSettings.Read.Group` | Reads channel metadata, including channel names and descriptions    |
| `TeamSettings.Read.Group`    | Reads team metadata, including team names and descriptions          |

The Cursor app supports personal chats, team channels, and group chats in Microsoft Teams.

## Disclaimer

Cursor can make mistakes. Please double-check code and responses.

## Privacy Policy

For information about how Cursor collects, uses, and protects your data, see our [Privacy Policy](https://cursor.com/privacy).


---

## Sitemap

[Overview of all docs pages](/llms.txt)
