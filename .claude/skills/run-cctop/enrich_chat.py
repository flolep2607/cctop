#!/usr/bin/env python3
"""Append turns to the fixture's Claude transcript that exercise the chat view.

The fixture `driver.sh` writes is two turns of plain text, which is enough for
a table row and nothing else. The conversation view renders markdown, tables,
fenced code, tool calls, and the blocks the harness writes through the user's
turn — none of which the bare fixture contains, so a change to any of it looks
fine against a fixture that never asks it to do anything.

Idempotent: re-running replaces the appended block rather than stacking another
copy, so it is safe to call before every run.
"""

import glob
import json
import os
import sys

MARK = "cctop-fixture-chat"


def turns(cwd: str) -> list[dict]:
    """One of everything the conversation view knows how to draw."""
    say = lambda role, text, **kw: {  # noqa: E731
        "type": role,
        "timestamp": "2026-08-26T03:00:00.000Z",
        "cwd": cwd,
        "message": {"role": role, "content": text},
        **kw,
    }
    return [
        # A slash command, which the harness records as four tags. Shown raw it
        # is four lines of markup for one token.
        say(
            "user",
            "<command-name>/loop</command-name>\n"
            "<command-message>loop</command-message>\n"
            "<command-args>5m</command-args>\n"
            "<local-command-stdout>started</local-command-stdout>",
        ),
        # A reminder: prose in a wrapper, and the wrapper is not worth drawing.
        say("user", "<system-reminder>\nthe file changed on disk\n</system-reminder>"),
        say("user", f"what does `{MARK}` cover?"),
        {
            "type": "assistant",
            "timestamp": "2026-08-26T03:00:01.000Z",
            "requestId": "req_fixture_chat",
            "message": {
                "id": "m_fixture_chat",
                "role": "assistant",
                "model": "claude-opus-5",
                "content": [
                    {
                        "type": "text",
                        "text": (
                            "## What this fixture covers\n\n"
                            "Every block the view knows, so a change to one of "
                            "them **fails visibly** rather than quietly.\n\n"
                            "- a bullet, with `inline code`\n"
                            "- a [link](https://example.com) and a bare "
                            "https://cctop.dev/x\n"
                            "- a bullet that wraps onto a second line so the "
                            "list item joining is exercised too\n\n"
                            "1. ordered\n2. list\n\n"
                            "| key | action | presses |\n"
                            "|-----|:------:|--------:|\n"
                            "| `F12` | back to the **dashboard** | 1 |\n"
                            "| `Alt+n` | the new-tab launcher, a longer cell "
                            "so a column has to give | 12 |\n\n"
                            "```rust\nfn main() { println!(\"hi\"); }\n```\n\n"
                            "> and a quote to finish."
                        ),
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_fixture_1",
                        "name": "Bash",
                        "input": {"command": "ls -la", "description": "List files"},
                    },
                ],
                "usage": {"input_tokens": 1200, "output_tokens": 320},
            },
        },
        {
            "type": "user",
            "timestamp": "2026-08-26T03:00:02.000Z",
            "cwd": cwd,
            "message": {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_fixture_1",
                        "content": "total 0\ndrwxr-xr-x 2 flo flo 40 Aug 26 03:00 .",
                    }
                ],
            },
        },
    ]


def main() -> int:
    home = sys.argv[1] if len(sys.argv) > 1 else "/tmp/cctop-drive/home"
    found = glob.glob(os.path.join(home, ".claude/projects/*/*.jsonl"))
    if not found:
        print(f"no fixture transcript under {home} — run driver.sh fixture", file=sys.stderr)
        return 1
    path = found[0]
    cwd = os.path.join(home, "repo")

    with open(path) as fh:
        kept = [line for line in fh if MARK not in line and "fixture_chat" not in line]
    # The two harness blocks carry no marker of their own, so they are dropped
    # by shape instead: re-running must not stack a second `/loop`.
    kept = [
        line
        for line in kept
        if "<command-name>" not in line and "<system-reminder>" not in line
    ]
    with open(path, "w") as fh:
        fh.writelines(kept)
        for turn in turns(cwd):
            fh.write(json.dumps(turn) + "\n")
    print(f"enriched {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
