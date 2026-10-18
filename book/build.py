#!/usr/bin/env python3
"""Assemble book/src from the pages SUMMARY.md lists, then run mdbook.

The pages stay where they are in the repository, and are copied here at the
same paths (README.md, docs/the-table.md, ...), rather than moved into the
book: the README is what GitHub and crates.io show, and docs/ is linked from
the code and from `cctop --help`. Copying keeps one source and keeps every
relative link between pages valid. A link that leaves the set of pages, to
source code or to a file the site does not carry, is rewritten to the file
on GitHub, so nothing on the site is a 404.

Usage: book/build.py [mdbook args...]   (default: build)
"""
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

BOOK = Path(__file__).resolve().parent
ROOT = BOOK.parent
SRC = BOOK / "src"
BLOB = "https://github.com/flolep2607/cctop/blob/main/"
TREE = "https://github.com/flolep2607/cctop/tree/main/"

pages = re.findall(r"\]\(([^)]+\.md)\)", (BOOK / "SUMMARY.md").read_text())
page_set = set(pages)
LINK = re.compile(r"(\]\(|(?:src|href)=\")([^)\"#\s]+)([^)\"\s]*)")


def rewrite(page: str, text: str) -> str:
    base = Path(page).parent

    def fix(m: re.Match) -> str:
        lead, target, rest = m.groups()
        if re.match(r"^[a-z]+:", target) or target.startswith("/"):
            return m.group(0)
        resolved = os.path.normpath(base / target)
        if (ROOT / resolved).is_dir():
            if f"{resolved}/README.md" not in page_set:
                return lead + TREE + resolved + rest
            resolved += "/README.md"
        if resolved in page_set and Path(resolved).name == "README.md":
            # mdbook publishes a README as index.html but leaves links to it
            # pointing at README.html, which does not exist.
            index = Path(resolved).with_name("index.html")
            return lead + os.path.relpath(index, base) + rest
        if resolved in page_set or resolved.startswith("docs/assets/"):
            return m.group(0)
        return lead + BLOB + resolved + rest

    # Leave fenced code alone: a path in a shell example is not a link.
    parts = re.split(r"(^```.*?^```)", text, flags=re.S | re.M)
    return "".join(p if p.startswith("```") else LINK.sub(fix, p) for p in parts)


shutil.rmtree(SRC, ignore_errors=True)
for page in pages:
    out = SRC / page
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(rewrite(page, (ROOT / page).read_text()))
shutil.copytree(ROOT / "docs/assets", SRC / "docs/assets",
                ignore=shutil.ignore_patterns("*.py", "__pycache__"))
shutil.copy(BOOK / "SUMMARY.md", SRC / "SUMMARY.md")

mdbook = os.environ.get("MDBOOK", "mdbook")
sys.exit(subprocess.call([mdbook, *(sys.argv[1:] or ["build"])], cwd=BOOK))
