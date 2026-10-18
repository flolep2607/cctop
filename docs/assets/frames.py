#!/usr/bin/env python3
"""Render the snapshot fixture's frames to PNG for the documentation.

The pictures in docs/ are cctop drawing the snapshot tests' made-up sessions
(crates/ui/src/snapshot.rs), not a capture of a real machine: nothing personal
can be in frame, and the same command redraws all of them after a change to
the UI. The tests write each large frame as ANSI when CCTOP_FRAMES names a
directory; shot.py's rasteriser turns them into images.

    python3 docs/assets/frames.py            # every frame in FRAMES below

Add a picture by naming its snapshot in FRAMES and linking the PNG.
"""

import os
import re
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from shot import cells, render  # noqa: E402

# A hyperlink's escape rides in one cell's symbol; the picture wants its label.
OSC8 = re.compile(r"\x1b\]8;[^\x07\x1b]*(?:\x1b\\|\x07)")

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "docs", "assets", "frames")

# snapshot name -> rows to keep from the top (None for the whole frame). A
# modal or a menu sits over the table, and the table under it is context worth
# keeping; the tall bottom of a frame with nothing on it is not.
FRAMES = {
    "dashboard": None,
    "context": None,
    "help": None,
    "sort": None,
    "filter_active": None,
    "search_typing": None,
    "row_menu": None,
    "handoff_picker": None,
    "new_tab_remote": None,
    "launch_options": None,
    "new_worktree": None,
    "delete_confirm": None,
    "idle_view": None,
    "idle_reclaim": None,
    "dead_pane": None,
    "remote_tabs": None,
    "share_address": None,
    "settings": None,
    "settings_tunnel": None,
    "settings_tunnel_qr": None,
    "octant_charts": None,
}


def main():
    os.makedirs(OUT, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        env = dict(os.environ, CCTOP_FRAMES=tmp)
        subprocess.run(["cargo", "test", "-p", "cctop-ui", "snapshot::"],
                       cwd=ROOT, env=env, check=True, stdout=subprocess.DEVNULL)
        for name, keep in FRAMES.items():
            with open(os.path.join(tmp, f"{name}.ansi"), encoding="utf-8") as fh:
                rows = cells(OSC8.sub("", fh.read()))[:keep]
            # Stripping the escape leaves the label plus the blank cells that
            # stood behind it: trim that padding back to the frame's width.
            width = 120
            for row in rows:
                while len(row) > width and row[-1][0] == " ":
                    row.pop()
            path = os.path.join(OUT, f"{name}.png")
            # Twice the pixels for a sharp page on a dense screen; a terminal
            # frame has few colours, so a palette image costs a quarter.
            image = render(rows, 2, 14).convert("P", palette=1, colors=256)
            image.save(path, optimize=True)
            print(path)


if __name__ == "__main__":
    main()
