# JetBrains

Use Cursor's AI agent in IntelliJ IDEA, PyCharm, WebStorm, and other JetBrains IDEs through the [Agent Client Protocol](https://agentclientprotocol.com/) (ACP).

ACP lets you stay in your JetBrains IDE while Cursor handles agent-driven development. You get access to frontier models from OpenAI, Anthropic, Google, and Cursor.

## Prerequisites

- A paid [Cursor plan](https://cursor.com/docs/models-and-pricing.md)
- A JetBrains IDE with the [AI Assistant](https://plugins.jetbrains.com/plugin/22282-ai-assistant) plugin enabled (2025.1+)

## Get started

### Open the AI Chat plugin

Open the AI Chat panel in your JetBrains IDE. You can find it in the right sidebar or through **View** > **Tool Windows** > **AI Chat**.

### Install Cursor from the ACP registry

In the AI Chat panel, open the agent provider list and select **Add Agent from Registry**. Search for **Cursor** and install it.

### Authenticate

After installing, select Cursor as your agent provider.

### Start coding

Send a prompt in the AI Chat panel. Cursor's agent reads your project, edits files, runs terminal commands, and creates code directly in your JetBrains IDE.

## What you get

Cursor ACP in JetBrains IDEs provides many of the same agent capabilities available across other Cursor surfaces.

- **Model selection** — Choose from [frontier models](https://cursor.com/docs/models-and-pricing.md) suited to your task. Different models handle different kinds of work better; switch between them as needed.
- **File editing** — The agent reads and writes files in your project, with changes reflected in your JetBrains editor.
- **Terminal commands** — The agent runs shell commands in the IDE's integrated terminal.

## How it works

Cursor ACP uses the [Agent Client Protocol](https://agentclientprotocol.com/), an open standard for connecting AI agents to IDEs. Your JetBrains IDE acts as the ACP client, and Cursor's agent acts as the server.

When you send a prompt, the AI Chat plugin forwards it to Cursor's agent through ACP. The agent processes your request, reads your project files, and streams edits and terminal commands back to the IDE.

## Pricing

Cursor ACP uses the same usage-based pricing as your Cursor subscription. See [pricing](https://cursor.com/docs/models-and-pricing.md) for details.

## Related

### ACP reference

Full ACP protocol details, transport, and client examples

### Models

Available models and their capabilities


---

## Sitemap

[Overview of all docs pages](/llms.txt)
