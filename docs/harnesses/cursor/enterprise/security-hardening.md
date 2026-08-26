# Security and Privacy Hardening

This page consolidates the security and privacy guidance spread across the Cursor docs into one reference, so teams configuring Cursor can review and apply the right controls without hunting through every page. Each item links to its source doc for the full detail.

## Shared responsibility

Cursor and your team share responsibility for a secure deployment. Cursor builds, secures, and operates the platform; you decide how to configure and adopt it for your environment. This page focuses on the controls you own. For Cursor's own posture, see the [Trust Center](https://trust.cursor.com/), [Security page](https://cursor.com/security), and [Data Use](https://cursor.com/data-use) policies.

- **Cursor handles** platform security, encryption, infrastructure, certifications, and the contractual commitments documented in the Trust Center.
- **You configure** identity, privacy enforcement, agent controls, extensibility trust, and monitoring, covered in the sections below.
- Layer controls for defense in depth: pair best-effort guardrails (Auto-review, allowlists, `.cursorignore`) with deterministic ones (approvals, hooks, sandboxing) rather than relying on a single layer.
- Most **enforcement** levers here (org-wide policies, MDM, SIEM streaming) are **Enterprise** features set in the [team dashboard](https://cursor.com/docs/account/teams/dashboard.md) or through MDM. Per-user controls such as `.cursorignore` and Run Mode defaults apply more broadly.

## Admin quickstart

Do these first. Each links to its detail page. Other items for regulated orgs, like Cloud Agent retention windows, appear in the tables below.

1. Enforce **[Privacy Mode](https://cursor.com/docs/enterprise/privacy-and-data-governance.md#privacy-mode-enforcement)** org-wide so members can't disable Privacy Mode or its zero data retention guarantees for Cursor-routed models.
2. Set the org **[Run Mode policy](https://cursor.com/docs/agent/security/run-modes.md#run-mode)** to **Auto-review** (not Run Everything) and enable [sandboxing](https://cursor.com/docs/agent/security/run-modes.md#sandboxing).
3. Distribute **[hooks](https://cursor.com/docs/hooks.md#team-distribution)** for enforcement and logging across the team.
4. Apply **[network allowlisting](https://cursor.com/docs/enterprise/network-configuration.md#ip-allowlisting)** and exclude Cursor domains from [SSL inspection](https://cursor.com/docs/enterprise/network-configuration.md#ssl-inspection-and-dlp); set **[Cloud Agent network egress](https://cursor.com/docs/cloud-agent/security-network.md#network-access)** if you use Cloud Agents.
5. Set a **[Rules](https://cursor.com/docs/rules.md#team-rules)** baseline for steering, knowing rules are non-deterministic.
6. Govern **[plugins](https://cursor.com/docs/plugins.md)** and **[MCP servers](https://cursor.com/docs/enterprise/model-and-integration-management.md#mcp-allowlist)** by reviewing what they install and approving trusted sources.
7. Add **[`.cursorignore`](https://cursor.com/docs/reference/ignore-file.md)** entries for secrets and regulated paths.
8. Lock identity with **[SSO](https://cursor.com/docs/account/teams/sso.md)**, **[SCIM](https://cursor.com/docs/account/teams/scim.md)**, and **[Allowed Team IDs](https://cursor.com/docs/enterprise/identity-and-access-management.md#allowed-team-ids)** (MDM); restrict **[extensions](https://cursor.com/docs/enterprise/identity-and-access-management.md#allowed-extensions)**, set an **[install cooldown](https://cursor.com/help/customization/extensions.md#marketplace-install-cooldown)** (and optional [signature verification](https://cursor.com/help/customization/extensions.md#extension-signature-verification)), and keep clients on a [supported version](https://cursor.com/docs/enterprise/deployment-patterns.md#minimum-versions).
9. Decide which **[models](https://cursor.com/docs/enterprise/model-and-integration-management.md#model-access-control)** your organization allows and restrict the rest; restrict **[personal API keys (BYOK)](https://cursor.com/docs/enterprise/model-and-integration-management.md#restrict-personal-api-keys-byok-controls)** if you rely on Cursor's ZDR agreements.
10. Periodically review and stream **[audit logs](https://cursor.com/docs/enterprise/compliance-and-monitoring.md#audit-logs)** to your SIEM.
11. For encryption with your own keys, enable **[CMEK](https://cursor.com/docs/enterprise/privacy-and-data-governance.md#data-encryption)** when your compliance program requires it (embeddings and Cloud Agent data).

## Identity and access

Control who signs in and on which device.

| Control                        | Recommendation                                                                                                                                                                                                  | Learn more                                                                                                                                                                                       |
| :----------------------------- | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **SSO and SCIM**               | Centralize authentication and automate user deprovisioning.                                                                                                                                                     | [SSO](https://cursor.com/docs/account/teams/sso.md), [SCIM](https://cursor.com/docs/account/teams/scim.md)                                                                                       |
| **Allowed Team IDs**           | Block personal accounts on corporate devices via MDM so Privacy Mode always applies.                                                                                                                            | [Identity](https://cursor.com/docs/enterprise/identity-and-access-management.md#allowed-team-ids)                                                                                                |
| **Allowed Extensions**         | Allowlist trusted publishers; any entry blocks the rest unless you add `"*": true`.                                                                                                                             | [Extensions](https://cursor.com/docs/enterprise/identity-and-access-management.md#allowed-extensions)                                                                                            |
| **Extension install cooldown** | Defer extension installs and updates until a marketplace version has been public for a set number of hours (enforced fleet-wide), with optional signature verification, to blunt short-lived malicious uploads. | [Cooldown](https://cursor.com/help/customization/extensions.md#marketplace-install-cooldown), [Signatures](https://cursor.com/help/customization/extensions.md#extension-signature-verification) |
| **Supported version**          | Keep clients current and manage updates with the `UpdateMode` MDM policy.                                                                                                                                       | [Versions](https://cursor.com/docs/enterprise/deployment-patterns.md#minimum-versions)                                                                                                           |
| **Workspace Trust**            | Enforce through MDM so untrusted folders open in restricted mode. Restricted mode limits AI features; use it for truly untrusted trees, not day-to-day repos.                                                   | [Workspace Trust](https://cursor.com/docs/enterprise/identity-and-access-management.md#workspace-trust)                                                                                          |

## Privacy and data

Control how your code and data are handled.

| Control                      | Recommendation                                                                                                                                                                                      | Learn more                                                                                                              |
| :--------------------------- | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :---------------------------------------------------------------------------------------------------------------------- |
| **Privacy Mode**             | Enforce org-wide so members can't disable Privacy Mode or its ZDR commitments for Cursor-routed models; on by default for Enterprise. See exceptions under BYOK and models with provider retention. | [Privacy](https://cursor.com/docs/enterprise/privacy-and-data-governance.md#privacy-mode-enforcement)                   |
| **Personal API keys (BYOK)** | Restrict them; with your own keys, zero data retention is subject to your own agreement with the model provider, not Cursor's.                                                                      | [BYOK](https://cursor.com/docs/enterprise/model-and-integration-management.md#restrict-personal-api-keys-byok-controls) |
| **CMEK**                     | Encrypt embeddings and Cloud Agent data with your own key when your compliance program requires customer-managed keys.                                                                              | [CMEK](https://cursor.com/docs/enterprise/privacy-and-data-governance.md#data-encryption)                               |
| **Model access**             | Approve specific models for use by your organization. Non-ZDR models require admin approval.                                                                                                        | [Models](https://cursor.com/docs/enterprise/model-and-integration-management.md#model-access-control)                   |
| **Repository blocklist**     | Keep sensitive repos out of Cursor entirely.                                                                                                                                                        | [Blocklist](https://cursor.com/docs/enterprise/model-and-integration-management.md#git-repository-blocklist)            |
| **Protected Git Scopes**     | Lock your Git org or namespace so only your teams use those repos with Cloud Agents and Bugbot.                                                                                                     | [Scopes](https://cursor.com/docs/enterprise/model-and-integration-management.md#protected-git-scopes)                   |

Also see [HIPAA BAA](https://cursor.com/docs/enterprise/baa.md) and [Cyber Safeguards](https://cursor.com/docs/account/enterprise/cyber-safeguards.md) when those apply to your deployment.

## Data retention and deletion

You and your users have several ways to manage your data.

| Mechanism                             | What it covers                                                                                                                                           | How                                                                                                                                                                          |
| :------------------------------------ | :------------------------------------------------------------------------------------------------------------------------------------------------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Individual account deletion**       | That user's account and associated data, including indexed codebases; removed within 30 days. Does not by itself complete enterprise tenant offboarding. | Dashboard → Advanced Account Settings → Delete Account ([guide](https://cursor.com/help/account-and-billing/delete-account.md))                                              |
| **Data subject requests**             | Personal Data access, correction, or deletion requests (DSAR).                                                                                           | Email [hi@cursor.com](mailto:hi@cursor.com) to exercise privacy rights ([Privacy Policy](https://cursor.com/privacy))                                                        |
| **Shared chats and canvases**         | Published share links.                                                                                                                                   | Delete from the dashboard ([shared chats](https://cursor.com/dashboard/shared-chats), [shared canvases](https://cursor.com/dashboard/shared-canvases))                       |
| **Cloud Agent deletion**              | An agent's conversation transcript and artifacts, on demand.                                                                                             | [Delete Agent API](https://cursor.com/docs/cloud-agent/api/endpoints.md#delete-an-agent-permanently)                                                                         |
| **Automatic expiry**                  | Indexed codebases (6 weeks inactivity); Cloud Agent snapshots (90 days inactivity).                                                                      | Automatic, no action needed ([indexing](https://cursor.com/docs/agent/tools/search.md), [snapshots](https://cursor.com/docs/cloud-agent/security-network.md#data-retention)) |
| **Enterprise retention windows**      | Cap Cloud Agent data retention (Indefinite or 90 days; custom windows in early access).                                                                  | [Cloud Agent retention](https://cursor.com/docs/cloud-agent/security-network.md#cloud-agent-retention-policies); contact sales                                               |
| **Contract termination (Enterprise)** | Return or deletion of personal data for the enterprise engagement.                                                                                       | Governed by the [DPA](https://cursor.com/terms/dpa); coordinate with your account team                                                                                       |

## Agent runtime and deterministic controls

The hard boundaries on what agents can do. Steering belongs with these, never instead of them.

| Control                        | Recommendation                                                                                                                                                                  | Learn more                                                                                                                                                                                                |
| :----------------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Auto-review (Run Mode)**     | Prefer it over Run Everything; it runs allowlisted calls, sandboxes shell commands when it can, and routes the rest through a best-effort classifier, so combine it with hooks. | [Run Modes](https://cursor.com/docs/agent/security/run-modes.md#run-mode), [Sandboxing](https://cursor.com/docs/agent/security/run-modes.md#sandboxing)                                                   |
| **Network allowlisting**       | Allowlist `*.cursor.sh` and set per-server MCP network policy; exclude Cursor domains from SSL inspection so users don't disable security to "make it work."                    | [Network](https://cursor.com/docs/enterprise/network-configuration.md#ip-allowlisting), [MCP network](https://cursor.com/docs/enterprise/model-and-integration-management.md#per-server-network-controls) |
| **Cloud Agent network egress** | Restrict Cloud Agents' outbound access with Default + allowlist or Allowlist-only modes; Enterprise admins can lock the policy org-wide.                                        | [Cloud Agent network](https://cursor.com/docs/cloud-agent/security-network.md#network-access)                                                                                                             |
| **Cloud Agent OIDC tokens**    | Federate Cursor-managed Cloud Agent VMs into AWS, GCP, Azure, or custom OIDC verifiers with short-lived JWTs instead of long-lived cloud keys.                                  | [OIDC tokens](https://cursor.com/docs/cloud-agent/identity.md)                                                                                                                                            |
| **Private connectivity**       | Reach private source control through PrivateLink or Cloudflare Tunnel, and align Cursor traffic with your endpoint security (AV/EDR/DLP).                                       | [Connectivity](https://cursor.com/docs/cloud-agent/private-connectivity.md), [Endpoint](https://cursor.com/docs/enterprise/endpoint-security.md)                                                          |
| **Hooks**                      | Enforce and observe at agent lifecycle points (block commands, scrub secrets, audit); distribute by MDM or cloud and set `failClosed` for critical hooks.                       | [Hooks](https://cursor.com/docs/hooks.md#team-distribution)                                                                                                                                               |
| **Integrate your own tools**   | Call your SIEM, DLP, allowlist, or policy APIs from hooks instead of relying only on defaults.                                                                                  | [Examples](https://cursor.com/docs/enterprise/llm-safety-and-controls.md#enforcement-hooks), [Partners](https://cursor.com/docs/hooks.md#partner-integrations)                                            |
| **`.cursorignore`**            | Block agent read and context for secrets and regulated trees; terminal and MCP tools can't honor it, so pair with approvals and file permissions.                               | [Ignore files](https://cursor.com/docs/reference/ignore-file.md)                                                                                                                                          |
| **Other protections**          | Keep Browser, File-Deletion, External-File, and `.cursor` directory protection enabled so risky actions still require approval.                                                 | [Protections](https://cursor.com/docs/agent/security/run-modes.md#other-protections), [.cursor](https://cursor.com/docs/enterprise/llm-safety-and-controls.md#cursor-directory-protection)                |

## Steering and extensibility

Guidance and add-ons shape behavior and expand capability. Both are non-deterministic and are trust decisions.

| Control     | Recommendation                                                                                                                        | Learn more                                                                                                                                                                |
| :---------- | :------------------------------------------------------------------------------------------------------------------------------------ | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Rules**   | Steer behavior org-wide with Team Rules, but treat them as suggestions and pair them with the deterministic controls above.           | [Rules](https://cursor.com/docs/rules.md#team-rules)                                                                                                                      |
| **Plugins** | A plugin can bundle MCP servers, skills, subagents, rules, and hooks, so review what it installs and favor private team marketplaces. | [Plugins](https://cursor.com/docs/plugins.md), [Marketplace security](https://cursor.com/help/security-and-privacy/marketplace-security.md)                               |
| **MCP**     | Approve servers with the allowlist, restrict per-server tools, and apply network modes; review each server before enabling.           | [MCP allowlist](https://cursor.com/docs/enterprise/model-and-integration-management.md#mcp-allowlist), [Security](https://cursor.com/docs/mcp.md#security-considerations) |

## Monitor and respond

Review output, verify controls, and keep an audit trail.

| Practice                   | Recommendation                                                                                    | Learn more                                                                                                          |
| :------------------------- | :------------------------------------------------------------------------------------------------ | :------------------------------------------------------------------------------------------------------------------ |
| **Pre-production review**  | Have Bugbot and Security Agents review Cursor-generated code before it ships to production.       | [Bugbot](https://cursor.com/docs/bugbot.md), [Security Agents](https://cursor.com/docs/security-agents.md)          |
| **Audit logs**             | Periodically review them and stream to SIEM, webhooks, or S3 for authentication and admin events. | [Compliance](https://cursor.com/docs/enterprise/compliance-and-monitoring.md#audit-logs)                            |
| **Compliance logging**     | Use hooks to capture development-activity metadata beyond Cursor's audit logs.                    | [Hooks logging](https://cursor.com/docs/enterprise/compliance-and-monitoring.md#using-hooks-for-compliance-logging) |
| **Responsible disclosure** | Report vulnerabilities to [security-reports@cursor.com](mailto:security-reports@cursor.com).      | [Disclosure](https://cursor.com/docs/agent/security.md#responsible-disclosure)                                      |

## Further reading

Background and product context. The docs linked throughout this page are the authority for each control.

- [Governing agent autonomy with Auto-review](https://cursor.com/blog/agent-autonomy-auto-review)
- [Implementing a secure sandbox for local agents](https://cursor.com/blog/agent-sandboxing)
- [Hooks for security and platform teams](https://cursor.com/blog/hooks-partners)

## Platform commitments

The controls on this page describe how to configure your own environment. They complement Cursor's platform security and contractual commitments:

- [Trust Center](https://trust.cursor.com/) for certifications, security architecture, and subprocessors
- [Master Services Agreement](https://cursor.com/terms/msa) and [Data Processing Agreement](https://cursor.com/terms/dpa) for contractual and data-protection terms

Subscribe in the [Trust Center](https://trust.cursor.com/) to get notified when [subprocessors](https://trust.cursor.com/subprocessors) or Cursor's security posture change. Cursor sends a confirmation email to the address you enter, and you must verify it before updates start arriving.

### Harden Cursor for your organization

Contact our team to enable org-wide enforcement, CMEK, and SIEM streaming.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
