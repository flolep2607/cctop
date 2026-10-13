//! Markdown to HTML, for the conversation page.
//!
//! An agent writes markdown, so its replies are shown as markdown. They used to
//! be rendered in the browser by react-markdown and remark-gfm, which were the
//! largest thing in the page's bundle after React itself; the server already
//! has a markdown parser (`pulldown-cmark`, which the TUI reads replies with),
//! so the page is now sent the HTML and the bundle carries no parser at all.
//!
//! The HTML is set as markup by the page, so everything that goes into it is
//! somebody else's text: a transcript holds whatever a tool printed and
//! whatever a fetched web page said. What keeps that safe is done here, on the
//! event stream, before any HTML is written:
//!
//! - **Raw HTML is text.** A block or inline HTML event is turned into a text
//!   event, which the writer escapes — so `<script>` reads as `<script>` and
//!   never runs. react-markdown dropped it instead; showing it is kinder to a
//!   reply that was talking *about* a tag.
//! - **Links go only where a browser should go**: `http:`, `https:`,
//!   `mailto:`, or a relative URL — the allowlist react-markdown's
//!   `defaultUrlTransform` used. Any other link keeps its text and loses the
//!   link. A browser strips tabs and newlines from a scheme before reading it,
//!   but an allowlist does not care: `java\nscript:` is not on it either.
//! - **No images.** An image becomes a link to it with its alt text, so there
//!   is no element for an `onerror` to hang on, and nothing is fetched from
//!   anywhere — the page's content policy would refuse the fetch anyway.
//!
//! Every link opens in a new tab with `noopener noreferrer`, and a table is
//! wrapped so a wide one scrolls rather than widening the page.

use pulldown_cmark::{CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};

/// The GFM a reply is written in: tables, strikethrough and task lists. Not
/// footnotes, whose in-page anchors would open in a new tab like every other
/// link; not math; not smart punctuation — a reply's quotes are what the
/// agent typed.
fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS
}

/// Whether a link to `url` may be followed from the page.
///
/// Relative when no `:` comes before the first `/`, `?` or `#` — the rule
/// `defaultUrlTransform` uses, so `a/b:c` is a path and `x:y` is a scheme.
pub fn safe_url(url: &str) -> bool {
    let Some(colon) = url.find(':') else {
        return true;
    };
    let before = &url[..colon];
    if before.contains(['/', '?', '#']) {
        return true;
    }
    ["http", "https", "mailto"]
        .iter()
        .any(|scheme| before.eq_ignore_ascii_case(scheme))
}

/// `text` as HTML that is safe to set as the page's markup.
pub fn to_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    // Whether each open link or image was kept, so its end can be dropped with
    // it. A stack, because an image can sit inside a link.
    let mut kept: Vec<bool> = Vec::new();
    // Inside a link or a code block, a bare URL is not linked again.
    let mut in_code = 0usize;
    let mut events: Vec<Event> = Vec::new();
    // Text waiting to be linked. The parser splits a run of text at every
    // character that might have been markup — the `_` in `a_(b)` — so a URL
    // can span several text events, and is only found once they are joined.
    let mut pending = String::new();
    for event in Parser::new_ext(text, options()) {
        if let Event::Text(t) = &event
            && kept.is_empty()
            && in_code == 0
        {
            pending.push_str(t);
            continue;
        }
        if !pending.is_empty() {
            linkify(CowStr::from(std::mem::take(&mut pending)), &mut events);
        }
        match event {
            Event::Html(raw) | Event::InlineHtml(raw) => events.push(Event::Text(raw)),
            // A block of raw HTML is shown as the paragraph of text it now is.
            Event::Start(Tag::HtmlBlock) => events.push(Event::Start(Tag::Paragraph)),
            Event::End(TagEnd::HtmlBlock) => events.push(Event::End(TagEnd::Paragraph)),
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let keep = link_type == LinkType::Email || safe_url(&dest_url);
                kept.push(keep);
                if keep {
                    events.push(Event::Start(Tag::Link {
                        link_type,
                        dest_url,
                        title,
                        id,
                    }));
                }
            }
            Event::Start(Tag::Image {
                dest_url,
                title,
                id,
                ..
            }) => {
                let keep = safe_url(&dest_url);
                kept.push(keep);
                if keep {
                    events.push(Event::Start(Tag::Link {
                        link_type: LinkType::Inline,
                        dest_url,
                        title,
                        id,
                    }));
                }
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                if kept.pop().unwrap_or(false) {
                    events.push(Event::End(TagEnd::Link));
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code += 1;
                events.push(Event::Start(Tag::CodeBlock(kind)));
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code = in_code.saturating_sub(1);
                events.push(Event::End(TagEnd::CodeBlock));
            }
            other => events.push(other),
        }
    }
    if !pending.is_empty() {
        linkify(CowStr::from(pending), &mut events);
    }
    pulldown_cmark::html::push_html(&mut out, events.into_iter());
    // The writer has no hook for attributes, and raw HTML never reaches it, so
    // every `<a href=` and `<table>` in its output is one it wrote.
    out.replace(
        "<a href=\"",
        "<a target=\"_blank\" rel=\"noopener noreferrer\" href=\"",
    )
    .replace("<table>", "<div class=\"md-table\"><table>")
    .replace("</table>", "</table></div>")
}

/// `text` with every bare `http://` or `https://` URL in it made a link, the
/// way GFM's autolink extension does and pulldown-cmark does not.
///
/// The URL runs to the next space or `<`; trailing punctuation that ends a
/// sentence is left out, and so is a `)` that closes a parenthesis opened
/// before the URL rather than inside it.
fn linkify<'a>(text: CowStr<'a>, events: &mut Vec<Event<'a>>) {
    let s: &str = &text;
    let mut rest = 0;
    let mut out: Vec<Event<'a>> = Vec::new();
    let mut at = 0;
    while let Some(found) = next_url(&s[at..]) {
        let start = at + found;
        let mut end = start
            + s[start..]
                .find(|c: char| c.is_whitespace() || c == '<')
                .unwrap_or(s.len() - start);
        loop {
            let url = &s[start..end];
            match url.chars().last() {
                Some('.' | ',' | ':' | ';' | '!' | '?' | '"' | '\'' | '*' | '_' | '~') => end -= 1,
                Some(')') if url.matches('(').count() < url.matches(')').count() => end -= 1,
                _ => break,
            }
        }
        let url = &s[start..end];
        // A scheme alone is not a link.
        if !url.ends_with("//") {
            if start > rest {
                out.push(Event::Text(CowStr::from(s[rest..start].to_string())));
            }
            out.push(Event::Start(Tag::Link {
                link_type: LinkType::Autolink,
                dest_url: CowStr::from(url.to_string()),
                title: CowStr::from(""),
                id: CowStr::from(""),
            }));
            out.push(Event::Text(CowStr::from(url.to_string())));
            out.push(Event::End(TagEnd::Link));
            rest = end;
        }
        at = end.max(start + 1);
    }
    if out.is_empty() {
        return events.push(Event::Text(text));
    }
    if rest < s.len() {
        out.push(Event::Text(CowStr::from(s[rest..].to_string())));
    }
    events.extend(out);
}

/// Where the next `http://` or `https://` starts in `s`, at a word boundary —
/// `xhttp://` is not a URL.
fn next_url(s: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = s[from..].find("http") {
        let i = from + i;
        let tail = &s[i..];
        let boundary = s[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        if boundary && (tail.starts_with("http://") || tail.starts_with("https://")) {
            return Some(i);
        }
        from = i + 4;
    }
    None
}

/// Fill the `html` of every turn, and of every subagent's report, in
/// `conversation` — what the conversation route does before it answers.
pub fn render(conversation: &mut cctop_core::chat::Conversation) {
    for turn in &mut conversation.turns {
        if !turn.text.is_empty() {
            turn.html = Some(to_html(&turn.text));
        }
        for tool in &mut turn.tools {
            if let Some(agent) = tool.agent.as_mut()
                && let Some(report) = agent.report.as_deref()
            {
                agent.report_html = Some(to_html(report));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gfm_renders() {
        let html = to_html("| a | b |\n|---|---|\n| 1 | 2 |\n\n~~gone~~\n\n- [x] done\n- [ ] not");
        assert!(html.contains("<div class=\"md-table\"><table>"), "{html}");
        assert!(html.contains("</table></div>"), "{html}");
        assert!(html.contains("<del>gone</del>"), "{html}");
        assert!(html.contains("type=\"checkbox\""), "{html}");
    }

    #[test]
    fn code_is_escaped_and_kept() {
        let html = to_html("```rust\nlet a = \"<b>\";\n```\n\nand `<i>`");
        assert!(html.contains("class=\"language-rust\""), "{html}");
        assert!(html.contains("&lt;b&gt;"), "{html}");
        assert!(html.contains("<code>&lt;i&gt;</code>"), "{html}");
    }

    #[test]
    fn a_script_tag_is_text() {
        for input in [
            "<script>alert(1)</script>",
            "hello <script>alert(1)</script> there",
            "<div>\n<script>alert(1)</script>\n</div>",
        ] {
            let html = to_html(input);
            assert!(!html.contains("<script"), "{input:?} -> {html}");
            assert!(html.contains("&lt;script&gt;"), "{input:?} -> {html}");
        }
    }

    #[test]
    fn a_raw_html_block_is_text() {
        let html = to_html("<div onclick=\"x()\">\n<iframe src=\"https://evil\"></iframe>\n</div>");
        assert!(!html.contains("<div onclick"), "{html}");
        assert!(!html.contains("<iframe"), "{html}");
        assert!(html.contains("&lt;iframe"), "{html}");
    }

    /// Every tag in `html`. Text never holds a raw `<` — the writer escapes it —
    /// so whatever lies between a `<` and the next `>` is markup.
    fn tags(html: &str) -> Vec<&str> {
        html.split('<')
            .skip(1)
            .filter_map(|s| s.split_once('>').map(|(tag, _)| tag))
            .collect()
    }

    #[test]
    fn an_image_is_never_an_img() {
        for input in [
            "![x](https://example.com/a.png)",
            "<img src=x onerror=alert(1)>",
            "![x\" onerror=\"alert(1)](x)",
            "[![inner](https://a/b.png)](https://example.com)",
        ] {
            let html = to_html(input);
            assert!(!html.contains("<img"), "{input:?} -> {html}");
            // As text it may say `onerror`; inside a tag it may not.
            assert!(
                tags(&html).iter().all(|t| !t.contains("onerror")),
                "{input:?} -> {html}"
            );
        }
        let html = to_html("![a picture](https://example.com/a.png)");
        assert!(
            html.contains("href=\"https://example.com/a.png\""),
            "{html}"
        );
        assert!(html.contains(">a picture</a>"), "{html}");
    }

    #[test]
    fn only_safe_links_are_links() {
        for input in [
            "[click](javascript:alert(1))",
            "[click](JavaScript:alert(1))",
            "[click](&#106;avascript:alert(1))",
            "[click](java\tscript:alert(1))",
            "[click](data:text/html;base64,PHNjcmlwdD4=)",
            "[click](vbscript:msgbox(1))",
            "<javascript:alert(1)>",
            "[click]( javascript:alert(1))",
            "[ref]\n\n[ref]: javascript:alert(1)",
            "![x](javascript:alert(1))",
        ] {
            let html = to_html(input);
            assert!(!html.contains("<a "), "{input:?} -> {html}");
            assert!(
                !html.to_lowercase().contains("href=\"javascript"),
                "{input:?} -> {html}"
            );
        }
        for (input, href) in [
            (
                "[a](https://example.com/x?y=1&z=2)",
                "https://example.com/x?y=1&amp;z=2",
            ),
            ("[a](http://example.com)", "http://example.com"),
            ("[a](mailto:me@example.com)", "mailto:me@example.com"),
            ("[a](/session/abc)", "/session/abc"),
            ("[a](docs/x:y.md)", "docs/x:y.md"),
            ("<me@example.com>", "mailto:me@example.com"),
        ] {
            let html = to_html(input);
            assert!(
                html.contains(&format!(
                    "<a target=\"_blank\" rel=\"noopener noreferrer\" href=\"{href}\""
                )),
                "{input:?} -> {html}"
            );
        }
    }

    #[test]
    fn an_attribute_cannot_be_broken_out_of() {
        let html = to_html("[a](https://example.com/\"onmouseover=\"alert(1))");
        assert!(!html.contains("\"onmouseover"), "{html}");
        let html = to_html("[a](https://example.com \"t\\\" onmouseover=\\\"x\")");
        assert!(!html.contains("\" onmouseover"), "{html}");
    }

    #[test]
    fn bare_urls_are_links() {
        let html = to_html("see https://example.com/a_(b). And (http://x.org/y).");
        assert!(
            html.contains("href=\"https://example.com/a_(b)\""),
            "{html}"
        );
        assert!(html.contains("href=\"http://x.org/y\""), "{html}");
        assert!(html.contains("</a>. And ("), "{html}");
        // Not inside code, and not twice inside a link.
        let html = to_html("`https://example.com` and [https://a.org](https://a.org)");
        assert_eq!(html.matches("<a ").count(), 1, "{html}");
        // Not a word that merely contains one.
        assert!(!to_html("xhttps://a.org").contains("<a "));
    }

    #[test]
    fn render_fills_turns_and_reports() {
        let mut conversation: cctop_core::chat::Conversation = serde_json::from_value(
            serde_json::json!({
                "supported": true,
                "earlier": 0,
                "turns": [
                    {"seq": 0, "role": "assistant", "kind": "message", "ts": "", "text": "**hi**",
                     "tools": [{"name": "Agent", "detail": "", "agent": {
                        "id": "a", "type": "t", "description": "", "status": "done", "report": "_done_"}}]},
                    {"seq": 1, "role": "assistant", "kind": "message", "ts": "", "text": "",
                     "tools": [{"name": "Bash", "detail": "ls"}]}
                ]
            }),
        )
        .unwrap();
        render(&mut conversation);
        let turns = &conversation.turns;
        assert_eq!(
            turns[0].html.as_deref(),
            Some("<p><strong>hi</strong></p>\n")
        );
        let agent = turns[0].tools[0].agent.as_ref().unwrap();
        assert_eq!(agent.report_html.as_deref(), Some("<p><em>done</em></p>\n"));
        assert_eq!(turns[1].html, None);
    }
}
