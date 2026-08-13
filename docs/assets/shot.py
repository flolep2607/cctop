#!/usr/bin/env python3
"""Capture a real cctop screen and render it to a PNG for the docs.

cctop is a TUI, so a screenshot has to come from a real pty. tmux provides one,
`capture-pane -e` gives the screen back with its colours intact, and this draws
that grid with a font bundled on the machine rather than trusting the reader's.

That last part is the reason this is a PNG and not an SVG. cctop draws its
sparklines with eight-dot braille (U+2840 and up), and DejaVu Sans Mono — one of
the most widely installed monospace faces there is — only covers the six-dot
block. In an SVG those cells become tofu on any reader whose font agrees. Here
the glyphs are rasterised once, from a face known to have them.

    python3 docs/assets/shot.py docs/assets/dashboard.png
    python3 docs/assets/shot.py docs/assets/context.png --keys Tab*7 --size 146x34

The tmux session must NOT be named `cctop-*`: cctop adopts sessions with that
prefix as its own tabs, and would attach to the one it is running in.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time
from PIL import Image, ImageDraw, ImageFont

BASE16 = [
    "#000000", "#cd0000", "#00cd00", "#cdcd00", "#0000ee", "#cd00cd", "#00cdcd", "#e5e5e5",
    "#7f7f7f", "#ff0000", "#00ff00", "#ffff00", "#5c5cff", "#ff00ff", "#00ffff", "#ffffff",
]
CUBE = [0, 95, 135, 175, 215, 255]
FG_DEFAULT, BG_DEFAULT = "#c8ccd4", "#0e1116"
SESSION = "shotbox"   # must not start with `cctop-`; see the note above
DEJAVU = "/usr/share/fonts/truetype/dejavu/"
SGR = re.compile(r"\x1b\[([0-9;]*)m")

# Always scrubbed, because cctop reads the signed-in account out of each
# harness's config and puts it in the Info panel — an address the person taking
# the screenshot never chose to publish and will not think to look for.
EMAIL = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")


def redact(rows, extra):
    """Overwrite sensitive spans in place, on the parsed grid.

    On the grid rather than on the text, so a replacement of a different length
    cannot shift the columns: each cell keeps its colour and its position, and
    only the character changes. A shorter replacement is padded, a longer one
    truncated — the screenshot stays exactly as wide as the terminal was.
    """
    rules = [(EMAIL, "you@example.com")] + list(extra)
    for row in rows:
        line = "".join(c[0] for c in row)
        for pattern, replacement in rules:
            for m in pattern.finditer(line):
                span = m.end() - m.start()
                fill = replacement.ljust(span)[:span]
                for i, ch in enumerate(fill):
                    cell = row[m.start() + i]
                    row[m.start() + i] = (ch,) + cell[1:]


def palette(n):
    if n < 16:
        return BASE16[n]
    if n < 232:
        n -= 16
        return "#%02x%02x%02x" % (CUBE[n // 36], CUBE[(n // 6) % 6], CUBE[n % 6])
    v = 8 + (n - 232) * 10
    return "#%02x%02x%02x" % (v, v, v)


def cells(text):
    """Parse ANSI into rows of (char, fg, bg, bold)."""
    rows = []
    for raw in text.rstrip("\n").split("\n"):
        fg, bg, bold, row, i = None, None, False, [], 0
        while i < len(raw):
            m = SGR.match(raw, i)
            if m:
                args = [int(x) for x in m.group(1).split(";") if x != ""] or [0]
                j = 0
                while j < len(args):
                    a = args[j]
                    if a == 0:
                        fg, bg, bold = None, None, False
                    elif a == 1:
                        bold = True
                    elif a == 22:
                        bold = False
                    elif a == 39:
                        fg = None
                    elif a == 49:
                        bg = None
                    elif 30 <= a <= 37:
                        fg = palette(a - 30)
                    elif 90 <= a <= 97:
                        fg = palette(a - 90 + 8)
                    elif 40 <= a <= 47:
                        bg = palette(a - 40)
                    elif 100 <= a <= 107:
                        bg = palette(a - 100 + 8)
                    elif a in (38, 48) and j + 2 < len(args) and args[j + 1] == 5:
                        if a == 38:
                            fg = palette(args[j + 2])
                        else:
                            bg = palette(args[j + 2])
                        j += 2
                    j += 1
                i = m.end()
                continue
            row.append((raw[i], fg, bg, bold))
            i += 1
        rows.append(row)
    return rows


def runs(row):
    """Collapse a row into (column, text, fg, bg, bold) runs of one style."""
    out, start = [], 0
    for i, cell in enumerate(row):
        if i and cell[1:] != row[i - 1][1:]:
            out.append((start, "".join(c[0] for c in row[start:i])) + row[start][1:])
            start = i
    if row:
        out.append((start, "".join(c[0] for c in row[start:])) + row[start][1:])
    return out


def newest_build():
    """The most recently built binary, not release-by-preference.

    A stale release binary left over from an earlier version silently produces a
    picture of an old UI — which is how this first shipped a screenshot missing
    two columns that had been added that afternoon.
    """
    builds = [p for p in ("./target/release/cctop", "./target/debug/cctop")
              if os.path.exists(p)]
    if not builds:
        sys.exit("build cctop first: cargo build --release")
    binary = max(builds, key=os.path.getmtime)
    print(f"capturing {binary}")
    return binary


def capture(size, keys, settle):
    cols, lines = size.split("x")
    subprocess.run(["tmux", "kill-session", "-t", SESSION],
                   stderr=subprocess.DEVNULL, check=False)
    binary = newest_build()
    # CI=1 suppresses the first-run alias prompt, which would sit over the UI.
    subprocess.run(["tmux", "new-session", "-d", "-s", SESSION, "-x", cols, "-y", lines,
                    f"CI=1 {binary}"], check=True)
    try:
        time.sleep(settle)
        for key in keys:
            subprocess.run(["tmux", "send-keys", "-t", SESSION, key], check=True)
            time.sleep(0.4)
        if keys:
            time.sleep(2)
        out = subprocess.run(["tmux", "capture-pane", "-t", SESSION, "-p", "-e"],
                             capture_output=True, text=True, check=True)
        return out.stdout
    finally:
        subprocess.run(["tmux", "kill-session", "-t", SESSION],
                       stderr=subprocess.DEVNULL, check=False)


def render(rows, scale, pt):
    fs = pt * scale
    reg = ImageFont.truetype(DEJAVU + "DejaVuSansMono.ttf", fs)
    bold_f = ImageFont.truetype(DEJAVU + "DejaVuSansMono-Bold.ttf", fs)
    fallback = ImageFont.truetype(DEJAVU + "DejaVuSans.ttf", fs)
    notdef = reg.getmask("￾").getbbox()
    cw, lh, pad = reg.getlength("M"), int(fs * 1.22), 14 * scale

    def absent(ch):
        return reg.getmask(ch).getbbox() == notdef

    cols = max(len(r) for r in rows)
    img = Image.new("RGB", (int(cols * cw + pad * 2), int(len(rows) * lh + pad * 2)),
                    BG_DEFAULT)
    draw = ImageDraw.Draw(img)
    # Backgrounds as one layer, so no glyph is clipped by the next cell's fill.
    for y, row in enumerate(rows):
        for col, text, _fg, bg, _b in runs(row):
            if bg:
                x, top = pad + col * cw, pad + y * lh
                draw.rectangle([x, top, x + len(text) * cw, top + lh], fill=bg)
    for y, row in enumerate(rows):
        for col, text, fg, _bg, bold in runs(row):
            if not text.strip():
                continue
            colour, font = fg or FG_DEFAULT, bold_f if bold else reg
            if any(absent(c) for c in text):
                for k, ch in enumerate(text):
                    draw.text((pad + (col + k) * cw, pad + y * lh), ch,
                              font=fallback if absent(ch) else font, fill=colour)
            else:
                draw.text((pad + col * cw, pad + y * lh), text, font=font, fill=colour)
    return img


def to_ansi(rows):
    """Serialise a (possibly redacted) grid back to ANSI.

    The cast has to carry the scrubbed screen, not the captured one, so the
    frames are rebuilt from the grid rather than replayed from the capture.
    """
    out = []
    for row in rows:
        line = ["\x1b[0m"]
        for _col, text, fg, bg, bold in runs(row):
            codes = ["0"]
            if bold:
                codes.append("1")
            for colour, prefix in ((fg, "38"), (bg, "48")):
                if colour:
                    codes.append(f"{prefix};5;{nearest256(colour)}")
            line.append(f"\x1b[{';'.join(codes)}m{text}")
        out.append("".join(line) + "\x1b[0m")
    return "\r\n".join(out)


def nearest256(hexcolour):
    """Map a hex colour back to its 256-palette index."""
    want = tuple(int(hexcolour[i:i + 2], 16) for i in (1, 3, 5))
    best, dist = 0, 1 << 30
    for n in range(256):
        have = tuple(int(palette(n)[i:i + 2], 16) for i in (1, 3, 5))
        d = sum((a - b) ** 2 for a, b in zip(want, have))
        if d < dist:
            best, dist = n, d
    return best


# What the recording shows: settle, walk the table, step through the panels to
# the context breakdown, then filter. Each entry is (key or literal, frames to
# hold afterwards) — the hold is what gives a reader time to read the screen.
DEMO = [
    ("", 4),
    ("Down", 1), ("Down", 3),
    ("Right", 2), ("Right", 2), ("Right", 4),
    ("Right", 1), ("Right", 1), ("Right", 1), ("Right", 5),
    ("/", 2), ("cctop", 4), ("Escape", 3),
]
KEYNAMES = {"Down", "Up", "Left", "Right", "Escape", "Tab", "Enter", "Space"}


def record(size, settle, extra, scale):
    """Drive the demo and return (rendered frames, redacted ANSI frames)."""
    cols, lines = size.split("x")
    subprocess.run(["tmux", "kill-session", "-t", SESSION],
                   stderr=subprocess.DEVNULL, check=False)
    binary = newest_build()
    subprocess.run(["tmux", "new-session", "-d", "-s", SESSION, "-x", cols, "-y", lines,
                    f"CI=1 {binary}"], check=True)
    images, casts = [], []
    try:
        time.sleep(settle)
        for key, hold in DEMO:
            if key:
                flag = [] if key in KEYNAMES else ["-l"]
                subprocess.run(["tmux", "send-keys", "-t", SESSION] + flag + [key],
                               check=True)
                time.sleep(0.35)
            for _ in range(hold):
                out = subprocess.run(["tmux", "capture-pane", "-t", SESSION, "-p", "-e"],
                                     capture_output=True, text=True, check=True)
                rows = cells(out.stdout)
                redact(rows, extra)
                images.append(render(rows, scale, 14))
                casts.append(to_ansi(rows))
                time.sleep(0.55)
    finally:
        subprocess.run(["tmux", "kill-session", "-t", SESSION],
                       stderr=subprocess.DEVNULL, check=False)
    return images, casts


def write_cast(path, frames, size, delay):
    """An asciinema v2 cast: a header line then [time, "o", data] events.

    Each frame is a full repaint rather than a diff. A capture-based recording
    has no keystroke-level output to replay, and a flipbook of whole screens is
    the honest shape of what was actually observed.
    """
    cols, lines = size.split("x")
    header = {"version": 2, "width": int(cols), "height": int(lines),
              "env": {"TERM": "xterm-256color", "SHELL": "/bin/sh"}}
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(json.dumps(header) + "\n")
        for i, frame in enumerate(frames):
            payload = "\x1b[H\x1b[2J" + frame
            fh.write(json.dumps([round(i * delay, 3), "o", payload]) + "\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("out")
    ap.add_argument("--size", default="146x30", help="terminal size, COLSxLINES")
    ap.add_argument("--keys", default="", help="keys to send, e.g. Tab*7")
    ap.add_argument("--settle", type=float, default=18.0,
                    help="seconds to wait for the first full load")
    ap.add_argument("--scale", type=int, default=2)
    ap.add_argument("--redact", action="append", default=[], metavar="TEXT[=WITH]",
                    help="also scrub this literal; email addresses always are")
    ap.add_argument("--record", metavar="CAST",
                    help="record the demo: writes an animated GIF to `out` and "
                         "an asciinema cast to this path")
    args = ap.parse_args()

    extra = []
    for rule in args.redact:
        target, _, replacement = rule.partition("=")
        extra.append((re.compile(re.escape(target)), replacement or "redacted"))

    keys = []
    if args.keys:
        name, _, count = args.keys.partition("*")
        keys = [name] * int(count or 1)

    if args.record:
        delay = 0.9
        images, casts = record(args.size, args.settle, extra, args.scale)
        write_cast(args.record, casts, args.size, delay)
        # Quantised to one shared palette: a terminal uses few colours, and a
        # per-frame palette is what makes a screen recording enormous.
        first, rest = images[0], images[1:]
        first.save(args.out, save_all=True, append_images=rest,
                   duration=int(delay * 1000), loop=0, optimize=True)
        print(f"wrote {args.out} {first.size[0]}x{first.size[1]} "
              f"({len(images)} frames) and {args.record}")
        return

    rows = cells(capture(args.size, keys, args.settle))
    redact(rows, extra)
    img = render(rows, args.scale, 15)
    img.save(args.out, optimize=True)
    print(f"wrote {args.out} {img.size[0]}x{img.size[1]}")


if __name__ == "__main__":
    sys.exit(main())
