# Jira

With Cursor's integration for Jira, you can use [Cloud Agents](https://cursor.com/docs/cloud-agent.md) to work on Jira work items by assigning them to Cursor or mentioning `@Cursor` in Jira.

## Get started

### Requirements

Before you install the Jira integration, make sure you have:

- Jira Commercial Cloud with Rovo enabled
- Admin access to the Jira site where you want to install the app
- Cursor admin access to the team you want to connect
- GitHub, GitLab, Azure DevOps, or Bitbucket connected to Cursor for repository access and pull requests

The Cursor Jira integration is currently available only on Cursor Teams and
Enterprise plans.

The Cursor Jira integration is not currently supported in Atlassian HIPAA or
FedRAMP (including Government Cloud) instances.

### Installation

1. As a Cursor admin, go to [Cursor integrations](https://www.cursor.com/dashboard/integrations)

2. Click *Connect* next to Jira

3. Continue to the Cursor app listing in the [Atlassian Marketplace](https://marketplace.atlassian.com/apps/3903220956/cursor)

4. Click *Get it now*

5. Select the Jira site where you want to install Cursor and click *Review*

6. Review the app, then click *Get it now*

7. Once installation completes, you should be dropped into the Cursor Jira app configuration page. Wait a few minutes for Atlassian to notify us of the installation, and then click the *Connect to Cursor* button.

8. Connect the Jira site to your Cursor team

   - If you want to enable user-level authentication (which gives team members more visibility and control over their agents) instead of running agents with a service account, flip the *Require individual authentication* toggle.

9. Complete any remaining Cloud Agent setup in Cursor:

   - Connect GitHub, GitLab, Azure DevOps, or Bitbucket
   - Enable usage-based pricing
   - Confirm privacy settings
   - Choose a default repository, model, and base branch (under Cloud Agents settings in the Cursor Dashboard)

10. Return to Jira and start using Cursor from a work item

    - If you enabled user-level authentication, each user will need to do more set up below

### Authentication mode

You can choose how Jira authenticates Cloud Agents on the Cursor Jira integration admin dashboard by enabling the *Require individual authentication* toggle.

| Mode                           | How it works                                     | Settings used                                                                                                                                              |
| :----------------------------- | :----------------------------------------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Service account authentication | Cloud Agents run under a service account.        | Uses only the team's Cloud Agent settings for routing, models, repositories, and defaults.                                                                 |
| User-level authentication      | Runs all Cloud Agents under each user's account. | Uses each user's Cloud Agent settings for routing, models, repositories, and defaults. Also allows users to find their running agents under their account. |

To connect each user:

1. Kick off an agent on a Jira work item
2. A prompt will appear to connect the account
3. Follow the link to connect the Jira account to a Cursor account associated with the team
4. Complete any remaining Cloud Agent setup in Cursor:
   - Connect GitHub, GitLab, Azure DevOps, or Bitbucket if you haven't connected a repository provider yet
   - Choose a default repository, model, and base branch (under *My Settings* Cloud Agents settings in the Cursor Dashboard)

## How to use

Open a Jira work item and then assign it to Cursor or mention `@Cursor` in a comment. Cursor uses the work item title, description, comments, and available repository settings to start a Cloud Agent.

You can ask Cursor to fix bugs, add features, update tests, or investigate a task described in the work item.

### Delegating work items

Assign a Jira work item to Cursor when the ticket already describes the task clearly.

1. Open the Jira work item
2. Click the assignee field
3. Select Cursor
4. Review the Cloud Agent that starts from the work item

### Mentioning Cursor

Mention `@Cursor` in a Jira comment when you want to add specific instructions. You can include a repository, branch, or model in the same comment.

Examples:

- `@Cursor please investigate this regression`
- `@Cursor repo=acme/backend branch=release fix this before the release cut`
- `@Cursor model=gpt-5.6-sol and update the related tests`

### Follow-up instructions

Open Rovo chat from the Jira work item to continue the conversation with Cursor.

### Status updates and handoff

When a Cloud Agent starts, Jira shows agent status on the work item. Cursor posts progress while it works and returns a summary when the task completes.

If Cursor opens a pull request, the completion update links to the PR for review.

## Configuration

Manage default settings and privacy options from [Dashboard -> Cloud Agents](https://www.cursor.com/dashboard/cloud-agents) under *Team Settings* or *My Settings*.

### Settings

#### Default model

Used when no model is specified in the Jira work item or comment. See [settings](https://www.cursor.com/dashboard/cloud-agents) for available options.

#### Repository selection

Cursor selects the repository based on:

1. **Explicit values**: `repo`, `branch`, or `model` values in the Jira comment or work item
2. **Work item content**: repository names, service names, or keywords in the title, description, and comments
3. **Routing rules**: [custom keyword-to-repository mappings](https://cursor.com/docs/integrations/jira.md#routing-rules)
4. **Recent agent activity**: repositories you've used recently
5. **Default repository**: fallback when no match is found

To use a specific repository, include it in your comment. For example: `@Cursor repo=acme/mobile-app fix the login bug`. Your team service account or user account (depending on the mode you have turned on) *must* have access to this repo or else the attempt to kick off a Cloud Agent will fail.

#### Base branch

Starting branch for Cloud Agent. Leave blank to use the repository's default branch (recommended).

#### Branch prefix

Prefix for branch names created by Cloud Agents.

### Options

Customize Cloud Agent behavior while using mentions with `@Cursor` with these options:

| Option   | Description         | Example             |
| :------- | :------------------ | :------------------ |
| `repo`   | Specify repository  | `repo=acme/web-app` |
| `branch` | Specify base branch | `branch=main`       |
| `model`  | Specify model       | `model=opus`        |

### Routing rules

Routing rules let you define keywords that automatically map to specific repositories. When a Jira work item or comment contains specific keywords, Cursor routes the Cloud Agent to the associated repository.

Routing rules are the way you can tell the agent which projects, work items, key words, and other data should decide which repositories are used for which work items.

#### Setting up routing rules

1. Go to [Dashboard -> Cloud Agents](https://www.cursor.com/dashboard/cloud-agents)
2. Find the **Routing Rules** section
3. Add keyword-to-repository mappings

#### Example rules

| Keyword    | Repository              |
| :--------- | :---------------------- |
| `frontend` | `acme/web-app`          |
| `mobile`   | `acme/mobile-app`       |
| `api`      | `acme/backend-services` |
| `docs`     | `acme/documentation`    |

With these rules configured:

- A work item titled `Fix the frontend nav bug` routes to `acme/web-app`
- A comment saying `@Cursor update the mobile onboarding flow` routes to `acme/mobile-app`
- A comment saying `@Cursor add rate limiting to the api` routes to `acme/backend-services`

#### How routing works

Cursor evaluates Jira work items and comments in this order:

1. **Explicit values**: `repo`, `branch`, or `model` values in the Jira comment or work item
2. **Work item content**: repository names, service names, or keywords in the title, description, and comments
3. **Routing rules**: custom keyword-to-repository mappings
4. **Recent agent activity**: repositories you've used recently
5. **Default repository**: fallback when no match is found

### Privacy

Cloud Agents support Privacy Mode.

Read more about [Privacy Mode](https://www.cursor.com/privacy-overview) or manage your [privacy settings](https://www.cursor.com/dashboard/cloud-agents).

Privacy Mode (Legacy) is not supported. Cloud Agents require temporary code
storage while running.

## Permissions

During installation, Jira shows the permissions requested by the Cursor app. Cursor uses these permissions to:

- Identify the Jira user starting or managing a Cloud Agent
- Read work item fields, descriptions, comments, and related context
- Post status updates, completion summaries, and pull request links
- Receive events when work items are assigned to Cursor or mention `@Cursor`

Review the permission prompt in Atlassian Marketplace before installing the app.

## FAQ

### Which Jira sites are supported?

The Cursor Jira integration supports Atlassian commercial cloud sites with Rovo enabled. Atlassian HIPAA, FedRAMP, and Government Cloud instances are not supported.

### Do I need usage-based billing?

Yes. Cloud Agents require usage-based billing. Enable usage-based billing while completing Cloud Agent setup in Cursor.

### Who can install the Jira integration?

A user that is both a Jira admin and Cursor team admin will need to do the initial setup.

### Do users need to connect their own Cursor accounts?

It depends on the authentication mode you choose. Service account authentication runs all Cloud Agents under a service account and uses team settings. User-level authentication connects each Jira user to Cursor, lets users find their running Cloud Agents from Jira in their Cursor dashboard, and uses each user's settings for routing, models, repositories, and defaults.

### What else needs to be set up before Cursor can create PRs?

Connect GitHub, GitLab, Azure DevOps, or Bitbucket to Cursor and make sure Cloud Agent settings include the repositories, models, and base branches your team wants to use.

### How do users continue a conversation with Cursor?

Open Rovo chat from the Jira work item to continue the conversation with Cursor. Alternately, open the Cloud Agent in Cursor and continue the conversation there.

## Disclaimer

Cursor can make mistakes. Please double-check code and responses.

## Privacy Policy

For information about how Cursor collects, uses, and protects your data, see our [Privacy Policy](https://cursor.com/privacy).


---

## Sitemap

[Overview of all docs pages](/llms.txt)
