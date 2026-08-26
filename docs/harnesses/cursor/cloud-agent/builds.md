# Cloud Agent Builds

Builds prepare your Cloud Agent environment in the background. Each agent starts from a pre-built machine with your repositories, tools, and dependencies ready.

With Builds, you get:

- **Faster starts**: Clone, install, and dependency work happen ahead of time, so agents boot from a ready environment instead of waiting on every start.
- **Reliable starts**: Agents always start from the latest successful Build. A failed install or bad config doesn't replace the working one.
- **Observable environments**: You can see every Build, inspect its logs and commits, and trace which Build each agent used.

## How Builds work

A Build is a bootable snapshot of a prepared Cloud Agent environment. Cursor creates Builds ahead of agent runs and keeps the latest successful one ready to start.

Each Build follows this lifecycle:

1. **Trigger**: A Build starts on a schedule, after you save an environment version, from a manual request, or at an agent's request. See [When Builds occur](https://cursor.com/docs/cloud-agent/builds.md#when-builds-occur).
2. **Prepare**: Cursor starts from your base image, clones every repository in the environment at its default branch, and runs the `install` command to completion.
3. **Snapshot**: Cursor saves the machine's disk state with the environment version and exact commit SHA for each repository.
4. **Activate**: A successful Build becomes active.
5. **Start agents**: New agents, automations, and code reviews start from the active Build.

Cursor keeps pre-warmed copies of active Builds ready. This removes repository cloning and dependency installation from the agent startup path.

If a new Build fails, agents continue to use the last successful Build. A broken dependency update, install command, or Dockerfile doesn't replace the active environment.

## When Builds occur

Cursor starts a Build for four reasons. The Builds tab labels each one with its trigger type.

| Trigger              | When it runs                                                          |
| :------------------- | :-------------------------------------------------------------------- |
| Recurring            | On a regular schedule for every environment                           |
| Configuration change | When you save the environment configuration or change its secrets     |
| Manual               | When you select **Trigger build** in the Builds tab                   |
| Agent-requested      | When an agent runs a test Build, for example during environment setup |

### Recurring Builds

Cursor regularly checks each environment and rebuilds it when something changed. This keeps the active Build close to the head of each repository's default branch, so agents start with fresh code and warm dependency caches instead of pulling and reinstalling at startup.

### Skipped Builds

A recurring check skips the Build when nothing changed since the last completed one: no new commits on the default branch of any repository in the environment, and no configuration or secret changes. The Builds tab records these checks with a **Skipped** status. They complete in seconds, run no install commands, and leave the active Build in place.

A steady stream of Recurring entries mixing Skipped and Success statuses is the expected state for a healthy environment. Quiet repositories produce mostly Skipped entries. Active repositories rebuild more often.

Cursor only skips recurring Builds. Manual, agent-requested, and configuration-change Builds always run.

## What runs during a Build and an agent start

Use each environment command for a distinct phase:

| Command     | When it runs                   | Use it for                                                                             |
| :---------- | :----------------------------- | :------------------------------------------------------------------------------------- |
| `install`   | During each Build              | Installing dependencies, generating code, compiling artifacts, and warming disk caches |
| `start`     | At the start of each agent run | Starting Docker, databases, tunnels, and other services                                |
| `terminals` | At the start of each agent run | Starting app processes in `tmux` terminals shared with the agent                       |

Make `install` complete and idempotent. It can run repeatedly and may run on top of previously prepared disk state. Commands such as `npm install`, `pnpm install`, and `pip install` already support this pattern.

Builds preserve disk state only. Running processes, shell exports, and
in-memory caches stop when Cursor snapshots the machine. Put services and
other session-specific work in `start` or `terminals`.

Your existing environment inputs still apply. Builds use saved snapshots, `.cursor/environment.json`, Dockerfiles, install and startup commands, secrets, and network settings.

## How Builds handle Git state

A Build records the commit checked out for each repository when it runs.

- **Default branch runs** start from the commit recorded in the active Build. Scheduled Builds refresh that commit in the background. When **Update stale builds** is on and a Build is older than your **Staleness threshold**, agents pull the latest default-branch code at start. When that setting is off, agents use the Build's recorded commit as-is. The default threshold is 24 hours. Set it to `0` to always pull.
- **Feature branch runs** start from the active Build's prepared disk, then Cursor checks out the requested branch. The source code matches the branch you selected while reusing dependencies from the Build.
- **Multi-repo environments** record one commit per repository and prepare the complete workspace together.

If a feature branch changes dependencies, the agent receives your environment context and install command so it can refresh the environment before testing.

## How secrets work with Builds

Builds can access team and environment secrets. Use these for private package registries, artifact stores, and other credentials required by `install`.

User secrets are added only when an agent starts. They aren't available during Builds and don't become part of a shared snapshot.

Saving environment configuration or changing its secrets triggers a new Build.

## Manage Builds

Open an environment's **Builds** tab to:

- See every Build's type, status, and start time
- Open a Build to inspect its details and logs
- Select **Trigger build** to run a Build on demand
- Activate a draft Build or deactivate a Build
- Cancel an in-progress Build
- Start an agent from a specific Build
- Configure **Update stale builds** and the **Staleness threshold**

Every agent run records the Build it started from. Use this provenance to compare environment behavior with the exact configuration and repository commits in the Build.

## Debug a Build

Open a failed Build to inspect its events and logs. Agents continue to start from the active successful Build while you diagnose the failure.

For an exact reproduction, start an agent from the failed Build. The agent opens the machine in its failed state, where it can inspect logs, update the environment, run a test Build, and verify the result.

You can also ask a Cloud Agent to inspect and manage Builds through the built-in [Cursor Cloud MCP](https://cursor.com/docs/cloud-agent/capabilities.md#cursor-cloud-mcp). For example:

```text
Inspect the latest failed Build for this environment. Fix the environment
configuration, run a test Build, and verify it before proposing the final
install and start commands.
```

## Build behavior reference

### Which Build does an agent use?

By default, an agent uses the latest successful active Build for its environment.

You can also start an agent from a specific Build when testing or debugging.

### What happens before the first successful Build?

Agents use the standard environment startup flow until the first Build completes successfully. A failed Build doesn't interrupt existing agent workflows.

### How fresh is the source code?

Feature branch runs check out the requested branch after the Build starts. Default branch runs begin at the commit recorded by the active Build. If **Update stale builds** is on and the Build is older than your **Staleness threshold**, agents pull the latest default-branch code at start.

### Do Builds replace snapshots or Dockerfiles?

No. A saved snapshot or Dockerfile defines the base machine used to create a Build. Cursor then clones the repositories, runs `install`, and creates a fresh bootable snapshot.

### Do Builds support multiple repositories?

Yes. One Build prepares all repositories in the environment and records the commit used for each one.

### Do Builds cost extra?

No. Builds are included with Cloud Agents.

## Related

- [Cloud Environment Setup](https://cursor.com/docs/cloud-agent/setup.md)
- [Cloud Agent capabilities](https://cursor.com/docs/cloud-agent/capabilities.md)
- [Cloud Agents settings](https://cursor.com/docs/cloud-agent/settings.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
