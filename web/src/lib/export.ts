import { toast } from "sonner";
import { ask } from "./api";
import { copyText } from "./format";

/**
 * The markdown documents `/api/chat/<id>/markdown` serves (`serve::export`):
 * the conversation, the conversation with each tool result, or the handoff
 * brief — the summary written for another agent, which is the one that fits
 * when a session is long.
 */
export type Variant = "conversation" | "tools" | "brief";

const NOUN: Record<Variant, string> = {
  conversation: "the conversation",
  tools: "the conversation with tool output",
  brief: "the handoff brief",
};

async function fetchMarkdown(id: string, variant: Variant): Promise<string> {
  const response = await ask("/api/chat/" + encodeURIComponent(id) + "/markdown", undefined, { variant });
  return response.text();
}

// The server files every turn under one `## ` heading and quotes everything
// said inside one, so a line that starts with `## ` is a turn and never a
// heading someone typed. The brief's sections are not turns, so it has none.
function turnsIn(markdown: string): number {
  return (markdown.match(/^## /gm) ?? []).length;
}

function size(markdown: string): string {
  const bytes = new Blob([markdown]).size;
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return Math.round(bytes / 1024) + " KB";
  return (bytes / 1024 / 1024).toFixed(1) + " MB";
}

function saveFile(markdown: string, filename: string) {
  const url = URL.createObjectURL(new Blob([markdown], { type: "text/markdown;charset=utf-8" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  // Revoked on the next turn rather than now: some browsers start the
  // download after `click` returns, from the URL as it stands then.
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

function filename(id: string, variant: Variant): string {
  return "cctop-" + id.slice(0, 8) + (variant === "brief" ? "-brief" : "") + ".md";
}

function done(verb: "Copied" | "Saved", markdown: string, variant: Variant): string {
  const turns = turnsIn(markdown);
  const what = variant === "brief" ? "the handoff brief" : turns + (turns === 1 ? " turn" : " turns");
  return verb + " " + what + " (" + size(markdown) + ")";
}

/** Fetch one markdown variant and save it as a file. */
export async function downloadMarkdown(id: string, variant: Variant): Promise<void> {
  try {
    const markdown = await fetchMarkdown(id, variant);
    saveFile(markdown, filename(id, variant));
    toast.success(done("Saved", markdown, variant));
  } catch (e) {
    toast.error("Could not export " + NOUN[variant] + ": " + String((e as Error)?.message || e));
  }
}

/**
 * Fetch one markdown variant and put it on the clipboard.
 *
 * The clipboard is the fragile half. A copy is only allowed close to the click
 * that asked for it, and the fetch in between can outlast that on a long
 * session; on plain http — a LAN address, which is how cctop is usually
 * opened — `navigator.clipboard` does not exist at all and the textarea
 * fallback is all there is. Where `ClipboardItem` takes a promise it is handed
 * one, which keeps the copy inside the click however long the fetch takes.
 * Whatever still fails is answered with the same document as a download,
 * which needs no permission at all.
 */
export async function copyMarkdown(id: string, variant: Variant): Promise<void> {
  const pending = fetchMarkdown(id, variant);
  let markdown: string;
  try {
    markdown = await viaClipboardItem(pending);
  } catch {
    try {
      markdown = await pending;
    } catch (e) {
      toast.error("Could not export " + NOUN[variant] + ": " + String((e as Error)?.message || e));
      return;
    }
    try {
      await copyText(markdown);
    } catch {
      const text = markdown;
      toast.error("This browser would not copy " + NOUN[variant] + " (" + size(text) + ").", {
        description: "Clipboard access needs https or localhost. Download it as a file instead.",
        action: { label: "Download .md", onClick: () => saveFile(text, filename(id, variant)) },
        duration: 15000,
      });
      return;
    }
  }
  toast.success(done("Copied", markdown, variant));
}

async function viaClipboardItem(pending: Promise<string>): Promise<string> {
  if (!window.isSecureContext || !navigator.clipboard?.write || typeof ClipboardItem === "undefined") {
    throw new Error("no async clipboard");
  }
  const blob = pending.then((text) => new Blob([text], { type: "text/plain" }));
  await navigator.clipboard.write([new ClipboardItem({ "text/plain": blob })]);
  return pending;
}
