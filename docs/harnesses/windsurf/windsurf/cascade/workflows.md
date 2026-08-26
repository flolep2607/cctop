# Workflows

Automate repetitive tasks in Cascade with reusable workflows defined as markdown files. Create PR review, deployment, testing, and code formatting workflows.

Workflows enable users to define a series of steps to guide Cascade through a repetitive set of tasks, such as deploying a service or responding to PR comments.

These Workflows are saved as markdown files, allowing users and their teams an easy repeatable way to run key processes.

Once saved, Workflows can be invoked in Cascade via a slash command with the format of `/[name-of-workflow]`.

<Note>
  Workflows are **manual-only** — Cascade will never invoke a workflow automatically. If you want Cascade to pick up a procedure on its own, use a [Skill](/windsurf/cascade/skills) instead.
</Note>

## How it works

Rules generally provide large language models with guidance by providing persistent, reusable context at the prompt level.

Workflows extend this concept by providing a structured sequence of steps or prompts at the trajectory level, guiding the model through a series of interconnected tasks or actions.

<Frame>
  <img />
</Frame>

To execute a Workflow, users simply invoke it in Cascade using the `/[workflow-name]` command.

<Tip>You can call other Workflows from within a Workflow! <br /><br />For example, /workflow-1 can include instructions like "Call /workflow-2" and "Call /workflow-3".</Tip>

Upon invocation, Cascade sequentially processes each step defined in the Workflow, performing actions or generating responses as specified.

## How to create a Workflow

To get started with Workflows, click on the `Customizations` icon in the top right slider menu in Cascade, then navigate to the `Workflows` panel. Here, you can click on the `+ Workflow` button to create a new Workflow.

Workflows are saved as markdown files within `.windsurf/workflows/` directories and contain a title, description, and a series of steps with specific instructions for Cascade to follow.

## Workflow Discovery

Windsurf automatically discovers workflows from multiple locations to provide flexible organization:

* **Current workspace and sub-directories**: All `.windsurf/workflows/` directories within your current workspace and its sub-directories
* **Git repository structure**: For git repositories, Windsurf also searches up to the git root directory to find workflows in parent directories
* **Multiple workspace support**: When multiple folders are open in the same workspace, workflows are deduplicated and displayed with the shortest relative path

### Workflow Storage Locations

| Scope                                                     | Location                                      | Notes                                                                                                               |
| --------------------------------------------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Workspace                                                 | `.windsurf/workflows/*.md`                    | In your current workspace, any sub-directory, or any parent directory up to the git root. Committed with your repo. |
| Global                                                    | `~/.codeium/windsurf/global_workflows/*.md`   | Available in every workspace on your machine. Not committed.                                                        |
| Built-in                                                  | Managed by Windsurf                           | Templates shipped with Windsurf (e.g. `/plan`).                                                                     |
| [System (Enterprise)](#system-level-workflows-enterprise) | OS-specific (e.g. `/etc/windsurf/workflows/`) | Deployed by IT, read-only for end users.                                                                            |

When you create a new workflow through the UI, it will be saved in the `.windsurf/workflows/` directory of your current workspace, not necessarily at the git root. To create a global workflow, use the `+ Global` button in the Workflows panel or create the file directly in `~/.codeium/windsurf/global_workflows/`.

Workflow files are limited to 12000 characters each.

<video />

### Generate a Workflow with Cascade

You can also ask Cascade to generate Workflows for you! This works particularly well for Workflows involving a series of steps in a particular CLI tool.

<video />

## Example Workflows

There are a myriad of use cases for Workflows, such as:

<Card title="/address-pr-comments">
  This is a Workflow our team uses internally to address PR comments:

  ```
  1. Check out the PR branch: `gh pr checkout [id]`

  2. Get comments on PR

   bash
   gh api --paginate repos/[owner]/[repo]/pulls/[id]/comments | jq '.[] | {user: .user.login, body, path, line, original_line, created_at, in_reply_to_id, pull_request_review_id, commit_id}'

  3. For EACH comment, do the following. Remember to address one comment at a time.
   a. Print out the following: "(index). From [user] on [file]:[lines] — [body]"
   b. Analyze the file and the line range.
   c. If you don't understand the comment, do not make a change. Just ask me for clarification, or to implement it myself.
   d. If you think you can make the change, make the change BEFORE moving onto the next comment.

  4. After all comments are processed, summarize what you did, and which comments need the USER's attention.
  ```
</Card>

<Card title="/git-workflows">
  Commit using predefined formats and create a pull requests with standardized title and descriptions using the appropriate CLI commands.
</Card>

<Card title="/dependency-management">
  Automate the installation or updating of project dependencies based on a configuration file (e.g., requirements.txt, package.json).
</Card>

<Card title="/code-formatting">
  Automatically run code formatters (like Prettier, Black) and linters (like ESLint, Flake8) on file save or before committing to maintain code style and catch errors early.
</Card>

<Card title="/run-tests-and-fix">
  Run or add unit or end-to-end tests and fix the errors automatically to ensure code quality before committing, merging, or deploying.
</Card>

<Card title="/deployment">
  Automate the steps to deploy your application to various environments (development, staging, production), including any necessary pre-deployment checks or post-deployment verifications.
</Card>

<Card title="/security-scan">
  Integrate and trigger security vulnerability scans on your codebase as part of the CI/CD pipeline or on demand.
</Card>

## System-Level Workflows (Enterprise)

Enterprise organizations can deploy system-level workflows that are available globally across all workspaces and cannot be modified by end users without administrator permissions. This is ideal for enforcing organization-wide development processes, deployment procedures, and compliance workflows.

System-level workflows are loaded from OS-specific directories:

**macOS:**

```
/Library/Application Support/Windsurf/workflows/*.md
```

**Linux/WSL:**

```
/etc/windsurf/workflows/*.md
```

**Windows:**

```
C:\ProgramData\Windsurf\workflows\*.md
```

Place your workflow files (as `.md` files) in the appropriate directory for your operating system. The system will automatically load all `.md` files from these directories.

### Workflow Precedence

When workflows with the same name exist at multiple levels, system-level workflows take the highest precedence:

1. **System** (highest priority) - Organization-wide workflows deployed by IT
2. **Workspace** - Project-specific workflows in `.windsurf/workflows/`
3. **Global** - User-defined workflows in `~/.codeium/windsurf/global_workflows/`
4. **Built-in** - Default workflows provided by Windsurf

This means that if an organization deploys a system-level workflow with a specific name, it will override any workspace, global, or built-in workflow with the same name.

In the Windsurf UI, system-level workflows are displayed with a "System" label and cannot be deleted by end users.

<Note>
  **Important**: System-level workflows should be managed by your IT or security team. Ensure your internal teams handle deployment, updates, and compliance according to your organization's policies. You can use standard tools and workflows such as Mobile Device Management (MDM) or Configuration Management to do so.
</Note>
