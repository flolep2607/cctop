# Domain Verification

Verify your organization's domain ownership with DNS TXT records to enable SSO, user management, and automatic team invitations in Windsurf.

Domain verification is the process of proving that your organization owns or controls a specific domain. This prevents spoofing or unauthorized use of your domain and enables secure organization-level features in Windsurf, such as SSO and user management.

In Windsurf, verifying your domain is required so that users with emails from your organization can be recognized and managed. The domain you need to verify should be the top-level domain of your users’ email addresses (for example, if your users log in with [name@company.com](mailto:name@company.com), you must verify company.com).

## How to verify your domain in Windsurf

<Steps>
  <Step title="Add your domain in the Windsurf portal">
    Enter the domain you want to verify (e.g., company.com). Windsurf will generate a unique verification token and TXT record.

    ⚠️ This token will only be shown once. Be sure to copy it before closing the window.
  </Step>

  <Step title="Add the TXT record to your DNS settings">
    In your DNS provider’s management console, create a new TXT record with the value provided. For example:

    <Frame>
      windsurf-verification=\<your-verification-token>
    </Frame>

    * Name/Host: as specified in the Windsurf portal (often @ or left blank).
    * Value/Content: the exact token string shown in the portal.
  </Step>

  <Step title="Click “Verify” in Windsurf">
    After adding the record, return to the Windsurf portal and click the Verify button to complete the process.
  </Step>

  <Step title="Done">
    If the TXT record is detected, your domain will be marked as verified.
  </Step>
</Steps>

<Note>DNS changes can take up to 24–48 hours to propagate. If verification does not succeed immediately, wait a bit longer and try again.</Note>

## What happens after domain verification

Once your domain is verified, the following behavior will occur:

### For teams with SSO enabled

Any user with an email that ends in your verified domain will only be able to sign up for an account through your SSO integration. Other sign-up attempts (such as username + password or Google OAuth) will be redirected to your SSO portal. Users will be automatically added to your team without an additional approval process.

### For teams without SSO enabled

Users with an email that ends in your verified domain will still be able to sign up for an account using any available method. These users will be automatically invited to your team, but will need to be accepted by a team admin before gaining access.
