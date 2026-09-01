GPT-5.6 Sol is OpenAI's flagship GPT-5.6 model in Cursor. It is the strongest of the Sol, Terra, and Luna family: persistent on long-running agent work, fast enough for interactive sessions, and notably concise in how it communicates.

## Strengths

- Highest intelligence in the GPT-5.6 family for challenging coding and reasoning tasks.
- Strong persistence on long-running work; it keeps going through multi-hour agent sessions instead of stopping early.
- Clean, direct communication with less comment and final-message slop than many Claude models.
- Concise and easy to skim; strong as a rubber-duck partner for planning and debugging.
- Competitive speed relative to similarly intelligent models.

## Limitations

- Higher per-token pricing than Terra and Luna.
- Can over-use subagents on mid-sized tasks.
- Sometimes waits for an explicit "do it" after agreeing with feedback, instead of executing immediately.
- Instruction-following can lag the strongest Claude models on some agent behavior evals.

## Tools

GPT-5.6 Sol has access to all agent tools when used with Cursor including:

Learn more about [how tools work](https://cursor.com/docs/agent/overview.md#tools) and [tool calling fundamentals](https://cursor.com/learn/tool-calling.md).

## Pricing

Cursor [plans](https://cursor.com/docs/models-and-pricing.md) include two usage pools. GPT-5.6 Sol draws from the third-party **Other Models** pool, which charges at the rates below. All prices are per million tokens.

A **Fast mode** tier (`gpt-5.6-sol-fast`) is available for priority processing at 2x the standard rates.

When input exceeds 272k tokens (long context), input pricing doubles and output pricing is 1.5x the standard rate.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
