import { useCallback, useEffect, useState } from "react";
import { getJson } from "@/lib/api";

type Extra = Record<string, string | number | null | undefined>;

export interface PollOptions {
  /** False holds the poll — nothing is fetched and nothing is kept. */
  enabled?: boolean;
  /**
   * Skip the reads while the tab is hidden, and read at once when it is shown
   * again. On by default: a background tab polling every few seconds is load
   * on the server and battery on a laptop for a page nobody is looking at.
   */
  visibleOnly?: boolean;
  /** Query parameters, beside the token `getJson` adds. */
  extra?: Extra;
  /**
   * Stop once one read has landed: a document fetched once, retried every
   * `everyMs` until it arrives — the session report, which is not live.
   */
  once?: boolean;
}

export interface Poll<T> {
  /** The last read, or null before the first one landed. Kept through a failed read. */
  data: T | null;
  /** Why the latest read failed, or "" once one succeeds. */
  error: string;
  /** When `data` arrived, by the browser's clock; 0 before it has. */
  at: number;
  /** Read now, without waiting for the next tick. */
  refresh: () => void;
}

/**
 * A JSON route, read every `everyMs`.
 *
 * The one loop the pages share, so each gets the same care: a read that lands
 * after the page moved on is dropped, a read is never sent while the previous
 * one is still out (a slow tunnel must not stack them), and a hidden tab
 * pauses. What it holds is keyed on the path and its query, so a page that
 * starts reading another session never shows the last one's answer as this
 * one's.
 */
export function usePoll<T>(path: string, everyMs: number, { enabled = true, visibleOnly = true, extra, once = false }: PollOptions = {}): Poll<T> {
  const query = JSON.stringify(extra ?? null);
  const key = path + "\n" + query;
  const [held, setHeld] = useState<{ key: string; data: T | null; error: string; at: number }>({ key: "", data: null, error: "", at: 0 });
  const [nudged, setNudged] = useState(0);

  useEffect(() => {
    if (!enabled) return;
    const args = JSON.parse(query) as Extra | null;
    let live = true;
    let out = false;
    let landed = false;
    const read = () => {
      if (out || (once && landed) || (visibleOnly && document.visibilityState !== "visible")) return;
      out = true;
      getJson<T>(path, args ?? undefined)
        .then(
          (data) => {
            if (!live) return;
            landed = true;
            setHeld({ key, data, error: "", at: Date.now() });
          },
          (e) => {
            if (!live) return;
            const error = String((e as Error).message || e);
            setHeld((h) => (h.key === key ? { ...h, error } : { key, data: null, error, at: 0 }));
          },
        )
        .finally(() => {
          out = false;
        });
    };
    const onVisible = () => {
      if (document.visibilityState === "visible") read();
    };
    read();
    const timer = setInterval(read, everyMs);
    if (visibleOnly) document.addEventListener("visibilitychange", onVisible);
    return () => {
      live = false;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [path, query, key, everyMs, enabled, visibleOnly, once, nudged]);

  const refresh = useCallback(() => setNudged((n) => n + 1), []);
  const mine = held.key === key && enabled;
  return { data: mine ? held.data : null, error: mine ? held.error : "", at: mine ? held.at : 0, refresh };
}
