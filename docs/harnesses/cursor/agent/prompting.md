# Prompting agents

Direct Agent with text prompts in the chat input. You can attach context, images, and voice, and switch models at any point.

## @ mentions

Type `@` in the chat input to attach specific context to your prompt. Start typing after `@` and Cursor shows matching suggestions.

- **Files & Folders**: `@auth.ts` or `@src/components/` to include files or folders (type `/` after selecting a folder to navigate deeper)
- **Terminals**: `@Terminals` to include terminal output as context
- **Chats**: `@Chats` to reference context from a previous conversation
- **Git diffs**: `@Commit (Diff of Working State)` for uncommitted changes, or `@Branch (Diff with Main)` for your full branch diff
- **Browser**: `@Browser` to attach context from the built-in browser

Use @ mentions when you know which files are relevant. If you're not sure which files matter, skip it — Agent finds relevant files through its own search.

## Custom Modes

Type `/` in the chat input to invoke a [skill](https://cursor.com/docs/skills.md). Pressing Enter attaches the skill to one message, and it fades as the conversation moves on. Use any skill as a Custom Mode to keep the agent focused on it while it works.

Pick the skill from the `/` menu and press Option+Enter (Mac) or Alt+Enter (Windows) instead. You can also select **Use as Mode** from the skill entry. Inside a mode, the skill stays in context on every turn, even as the agent works for hours, until you exit the mode.

Custom Modes work well for skills that describe how to work rather than a one-shot task. Keep a code-review checklist active while you move through several files, or hold a team playbook like `/tdd` on for an entire feature.

Custom Modes are available in the [Agents Window](https://cursor.com/docs/agent/agents-window.md) and the [CLI](https://cursor.com/docs/cli/overview.md). Any skill with a valid frontmatter block can back a mode, and the optional `icon` and `color` frontmatter fields style the mode's badge. See [Using a skill as a Custom Mode](https://cursor.com/docs/skills.md#using-a-skill-as-a-custom-mode).

## Image input

Attach images to your prompt to provide visual context for UI work, debugging, and design implementation.

- **Drag and drop** an image file into the chat input
- **Paste from clipboard** with Cmd+V, including screenshots

This is useful for implementing design mockups, debugging visual issues, and referencing error messages or stack traces without manual transcription.

## Voice input

Click the microphone icon in the chat input to dictate your prompt instead of typing. Speak naturally, include technical details like file and function names, and review the transcription before sending.

## Context usage

Every chat shares a fixed context window with the model. As you add files, run tools, and exchange messages, those tokens fill up. When the window gets close to full, Cursor compresses older parts of the conversation into a summary to leave more room for new conversation.

The context ring next to your prompt input shows how full the window is at a glance. Click the ring to open the breakdown tray, which shows the total tokens used split by category:

- **System prompt**: Cursor's built-in instructions for the model
- **Tools**: definitions of every tool available to the agent
- **Rules**: project and user rules included in the prompt
- **Skills**: skill descriptions injected into the system context
- **MCP**: instructions and catalog from connected MCP servers
- **Subagents**: documentation for subagent types the agent can launch
- **Summarized conversation**: compressed summaries of earlier turns
- **Conversation**: your messages, the agent's replies, and tool results

Hover a segment in the bar or a row in the list to highlight that category.

## Changing models

Use the model picker dropdown at the top of the chat input to switch models, or press Cmd / to cycle through models. The change applies to the current conversation going forward. Set a default model in **Cursor Settings > Models**.

- **Faster models** work well for quick edits and routine tasks
- **More capable models** are better for complex reasoning and multi-file refactoring

You can switch models mid-conversation, for example when a faster model handled exploration but you need deeper reasoning for implementation. See [Models & Pricing](https://cursor.com/docs/models-and-pricing.md) for the full list.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
