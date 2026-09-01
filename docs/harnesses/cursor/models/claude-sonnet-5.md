Claude Sonnet 5 is Anthropic's latest medium-tier model and replaces Sonnet 4.6. It pushes quality close to Opus while keeping Sonnet's lower per-token price. It supports thinking mode and context windows up to 1M tokens, making it a strong default for everyday coding when you want frontier reasoning without Opus pricing.

## Strengths

- Near-Opus quality. Sonnet 5 closes most of the gap to Opus 5 on real coding work while staying far cheaper per token.
- Strong reasoning. Thinking mode handles multi-step tasks, planning, and debugging with depth.
- Reliable tool use. It calls tools purposefully and chains results into follow-up actions.
- Same provider and style as Opus at a lower price point.

## Limitations

- For peak quality on the hardest tasks, [Opus 5](https://cursor.com/docs/models/claude-opus-5.md) remains the stronger choice.
- The updated tokenizer maps the same input to more tokens, so token counts run higher than older Sonnet models.

## Tools

Sonnet 5 has access to all agent tools when used with Cursor including:

Learn more about [how tools work](https://cursor.com/docs/agent/overview.md#tools) and [tool calling fundamentals](https://cursor.com/learn/tool-calling.md).

## Pricing

Cursor [plans](https://cursor.com/docs/models-and-pricing.md) include two usage pools. Sonnet 5 draws from the third-party **Other Models** pool, which charges at the rates below. All prices are per million tokens.

All Sonnet 5 prompts bill at the base per-token rates in the table above, including when context goes above 200k. There is no separate long-context multiplier for Sonnet 5.

A thinking variant is available for deeper reasoning.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
