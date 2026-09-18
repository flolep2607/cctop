#!/usr/bin/env python3
"""Screenshot one cctop page, and print the text it actually rendered.

Driven by `web.sh shot`, which passes everything through the environment so the
argument quoting stays in one place.

`CCTOP_DEAD=1` answers every `/api/**` request with a Cloudflare 502 page
instead of letting it through. That is what a trycloudflare tunnel serves once
the cctop behind it has gone, and it is the failure the pages have to survive
without pasting somebody else's HTML into the view — so it is worth being able
to produce on demand rather than by unplugging something.
"""

import os
import sys
import time

from playwright.sync_api import sync_playwright

URL = os.environ["CCTOP_URL"]
OUT = os.environ["CCTOP_OUT"]
BROWSER = os.environ["CCTOP_BROWSER"]
DEAD = os.environ.get("CCTOP_DEAD") == "1"

# Verbatim in shape from what the edge returns: an HTML error page, served with
# an HTML content type, far longer than any message cctop would send.
CLOUDFLARE_502 = (
    "<!DOCTYPE html><html><head><title>trycloudflare.com | 502: Bad gateway"
    "</title></head><body><h1>Bad gateway</h1><span>Error code 502</span>"
    + "<div>Visit cloudflare.com for more information.</div>" * 80
    + "</body></html>"
)


def main() -> int:
    with sync_playwright() as p:
        browser = p.chromium.launch(executable_path=BROWSER)
        page = browser.new_page(viewport={"width": 1100, "height": 1000})
        problems: list[str] = []
        page.on("pageerror", lambda e: problems.append(f"page error: {e}"))
        page.on(
            "console",
            lambda m: problems.append(f"console {m.type}: {m.text}")
            if m.type == "error"
            else None,
        )
        if DEAD:
            page.route(
                "**/api/**",
                lambda route: route.fulfill(
                    status=502,
                    content_type="text/html; charset=UTF-8",
                    body=CLOUDFLARE_502,
                ),
            )
        # The document's own status, kept so a run that renders nothing can
        # say whether the server even sent a page — a bare 404 body renders
        # no element the selector list knows, and without this the failure
        # is indistinguishable from a render that silently died.
        document_status: list[int] = []
        page.on(
            "response",
            lambda r: document_status.append(r.status)
            if r.request.resource_type == "document" and r.url == URL
            else None,
        )
        page.goto(URL)
        # The pages build themselves from a fetch, so "loaded" is when something
        # other than the placeholder is on screen. Either outcome is a real
        # answer — an error banner is what the dead-tunnel run is here to see.
        # `.card` is the fallback for a state the other selectors do not name —
        # a session with no transcript renders a card explaining that, and it
        # is still a page that worked.
        #
        # Polled in Python rather than `wait_for_selector`: the injected waiter
        # has starved to timeout on a live dashboard — SSE-driven DOM churn —
        # while `query_selector_all` at the same moment returned seven visible
        # rows. Protocol queries see the truth; the page-side poll does not.
        deadline = time.time() + 15
        rendered = False
        while not rendered and time.time() < deadline:
            rendered = any(
                e.is_visible()
                for e in page.query_selector_all(
                    ".turn, .row, tbody tr, .empty, .banner, .card"
                )
            )
            if not rendered:
                time.sleep(0.25)
        if not rendered:
            status = document_status[-1] if document_status else "no response"
            problems.append(
                f"nothing rendered within 15s (document answered {status})"
            )
            # What the page actually holds, so a blank-looking failure can be
            # told from content the selector simply does not name.
            try:
                problems.append(
                    "body classes at timeout: "
                    + page.evaluate(
                        "() => Array.from(document.body.querySelectorAll('*'))"
                        ".slice(0, 400)"
                        ".map(e => e.className).filter(Boolean)"
                        ".slice(0, 40).join(' | ')"
                    )
                )
            except Exception as e:
                problems.append(f"could not inspect the body either: {e}")
        page.screenshot(path=f"{OUT}.png", full_page=True)
        text = page.inner_text("body")
        with open(f"{OUT}.txt", "w") as fh:
            fh.write(text)
        browser.close()

    print(text[:2000])
    print(f"\n[{OUT}.png · {OUT}.txt]")
    for problem in problems:
        print(f"  ! {problem}", file=sys.stderr)
    # A page that threw is a failing run even when it drew something: an
    # exception in the render path is exactly what a screenshot hides.
    return 1 if any(p.startswith("page error") for p in problems) else 0


if __name__ == "__main__":
    raise SystemExit(main())
