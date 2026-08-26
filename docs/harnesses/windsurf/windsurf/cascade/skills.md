# Skills

Skills help Cascade handle complex, multi-step tasks.

The hardest engineering tasks often take more than just good prompts. They might require reference scripts, templates, checklists, and other supporting files. Skills let you bundle all of these together into folders that Cascade can invoke (read and use).

Skills are a great way to teach Cascade how to execute multi-step workflows consistently.

Cascade uses [**progressive disclosure**](https://agentskills.io/what-are-skills#how-skills-work): only the skill's `name` and `description` are shown to the model by default. The full `SKILL.md` content and supporting files are loaded **only when Cascade decides to invoke the skill** (or when you `@mention` it). This keeps your context window lean even with many skills defined.

For more details on the Skills specification, visit [agentskills.io](https://agentskills.io/home).

## How to Create a Skill

### Using the UI (easiest)

1. Open the Cascade panel
2. Click the three dots in the top right of the panel to open up the customizations menu
3. Click on the `Skills` section
4. Click `+ Workspace` to create a workspace (project-specific) skill, or `+ Global` to create a global skill
5. Name the skill (lowercase letters, numbers, and hyphens only)

### Manual Creation

**Workspace Skill (project-specific):**

1. Create a directory: `.windsurf/skills/<skill-name>/`
2. Add a `SKILL.md` file with YAML frontmatter

**Global Skill (available in all workspaces):**

1. Create a directory: `~/.codeium/windsurf/skills/<skill-name>/`
2. Add a `SKILL.md` file with YAML frontmatter

## SKILL.md File Format

Each skill requires a `SKILL.md` file with YAML frontmatter containing the skill's metadata:

### Example skill

```markdown theme={null}
---
name: deploy-to-production
description: Guides the deployment process to production with safety checks
---

## Pre-deployment Checklist
1. Run all tests
2. Check for uncommitted changes
3. Verify environment variables

## Deployment Steps
Follow these steps to deploy safely...

[Reference supporting files in this directory as needed]
```

### Required Frontmatter Fields

* **name**: Unique identifier for the skill (displayed in UI and used for @-mentions)
* **description**: Brief explanation shown to the model to help it decide when to invoke the skill

Examples of valid names: `deploy-to-staging`, `code-review`, `setup-dev-environment`

## Adding Supporting Resources

Place any supporting files in the skill folder alongside `SKILL.md`. These files become available to Cascade when the skill is invoked:

```
.windsurf/skills/deploy-to-production/
├── SKILL.md
├── deployment-checklist.md
├── rollback-procedure.md
└── config-template.yaml
```

## Invoking Skills

### Automatic Invocation

When your request matches a skill's description, Cascade automatically invokes the skill and uses its instructions and resources to complete the task. This is the most common way skills are used—you simply describe what you want to do, and Cascade determines which skills are relevant.

The `description` field in your skill's frontmatter is key: it helps Cascade understand when to invoke the skill. Write descriptions that clearly explain what the skill does and when it should be used.

### Manual Invocation

You can always explicitly activate a skill by typing `@skill-name` in the Cascade input. This is useful when you want to ensure a specific skill is used, or when you want to invoke a skill that might not be automatically triggered by your request.

## Skill Scopes

| Scope               | Location                      | Availability                                      |
| ------------------- | ----------------------------- | ------------------------------------------------- |
| Workspace           | `.windsurf/skills/`           | Current workspace only. Committed with your repo. |
| Global              | `~/.codeium/windsurf/skills/` | All workspaces on your machine. Not committed.    |
| System (Enterprise) | OS-specific (see below)       | All workspaces, deployed by IT. Read-only.        |

<Note>
  For cross-agent compatibility, Windsurf also discovers skills in `.agents/skills/` and `~/.agents/skills/`. If you have enabled Claude Code config reading, `.claude/skills/` and `~/.claude/skills/` are scanned as well.
</Note>

### System-Level Skills (Enterprise)

Enterprise organizations can deploy skills that are available across all workspaces and cannot be modified by end users:

| OS        | Path                                            |
| --------- | ----------------------------------------------- |
| macOS     | `/Library/Application Support/Windsurf/skills/` |
| Linux/WSL | `/etc/windsurf/skills/`                         |
| Windows   | `C:\ProgramData\Windsurf\skills\`               |

Each skill is a subdirectory containing a `SKILL.md` file, just like workspace skills.

## Example Use Cases

### Deployment Workflow

Create a skill with deployment scripts, environment configs, and rollback procedures:

```
.windsurf/skills/deploy-staging/
├── SKILL.md
├── pre-deploy-checks.sh
├── environment-template.env
└── rollback-steps.md
```

### Code Review Guidelines

Include style guides, security checklists, and review templates:

```
.windsurf/skills/code-review/
├── SKILL.md
├── style-guide.md
├── security-checklist.md
└── review-template.md
```

### Testing Procedures

Bundle test templates, coverage requirements, and CI/CD configs:

```
.windsurf/skills/run-tests/
├── SKILL.md
├── test-template.py
├── coverage-config.json
└── ci-workflow.yaml
```

## Best Practices

1. **Write clear descriptions**: The description helps Cascade decide when to invoke the skill. Be specific about what the skill does and when it should be used.

2. **Include relevant resources**: Templates, checklists, and examples make skills more useful. Think about what files would help someone complete the task.

3. **Use descriptive names**: `deploy-to-staging` is better than `deploy1`. Names should clearly indicate what the skill does.

## Skills vs Rules vs Workflows

All three customize Cascade, but they differ in **structure**, **invocation**, and **context cost**:

|                       | Skills                                                                   | Rules                                              | Workflows                                |
| --------------------- | ------------------------------------------------------------------------ | -------------------------------------------------- | ---------------------------------------- |
| **Purpose**           | Multi-step procedures with supporting files                              | Behavioral guidelines ("how to behave")            | Prompt templates for repeatable tasks    |
| **Structure**         | Folder with `SKILL.md` + any resource files                              | Single `.md` file with frontmatter                 | Single `.md` file                        |
| **Invocation**        | Model decides (progressive disclosure) or `@mention`                     | `always_on` / `glob` / `model_decision` / `manual` | **Manual only** via `/slash-command`     |
| **In system prompt?** | No — only name + description until invoked                               | Depends on activation mode                         | No — listed as available commands        |
| **Best for**          | Deployments, code review, testing procedures that need scripts/templates | Coding style, project conventions, constraints     | One-shot runbooks you trigger explicitly |

**Rule of thumb:** if Cascade should pick it up automatically *and* it needs supporting files, use a Skill. If it's a short behavioral constraint, use a Rule. If you always want to trigger it yourself, use a Workflow.

## Related Documentation

If Skills aren't what you're looking for, check out these other Cascade features:

* **[Workflows](./workflows)** - Automate repetitive tasks with reusable markdown workflows invoked via slash commands
* **[AGENTS.md](./agents-md)** - Provide directory-scoped instructions that automatically apply based on file location
* **[Memories & Rules](./memories)** - Persist context across conversations with auto-generated memories and user-defined rules
