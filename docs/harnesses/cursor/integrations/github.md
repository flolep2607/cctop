# GitHub

The Cursor GitHub app connects your repositories so you can use features like [Cloud Agents](https://cursor.com/docs/cloud-agent.md) and [Bugbot](https://cursor.com/docs/bugbot.md).

## Setup

### GitHub.com

Requires Cursor admin access and GitHub org admin access.

1. Go to [Integrations in the dashboard](https://cursor.com/dashboard/integrations)
2. Click **Connect** next to GitHub (or **Manage Connections** if already connected)
3. Choose **All repositories** or **Selected repositories**
4. Return to the dashboard to configure features on your repositories

[GitHub setup](/docs-static/images/bugbot/bugbot-install.mp4)

To disconnect your GitHub account, return to the integrations dashboard and click **Disconnect Account**.

### GitHub Enterprise Server

### Prerequisites

- Running a supported version of GitHub Enterprise Server (v3.8 or later recommended)
- Admin privileges on your GHES instance
- Cursor team admin access (required to see the GitHub Enterprise registration option in the dashboard)

### Networking

GHES requires secure inbound access from Cursor and outbound access for webhook notifications.

#### IP whitelisting (recommended)

Add these IP addresses to your allowlist:

```text
184.73.225.134
3.209.66.12
52.44.113.131
34.192.39.182
50.16.106.255
44.217.29.124
3.223.245.201
54.164.185.10
34.194.133.23
35.170.116.221
```

For other connection options beyond IP whitelisting, see [Advanced networking](https://cursor.com/docs/integrations/github.md#advanced-networking).

#### Proxy requirements

If you run a proxy in front of GHES, make sure it allows Cursor's GitHub App integration to use authenticated GitHub REST and GraphQL APIs. Cursor uses these APIs during app setup and after webhook delivery to resolve repository identity, inspect pull request state, read checks and reviews, and update prior Bugbot output.

The proxy should allow authenticated GitHub API requests from Cursor without blocking or rewriting them.

### Register the Cursor Enterprise App

1. Go to [Integrations in the dashboard](https://cursor.com/dashboard/integrations). In the **Source Control** section, find the **GitHub Enterprise** row (listed under GitHub) and click **Manage apps**.
2. Enter the **base URL** of your GHES instance (e.g., `https://git.yourcompany.com`)
3. Enter the name of the **Organization** that will own the application
   - This should be your company's Organization inside your GHES installation
   - You need administrator privileges for this Organization
   - Other Organizations can access the app once registered
   - Leave blank to use your user account (not recommended)
4. Click **Register**
5. Choose a name for the Cursor Enterprise Application (default recommended)
6. The app will appear under your available GitHub Apps in your GHES instance
7. Return to the dashboard to configure features on your repositories

## IP allow list configuration

If your organization uses GitHub's IP allow list feature to restrict access to your repositories, Cursor can be configured to use a hosted egress proxy with a narrow set of IPs.

The recommended GitHub Apps setting is configured in GitHub. You do not need Cursor to enable it. If you add the git egress proxy IPs yourself instead, contact [hi@cursor.com](mailto:hi@cursor.com) so we can enable git egress proxy for your team. Otherwise git traffic may not come from those addresses.

### Enable IP allow list configuration for installed GitHub Apps (recommended)

The Cursor GitHub app has the IP list already pre-configured. You can enable the allowlist for installed apps to automatically inherit this list. This is the **recommended approach**, as it allows us to update the list and your organization receives updates automatically.

This inherits the IP list registered on the Cursor GitHub App. It does not allow every client that happens to use those addresses.

To enable this:

1. Go to your organization's Security settings
2. Navigate to IP allow list settings
3. Check **"Enable IP allow list configuration for installed GitHub Apps"**

For detailed instructions, see [GitHub's documentation](https://docs.github.com/en/enterprise-cloud@latest/organizations/keeping-your-organization-secure/managing-security-settings-for-your-organization/managing-allowed-ip-addresses-for-your-organization#allowing-access-by-github-apps).

### Add IPs directly to your allowlist

If your organization uses IdP-defined allowlists in GitHub or otherwise cannot use the pre-configured allowlist, add the proxy IPs listed in [Git egress proxy and IP allow list](https://cursor.com/docs/cloud-agent/security-network.md#git-egress-proxy-and-ip-allow-list).

## Advanced networking

Self-hosted instances support multiple connection methods beyond IP whitelisting. For setup details and supported private networking options, see [Private Connectivity](https://cursor.com/docs/cloud-agent/private-connectivity.md).

### AWS PrivateLink

Available for Enterprise customers. Use AWS PrivateLink when your GitHub Enterprise Server is in AWS or can sit behind an AWS Network Load Balancer. PrivateLink can cover Cursor accessing GHES and, when needed, GHES sending webhooks back to Cursor without public internet egress.

**Best for:** AWS-hosted GHES instances and teams that want private VPC endpoint connectivity

**Security:** HTTPS encryption, AWS PrivateLink, VPC endpoint policies, service account access tokens

**Drawbacks:** Requires coordination with Cursor and AWS endpoint service setup.

### Cloudflare Tunnel

Available for Enterprise customers. Use Cloudflare Tunnel when AWS PrivateLink is not practical or when you need an outbound-only deployment model. Your network runs `cloudflared`, and Cursor provides the tunnel hostname and token.

**Best for:** Environments without inbound network access

**Security:** HTTPS encryption, Cloudflare Tunnel, service account access tokens

**Drawbacks:** Requires running and maintaining `cloudflared` in your environment.

## Permissions

The GitHub app requests the following permissions to support Cursor features:

| Permission                         | Purpose                                                                      |
| ---------------------------------- | ---------------------------------------------------------------------------- |
| **Repository access**              | Clone your code and create working branches                                  |
| **Pull requests**                  | Create PRs and leave review comments                                         |
| **Issues**                         | Track bugs and tasks discovered during reviews                               |
| **Checks and statuses**            | Report on code quality and test results                                      |
| **Actions and workflows**          | Monitor CI/CD pipelines and trigger CI re-runs from pull requests            |
| **Administration**                 | Read branch protection and required check rules to determine PR mergeability |
| **Custom repository roles**        | Determine user access levels so the correct merge and review options appear  |
| **Organization custom properties** | Surface organization-defined repository metadata in filtering                |

All permissions follow the principle of least privilege.

## Protected Git Scopes

Lock your GitHub organization to your Cursor organization so only your teams can use its repositories with Cloud Agents, automations, and Bugbot. Protecting a scope requires GitHub organization owner or admin access. See [Protected Git Scopes](https://cursor.com/docs/enterprise/model-and-integration-management.md#protected-git-scopes).

## Troubleshooting

### Agent can't access repository

- Install the GitHub app with repository access
- Check repository permissions for private repos
- Verify your GitHub account permissions

### Permission denied for pull requests

- Grant the app write access to pull requests
- Check branch protection rules
- Reinstall if the app installation expired

### App not visible in GitHub settings

- Check if installed at organization level
- Reinstall from [github.com/apps/cursor](https://github.com/apps/cursor)
- Contact support if installation is corrupted

## Next steps

Once your GitHub integration is connected, configure the features that use it:

- [Bugbot](https://cursor.com/docs/bugbot.md) — automated PR reviews that catch bugs and security issues
- [Cloud Agents](https://cursor.com/docs/cloud-agent.md) — AI agents that run in the cloud on your repositories


---

## Sitemap

[Overview of all docs pages](/llms.txt)
