# SCIM

## Overview

SCIM 2.0 provisioning automatically manages your team members and directory groups through your identity provider. Available on Enterprise plans with SSO enabled, [contact sales](https://cursor.com/contact-sales?source=docs-scim) to get access.

This page covers SCIM for a single team. If you run multiple linked teams
under an [Organization](https://cursor.com/docs/enterprise/organizations.md), you can also sync
directory groups at the organization level and use them across teams through
[Organization Groups](https://cursor.com/docs/enterprise/organization-groups.md).

## Prerequisites

- Cursor Enterprise plan
- SSO configured first. **SCIM requires an active SSO connection**
- Admin access to your identity provider (Okta, Azure AD, etc.)
- Admin access to your Cursor team

## How it works

### User provisioning

Users are automatically added to Cursor when assigned to the SCIM application in your identity provider. When unassigned, they're removed. Changes sync in real-time.

### Directory groups

Directory groups and their membership sync from your identity provider. Manage groups and users in your identity provider; Cursor displays this information as read-only. Within a team, directory groups control spend limits and policy.

To drive team membership and roles from your directory, sync a directory group into an [Organization Group](https://cursor.com/docs/enterprise/organization-groups.md#set-up-scim-synced-groups) and map that group to a team. See [Use groups to power teams](https://cursor.com/docs/enterprise/organization-groups.md#use-groups-to-power-teams).

### Spend management

Set different per-user spend limits for each directory group. Directory group limits take precedence over team-level limits. Users in multiple groups receive the highest applicable spend limit.

## Setup

### Ensure SSO is configured

SCIM requires SSO to be set up first. If you haven't configured SSO yet,
follow the [SSO setup guide](https://cursor.com/docs/account/teams/sso.md) before proceeding.

### Access Active Directory Management

Navigate to
[cursor.com/dashboard/members?subtab=active-directory](https://www.cursor.com/dashboard/members?subtab=active-directory)
with an admin account, or go to your dashboard settings and select the "Members
& Groups" tab followed by the "Directory Groups" subtab.

### Start SCIM setup

Once SSO is verified, you'll see a link for step-by-step SCIM setup. Click
this to begin the configuration wizard.

### Configure SCIM in your identity provider

In your identity provider, create or configure your SCIM application, use
the SCIM endpoint and token provided by Cursor, enable user and push group
provisioning, then test the connection.

### Configure spend limits (optional)

Back in Cursor's Active Directory Management page, view your synchronized
directory groups, set per-user spend limits for specific groups, and
review which limits apply to users in multiple groups.

### Identity provider setup

For provider-specific setup instructions:

### Identity Provider Guides

Setup instructions for Okta, Azure AD, Google Workspace, and more.

## Managing users and groups

All user and group management must be done through your identity provider.
Changes made in your identity provider will automatically sync to Cursor, but
you cannot modify users or groups directly in Cursor.

### User management

- Add users by assigning them to your SCIM application in your identity provider
- Remove users by unassigning them from the SCIM application
- User profile changes (name, email) sync automatically from your identity provider

### Group management

- Directory groups are automatically synced from your identity provider
- Group membership changes are reflected in real-time
- Use groups to organize users and set different spend limits

### Spend limits

- Set different per-user limits for each directory group
- Users inherit the highest spend limit from their groups
- Group limits override the default team-wide per-user limit

## FAQ

### Why isn't SCIM management showing up in my dashboard?

Ensure SSO is properly configured and working before setting up SCIM. SCIM requires an active SSO connection to function.

### Why aren't users syncing?

Verify that users are assigned to the SCIM application in your identity provider. Users must be explicitly assigned to appear in Cursor.

### Why aren't groups appearing?

Check that push group provisioning is enabled in your identity provider's SCIM settings. Group sync must be configured separately from user sync.

### Why aren't spend limits applying?

Confirm users are properly assigned to the expected groups in your identity provider. Group membership determines which spend limits apply.

### Can I manage SCIM users and groups directly in Cursor?

No. All user and group management must be done through your identity provider. Cursor displays this information as read-only.

### How quickly do changes sync?

Changes made in your identity provider sync to Cursor in real-time. There may be a brief delay for large bulk operations.

### Can I sync user roles from my identity provider?

Roles are configured in Cursor. To set roles from your directory, sync the directory group into an [Organization Group](https://cursor.com/docs/enterprise/organization-groups.md#set-up-scim-synced-groups) and map that group to a team with a role. Members of the group receive that role on the team. See [Set team roles from mappings](https://cursor.com/docs/enterprise/organization-groups.md#set-team-roles-from-mappings).

The role on a mapping also controls dashboard editing. When a group is connected to a team with a role, sync manages roles on that team and admins can no longer change them in the dashboard. Roles stay editable in the dashboard only when no role was selected during group selection.

### Why are there users on my Members dashboard that aren't in the provisioned groups?

When team-level SCIM is set up, existing users are not automatically removed from Cursor. Either remove them manually, or sync them with SCIM once and deprovision them from your identity provider to have them removed from Cursor.

### Why don't the users from my synced groups match the users on the Cursor Members dashboard?

Once a user account is provisioned, they won't appear on the Cursor Members Dashboard until they sign in for the first time.

### SCIM is available on the Enterprise plan

Contact our team to request access.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
