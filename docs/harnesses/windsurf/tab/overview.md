# Windsurf Tab

Windsurf Tab provides AI-powered code suggestions with Tab to Jump, Tab to Import, and inline suggestions, powered by our custom model.

**Windsurf Tab** has evolved from a simple autocomplete tool into a contextually aware diff-suggestion and navigation engine for writing code.

It is powered by our custom in-house model, trained from scratch to optimize for speed and flow awareness.

<Frame>
  <video />
</Frame>

Suggestions are based on the context of your code, terminal, Cascade chat history, your prior actions around the editor, and even your clipboard (must opt in via advanced Settings).

Tab is able to make complex edits *both before and after* your current cursor position. You can press `esc` to cancel a suggestion.

Suggestions will also disappear if you continue typing or navigating without accepting them.

## Keyboard Shortcuts

* **Accept suggestion**: `tab`
* **Cancel suggestion**: `esc`
* **Accept suggestion word-by-word**: `⌘+→` (VS Code), `⌥+⇧+\` (JetBrains)

## Tab to Jump

Windsurf can also anticipate your next cursor position and prompt you with a `Tab to Jump` label at a certain line in the editor, allowing you to easily navigate through your file.

If you accept by simply pressing `tab`, then you will be taken to that next position.

<Frame>
  <video />
</Frame>

## Tab to Import

After defining a new dependency to use in a file, just simply hit `tab` to import it at the top of the file once the hint shows. Your cursor will stay in the same position.

<Frame>
  <video />
</Frame>

## Settings

Windsurf Tab is offered in two modes: Autocomplete and Supercomplete.

Supercomplete is our most powerful and recommended mode, appearing in small windows around your cursor to suggest both deletions and additions.

Autocomplete is a more traditional autocomplete mode that appears at your cursor.

You can also opt-in to using your clipboard as context. This means if you copy something to your clipboard, Windsurf will be able to use it as context.

Tab to Import and Tab to Jump functionalities are also individually configurable in the settings.

<Frame>
  <img />
</Frame>

## Context Awareness

Windsurf Tab is broadly context-aware and adaptively responds to your current coding context, including recent terminal activity, your recent code changes, and clipboard contents.

<Frame>
  <video />
</Frame>
