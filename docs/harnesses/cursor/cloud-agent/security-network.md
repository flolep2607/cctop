# Secrets & Network

Cloud Agents are available in Privacy Mode. We never train on your code and only retain code for running the agent. [Learn more about Privacy mode](https://www.cursor.com/privacy-overview).

For a walkthrough of how Cloud Agents are architected and secured, including the run lifecycle, access model, isolation, encryption, and data handling, see the [Security overview](https://cursor.com/docs/cloud-agent/security.md). This page is the configuration reference for the controls it describes.

**Privacy Mode (Legacy)** is not supported. Legacy privacy mode blocks cloud
data storage, and Cloud Agents need to store code and environment data in the
cloud while they run. Switch to Privacy Mode from [Dashboard → Cloud
Agents](https://cursor.com/dashboard/cloud-agents) before using Cloud Agents.

## Secret protection

Secrets provided to Cloud Agents are encrypted at rest and in transit. They are not visible to anyone other than the Cloud Agent user.

Secrets can be set as Environment Variables, Runtime Secrets, or Build Secrets.

### Environment Variables

Secrets set with type `Environment Variable` are visible to the cloud agent. These are best used for non-sensitive configuration that is helpful for the agent to view, such as flags or public URLs. They are still encrypted at rest and in transit as with other secret types.

### Runtime secrets

Previously, Runtime Secrets were called Redacted Secrets.

Secrets set with type `Runtime Secret` are still loaded as environment variables, but their contents are redacted from the agent's tool call results, chat transcript, commits, and commit messages, and replaced with the placeholder string `[REDACTED]`. These are best used for sensitive credentials that should not be exposed to the agent and should never be committed to the repository.

Because Runtime Secrets still function internally as environment variables, while they are not shown to the agent, they are still visible to users interacting with the agent's environment via the Terminal.

### Build secrets

Secrets set with type `Build Secret` are only available to the [Docker build process](https://cursor.com/docs/cloud-agent/security-network.md#manual-setup-with-dockerfile-advanced) (if you have configured one) and are not exposed to the running agent's environment. These are best used for private package registries or build-time credentials that should not be exposed to the agent.

In order to securely use a Build Secret within your Dockerfile, reference them from a `RUN` step using a [Docker secret mount](https://docs.docker.com/build/building/secrets/#secret-mounts), for example:

```docker
RUN --mount=type=secret,id=MY_TOKEN,env=MY_TOKEN,required=true \
    ./scripts/install-private-deps.sh
```

## OIDC identity tokens

For cloud roles and internal APIs, prefer short-lived [OIDC tokens](https://cursor.com/docs/cloud-agent/identity.md) over long-lived keys in Secrets. A Cloud Agent can mint a Cursor-signed JWT from a local socket and present it to AWS, GCP, Azure, Vault, or any OIDC verifier.

## Signed commits

Cloud Agents sign every commit with a HSM-backed Ed25519 key. On GitHub and GitLab, these commits display a "Verified" badge so your team can confirm the commit came from Cursor.

This works automatically for all Cloud Agents. No setup is required.

If your repository enforces branch protection rules that require signed commits, Cloud Agent PRs satisfy those rules without extra configuration.

## Protected Git Scopes

Team admins can lock a Git organization to your Cursor organization so only your teams can start Cloud Agents on its repositories. See [Protected Git Scopes](https://cursor.com/docs/enterprise/model-and-integration-management.md#protected-git-scopes).

## What you should know

1. Grant read-write privileges to our GitHub app for repos you want to edit. We use this to clone the repo and make changes.
2. Your code runs inside our AWS infrastructure in isolated VMs and is stored on VM disks while the agent is accessible.
3. The agent has internet access by default. You can configure [network egress controls](https://cursor.com/docs/cloud-agent/security-network.md#network-access) for users, teams, and saved environments to restrict the domains the agent can access.
4. The agent auto-runs all terminal commands, letting it iterate on tests. This differs from the foreground agent, which requires user approval for every command. Auto-running introduces data exfiltration risk: attackers could execute prompt injection attacks, tricking the agent to upload code to malicious websites. See [OpenAI's explanation about risks of prompt injection for cloud agents](https://platform.openai.com/docs/codex/agent-network#risks-of-agent-internet-access).
5. If privacy mode is disabled, we collect prompts and dev environments to improve the product.
6. If you disable privacy mode when starting a cloud agent, then enable it during the agent's run, the agent continues with privacy mode disabled until it completes.

## Data retention

Cloud Agents store two types of data for every run:

- **Conversation history.** The prompts, model responses, tool calls, and demo artifacts that make up the agent's transcript. This is the data you see when you open an agent on the web or from a desktop client.
- **Environment snapshots.** Encrypted point-in-time copies of the virtual machine disk. Snapshots let you customize VM environments and allow agents to start or resume without recloning the repository or running the setup again.

Conversation history is kept indefinitely by default so you can revisit and resume past runs. Environment snapshots are stored for a maximum of **90 days** of inactivity. Each time an agent starts or resumes from a snapshot, its expiry extends for another 90 days. Once a snapshot goes unused for 90 days, it's deleted automatically, regardless of plan or policy.

You can use the [Delete Agent API](https://cursor.com/docs/cloud-agent/api/endpoints.md#delete-an-agent-permanently) to explicitly delete a cloud agent's conversation history. This endpoint removes the conversation transcript and its artifacts. It doesn't delete environment snapshots, which can't be deleted on demand and instead follow the retention window above.

### Cloud agent retention policies

Custom retention windows are in early access for select Enterprise teams. [Contact sales](https://cursor.com/contact-sales?source=docs-cloud-agent-retention) to request access.

Enterprise team admins can cap how long the team's Cloud Agent data is kept from **Team Settings** on the [Cloud Agents dashboard](https://cursor.com/dashboard/cloud-agents). The available windows are **Indefinite** and **90 days**.

When you set the policy to **90 days**:

- A background job deletes conversations older than the retention policy window.
- Environment snapshots continue to follow the rolling 90-day inactivity window described above.
- The policy applies to every agent run the team owns, including runs from saved environments and the [API](https://cursor.com/docs/cloud-agent/api/v0.md).

Switching back to **Indefinite** stops further conversation deletions but doesn't restore data that's already been removed.

## Network access

Control which network resources your Cloud Agents can reach. These settings are available on the [Cloud Agents dashboard](https://cursor.com/dashboard/cloud-agents) for individual users, saved environments, and team admins.

### Private network access

Cloud Agents do not need to run on your hardware to reach private resources. For services in a VPC or intranet, use Tailscale userspace networking, Cloudflare Tunnel, or a similar private-network client in the Cloud Agent environment. See [Running Tailscale](https://cursor.com/docs/cloud-agent/setup.md#running-tailscale) and [Running Cloudflare Tunnel](https://cursor.com/docs/cloud-agent/setup.md#running-cloudflare-tunnel) for setup notes.

With either Tailscale or Cloudflare Tunnel, your private services do not need to accept inbound traffic from the public internet. The agent connects through an authenticated network path, while the service stays on your private network.

Cloudflare Tunnel is a good fit when the agent can reach the private service through an authenticated HTTPS hostname. A connector in your network dials out to Cloudflare, and the Cloud Agent calls that hostname like any other external URL. You can protect the hostname with Cloudflare Access service tokens, store the token values as Cursor Secrets, and add the hostname to your Cloud Agent allowlist.

For TCP targets such as private databases, use a tunnel client that exposes a local TCP listener in the agent environment. The agent then connects to `localhost`, while the tunnel forwards traffic to the private origin.

For private GitHub Enterprise Server, GitLab Enterprise, source control APIs, package registries such as Artifactory or Nexus, and related webhook traffic, Enterprise teams can use [private connectivity](https://cursor.com/docs/cloud-agent/private-connectivity.md) with AWS PrivateLink or Cloudflare Tunnel.

### Access modes

Three modes control outbound network access for Cloud Agents:

| Mode                         | Behavior                                                                                                                                                     |
| :--------------------------- | :----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Allow all network access** | Cloud Agents can reach any external host. No domain restrictions apply.                                                                                      |
| **Default + allowlist**      | Cloud Agents can reach the [default domains](https://cursor.com/docs/agent/security/run-modes.md#network-access) plus any domains you add to your allowlist. |
| **Allowlist only**           | Cloud Agents can only reach the domains you explicitly add to your allowlist.                                                                                |

Even in **Allowlist only** mode, a small set of domains remain accessible so Cloud Agents can function. These include Cursor's own services and source control management (SCM) providers.

### Artifact uploads

Cloud Agents upload [artifacts](https://cursor.com/docs/cloud-agent/capabilities.md#demos-and-artifacts) (screenshots, videos, and log references shown on PRs) to `cloud-agent-artifacts.s3.us-east-1.amazonaws.com`.

If you use **Default + allowlist** or **Allowlist only**, add the exact host to your allowlist so artifact uploads succeed. Don't broaden the entry to `*.s3.us-east-1.amazonaws.com`: the wildcard opens egress to every bucket in the region and creates an exfiltration path for a prompt-injected agent. Blocking the host disables uploads; agent sessions and other tool calls keep working.

### User-level settings

Individual users can configure their network access mode from the [Cloud Agents dashboard](https://cursor.com/dashboard/cloud-agents) under the **Security** header. Your user-level setting applies to all Cloud Agents you create.

When you select a mode that includes an allowlist (**Default + allowlist** or **Allowlist only**), an allowlist configuration section appears below the setting where you can add your custom domains.

### Environment-level settings

Saved environments can have their own network access mode and allowlist. Use environment-level settings when one repo or repo group needs stricter egress than the rest of your team.

For example, you can keep a production-adjacent environment on **Allowlist only** while leaving a less sensitive environment on **Default + allowlist**. Agents that use the stricter environment inherit those restrictions.

Environment-level settings include two inheritance options:

| Mode                                         | Behavior                                                                                  |
| :------------------------------------------- | :---------------------------------------------------------------------------------------- |
| **Inherit settings**                         | Uses the applicable user or team network access setting.                                  |
| **Inherit settings + environment allowlist** | Uses the applicable user or team setting and adds domains from the environment allowlist. |

You can also set an environment directly to **Allow all network access**, **Default + allowlist**, or **Allowlist only**.

### Team-level settings

Team admins can set a default network access mode for the entire team from the same dashboard. The team-level allowlist is the same allowlist that admins configure for the [sandbox default network allowlist](https://cursor.com/docs/agent/security/run-modes.md#network-access). There is no separate allowlist to manage; one allowlist controls both Cloud Agent network access and the sandbox defaults.

When a team-level setting exists:

- If an environment defines its own mode, the **environment setting applies** to agents that use that environment.
- If an environment inherits settings and a user has configured their own setting, the **user setting takes precedence**.
- If neither the environment nor the user has configured a setting, the **team default applies**.

### Locking the setting (Enterprise)

Locking is available for Enterprise teams only.

Enterprise team admins can lock the network access setting using the **Lock Network Access Policy** option. When locked:

- The team-level setting applies to every member, regardless of their individual preference.
- Users cannot override the locked setting from their own dashboard.

This gives admins full control over Cloud Agent network access across the organization.

### Relationship to sandbox network policy

The "Default" domains in the **Default + allowlist** mode are the same [default network allowlist](https://cursor.com/docs/agent/security/run-modes.md#network-access) used by the desktop Agent's sandbox. The team-level allowlist is also shared: when an admin configures an allowlist on the dashboard, it applies to both Cloud Agent network access and the [sandbox network policy](https://cursor.com/docs/reference/sandbox.md).

## Egress IP ranges

Cloud Agents make network connections from specific IP address ranges when accessing external services, APIs, or repositories.

### API endpoint

The IP ranges are available via a [JSON API endpoint](https://cursor.com/docs/ips.json):

```bash
curl https://cursor.com/docs/ips.json
```

#### Response format

```json
{
  "version": 1,
  "modified": "2025-09-24T16:00:00.000Z",
  "cloudAgents": {
    "us3p": ["100.26.13.169/32", "34.195.201.10/32", "..."],
    "us4p": ["54.184.235.255/32", "35.167.37.158/32", "..."],
    "us5p": ["3.12.82.200/32", "52.14.104.140/32", "..."]
  },
  "gitEgressProxy": ["184.73.225.134/32", "3.209.66.12/32", "52.44.113.131/32"]
}
```

- **version**: Schema version number for the API response
- **modified**: ISO 8601 timestamp of when the IP ranges were last updated
- **cloudAgents**: Object containing IP ranges, keyed by cluster
- **gitEgressProxy**: IP addresses used by the [git egress proxy](https://cursor.com/docs/cloud-agent/security-network.md#git-egress-proxy-and-ip-allow-list)

IP ranges published in [CIDR notation](https://en.wikipedia.org/wiki/Classless_Inter-Domain_Routing). You can use an online conversion tool to convert from CIDR notation to IP address ranges if needed.

### Using the IP ranges

These published IP ranges may be used by Cloud Agents to:

- Clone and push to remote repositories (unless using the [git egress proxy](https://cursor.com/docs/integrations/github.md#ip-allow-list-configuration))
- Download packages and dependencies
- Make API calls to external services
- Access web resources during agent execution

If your organization uses firewall rules or IP allowlists to control network access, you may need to allowlist these IP ranges to ensure Cloud Agents can properly access your services.

**Important considerations:**

- We make changes to our IP addresses from time to time for scaling and operational needs.
- We do not recommend allowlisting by IP address as your primary security mechanism.
- If you must use these IP ranges, we strongly encourage regular monitoring of the JSON API endpoint.

### Git egress proxy and IP allow list

Cursor supports a similar but distinct feature to [use a git egress proxy for IP allow lists](https://cursor.com/docs/integrations/github.md#ip-allow-list-configuration). This proxy routes all git traffic through a narrower set of IPs and works across all git hosts, including GitHub, GitLab, Azure DevOps, and Bitbucket.

For git hosts specifically, we recommend the IP allow list configuration described in the link above, as it integrates directly with the Cursor GitHub app.

If you need to add the proxy IPs directly to an allowlist, use these addresses:

```text
184.73.225.134
3.209.66.12
52.44.113.131
```

### Cursor Review IPs

If your team uses Cloud Agents alongside [Cursor Review](https://cursor.com/docs/cursor-review/overview.md), allowlist these additional IPs on top of the git egress proxy IPs above:

```text
34.192.39.182
50.16.106.255
44.217.29.124
3.223.245.201
54.164.185.10
34.194.133.23
35.170.116.221
```

These IP addresses are stable. If the list ever changes, teams using IP allow
lists will get advance notice before any address is added or removed.

Enterprise customers with private GitHub Enterprise Server or GitLab Enterprise deployments can use [private connectivity options](https://cursor.com/docs/cloud-agent/private-connectivity.md), so Cloud Agents and Bugbot can access private source control systems.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
