# Models & Pricing

Cursor supports frontier models from OpenAI, Anthropic, Google, SpaceXAI, and more. Pro, Pro Plus, and Ultra include two usage pools so you can pick the right balance of intelligence, speed, and cost. Start, our plan for developers in India, covers the Cursor Models pool.

## Usage pools

There are two separate usage pools, each resetting with your monthly billing cycle:

- **Cursor Models**: Significantly more included usage for Cursor Grok 4.6, Grok 4.5, and Composer 2.5.
- **Other Models**: The pool for third-party models, charged at the model's API price. Pro, Pro Plus, and Ultra include this pool, with the option to pay for additional usage as needed. The Start plan does not include this pool.

Both pools are visible in your editor settings and on your [usage dashboard](https://cursor.com/dashboard/usage).

## Cursor Models

The Cursor Models pool includes Cursor Grok 4.6, Grok 4.5, and Composer 2.5.

On Teams and Enterprise plans, [Cursor Router](https://cursor.com/docs/cursor-router.md) picks the model for each Auto request based on your optimization mode.

| Model                                                       | Provider | Input | Cache write | Cache read | Output | Notes                                  |
| ----------------------------------------------------------- | -------- | ----- | ----------- | ---------- | ------ | -------------------------------------- |
| Grok 4.6                                                    | Cursor   | $2    | -           | $0.5       | $6     | Jointly trained by Cursor and SpaceXAI |
| Grok 4.6 (Fast)                                             | Cursor   | $4    | -           | $1         | $12    | Jointly trained by Cursor and SpaceXAI |
| Grok 4.5                                                    | Cursor   | $2    | -           | $0.5       | $6     | Jointly trained by Cursor and SpaceXAI |
| Grok 4.5 (Fast)                                             | Cursor   | $4    | -           | $1         | $18    | Jointly trained by Cursor and SpaceXAI |
| [Composer 2.5](https://cursor.com/blog/composer-2-5)        | Cursor   | $0.5  | -           | $0.2       | $2.5   | -                                      |
| [Composer 2.5 (Fast)](https://cursor.com/blog/composer-2-5) | Cursor   | $3    | -           | $0.5       | $15    | -                                      |

## Other Models

When you select a specific third-party model, usage is drawn from the **Other Models** pool at that model's API rate.

### Model pricing

All prices are per million tokens:

| Model                                                                                         | Provider  | Input | Cache write | Cache read | Output | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| --------------------------------------------------------------------------------------------- | --------- | ----- | ----------- | ---------- | ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [Claude 4 Sonnet](https://www.anthropic.com/claude/sonnet)                                    | Anthropic | $3    | $3.75       | $0.3       | $15    | Hidden by default; Thinking variant counts as 2 requests in legacy pricing                                                                                                                                                                                                                                                                                                                                                                    |
| [Claude 4 Sonnet 1M](https://www.anthropic.com/claude/sonnet)                                 | Anthropic | $6    | $7.5        | $0.6       | $22.5  | Hidden by default; Thinking variant counts as 2 requests in legacy pricing; This model can be very expensive due to the large context window; The cost is 2x when the input exceeds 200k tokens                                                                                                                                                                                                                                               |
| [Claude 4.5 Haiku](https://www.anthropic.com/claude/haiku)                                    | Anthropic | $1    | $1.25       | $0.1       | $5     | Hidden by default; Bedrock/Vertex: regional endpoints +10% surcharge; Cache: writes 1.25x, reads 0.1x                                                                                                                                                                                                                                                                                                                                         |
| [Claude 4.5 Opus](https://www.anthropic.com/claude/opus)                                      | Anthropic | $5    | $6.25       | $0.5       | $25    | Hidden by default; Requires Max Mode on legacy request-based plans                                                                                                                                                                                                                                                                                                                                                                            |
| [Claude 4.5 Sonnet](https://www.anthropic.com/claude/sonnet)                                  | Anthropic | $3    | $3.75       | $0.3       | $15    | Hidden by default; Requires Max Mode on legacy request-based plans; Up to 1M tokens with extended context at the same per-token rates (no long-context surcharge)                                                                                                                                                                                                                                                                             |
| [Claude 4.6 Opus](https://www.anthropic.com/claude/opus)                                      | Anthropic | $5    | $6.25       | $0.5       | $25    | Hidden by default; Requires Max Mode on legacy request-based plans; Up to 1M tokens with extended context at the same per-token rates (no long-context surcharge)                                                                                                                                                                                                                                                                             |
| [Claude 4.6 Sonnet](https://www.anthropic.com/claude/sonnet)                                  | Anthropic | $3    | $3.75       | $0.3       | $15    | Hidden by default; Requires Max Mode on legacy request-based plans; Up to 1M tokens with extended context at the same per-token rates (no long-context surcharge)                                                                                                                                                                                                                                                                             |
| [Claude 4.7 Opus](https://www.anthropic.com/claude/opus)                                      | Anthropic | $5    | $6.25       | $0.5       | $25    | Hidden by default; Requires Max Mode on legacy request-based plans; Up to 1M tokens with extended context at the same per-token rates (no long-context surcharge)                                                                                                                                                                                                                                                                             |
| [Claude Fable 5](https://www.anthropic.com/claude)                                            | Anthropic | $10   | $12.5       | $1         | $50    | Requires data retention approval for Enterprise customers, Teams and individual customers with Privacy Mode enabled; Anthropic stores agent input and output data for harm-prevention processes; this data is not used to train or improve Anthropic models or products; Requests that trip a security guardrail are automatically routed to Claude Opus; About 2x the cost of Claude Opus 5; Requires Max Mode on legacy request-based plans |
| [Claude Opus 4.7 (fast mode)](https://www.anthropic.com/claude/opus)                          | Anthropic | $30   | $37.5       | $3         | $150   | Hidden by default; Requires Max Mode on legacy request-based plans; Limited research preview; Up to 1M tokens with extended context at the same per-token rates as shorter context                                                                                                                                                                                                                                                            |
| [Claude Opus 4.8](https://www.anthropic.com/claude/opus)                                      | Anthropic | $5    | $6.25       | $0.5       | $25    | Hidden by default; Requires Max Mode on legacy request-based plans; Fast mode (\`claude-opus-4-8-fast\`) requires Max Mode on legacy request-based plans; Fast mode is 3x lower per-token pricing than Opus 4.7 fast mode; Up to 1M tokens with extended context at the same per-token rates (no long-context surcharge)                                                                                                                      |
| [Claude Opus 5](https://www.anthropic.com/claude/opus)                                        | Anthropic | $5    | $6.25       | $0.5       | $25    | Requires Max Mode on legacy request-based plans; Fast mode (\`claude-opus-5-fast\`) requires Max Mode on legacy request-based plans; Up to 1M tokens with extended context at the same per-token rates (no long-context surcharge)                                                                                                                                                                                                            |
| [Claude Sonnet 5](https://www.anthropic.com/claude/sonnet)                                    | Anthropic | $2    | $2.5        | $0.2       | $10    | Requires Max Mode on legacy request-based plans; Up to 1M tokens with extended context at the same per-token rates (no long-context surcharge); Uses an updated tokenizer, so the same input can map to more tokens                                                                                                                                                                                                                           |
| [Gemini 2.5 Flash](https://developers.googleblog.com/en/start-building-with-gemini-25-flash/) | Google    | $0.3  | -           | $0.03      | $2.5   | Hidden by default                                                                                                                                                                                                                                                                                                                                                                                                                             |
| [Gemini 3 Flash](https://ai.google.dev/gemini-api/docs)                                       | Google    | $0.5  | -           | $0.05      | $3     | Hidden by default                                                                                                                                                                                                                                                                                                                                                                                                                             |
| [Gemini 3 Pro](https://ai.google.dev/gemini-api/docs)                                         | Google    | $2    | -           | $0.2       | $12    | Hidden by default                                                                                                                                                                                                                                                                                                                                                                                                                             |
| [Gemini 3 Pro Image Preview](https://ai.google.dev/gemini-api/docs)                           | Google    | $2    | -           | $0.2       | $12    | Hidden by default; Native image generation model optimized for speed, flexibility, and contextual understanding; Text input and output priced the same as Gemini 3 Pro; Image output: $120/1M tokens (\~$0.134 per 1K/2K image, \~$0.24 per 4K image); Preview models may change before becoming stable and have more restrictive rate limits                                                                                                 |
| [Gemini 3.1 Pro](https://ai.google.dev/gemini-api/docs)                                       | Google    | $2    | -           | $0.2       | $12    | -                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| [Gemini 3.5 Flash](https://ai.google.dev/gemini-api/docs)                                     | Google    | $1.5  | -           | $0.15      | $9     | Hidden by default                                                                                                                                                                                                                                                                                                                                                                                                                             |
| [Gemini 3.6 Flash](https://ai.google.dev/gemini-api/docs)                                     | Google    | $1.5  | -           | $0.15      | $7.5   | Hidden by default                                                                                                                                                                                                                                                                                                                                                                                                                             |
| [Gemini 3.7 Flash](https://ai.google.dev/gemini-api/docs)                                     | Google    | $0.75 | -           | $0.075     | $3.5   | -                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| [GLM 5.2](https://z.ai)                                                                       | Z.ai      | $1.4  | -           | $0.26      | $4.4   | Hidden by default                                                                                                                                                                                                                                                                                                                                                                                                                             |
| [GPT-5](https://openai.com/index/gpt-5/)                                                      | OpenAI    | $1.25 | -           | $0.125     | $10    | Hidden by default; Agentic and reasoning capabilities; Available reasoning effort variant is gpt-5-high                                                                                                                                                                                                                                                                                                                                       |
| [GPT-5 Fast](https://openai.com/index/gpt-5/)                                                 | OpenAI    | $2.5  | -           | $0.25      | $20    | Hidden by default; Faster speed but 2x price; Available reasoning effort variants are gpt-5-high-fast, gpt-5-low-fast                                                                                                                                                                                                                                                                                                                         |
| [GPT-5 Mini](https://openai.com/index/gpt-5/)                                                 | OpenAI    | $0.25 | -           | $0.025     | $2     | Hidden by default                                                                                                                                                                                                                                                                                                                                                                                                                             |
| [GPT-5-Codex](https://platform.openai.com/docs/models/gpt-5-codex)                            | OpenAI    | $1.25 | -           | $0.125     | $10    | Hidden by default; Agentic and reasoning capabilities                                                                                                                                                                                                                                                                                                                                                                                         |
| [GPT-5.1 Codex](https://platform.openai.com/docs/models/gpt-5-codex)                          | OpenAI    | $1.25 | -           | $0.125     | $10    | Hidden by default; Agentic and reasoning capabilities                                                                                                                                                                                                                                                                                                                                                                                         |
| [GPT-5.1 Codex Max](https://platform.openai.com/docs/models/gpt-5-codex)                      | OpenAI    | $1.25 | -           | $0.125     | $10    | Hidden by default                                                                                                                                                                                                                                                                                                                                                                                                                             |
| [GPT-5.1 Codex Mini](https://platform.openai.com/docs/models/gpt-5-codex)                     | OpenAI    | $0.25 | -           | $0.025     | $2     | Hidden by default; Agentic and reasoning capabilities; 4x rate limits compared to GPT-5.1 Codex                                                                                                                                                                                                                                                                                                                                               |
| [GPT-5.2](https://openai.com/index/gpt-5/)                                                    | OpenAI    | $1.75 | -           | $0.175     | $14    | Hidden by default; Agentic and reasoning capabilities; Available reasoning effort variant is gpt-5.2-high                                                                                                                                                                                                                                                                                                                                     |
| [GPT-5.2 Codex](https://platform.openai.com/docs/models/gpt-5-codex)                          | OpenAI    | $1.75 | -           | $0.175     | $14    | Hidden by default; Agentic and reasoning capabilities                                                                                                                                                                                                                                                                                                                                                                                         |
| [GPT-5.3 Codex](https://platform.openai.com/docs/models/gpt-5-codex)                          | OpenAI    | $1.75 | -           | $0.175     | $14    | Hidden by default; Requires Max Mode on legacy request-based plans; Agentic and reasoning capabilities; Available reasoning effort variant is gpt-5.3-codex-high                                                                                                                                                                                                                                                                              |
| [GPT-5.4](https://developers.openai.com/api/docs/models/gpt-5.4)                              | OpenAI    | $2.5  | -           | $0.25      | $15    | Hidden by default; Requires Max Mode on legacy request-based plans; Agentic and reasoning capabilities; 90% discount on cached input tokens; Fast mode is 15% faster with 2x pricing; Long context supports up to 1M tokens with 2x input pricing                                                                                                                                                                                             |
| [GPT-5.4 Mini](https://developers.openai.com/api/docs/models/gpt-5.4-mini)                    | OpenAI    | $0.75 | -           | $0.075     | $4.5   | Hidden by default; Smaller, faster variant of GPT-5.4; 90% discount on cached input tokens                                                                                                                                                                                                                                                                                                                                                    |
| [GPT-5.4 Nano](https://developers.openai.com/api/docs/models/gpt-5.4-nano)                    | OpenAI    | $0.2  | -           | $0.02      | $1.25  | Hidden by default; Smallest GPT-5.4 variant, optimized for cost; 90% discount on cached input tokens                                                                                                                                                                                                                                                                                                                                          |
| [GPT-5.5](https://developers.openai.com/api/docs/models/gpt-5.5)                              | OpenAI    | $5    | -           | $0.5       | $30    | Hidden by default; Requires Max Mode on legacy request-based plans; Agentic and reasoning capabilities; More token-efficient than GPT-5.4 on comparable tasks; Improved persistence on long-running tasks; Fast mode is available at higher rates; Long context supports up to 1M tokens with 2x input pricing                                                                                                                                |
| [GPT-5.6 Luna](https://openai.com/index/previewing-gpt-5-6-sol/)                              | OpenAI    | $0.2  | $0.25       | $0.02      | $1.2   | Smallest GPT-5.6 variant, optimized for cost and speed; Agentic and reasoning capabilities; Fast mode is available at 2x pricing; Cache writes are billed at 1.25x the uncached input rate                                                                                                                                                                                                                                                    |
| [GPT-5.6 Sol](https://openai.com/index/previewing-gpt-5-6-sol/)                               | OpenAI    | $4    | $5          | $0.4       | $20    | Requires Max Mode on legacy request-based plans; Agentic and reasoning capabilities; Fast mode is available at 2x pricing; Long context supports up to 1M tokens with 2x input pricing; Cache writes are billed at 1.25x the uncached input rate; Promotional pricing through November 21, 2026                                                                                                                                               |
| [GPT-5.6 Terra](https://openai.com/index/previewing-gpt-5-6-sol/)                             | OpenAI    | $2    | $2.5        | $0.2       | $12    | Mid-tier GPT-5.6 variant between Sol and Luna; Agentic and reasoning capabilities; Fast mode is available at 2x pricing; Cache writes are billed at 1.25x the uncached input rate                                                                                                                                                                                                                                                             |
| Kimi K2.7 Code                                                                                | Moonshot  | $0.95 | -           | $0.19      | $4     | Hidden by default                                                                                                                                                                                                                                                                                                                                                                                                                             |
| [Kimi K3](https://www.moonshot.ai)                                                            | Moonshot  | $3    | -           | $0.3       | $15    | Hidden by default; Requires Max Mode on legacy request-based plans; Up to 1M tokens with extended context at the same per-token rates (no long-context surcharge); No separate cache-write fee                                                                                                                                                                                                                                                |

Opting in to regional data residency incurs a 10% uplift on Model pricing for eligible Models. See [Privacy and Data Governance](https://cursor.com/docs/enterprise/privacy-and-data-governance.md) for details on supported regions, Models, functions and data residency policies.

## Plans

Pro, Pro Plus, and Ultra include unlimited tab completions, extended agent usage limits on all models, access to Bugbot, and access to Cloud Agents. Start is a lower-priced plan for developers in India that covers the Cursor Models pool and Cloud Agents.

| Plan                   | Price                  | Cursor Models | Other Models |
| :--------------------- | :--------------------- | :------------ | :----------- |
| **Start** (India only) | ₹649/mo, tax inclusive | Included      | Not included |
| **Pro**                | $20/mo                 | Included      | Included     |
| **Pro Plus**           | $60/mo                 | Included      | Included     |
| **Ultra**              | $200/mo                | Included      | Included     |

Since different models have different API costs, your model selection affects how quickly your included usage is consumed.

### Start (India only)

Start is available to developers in India. It costs ₹649 per month, tax inclusive, billed monthly in INR with UPI, credit card, or debit card. Every other individual plan displays its price before tax.

Start includes generous usage of the Cursor Models pool, so you can run Grok 4.6, Grok 4.5, and Composer 2.5 for daily building. On Start, all three models run in non-fast mode, and both Grok 4.6 and Grok 4.5 use a fixed medium effort level. You cannot change effort levels or enable Fast mode on Start. Upgrade to Pro or higher to choose effort levels and Fast mode.

Start also includes [Cloud Agents](https://cursor.com/docs/cloud-agent.md), [Cursor for iOS](https://cursor.com/docs/cloud-agent/mobile.md), and plugins, MCP servers, hooks, and skills.

Start does not include the Other Models pool, on-demand usage, Bugbot, Auto, Automations, or the Cursor SDK. Upgrade to Pro for those. Read the [Cursor Start announcement](https://cursor.com/blog/cursor-start-india) for more detail.

### How much usage do I need?

- **Daily Tab users**: Typically stay within included usage
- **Limited Agent users**: Often stay within included usage
- **Daily Agent users**: Typically $60–$100/mo total usage
- **Power users (multiple agents/automation)**: Often $200+/mo total usage

### What happens when I reach my limit?

When you exceed your included monthly usage, you can either:

- **Add on-demand usage**: Continue at the same API rates with pay-as-you-go billing
- **Upgrade your plan**: Move to a higher tier for more included usage

On-demand usage is billed monthly at the same rates. Requests are never downgraded in quality or speed.

### Teams

There are two business plans: Teams and Enterprise (Custom). Teams offers two seat types: Standard ($40/user/mo) and Premium ($120/user/mo), where Premium adds 5x the Standard limits on Agent.

Team plans provide additional features like centralized team billing and administration, a team marketplace for internal rules, skills, and plugins, agentic code reviews with Bugbot, cloud agents and automations with shared team context, usage analytics, team-wide privacy mode enforcement, and SAML/OIDC SSO.

We recommend Teams for any customer that is happy self-serving. We recommend [Enterprise](https://cursor.com/contact-sales?source=docs-models-pricing) for customers that need priority support, pooled usage, invoicing, SCIM, or advanced security controls.

Learn more about [Teams pricing](https://cursor.com/docs/account/teams/pricing.md).

## Cursor Token Rate

On Teams and Enterprise plans, third-party model requests include a Cursor Token Rate of $0.25 per million tokens. This rate applies on top of model API pricing for included usage, on-demand usage, and BYOK usage.

The Cursor Token Rate applies when you select a third-party model directly, and when Auto routes to a third-party model. First-party Cursor models, including Grok and Composer, are exempt from the Cursor Token Rate.

## Auto modes

Auto has three modes: Cost, Balance, and Intelligence.

All Auto modes bill at the list price of the model each request is routed to. See [Model pricing](https://cursor.com/docs/models-and-pricing.md#model-pricing) for per-model rates. Third-party models also incur the [Cursor Token Rate](https://cursor.com/docs/models-and-pricing.md#cursor-token-rate).

### Legacy Enterprise Auto

Until September 7, 2026, Enterprise Auto pricing is set per million tokens, regardless of which model is used.

## Legacy request-based pricing

### Max Mode

Max Mode is available only on legacy request-based plans. It extends a model's context window beyond the default limit and is billed at the model's API rate plus 20%. See [Max Mode on legacy plans](https://cursor.com/help/ai-features/max-mode.md) for details.

## FAQ

### Where are models hosted?

Models are hosted by the model provider, a trusted partner, or Cursor. See our list of [sub-processors](https://trust.cursor.com/subprocessors) for details.

### Where can I find pricing terms?

For enterprise pricing details, billing terms, and fee calculations, see the [Pricing Policy](https://cursor.com/terms/pricing).


---

## Sitemap

[Overview of all docs pages](/llms.txt)
