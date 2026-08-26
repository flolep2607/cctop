# Cursor Agent

Agent is Cursor's assistant that can complete complex coding tasks independently, run terminal commands, and edit code. Access in sidepane with Cmd+I.

Learn more about [how agents work](https://cursor.com/learn/agents.md) and help you build faster.

## How Agent works

An agent is built on three components:

1. **Instructions**: The system prompt and [rules](https://cursor.com/docs/rules.md) that guide agent behavior
2. **Tools**: File editing, codebase search, terminal execution, and more
3. **Model**: The agent model you pick for the task

Cursor's agent orchestrates these components for each model we support, tuning instructions and tools specifically for every frontier model. As new models are released, you can focus on building software while Cursor handles the model-specific optimizations.

## Tools

Tools are the building blocks of Agent. They are used to search your codebase and the web to find relevant information, make edits to your files, run terminal commands, and more.

To understand how tool calling works under the hood, see our [tool calling fundamentals](https://cursor.com/learn/tool-calling.md).

There is no limit on the number of tool calls Agent can make during a task.

### Search files and folders

Search for files by name, read directory structures, and find exact keywords or patterns within files.

### Web

Generate search queries and perform web searches.

### Fetch Rules

Retrieve specific [rules](https://cursor.com/docs/rules.md) based on type and description.

### Read files

Intelligently read the content of a file. Also supports image files (.png, .jpg, .gif, .webp, .svg) and includes them in the conversation context for analysis by vision-capable models.

### Edit files

Suggest edits to files and apply them automatically.

### Run shell commands

Execute terminal commands and monitor output. By default, Cursor uses the first terminal profile available.

To set your preferred terminal profile:

1. Open Command Palette (`Cmd/Ctrl+Shift+P`)
2. Search for "Terminal: Select Default Profile"
3. Choose your desired profile

### Browser

Control a browser to take screenshots, test applications, and verify visual changes. Agent can navigate pages, interact with elements, and capture the current state for analysis. See the [Browser documentation](https://cursor.com/docs/agent/tools/browser.md) for details.

### Image generation

Generate images from text descriptions or reference images. Useful for creating UI mockups, product assets, and visualizing architecture diagrams. Images are saved to your project's `assets/` folder by default and shown inline in chat.

### Ask questions

Ask clarifying questions during a task. While waiting for your response, the agent continues reading files, making edits, or running commands. Your answer is incorporated as soon as it arrives.

## Checkpoints

Checkpoints save snapshots of your codebase during an Agent session. Agent automatically creates them before making significant changes, capturing the state of all modified files.

If Agent takes a wrong turn, click any checkpoint in the chat timeline to preview your files at that point, then restore to revert all files to that state. You can also restore from the `Restore Checkpoint` button on previous requests or the + button when hovering over a message.

Checkpoints are useful for exploratory work, complex refactoring, and iterative development where you want safe rollback points.

Checkpoints are stored locally and separate from Git. Only use them for undoing Agent changes; use Git for permanent version control.

## Queued messages

You have two ways to talk to an agent while it works. Queue a message and it waits for the current task to finish. [Send a follow-up now](https://cursor.com/docs/agent/overview.md#steer-a-running-agent) and it steers the active turn at the agent's next tool call.

[Media](/docs-static/images/agent/planning/agent-queue.mp4)

### Using the queue

1. While Agent is working, type your next instruction
2. Press Enter to add it to the queue
3. Messages appear in order below the active task
4. Drag to reorder queued messages as needed
5. Agent processes them sequentially after finishing

### Keyboard shortcuts

While Agent is working:

- Press Enter to queue your message (it waits until Agent finishes the current task)
- Press Cmd+Enter to send immediately, bypassing the queue

### Immediate messaging

When you use Cmd+Enter to send immediately, your message is appended to the most recent user message in the chat and processed right away without waiting in the queue.

- Your message attaches to tool results and sends immediately
- This creates a more responsive experience for urgent follow-ups
- Use this when you need to interrupt or redirect Agent's current work

### Steer a running agent

You can send a follow-up to steer the agent while it's working, without interrupting it. Type a follow-up and hit **Send now**, or press Enter twice. The message is delivered at the agent's next tool call instead of cutting off work mid-action, which preserves in-flight work and keeps the agent on task.

This is available on [cursor.com/agents](https://cursor.com/agents) now and rolling out in the [Agents Window](https://cursor.com/docs/agent/agents-window.md). Press Tab to queue the message for after the turn instead.

In the [CLI](https://cursor.com/docs/cli/overview.md), pressing Enter while the agent works steers the active run at a safe boundary, and pressing Enter again interrupts the turn.

## Goals with /goal

Agent reads each message as a new job. Use `/goal` to give the agent a long-lived objective to work towards until it's fully complete:

```text
/goal fix all flaky tests and make CI green
```

In the [CLI](https://cursor.com/docs/cli/overview.md), Ctrl+C pauses the goal. Pair a goal with a [Custom Mode](https://cursor.com/docs/agent/prompting.md#custom-modes) when you want the agent to follow a playbook, or with the built-in [`/loop`](https://cursor.com/docs/skills.md#built-in-cursor-skills) skill for recurring check-ins while it pursues the objective.

`/goal` is rolling out. If you don't see it, try it in a new chat.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
