# Terminal

Cursor runs shell commands directly in your terminal. Your [Run Mode](https://cursor.com/docs/agent/security/run-modes.md) controls when commands run, when Cursor asks, and when terminal commands enter the sandbox.

## Sandbox

The sandbox runs terminal commands in a restricted environment that blocks unauthorized file access and network activity. For platform requirements, network modes, environment variables, and `sandbox.json` configuration, read [Run Modes > Sandboxing](https://cursor.com/docs/agent/security/run-modes.md#sandboxing).

## Troubleshooting

Some shell themes (for example, Powerlevel9k/Powerlevel10k) can interfere with
the inline terminal output. If your command output looks truncated or
misformatted, disable the theme or switch to a simpler prompt when Cursor runs.

### Disable heavy prompts for Cursor sessions

Use the `CURSOR_AGENT` environment variable in your shell config to detect when
Cursor is running and skip initializing fancy prompts/themes.

```zsh
# ~/.zshrc - disable Powerlevel10k when Cursor runs
if [[ -n "$CURSOR_AGENT" ]]; then
  # Skip theme initialization for better compatibility
else
  [[ -r ~/.p10k.zsh ]] && source ~/.p10k.zsh
fi
```

```bash
# ~/.bashrc - fall back to a simple prompt in Cursor sessions
if [[ -n "$CURSOR_AGENT" ]]; then
  PS1='\u@\h \W \$ '
fi
```

## Related

- [Terminal help](https://cursor.com/help/ai-features/terminal.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
