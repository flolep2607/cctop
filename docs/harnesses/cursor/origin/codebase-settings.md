# Codebase settings

Origin is currently released in early beta. You can create repos, push and pull with git, mirror from GitHub, browse and search code, open and merge pull requests, and share with your Cursor team.

Please submit any and all feedback to [hi@cursor.com](mailto:hi@cursor.com) to help us make the product better.

Codebase settings apply across your team's Origin repos at [cursor.com/codebase](https://cursor.com/codebase), not to a single repository. Open codebase settings from the codebase home (separate from a repo's **Settings** tab).

For per-repository sync status, permissions, and rules, see [Repository settings](https://cursor.com/docs/origin/settings.md).

## Permissions

Team-level permissions control who can use Origin for your codebase: who can access repos under your claimed codebase name, and how Origin relates to your Cursor team membership.

- A team admin [claims the codebase name](https://cursor.com/docs/origin.md#enable-origin) and enables Origin; non-admins can request access from the same page
- Once Origin is enabled, admins can create repositories and use Permissions to grant access, including repository creation, to other members
- Admins can disable Origin for the team at any time from the dashboard; teams on legacy privacy mode cannot enable Origin, so switch to [Privacy Mode](https://cursor.com/help/security-and-privacy/privacy.md#how-do-i-enable-privacy-mode) first if you want access

Exact controls in the Permissions UI may change during early beta.

## Apps

**Apps** under codebase settings (also at [cursor.com/codebase/settings/apps](https://cursor.com/codebase/settings/apps)) is where you install and manage apps for your codebase:

- **Third-party apps** such as Vercel, Depot, and Buildkite: install them here, then see which apps are active on a given repository from that repository's **Apps** tab in [Repository settings](https://cursor.com/docs/origin/settings.md#apps)
- **Internal apps** for API access: create an app for your team, then use the app credentials and installation flow to call the Origin API

Apps authenticate with app JWTs and installation access tokens. For base URL, authentication, scopes, webhooks, and endpoint reference, see the [Origin API](https://cursor.com/docs/api/origin.md).


---

## Sitemap

[Overview of all docs pages](/llms.txt)
