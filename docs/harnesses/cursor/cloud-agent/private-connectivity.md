# Private Connectivity

Cursor supports private network connectivity for Enterprise teams that need Cursor to work with systems that are not reachable from the public internet. This includes self-hosted GitHub Enterprise Server, GitLab Enterprise, Bitbucket Data Center, Artifactory, Nexus, private source control APIs, and webhook traffic from those systems back to Cursor.

The same private connectivity setup is used across Cursor services that need access to your source control system, including [Cloud Agents](https://cursor.com/docs/cloud-agent.md), [Bugbot](https://cursor.com/docs/bugbot.md), and Cursor backend services.

To set up private connectivity, contact [hi@cursor.com](mailto:hi@cursor.com) or your Cursor sales representative.

## Supported options

| Option            | Best for                                                                                                                | Cloud provider                             | Status    |
| :---------------- | :---------------------------------------------------------------------------------------------------------------------- | :----------------------------------------- | :-------- |
| AWS PrivateLink   | Private connectivity between Cursor and your Git provider or package registry, including webhook traffic back to Cursor | AWS                                        | Supported |
| Cloudflare Tunnel | Cursor accessing a private origin when AWS PrivateLink is not practical                                                 | Any environment that can run `cloudflared` | Supported |

## How to choose

Use AWS PrivateLink when your private Git provider or package registry is in AWS or can sit behind an AWS Network Load Balancer. This is the preferred path for self-hosted GitHub Enterprise Server and GitLab Enterprise.

AWS PrivateLink can cover two traffic directions:

- Cursor accessing your private Git provider to clone repositories and call Git APIs.
- Your Git provider sending webhooks or callbacks to Cursor over `api2.cursor.sh` without public internet egress.

Use Cloudflare Tunnel when you cannot publish an AWS endpoint service or when you need a deployment model that only requires an outbound tunnel from your network.

If your team requires Google Private Service Connect (PSC), contact Cursor. Cursor does not currently offer a customer-facing PSC service.

## Prerequisites

Before starting, make sure you have:

- A Cursor Enterprise workspace
- A self-hosted GitHub Enterprise Server, GitLab Enterprise, Bitbucket Data Center, or private package registry (such as Artifactory or Nexus) reachable over HTTPS on port 443
- A publicly trusted TLS certificate for the Git or registry hostname
- DNS ownership for that hostname
- AWS permissions to create endpoint services or interface VPC endpoints, if using AWS PrivateLink
- Permission to run `cloudflared`, if using Cloudflare Tunnel

Cursor does not support self-signed certificates, unencrypted connections, SSH, custom ports, or IPv6-only endpoint services for these private connectivity paths.

If you run a proxy in front of GitHub Enterprise Server, make sure it allows Cursor's GitHub App integration to use authenticated GitHub REST and GraphQL APIs.

## AWS PrivateLink

AWS PrivateLink supports private traffic in either direction between Cursor and your Git provider or package registry. You may need one direction or both, depending on your network policy.

### Direction 1: Cursor to your Git provider or package registry

Use this option when Cursor needs to clone repositories, call Git APIs, or reach a private package registry such as Artifactory or Nexus.

#### 1. Create an AWS endpoint service

Create a Network Load Balancer in front of your Git provider or package registry HTTPS endpoint. Publish that load balancer as an AWS VPC endpoint service.

Send Cursor:

- Endpoint service name, for example `com.amazonaws.vpce.us-east-1.vpce-svc-0123456789abcdef0`
- AWS region
- Git or registry hostname, for example `github.example.com` or `artifactory.example.com`
- Whether your endpoint service has AWS-managed private DNS enabled
- Whether your Network Load Balancer preserves client IPs or your backend filters source IPs

If your endpoint service is outside `us-east-1`, enable cross-region access on the endpoint service.

#### 2. Allow Cursor's AWS principal

Cursor will provide the AWS principal to add to your endpoint service allowed principals. Add the exact principal Cursor provides:

```text
arn:aws:iam::<cursor-aws-account-id>:role/<cursor-provided-role>
```

Cursor cannot create its interface endpoint until this principal is allowed. If the principal is missing or does not match exactly, AWS returns `InvalidServiceName`.

If your load balancer preserves client IPs, or if your backend filters source IPs, allow these Cursor PrivateLink subnet CIDRs:

```text
10.2.8.0/21
10.2.24.0/21
10.2.40.0/21
```

#### 3. Accept the endpoint connection

After Cursor creates the interface endpoint, accept the endpoint connection in your AWS account if your endpoint service requires manual acceptance.

#### 4. Configure DNS

If your endpoint service exposes AWS-managed private DNS for your Git or registry hostname, Cursor enables private DNS on its interface endpoint.

If your endpoint service does not expose private DNS, Cursor creates private DNS on its side and maps that hostname to the endpoint DNS name.

Use the same hostname in Cursor that appears on the TLS certificate and in DNS.

### Direction 2: Your Git provider to `api2.cursor.sh`

Use this option when your GitHub Enterprise Server or GitLab Enterprise host cannot reach the public internet but still needs to send webhooks or callbacks to Cursor.

Cursor publishes an AWS PrivateLink endpoint service for `api2.cursor.sh`. You create an interface VPC endpoint in your AWS account and enable private DNS so `api2.cursor.sh` resolves to private endpoint IPs from your Git provider network.

#### Endpoint service details

Cursor will confirm your AWS principal is allowlisted before you create the endpoint.

| Field                      | Value                                                                                |
| :------------------------- | :----------------------------------------------------------------------------------- |
| Service name               | `com.amazonaws.vpce.us-east-1.vpce-svc-054b15427d4bea2b7`                            |
| Service ID                 | `vpce-svc-054b15427d4bea2b7`                                                         |
| Home region                | `us-east-1`                                                                          |
| Supported consumer regions | `us-east-1`, `us-east-2`, `us-west-2`, `eu-central-1`, `eu-west-1`, `ap-southeast-2` |
| IP address types           | IPv4 only                                                                            |
| Private DNS name           | `api2.cursor.sh`                                                                     |

#### Mode 1: AWS-managed private DNS

This is the recommended mode. Set `private_dns_enabled = true`.

```hcl
resource "aws_vpc_endpoint" "cursor_api2" {
  vpc_id              = aws_vpc.app.id
  service_name        = "com.amazonaws.vpce.us-east-1.vpce-svc-054b15427d4bea2b7"
  service_region      = "us-east-1"
  vpc_endpoint_type   = "Interface"
  subnet_ids          = [for subnet in aws_subnet.app_private : subnet.id]
  private_dns_enabled = true
  security_group_ids  = [aws_security_group.cursor_api2_endpoint.id]
}
```

AWS associates your VPC with the managed private hosted zone for `api2.cursor.sh`. Inside the VPC, `api2.cursor.sh` resolves to the endpoint ENI IPs. No Route 53 record is required.

#### Mode 2: Customer-managed private hosted zone

Use this mode if you want to own the DNS record. Set `private_dns_enabled = false`, then create a private hosted zone for `api2.cursor.sh` scoped to the consumer VPC.

```hcl
resource "aws_vpc_endpoint" "cursor_api2" {
  vpc_id              = aws_vpc.app.id
  service_name        = "com.amazonaws.vpce.us-east-1.vpce-svc-054b15427d4bea2b7"
  service_region      = "us-east-1"
  vpc_endpoint_type   = "Interface"
  subnet_ids          = [for subnet in aws_subnet.app_private : subnet.id]
  private_dns_enabled = false
  security_group_ids  = [aws_security_group.cursor_api2_endpoint.id]
}

resource "aws_route53_zone" "cursor_api2" {
  name    = "api2.cursor.sh"
  comment = "Customer-managed PHZ for api2.cursor.sh scoped to the app VPC."

  vpc {
    vpc_id = aws_vpc.app.id
  }
}

resource "aws_route53_record" "cursor_api2_a" {
  zone_id = aws_route53_zone.cursor_api2.zone_id
  name    = "api2.cursor.sh"
  type    = "A"

  alias {
    name                   = aws_vpc_endpoint.cursor_api2.dns_entry[0].dns_name
    zone_id                = aws_vpc_endpoint.cursor_api2.dns_entry[0].hosted_zone_id
    evaluate_target_health = false
  }
}
```

If GitHub Enterprise Server or GitLab Enterprise uses DNS outside the endpoint VPC, forward `api2.cursor.sh` queries to the VPC resolver or create an equivalent private DNS override. Do not create a public DNS override.

## Cloudflare Tunnel

Use Cloudflare Tunnel when AWS PrivateLink is not a fit.

Cursor creates the tunnel and shares:

- A public hostname under Cursor-controlled DNS
- A tunnel token through a secure 1Password share
- A sample `cloudflared` configuration

Your network runs `cloudflared` and opens outbound connections to Cloudflare. No inbound firewall rule is required.

Example `cloudflared` configuration:

```yaml
ingress:
  - hostname: <cursor-provided-hostname>
    service: https://<your-internal-service>:443
  - service: http_status:404
```

Example run command:

```bash
docker run -d --restart=always --name cloudflared \
  -v /path/to/config.yml:/etc/cloudflared/config.yml \
  cloudflare/cloudflared:latest \
  tunnel --config /etc/cloudflared/config.yml \
  run --token <TUNNEL_TOKEN>
```

Keep the tunnel token secret. Do not send it through email or chat.

## Complete the source control connection

After private networking is configured, complete the source control setup in Cursor:

- For GitHub Enterprise Server, follow the [GitHub integration setup](https://cursor.com/docs/integrations/github.md#setup).
- For GitLab Enterprise, follow the [GitLab integration setup](https://cursor.com/docs/integrations/gitlab.md#setup).
- For Bitbucket Data Center, follow the [Bitbucket integration setup](https://cursor.com/docs/integrations/bitbucket.md#setup).
- Use the same hostname that is covered by your TLS certificate and private DNS configuration.
- If a proxy sits in front of your Git provider, make sure it allows the authenticated API traffic described in [Prerequisites](https://cursor.com/docs/cloud-agent/private-connectivity.md#prerequisites).

Cursor uses the connected source control integration for Cloud Agents, Bugbot, and other Cursor services that need repository access.

### Check the private webhook path

If your Git provider sends webhooks to Cursor through the `api2.cursor.sh` PrivateLink path, run these checks from the same network path used by GitHub Enterprise Server or GitLab Enterprise:

```bash
getent hosts api2.cursor.sh
# or, if dig is available
dig +short api2.cursor.sh
curl -sS https://api2.cursor.sh/
```

Every resolved IP should be inside your consumer VPC CIDR. If you see public IPs such as `3.x.x.x` or `44.x.x.x`, private DNS is not in effect.

The `curl` request should return HTTP `200` with a body that starts with `Welcome to Cursor.` That response means the request reached a live Cursor `api2` backend.

## Troubleshooting

| Symptom                                                                                            | Likely cause                                                                                        | Fix                                                                                                                                       |
| :------------------------------------------------------------------------------------------------- | :-------------------------------------------------------------------------------------------------- | :---------------------------------------------------------------------------------------------------------------------------------------- |
| Cursor cannot complete the private connection to your Git provider                                 | Cursor cannot reach or attach to the endpoint service                                               | Confirm the endpoint service name, region, and allowed principal match the values Cursor provided, then contact Cursor with the timestamp |
| Cursor reports that the endpoint connection is waiting for customer action                         | The endpoint service requires approval in your AWS account                                          | Review pending endpoint connection requests for the service and approve the Cursor request                                                |
| Bugbot or Cloud Agents connect to GHES but fail during app setup, repo sync, or webhook processing | A proxy in front of GHES is blocking or rewriting authenticated GitHub REST or GraphQL API requests | Allow Cursor's GitHub App integration to use authenticated GitHub REST and GraphQL APIs                                                   |
| `api2.cursor.sh` resolves to public IPs                                                            | Private DNS is not in the resolver path used by GitHub Enterprise Server or GitLab Enterprise       | Enable AWS-managed private DNS, or forward DNS to the endpoint VPC resolver                                                               |
| TCP to `api2.cursor.sh:443` times out                                                              | Security group, NACL, route table, or firewall blocks traffic to endpoint ENIs                      | Allow TCP 443 from your Git provider network to the endpoint ENIs                                                                         |
| TLS fails for `api2.cursor.sh`                                                                     | DNS points to the wrong target or the client is not using SNI                                       | Check endpoint DNS and retry with SNI enabled                                                                                             |
| `curl https://api2.cursor.sh/` does not return `Welcome to Cursor.`                                | Traffic is not reaching a healthy Cursor backend                                                    | Send Cursor the timestamp, source VPC, and resolved endpoint IPs                                                                          |
| Cloudflare Tunnel does not connect                                                                 | `cloudflared` cannot reach Cloudflare or the token/config is wrong                                  | Check outbound firewall rules, token, and `cloudflared` logs                                                                              |

## Google Private Service Connect

Cursor does not currently offer customer-facing Google Private Service Connect.

If you need private connectivity from a GCP VPC to Cursor services, or from Cursor to a private service in your GCP project, contact Cursor so we can scope the requirement. Today, use AWS PrivateLink or Cloudflare Tunnel when those deployment models fit.

## What to send Cursor

For AWS PrivateLink to your Git provider or package registry:

- Endpoint service name
- AWS region
- Git or registry hostname
- Whether private DNS is enabled
- Whether your load balancer preserves client IPs or filters source IPs

For `api2.cursor.sh` over AWS PrivateLink:

- AWS principal Cursor should allowlist
- VPC and region where you will create the interface endpoint
- Whether you plan to use AWS-managed private DNS or customer-managed DNS

For Cloudflare Tunnel:

- Internal origin URL
- Customer contacts for the secure 1Password share
- Any hostname or naming restrictions

## Further reading

- [AWS: Create an endpoint service](https://docs.aws.amazon.com/vpc/latest/privatelink/create-endpoint-service.html)
- [AWS: Manage DNS names for VPC endpoint services](https://docs.aws.amazon.com/vpc/latest/privatelink/manage-dns-names.html)
- [AWS: Access an AWS service using an interface VPC endpoint](https://docs.aws.amazon.com/vpc/latest/privatelink/create-interface-endpoint.html)
- [Cloudflare Tunnel documentation](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
