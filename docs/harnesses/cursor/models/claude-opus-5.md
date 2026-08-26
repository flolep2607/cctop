Claude Opus 5 is Anthropic's latest Opus model and replaces Opus 4.8. It is a step-change over Opus 4.8 on agentic coding, professional knowledge work, and long-horizon reasoning, and it lands on par with Fable 5 on [CursorBench](https://cursor.com/cursorbench) at Opus pricing. We recommend the high thinking variant for the best results.

## Strengths

- Stronger than Opus 4.8 on thoroughness, carefulness, and intent understanding. It holds the goal across long sessions and finishes multi-step work with less hand-holding.
- Competitive with Fable 5 on hard coding work, often finishing tasks faster end-to-end while charging standard Opus rates.
- Strong at planning and tool use. It maps work before executing, chains tool results into follow-up actions, and adapts when tool output surprises it.
- Zero Data Retention compatible. Unlike Fable 5, Opus 5 does not require Anthropic data-retention opt-in.

## Limitations

- Same per-token price as Opus 4.8, so it still consumes the Other Models pool faster than Sonnet or Composer.
- Can over-elaborate in long sessions where brevity matters more than depth.

## Tools

Opus 5 has access to all agent tools when used with Cursor including:

Learn more about [how tools work](https://cursor.com/docs/agent/overview.md#tools) and [tool calling fundamentals](https://cursor.com/learn/tool-calling.md).

## Pricing

Cursor [plans](https://cursor.com/docs/models-and-pricing.md) include two usage pools. Opus 5 draws from the third-party **Other Models** pool, which charges at the rates below. All prices are per million tokens.

A **Fast mode** tier (`claude-opus-5-fast`) is available at launch for higher-priority output. On legacy request-based plans, it requires Max Mode. It bills at $10/M input and $50/M output tokens. Use it selectively for time-sensitive or critical work.

All Opus 5 prompts bill at the base per-token rates in the table above, including when context goes above 300k. There is no separate long-context multiplier for Opus 5. Context windows up to 1M tokens use the same rates.

Opus 5 supports a thinking variant for deeper reasoning. We recommend using the high thinking variant for the strongest results.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
