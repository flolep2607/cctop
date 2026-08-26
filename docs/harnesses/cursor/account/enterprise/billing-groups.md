# Billing Groups

[Billing groups](https://cursor.com/dashboard/members?subtab=billing-groups) allow Enterprise admins to understand and manage spend across groups of users. This functionality is useful for reporting, internal chargebacks, and budgeting.

For org-level cohorts across linked teams, see [Organization Groups](https://cursor.com/docs/enterprise/organization-groups.md).

## Billing group architecture

Admins can assign each member to a billing group. Members can only be in one billing group at a time. Members not actively assigned in any other billing group are placed in a reserved `Unassigned` group.

All usage is attributed to the user's group at the time it occurs. Historical data does not change when users move between groups, though it can be reassigned only when a group is deleted. In that case, all of its usage is moved to the Unassigned group.

## View billing groups

Enterprise admins can view billing groups in the web dashboard under the `Members & Groups` tab. This table shows each group, how it is configured, the number of members in it, and spend for the period.

![](/docs-static/images/account/enterprise/billing-groups/billing-groups-view.png)

## Create and add members to a billing group

Admins can create billing groups by clicking `Create Group`. After naming the group, there are four ways to assign members to that group:

1. **SCIM**: Sync the billing group with an existing [SCIM group](https://cursor.com/docs/account/teams/scim.md).

2. **API**: Create groups and add members programmatically via the [Admin API](https://cursor.com/docs/account/teams/admin-api.md#billing-groups).

3. **CSV**: Upload a CSV with group names and email addresses of members.

4. **Manual**: Click `Add Members` and manually select `Unassigned` members to be added.

Billing groups synced with SCIM cannot be edited via CSV, API, or manual UI changes. All member assignment for SCIM-synced groups must be handled via SCIM.

## Move members between billing groups

Admins can move members from manual billing groups by clicking on the billing group and selecting `Move`.

- **SCIM**: When members are moved between SCIM groups in your identity provider, the billing group follows those changes automatically.
- **API**: Use the [add members](https://cursor.com/docs/account/teams/admin-api.md#add-members-to-group) and [remove members](https://cursor.com/docs/account/teams/admin-api.md#remove-members-from-group) endpoints to move members programmatically.

## Rename a billing group

Billing groups can be renamed by clicking the gear button on the main menu, or by clicking `Rename` on the page for that specific billing group.

- **API**: Use the [update group](https://cursor.com/docs/account/teams/admin-api.md#update-group) endpoint to rename groups programmatically.

## Delete a billing group

Billing groups can be deleted by clicking the gear button on the main menu, or by clicking `Delete` on the page for that specific billing group.

- **API**: Use the [delete group](https://cursor.com/docs/account/teams/admin-api.md#delete-group) endpoint to delete groups programmatically.

Deleting a billing group is a destructive operation; data cannot be recovered. All historic usage for deleted groups is assigned retroactively to the `Unassigned` group.

### Billing groups are available on the Enterprise plan

Contact our team to learn about spend management and reporting.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
