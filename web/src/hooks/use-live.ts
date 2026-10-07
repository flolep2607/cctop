import { useEffect, useState } from "react";
import { withToken } from "@/lib/config";
import { getJson } from "@/lib/api";
import { applyRows, settleAsking, type RowsDelta } from "@/lib/stream";
import type { Session, Tab } from "@/lib/types";

/**
 * How long the stream may say nothing at all before it is taken for dead.
 *
 * The server pings every 15 seconds when it has nothing else to write
 * (`SSE_KEEPALIVE` in src/serve/mod.rs), so this is two of those and some
 * slack. EventSource never notices a stream that stalled without closing — a
 * half-open connection, a tunnel holding the bytes — so without this the page
 * sat on an old table indefinitely and called itself live.
 */
const STALE_AFTER = 35_000;

/** The event a page raises when the server has said `id` is not asking. */
const NOT_ASKING = "cctop:not-asking";

/**
 * Take `id`'s prompt down on every table this page holds, and resync them.
 *
 * For a 409 "not asking" from an answer: the server's table is current when it
 * says that, so the page is the one behind. The row is settled at once and the
 * stream reopened, which brings the server's whole table rather than waiting
 * for whatever event would have corrected it.
 */
export function dropPrompt(id: string) {
  window.dispatchEvent(new CustomEvent(NOT_ASKING, { detail: id }));
}

/**
 * The session table, live, over the same event stream every cctop page reads.
 * With `only`, the server sends that session's row alone, each time it moves.
 * Without, it sends the table once as `sessions` and then only the rows that
 * changed, as `rows` — see `applyRows`.
 *
 * Named events: `onmessage` only ever sees unnamed ones. A stream that was
 * never a stream (a tunnel's error page) closes for good rather than
 * reconnecting, so a closed source is reopened on a timer; and one that goes
 * quiet for longer than the server's pings allow is closed and reopened too.
 */
export function useSessions(only?: string): { sessions: Session[] | null; live: boolean } {
  const [sessions, setSessions] = useState<Session[] | null>(null);
  const [live, setLive] = useState(false);
  useEffect(() => {
    let source: EventSource | null = null;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let gone = false;
    let heard = Date.now();
    // Each source's listeners are its own: a stale one being closed must not
    // have a late event of its own land on the table its replacement keeps.
    const connect = () => {
      clearTimeout(retry);
      source?.close();
      heard = Date.now();
      // `rows: "patch"` asks for rows as the fields that changed, which a
      // page from before that shape existed would not know to apply.
      const mine = new EventSource(withToken("/api/events", only ? { session: only } : { rows: "patch" }));
      source = mine;
      const fresh = () => {
        if (source !== mine) return false;
        heard = Date.now();
        return true;
      };
      mine.addEventListener("sessions", (event) => {
        if (!fresh()) return;
        try {
          const list = JSON.parse((event as MessageEvent).data);
          setSessions(Array.isArray(list) ? list : []);
          setLive(true);
        } catch {
          /* a bad event is skipped, not fatal */
        }
      });
      mine.addEventListener("rows", (event) => {
        if (!fresh()) return;
        try {
          const delta = JSON.parse((event as MessageEvent).data) as RowsDelta;
          setSessions((list) => (list ? applyRows(list, delta) : list));
          setLive(true);
        } catch {
          /* a bad event is skipped, not fatal */
        }
      });
      mine.addEventListener("ping", () => {
        if (fresh()) setLive(true);
      });
      mine.addEventListener("open", () => {
        if (fresh()) setLive(true);
      });
      mine.addEventListener("error", () => {
        if (source !== mine) return;
        setLive(false);
        if (mine.readyState === EventSource.CLOSED && !gone) retry = setTimeout(connect, 5000);
      });
    };
    const watchdog = setInterval(() => {
      if (gone || Date.now() - heard < STALE_AFTER) return;
      setLive(false);
      connect();
    }, 5000);
    const onNotAsking = (event: Event) => {
      const id = (event as CustomEvent<string>).detail;
      setSessions((list) => (list ? settleAsking(list, id) : list));
      connect();
    };
    window.addEventListener(NOT_ASKING, onNotAsking);
    connect();
    return () => {
      gone = true;
      clearTimeout(retry);
      clearInterval(watchdog);
      window.removeEventListener(NOT_ASKING, onNotAsking);
      source?.close();
    };
  }, [only]);
  return { sessions, live };
}

/** The tabs cctop has open in its multiplexer, every few seconds. */
export function useTabs(every = 3000): { tabs: Tab[] | null; error: string } {
  const [tabs, setTabs] = useState<Tab[] | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let live = true;
    const poll = async () => {
      try {
        const body = await getJson<{ tabs?: Tab[] }>("/api/tabs");
        if (!live) return;
        setTabs(body.tabs ?? []);
        setError("");
      } catch (e) {
        if (live) setError(String((e as Error).message || e));
      }
    };
    poll();
    const timer = setInterval(poll, every);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [every]);
  return { tabs, error };
}

/**
 * Re-render on a slow clock, so "4m ago" stays honest between events.
 *
 * Returns the time of the latest tick, which is the "now" a render should
 * use: reading the clock during render makes the render impure — two renders
 * of the same state could draw different pages — where this one is state.
 */
export function useTick(ms = 15000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(t);
  }, [ms]);
  return now;
}

/** State kept in localStorage — per viewer, best effort, never required. */
export function useStored<T>(key: string, initial: T, valid?: (v: unknown) => v is T): [T, (v: T) => void] {
  const [value, setValue] = useState<T>(() => {
    try {
      const raw = localStorage.getItem(key);
      if (raw === null) return initial;
      const parsed = JSON.parse(raw);
      return valid && !valid(parsed) ? initial : (parsed as T);
    } catch {
      return initial;
    }
  });
  const set = (v: T) => {
    setValue(v);
    try {
      localStorage.setItem(key, JSON.stringify(v));
    } catch {
      /* no store, no memory */
    }
  };
  return [value, set];
}
