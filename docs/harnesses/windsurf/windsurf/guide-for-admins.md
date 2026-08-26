# Guide for Admins

Enterprise admin guide for deploying Windsurf at scale. Configure SSO, SCIM, RBAC, analytics, and team management for large organizations.

# Windsurf Guide for Enterprise Admins

> **Purpose**   This guide helps enterprise *platform / developer-experience* administrators plan, roll out, and operate Windsurf for organizations with **large enterprise teams**.  It is intentionally *opinionated* and links out to detailed “how-to” docs per topic.  Treat it both as a **read-through guide** *and* as a **check-list** when onboarding.

***

## 1.   Audience & Pre-Requisites

|                       | Details                                                                            |
| --------------------- | ---------------------------------------------------------------------------------- |
| **Who should read**   | Platform / Dev-Ex admins, Corporate IT, Centralized Tooling teams                  |
| **Assumed knowledge** | Basic Windsurf terms (team, role), Enterprise IdP concepts (SAML, SCIM), CLI usage |
| **Out-of-scope**      | Deep security / compliance internals → see **Security & Compliance** docs          |

***

## 2.   Quick-Start Checklist

1. Confirm organization-wide settings
2. Set up **SSO** (Okta, Azure AD, Google; see SAML docs for others)
3. Enable **SCIM** & map IdP groups → Windsurf *teams*
4. Define **role** & **permission** model (least privilege)
5. Configure **Admin Portal**: team view & security controls
6. Distribute **Windsurf clients/extensions** to end users
7. View **analytics dashboards** & **API access tokens**

> Use this list as your “Day 0” deployment tracker.

***

## 3.   Core Windsurf Concepts

* **Team** – flat collections of members; no nested teams. Teams (also called *Groups*) drive **role assignment** and **analytics grouping**, letting you scope permissions and view usage metrics per cohort.
* **Roles & Permissions** – predefined RBAC; admins are primarily responsible for **team management**, **Windsurf feature settings**, and **analytics**. Built-in roles usually cover these needs, but creating a custom role with *analytics-view* permission lets team managers and leads see metrics for their own teams. (<a href="/windsurf/accounts/rbac-role-management">RBAC docs</a>)
* **Admin Portal** – centralized UI for user & team management, credit usage, SSO configuration, feature toggles (<a href="/windsurf/cascade/web-search">Web Search</a>, <a href="/windsurf/cascade/mcp">MCP</a>, <a href="/windsurf/cascade/app-deploys">Deploys</a>), analytics dashboards/report export, service keys for API usage, and role/permission controls.
* **Agents & Workspaces** – Windsurf IDE and Jetbrains Plugins are Agentic

### 3.1   Admin Portal Overview

The Admin Portal provides centralized management for all Windsurf enterprise features through an intuitive web interface. Core capabilities include:

#### User & Team Management

* Add, remove, and manage users across your organization
* Configure teams with proper role assignments
* User status and activity monitoring

#### Authentication & Security

* Configure SSO integration with major identity providers
* Set up SCIM provisioning for automated user lifecycle management
* Manage role-based access controls (RBAC)
* Create and manage **service keys** for API automations with scoped permissions

#### Feature Toggles & Controls

> **Important:** These feature controls affect behavior for your entire organization and can only be modified by administrators. New major features with data privacy implications are released in the "off" state by default to ensure you have control over when and how they're enabled.

The <a href="https://windsurf.com/team/settings">Admin Portal</a> gives you granular control over Windsurf features that can be enabled or disabled per team. **Data Privacy Note:** Some features require storing additional data or telemetry as noted below:

**Models Configuration**

* Configure which AI models your teams can access within Windsurf
* You can **filter by model** (choose specific models such as SWE-1.5, Claude Opus 4.6, etc.) or **filter by provider** (e.g., OpenAI, Anthropic, Google). Only one filter type is enforced at a time.
* Select multiple models or providers for different use cases (Cascade, Command, chat, etc.)

**Default Model Override**

* Set the default Cascade model for users on your team
* This model is pre-selected each time a user opens Windsurf (not just the first time)
* Users can still change their model at any time during a session
* Only models enabled in Models Configuration are available as default options

**Auto Run Terminal Commands** *(Beta)*

* Set the maximum auto-execution level for terminal commands across your organization
* Four levels available: **Disabled** (no auto-execution), **Allowlist Only** (only allowlisted commands), **Auto** (AI-judged safe commands), and **Turbo** (all commands except denylisted)
* Users can select any level up to the maximum you configure, giving them flexibility within your security policy
* [Learn more about auto-executed commands](https://docs.windsurf.com/windsurf/terminal#auto-executed-cascade-commands)

**Terminal Command Lists** *(Beta)*

* Configure **team-wide allowlist and denylist** for terminal commands that apply to all team members
* **Allowlist**: Commands in this list will be auto-executed without user confirmation (when auto-execution is enabled)
* **Denylist**: Commands in this list will always require user approval before execution
* **Precedence**: The denylist takes precedence over the allowlist—if a command matches both lists, it will require approval
* Access via <a href="https://windsurf.com/team/settings">Admin Portal</a> → Team Settings → Terminal Commands → **Manage Lists**
* These team-level lists are merged with individual user allow/deny lists configured in Windsurf settings

**MCP Servers** *(Beta)*

* Enable users to configure and use Model Context Protocol (MCP) servers
* Maintain whitelisted MCP servers for approved integrations
* **Security Note:** Review operational and security implications before enabling, as MCP can create infrastructure resources outside Windsurf's security monitoring
* <a href="https://docs.windsurf.com/plugins/cascade/mcp#model-context-protocol-mcp">Learn more about Model Context Protocol (MCP)</a>
* <a href="https://docs.windsurf.com/plugins/cascade/mcp#admin-controls-teams-%26-enterprises">MCP admin controls for teams & enterprises</a>

**App Deploys** *(Beta)*

* Manage deployment permissions for your teams in Cascade
* <a href="https://docs.windsurf.com/windsurf/cascade/app-deploys#app-deploys">Learn more about App Deploys</a>

**Conversation Sharing**

* Allow team members to share Cascade conversations with others
* Conversations are securely uploaded to Windsurf servers
* Shareable links are restricted to logged-in team members only
* <a href="https://docs.windsurf.com/windsurf/cascade/cascade#sharing-your-conversation">Learn more about sharing conversations</a>

**Devin**

* **What Devin does** — Delegate complex tasks end to end (debugging, deployment, testing, and more) to an autonomous cloud agent. Each Devin session runs on its own VM with a desktop, browser, and computer use, so it can keep working after you close your laptop.
* **Delegating work to Devin** — Plan with a local Cascade agent and, with a single click, send the task to Devin for implementation. Devin spins up its own machine and gets to work while your team keeps coding locally.
* **Where Devin shows up** — Devin sessions appear alongside local Cascade sessions in the Agent Command Center and can be organized into Spaces along with everything else related to a task.
* <a href="https://docs.windsurf.com/windsurf/devin">Learn more about Devin</a>

**PR Reviews (GitHub Integration)**

* Install Windsurf in your team's GitHub organization
* Enable PR review automation and description editing
* <a href="https://docs.windsurf.com/windsurf-reviews/windsurf-reviews#windsurf-pr-reviews">Learn more about Windsurf PR Reviews</a>
* **We recommend <a href="https://app.devin.ai/review">Devin Review</a> as our new and improved code review experience.** <a href="https://docs.devin.ai/work-with-devin/devin-review">Learn more</a>.

**Knowledge Base Management**

* Curate knowledge from Google Drive sources for your development teams
* Upload and organize internal documentation and resources
* <a href="https://docs.windsurf.com/context-awareness/overview#knowledge-base-beta">Learn more about Knowledge Base</a>

***

## 4.   Identity & Access Management

> **Recommendation:** Use **SSO plus SCIM** wherever possible for automated provisioning, de-provisioning, and group management.

### 4.1   Single Sign-On (SSO)

|                          | Guidance                                                                                                               |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------- |
| **IdPs supported**       | Okta, Azure AD, Google (others via generic SAML)                                                                       |
| **Recommended approach** | Create Windsurf-specific *app* in IdP; use **role-based** group assignments rather than org-wide `All Employees` group |
| **Common pitfalls**      | Email suffix mismatches, duplicate user aliases                                                                        |

*See the <a href="https://docs.windsurf.com/windsurf/accounts/sso-scim">SSO & SCIM Setup Guide</a> for step-by-step configuration for Okta, Azure AD, Google, and Generic SAML.*

### 4.2   SCIM Provisioning

* **Why** – automated user lifecycle & team membership management at scale
* **Capabilities**
  * Create / deactivate **users** automatically
  * Create **teams** automatically (or manage manually)
  * Users can belong to **multiple teams**
  * Custom team creation via SCIM API (<a href="https://docs.windsurf.com/windsurf/accounts/sso-scim#scim-api">docs</a>)
* **Mapping strategies**
  * 1 IdP group → 1 Windsurf team (simple, most common)
  * Functional vs. project-based group prefixes (e.g. `proj-foo-devs`)
* **Things to decide**
  * Which groups to *exclude* (e.g. interns, contractors)
  * Renaming rules when IdP group names change
* **Caution**: SCIM should remain your **source of truth**—mixing SCIM and manual / API updates can create drift. Use the API mainly for adding supplemental groups.

***

## 5.   User & Team Management at Scale

* Flat *team* → design team taxonomy carefully (no nesting to fall back on)
* Users can belong to **multiple groups**. Groups are used to view analytics
* Today, SCIM does not support assigning roles to users. SCIM only supports assigning users to Groups

***

## 6.   Analytics & API Access

### 6.1   Built-In Analytics

| Dashboard             | Use-case                                   |
| --------------------- | ------------------------------------------ |
| **Adoption Overview** | Track total active users, daily engagement |
| **Team Activity**     | Team usage                                 |

Analytics shows the **percentage of code written by Windsurf**, helping quantify impact—see your dashboards at <a href="https://windsurf.com/team/analytics">team analytics</a>.

### 6.2   APIs

| API      | Typical admin scenarios    |
| -------- | -------------------------- |
| **REST** | SCIM management, analytics |

* Generate service keys under <a href="https://windsurf.com/team/settings">**Team Settings → Service Keys**</a>. Scope keys to *least privilege* needed.
* More advanced reporting: see the <a href="https://docs.windsurf.com/plugins/accounts/api-reference/introduction">Analytics API Reference</a>.
* For team management: see the <a href="https://docs.windsurf.com/windsurf/accounts/sso-scim#scim-api">SCIM API – Custom Teams</a>.

***

## 7.   Operational Considerations

* **Status Pages** – monitor live service health: <a href="https://status.windsurf.com/">Windsurf</a>, <a href="https://status.anthropic.com/">Anthropic</a>, <a href="https://status.openai.com/">OpenAI</a>
* **Support Channels** – windsurf.com/support

***

## 8.   Setting Up End Users for Success

1. Point end users to the <a href="https://docs.windsurf.com/windsurf/getting-started">Windsurf installation guide</a> to install the appropriate extension or desktop client.
2. Publish an internal “Getting Started with Windsurf” page (link to official docs)
3. Hold live onboarding sessions / record short demos
4. Curate starter project templates & sample prompts
5. Collect feedback via survey after 2 weeks; iterate

***

## 9.   Additional Resources

* <a href="https://docs.windsurf.com/windsurf/accounts/sso-scim">SSO & SCIM Setup Guide</a>
* <a href="https://docs.windsurf.com/windsurf/accounts/sso-scim#scim-api">SCIM API – Custom Teams</a>
* <a href="https://docs.windsurf.com/plugins/accounts/api-reference/introduction">Analytics API Reference</a>
* <a href="/windsurf/accounts/rbac-role-management">RBAC Controls</a>
