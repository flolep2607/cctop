# Debug Mode

Debug Mode helps you find root causes and fix tricky bugs that are hard to reproduce or understand. Instead of immediately writing code, the agent generates hypotheses, adds log statements, and uses runtime information to pinpoint the exact issue before making a targeted fix.

## When to use Debug Mode

Debug Mode works best for:

- **Bugs you can reproduce but can't figure out**: When you know something is wrong but the cause isn't obvious from reading the code
- **Race conditions and timing issues**: Problems that depend on execution order or async behavior
- **Performance problems and memory leaks**: Issues that require runtime profiling to understand
- **Regressions where something used to work**: When you need to trace what changed

When standard Agent interactions struggle with a bug, Debug Mode provides a different approach using runtime evidence rather than guessing at fixes.

## How it works

1. **Explore and hypothesize**: The agent explores relevant files, builds context, and generates multiple hypotheses about potential root causes.

2. **Add instrumentation**: The agent adds log statements that send data to a local debug server running in a Cursor extension.

3. **Reproduce the bug**: Debug Mode asks you to reproduce the bug and provides specific steps. This keeps you in the loop and ensures the agent captures real runtime behavior.

4. **Analyze logs**: After reproduction, the agent reviews the collected logs to identify the actual root cause based on runtime evidence.

5. **Make targeted fix**: The agent makes a focused fix that directly addresses the root cause, often just a few lines of code.

6. **Verify and clean up**: You can re-run the reproduction steps to verify the fix. Once confirmed, the agent removes all instrumentation.

## Tips for Debug Mode

- **Provide detailed context**: The more you describe the bug and how to reproduce it, the better the agent's instrumentation will be. Include error messages, stack traces, and specific steps.
- **Follow reproduction steps exactly**: Execute the steps the agent provides to ensure logs capture the actual issue.
- **Reproduce multiple times if needed**: Reproducing the bug multiple times may help the agent identify tricky problems like race conditions.
- **Be specific about expected vs. actual behavior**: Help the agent understand what should happen versus what is happening.

## Switching modes

- Use the mode picker dropdown in Agent
- Press Shift+Tab for quick switching

## Related

- [Debug mode help](https://cursor.com/help/ai-features/debug-mode.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
