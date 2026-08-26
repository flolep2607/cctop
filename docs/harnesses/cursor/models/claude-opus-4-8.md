We recommend using [Claude Opus 5](https://cursor.com/docs/models/claude-opus-5.md). It replaces Opus 4.8 with stronger agentic coding and long-horizon reasoning at the same price.

Claude Opus 4.8 is Anthropic's previous Opus model and a meaningful jump over Opus 4.7 on [CursorBench](https://cursor.com/blog/cursorbench). It excels at autonomous, multi-step work: it holds intent across long sessions, self-corrects when it hits friction, and writes production-ready code without hand-holding. We recommend the high thinking variant for the best results.

## Strengths

- Autonomous and self-directed. Opus 4.8 drives multi-step tasks to completion without losing track of the goal, even across large codebases and long conversations.
- Creative reasoning. It approaches problems from unexpected angles, explores alternative solutions, and produces more inventive code than its predecessor.
- Strong at planning. It maps out work before executing, catches edge cases early, and builds coherent architectures across many files.
- Reliable tool use. It calls tools purposefully, chains tool results into follow-up actions, and adapts when tool output surprises it.

## Limitations

- Most expensive model. Consumes usage limits faster than alternatives.
- Can over-elaborate in long sessions where brevity matters more than depth.

## Tools

Opus 4.8 has access to all agent tools when used with Cursor including:

Learn more about [how tools work](https://cursor.com/docs/agent/overview.md#tools) and [tool calling fundamentals](https://cursor.com/learn/tool-calling.md).

## Pricing

Cursor [plans](https://cursor.com/docs/models-and-pricing.md) include two usage pools. Opus 4.8 draws from the third-party **Other Models** pool, which charges at the rates below. All prices are per million tokens.

A **Fast mode** tier (`claude-opus-4-8-fast`) is available for roughly 2.5x faster output. On legacy request-based plans, it requires Max Mode. It bills at $10/M input and $50/M output tokens, 3x lower than Opus 4.7 fast mode. Use it selectively for time-sensitive or critical work.

All Opus 4.8 prompts bill at the base per-token rates in the table above, including when context goes above 300k. There is no separate long-context multiplier for Opus 4.8. Context windows up to 1M tokens use the same rates.

Opus 4.8 supports a thinking variant for deeper reasoning. We recommend using the high thinking variant for the strongest results.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
