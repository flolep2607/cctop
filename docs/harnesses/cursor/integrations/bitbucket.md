# Bitbucket

Connect Bitbucket Cloud repositories to [Cloud Agents](https://cursor.com/docs/cloud-agent.md) and [Bugbot](https://cursor.com/docs/bugbot.md). Connect Bitbucket Data Center repositories to Bugbot.

## Setup

### Bitbucket Cloud

The Bitbucket Cloud integration is in public beta. It supports repositories on `bitbucket.org`.

Bitbucket Cloud setup has two parts, and each part requires a different role:

- Each developer connects their Bitbucket account so Cursor can clone repositories, push branches, and open pull requests as that user.
- A Bitbucket workspace admin installs the Cursor app so Bugbot and Cloud Agent status comments can appear as Cursor. Linking the installed app to your Cursor team requires a Cursor team admin with access to the Bitbucket workspace.

### Connect your Bitbucket account

Requires access to the Bitbucket workspace repositories you want to use.

1. Go to [Integrations in the dashboard](https://cursor.com/dashboard/integrations)
2. Click **Connect** next to Bitbucket
3. Authorize Cursor in Bitbucket
4. Return to the dashboard and confirm the integration shows **Connected**

[Bitbucket setup](/docs-static/images/bitbucket/bitbucket-demo.mp4)

Each person who starts Cloud Agents or opens pull requests from Cursor should connect their own Bitbucket account.

### Install the Cursor app in Bitbucket

A Bitbucket workspace admin must install the Cursor app for the workspace. This gives Cursor a stable app identity for repository events, Bugbot review comments, inline comments, and status updates.

Steps 1 through 4 require a Bitbucket workspace admin. Steps 5 and 6 require a Cursor team admin with access to the Bitbucket workspace. If one person handles the full setup, they need both roles.

1. Go to [Integrations in the dashboard](https://cursor.com/dashboard/integrations)
2. Find Bitbucket and open the manage menu
3. Click **Install Cursor app**
4. Install the app in your Bitbucket workspace
5. In Bitbucket workspace settings, under **Forge Apps**, select **Cursor**
6. Click **Connect to Cursor** and choose the Cursor team to link

When the workspace is linked, the dashboard shows the Cursor app installed for that Bitbucket workspace.

Bitbucket can take a few minutes to notify Cursor after app installation. If Cursor cannot find the workspace right away, wait and try the link step again.

### How Cursor uses Bitbucket Cloud identities

Cursor uses different Bitbucket identities for different actions:

| Action                                                       | Bitbucket identity           |
| ------------------------------------------------------------ | ---------------------------- |
| Bugbot summary comments, inline comments, and build statuses | Cursor app                   |
| Cloud Agent progress comments on pull requests               | Cursor app                   |
| Git clone, branch push, commits, and pull request creation   | The connected Bitbucket user |

Pull requests opened by Cloud Agents are attributed to the person who started the agent. Bugbot output appears from Cursor when the workspace app is installed.

### Permissions

The Bitbucket Cloud integration asks for permissions needed to support Cloud Agents and Bugbot:

| Permission             | Purpose                                    |
| ---------------------- | ------------------------------------------ |
| **Account**            | Identify the connected Bitbucket user      |
| **Repositories**       | List, clone, and read repository content   |
| **Pull requests**      | Read pull request diffs and metadata       |
| **Pull request write** | Create pull requests and post comments     |
| **Webhooks**           | Receive repository and pull request events |

### Disconnect Bitbucket Cloud

Disconnecting Bitbucket Cloud has two separate effects:

- **Disconnect account** removes your personal Bitbucket OAuth connection from Cursor.
- **Disconnect Cursor app** unlinks the Bitbucket workspace from your Cursor team. The app can remain installed in Bitbucket until a workspace admin removes it there.

To fully stop app events from the workspace, uninstall the Cursor app from Bitbucket workspace settings.

### Bitbucket Data Center

Bitbucket Data Center requires a [Teams](https://cursor.com/docs/account/teams/pricing.md) or [Enterprise](https://cursor.com/docs/enterprise.md) plan. It supports Bugbot. Cloud Agents are not supported.

### Prerequisites

- Cursor team admin access
- A dedicated Bitbucket Data Center service account
- An HTTP access token for the service account
- Repository admin access for the service account on each repository you want to use with Bugbot

The service account reads repositories and pull requests, manages webhooks, and posts Bugbot comments and build statuses.

### Networking

Cursor needs HTTPS access to your Bitbucket Data Center instance for API requests and Git clones. Your instance also needs outbound HTTPS access to send webhook notifications to Cursor.

#### IP allowlisting (recommended)

Add these Cursor IP addresses to your inbound allowlist:

```text
184.73.225.134
3.209.66.12
52.44.113.131
```

If Cursor should use a load balancer or API hostname that differs from the repository clone hostname, enter it as the external host during registration.

For instances without public inbound access, see [Advanced networking](https://cursor.com/docs/integrations/bitbucket.md#advanced-networking).

### Register with Cursor

1. Create an HTTP access token for the Bitbucket service account
2. Go to [Integrations in the dashboard](https://cursor.com/dashboard/integrations)
3. Open **Advanced**, then select **Bitbucket Data Center**
4. Enter the **Bitbucket Hostname** used in repository clone URLs
5. If API traffic uses a different hostname, enter it as the **External Host**
6. Enter the **Service Account Token**
7. Click **Register**
8. Open [Bugbot in Automations](https://cursor.com/automations/from-cursor/bugbot) to enable it on repositories from the instance

Cursor uses the service account identity for Bugbot review comments, inline findings, webhooks, and build statuses.

### Disconnect Bitbucket Data Center

Go to [Integrations in the dashboard](https://cursor.com/dashboard/integrations), open the Bitbucket Data Center configuration page, and delete the registered instance. Revoke the service account token in Bitbucket Data Center after removing the instance from Cursor.

## Advanced networking

Bitbucket Data Center supports private connection methods for instances that cannot accept traffic from the public internet.

### AWS PrivateLink or Cloudflare Tunnel

Available for Enterprise customers. Allow Cursor to access your instance over a private network connection. See [Private Connectivity](https://cursor.com/docs/cloud-agent/private-connectivity.md) or [contact your Cursor representative](https://cursor.com/contact-sales?source=docs-bugbot-private-network) for setup.

**Best for:** Instances behind a firewall, including AWS-hosted instances and environments that can run `cloudflared`

**Security:** HTTPS encryption, AWS PrivateLink, Cloudflare Tunnel, VPC allowlisting, service account access tokens

**Drawbacks:** Requires coordination with Cursor. Google Private Service Connect is not currently supported.

### Reverse Proxy Tunnel

Available for Enterprise customers. Run a reverse proxy tunnel on-premises that establishes an outbound connection to Cursor. Network requests are forwarded through the tunnel, so the Bitbucket instance does not need public inbound access. [Contact your Cursor representative](https://cursor.com/contact-sales?source=docs-bugbot-on-prem-proxy) for setup.

**Best for:** Environments without inbound network access

**Security:** HTTPS encryption, service account access tokens

**Drawbacks:** Introduces additional complexity, maintenance requirements, and potential security considerations compared to more direct connection methods

## Troubleshooting

### Cursor cannot find my Bitbucket workspace

Bitbucket app installation events can take a few minutes to sync. Wait and retry the **Connect to Cursor** step from Bitbucket workspace settings.

### I installed the Cursor app but cannot link it to my Cursor team

- Confirm you are a Cursor team admin. Installing the app requires a Bitbucket workspace admin, but linking the workspace to a Cursor team requires a Cursor team admin.
- Confirm your Bitbucket user has access to the workspace you are linking.

### Bugbot comments do not appear as Cursor

Install the Cursor app in the Bitbucket workspace and link it to your Cursor team. A personal Bitbucket connection alone is not enough for Cursor app comments.

### Cloud Agent cannot access a Bitbucket repository

- Connect your personal Bitbucket account from the integrations dashboard.
- Confirm your Bitbucket user has read-write access to the repository.
- Confirm the repository is on Bitbucket Cloud at `bitbucket.org`.

### Cursor cannot list my Bitbucket Data Center repositories

- Confirm the service account token is active.
- Confirm the service account can access the repositories.
- Confirm Cursor can reach the instance hostname or configured external host over HTTPS.

### I cannot enable Bugbot on a Bitbucket Data Center repository

- Confirm you are a Cursor team admin.
- Confirm the registered instance has a service account token.
- Confirm the service account has repository admin access.

## Next steps

Once Bitbucket is connected, configure the features that use it:

- [Bugbot](https://cursor.com/docs/bugbot.md) - automated PR reviews that catch bugs and security issues
- [Cloud Agents](https://cursor.com/docs/cloud-agent.md) - AI agents for Bitbucket Cloud repositories


---

## Sitemap

[Overview of all docs pages](/llms.txt)
