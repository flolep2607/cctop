# Canvases

Canvases let Cursor create interactive artifacts that render next to the chat. Instead of scrolling through a long markdown table or code block, you get a standalone view, laid out with sections, stats, and tables, that you can reopen, edit, and iterate on.

Ask agents for a dashboard, analysis, audit, or report, and Cursor opens the result in a canvas when that is a better fit.

## How it works

1. Cursor decides that your task benefits from a visual or interactive view, or you ask for one directly.
2. Cursor builds the canvas and inserts a reference to it in your chat.
3. You review the rendered view, switch to the source to tweak it, or ask Cursor to change it.
4. Cursor saves the canvas so you can reopen and rerun it later with fresh data.

Each canvas appears in your workspace's canvas list, so you can jump back to past ones without rerunning them.

## Opening a canvas

- **From Cursor**: when Cursor creates a canvas, a card appears at the end of the response. Click it to open.
- **Command Palette**: run **Open Canvas** from the palette, listed under View.
- **Agents Window**: open a canvas tab directly from the new tab menu in the [Agents Window](https://cursor.com/docs/agent/agents-window.md).

## Sharing canvases

Shared canvases turn an interactive artifact into something your whole team can open, not just you. When you share a canvas, Cursor uploads a live snapshot of the view and gives you a link teammates can open in the browser — same layout, charts, and tables, without rerunning the agent or digging through chat history. Use **Publish** from the canvas toolbar to publish or refresh a share; browse everything your team has published from **Shared Canvases** on the [dashboard](https://cursor.com/dashboard).

Shared canvases are available on paid plans (Pro, Teams, and Enterprise). Free accounts cannot create shares. Because each share is team-visible, you need to be on a team — Pro users on a team can share too. Sharing also requires a privacy mode that allows data storage (Legacy Privacy Mode blocks it).

Team admins can turn shared canvases off for the organization from [team settings](https://cursor.com/dashboard/settings#shared-canvases) under **Shared Canvases**.

## Iterating on a canvas

Canvases are designed to be easy to refine.

- If the layout isn't right, tell Cursor what to change instead of editing by hand.
- If the numbers look stale or off, ask Cursor to rerun the underlying query or show its work.
- For larger reworks, revert and prompt Cursor again with more details. This is usually faster than nudging through small follow-ups.
- For small tweaks, you can also manually edit the source code.

## Packaging in skills

Common canvas workflows can be packaged as [skills](https://cursor.com/docs/skills.md) so Cursor produces a consistent layout every time you ask.

A canvas skill typically includes:

- **A trigger description** so Cursor knows when to reach for it, like "quarterly revenue report" or "dependency audit".
- **Layout instructions** that define the sections, stats, and tables the canvas should contain.
- **Data sources and queries** Cursor should run to populate the view, such as a SQL query, API call, or shell command.
- **Formatting rules** like units, date ranges, or sort order.

Once the skill is in place, a short prompt is enough to regenerate the canvas with fresh data, and every teammate using the skill gets the same output shape.

## Related

- [Agents Window](https://cursor.com/docs/agent/agents-window.md)
- [Skills](https://cursor.com/docs/skills.md)
- [Prompting](https://cursor.com/docs/agent/prompting.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
