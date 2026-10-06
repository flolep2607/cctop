import { memo } from "react";
import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";

// What an agent writes is markdown, so it is shown as markdown. react-markdown
// builds elements, never HTML strings, and drops raw HTML — a transcript is
// somebody else's text, including whatever a tool printed into it. Links go
// only where a browser should go; anything else is left as its text.
const safeUrl = (url: string) => (/^(https?:|mailto:)/i.test(url) ? defaultUrlTransform(url) : "");

export const Markdown = memo(function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        urlTransform={safeUrl}
        components={{
          a: ({ href, children }) =>
            href ? (
              <a href={href} target="_blank" rel="noopener noreferrer">
                {children}
              </a>
            ) : (
              <span>{children}</span>
            ),
          table: ({ children }) => (
            <div className="overflow-x-auto">
              <table>{children}</table>
            </div>
          ),
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
});
