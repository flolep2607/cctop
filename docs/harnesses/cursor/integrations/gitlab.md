# GitLab

The GitLab integration connects your repositories so you can use features like [Cloud Agents](https://cursor.com/docs/cloud-agent.md) and [Bugbot](https://cursor.com/docs/bugbot.md).

## Setup

### GitLab.com

Requires Cursor admin access and GitLab maintainer access.

GitLab integration requires a **paid GitLab plan** (Premium or Ultimate). Project access tokens, which are required for this integration, are not available on GitLab Free.

1. Go to [Integrations in the dashboard](https://cursor.com/dashboard/integrations)
2. Click **Connect** next to GitLab (or **Manage Connections** if already connected)
3. Follow the GitLab installation flow
4. Back on the Integrations tab, click **Manage** next to your GitLab connection and select **Sync Repos**
5. Return to the dashboard to configure features on your repositories

[GitLab setup](/docs-static/images/bugbot/bugbot-gitlab.mp4)

To disconnect your GitLab account, return to the integrations dashboard and click **Disconnect Account**.

### GitLab Self-Hosted

GitLab integration requires a **paid GitLab plan** (Premium or Ultimate). Project access tokens, which are required for this integration, are not available on GitLab Free.

GitLab Self-Hosted requires a [Teams](https://cursor.com/docs/account/teams/pricing.md) or [Enterprise](https://cursor.com/docs/enterprise.md) plan.

### Networking

- GitLab self-hosted requires secure inbound access from Cursor and outbound access for webhook notifications.
- You need admin privileges on your GitLab instance to create the application.

#### IP whitelisting (recommended)

Add these IP addresses to your allowlist:

```text
184.73.225.134
3.209.66.12
52.44.113.131
```

For other connection options beyond IP whitelisting, see [Advanced networking](https://cursor.com/docs/integrations/gitlab.md#advanced-networking).

### Create GitLab application

1. In your GitLab instance, create a new application (Instance level preferred)
2. Set the redirect URI to `https://cursor.com/gitlab-connected`
3. Configure the application:
   - **Trusted**: `true`
   - **Confidential**: `true`
   - **Scopes**: `api` and `write_repository`
4. After creation, you'll receive an **Application ID** and **Secret**

### Register with Cursor

1. Go to [Integrations in the dashboard](https://cursor.com/dashboard/integrations) → **Advanced** → **GitLab Self-Hosted**
2. Enter your GitLab instance **hostname**
3. Paste the **Application ID** and **Secret**
4. Click **Register**
5. Select your GitLab instance from the dropdown
6. Click **Connect** to complete the installation
7. Back on the Integrations tab, click **Manage** next to your GitLab connection and select **Sync Repos**
8. Return to the dashboard to configure features on your repositories

## Advanced networking

Self-hosted instances support multiple connection methods beyond IP whitelisting.

### AWS PrivateLink or Cloudflare Tunnel

Available for Enterprise customers. Allow Cursor to access your instance over a private network connection. See [Private Connectivity](https://cursor.com/docs/cloud-agent/private-connectivity.md) or [contact your Cursor representative](https://cursor.com/contact-sales?source=docs-bugbot-private-network) for setup.

**Best for:** Instances behind a firewall on a private network, including AWS-hosted instances and environments that can run `cloudflared`

**Security:** HTTPS encryption, AWS PrivateLink, Cloudflare Tunnel, VPC allowlisting, service account access tokens

**Drawbacks:** Requires coordination with Cursor. Google Private Service Connect is not currently supported.

### Reverse Proxy Tunnel

Available for Enterprise customers. Run a reverse proxy tunnel on-premises that establishes a long-lived websocket connection to Cursor's servers. Network requests are forwarded through to your instance. Requires no inbound network access. [Contact your Cursor representative](https://cursor.com/contact-sales?source=docs-bugbot-on-prem-proxy) for setup.

**Best for:** Environments without inbound network access

**Security:** HTTPS encryption, service account access tokens

**Drawbacks:** Introduces additional complexity, maintenance requirements, and potential security considerations compared to more direct connection methods

## Protected Git Scopes

Lock your GitLab group or namespace to your Cursor organization so only your teams can use its repositories with Cloud Agents, automations, and Bugbot. Protecting a scope requires the GitLab Owner role. See [Protected Git Scopes](https://cursor.com/docs/enterprise/model-and-integration-management.md#protected-git-scopes).

## Next steps

Once your GitLab integration is connected, configure the features that use it:

- [Bugbot](https://cursor.com/docs/bugbot.md) — automated PR reviews that catch bugs and security issues
- [Cloud Agents](https://cursor.com/docs/cloud-agent.md) — AI agents that run in the cloud on your repositories


---

## Sitemap

[Overview of all docs pages](/llms.txt)
