# Worktrees

The UI-native worktrees feature described on this page is only available in the Agents Window. In the IDE, use the Worktree Skills commands below.

[Media](/docs-static/images/configuration/worktrees/cursor-worktrees-2.mp4)

Worktrees let Agent work in isolated Git checkouts. Each task gets its own files, dependencies, and changes while your main checkout stays untouched.

Use worktrees when you want to start several agents on the same repo without conflicts.

## Create a worktree in the Agents Window

When you start or move an agent into a worktree from the Agents Window, Cursor creates a separate checkout for that agent. The agent continues the task inside the worktree, so changes stay isolated from your main checkout.

After the agent finishes, review the result in the Agents Window. You can keep working in the worktree, create a commit or PR from that checkout, or bring the result back into your main workspace.

## How does worktree setup work?

You can customize worktree setup with `.cursor/worktrees.json`. Cursor checks this file when it creates a worktree in the Agents Window, the IDE, or the [Cursor CLI](https://cursor.com/docs/cli/using.md#cli-worktrees).

Cursor looks for `.cursor/worktrees.json` in this order:

1. In the worktree path
2. In the root path of your project

### Configuration options

The `worktrees.json` file supports three setup keys:

- **`setup-worktree-unix`**: Commands or a script path for macOS and Linux. This takes precedence over `setup-worktree` on Unix systems.
- **`setup-worktree-windows`**: Commands or a script path for Windows. This takes precedence over `setup-worktree` on Windows.
- **`setup-worktree`**: Generic fallback for all operating systems.

Each key accepts either:

- **An array of shell commands**: executed sequentially in the worktree
- **A string filepath**: path to a script file relative to `.cursor/worktrees.json`

## Example setup configurations

### Using command arrays

#### Node.js project

```json
{
  "setup-worktree": [
    "npm ci",
    "cp $ROOT_WORKTREE_PATH/.env .env"
  ]
}
```

We do not recommend symlinking dependencies into the worktree. This can cause issues in the main worktree. Use a fast package manager such as `bun`, `pnpm`, or `uv` instead.

#### Python project with virtual environment

```json
{
  "setup-worktree": [
    "python -m venv venv",
    "source venv/bin/activate && pip install -r requirements.txt",
    "cp $ROOT_WORKTREE_PATH/.env .env"
  ]
}
```

#### Project with database migrations

```json
{
  "setup-worktree": [
    "npm ci",
    "cp $ROOT_WORKTREE_PATH/.env .env",
    "npm run db:migrate"
  ]
}
```

#### Build and link dependencies

```json
{
  "setup-worktree": [
    "pnpm install",
    "pnpm run build",
    "cp $ROOT_WORKTREE_PATH/.env.local .env.local"
  ]
}
```

### Using script files

For more complex setups, reference script files instead of inline commands:

```json
{
  "setup-worktree-unix": "setup-worktree-unix.sh",
  "setup-worktree-windows": "setup-worktree-windows.ps1",
  "setup-worktree": [
    "echo 'Using generic fallback. For better support, define OS-specific scripts.'"
  ]
}
```

Place your scripts in the `.cursor/` directory next to `worktrees.json`.

**setup-worktree-unix.sh** (Unix and macOS):

```bash
#!/bin/bash
set -e

# Install dependencies
npm ci

# Copy environment file
cp "$ROOT_WORKTREE_PATH/.env" .env

# Run database migrations
npm run db:migrate

echo "Worktree setup complete!"
```

**setup-worktree-windows.ps1** (Windows):

```powershell
$ErrorActionPreference = 'Stop'

# Install dependencies
npm ci

# Copy environment file
Copy-Item "$env:ROOT_WORKTREE_PATH\.env" .env

# Run database migrations
npm run db:migrate

Write-Host "Worktree setup complete!"
```

### OS-specific configurations

You can provide different setup commands for different operating systems:

```json
{
  "setup-worktree-unix": [
    "npm ci",
    "cp $ROOT_WORKTREE_PATH/.env .env",
    "chmod +x scripts/*.sh"
  ],
  "setup-worktree-windows": [
    "npm ci",
    "copy %ROOT_WORKTREE_PATH%\\.env .env"
  ]
}
```

### Debugging

If you want to debug worktree setup, open the Output panel in the editor and select `Worktrees Setup`.

## How does Cursor discover existing worktrees?

Cursor 3.5 keeps a modified time checkpoint for the machine worktree root and for each workspace subdirectory. On startup, Cursor re-scans the filesystem unless those timestamps prove nothing changed since the last discovery. This avoids skipping new worktrees that were created while Cursor was closed and eliminates the older `worktree.discoveryComplete` flag.

## Worktrees cleanup

The cleanup behavior in this section reflects Cursor 3.5 and later.

Cursor can clean up older worktrees automatically to limit disk usage. Cleanup runs on an interval and keeps the newest worktrees up to the configured machine-wide maximum count across every workspace on the device.

```json
{
  "cursor.worktreeCleanupIntervalHours": 6,
  "cursor.worktreeMaxCount": 25
}
```

Use these machine-scoped settings to control cleanup:

- **`cursor.worktreeCleanupIntervalHours`**: how often Cursor checks for old worktrees. Cursor 3.5 catches up after restarts by scheduling a delayed cleanup if the last successful run is older than this interval.
- **`cursor.worktreeMaxCount`**: the maximum number of worktrees Cursor keeps before cleaning up older ones. The default cap is 25 worktrees per machine, and all workspaces contribute toward the same limit.

Cursor re-discovers the worktree root on every cleanup pass, so worktrees created outside the manager (for example, worktrees created by `/worktree` skills or `git worktree add`) are eligible for deletion. When creating a worktree would exceed the cap, Cursor debounces bursts of events and starts an immediate cleanup instead of waiting for the next interval.

## Worktree Skills in IDE

In the IDE, you can use the `/worktree` and `/best-of-n` commands to run tasks in isolated worktrees.

### Use `/worktree` for one isolated run

Start a task with `/worktree` when you want Cursor to do the rest of that chat in a separate checkout.

- Keep experimental edits away from your main checkout
- Run installs, builds, and tests without disturbing your current branch
- Work on risky refactors with a simple cleanup path

```text
/worktree fix the failing auth tests and update the login copy
```

In many cases, you can commit and push directly from the worktree. Ask the agent:

```text
Commit and push these changes, then open a PR
```

If you want to bring the changes into your main checkout to test them, use `/apply-worktree`. When you are done with the isolated checkout, use `/delete-worktree`.

If you want to see all worktrees in your repository, run:

```bash
git worktree list
```

### Use `/best-of-n` to compare multiple models

`/best-of-n` runs the same task across multiple models at once. Each run gets its own worktree, so the candidates stay isolated from each other and from your main checkout.

```text
/best-of-n sonnet,gpt,composer fix the flaky logout test
```

Use it when you want to:

- Compare different models on the same prompt
- Try multiple approaches for a hard change
- Pick the strongest result before applying anything

`/best-of-n` compares runs only. It does not merge changes back into your main checkout for you. After you pick a winner, you can commit and push directly from the worktree or use `/apply-worktree` to bring the changes into your main checkout.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
