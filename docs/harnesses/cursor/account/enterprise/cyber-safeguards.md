# Cyber Safeguards

Cyber Safeguards settings are available on the [Enterprise plan](https://cursor.com/docs/enterprise.md) and are configured per [Organization Group](https://cursor.com/docs/enterprise/organizations.md#groups) at **Organization → Groups**. These are not [billing groups](https://cursor.com/docs/account/enterprise/billing-groups.md) or SCIM [directory groups](https://cursor.com/docs/account/teams/scim.md).

Anthropic applies cyber safeguards to its latest generations of Opus models. These safeguards can limit responses to some legitimate security and cyber-defense tasks. Through Anthropic's Cyber Verification Program (CVP), approved organizations can use eligible models without those safeguards for legitimate defensive work.

Cursor facilitates your application for the CVP, but any agreement for the program is only between your organization and Anthropic. All terms are set by Anthropic, and Cursor does not take on any of its obligations.

Once approved for the CVP by Anthropic, you can turn on the Cyber option for Opus 4.7, Opus 4.8, and Opus 5 from your settings.

It's important to understand that the blocks you might experience in Cursor when interacting with a model don't come from Cursor; they come directly from Anthropic's API.

## About the program

The CVP belongs to Anthropic. You apply with Anthropic, and any agreement is between your organization and Anthropic. Cursor surfaces the application and the model controls in the dashboard so you can manage everything in one place. Cursor is not a party to the program, sets none of its terms, and takes on none of its obligations.

Anthropic sets the privacy policy for cyber-verified models. Today, zero data retention is turned off for requests going through these models.

## Privacy and data retention

When you use Opus models with cyber safeguards turned off, Anthropic's CVP terms apply. Cursor cannot honor zero data retention or your team's [Privacy Mode](https://cursor.com/docs/enterprise/privacy-and-data-governance.md) policy for those requests.

This applies only to the cyber-verified model you select. Other models in the same conversation, and every other request on your account, still follow your normal Privacy Mode settings.

Turning on Cyber for a group does not disable Privacy Mode organization-wide.

## Setting up a dedicated group

Your organization can apply for the CVP only at the [Organization Group](https://cursor.com/docs/enterprise/organizations.md#groups) level so access stays with only the people who need it.

Turning on the Cyber option in a group only grants it to that group's members. It does not change anything for the rest of your team.

## Apply for the program

Read Anthropic's terms in full before you apply. By applying, you confirm that you're an authorized representative of your organization and that you agree to Anthropic's terms, not Cursor's.

### Open your Organization

Click your profile in the bottom-left corner and select **Organization** from the menu.

### Create or open an Organization Group

In the left sidebar, open **Groups**. Create a new group for your security team (for example, `Security` or `CVP`) and add the relevant members, or click an existing group. If you see "No groups yet", create one before continuing.

### Open Cyber Safeguards Models Settings

On that group's settings page, find the **Cyber Safeguards Models Settings** section. It appears alongside Spend Limit Overrides and Auto-Run Controls.

### Start the application

Click **Apply** to open the program terms.

### Review the terms and confirm

Read the **Apply for Cyber Verification Program** dialog. Click **I understand** only if you agree and are authorized to apply on behalf of your company, then click **Save**.

## Anthropic Approval Process

Anthropic reviews your application directly and contacts you about the outcome. Cursor rechecks your status every two hours, so the dashboard updates on its own and shows **Approved** once you're cleared. Cursor is not responsible for the status of your application or for tracking its progress. If you'd like to see your status in real time, check the [Anthropic portal](https://portal.anthropic.com).

## Enabling Cyber on a specific model

Once your group is approved, turn on the Cyber option in the same group's model access settings, shown above the Cyber Safeguards section.

Requests through a cyber-verified model run with Privacy Mode off for that model only. Cursor cannot honor zero data retention for those requests. See [Privacy and data retention](https://cursor.com/docs/account/enterprise/cyber-safeguards.md#privacy-and-data-retention) for details.

## Supported models

Enabling this mode works with Anthropic models today: **Opus 4.7**, **Opus 4.8**, and **Opus 5**.

## FAQ

### How does pricing work for cyber-verified models?

Pricing is the same as the base model. See [models & pricing](https://cursor.com/docs/models-and-pricing.md) for details.

### Who can apply for the CVP through Cursor?

Only Organization admins can apply for the CVP, as they have to first create a group.

### What is the privacy policy for cyber-verified models?

Anthropic sets the policy for cyber-verified models. Today, zero data retention is turned off for requests going through these models. When you use a model with cyber safeguards off, Privacy Mode is off for that model only. Cursor cannot honor your data retention policy for those requests. Other models in the same conversation still follow your normal Privacy Mode settings.

### I applied for the CVP but don't see approval?

Approval is not instant. After you apply for the CVP, Cursor checks for your approval every two hours. Cursor is not responsible for the status of your application or for tracking its progress. If you'd like to see your status in real time, check the [Anthropic portal](https://portal.anthropic.com).

### Is Mythos included as a part of the CVP?

No. Only Opus 4.7, Opus 4.8, and Opus 5 are a part of the program.

### I only see Spend Limit Overrides and Auto-Run Controls

You're likely in team directory groups or billing groups, not Organization Groups. Open **Organization → Groups** from your profile menu, then click into a specific group. Cyber Safeguards Models Settings appear on that group's settings page.

### My group is synced from Okta via SCIM. Does that work for CVP?

CVP applies to Organization Groups under **Organization → Groups**. SCIM [directory groups](https://cursor.com/docs/account/teams/scim.md) are a separate concept used for team-level spend and policy. Create or use a group in your organization's Groups page to apply for CVP.

### A link in the email took me to the Overview page

CVP settings aren't on the org Overview page. Go to **Organization → Groups**, open your group, and look for **Cyber Safeguards Models Settings** on that group's settings page.

### Cyber Safeguards are available on the Enterprise plan

Talk to our team about applying for Anthropic's Cyber Verification Program for your security group.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
