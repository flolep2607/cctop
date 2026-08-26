# Integrations

Origin is currently released in early beta. You can create repos, push and pull with git, mirror from GitHub, browse and search code, open and merge pull requests, and share with your Cursor team.

Please submit any and all feedback to [hi@cursor.com](mailto:hi@cursor.com) to help us make the product better.

Origin works with Cursor Automations and cloud agents so agents can review, change, and push code on your Origin repos.

## Automations

[Automations](https://cursor.com/docs/cloud-agent/automations.md) run cloud agents on a schedule or when source-control events fire. Point an automation at an Origin repository the same way you would at a connected GitHub or GitLab repo.

Common triggers for Origin repos:

- **Push to branch** — for example push to `main` or `master`
- **Pull request opened**, **Pull request pushed**, and related PR events

Create automations from [cursor.com/automations](https://cursor.com/automations), the Agents Window, or the `/automate` skill. Choose the Origin repository (or a multi-repo environment that includes it), set the trigger and prompt, then save and activate.

See [Automations](https://cursor.com/docs/cloud-agent/automations.md) for billing, permission scopes, and the full trigger list.

## Cloud agents

[Cloud agents](https://cursor.com/docs/cloud-agent.md) can work against Origin repositories: clone, branch, commit, push, and open pull requests.

Attach a cloud agent to an Origin repo from your team's codebase the same way you attach one to other connected source control. Agents use your Cursor account's Origin access.

### Local agents

A local agent in Cursor can also [create Origin repositories](https://cursor.com/docs/origin/create-repository.md#create-with-a-cursor-agent) with the [Origin CLI](https://cursor.com/docs/origin/cli.md): install, sign in, `origin repo create`, set the remote, and push.

## Related

- [Create a repository](https://cursor.com/docs/origin/create-repository.md)
- [Pull requests](https://cursor.com/docs/origin/pull-requests.md)
- [Origin API](https://cursor.com/docs/api/origin.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
