import { memo } from "react";
import { withToken } from "@/lib/config";

// A session's terminal, drawn inside the page. rmux's terminal app is served
// from this origin at /term/ precisely so it can be framed; a share link
// carries everything in its fragment, which a browser never sends to any
// server, so the same fragment on our copy opens the same terminal.
//
// Memoised on the url alone: re-rendering it would be harmless, but moving it
// in the DOM is not — a moved iframe reloads, dropping the socket and the
// agent's screen. Callers keep it in place.
export const TerminalFrame = memo(function TerminalFrame({ url, title }: { url: string; title: string }) {
  const at = url.indexOf("#");
  return (
    <iframe
      className="bg-terminal block size-full min-h-0 flex-1 border-0"
      title={title}
      allow="clipboard-read; clipboard-write"
      src={withToken("/term/") + (at >= 0 ? url.slice(at) : "")}
    />
  );
});
