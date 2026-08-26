# OIDC tokens

Cloud Agents can mint short-lived [OIDC](https://openid.net/specs/openid-connect-core-1_0.html) JWTs from inside the VM and use them to assume cloud roles or call internal services without storing long-lived credentials in [Secrets](https://cursor.com/docs/cloud-agent/security-network.md#secret-protection).

Agents call this API with their terminal tools. You don't need to run these requests yourself.

To have an agent mint tokens, include this in your prompt:

```text
To mint OIDC tokens, follow the instructions at
https://cursor.com/docs/cloud-agent/identity
```

This API is local to the agent VM. It is unrelated to the [Cloud Agents API](https://cursor.com/docs/cloud-agent/api/endpoints.md), which uses Cursor API keys and manages agents from outside the VM. The same socket also serves [agent metadata](https://cursor.com/docs/cloud-agent/metadata.md) for values that don't belong in a credential.

Cursor-managed Cloud Agent VMs serve the token socket. Every token they mint carries `agent_runtime: managed`.

## How it works

1. The agent calls the local socket and asks for a token with an audience the verifier expects.
2. Cursor signs an RS256 JWT bound to that agent and owner.
3. The agent sends the JWT to your cloud or verifier (AWS STS, GCP, Azure, Vault, or a service you run).
4. The verifier checks the signature against Cursor's published JWKS and authorizes on claims such as `sub`, `team_id`, or `cloud_agent_id`.

## Mint a token

The agent mints a token over the Unix socket at `CURSOR_AGENT_SOCKET`. On Cursor-managed VMs the default is `/run/cursor/api.sock`.

```bash
curl --unix-socket "${CURSOR_AGENT_SOCKET:-/run/cursor/api.sock}" \
  -H 'Content-Type: application/json' \
  -d '{"aud":"sts.amazonaws.com"}' \
  http://cursor-agent/v1/tokens/oidc
```

Requests are HTTP over a Unix socket. The hostname in the URL is ignored.

Include an optional `nonce` when the verifier expects replay binding:

```bash
curl --unix-socket "${CURSOR_AGENT_SOCKET:-/run/cursor/api.sock}" \
  -H 'Content-Type: application/json' \
  -d '{"aud":"https://oidc.example.com","nonce":"unpredictable-value"}' \
  http://cursor-agent/v1/tokens/oidc
```

### Request

`POST /v1/tokens/oidc` over the Unix socket. `Content-Type: application/json` is required. Maximum body size is 4 KB.

| Field       | Required | Description                                                                                                                                                                                                                                                                                                                                                                                        |
| :---------- | :------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `aud`       | Yes      | Audience string your verifier checks. Printable ASCII, no whitespace, up to 512 characters. Examples: `sts.amazonaws.com`, `https://oidc.example.com`.                                                                                                                                                                                                                                             |
| `nonce`     | No       | Opaque string echoed into the JWT `nonce` claim. Up to 512 characters.                                                                                                                                                                                                                                                                                                                             |
| `sub_claim` | No       | Claim name to put in `sub` as `<name>:<value>`, for verifiers that only match `sub` and `aud`. Up to 64 characters. Discovery lists the supported names in `x_cursor_sub_claims_supported`; currently `team_id`. Unsupported names are rejected. If the claim has no value for this agent, such as `team_id` on a personal account, the mint fails instead of falling back to the default subject. |

Cursor doesn't allowlist audiences. Your verifier must reject unexpected `aud` values.

### Response

```json
{
  "token": "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6Ii4uLiJ9...",
  "expires_at": 1785500000
}
```

| Field        | Description                                              |
| :----------- | :------------------------------------------------------- |
| `token`      | Signed JWT.                                              |
| `expires_at` | Expiration as Unix seconds. Matches the JWT `exp` claim. |

Tokens are valid for **5 minutes**. There is no refresh endpoint. Mint again when a new token is needed.

### When claims appear

[Install scripts](https://cursor.com/docs/cloud-agent/setup.md) can mint on the same socket. A token only includes claims that have a value when it is minted: `turn_id` and `turn_start` are absent until a coding turn starts, and `branch_name` is absent until the run records a branch. Owner, team, and repository claims are set from agent creation onward.

If the socket is missing right after boot, retry the connection.

## Verify a token

Publish these URLs to your identity provider or resource server:

| Endpoint  | URL                                                       |
| :-------- | :-------------------------------------------------------- |
| Issuer    | `https://api.cursor.com`                                  |
| Discovery | `https://api.cursor.com/.well-known/openid-configuration` |
| JWKS      | `https://api.cursor.com/keys`                             |

```bash
curl -sS https://api.cursor.com/.well-known/openid-configuration
curl -sS https://api.cursor.com/keys
```

Discovery follows [OpenID Connect Discovery 1.0](https://openid.net/specs/openid-connect-discovery-1_0.html). Tokens are minted on the agent VM, so the discovery document has no `authorization_endpoint` or `token_endpoint`.

### Older issuer URL

Cursor still serves a second discovery document at
`https://api2.cursor.sh/cloud-agent/identity`. Minted tokens no longer carry
that issuer. Point verifiers at `https://api.cursor.com`.

Check at least:

- Signature with RS256 and the JWKS `kid`
- `iss` is `https://api.cursor.com`
- `aud` is the audience your service expects
- `nbf` / `exp` with a small clock-skew allowance (`nbf` is 5 seconds before `iat`)
- `sub` or other claims your policy uses

Discovery includes `x_cursor_audience_bound: true`. Every token is minted for the caller-supplied `aud`. Don't accept a token issued for a different audience. Discovery also publishes `x_cursor_sub_claims_supported`, the claim names a mint request can project into `sub` with `sub_claim`.

## JWT claims

Header: `alg=RS256`, `typ=JWT`, plus `kid`.

| Claim                      | Always present        | Description                                                                                                                                                                                                                  |
| :------------------------- | :-------------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `iss`                      | Yes                   | `https://api.cursor.com`                                                                                                                                                                                                     |
| `sub`                      | Yes                   | Stable owner subject: `user:<id>` or `service_account:<id>` by default, or `<claim>:<value>` (for example `team_id:123`) when the mint request set `sub_claim`. Not an email.                                                |
| `aud`                      | Yes                   | Audience from the mint request.                                                                                                                                                                                              |
| `iat`                      | Yes                   | Issued-at, Unix seconds.                                                                                                                                                                                                     |
| `nbf`                      | Yes                   | Not-before (`iat - 5`).                                                                                                                                                                                                      |
| `exp`                      | Yes                   | Expiration (`iat + 300`).                                                                                                                                                                                                    |
| `jti`                      | Yes                   | Unique id per mint.                                                                                                                                                                                                          |
| `cloud_agent_id`           | Yes                   | Cloud Agent id (`bcId`).                                                                                                                                                                                                     |
| `nonce`                    | No                    | Present only when the mint request included one.                                                                                                                                                                             |
| `agent_runtime`            | Yes                   | `managed` on Cursor-managed Cloud Agent VMs.                                                                                                                                                                                 |
| `owner_email`              | When known            | Lowercased user email. Prefer `sub` or `owner_user_id` for allowlists; email can change.                                                                                                                                     |
| `owner_user_id`            | When known            | Cursor user id, as a decimal string.                                                                                                                                                                                         |
| `owner_service_account_id` | When known            | Service account id when a service account owns the agent.                                                                                                                                                                    |
| `team_id`                  | When known            | Owning team id, as a decimal string.                                                                                                                                                                                         |
| `turn_id`                  | When a turn is active | Id of this coding turn. Different from `cloud_agent_id`, which is the Cloud Agent id (`bcId`).                                                                                                                               |
| `turn_start`               | When a turn is active | Run start, Unix seconds.                                                                                                                                                                                                     |
| `repo_url`                 | When known            | Primary repository in `host/path` form, such as `github.com/acme/widgets`. Hostname is lowercased, with no scheme, credentials, port, query, or `.git` suffix. On a multi-repo agent, this is only the primary repository.   |
| `repo_urls`                | When known            | Every repository in the workspace, same form as `repo_url`. Primary repository first, then the rest sorted. Present only when the set is known complete. Missing means the set isn't known, not that there is only one repo. |
| `repo_count`               | When known            | Number of entries in `repo_urls`. Present exactly when `repo_urls` is. Use this with `repo_url` when your verifier can only match a single value (`repo_count == 1`).                                                        |
| `branch_name`              | When known            | Current branch.                                                                                                                                                                                                              |
| `environment_id`           | When known            | Id of the Cursor environment this run used.                                                                                                                                                                                  |
| `source`                   | When known            | How the agent was started, such as `WEBSITE`, `API`, `SLACK`, or `AUTOMATIONS`.                                                                                                                                              |
| `automation_id`            | For automations       | Automation id when `source` is automations.                                                                                                                                                                                  |

`repo_url` is the primary repository. To confine an agent to specific repositories, pin the complete set with `repo_urls`.

## Trust model

The token identifies the Cloud Agent run, not a specific process inside the VM. Any process that can reach the socket can mint a token: the agent, code it runs, and hooks. Scope permissions to what you would grant that run as a whole.

You don't choose which agent the token is for. Cursor fills claims from this run, so a process in the VM can't mint a token for a different agent.

## Rate limits and errors

Each agent VM can mint **30 tokens per minute**, in bursts of up to 10. The socket also accepts at most 8 connections at once. That cap is shared with [agent metadata](https://cursor.com/docs/cloud-agent/metadata.md). Cache a token until it expires instead of minting per call.

Retry `429`, `503`, `500`, `502`, and `504` with backoff. Treat `403` as fatal: this agent isn't allowed to mint.

Error bodies carry a machine-readable code. Invalid-request errors (400, 404, 405, 413, and 415) also include a `usage` string that restates the full request contract. Rate-limit and saturation errors stay code-only:

```json
{ "error": "invalid_aud", "usage": "POST /v1/tokens/oidc ..." }
```

```json
{ "error": "rate_limited" }
```

| HTTP      | `error`                                                                | When                                                                                                                                                                     |
| :-------- | :--------------------------------------------------------------------- | :----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 400       | `invalid_json`, `invalid_aud`, `invalid_nonce`, or `invalid_sub_claim` | Bad request body                                                                                                                                                         |
| 404       | `not_found`                                                            | Wrong path                                                                                                                                                               |
| 405       | `method_not_allowed`                                                   | Not `POST`                                                                                                                                                               |
| 413       | `body_too_large`                                                       | Body over 4 KB                                                                                                                                                           |
| 415       | `invalid_content_type`                                                 | Missing or non-JSON `Content-Type`                                                                                                                                       |
| 429       | `rate_limited`                                                         | Over the per-agent mint budget; honor `Retry-After`                                                                                                                      |
| 503       | `saturated`                                                            | Too many connections; honor `Retry-After`                                                                                                                                |
| 500       | `host_error`                                                           | Internal error; retry                                                                                                                                                    |
| 502 / 504 | `backend_unreachable`                                                  | Cursor couldn't mint the token; retry                                                                                                                                    |
| Other     | `backend_error`                                                        | Cursor rejected the mint. `400` means fix the request (for example an unsupported `sub_claim`, or one with no value for this agent). `403` is fatal. `503` is retryable. |

## AWS IAM example

Use OIDC when you want AWS to trust Cursor-signed JWTs with `AssumeRoleWithWebIdentity`. For the simpler Cursor-managed assume-role flow (External ID + `CURSOR_AWS_ASSUME_IAM_ROLE_ARN`), see [Using AWS IAM Roles](https://cursor.com/docs/cloud-agent/setup.md#using-aws-iam-roles).

1. Create an IAM OIDC identity provider whose URL is `https://api.cursor.com`.
2. Set the audience to `sts.amazonaws.com` (or another audience your role expects).
3. Trust the role only for subjects and teams you intend to allow.

Example trust policy:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Federated": "arn:aws:iam::123456789012:oidc-provider/api.cursor.com"
      },
      "Action": "sts:AssumeRoleWithWebIdentity",
      "Condition": {
        "StringEquals": {
          "api.cursor.com:aud": "sts.amazonaws.com"
        },
        "StringLike": {
          "api.cursor.com:sub": "user:*"
        }
      }
    }
  ]
}
```

Tighten this with an exact `sub`, such as `user:42` for one user or `service_account:<id>` for an agent that runs as a service account. AWS trust policies only match `aud` and `sub`, so scope trust to a team by minting with `"sub_claim":"team_id"` and matching the projected subject:

```json
"StringEquals": {
  "api.cursor.com:aud": "sts.amazonaws.com",
  "api.cursor.com:sub": "team_id:123"
}
```

Follow current [AWS IAM OIDC](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_roles_providers_create_oidc.html) instructions for provider creation and thumbprints.

The agent mints with `"aud":"sts.amazonaws.com"` (plus `"sub_claim":"team_id"` when the trust policy matches the team subject) and passes the JWT to STS. If you use [network allowlists](https://cursor.com/docs/cloud-agent/security-network.md#network-access), allow `sts.amazonaws.com` (and any regional STS host you call).

## Other verifiers

The same tokens work with any OIDC-compliant verifier:

- **GCP** Workload Identity Federation
- **Azure** federated credentials / Entra ID
- **Vault** JWT/OIDC auth
- Internal APIs that validate RS256 JWTs

Point the provider at the discovery URL, require your audience, and authorize on claims such as `sub`, `team_id`, or `cloud_agent_id`. To confine an agent to specific repositories, pin the complete set with `repo_urls`; `repo_url` names only the primary repository.

Minting uses the local socket only. Exchanging the JWT with AWS, GCP, Azure, or your service still needs outbound network access to those hosts.

## Related pages

- [Agent metadata](https://cursor.com/docs/cloud-agent/metadata.md) for key-value run metadata on the same socket
- [Secrets & Network](https://cursor.com/docs/cloud-agent/security-network.md) for dashboard secrets and egress controls
- [Cloud agent setup](https://cursor.com/docs/cloud-agent/setup.md#using-aws-iam-roles) for Cursor-managed AWS role assumption
- [Security overview](https://cursor.com/docs/cloud-agent/security.md) for isolation and access model
- [Service accounts](https://cursor.com/docs/account/enterprise/service-accounts.md) when agents run as a team service account


---

## Sitemap

[Overview of all docs pages](/llms.txt)
