# Linear

Use [Cloud Agents](https://cursor.com/docs/cloud-agent.md) directly from Linear by delegating issues to Cursor or mentioning `@Cursor` in comments.

[Media](/docs-static/images/integrations/linear/linear-agent.mp4)

## Get started

### Installation

You must be a Cursor admin to connect the Linear integration. Other team
settings are available to non-admin members.

1. Go to [Cursor integrations](https://www.cursor.com/dashboard/integrations)
2. Click *Connect* next to Linear
3. Connect your Linear workspace and select team
4. Click *Authorize*
5. Complete any remaining Cloud Agent setup in Cursor:
   - Connect a repository provider and select a default repository
   - Enable usage-based pricing
   - Confirm privacy settings

### Account linking

First use prompts account linking between Cursor and Linear. A repository provider connection is required for PR creation.

## How to use

Delegate issues to Cursor or mention `@Cursor` in comments. Cursor analyzes issues and filters out non-development work automatically.

### Delegating issues

1. Open Linear issue
2. Click assignee field
3. Select "Cursor"

![Delegating an issue to Cursor in Linear](/docs-static/images/integrations/linear/linear-delegate.png)

### Mentioning Cursor

Mention `@Cursor` in a comment to assign a new agent or provide additional instructions, for example: `@Cursor fix the authentication bug described above`.

## Workflow

Cloud Agents show real-time status in Linear and create PRs automatically when complete. Track progress in [Cursor dashboard](https://www.cursor.com/dashboard/cloud-agents).

![Cloud Agent status updates in Linear](/docs-static/images/integrations/linear/linear-activity.png)

### Follow-up instructions

You can respond in the agent session and it'll get sent as a follow-up to the agent. Simply mention `@Cursor` in a Linear comment to provide additional guidance to a running Cloud Agent.

## Configuration

Configure Cloud Agent settings from [Dashboard → Cloud Agents](https://www.cursor.com/dashboard/cloud-agents).

| Setting                | Location         | Description                                               |
| :--------------------- | :--------------- | :-------------------------------------------------------- |
| **Default Repository** | Cursor Dashboard | Primary repository when no project repository configured  |
| **Default Model**      | Cursor Dashboard | AI model for Cloud Agents                                 |
| **Base Branch**        | Cursor Dashboard | Branch to create PRs from (typically `main` or `develop`) |

### Configuration options

You can configure Cloud Agent behavior using several methods:

**Issue description or comments**: Use `[key=value]` syntax, for example:

- `@cursor please fix [repo=anysphere/everysphere]`
- `@cursor implement feature [model=claude-3.5-sonnet] [branch=feature-branch]`

**Issue labels**: Use parent-child label structure where the parent label is the configuration key and the child label is the value.

**Project labels**: Same parent-child structure as issue labels, applied at the project level.

Supported configuration keys:

- `repo`: Specify target repository (e.g., `owner/repository`)
- `branch`: Specify base branch for PR creation
- `model`: Specify AI model to use

### Repository selection

Cursor determines which repository to work on using this priority order:

1. **Issue description/comments**: `[repo=owner/repository]` syntax in issue text or comments
2. **Issue labels**: Repository labels attached to the specific Linear issue
3. **Project labels**: Repository labels attached to the Linear project
4. **Default repository**: Repository specified in Cursor dashboard settings

#### Setting up repository labels

To create repository labels in Linear:

1. Go to **Settings** in your Linear workspace
2. Click **Labels**
3. Click **New group**
4. Name the group "repo" (case insensitive - must be exactly "repo", not "Repository" or other variations)
5. Within that group, create labels for each repository using the format `owner/repo`

These labels can then be assigned to issues or projects to specify which repository the Cloud Agent should work on.

![Configuring repository labels in Linear](/docs-static/images/integrations/linear/linear-project-labels.png)

## Advanced features

### Triage rules (Advanced)

Set up automation rules in Linear to automatically delegate issues to Cursor:

1. Go to Linear project settings
2. Navigate to triage rules
3. Create rules that automatically:
   - Add specific labels
   - Assign issues to Cursor
   - Trigger Cloud Agents based on conditions

Triage rules are an advanced feature with some current limitations. Linear
requires a human assignee for rules to fire, though this requirement may be
removed in future updates.

### Getting help

Check [agent activity](https://www.cursor.com/dashboard/cloud-agents) and include request IDs when contacting support.

## Feedback

Share feedback through Linear comments or your Cursor dashboard support channels.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
