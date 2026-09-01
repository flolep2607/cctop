# Organization Groups

Organization Groups are org-level cohorts in Cursor Enterprise, such as Engineering, Contractors, or Pilot Users. Members can come from any team in your [Organization](https://cursor.com/docs/enterprise/organizations.md), and a user can belong to many groups at once.

Groups do two jobs:

1. **Apply settings to a cohort.** Spend limits, model access, and agent controls follow group members across teams.
2. **Power teams.** Map a group to a team, and Cursor keeps that team's membership and roles aligned with the cohort.

Manage membership by hand, or sync it from your identity provider through SCIM. This page walks through the SCIM-synced setup first, then shows how to use groups to drive team membership and roles.

Organization Groups are separate from [Billing
Groups](https://cursor.com/docs/account/enterprise/billing-groups.md), which attribute spend for
reporting, and from team-level [directory
groups](https://cursor.com/docs/account/teams/scim.md#directory-groups), which set spend and
policy within one team.

## Prerequisites

- A Cursor Enterprise plan with an [Organization](https://cursor.com/docs/enterprise/organizations.md)
- Org admin access. Org admins create and manage groups. Team admins don't, though they can select groups where a team control supports it, such as [marketplace access](https://cursor.com/docs/enterprise/organization-groups.md#team-marketplace-access)
- For SCIM-synced groups: [SCIM provisioning](https://cursor.com/docs/account/teams/scim.md) connected to your Organization

## Set up SCIM-synced groups

A SCIM-synced group mirrors a directory group from your identity provider. Your identity provider decides who belongs. Cursor decides what the group can do.

### Connect SCIM

Follow [SCIM provisioning](https://cursor.com/docs/account/teams/scim.md#setup) to connect your identity provider, and enable push group provisioning so directory groups sync into Cursor. At the organization level, each identity provider connection has a **SCIM Directory** section in the Organization's **Settings**; select **Sync Directory** to register that connection's directory in Cursor.

Two org-level details:

- An identity provider connection supports one SCIM directory, so an Organization has one directory through its own identity provider. An Organization can still draw from more than one directory when linked teams run their own identity providers, since each of those connections brings its own SCIM directory. Directory groups from every connected directory are available when you create synced groups.
- If teams set up their own identity providers before joining the Organization, org admins can consolidate them into one shared organization identity provider first. See [Consolidate team identity providers](https://cursor.com/docs/enterprise/organizations.md#consolidate-team-identity-providers).

### Create a group

### Open Groups

In the dashboard, open **Organization -> Groups** from your profile menu,
then select **Add**.

![Groups page in the Organization dashboard showing the group list and Add button](/docs-static/images/enterprise/organization-groups/org-groups-list.png)

### Create the group

Name the group and set **Type** to **Synced**, then pick the **Directory
group** to sync from. If more than one directory is connected, groups from
each appear in the picker.

![Create group dialog with Type set to Synced and a directory group selected](/docs-static/images/enterprise/organization-groups/org-groups-create-synced.png)

### Configure settings

Set spend limits, model access, and agent controls on the group's settings
page. See [Configure group settings](https://cursor.com/docs/enterprise/organization-groups.md#configure-group-settings).

To create a manual group instead, set **Type** to **Manual**. Admins then manage membership in the dashboard, by CSV import, or through the [Organization API](https://cursor.com/docs/account/organizations/organization-admin-api.md#organization-groups). To connect an existing group to SCIM later, open the group's **Settings** and select **Connect** next to **SCIM directory group**.

### What syncs, and what you still control

For a SCIM-synced group:

- **Your identity provider owns membership.** Cursor shows synced members as read-only and disables manual changes the next sync would overwrite. To change who belongs, update the directory group in your identity provider.
- **You own the settings.** Spend limits, model access, agent controls, and team mappings live in Cursor and never sync from your directory.
- **You own roles.** Team roles come from the role you set on a group-to-team mapping in Cursor, not from role attributes in your identity provider. See [Set team roles from mappings](https://cursor.com/docs/enterprise/organization-groups.md#set-team-roles-from-mappings).

Membership changes in your identity provider sync automatically. See the [SCIM FAQ](https://cursor.com/docs/account/teams/scim.md#faq) for common sync issues.

### Manage members

Open a group and select **Members** to view membership.

For manual groups, admins can:

- Add existing Organization members
- Import members by CSV
- Move members to another group
- Remove members
- Search and sort the member list

For SCIM-synced groups, the member list is read-only. Manage membership in your identity provider.

![Members tab of a SCIM-synced group showing the read-only synced member list](/docs-static/images/enterprise/organization-groups/org-groups-members-readonly.png)

### Edit or delete a group

Open a group to rename it or change its settings. For manual groups, you can also change membership here.

Deleting a group removes the group and its settings, such as spend limits and model access. It doesn't delete member accounts, and members keep access granted by other groups, their teams, or per-user settings.

If a group is mapped to a team as a [membership
source](https://cursor.com/docs/enterprise/organization-groups.md#use-groups-to-power-teams), deleting it works like removing the
mapping: members only that group supplied are removed from the team, and the
roster recalculates from any remaining mapped groups. If none remain, current
members stay on the team and the team returns to manual management. See [How
membership reconciles](https://cursor.com/docs/enterprise/organization-groups.md#how-membership-reconciles).

## Configure group settings

Open a group and select **Settings**. Group settings apply to everyone in the group, then combine with each user's team settings. Overlaps resolve toward the most permissive result, so keep strict defaults at the team level and use groups to widen access for selected cohorts. See [How group and team settings combine](https://cursor.com/docs/enterprise/organization-groups.md#how-group-and-team-settings-combine).

![Group settings page with SCIM directory group connection, Auto-Run Controls, monthly spending limit, and model providers](/docs-static/images/enterprise/organization-groups/org-groups-settings.png)

### Spend limits

Set a per-user monthly spend limit on the group. When a user belongs to multiple groups or also has team-level limits, the highest applicable limit wins. For example, if a team default is stricter and a group carries a higher limit, the group limit applies to that user. A group limit lower than a more permissive team setting doesn't tighten the user's access.

### Model access

Use the **Models** tab to control which models group members can use. This suits controlled rollouts, approvals, and cohorts that need models not enabled for everyone.

Model access is a **union** with the team setting: access is granted if the team or any of the user's groups allows it. Neither the team nor a group fully supersedes the other; the most permissive result wins. Set restrictive defaults on the team, then widen access per group. Groups cannot take away a model that the team (or another of the user's groups) already allows.

Personal API key (BYOK) controls stay on the team's **Team Settings → Models** page. Group Models settings do not configure BYOK.

See [Model and Integration Management](https://cursor.com/docs/enterprise/model-and-integration-management.md#how-team-and-group-model-access-combine) for the same priority rules from the team side.

### Auto-run and Smart Auto

Groups can carry group-level agent controls, including Auto-run and Smart Auto settings where available.

When the team and a group both define the same Auto-run setting, Cursor merges each field independently. Inactive policies don't participate: if the team policy is disabled and the group policy is active, the group policy applies.

| Setting                    | How team and group values combine                                                                                                              |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Run modes                  | Union. A mode is available if either level enables it: Allowlist, Auto-review, or Run Everything.                                              |
| Terminal command allowlist | Union with deduplication. Commands allowed by either level are allowed.                                                                        |
| Delete File Protection     | Enabled if either level enables it.                                                                                                            |
| Browser Protection         | Enabled if either level enables it.                                                                                                            |
| Sandboxing Mode            | Loosest setting wins. `disabled` beats `enabled`, so sandboxing applies only when both levels enable it.                                       |
| Sandbox Networking         | Loosest setting wins. `user_controlled` beats `always_disabled`, so networking is always disabled only when both levels set `always_disabled`. |
| Sandbox Git Access         | Same as Sandbox Networking: `user_controlled` beats `always_disabled`.                                                                         |

When several groups apply to the same user, the same field-wise merge runs across those groups. Auto-review instructions are the exception: if a group defines instructions, they replace the team-level instructions for that user.

### Team marketplace access

Team admins can restrict a [team marketplace](https://cursor.com/docs/plugins.md#team-marketplaces) to selected groups. Open **Dashboard -> Plugins**, select a marketplace, then choose groups under **Marketplace Settings -> Marketplace Access**.

A marketplace stays scoped to its owning team: selecting a group grants access only to group members who also belong to that team. Team admins keep access, and a marketplace with no selected groups is open to everyone in the team. Existing marketplaces that use team-level directory groups keep those assignments; Cursor doesn't migrate them.

## Use groups to power teams

Groups can drive team membership, not only settings. Map a group to a team as a membership source, and Cursor keeps the team's roster aligned with the group. When someone joins or leaves the group, their membership on the mapped team follows.

Combined with SCIM, this closes the loop from your directory to Cursor teams: your identity provider updates the directory group, the sync updates the Organization Group, and the mapping updates the team.

### Map a group to a team

Map an Organization Group, manual or SCIM-synced, to a team. To drive a team from a directory group in your identity provider, sync that directory group into an Organization Group first, then map the group to the team. A team can have several mapped groups at once; its roster is the combined membership of all of them.

When you create a team, set **Membership Type** to **Synced**, then add one or more groups under **Membership sources**. The team's members are the union of the listed groups; remove a group to stop syncing it.

![Create team dialog with Membership Type set to Synced and two groups added as membership sources](/docs-static/images/enterprise/organization-groups/team-membership-sources.png)

If your Organization uses **Auto Add Users to Root Team**, every synced user
is added to the root team automatically. Turn it off in the Organization's
**Settings** when group-to-team mappings should control team membership.

### How membership reconciles

Once a team has at least one mapped group, sync manages its entire roster:

- **Adds**: when someone joins a mapped group, they're added to the team.
- **Removals**: when someone leaves, they're removed from the team, unless another group mapped to the same team still includes them.
- **Manual edits are disabled.** Admins can't add or remove members on a synced team from the dashboard. Change membership through the mapped groups instead.

If sync removes someone from their last team in the Organization, they leave the Organization as well. Members who hold an org-level role directly, such as org admins, keep their Organization access.

Removing a mapping recalculates the team's roster from the remaining mapped groups: members only the removed group supplied are removed from the team, and anyone another mapped group still supplies stays. Removing the last mapping removes no one. Current members stay on the team, and the team returns to manual management.

### Set team roles from mappings

A mapping can also assign a team role: member, admin, or no role. Map an Engineering Leads group with the admin role, and everyone in the group becomes an admin on that team. Roles are configured in Cursor on the mapping; role attributes in your identity provider don't sync. See [Members, roles, and seat types](https://cursor.com/docs/account/teams/members.md) for what each role can do.

The role setting works on its own axis, separate from membership sync:

- **Mapping has a role**: sync enforces that role, and roles on the team can no longer be changed from the dashboard.
- **No mapping has a role**: members are added with the Member role, and admins can still change roles in the dashboard.

When a user belongs to several groups mapped to the same team, they receive the highest role across those mappings. A user in one group mapped as admin and another mapped as member becomes an admin, and an assigned role beats a mapping with no role.

In the team form, turn on **Use groups to set role**, then choose **Member** or **Admin** per group. Leave the toggle off to sync members without managing roles.

![Create team dialog with Use groups to set role enabled and Member and Admin roles chosen per group](/docs-static/images/enterprise/organization-groups/team-roles-from-groups.png)

### How group and team settings combine

A user's effective settings come from their team plus every group they belong to. The general rule: most permissive wins.

| Setting type             | How it resolves across groups and teams                                                                                                                                   |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Spend limits             | The highest applicable per-user limit applies.                                                                                                                            |
| Model access             | Union. Access is granted if the team or any group allows it.                                                                                                              |
| Auto-run and Smart Auto  | Field-by-field merge, with the loosest value winning per field. See the [table above](https://cursor.com/docs/enterprise/organization-groups.md#auto-run-and-smart-auto). |
| Auto-review instructions | Group instructions replace team instructions for that user.                                                                                                               |
| Team role                | Set by [group-to-team mappings](https://cursor.com/docs/enterprise/organization-groups.md#set-team-roles-from-mappings), not by the merge rules above.                    |

For model access specifically, neither the team nor a group fully supersedes the other: if either allows a model, the user can use it. Set the strictest baseline on the team, then widen with groups. For a layered view that also includes per-user overrides and team directory groups, see [How limits and permissions combine](https://cursor.com/docs/enterprise/organizations.md#how-limits-and-permissions-combine).

## Manage groups with the API

List groups, read members, and add or remove members through the [Organization API](https://cursor.com/docs/account/organizations/organization-admin-api.md#organization-groups). Group routes use Organization API keys, and group IDs use the `g_` prefix.

## Related docs

- [Organizations](https://cursor.com/docs/enterprise/organizations.md)
- [Identity & access management](https://cursor.com/docs/enterprise/identity-and-access-management.md)
- [SCIM provisioning](https://cursor.com/docs/account/teams/scim.md)
- [Members, roles, and seat types](https://cursor.com/docs/account/teams/members.md)
- [Organization API](https://cursor.com/docs/account/organizations/organization-admin-api.md#organization-groups)
- [Pooled usage](https://cursor.com/docs/enterprise/pooled-usage.md)
- [Spend limits](https://cursor.com/help/account-and-billing/spend-limits.md)

### Organization Groups are available on Enterprise

Contact our team to learn more about organization-level administration.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
