"""Split docs.windsurf.com's llms-full.txt back into one markdown file per page.

Windsurf is the one harness whose docs site will not hand over the markdown
behind a page — every `.md` URL answers with the single-page app. The one file
it does publish concatenates the whole site, each page introduced by its title
and a `Source:` line, so that line is what the pages are cut on.
"""

import os
import re
import sys

PAGE = re.compile(r"(?m)^(#[^\n]*\nSource: https://docs\.windsurf\.com/\S+)$")


def main(src: str, dst: str) -> None:
    parts = PAGE.split(open(src, encoding="utf-8").read())
    for head, body in zip(parts[1::2], parts[2::2]):
        title, source = head.split("\n", 1)
        rel = source.split("docs.windsurf.com/", 1)[1].strip() + ".md"
        out = os.path.join(dst, rel)
        os.makedirs(os.path.dirname(out), exist_ok=True)
        with open(out, "w", encoding="utf-8") as f:
            f.write(f"{title}\n\n{body.strip()}\n")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
