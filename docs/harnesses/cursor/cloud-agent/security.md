# Security overview

This page explains how Cloud Agents are built and secured. It walks through what happens when an agent runs, how access is granted, where the code and data live, how they're isolated and encrypted, and what controls you have over each stage. It answers the questions that come up when a team evaluates Cloud Agents against its security requirements.

For the configuration reference, including secret types, network access modes, and egress IP ranges, see [Secrets & Network](https://cursor.com/docs/cloud-agent/security-network.md). For federating a VM into AWS, GCP, Azure, or a custom verifier without long-lived keys, see [OIDC tokens](https://cursor.com/docs/cloud-agent/identity.md). This page explains the model behind those controls; those pages tell you how to set them.

This is a companion to Cursor's broader security material. See the [Trust Center](https://trust.cursor.com/) for certifications, subprocessors, and architecture, and the [Security and Privacy Hardening](https://cursor.com/docs/enterprise/security-hardening.md) reference for the controls you own across all of Cursor. Cursor is SOC 2 Type 2 compliant and commits to at-least-annual penetration testing by reputable third parties.

## How Cloud Agents work

A Cloud Agent is a coding agent that runs in a virtual machine in Cursor's cloud instead of on a developer's laptop. The VM holds a full development environment: the cloned repository, installed dependencies, configured secrets, and network access.

A single run moves through these stages:

1. **Start.** A user or integration starts a task from the web app, IDE, CLI, API, Slack, or a linked issue or pull request.
2. **Provision.** Cursor provisions an isolated VM for that agent and clones the authorized repository into it.
3. **Run.** The agent runs code and tools inside the VM and streams its progress, output, and artifacts back to the user.
4. **Persist.** Conversation state, metadata, and artifacts are saved to Cursor-managed storage so you can review and resume the run.
5. **Hand off.** The agent pushes its branch and opens a draft pull request for a human to review before anything merges.
6. **Recycle.** VM runtime resources are hibernated and then deleted on lifecycle timers once the run is idle.

## Access and authorization

Cloud Agents reach your code through the Cursor GitHub or GitLab App, not through any single person's credentials.

- **Admins install the app.** Enabling Cloud Agents takes admin privileges on both Cursor and your Git provider. An admin installs the Cursor app on your Git organization and grants access only to the repositories you choose.
- **Users connect their own account.** Once the app is installed, each user who wants to start an agent connects their own Git account. This is a second, per-user layer on top of the org-level install.
- **Access is inherited, never widened.** A Cloud Agent can only reach repositories the triggering user could already reach. Starting an agent never grants access to a repository the user didn't already have.

Team admins can go further and lock a Git organization to your Cursor organization with [Protected Git Scopes](https://cursor.com/docs/enterprise/model-and-integration-management.md#protected-git-scopes), so only your teams can start Cloud Agents on its repositories. You can also keep sensitive repositories out entirely with a [repository blocklist](https://cursor.com/docs/enterprise/model-and-integration-management.md#git-repository-blocklist).

Cursor employees do not have access to the code inside Cloud Agent VMs. Access attempts are monitored by Cursor's security team.

## Isolation and infrastructure

Each agent runs in its own VM boundary, not a shared process sandbox. One agent cannot see another agent's code, environment, or state.

- **Per-agent VMs.** Every agent gets a dedicated environment, isolated from other agents and other users.
- **MicroVM isolation.** Runtime workspaces run on Firecracker-based microVM infrastructure.
- **Account-level separation.** Cloud Agent VMs run in a separate AWS account from the rest of Cursor's production infrastructure, so the code-execution environment is walled off from Cursor's other services.

## Encryption

Cursor encrypts Cloud Agent data in transit and at rest.

- **In transit.** TLS 1.2 or higher for service-to-service and client-to-service traffic.
- **At rest.** AES-256, with per-agent keys so each agent's session data is encrypted under its own key.
- **Customer-managed keys.** Enterprise teams can map a customer-managed KMS key (CMEK/BYOK) for Cloud Agent server-side encryption, so you control key rotation and access. See [Data encryption](https://cursor.com/docs/enterprise/privacy-and-data-governance.md#data-encryption).

## What data is stored, where, and for how long

A Cloud Agent touches four kinds of data. Each is stored in a different place and follows its own retention rule.

| Data                   | What it holds                                                                                          | Where it lives                                            | Retention                                                                                           |
| :--------------------- | :----------------------------------------------------------------------------------------------------- | :-------------------------------------------------------- | :-------------------------------------------------------------------------------------------------- |
| **Runtime workspace**  | The checked-out repository, build artifacts, and tool-execution context for a live run                 | The isolated Cloud Agent VM                               | Recycled automatically after the run goes idle; the timer refreshes when you send follow-up prompts |
| **VM snapshots**       | Point-in-time copies of the VM disk (including cloned code) used to start and resume without recloning | Snapshot and cache layer outside the active VM, encrypted | Rolling 90 days of inactivity; each start or resume extends it, then automatic deletion             |
| **Conversation state** | Prompts, model responses, tool calls, diff context, and demo artifacts that make up the transcript     | Cursor backend, encrypted with per-agent keys             | Kept indefinitely by default so you can revisit and resume runs; deletable on demand                |
| **Secrets and tokens** | Cloud Agent secrets, OAuth tokens, and API credentials you configure                                   | Encrypted credential stores in Cursor's backend           | Kept until you delete or remove them                                                                |

The [Delete Agent API](https://cursor.com/docs/cloud-agent/api/endpoints.md#delete-an-agent-permanently) removes an agent's conversation transcript and artifacts on demand. Snapshots can't be deleted on demand; they follow the 90-day inactivity window above. Enterprise teams can also cap conversation retention with [retention policies](https://cursor.com/docs/cloud-agent/security-network.md#cloud-agent-retention-policies). For the full detail on retention and deletion, see [Data retention](https://cursor.com/docs/cloud-agent/security-network.md#data-retention).

## Privacy and model data

Cloud Agents run in [Privacy Mode](https://cursor.com/privacy-overview). With Privacy Mode on, Cursor never trains on code accessed by Cloud Agents or on the prompts and responses their runs generate. Most models also run under Cursor's zero-data-retention agreements, so providers don't store or train on requests and responses. See [Privacy and Data Governance](https://cursor.com/docs/enterprise/privacy-and-data-governance.md) for the model-by-model detail and the exceptions.

Legacy Privacy Mode is not supported for Cloud Agents, because agents need to store code and environment data in the cloud while they run. Enforce standard Privacy Mode org-wide so every run inherits its zero-data-retention guarantees.

## Autonomy and prompt injection

Cloud Agents auto-run terminal commands so they can iterate on tests without stopping for approval on every step. This is more autonomous than the foreground agent, and it changes the risk model: an attacker who plants instructions in content the agent reads (a prompt-injection attack) could try to make the agent exfiltrate code to an external host. See [OpenAI's explanation of prompt-injection risk for cloud agents](https://platform.openai.com/docs/codex/agent-network#risks-of-agent-internet-access).

The layers that contain this risk:

- **Network egress controls.** Restrict outbound traffic to a default set plus your allowlist, or to your allowlist only, so a compromised agent has nowhere to send data. Enterprise admins can lock the policy org-wide. See [Network access](https://cursor.com/docs/cloud-agent/security-network.md#network-access).
- **Redacted runtime secrets.** Mark secrets as [Runtime Secrets](https://cursor.com/docs/cloud-agent/security-network.md#runtime-secrets) so their values are stripped from the transcript, tool output, and commits and never reach the model.
- **File exclusion.** Add sensitive paths to [`.cursorignore`](https://cursor.com/docs/reference/ignore-file.md) to keep them out of the agent's context.
- **Human-in-the-loop handoff.** Agents open draft pull requests. Nothing merges until a person reviews the change.
- **Signed commits.** Every agent commit is signed with an HSM-backed Ed25519 key and shows a "Verified" badge, so agent-authored changes are attributable and can satisfy signed-commit branch protection. See [Signed commits](https://cursor.com/docs/cloud-agent/security-network.md#signed-commits).

For deeper defense, pair these with [hooks](https://cursor.com/docs/hooks.md) to enforce policy and log activity at agent lifecycle points, and have [Bugbot](https://cursor.com/docs/bugbot.md) or [Security Agents](https://cursor.com/docs/security-agents.md) review agent output before it ships.

## Risk considerations

| Risk                                | Mitigation                                                                                                                                                                                                                    |
| :---------------------------------- | :---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Full codebase in the cloud**      | Isolated per-agent VMs, AES-256 encryption, and automatic VM and snapshot deletion on lifecycle timers.                                                                                                                       |
| **Third-party or insider access**   | No Cursor employee access to code in agent VMs, with monitored access attempts. VMs run in a separate AWS account from other Cursor services.                                                                                 |
| **Agent autonomy**                  | Scope bounded by the repository and the triggering user's access. External reach is limited to configured tools and terminal commands, gated by network egress controls and reviewed through draft PRs.                       |
| **Network access and exfiltration** | Internet access is on by default but can be restricted to allowlisted domains, down to allowlist-only, and locked org-wide.                                                                                                   |
| **Secret exposure**                 | Encrypted secret storage, redacted runtime secrets kept out of the model, build-only secrets scoped to the Docker build, and [OIDC tokens](https://cursor.com/docs/cloud-agent/identity.md) for short-lived cloud federation. |

## Auditability

Cloud Agent activity is logged and attributable.

- **Session logging.** Runs are logged, and team admins can review activity from the [Cloud Agents dashboard](https://cursor.com/dashboard/cloud-agents).
- **Attributed changes.** Every commit and pull request an agent creates is attributed and visible in your Git history, with signed, verified commits.
- **Audit logs.** Authentication and admin events flow to your [audit logs](https://cursor.com/docs/enterprise/compliance-and-monitoring.md#audit-logs), which Enterprise teams can stream to a SIEM, webhook, or S3.
- **Run diagnostics.** The built-in [Cursor Cloud MCP](https://cursor.com/docs/cloud-agent/capabilities.md#cursor-cloud-mcp) exposes transcripts, run events, environment details, and setup logs for a run.

## Data deletion

| Mechanism                         | What it removes                                  | How                                                                                                          |
| :-------------------------------- | :----------------------------------------------- | :----------------------------------------------------------------------------------------------------------- |
| **Archive**                       | Hides an agent from the dashboard                | Archive from the [dashboard](https://cursor.com/dashboard/cloud-agents)                                      |
| **Delete Agent API**              | An agent's conversation transcript and artifacts | [Delete Agent API](https://cursor.com/docs/cloud-agent/api/endpoints.md#delete-an-agent-permanently)         |
| **Snapshot expiry**               | VM snapshots and cached code                     | Automatic after 90 days of inactivity                                                                        |
| **Retention policy (Enterprise)** | Conversations older than your chosen window      | [Retention policies](https://cursor.com/docs/cloud-agent/security-network.md#cloud-agent-retention-policies) |
| **Account deletion**              | The account and its associated data              | [Delete account](https://cursor.com/help/account-and-billing/delete-account.md)                              |

## FAQ

### Are Cloud Agents less secure than local agents?

They carry a different risk profile, not a worse one. Running an agent unattended in an isolated sandbox with egress restrictions and minimal permissions can be tighter than a developer's laptop, which usually has full internet access and elevated privileges.

### Does Cursor store an entire repository permanently?

No. Cursor clones the repository to run the agent, and that clone can live in VM snapshots to speed up future starts, but it isn't kept indefinitely. Snapshots are deleted after 90 days of inactivity.

### Can a Cloud Agent access repositories the developer can't?

No. Access is gated by the triggering developer's Git access. A Cloud Agent can't reach a repository the developer didn't already have access to.

### Can we restrict which repositories Cloud Agents reach?

Yes. Cloud Agents can only reach repositories you authorize through your Git provider connection. You control which repositories are available, and admins can lock scopes with [Protected Git Scopes](https://cursor.com/docs/enterprise/model-and-integration-management.md#protected-git-scopes) or exclude repositories with a [blocklist](https://cursor.com/docs/enterprise/model-and-integration-management.md#git-repository-blocklist).

### Can a Cloud Agent read secrets and credentials?

Configure secrets through the Secrets tab in your dashboard. They're encrypted at rest with KMS, encrypted in transit, and injected as environment variables at runtime. Mark sensitive values as [Runtime Secrets](https://cursor.com/docs/cloud-agent/security-network.md#runtime-secrets) to keep them out of the transcript, tool output, and commits. As a matter of practice, keep secrets out of the repository; if sensitive files must live there, add them to [`.cursorignore`](https://cursor.com/docs/reference/ignore-file.md). For cloud roles, mint [OIDC tokens](https://cursor.com/docs/cloud-agent/identity.md) from the VM instead of storing long-lived access keys.

### Can we audit Cloud Agent activity?

Yes. Sessions are logged, admins can review activity from the dashboard, and every commit and pull request an agent creates is attributed in your Git history. Enterprise teams can stream audit logs to a SIEM.

### How is data deleted?

Archive an agent from the dashboard, or use the [Delete Agent API](https://cursor.com/docs/cloud-agent/api/endpoints.md#delete-an-agent-permanently) to remove its transcript and artifacts. Full account deletion and Enterprise retention policies remove data on a broader schedule.

## Related pages

- [Secrets & Network](https://cursor.com/docs/cloud-agent/security-network.md) for secret types, network access modes, egress IP ranges, and signed commits.
- [OIDC tokens](https://cursor.com/docs/cloud-agent/identity.md) for short-lived JWTs and cloud federation.
- [Privacy and Data Governance](https://cursor.com/docs/enterprise/privacy-and-data-governance.md) for data flows, Privacy Mode, and encryption.
- [Security and Privacy Hardening](https://cursor.com/docs/enterprise/security-hardening.md) for the controls you configure across Cursor.
- [Trust Center](https://trust.cursor.com/) for certifications, subprocessors, and architecture.

This summary aids understanding and does not override the [MSA](https://cursor.com/terms/msa), [DPA](https://cursor.com/terms/dpa), or other binding contractual terms.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
