# Design Mode

Design Mode lets you direct agents with visual prompts. From the browser in the [Agents Window](https://cursor.com/docs/agent/agents-window.md), you can click an element, draw on the page, or describe a change by voice. Cursor captures the context it needs and edits the code while you move on to the next change.

UI work tends to be spatial. Instead of describing a change in a sentence, your instruction can include the selected element, the code behind it, the surrounding layout, and the visual relationships on the page. This tightens the loop between noticing something and fixing it.

Click an element in the running app, prompt against that selected element, and let the agent edit the code.

## Open Design Mode

Design Mode lives in the browser inside the Agents Window. Open the browser, then toggle Design Mode with Cmd + Shift + D. Toggle it off with the same shortcut to return to normal browsing.

## Ways to direct the agent

Design Mode gives you several ways to convey intent.

### Select an element

Click any element in the running product to target it. The agent gets the element and its code, so you can prompt against the exact thing you see without leaving the app.

### Select multiple elements

Multi-select helps when the change depends on a relationship between elements. Reference two components and ask the agent to make one match the other, remove repeated content, or adjust a group together.

Select multiple elements and describe how they should change together.

### Draw on the page

Drawing tells the agent which area of the page your instruction applies to. Circle a crowded section, box in a region, or mark part of an animated page. The annotation sits over a frozen frame of the viewport, so the agent sees the exact page state you were responding to.

### Narrate by voice

You can narrate instructions with your voice instead of typing. The mic stays available while agents run, so you can queue the next change without waiting.

Use voice input and drawing together to describe a change.

## Keyboard shortcuts

| Action               | Shortcut        |
| :------------------- | :-------------- |
| Toggle Design Mode   | Cmd + Shift + D |
| Select an area       | Shift + drag    |
| Add element to chat  | Cmd + L         |
| Add element to input | Option + click  |

## What the agent sees

Picking an element adds two complementary signals to context:

- **Element identity**: the xpath, the component, attributes, computed styles, and props from the fiber tree. This helps the agent find the source and edit the right code.
- **A screenshot**: the layout, surrounding elements, and the exact page state. This gives the agent spatial context for the change.

## Work in flow

When you refine an interface, one edit usually leads to the next. You adjust a component, notice the spacing around it, then see how another component should match.

Design Mode lets you send those edits away as you notice them. Point at one element, describe the change, move to another part of the page, and send another edit before the first one finishes. This makes it easy to multitask and manage several subagents at once. As agents finish, the app hot reloads and your changes appear in the running product.

This flow works best with a fast model that is strong at interface work. We recommend [Composer 2.5](/blog/composer-2-5).

## Related

- [Agents Window](https://cursor.com/docs/agent/agents-window.md)
- [Browser](https://cursor.com/docs/agent/tools/browser.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
