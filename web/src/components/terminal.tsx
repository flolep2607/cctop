import { memo, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { getJson } from "@/lib/api";
import { withToken } from "@/lib/config";

// A session's terminal, drawn inside the page. rmux's terminal app is served
// from this origin at /term/ precisely so it can be framed; a share link
// carries everything in its fragment, which a browser never sends to any
// server, so the same fragment on our copy opens the same terminal.
//
// Moving the frame in the DOM would reload it — dropping the socket and the
// agent's screen — so callers keep it in place; re-rendering it is harmless.

export type Terminal = { url: string; tunnelled: boolean; name?: string };

/** The /term/ address a share link's fragment opens on this origin. */
export const termSrc = (url: string) => {
  const at = url.indexOf("#");
  return withToken("/term/") + (at >= 0 ? url.slice(at) : "");
};

// How long after the last touch of the terminal the next one counts as coming
// back to it, and how long the nudge holds the frame narrower.
const QUIET_MS = 1500;
const NUDGE_MS = 220;

// The grid the frame's xterm is showing, read off its DOM: rows from the row
// elements, columns from the screen's width over one character's. `null` when
// the renderer draws without those elements, and then no nudge is attempted.
function grid(frame: HTMLIFrameElement): { cols: number; rows: number } | null {
  try {
    const doc = frame.contentDocument;
    const screen = doc?.querySelector<HTMLElement>(".xterm-screen");
    const rows = doc?.querySelector(".xterm-rows")?.children.length;
    const measure = doc?.querySelector<HTMLElement>(".xterm-char-measure-element");
    if (!screen || !rows || !measure?.textContent?.length) return null;
    const cell = measure.offsetWidth / measure.textContent.length;
    return cell > 0 ? { cols: Math.round(screen.offsetWidth / cell), rows } : null;
  } catch {
    return null;
  }
}

/**
 * The terminal frame, which also takes the rmux window back when you use it.
 *
 * rmux fits a window to one client — the TUI's or this browser's — and its
 * `latest` setting only moves on attach or resize, never on input. So on the
 * first key, click, scroll or focus after a pause, the frame asks the server
 * how big the window is; if that is not this frame's grid, it narrows itself
 * for a moment and back, and xterm's resize is what rmux listens to.
 */
export const TerminalFrame = memo(function TerminalFrame({ url, title, name }: { url: string; title: string; name?: string }) {
  const ref = useRef<HTMLIFrameElement>(null);
  const [narrow, setNarrow] = useState(false);
  useEffect(() => {
    const frame = ref.current;
    if (!frame || !name) return;
    let last = 0;
    let checking = false;
    const touched = async () => {
      const now = Date.now();
      const back = now - last >= QUIET_MS;
      last = now;
      if (!back || checking) return;
      checking = true;
      try {
        const own = grid(frame);
        if (!own) return;
        const win = await getJson<{ cols: number; rows: number }>("/api/window/" + encodeURIComponent(name));
        if (win.cols === own.cols && win.rows === own.rows) return;
        setNarrow(true);
        setTimeout(() => setNarrow(false), NUDGE_MS);
      } catch {
        /* an older cctop, or a tab that just closed: nothing to take back */
      } finally {
        checking = false;
      }
    };
    // Same-origin, so the frame's own events are ours to hear. Re-wired on
    // every load, since a reconnect replaces the frame's document.
    const wire = () => {
      const win = frame.contentWindow;
      if (!win) return;
      for (const kind of ["keydown", "pointerdown", "wheel", "focus"]) win.addEventListener(kind, touched, { capture: true, passive: true });
    };
    frame.addEventListener("load", wire);
    wire();
    return () => frame.removeEventListener("load", wire);
  }, [name, url]);
  return (
    <iframe
      ref={ref}
      className="bg-terminal block h-full min-h-0 flex-1 border-0"
      style={{ width: narrow ? "calc(100% - 24px)" : "100%" }}
      title={title}
      allow="clipboard-read; clipboard-write"
      src={termSrc(url)}
    />
  );
});

/**
 * The page a popped-out terminal lives in: the frame, filling the window. The
 * share link rides in the fragment, as it does for /term/, so it never reaches
 * a server log; the rmux session name is the path.
 */
export function TerminalWindow({ name }: { name: string }) {
  const [hash] = useState(() => location.hash);
  useEffect(() => {
    document.title = name.replace(/^cctop-/, "") + " — cctop";
  }, [name]);
  if (!hash) return <div className="m-auto p-10 text-sm text-neutral-400">This window has no terminal link. Open it from cctop's pop-out button.</div>;
  return (
    <div className="bg-terminal flex h-dvh">
      <TerminalFrame url={hash} name={name} title={"Terminal — " + name} />
    </div>
  );
}

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
