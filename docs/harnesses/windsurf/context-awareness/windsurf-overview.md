# Context Awareness for Windsurf

Windsurf's RAG-based context engine indexes your codebase for intelligent code suggestions. Supports remote repositories for Teams and Enterprise.

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

## Chat-Specific Context Features

When conversing with Windsurf Chat, you have various ways of leveraging codebase context,
like [@-mentions](/chat/overview/#mentions) or custom guidelines.
See the [Chat page](/chat/overview) for more information.

<video />

## Frequently Asked Questions (FAQs)

### Does Windsurf index my codebase?

Yes, Windsurf does index your codebase. It also uses LLMs to perform retrieval-augmented generation (RAG) on your codebase using our own [M-Query](https://youtu.be/DuZXbinJ4Uc?feature=shared\&t=606) techniques.

Indexing performance and features vary based on your workflow and your Windsurf plan. For more information, please visit our [context awareness page](https://windsurf.com/context).
