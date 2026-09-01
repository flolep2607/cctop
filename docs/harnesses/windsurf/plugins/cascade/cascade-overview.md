# Cascade Overview

Cascade brings agentic AI coding to JetBrains with Write/Chat modes, voice input, tool access, turbo mode, and real-time collaboration.

Windsurf's Cascade brings the best of agentic coding to the JetBrains suite.

To open Cascade, press `Cmd/Ctrl+L` or click the Cascade icon.

# Model selection

Select your desired model from the selection menu below the Cascade conversation input box. Click below too see the full breakdown of the available models and their availability across different plans and pricing.

<Card title="Models" icon="robot" href="/windsurf/models">
  Model availability in Windsurf.
</Card>

# Write/Chat Modes

Cascade comes in two modes: **Write** and **Chat**.

Write mode allows Cascade to create and make modifications to your codebase, while Chat mode is optimized for questions around your codebase or general coding principles.

<video />

# Queued Messages

While you are waiting for Cascade to finish its current task, you can queue up new messages to execute in order once the task is complete.

To add a message to the queue, simply type in your message while Cascade is working and press `Enter`.

* **Send immediately**: Press Enter again on an empty text box to send it right away.
* **Delete**: Remove any message from the queue before it's sent

# Access to Tools

Cascade has a variety of tools at its disposal, such as Search, Analyze, [Web Search](/windsurf/cascade/web-search), and the [terminal](/windsurf/terminal).

It can detect which packages and tools that you're using, which ones need to be installed, and even install them for you. Just ask Cascade how to run your project and press Accept.

<Note>Cascade can make up to 25 tool calls per prompt. If the trajectory stops, simply type in `continue` and Cascade will resume from where it left off. Each `continue` will count as a new prompt. </Note>

# Voice input

Use Voice input to use your voice to interact with Cascade. In its current form it can transcribe your speech to text.

<video />

# Revert to previous steps

You have the ability to revert changes that Cascade has made if you want to. Simply hover your mouse over the original prompt and click on the revert arrow on the right, or revert directly from the table of contents. This will revert all code changes back to the state of your codebase at the desired step.

<Warning>Reverts are currently irreversible, so be careful!</Warning>

<video />

# Auto-Execution Modes

Cascade supports three levels of command auto-execution in JetBrains: **Off**, **Auto**, and **Turbo**. You can select your preferred level via the Windsurf Settings panel.

| Level     | Description                                                                                                    |
| --------- | -------------------------------------------------------------------------------------------------------------- |
| **Off**   | Never auto-execute terminal commands, except those in your allow list.                                         |
| **Auto**  | Model decides whether to auto-execute commands based on safety assessment. Available with premium models only. |
| **Turbo** | Always auto-execute terminal commands and browser controls, except those in your deny list.                    |

<Note>For Teams and Enterprise users, administrators can set a maximum allowed auto-execution level. Users can select any level up to that maximum, but cannot exceed it.</Note>

<Frame>
  <img />
</Frame>

For more details on auto-execution levels and allow/deny lists, see the [Terminal documentation](/windsurf/terminal#auto-executed-cascade-commands).

# Real-time collaboration

A unique capability of Cascade is that it is aware of your real-time actions.

You no longer necessarily need to prompt with context on your prior actions, as Cascade is already aware.

Try making a manual change in the code editor, and then prompt Cascade to "continue my work"!

# Ignoring files

If you'd like Cascade to ignore files, you can add your files to `.codeiumignore` at the root of your workspace. This will prevent Cascade from viewing, editing or creating files inside of the paths designated. You can declare the file paths in a format similar to `.gitignore`.

## Global .codeiumignore

For enterprise customers managing multiple repositories, you can enforce ignore rules across all repositories by placing a global `.codeiumignore` file in the `~/.codeium/` folder. This global configuration will apply to all Windsurf workspaces on your system and works in addition to any repository-specific `.codeiumignore` files.
