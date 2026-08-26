# Privacy and Data Governance

Understanding how data flows through Cursor is critical for security reviews and compliance assessments. This documentation explains what data goes where, what guarantees you have, and where that data lives geographically.

## Two data flows

There are two ways data leaves your local environment when using Cursor:

### 1. LLM requests

When you use AI features, we send prompts and code context to language model providers like OpenAI, Anthropic, and Google. If you are using Cursor's custom models (e.g. Composer), your data may also be processed by our inference providers. See our list of [sub-processors](https://trust.cursor.com/subprocessors).

**With Privacy Mode enabled** your code is never used for training by Cursor or other AI model providers.

Privacy Mode is on by default for Enterprise teams. See [Privacy Overview](https://cursor.com/privacy-overview) for details.

### 2. Cloud Agents

Cloud Agents are the only feature that requires Cursor to store code. Unlike the indexing process or LLM requests, Cloud Agents need access to your repository over time to make changes.

**Architecture:**

- Agents run in isolated virtual machines
- Each agent has a dedicated environment
- Isolated from other agents and users

**What gets stored:**

- Encrypted copies of repositories that Cloud Agents work on
- Stored temporarily while the agent runs
- Deleted after the agent completes

Cloud Agents are optional. If your security policy prohibits code storage, don't enable Cloud Agents. You can still use all other Cursor features.

See [Cloud Agents](https://cursor.com/docs/cloud-agent.md) for details.

## Models with data retention

Most models run under Cursor's ZDR agreements, so providers don't store inputs or outputs or train on your data ([read more](https://cursor.com/data-use) about our data use policies). A few models require data retention with the provider and fall outside these agreements. For Enterprise customers, Teams with Privacy Mode enabled, and individual customers with Privacy Mode enabled, Cursor requires admin approval before use.

[Claude Fable 5](https://cursor.com/docs/models/claude-fable-5.md) works this way. Anthropic stores its inputs and outputs to run automatic and human harm-prevention reviews. This data is not used for training or product improvement. For Enterprise customers and customers with Privacy Mode enabled, requests to Fable 5 fail until the model's data retention policy is approved from the [dashboard](https://cursor.com/dashboard/restricted_models/claude-fable-5). Opting in applies to the whole team. Enterprise admins can still limit which user groups can select the model with [model access control](https://cursor.com/docs/enterprise/model-and-integration-management.md#model-access-control).

When a Fable 5 request trips one of its security guardrails, Cursor routes that request to Claude Opus automatically so your work continues.

## Privacy Mode enforcement

Privacy Mode can be enabled at the team level to ensure all team members benefit from ZDR guarantees.

**Team-level enforcement:**

1. Go to your [team dashboard](https://cursor.com/dashboard)
2. Navigate to Settings
3. Enable Privacy Mode for the team
4. Optionally enforce it so members can't disable it

**MDM enforcement:**
For additional assurance, use the Allowed Team IDs policy. This prevents users from logging into personal accounts (which might not have Privacy Mode enabled) on corporate devices.

See [Identity and Access Management](https://cursor.com/docs/enterprise/identity-and-access-management.md#allowed-team-ids) for policy details and [Deployment Patterns](https://cursor.com/docs/enterprise/deployment-patterns.md#mdm-configuration) for MDM configuration.

## Compliance and contracts

Our [DPA](https://cursor.com/terms/dpa) includes comprehensive data protection commitments that follow industry standards, including data minimization, access control, and secure processing.

All [sub-processors](https://trust.cursor.com/subprocessors) are covered by appropriate data processing agreements.

## Data encryption

Cursor encrypts data for all infrastructure, including:

- TLS 1.2+ in transit
- AES-256 at rest

For enhanced security control, enterprise customers can use Customer Managed Encryption Keys (CMEK) for encrypting data stored in Cursor's infrastructure.

With CMEK enabled:

- Embeddings are encrypted using your customer encryption key
- Cloud Agent data is encrypted using your customer encryption key
- You control key rotation and access
- Provides additional layer of security beyond standard encryption

[Contact sales](https://cursor.com/contact-sales?source=docs-cmek) to enable CMEK for your organization.

## Data residency

Data residency controls let customers enrolled in the program manage where their code and data are processed and stored. When data residency is enabled for a team, model inference, data processing, and data storage for in-scope features stay in the selected region.

### What data residency covers

Data residency applies across three independent layers for supported features and models:

| Layer               | Scope                                                                   |
| :------------------ | :---------------------------------------------------------------------- |
| **Inference**       | Model inference runs entirely in the selected region.                   |
| **Data processing** | Data pipelines that touch your content run only in the selected region. |
| **Data storage**    | Customer Data is stored only in the selected region, including backups. |

- Today, customers can enroll in **US-only data residency** (inference, processing, and storage in the US).
- EU + Iceland inference-only coverage is available on request. Broader EU support and additional regions are in active development.
- Contact your account executive if you are interested in either option.

### US data residency

#### Model availability

Under US-only data residency, only the following model families run in-region:

- GPT (`gpt-*`)
- Claude 4.6 and above
- Gemini 2.5 Flash
- Composer
- Grok 4.5

Selecting a model that isn't eligible returns an error, and the model is unavailable while data residency is enabled. Auto will select from only models on this list.

#### What stays in-region

When US-only data residency is enabled for a team, the following stay on US-based infrastructure:

- Inference on inputs and suggestions for supported models
- Data processing pipelines that handle your content
- Storage of your Customer Data
- Use of Cloud Agents, including inference, processing, and storage
- Tab, editing, autocomplete, and semantic search

**Traveling users:** If a user on a US-only team is abroad, their requests still route to US-only infrastructure. This may add latency.

#### Pricing

US-only data residency incurs a 10% uplift on Model pricing for eligible Models.

#### How to enable

US-only data residency is available to Enterprise customers and is enabled per team. Enablement is handled by your account team while self-serve controls are built out; plan for up to two weeks from the time the request comes in. Contact your account team to enroll a team.

#### FAQ

### Does US-only data residency apply to my whole organization?

It's enabled per team, so you can scope it to the teams that need it.

### Does it cover Cloud Agents?

Yes. Inference, processing, and storage for Cloud Agents can be US-only today.

### What happens if a user travels outside the US?

Their requests still route to US-only infrastructure. Expect some added latency, but the US-only guarantee is preserved.

### Can I use any model I want?

No. Only the eligible model families listed under Model availability are supported. Choosing an unsupported model returns an error while US-only data residency is active.

### What might leave the region?

Some functionality depends on external services or on infrastructure Cursor doesn't control, so US data residency can't be provided for it:

- **SSO / authentication** — Routes through Cursor's identity provider (WorkOS) regardless of region.
- **Codebase indexing** — If your codebase is stored outside of the US, we cannot guarantee US-only indexing.
- **Bring your own key (BYOK)** — US data residency is not supported for BYOK.
- **Custom models** — A custom model reached via an OpenAI-compatible base URL override or a third-party gateway carries the region of that gateway or model, which may not be in the US.
- **MCPs and external integrations** — `@Web` and user-configured MCPs or connectors are separate services, each with its own region.
- **Bugbot / code review** — Runs against your repository's infrastructure, so its region depends on where your repositories are located.
- **Shared links** — If a link is shared outside a US-only team, US-only residency can't be guaranteed for the recipient.
- **Slack- or web-triggered Cloud Agents** — The region of the issuing command can't be guaranteed.

### Is data residency available outside the US?

US-only data residency is available today. Coverage for the EU/EEA and APAC is in active development. Please reach out to your account manager for more information.

### Enterprise privacy and data controls

Contact our team to learn about data residency, CMEK, Privacy Mode enforcement, and more.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
