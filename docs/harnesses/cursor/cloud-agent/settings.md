# Cloud Agents settings

Workspace admins can configure Cloud Agents from the [Cloud Agents dashboard](https://cursor.com/dashboard/cloud-agents).

## Environment management

The **Environments** view lists the saved environments available to your team. Environments can be scoped to one repo or to a group of repos.

Open an environment to review:

- The repositories it applies to
- Whether it uses a snapshot or `.cursor/environment.json`
- The install script that runs during [Builds](https://cursor.com/docs/cloud-agent/builds.md)
- Runtime secrets and build secrets
- Network access settings
- Version history, Builds, and setup runs

Use **Update with Agent** when you want Cursor to inspect the current environment and propose a new setup. Use **New Setup Run** when you want Cursor to start setting up the environment fresh. Use **Restore** from version history to make a prior environment version active again.

The **Builds** tab shows the prepared environment versions available to Cloud Agents. You can inspect logs, trigger a Build, choose or pin the active Build, and start an agent from a specific Build. See [Cloud Agent Builds](https://cursor.com/docs/cloud-agent/builds.md) for details.

## Default settings

- **Default model** – the model used when a run does not specify one. Pick any model available for cloud agents.
- **Default repository** – when empty, agents ask the user to choose a repo. Supplying a repo here lets users skip that step.
- **Base branch** – the branch agents fork from when creating pull requests. Leave blank to use the repository’s default branch.

## Network access settings

Control which network resources Cloud Agents can reach. User and team settings support three modes:

- **Allow all network access** – no domain restrictions.
- **Default + allowlist** – the [default domains](https://cursor.com/docs/agent/security/run-modes.md#network-access) plus any domains you add.
- **Allowlist only** – only domains you explicitly add.

Users, team admins, and environment owners can configure network access. Environment-level settings can inherit user or team policy, add an environment allowlist, or define their own access mode. See [Network Access](https://cursor.com/docs/cloud-agent/security-network.md) for full details.

## Security settings

All security options require admin privileges.

- **Display agent summary** – controls whether Cursor shows the agent's file-diff images and code snippets. Disable this if you prefer not to expose file paths or code in the sidebar.
- **Display agent summary in external channels** – extends the previous toggle to Slack or any external channel you've connected.
- **Team follow-ups** – controls whether team members can send follow-up messages to cloud agents created by other users on the team. See [team follow-ups](https://cursor.com/docs/cloud-agent/settings.md#team-follow-ups) below.

## Team feature settings

Team admins can enable or disable these features for their team:

- **Long running agents** – controls whether team members can run agents for extended durations. Admins can enable or restrict this capability at the team level. Long-running is not available for multi-repo environments yet. Selecting a multi-repo environment disables the toggle.
- **Computer use** – controls whether agents can use computer interaction capabilities (available to enterprise teams only).

Changes save instantly and affect new agents immediately.

### Team follow-ups

Team members can send follow-up messages to cloud agents created by other users on the same team. This is useful when a teammate starts an agent and you need to course-correct, add context, or continue the work while they're unavailable.

Follow-ups build on agent visibility. A teammate must be able to view the agent before they can send follow-ups: they must belong to the same Cursor team, and they need their own access to the agent's repository. See [Share agents with your team](https://cursor.com/docs/cloud-agent.md#share-agents-with-your-team).

Team admins control this behavior from the [Cloud Agents security settings](https://cursor.com/dashboard/cloud-agents) with three options:

| Setting                   | Behavior                                                                                                                                                                                   |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Disabled**              | Only the original creator can send follow-ups to their agent. No team follow-ups are allowed.                                                                                              |
| **Service accounts only** | Team members can send follow-ups to agents created by a [service account](https://cursor.com/docs/account/enterprise/service-accounts.md), but not to agents created by other human users. |
| **All**                   | Any team member can send follow-ups to any agent on the team, regardless of who created it.                                                                                                |

### Lateral movement and secret exposure

Enabling team follow-ups means a user can influence the execution of a cloud agent that runs with *another user's* secrets and credentials. A follow-up message can instruct the agent to read environment variables, print secrets to logs, push credentials to an external endpoint, or perform actions using the original creator's access tokens.

A team member with limited permissions could escalate their access by directing an agent that holds a more privileged user's secrets. Treat this setting with the same care you would give shared SSH keys or service credentials.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
