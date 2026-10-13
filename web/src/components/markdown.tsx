import { memo } from "react";

// What an agent writes is markdown, so it is shown as markdown — rendered by
// the server (`crates/serve/src/markdown.rs`), which sends each turn's text
// beside it as `html`. That HTML is safe to set as markup by construction: raw
// HTML in a transcript arrives escaped, images arrive as links, and a link is
// kept only when it goes somewhere a browser should (http, https, mailto, or
// relative). The page carries no markdown parser of its own.
//
// A turn without `html` — from a cctop too old to send it — is shown as the
// text it is, line breaks kept.
export const Markdown = memo(function Markdown({ html, text }: { html?: string | null; text: string }) {
  if (html == null) return <div className="md whitespace-pre-wrap">{text}</div>;
  return <div className="md" dangerouslySetInnerHTML={{ __html: html }} />;
});
