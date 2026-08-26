# Context Awareness Overview

Windsurf's RAG-based context engine indexes your codebase for intelligent suggestions. Learn about context pinning, knowledge base, and M-Query retrieval.

Windsurf's context engine builds a deep understanding of your codebase, past actions, and next intent.

Historically, code-generation approaches focused on fine-tuning large language models (LLMs) on a codebase,
which is difficult to scale to the needs of every individual user.
A more recent and popular approach leverages retrieval-augmented generation (RAG),
which focuses on techniques to construct highly relevant, context-rich prompts
to elicit accurate answers from an LLM.

We've implemented an optimized RAG approach to codebase context,
which produces higher quality suggestions and fewer hallucinations.

<Note>
  Windsurf offers full fine-tuning for enterprises, and the best solution
  combines fine-tuning with RAG.
</Note>

## Default Context

Out of the box, Windsurf takes multiple relevant sources of context into consideration.

* The current file and other open files in your IDE, which are often very relevant to the code you are currently writing.
* The entire local codebase is then indexed (including files that are not open),
  and relevant code snippets are sourced by Windsurf's retrieval engine as you write code, ask questions, or invoke commands.
* For Pro users, we offer expanded context lengths increased indexing limits, and higher limits on custom context and pinned context items.
* For Teams and Enterprise users, Windsurf can also index remote repositories.
  This is useful for companies whose development organization works across multiple repositories.

## Knowledge Base (Beta)

<Note>Only available for Teams and Enterprise customers.</Note>

This feature allows teams to pull in Google Docs as shared context or knowledge sources for their entire team.

Currently, only Google Docs are supported. Images are not imported, but charts, tables, and formatted text are fully supported.

<Card title="Knowledge Base" icon="people-group" href="https://windsurf.com/team/settings">
  Configure knowledge base settings for your team. This page will only be visible with admin privileges.
</Card>

Admins must manually connect with Google Drive via OAuth, after which they can add up to 50 Google Docs as team knowledge sources.

Cascade will have access to the docs specified in the Windsurf dashboard. These docs do not obey individual user access controls, meaning if an admin makes a doc available to the team, all users will have access to it regardless of access controls on the Google Drive side.

### Best Practices

Context Pinning is great when your task in your current file depends on information from other files.
Try to pin only what you need. Pinning too much may slow down or negatively impact model performance.

Here are some ideas for effective context pinning:

* Module Definitions: pinning class/struct definition files that are inside your repo but in a module separate from your currently active file.
* Internal Frameworks/Libraries: pinning directories with code examples for using frameworks/libraries.
* Specific Tasks: pinning a file or folder defining a particular interface (e.g., `.proto` files, abstract class files, config templates).
* Current Focus Area: pinning the "lowest common denominator" directory containing the majority of files needed for your current coding session.
* Testing: pinning a particular file with the class you are writing unit tests for.

## Chat-Specific Context Features

When conversing with Windsurf Chat, you have various ways of leveraging codebase context,
like [@-mentions](/chat/overview/#mentions) or custom guidelines.
See the [Chat page](/chat/overview) for more information.

<video />

## Frequently Asked Questions (FAQs)

### Does Windsurf index my codebase?

Yes, Windsurf does index your codebase. It also uses LLMs to perform retrieval-augmented generation (RAG) on your codebase using our own [M-Query](https://youtu.be/DuZXbinJ4Uc?feature=shared\&t=606) techniques.

Indexing performance and features vary based on your workflow and your Windsurf plan. For more information, please visit our [context awareness page](https://windsurf.com/context).
