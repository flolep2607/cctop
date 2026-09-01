# Search

## Instant Grep

The fastest way to find code is an exact match: a function name, variable, error string, or regex pattern. Agent uses grep automatically when you reference specific symbols.

Cursor ships with [Instant Grep](/changelog/2-1#instant-grep-beta), a custom search engine that outperforms `ripgrep` on large codebases. It runs automatically; no configuration needed.

Instant Grep supports full regex and word-boundary matching, so Agent can construct patterns like `import.*PaymentService` or `PaymentFailedError` to trace references across files.

## Privacy and security

File paths are encrypted before being sent to Cursor's servers. Code content is never stored in plaintext; it is held in memory during indexing, then discarded.

## Explore subagent

Agent can spawn an [Explore subagent](https://cursor.com/docs/subagents.md) that runs in its own context window with a faster model. It executes many parallel searches without bloating the main conversation, returning only the relevant findings.

Agent uses the Explore subagent automatically when it decides a task benefits from broad search. You can also request it directly: "use a subagent to find all the places we validate user input."

This is useful for context management. Searching through many files generates a lot of context. The subagent keeps the main conversation focused by summarizing results instead of dumping raw file contents.

## FAQ

### Is my source code stored on Cursor servers?

No. Cursor creates embeddings without storing filenames or source code. Filenames are obfuscated and code chunks are encrypted. When Agent searches, Cursor retrieves the embeddings and decrypts the chunks on the client side.

### Can I customize path encryption?

Create a `.cursor/keys` file in your workspace root:

```json
{
  "path_decryption_key": "your-custom-key-here"
}
```

### How does team sharing work?

Indexes can be shared across team members for faster indexing of similar codebases. Cursor respects file access permissions and only shares accessible content.

### Does Cursor support multi-root workspaces?

Yes. Cursor supports [multi-root workspaces](https://code.visualstudio.com/docs/editor/workspaces#_multiroot-workspaces). All codebases get indexed automatically, and each codebase's context is available to Agent. Some features that rely on a single git root, like worktrees, are disabled for multi-root workspaces. Cloud Agents do not support multi-root workspaces.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
