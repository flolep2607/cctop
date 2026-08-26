# Enterprise

Cursor provides enterprise-grade security, compliance, and administrative controls for organizations deploying AI-assisted development at scale.

If you are rolling out multiple linked teams, start with [Organizations](https://cursor.com/docs/enterprise/organizations.md).

## Security and compliance resources

For security reviews and compliance assessments, start with these resources:

- [Trust Center](https://trust.cursor.com/) - Security practices, certifications, and compliance information
- [Security page](https://cursor.com/security) - Detailed security architecture and controls
- [Privacy Overview](https://cursor.com/privacy-overview) - Data handling and privacy guarantees
- [Data Processing Agreement](https://cursor.com/terms/dpa) - GDPR-compliant DPA with data protection commitments

Our certifications include SOC2 Type II, and we maintain GDPR compliance. Visit the [Trust Center](https://trust.cursor.com/) for the latest certification documents and third-party assessment reports.

## Enterprise documentation

Learn how to deploy, configure, and manage Cursor for your organization. This documentation covers:

- [Admin Setup Guide](https://cursor.com/docs/enterprise/admin-setup-guide.md) - Short checklist of what admins should plan when rolling out Cursor
- [Security and Privacy Hardening](https://cursor.com/docs/enterprise/security-hardening.md) - One-page checklist of security and privacy controls with links to configure each one
- [Organizations](https://cursor.com/docs/enterprise/organizations.md) - Org-wide team membership sync and organization groups
- [Identity & access](https://cursor.com/docs/enterprise/identity-and-access-management.md) - SSO, SCIM, RBAC, and MDM policies
- [Privacy & data governance](https://cursor.com/docs/enterprise/privacy-and-data-governance.md) - Data flows, Privacy Mode, and data residency
- [Network configuration](https://cursor.com/docs/enterprise/network-configuration.md) - Proxy setup, IP allowlisting, and encryption
- [Private connectivity](https://cursor.com/docs/cloud-agent/private-connectivity.md) - AWS PrivateLink and Cloudflare Tunnel for private source control access
- [Endpoint security](https://cursor.com/docs/enterprise/endpoint-security.md) - Configure antivirus, EDR, and DLP software
- [LLM safety & controls](https://cursor.com/docs/enterprise/llm-safety-and-controls.md) - Hooks, terminal sandboxing, and agent controls
- [Models & integrations](https://cursor.com/docs/enterprise/model-and-integration-management.md) - Model controls, MCP, and third-party integrations
- [Cyber Safeguards](https://cursor.com/docs/account/enterprise/cyber-safeguards.md) - Apply for Anthropic's Cyber Verification Program (CVP) to use eligible Claude models without cyber safeguards
- [Spend Limits](https://cursor.com/help/account-and-billing/spend-limits.md) - Configure spending limits to control costs
- [Enterprise help articles](https://cursor.com/help/account-and-billing/enterprise.md) - Common questions about enterprise plans
- [Compliance & monitoring](https://cursor.com/docs/enterprise/compliance-and-monitoring.md) - Audit logs and tracking
- [OpenTelemetry Export](https://cursor.com/docs/enterprise/opentelemetry-export.md) - Usage metrics and logs delivered to your observability stack over OTLP
- [HIPAA Business Associate Agreements](https://cursor.com/docs/enterprise/baa.md) - Request BAA support for Enterprise customers
- [Deployment patterns](https://cursor.com/docs/enterprise/deployment-patterns.md) - MDM-managed editor vs self-hosted CLI

## Key features

### Identity and access

- [SSO and SAML](https://cursor.com/docs/account/teams/sso.md) - Single sign-on for streamlined authentication
- [SCIM](https://cursor.com/docs/account/teams/scim.md) - Automated user provisioning and deprovisioning
- [MDM policies](https://cursor.com/docs/enterprise/identity-and-access-management.md#mdm-policies) - Enforce allowed team IDs and extensions on user devices

### Privacy and security

- [Privacy Mode](https://cursor.com/privacy-overview) - No training on your data by Cursor or other AI providers
- [Agent Security](https://cursor.com/docs/agent/security.md) - Guardrails for agent tool execution
- [Hooks](https://cursor.com/docs/hooks.md) - Custom security and compliance workflows

### Administrative controls

- [Dashboard](https://cursor.com/docs/account/teams/dashboard.md) - Team management, settings, and monitoring
- [Admin API](https://cursor.com/docs/account/teams/admin-api.md) - Programmatic access to admin features
- [Analytics](https://cursor.com/docs/account/teams/analytics.md) - Usage metrics and insights
- [Conversation Insights](https://cursor.com/docs/account/teams/analytics.md#conversation-insights) - Understand the type of work being done with Cursor (Enterprise only)
- [AI Code Tracking API](https://cursor.com/docs/account/teams/ai-code-tracking-api.md) - Per-commit AI usage metrics (Enterprise only)
- [Cursor Blame](https://cursor.com/docs/integrations/cursor-blame.md) - AI-aware git blame that shows AI vs human code attribution (Enterprise only)
- [Analytics API](https://cursor.com/docs/account/teams/analytics-api.md) - Usage metrics and insights
- [Billing Groups](https://cursor.com/docs/account/enterprise/billing-groups.md) - Manage spend across groups of users for reporting and chargebacks (Enterprise only)
- [Service Accounts](https://cursor.com/docs/account/enterprise/service-accounts.md) - Non-human accounts for automated workflows (Enterprise only)

### Models and integrations

- [Models](https://cursor.com/docs/models-and-pricing.md) - Available models and configuration
- [Cyber Safeguards](https://cursor.com/docs/account/enterprise/cyber-safeguards.md) - Anthropic Cyber Verification Program (CVP) access for security groups (Enterprise only)
- [MCP](https://cursor.com/docs/mcp.md) - Model Context Protocol server trust management
- [Slack](https://cursor.com/docs/integrations/slack.md) - Cloud Agents in Slack
- [GitHub](https://cursor.com/docs/integrations/github.md) - Repository integration
- [Linear](https://cursor.com/docs/integrations/linear.md) - Issue tracking integration
- [Bugbot](https://cursor.com/docs/bugbot.md) - Automated bug detection and fixing

### Monitoring and compliance

- Audit logs - Track authentication, user management, and administrative actions (Enterprise only)
- SIEM integration - Stream audit logs to your security tools
- [OpenTelemetry Export](https://cursor.com/docs/enterprise/opentelemetry-export.md) - Stream usage metrics and logs to your observability stack over OTLP (Enterprise only)
- [HIPAA Business Associate Agreements](https://cursor.com/docs/enterprise/baa.md) - BAA support for Enterprise customers

## Getting started

Start with the [Admin Setup Guide](https://cursor.com/docs/enterprise/admin-setup-guide.md) for a short rollout checklist. In short:

1. Review the [Trust Center](https://trust.cursor.com/) and [Security page](https://cursor.com/security) for your security assessment
2. Read through the [enterprise documentation](https://cursor.com/docs/enterprise.md) to understand deployment options
3. Set up [SSO](https://cursor.com/docs/account/teams/sso.md) and [SCIM](https://cursor.com/docs/account/teams/scim.md) for user management
4. Deploy Cursor and configure [MDM policies](https://cursor.com/docs/enterprise/deployment-patterns.md#mdm-configuration) to enforce team IDs and extensions
5. Review the [Dashboard](https://cursor.com/docs/account/teams/dashboard.md) to monitor team usage

## Plan Comparison

### Team Admin & Billing

| Capability                                                                                                          | Individual Plans | Teams                                                                     | Enterprise                                                                                                                                                                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------- | ---------------- | ------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Centralized Billing                                                                                                 |                  | ✓                                                                         | ✓                                                                                                                                                                                                                                                                 |
| Usage Spend Controls                                                                                                | Personal limits  | Team limits                                                               | [Pooled usage + admin-only controls](https://cursor.com/help/account-and-billing/spend-limits.md#what-types-of-spend-limits-are-available)                                                                                                                        |
| [Billing Groups](https://cursor.com/docs/account/enterprise/billing-groups.md)                                      |                  |                                                                           | ✓                                                                                                                                                                                                                                                                 |
| [Team Usage Analytics](https://cursor.com/docs/account/teams/analytics.md)                                          |                  | [Analytics Dashboard](https://cursor.com/docs/account/teams/analytics.md) | [Analytics Dashboard](https://cursor.com/docs/account/teams/analytics.md),[AI Code Tracking API](https://cursor.com/docs/account/teams/ai-code-tracking-api.md),[Conversation Insights](https://cursor.com/docs/account/teams/analytics.md#conversation-insights) |
| [Cursor Blame](https://cursor.com/docs/integrations/cursor-blame.md)                                                |                  |                                                                           | ✓                                                                                                                                                                                                                                                                 |
| [SSO (SAML/OIDC)](https://cursor.com/docs/enterprise/identity-and-access-management.md#single-sign-on-sso-and-saml) |                  | ✓                                                                         | ✓                                                                                                                                                                                                                                                                 |
| [SCIM Provisioning](https://cursor.com/docs/account/teams/scim.md)                                                  |                  |                                                                           | ✓                                                                                                                                                                                                                                                                 |
| [Audit Logs](https://cursor.com/docs/enterprise/compliance-and-monitoring.md#audit-logs)                            |                  |                                                                           | ✓                                                                                                                                                                                                                                                                 |
| [Service Accounts](https://cursor.com/docs/account/enterprise/service-accounts.md)                                  |                  |                                                                           | ✓                                                                                                                                                                                                                                                                 |

### Marketplace

| Capability                        | Individual Plans | Teams                 | Enterprise                                                                                                         |
| --------------------------------- | ---------------- | --------------------- | ------------------------------------------------------------------------------------------------------------------ |
| Team marketplaces                 |                  | 1 team marketplace    | Unlimited team marketplaces                                                                                        |
| Community plugin import           |                  | On by default         | Off by default                                                                                                     |
| Marketplace edits                 |                  | All teams can edit    | Only admin edits                                                                                                   |
| SCIM distribution & access gating |                  | No SCIM access gating | Scope distribution and gate access via SCIM (control who sees which marketplace based on identity provider groups) |

### Centralized Agent Controls

| Capability                                                                                                              | Individual Plans | Teams                  | Enterprise                                                                           |
| ----------------------------------------------------------------------------------------------------------------------- | ---------------- | ---------------------- | ------------------------------------------------------------------------------------ |
| [Privacy Mode](https://cursor.com/docs/enterprise/privacy-and-data-governance.md#privacy-mode-enforcement)              | User choice      | Enforce org-wide       | Enforce org-wide                                                                     |
| [Team Rules](https://cursor.com/docs/rules.md#team-rules)                                                               |                  | Enforceable + Optional | Enforceable + Optional                                                               |
| [Hooks for Logging,Auditing, and more](https://cursor.com/docs/hooks.md)                                                | ✓                | MDM Distribution       | [MDM & Server-side distribution](https://cursor.com/docs/hooks.md#team-distribution) |
| [Agent Sandbox Mode](https://cursor.com/docs/agent/security/run-modes.md#sandboxing)                                    | ✓                | ✓                      | Enforce org-wide                                                                     |
| [Repository Blocklist](https://cursor.com/docs/enterprise/model-and-integration-management.md#git-repository-blocklist) |                  |                        | ✓                                                                                    |
| [Model Access Restrictions](https://cursor.com/docs/enterprise/model-and-integration-management.md)                     |                  |                        | ✓                                                                                    |
| [Auto-run, Browser, and Network Controls](https://cursor.com/docs/enterprise/llm-safety-and-controls.md)                |                  |                        | ✓                                                                                    |

### User Access Controls

| Capability   | Individual & Teams Plans | Enterprise                                     |
| ------------ | ------------------------ | ---------------------------------------------- |
| Cursor CLI   |                          | Restrict which users can access agents via CLI |
| Cloud Agents |                          | Restrict which users can create Cloud Agents   |
| Analytics    |                          | Restrict analytics dashboard to admins only    |
| BYOK         |                          | Disable users from using their own API keys    |

### Support & Legal

| Capability        | Individual Plans                                          | Teams                                                     | Enterprise                                                          |
| ----------------- | --------------------------------------------------------- | --------------------------------------------------------- | ------------------------------------------------------------------- |
| Technical Support | [Community & Standard Support](https://forum.cursor.com/) | [Community & Standard Support](https://forum.cursor.com/) | First human response times: 8 hours (critical), 24 hours (standard) |
| Terms             | [Online Terms](https://cursor.com/terms-of-service)       | [MSA & DPA](https://cursor.com/terms/msa)                 | [MSA & DPA](https://cursor.com/terms/msa)                           |

For security vulnerabilities, see our [responsible disclosure program](https://cursor.com/docs/agent/security.md#responsible-disclosure).

### Ready to deploy Cursor at scale?

Contact our team to discuss your organization's needs.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
