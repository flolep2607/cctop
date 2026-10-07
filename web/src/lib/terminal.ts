import { toast } from "sonner";
import { withToken } from "@/lib/config";

// The terminal that is not drawn inside the page: what a share is, and the
// window a terminal pops out into. `components/terminal.tsx` draws the frame.

export type Terminal = { url: string; tunnelled: boolean; name?: string };

/**
 * Open a terminal in a window of its own, handing it over from the page.
 *
 * A share link admits one browser at a time, so opening the page's link a
 * second time is refused ("connection refused"). Instead the page lets go of
 * its frame and the window gets a share of its own, minted by `mint`. The
 * window is opened inside the click — a popup opened after an await is
 * blocked — and pointed at the terminal once the share exists. `onClosed`
 * fires when the window is closed, so the page can take the terminal back.
 */
export function popOut({ key, title, release, mint, onClosed }: {
  key: string; title: string; release: () => void; mint: () => Promise<Terminal>; onClosed: () => void;
}): boolean {
  const w = window.open("", "cctop-term-" + key, "popup=yes,width=1100,height=720");
  if (!w) {
    toast.error("The browser blocked the window — allow pop-ups for this page, or use its own tab's link.");
    return false;
  }
  try {
    w.document.title = title;
    w.document.body.style.cssText = "margin:0;background:#060807;color:#999;font:14px system-ui;display:grid;place-items:center;height:100vh";
    w.document.body.textContent = "Opening the terminal…";
  } catch {
    /* a window already showing the terminal: it is about to be repointed */
  }
  release();
  mint().then(
    (t) => {
      // cctop's own full-window page around the terminal rather than the bare
      // terminal, so the window takes the rmux window back when it is used,
      // as the frame in the page does.
      const at = t.url.indexOf("#");
      if (!w.closed) w.location.href = withToken("/window/" + encodeURIComponent(t.name ?? key)) + (at >= 0 ? t.url.slice(at) : "");
    },
    (e) => {
      if (!w.closed) w.document.body.textContent = String(e?.message || e);
    },
  );
  const poll = setInterval(() => {
    if (w.closed) {
      clearInterval(poll);
      onClosed();
    }
  }, 800);
  return true;
}
