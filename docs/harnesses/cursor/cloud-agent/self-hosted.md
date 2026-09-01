# Governed Cloud Agents for private code and internal services

Coding agents that work on private code have to clear a specific bar: source that can't leave your network, a compliance perimeter to honor, access and network policy you control, and a setup that fits the infrastructure you already run.

Cursor-managed Cloud Agents are built for that bar. Agents reach your private systems over connectivity you control, run under the policies you set, and keep your code inside your perimeter, while Cursor operates the hosts.

What that looks like in practice:

- Private source and internal services stay reachable without exposing them to the public internet.
- Code and sensitive data stay inside your security and compliance perimeter.
- Network and access policy stays yours to define and enforce.
- There's no agent fleet to size, patch, scale, or keep on call.

## Reach private code and internal services

You don't need to own the compute to keep agents inside your security perimeter. Cursor-managed Cloud Agents connect to private Git providers and internal services through the same private-network paths the rest of your infrastructure uses:

- **AWS PrivateLink.** The preferred path for AWS environments and self-hosted GitHub Enterprise Server, GitLab Enterprise, and private source control APIs. Traffic stays on private endpoints instead of crossing the public internet.
- **Cloudflare Tunnel.** An outbound-only tunnel for reaching a private origin when PrivateLink isn't practical.
- **Tailscale or a similar client.** Userspace networking inside the agent environment for services in your VPC or intranet.

See [Private connectivity](https://cursor.com/docs/cloud-agent/private-connectivity.md) for AWS PrivateLink and Cloudflare Tunnel setup, and [Cloud Agent security and network](https://cursor.com/docs/cloud-agent/security-network.md#private-network-access) for private network access and Tailscale.

## Guardrails and policies you control

Governance comes from the controls you set. Every Cursor-managed Cloud Agent runs in an isolated cloud VM, and you decide what it can reach and what it retains:

- Isolated VMs per agent, provisioned and torn down by Cursor.
- [Network allowlists](https://cursor.com/docs/cloud-agent/security-network.md#network-access) that restrict outbound domains by user, team, or environment.
- Privacy Mode, so your code isn't used for training and is retained only to run the agent.
- Customer-controlled secrets, scoped to the environments you choose.

You own repository access, secrets, and network policy. Cursor manages host provisioning, isolation, and the environment lifecycle. See [Cloud Agent security and network](https://cursor.com/docs/cloud-agent/security-network.md) for the full model.

## Operations and cost

Managed Cloud Agents take the fleet off your plate. There's no worker pool to size, patch, scale, reset, or monitor, and no on-call rotation for agent hosts. Cursor handles VM provisioning, isolation, and scaling.

Cost follows [model pricing](https://cursor.com/docs/models-and-pricing.md#model-pricing). The compute an agent runs on is part of the managed service, so you don't reserve worker capacity or take on a separate cloud infrastructure bill for agent VMs.

Need to run agents on your own infrastructure for a regulated or complex setup? Learn more about our [self-hosted option](https://cursor.com/docs/cloud-agent/self-hosted-guides/pool.md).

## Next steps

- [Cloud Agents overview](https://cursor.com/docs/cloud-agent.md) for how managed agents work.
- [Cloud Agent setup](https://cursor.com/docs/cloud-agent/setup.md) to configure environments, secrets, and network access.
- [Private connectivity](https://cursor.com/docs/cloud-agent/private-connectivity.md) for AWS PrivateLink and Cloudflare Tunnel.
- [Cloud Agent security and network](https://cursor.com/docs/cloud-agent/security-network.md) for the security and retention model.

### Bring Cloud Agents to your enterprise

Talk to sales about private connectivity, security controls, and rollout for Cursor-managed Cloud Agents.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
