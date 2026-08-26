# SSO

## Overview

SAML 2.0 SSO is available at no additional cost on Teams and Enterprise plans. Use your existing identity provider (IdP) to authenticate team members without separate Cursor accounts.

## Prerequisites

- Cursor Team plan
- Admin access to your identity provider (e.g., Okta)
- Admin access to your Cursor organization

## Configuration Steps

### Sign in to your Cursor account

Navigate to the [Single Sign-On (SSO) settings](https://cursor.com/dashboard/team-settings#single-sign-on-sso) with an admin account.

### Locate the SSO configuration

Find the "Single Sign-On (SSO)" section and expand it.

### Begin the setup process

Click "Configure" next to "SSO-Provider Connection Settings" to start SSO setup and follow the wizard.

### Configure your identity provider

In your identity provider (e.g., Okta):

- Create new SAML application
- Configure SAML settings using Cursor's information
- Set up Just-in-Time (JIT) provisioning

### Verify domain

Click "Configure" next to "Domain Verification Settings" to verify your users' domain.

## View your SSO configuration

Admins can review an existing SSO connection and its domains at any time:

1. Go to [Single Sign-On (SSO) settings](https://cursor.com/dashboard/team-settings#single-sign-on-sso) with an admin account.
2. Click "Configure" next to "SSO-Provider Connection Settings" to view the provider connection details.
3. Click "Configure" next to "Domain Verification Settings" to view or manage verified domains.

These settings are available to team admins.

### Identity Provider Setup Guides

For provider-specific setup instructions:

### Identity Provider Guides

Setup instructions for Okta, Azure AD, Google Workspace, and more.

## Additional Settings

Once domain verification and the SSO provider connection are active, users on that domain are required to sign in with SSO. There is no separate enforcement toggle.

- New users auto-enroll when signing in through SSO
- Handle user management through your identity provider

## Multiple domains

To handle multiple domains in your organization:

1. **Verify each domain separately** in Cursor through the domain verification settings
2. **Configure each domain** in your identity provider
3. Each domain needs to go through the verification process independently

## Troubleshooting

If issues occur:

- Verify domain is verified in Cursor
- Ensure SAML attributes are properly mapped
- Confirm the SSO connection is active and the domain is verified.
- Match first and last names between identity provider and Cursor
- Check provider-specific guides above
- Visit the [SSO help center](https://cursor.com/help/security-and-privacy/sso.md) if issues persist


---

## Sitemap

[Overview of all docs pages](/llms.txt)
