# Spaces

Spaces group all of the agent sessions, PRs, files, and context for a task or project into a single view in the Agent Command Center.

Spaces are how you organize work in the [Agent Command Center](/windsurf/agent-command-center).

A Space groups everything related to a specific task or project into a single view: agent sessions, PRs, files, and context. For example, an "Onboarding Flow Redesign" space might have one local Cascade session prototyping the UI and two cloud Devin sessions handling API changes and writing tests.

<Note>
  Every session is its own Space by default, even if it isn't shown as one. You don't need to create a Space to start working — you can group sessions into a shared Space whenever it's useful.
</Note>

## What lives in a Space

A Space brings together everything you need to work on a task without context switching:

* **Agent sessions** — Local Cascade sessions and cloud [Devin](/windsurf/devin) sessions running for this task.
* **Pull requests** — PRs opened by you or by agents working in the Space.
* **Files** — Files relevant to the task.
* **Context** — Project-level context that new sessions in the Space inherit.

## Context is shared across sessions

When you create a new session in a Space, it inherits everything the Space already knows about the project. This means new agents can start working immediately without you having to re-explain the project each time.

## Switching between Spaces

When you return to a Space, the view is restored exactly as you left it.

Switching between Spaces is the same as switching between tasks — except now each task has a team of agents working inside it.

<video />

## Creating a Space

There are a few ways to start a new Space:

* **Drag a session into another session.** In the sidebar, drag any session onto an existing session to group them together as a Space.
* **Open a new session in a split pane.** Press `Cmd/Ctrl+\` to split the current pane, then click **New Session** in the empty pane to start a new session in the same Space.
* **Use `Cmd/Ctrl+T`.** This opens a new session inside the current Space.

<video />
