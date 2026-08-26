# Chat Overview

Chat with your codebase using Windsurf Chat in VS Code and JetBrains. Use @-mentions, persistent context, pinned files, and inline citations.

<Note>
  Chat and its related features are only supported in: VS Code, JetBrains IDEs, Eclipse, X-Code, and Visual Studio.
</Note>

**Windsurf Chat** enables you to talk to your codebase from within your editor.
Chat is powered by our [context awareness](/context-awareness/overview.mdx) engine.
It combines built-in context retrieval with optional user guidance to provide accurate and grounded answers.

<Tabs>
  <Tab title="VS Code">
    In VS Code, Windsurf Chat can be found by default on the left sidebar.
    If you wish to move it elsewhere, you can click and drag the Windsurf icon and relocate it as desired.

    <Frame>
      <img />
    </Frame>

    You can use `⌘+⇧+A` on Mac or `Ctrl+⇧+A` on Windows/Linux to open the chat panel and toggle focus between it and the editor.
    You can also pop the chat window out of the IDE entirely by clicking the page icon at the top of the chat panel.
  </Tab>

  <Tab title="JetBrains">
    In JetBrains IDEs, Windsurf Chat can be found by default on the right sidebar.
    If you wish to move it elsewhere, you can click and drag the Windsurf icon and relocate it as desired.

    <Frame>
      <img />
    </Frame>

    You can use `⌘+⇧+L` on Mac or `Ctrl+⇧+L` on Windows/Linux to open the chat panel while you are typing in the editor.
    You can also open the chat in a popped-out browser window by clicking `Tools > Windsurf > Open Windsurf Chat in Browser` in the top menu bar.
  </Tab>
</Tabs>

## @-Mentions

<Tip>An @-mention is a deterministic way of bringing in context, and is guaranteed to be part of the context used to respond to a chat.</Tip>

In any given chat message you send, you can explicitly refer to context items from within the chat input by prefixing a word with `@`.

Context items available to be @-mentioned:

* Functions & classes
  * Only functions and classes in the local indexed
  * Also only available for languages we have built AST parsers for (Python, TypeScript, JavaScript, Go, Java, C, C++, PHP, Ruby, C#, Perl, Kotlin, Dart, Bash, COBOL, and more)
* Directories and files in your codebase
* Remote repositories
* The contents of your in-IDE terminal (VS Code only).

<Frame>
  <img />
</Frame>

You can also try `@diff`, which lets you chat about your repository's current `git diff` state.
The `@diff` feature is currently in beta.

<Tip>If you want to pull a section of code into the chat and you don't have @-Mentions available, you can: 1. highlight the code -> 2. right click -> 3. select 'Windsurf: Explain Selected Code Block'</Tip>

## Persistent Context

You can instruct the chat model to use certain context throughout a conversation and across different conversations
by clicking on the `Advanced` tab in the chat panel.

<Frame>
  <img />
</Frame>

In this tab, you can see:

* **Custom Chat Instructions**: a short prompt guideline like "Respond in Kotlin and assume I have little familiarity with it" to orient the model towards a certain type of response.
* **Pinned Contexts**: items from your codebase like files, directories, and code snippets that you would like explicitly for the model to take into account.
  See also [Context Pinning](/context-awareness/overview#context-pinning).
* **Active Document**: a marker for your currently active file, which receives special focus.
* **Local Indexes**: a list of local repositories that the Windsurf context engine has indexed.

## Slash Commands

You can prefix a message with `/explain` to ask the model to explain something of your choice.
Currently, `/explain` is the only supported slash command.
[Let us know](https://discord.com/invite/3XFf78nAx5) if there are other common workflows you want wrapped in a slash command.

## Copy and Insert

Sometimes, Chat responses will contain code blocks. You can copy a code block to your clipboard or insert it directly into the editor
at your cursor position by clicking the appropriate button atop the code block.

<Note>
  If you would like the AI to enact a change directly in your editor based on an instruction,
  consider using [Windsurf Command](/command/plugins-overview).
</Note>

## Inline Citations

Chat is aware of code context items, and its responses often contain linked references to snippets of code in your files.

<Frame>
  <video />
</Frame>

## Regenerate with Context

By default, Windsurf makes a judgment call whether any given question is general or if it requires codebase context.

You can force the model to use codebase context by submitting your question with `⌘⏎`.
For a question that has already received a response, you rerun with context by clicking the sparkle icon.

<Frame>
  <img />
</Frame>

## Stats for Nerds

Lots of things happen under the hood for every chat message. You can click the stats icon to see these statistics for yourself.

<Frame>
  <img />
</Frame>

## Chat History

To revisit past conversations, click the history icon at the top of the chat panel.
You can click the `+` to create a new conversation, and
you can click the `⋮` button to export your conversation. This applies only for the Windsurf Plugins.

<Frame>
  <img />
</Frame>

## Settings

Click on the gear icon to reach the `Settings` tab. Here, you can view settings that are applicable to your account. For example, you can update your theme preferences (light or dark), change autocomplete speed, view current plan, and change font size.
The settings panel also gives you an option to download diagnostics, which are debug logs that can be helpful for the Windsurf team to debug an issue, should you encounter one.

<Frame>
  <img />
</Frame>

## Telemetry

<Note>You may encounter issues with Chat if Telemetry is not enabled.</Note>

<Tabs>
  <Tab title="VS Code">
    To enable telemetry, open your VS Code settings and navigate to User > Application > Telemetry. In the following dropdown, select "all".

    <img />
  </Tab>

  <Tab title="JetBrains">
    To enable telemetry in JetBrains IDEs, open your Settings and navigate to Appearance & Hehavior > System Settings > Data Sharing.

    <img />
  </Tab>
</Tabs>
