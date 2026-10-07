import { memo, useEffect, useRef, useState } from "react";
import { getJson } from "@/lib/api";
import { withToken } from "@/lib/config";

// A session's terminal, drawn inside the page. rmux's terminal app is served
// from this origin at /term/ precisely so it can be framed; a share link
// carries everything in its fragment, which a browser never sends to any
// server, so the same fragment on our copy opens the same terminal.
//
// Moving the frame in the DOM would reload it — dropping the socket and the
// agent's screen — so callers keep it in place; re-rendering it is harmless.

/** The /term/ address a share link's fragment opens on this origin. */
const termSrc = (url: string) => {
  const at = url.indexOf("#");
  return withToken("/term/") + (at >= 0 ? url.slice(at) : "");
};

// How long after the last touch of the terminal the next one counts as coming
// back to it, how long the nudge holds the frame narrower, and how long after
// a nudge to look whether it took.
const QUIET_MS = 1500;
const NUDGE_MS = 220;
const VERIFY_MS = 1200;

type WindowInfo = { cols: number; rows: number; web?: [number, number][] };

// Whether the window is already a browser's: rmux lists each browser client
// with its exact size, so this is read from rmux, not measured off the frame.
// Any browser counts — two frames on one agent must not take it in turns.
const heldByBrowser = (w: WindowInfo) => (w.web ?? []).some(([c, r]) => c === w.cols && r === w.rows);

/**
 * The terminal frame, which also takes the rmux window back when you use it.
 *
 * rmux fits a window to one client — the TUI's or a browser's — and its
 * `latest` setting only moves on attach or resize, never on input. So on the
 * first key, click or scroll after a pause, the frame asks the server for the
 * window's size and the browsers' sizes. Only if no browser holds the window
 * does it narrow itself for a moment and back: xterm's resize is what rmux
 * listens to.
 *
 * And it gives up rather than loop. If the window still is not a browser's a
 * moment after a nudge — something else keeps it, or this browser cannot be
 * found among the clients — no further nudge is tried until the frame's own
 * size changes. A nudge that does not work, repeated, is a terminal that
 * reflows on every keystroke, which is worse than one at the wrong size.
 */
export const TerminalFrame = memo(function TerminalFrame({ url, title, name, onBroken }: {
  url: string; title: string; name?: string;
  /** The frame says its link would not connect; the caller mints a fresh one. */
  onBroken?: () => void;
}) {
  const ref = useRef<HTMLIFrameElement>(null);
  const [narrow, setNarrow] = useState(false);
  // rmux's app says so in its own words when its socket is refused or lost —
  // an expired share, a dropped tunnel — and offers only "try refreshing",
  // which reloads the same dead link. Reported once per link instead, so the
  // caller can mint a fresh share rather than leave a dead frame up.
  // The latest callback, so a parent re-rendering with a new closure does not
  // restart the watch — and with it the once-per-link promise.
  const broken = useRef(onBroken);
  useEffect(() => {
    broken.current = onBroken;
  });
  useEffect(() => {
    let told = false;
    const look = setInterval(() => {
      try {
        const text = ref.current?.contentDocument?.body?.innerText ?? "";
        if (!told && broken.current && /connection refused|lost connection|could not connect|handshake/i.test(text)) {
          told = true;
          broken.current();
        }
      } catch {
        /* a frame mid-navigation has no document to read */
      }
    }, 1500);
    return () => clearInterval(look);
  }, [url]);
  useEffect(() => {
    const frame = ref.current;
    if (!frame || !name) return;
    let last = 0;
    let busy = false;
    let gaveUp = false;
    let gone = false;
    const ask = () => getJson<WindowInfo>("/api/window/" + encodeURIComponent(name));
    const touched = async () => {
      const now = Date.now();
      const back = now - last >= QUIET_MS;
      last = now;
      if (!back || busy || gaveUp) return;
      busy = true;
      try {
        const w = await ask();
        if (!(w.web ?? []).length || heldByBrowser(w)) return;
        setNarrow(true);
        await new Promise((done) => setTimeout(done, NUDGE_MS));
        if (gone) return;
        setNarrow(false);
        await new Promise((done) => setTimeout(done, VERIFY_MS));
        if (gone) return;
        if (!heldByBrowser(await ask())) gaveUp = true;
      } catch {
        /* an older cctop, or a tab that just closed: nothing to take back */
      } finally {
        busy = false;
      }
    };
    // A different size is a new situation: the give-up applied to the old one.
    // Ignores the nudge's own resizes, which come and go while `busy`.
    const sized = new ResizeObserver(() => {
      if (!busy) gaveUp = false;
    });
    sized.observe(frame);
    // Same-origin, so the frame's own events are ours to hear. Re-wired on
    // every load, since a reconnect replaces the frame's document. Not
    // `focus`: it fires on every switch of window and on xterm's own
    // refocusing, which is not someone starting to use this terminal.
    const wire = () => {
      const win = frame.contentWindow;
      if (!win) return;
      for (const kind of ["keydown", "pointerdown", "wheel"]) win.addEventListener(kind, touched, { capture: true, passive: true });
    };
    frame.addEventListener("load", wire);
    wire();
    return () => {
      gone = true;
      sized.disconnect();
      frame.removeEventListener("load", wire);
    };
  }, [name, url]);
  return (
    <iframe
      ref={ref}
      className="bg-terminal block h-full min-h-0 flex-1 border-0"
      // The frame grows to fill a flex row, where a width alone changes
      // nothing; the nudge has to take the growing away too.
      style={narrow ? { flex: "0 0 calc(100% - 24px)", width: "calc(100% - 24px)" } : { width: "100%" }}
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
