# PR Routing & Approval

PR Routing & Approval routes pull requests to the right reviewers and can approve low-risk changes.

Configure PR Routing & Approval in [Automations](https://cursor.com/automations/from-cursor/pr-routing-and-approval).

## How it works

PR Routing & Approval runs on your pull requests. It assigns reviewers based on code ownership and commit history, and can approve low-risk PRs when your criteria are met.

It does not replace a full code review. It uses risk scoring, approval policy files, AI review agent findings, and your configuration to decide whether to route or approve.

## Core capabilities

### Reviewer assignment

PR Routing & Approval can assign reviewers to pull requests based on code ownership and commit history.

Use reviewer assignment to describe when the agent should request reviewers. The agent also considers applicable policy files, risk settings, AI reviewer findings, and the current review state.

### Risk-based approval

PR Routing & Approval can approve low-risk pull requests when your approval criteria are met.

Use approval criteria to describe the conditions a PR must meet before the agent approves it. The agent also considers applicable policy files, risk settings, AI reviewer findings, and the current review state.

## Core features

### AI reviewer awareness

PR Routing & Approval can use findings from other Cursor review systems:

- **Bugbot Review Context** uses Bugbot findings in the approval decision.
- **Security Review Context** uses Security Agent findings in the approval decision.

When these contexts are enabled, the agent waits for the relevant agentic reviewer checks to finish and uses their findings as approval signals.

If Bugbot or Security Agents report findings that need human review, PR Routing & Approval will not approve the PR.

Security Agents require a team or enterprise plan.

### Risk scoring

PR Routing & Approval can classify a PR by risk and enforce a maximum approval threshold.

- **Use Risk Score** enables risk classification which can be customized further with prompting.
- **Maximum Risk Threshold** sets the highest risk level the agent may approve.

If a PR exceeds the configured threshold, the agent will not approve it.

### Approval policy files

PR Routing & Approval can discover repository policy files and apply them before deciding whether to approve.

For each changed file, the agent checks the file's directory and each ancestor directory for this exact filename:

```text
APPROVAL_POLICY.md
```

Only exact basename matches are trusted. Files such as `POLICY.md`, `approval_policy.md`, `APPROVAL_POLICY.md.bak`, and `team_APPROVAL_POLICY.md` are ignored during directory policy discovery.

The closest applicable `APPROVAL_POLICY.md` has the highest priority for files under that directory. Ancestor policies still apply unless they conflict with a more specific policy.

### Routing policies

PR Routing & Approval also checks for a top-level routing file:

```text
.cursor/approval-policies/ROUTING.md
```

`ROUTING.md` is a YAML list of product entries. Each entry contains:

- `product`: the product or area name.
- `boundary`: a semantic boundary or explicit repository-relative path or glob.
- `policies`: policy prompt pointers, either explicit file paths or semantic descriptions.

If `ROUTING.md` is missing, directory-based `APPROVAL_POLICY.md` discovery still runs. Missing routing does not weaken policy discovery.

### Policy precedence

Applicable approval policy prompts override generic approval criteria, risk thresholds, reviewer-selection guidance, custom approval instructions, and the default automated-review posture.

If policies conflict, the agent follows the most specific policy. If specificity is unclear, it follows the stricter instruction and avoids auto-approval.

If a PR changes an approval policy, routing file, routed policy file, or reviewer-specific policy file, the agent does not use the changed content to relax review requirements for that same PR. It uses the base-branch version when available, or requires human review when the base version cannot be determined.

## Setup

Open [PR Routing & Approval in Automations](https://cursor.com/automations/from-cursor/pr-routing-and-approval) to configure it.

### Enable routing and approval

Turn on the capabilities you want:

- **Enable PR Routing and Requests for Review** assigns reviewers based on code ownership and commit history.
- **Automatically Approve PRs** approves low-risk PRs after you configure approval criteria.

Choose the organizations and repositories where the agent should run.

### Configure triggers

Triggers decide when the agent runs. PR Routing & Approval supports pull request events such as:

- **PR opened** runs the agent when a pull request is created.
- **PR pushed / updated** runs the agent when new commits are pushed to an existing PR.
- **PR commented** runs the agent when a comment matching a regex is posted on an existing PR.

Triggers can be scoped to repositories or organizations. For team-owned repositories, team admins can configure broader team scopes.

### Configure review signals

In **Configuration**, choose which signals the agent should use:

- **Use Bugbot Review Context**
- **Use Security Review Context**
- **Use Risk Score**
- **Maximum Risk for Approval**

Use these signals to decide whether the agent should rely on AI reviewer output, security findings, and risk thresholds before approving.

### Customize approval rules

Use the **Custom Prompt** to add approval criteria for your team. You can describe local review expectations, examples of PRs that are safe to approve, or cases that require human review.

Policy files still take precedence over the custom prompt for applicable files.

If the custom prompt is not set, the agent will use the default Cursor managed criteria.

### Configure tools and MCPs

The agent must have at least one primary action enabled:

- **Request Reviewers**
- **Approve PR**

Optional integrations can include:

- Slack notifications.
- Microsoft Teams notifications.
- MCP servers for additional tool access.

Use the custom prompt to guide how the agent should use MCP tools.

### Save and enable

After configuring, save the agent. Existing agents can be enabled or disabled from the detail page.

Team members without admin permission can view PR Routing & Approval but cannot edit it.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
