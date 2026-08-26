# Admin Setup Guide

Cursor is designed with enterprise rollouts in mind. On the [Cursor Admin Dashboard](https://cursor.com/dashboard), you'll find everything you need to manage Cursor usage for your organization.

Cursor is highly configurable. We recommend exploring the full [Enterprise Docs](https://cursor.com/docs/enterprise.md) for a comprehensive understanding of how you can govern and safeguard your team's Cursor usage.

This page serves as a brief digest of key actions we recommend taking to set up your Org in Cursor.

## Understand Cursor's security processes & certifications

- [Review the Trust Center](https://trust.cursor.com/) for information on audits, certifications, policies and subprocessors
- Read [Cursor's security brief](https://cursor.com/security)

## Set up identity and access

1. Complete [SSO](https://cursor.com/docs/account/teams/sso.md) with Okta, Entra, or any SAML 2.0 provider
2. Optionally enable [SCIM](https://cursor.com/docs/account/teams/scim.md) for automated provisioning
3. Optionally [configure Organizations, Teams & Groups](https://cursor.com/docs/enterprise/organizations.md) if further organization beyond a single Team is necessary. We recommend consulting with your Cursor account team for guidance on the ideal org setup for your needs.

## Connect your Git platform

Before anyone can start a Cloud Agent from a repository, a Cursor account admin needs to connect source control for the account. Set up [GitHub (Cloud and Enterprise Server)](https://cursor.com/docs/integrations/github.md), [GitLab (Cloud and Self-Hosted)](https://cursor.com/docs/integrations/gitlab.md), [Bitbucket Cloud](https://cursor.com/docs/integrations/bitbucket.md), or [Azure DevOps](https://cursor.com/docs/integrations/azure-devops.md).

## Deploy and monitor

1. Deploy Cursor and configure [MDM policies](https://cursor.com/docs/enterprise/deployment-patterns.md#mdm-configuration) for managed devices and extensions
2. [Configure AllowedTeamIDs via MDM](https://cursor.com/docs/enterprise/identity-and-access-management.md#allowed-team-ids), to restrict personal Cursor account use on company devices
3. Use the [Dashboard](https://cursor.com/docs/account/teams/dashboard.md) to monitor usage and set alerts
4. For a deeper security pass, follow [Security and Privacy Hardening](https://cursor.com/docs/enterprise/security-hardening.md)

## Enable suggested features for your Team

- Enable [browser automation](https://cursor.com/docs/agent/tools/browser.md#enabling-browser-for-enterprise) for built-in agent browser testing
- For JetBrains IDEs (IntelliJ, and others), install the [Cursor Agent via ACP](https://cursor.com/docs/integrations/jetbrains.md). This is included with Cursor and does not require a separate JetBrains AI license

## Related

- [Enterprise overview](https://cursor.com/docs/enterprise.md)
- [Identity and Access Management](https://cursor.com/docs/enterprise/identity-and-access-management.md)
- [Deployment patterns](https://cursor.com/docs/enterprise/deployment-patterns.md)
- [Security and Privacy Hardening](https://cursor.com/docs/enterprise/security-hardening.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
