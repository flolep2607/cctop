# Bugbot

Bugbot reviews pull requests and identifies bugs, security issues, and code quality problems.

Configure Bugbot in [Automations](https://cursor.com/automations/from-cursor/bugbot).

[Bugbot leaving comments on a PR](/docs-static/images/bugbot/bugbot-report-cropped.mp4)

## How it works

Bugbot analyzes PR diffs and leaves comments with explanations and fix suggestions. It runs automatically on each PR update or manually when triggered.

- Runs **automatic reviews** on every PR update
- **Manual trigger** by commenting `cursor review` or `bugbot run` on any PR
- **Uses existing PR comments as context**: reads connected PR comments (top-level and inline) to avoid duplicate suggestions and build on prior feedback
- **Fix in Cursor** links open issues directly in Cursor
- **Fix in Web** links open issues directly in [cursor.com/agents](https://cursor.com/agents)

## Setup

Connect your repositories through the Cursor dashboard to start using Bugbot.

- **GitHub** (including GitHub Enterprise Server): See the [GitHub integration page](https://cursor.com/docs/integrations/github.md)
- **GitLab** (including GitLab Self-Hosted): See the [GitLab integration page](https://cursor.com/docs/integrations/gitlab.md)
- **Bitbucket** (including Bitbucket Data Center): See the [Bitbucket integration page](https://cursor.com/docs/integrations/bitbucket.md)
- **Azure DevOps** (Azure DevOps Services, limited availability): See the [Azure DevOps integration page](https://cursor.com/docs/integrations/azure-devops.md#set-up-bugbot)

After connecting, open [Bugbot in Automations](https://cursor.com/automations/from-cursor/bugbot) to enable it on specific repositories.

## CI check statuses

Bugbot publishes a status for each review run. On GitHub, this appears as a check named `Cursor Bugbot`. On Bitbucket, this appears as a build status with the key `cursor-bugbot`. On Azure DevOps, this appears as a status with the context `cursor-bugbot/review`. The status uses these conclusions:

- `success`: Bugbot found no issues, and there are no unresolved Bugbot comments from earlier runs.
- `neutral`: Bugbot found issues, the run was cancelled by a newer commit, or Bugbot hit an internal error. This is the default conclusion when Bugbot reports findings.
- `failure`: Bugbot found issues and the check is configured to fail on unresolved issues.

If you use branch protection, require the Bugbot check or build status to make sure Bugbot runs before merge. Requiring the status alone does not block merges on findings because findings default to `neutral`. If fail-on-unresolved-issues behavior is available for your organization, enable it to make unresolved findings produce a failing status. Bugbot does not emit a `skipped` conclusion.

When Bugbot Autofix is enabled, GitHub may also show a separate `Cursor Bugbot Autofix` check. That check only uses `success` or `neutral`.

## Configuration

### Individual

### Repository settings

Enable or disable Bugbot per repository from your installations list. Bugbot runs only on PRs you author.

### Personal settings

- Run **only when mentioned** by commenting `cursor review` or `bugbot run`
- Run **only once** per PR, skipping subsequent commits

### Team

### Repository settings

Team users and admins can enable Bugbot per repository, configure allow/deny lists for reviewers, and set:

- Run **only once** per PR per installation, skipping subsequent commits

Bugbot runs for all contributors to enabled repositories, regardless of team membership.

### Personal settings

Team members can override settings for their own PRs:

- Run **only when mentioned** by commenting `cursor review` or `bugbot run`
- Run **only once** per PR, skipping subsequent commits
- **Enable reviews on draft PRs** to include draft pull requests in automatic reviews

### Enterprise

### Repository settings

Enterprise admins can enable Bugbot per repository, configure allow/deny lists for reviewers, and set:

- Run **only once** per PR per installation, skipping subsequent commits

Bugbot runs for all contributors to enabled repositories, regardless of team membership.

### Personal settings

Enterprise users can override settings for their own PRs:

- Run **only when mentioned** by commenting `cursor review` or `bugbot run`
- Run **only once** per PR, skipping subsequent commits
- **Enable reviews on draft PRs** to include draft pull requests in automatic reviews

## Analytics

Open [Bugbot in Automations](https://cursor.com/automations/from-cursor/bugbot) to view review activity and outcomes.

## API

Enterprise teams can use the Bugbot API to trigger reviews and retrieve per-review analytics. Create an API key from [Cursor Dashboard → API Keys](https://cursor.com/dashboard/api) and authenticate with [Basic Authentication](https://cursor.com/docs/api.md#basic-authentication).

### Trigger a review

/bugbot/review

Queue a Bugbot review for a pull request or merge request. The request returns when the review is queued; the review runs asynchronously.

Requires an API key with `admin:*` scope. The endpoint is limited to 30 requests per minute per team.

Set `dryRun` to `true` to run the full analysis pipeline without posting review comments, inline comments, checks, or other SCM side effects. Dry-run reviews still persist findings and are billed like normal reviews. Retrieve them with `GET /analytics/team/bugbot-reviews`. Dry-run requests have an additional limit of 10 requests per minute per team.

#### Request Body

`prUrl` string (required)

Full GitHub pull request or GitLab merge request URL.

`dryRun` boolean (optional)

When `true`, run analysis and persist findings without posting anything to the SCM provider. Default: `false`.

```bash
curl --request POST \
  --url https://api.cursor.com/bugbot/review \
  -u YOUR_API_KEY: \
  --header 'Content-Type: application/json' \
  --data '{
    "prUrl": "https://github.com/your-org/your-repo/pull/42"
  }'
```

```bash
curl --request POST \
  --url https://api.cursor.com/bugbot/review \
  -u YOUR_API_KEY: \
  --header 'Content-Type: application/json' \
  --data '{
    "prUrl": "https://github.com/your-org/your-repo/pull/42",
    "dryRun": true
  }'
```

**Response:**

```json
{
  "outcome": "success",
  "message": "Bugbot review queued",
  "request_id": "6e0d261c-86a2-4383-89f0-9162c1c10662",
  "dry_run": false
}
```

A dry-run response uses `"message": "Bugbot dry-run review queued"` and `"dry_run": true`.

Save `request_id` so you can match the completed review in the analytics endpoint.

If Bugbot cannot review the pull request, the endpoint returns `400 Bad Request` with the reason:

```json
{
  "outcome": "error",
  "message": "Bugbot is disabled for this repository"
}
```

### Review analytics

/analytics/team/bugbot-reviews

Return one item per completed Bugbot review, including the reviewed commit, findings count, billed cost, and per-finding resolution data.

Includes both posted reviews and dry-run reviews. Posted findings are identified by `comment_id` and `resolution_status`. Dry-run findings return `title`, `description`, and `locations` instead because nothing is posted to the SCM.

Requires an API key with `read:*` scope.

#### Query Parameters

`startDate` string (optional)

Start of the analytics range. Defaults to 7 days ago. See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats).

`endDate` string (optional)

End of the analytics range. Defaults to now. See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats).

`repo` string (optional)

Repository filter in `host/owner/repo` form. Protocol and `.git` suffix are optional.

`prNumber` number (optional)

Pull request or merge request number.

`page` number (optional)

Page number for pagination. Default: `1`.

`pageSize` number (optional)

Number of reviews per page. Default: `100`, max: `250`.

`dryRun` boolean (optional)

Filter to dry-run (`true`) or posted (`false`) reviews only.

```bash
curl --get https://api.cursor.com/analytics/team/bugbot-reviews \
  -u YOUR_API_KEY: \
  --data-urlencode 'startDate=2026-06-01' \
  --data-urlencode 'endDate=2026-06-29' \
  --data-urlencode 'repo=github.com/your-org/your-repo' \
  --data-urlencode 'prNumber=42' \
  --data-urlencode 'page=1' \
  --data-urlencode 'pageSize=100'
```

```bash
curl --get https://api.cursor.com/analytics/team/bugbot-reviews \
  -u YOUR_API_KEY: \
  --data-urlencode 'dryRun=true' \
  --data-urlencode 'repo=github.com/your-org/your-repo' \
  --data-urlencode 'prNumber=42'
```

**Response (posted review):**

```json
{
  "data": [
    {
      "request_id": "6e0d261c-86a2-4383-89f0-9162c1c10662",
      "timestamp": "2026-06-29T19:42:18.000Z",
      "repo": "github.com/your-org/your-repo",
      "repo_node_id": "R_kgDOABCDEF",
      "pr_number": 42,
      "commit_sha": "9f3c2a1b7d8e4f5061728394a5b6c7d8e9f0a1b2",
      "bugs_found": 2,
      "cost_cents": 42.5,
      "dry_run": false,
      "publication_status": "posted",
      "bugs": [
        {
          "comment_id": "2147483999",
          "resolution_status": "resolved",
          "severity": "high"
        },
        {
          "comment_id": "2147484000",
          "resolution_status": "unresolved",
          "severity": "medium"
        }
      ]
    }
  ],
  "pagination": {
    "page": 1,
    "pageSize": 100,
    "totalItems": 1,
    "totalPages": 1,
    "hasNextPage": false,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "bugbot-reviews",
    "teamId": 12345,
    "startDate": "2026-06-01",
    "endDate": "2026-06-29",
    "repo": "github.com/your-org/your-repo",
    "prNumber": 42,
    "page": 1,
    "pageSize": 100
  }
}
```

**Response (dry-run review):**

```json
{
  "data": [
    {
      "request_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
      "timestamp": "2026-06-29T20:15:03.000Z",
      "repo": "github.com/your-org/your-repo",
      "repo_node_id": "R_kgDOABCDEF",
      "pr_number": 42,
      "commit_sha": "9f3c2a1b7d8e4f5061728394a5b6c7d8e9f0a1b2",
      "bugs_found": 1,
      "cost_cents": null,
      "dry_run": true,
      "publication_status": "dry_run",
      "bugs": [
        {
          "comment_id": null,
          "resolution_status": null,
          "severity": "medium",
          "title": "Unbounded retry loop",
          "description": "retry() recurses without a ceiling.",
          "locations": [
            { "file": "src/net.ts", "start_line": 5, "end_line": 9 }
          ]
        }
      ]
    }
  ],
  "pagination": {
    "page": 1,
    "pageSize": 100,
    "totalItems": 1,
    "totalPages": 1,
    "hasNextPage": false,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "bugbot-reviews",
    "teamId": 12345,
    "startDate": "2026-06-01",
    "endDate": "2026-06-29",
    "repo": "github.com/your-org/your-repo",
    "prNumber": 42,
    "dryRun": true,
    "page": 1,
    "pageSize": 100
  }
}
```

`repo_node_id`, `pr_number`, `commit_sha`, `cost_cents`, `bugs[].comment_id`, `bugs[].resolution_status`, and `bugs[].severity` may be `null` when unavailable. `cost_cents` is `null` when the review is not billed separately. For dry-run reviews, `bugs[].title`, `bugs[].description`, and `bugs[].locations` carry the finding content. Dry-run findings have `comment_id: null` and `resolution_status: null` because nothing is posted to the SCM.

### Trigger and retrieve a review

1. Call `POST /bugbot/review` with the pull request URL. Pass `"dryRun": true` to analyze without posting to the SCM.
2. Save the returned `request_id`.
3. Poll `GET /analytics/team/bugbot-reviews`, filtering by `repo` and `prNumber`. Use `dryRun=true` when you triggered a dry-run review.
4. Find the item whose `request_id` matches the trigger response.

Analytics may take a short time to become available after a review is queued.

## Incremental reviews

By default, Bugbot reviews the full pull request diff on every push. Turn on **Incremental Review** in [Bugbot Automations](https://cursor.com/automations/from-cursor/bugbot) to review only the changes since the previous Bugbot review.

![Incremental Review setting in Bugbot Automations](/docs-static/images/bugbot/incremental-review-setting.png)

## Effort Levels

Effort levels control how much time Bugbot spends reasoning during a review. Higher effort levels can find more bugs, but each review may take longer and take up more usage.

Choose from these effort levels:

- **Default**: Optimizes for efficiency and speed. Reviews are less expensive, but Bugbot may find fewer bugs.
- **High**: Spends more time reasoning. Reviews are more expensive and take longer, but Bugbot may find more bugs.
- **Custom**: Lets you describe when Bugbot should use longer and deeper reviews. Cursor dynamically sets effort levels based on your instructions.

Effort levels are available only for usage-based Bugbot plans.

## Rules

Guide reviews with team rules, repository rules, and project `.cursor/BUGBOT.md` files.

### Team rules

Team admins can create rules in [Bugbot Automations](https://cursor.com/automations/from-cursor/bugbot) that apply to all repositories in the team. These rules are available to every enabled repository, making it easy to enforce organization-wide standards.

When Team Rules, repository rules, and project rule files all apply, Bugbot merges them into one review-rules block. Order of inclusion: Team Rules → project .cursor/BUGBOT.md (including nested files) → learned rules → manual rules.

### Rule limits

Each rule is truncated at 30,000 characters when included in a review. The combined rules Bugbot includes for a review are capped at 100,000 characters. If you exceed that combined cap, some rules may be omitted. Required team rules are prioritized over non-required rules.

### See which rules a review used

Comment `bugbot run verbose=true` or `cursor review verbose=true` on a pull request. Bugbot posts a table of every rule included in that run and flags any that were truncated or omitted.

### Repository rules

#### Project rules

Create `.cursor/BUGBOT.md` files to provide project-specific context for reviews. Bugbot always includes the root `.cursor/BUGBOT.md` file and any additional files found while traversing upward from changed files.

```bash
project/
  .cursor/BUGBOT.md          # Always included (project-wide rules)
  backend/
    .cursor/BUGBOT.md        # Included when reviewing backend files
    api/
      .cursor/BUGBOT.md      # Included when reviewing API files
  frontend/
    .cursor/BUGBOT.md        # Included when reviewing frontend files
```

Cursor [project rules](https://cursor.com/docs/rules.md) (`*.mdc` files in `.cursor/rules/`) do not apply to Bugbot runs.

#### Learned rules

In [Bugbot repository rules](https://cursor.com/dashboard/bugbot/repository-rules), enable learning for your organizations and repositories.

Rules are generated automatically from your team's activity on GitHub for that repository or by manually backfilling from the history of the repository.

You can also teach Bugbot new rules inline by commenting `@cursor remember [fact]` on any PR. Bugbot saves the fact as a learned rule and applies it to future reviews.

Cursor will automatically enable or disable rules as it learns more about your team's activity over time.

| Field            | Description                                                                                                    |
| :--------------- | :------------------------------------------------------------------------------------------------------------- |
| **Name**         | Short title for the rule.                                                                                      |
| **Rule content** | The instructions Bugbot should follow (i.e. style gates, paths, or review expectations).                       |
| **Scoped paths** | Optional glob patterns such as `src/components/**`. Leave empty to apply the rule across the whole repository. |

#### Manual rules

In [Bugbot repository rules](https://cursor.com/dashboard/bugbot/repository-rules), you can create manual rules for individual repositories.

| Field            | Description                                                                                                    |
| :--------------- | :------------------------------------------------------------------------------------------------------------- |
| **Name**         | Short title for the rule.                                                                                      |
| **Rule content** | The instructions Bugbot should follow (i.e. style gates, paths, or review expectations).                       |
| **Scoped paths** | Optional glob patterns such as `src/components/**`. Leave empty to apply the rule across the whole repository. |

#### Rule analytics

**Analytics** on a Bugbot rule show how it performs on real PRs:

| Metric              | Meaning                                                    |
| :------------------ | :--------------------------------------------------------- |
| **Issues found**    | Number of findings Bugbot reported that involve this rule. |
| **PRs reviewed**    | Number of pull requests where those findings appeared.     |
| **Accepted issues** | Number of findings your team accepted.                     |
| **Acceptance rate** | Percentage of findings that were accepted.                 |

#### Examples

### Security: Flag any use of eval() or exec()

```text
If any changed file contains the string pattern /\beval\s*\(|\bexec\s*\(/i, then:
- Add a blocking Bug with title "Dangerous dynamic execution" and body:
  "Usage of eval/exec was found. Replace with safe alternatives or justify with a detailed comment and tests."
- Assign the Bug to the PR author.
- Apply label "security".
```

### OSS licenses: Prevent importing disallowed licenses

```text
If the PR modifies dependency files (package.json, pnpm-lock.yaml, yarn.lock, requirements.txt, go.mod, Cargo.toml), then:
- Run the built-in License Scan.
- If any new or upgraded dependency has license in {GPL-2.0, GPL-3.0, AGPL-3.0}, then:
  - Add a blocking Bug titled "Disallowed license detected"
  - Include the offending package names, versions, and licenses in the Bug body
  - Apply labels "compliance" and "security"
```

### Language standards: Flag React componentWillMount usage

```text
For files matching **/*.{js,jsx,ts,tsx} in React projects:
If a changed file contains /componentWillMount\s*\(/, then:
- Add a blocking Bug titled "Deprecated React lifecycle method"
- Body: "Replace componentWillMount with constructor or useEffect. See React docs."
- Suggest an autofix snippet that migrates side effects to useEffect.
```

### Standards: Require tests for backend changes

```text
If the PR modifies files in {server/**, api/**, backend/**} and there are no changes in {**/*.test.*, **/__tests__/**, tests/**}, then:
- Add a blocking Bug titled "Missing tests for backend changes"
- Body: "This PR modifies backend code but includes no accompanying tests. Please add or update tests."
- Apply label "quality"
```

### Style: Disallow TODO comments

```text
If any changed file contains /(?:^|\s)(TODO|FIXME)(?:\s*:|\s+)/, then:
- Add a non-blocking Bug titled "TODO/FIXME comment found"
- Body: "Replace TODO/FIXME with a tracked issue reference, e.g., `TODO(#1234): ...`, or remove it."
- If the TODO already references an issue pattern /#\d+|[A-Z]+-\d+/, mark the Bug as resolved automatically.
```

## Run in your agent

Use the `/review-bugbot` or `/review` skills to run Bugbot from your agent before you push the code.

**What diff is reviewed:** By default, `/review-bugbot` reviews your branch changes: every change relative to the base branch, including committed and uncommitted changes. Ask it to review only your uncommitted changes when you want narrower feedback.

**Against which branch:** `/review-bugbot` compares against your default base branch. When your base branch isn't the default (such as `main`), tell the agent which branch to compare against or let it infer from the context.

![Running the /review-bugbot skill from the agent input](/docs-static/images/bugbot/review-bugbot-skill.png)

### Sync with your pull request

`/review-bugbot` reviews stay in sync with Bugbot on your connected SCM (GitHub, GitLab, or Bitbucket).

Under the hood, `/review-bugbot` stores the [patch ID](https://git-scm.com/docs/git-patch-id) of the reviewed diff. When Bugbot on your SCM sees a diff with the same patch ID, it skips the review and leaves a comment noting it already reviewed that diff.

A common use case: run `/review-bugbot`, then open a pull request with the same diff, and Bugbot recognizes the review and skips the remote PR review.

`/review` and `/review-bugbot` are available in Cursor 3.7+ and at [cursor.com/agents](https://cursor.com/agents). CLI support is coming soon.

## Autofix

Bugbot Autofix automatically spawns a [Cloud Agent](https://cursor.com/docs/cloud-agent.md) to fix bugs found during PR reviews.

### How it works

When Bugbot finds bugs during a PR review, it can automatically:

1. Spawn a Cloud Agent to analyze and fix the reported issues
2. Push fixes to the existing branch or a new branch (depending on your settings)
3. Post a comment on the original PR with the results

![Bugbot Autofix comment on a PR](/docs-static/images/bugbot/bugbot-autofix-comment.png)

### Configuration

Configure autofix behavior in [Bugbot Automations](https://cursor.com/automations/from-cursor/bugbot).

### Individual

Individual users can configure their autofix preference in their personal Bugbot settings:

- **Use Installation Default** — Follow your organization's settings
- **Off** — autofix is disabled; use manual "Fix in Cursor" or "Fix in Web" links
- **Create New Branch** (Recommended) — Push fixes to a new branch
- **Commit to Existing Branch** — Push fixes to your branch (max 3 attempts per PR to prevent loops)

User settings override team defaults for your own PRs.

### Team

Team admins can set a default autofix mode for all team members in a GitHub organization:

- **Off** — autofix is disabled by default
- **Create New Branch** (Recommended) — Push fixes to a new branch for team members
- **Commit to Existing Branch** — Push fixes directly to the PR branch (max 3 attempts per PR to prevent loops)

Individual team members can override these defaults in their personal settings.

Autofix uses your **Default agent model** from [Settings → Models](https://cursor.com/dashboard/settings). If you haven't set a personal model preference, autofix falls back to your team's default model (if you're on a team) or the system default.

### Requirements

Autofix requires:

- [On-demand usage](https://cursor.com/docs/models-and-pricing.md) pricing enabled
- Storage enabled (not in Legacy Privacy Mode)

### Billing

Autofix uses Cloud Agent credits and is billed at your plan rates. Cloud Agent billing follows your existing [pricing plan](https://cursor.com/docs/models-and-pricing.md).

## MCP support

Bugbot is integrated with your [MCP servers](https://cursor.com/docs/mcp.md) so your AI tools can interact with Bugbot directly. Use the MCP server to provide additional tools to guide Bugbot's review process.

To get started:

1. Follow the [MCP documentation](https://cursor.com/docs/mcp.md) for MCP server setup instructions.
2. Add the tools to [Bugbot in Automations](https://cursor.com/automations/from-cursor/bugbot).

MCP support is available on Team and Enterprise plans only.

## Admin Configuration API

Team admins can use the Bugbot Admin API to manage repositories and control which users can use Bugbot. Use it to automate repository management, enable Bugbot across multiple repositories, or integrate user provisioning with internal tools.

### Authentication

All endpoints require a team Admin API Key passed as a Bearer token:

```bash
Authorization: Bearer $API_KEY
```

To create an API key:

1. Visit [API Keys in the Cursor dashboard](https://cursor.com/dashboard/api)
2. Click **New API Key**
3. Save the API key

All endpoints are rate-limited to 60 requests per minute per team.

### Enabling or disabling repositories

Use the `/bugbot/repo/update` endpoint to toggle Bugbot on or off for a repository:

```bash
curl -X POST https://api.cursor.com/bugbot/repo/update \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "repoUrl": "https://github.com/your-org/your-repo",
    "enabled": true,
    "manualTriggerOnly": false
  }'
```

**Parameters:**

- `repoUrl` (string, required): The full URL of the repository
- `enabled` (boolean, required): `true` to enable Bugbot, `false` to disable it
- `manualTriggerOnly` (boolean, optional): When `true`, Bugbot won't run automatically on PR updates for this repository. Manual triggers, such as commenting `cursor review` or `bugbot run`, still work.

The Automations UI may take a moment to reflect changes made through the API due to caching. The API response shows the current state in the database.

### Listing repositories

Use the `/bugbot/repos` endpoint to list all repositories with their Bugbot settings for your team:

```bash
curl https://api.cursor.com/bugbot/repos \
  -H "Authorization: Bearer $API_KEY"
```

The response includes each repository's enabled status, manual-only setting, and timestamps.

### Managing user access

Use the `/bugbot/user/update` endpoint to control which GitHub, GitLab, or Bitbucket users can use your team's Bugbot licenses. Enterprises use this to integrate Bugbot provisioning with internal access-request tools.

#### Prerequisites

Before calling this endpoint, enable an allowlist or blocklist mode in your [team Bugbot settings](https://cursor.com/automations/from-cursor/bugbot):

- **Allowlist mode ("Only...")**: Only users on the list can use Bugbot
- **Blocklist mode ("Everyone but...")**: All users can use Bugbot except those on the list

If neither mode is enabled, the API returns an error.

#### Adding or removing a user

```bash
curl -X POST https://api.cursor.com/bugbot/user/update \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "username": "octocat",
    "allow": true
  }'
```

**Parameters:**

- `username` (string, required): The GitHub, GitLab, or Bitbucket username (case-insensitive)
- `allow` (boolean, required): Whether to grant or revoke access

How `allow` behaves depends on the active mode:

| Mode      | `allow: true`                                | `allow: false`                             |
| --------- | -------------------------------------------- | ------------------------------------------ |
| Allowlist | Adds user to list (can use Bugbot)           | Removes user from list (cannot use Bugbot) |
| Blocklist | Removes user from blocklist (can use Bugbot) | Adds user to blocklist (cannot use Bugbot) |

**Response:**

```json
{
  "outcome": "success",
  "message": "Updated team-level allowlist for @octocat",
  "updatedTeamSettings": true,
  "updatedInstallations": 0
}
```

The allowlist is stored at the team level and applies across all GitHub, GitLab, and Bitbucket installations owned by that team. Usernames are normalized to lowercase.

#### Example: provisioning users through an internal tool

Connect this API to an internal access-request portal. When an employee requests Bugbot access, the portal calls the API to add them. When they leave or lose access, it calls the API to remove them.

**Grant access:**

```bash
curl -X POST https://api.cursor.com/bugbot/user/update \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"username": "employee-scm-username", "allow": true}'
```

**Revoke access:**

```bash
curl -X POST https://api.cursor.com/bugbot/user/update \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"username": "employee-scm-username", "allow": false}'
```

## Pricing

Bugbot uses usage-based billing.

Bugbot pricing changed with the May 2026 pricing update. See the [announcement blog post](https://cursor.com/blog/may-2026-bugbot-changes) for background. If you're still on the old seat-based plan, see [legacy Bugbot pricing](https://cursor.com/docs/bugbot/legacy-pricing.md).

### Billing

### Individuals

### Usage-based billing

Bugbot includes:

- Reviews on all PRs across your repositories
- Access to Bugbot rules
- The ability to set the effort level Bugbot uses for reviews

Bugbot first consumes your included usage, then bills additional reviews through on-demand spend. See the [pricing page](https://cursor.com/pricing#bugbot) for current rates.

### Getting started

Subscribe through your account settings.

### Teams

### Usage-based billing

Bugbot Teams includes:

- Code reviews on all PRs
- Analytics and reporting
- The ability to set the effort level Bugbot uses for reviews
- Advanced rules and settings

Bugbot Teams bills from on-demand spend. See the [pricing page](https://cursor.com/pricing#bugbot) for current rates.

### Getting started

Subscribe through your team dashboard to enable billing.

## Troubleshooting

If Bugbot isn't working:

1. **Enable verbose mode** by commenting `cursor review verbose=true` or `bugbot run verbose=true` for detailed logs, which Bugbot rules loaded, and a request ID
2. **Check permissions** to verify Bugbot has repository access
3. **Verify installation** to confirm your repository provider integration is installed and enabled

Include the request ID from verbose mode when reporting issues.

## FAQ

### Does Bugbot read PR comments?

Yes. Bugbot reads both top-level and inline pull request comments from connected providers and includes them as context during reviews. This helps avoid duplicate suggestions and allows Bugbot to build on prior feedback from reviewers.

### How do I see which rules Bugbot used?

Comment `bugbot run verbose=true` or `cursor review verbose=true` on the pull request. Bugbot posts a table of every rule included in that run and flags any that were truncated or omitted. See [Rule limits](https://cursor.com/docs/bugbot.md#rule-limits) if a rule is missing or cut off.

### Is Bugbot privacy-mode compliant?

Yes, Bugbot follows the same privacy compliance as Cursor and processes data identically to other Cursor requests.

### What happens when I use all included Bugbot usage?

When you use all included Bugbot usage, additional Bugbot reviews bill from on-demand spend.

### How do I give Bugbot access to a self-hosted source control instance?

See the setup and networking guides on the respective integration pages:

- [GitHub Enterprise Server](https://cursor.com/docs/integrations/github.md#setup)
- [GitLab Self-Hosted](https://cursor.com/docs/integrations/gitlab.md#setup)
- [Bitbucket Data Center](https://cursor.com/docs/integrations/bitbucket.md#setup)

## Related

- [Bugbot help](https://cursor.com/help/ai-features/bugbot.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
