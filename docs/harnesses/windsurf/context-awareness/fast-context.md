# Fast Context

Fast Context is a specialized subagent that retrieves relevant code from your codebase up to 20x faster using SWE-grep models for rapid code retrieval.

Fast Context is a specialized subagent in Windsurf that retrieves relevant code from your codebase up to 20x faster than traditional agentic search. It powers Cascade's ability to quickly understand large codebases while maintaining the intelligence of frontier models.

<video />

## Using Fast Context

When Cascade receives a query that requires code search, Fast Context will trigger automatically.

You'll notice Fast Context is working when:

* Cascade quickly identifies relevant files across your codebase
* Large codebase queries complete faster than before
* Cascade spends less time reading irrelevant code

## How It Works

Fast Context uses `SWE-grep` and `SWE-grep-mini`, custom models trained specifically for rapid code retrieval. These models combine the speed of traditional embedding search with the intelligence of agentic exploration.

When you make a query to Cascade that requires searching through your codebase, Fast Context automatically activates to:

1. Identify relevant files and code sections using parallel tool calls
2. Execute multiple searches simultaneously
3. Return targeted results in seconds rather than minutes

This approach prevents context pollution and aims to mitigate the traditional speed-accuracy tradeoff. By delegating retrieval to a specialized subagent, Cascade conserves its context budget and intelligence for the actual task at hand.

## SWE-grep Models

Fast Context is powered by the SWE-grep model family:

* **SWE-grep**: High-intelligence variant optimized for complex retrieval tasks
* **SWE-grep-mini**: Ultra-fast variant serving at over 2,800 tokens per second

Both models are trained using reinforcement learning to excel at parallel tool calling and efficient codebase navigation. They execute up to 8 parallel tool calls per turn over a maximum of 4 turns, allowing them to explore different parts of your codebase simultaneously.

The models use a restricted set of cross-platform compatible tools (grep, read, glob) to ensure consistent performance across different operating systems and development environments.
