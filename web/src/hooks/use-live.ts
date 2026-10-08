import { useEffect, useState } from "react";
import { withToken } from "@/lib/config";
import { getJson } from "@/lib/api";
import { applyRows, settleAsking, type RowsDelta } from "@/lib/stream";
import type { Session, Tab } from "@/lib/types";

/**
 * How long the stream may say nothing at all before it is taken for dead.
 *
 * The server pings every 15 seconds when it has nothing else to write
 * (`SSE_KEEPALIVE` in crates/serve/src/lib.rs), so this is two of those and some
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
 * How long a newly opened stream has to say anything before the table is
 * fetched some other way. The server pings the moment a stream opens, so a
 * working one is heard from within a round trip; a Cloudflare quick tunnel,
 * which does not carry SSE, may hold those bytes indefinitely.
 */
const FIRST_EVENT_WITHIN = 5_000;

/**
 * How often the table is fetched while the stream is not getting through.
 * `/api/sessions` answers an unchanged table with a `304`, so a quiet table
 * costs a header each time; this is the delay a polled table can lag by.
 */
export const POLL_EVERY = 5_000;

/**
 * Where the table on screen comes from: the stream (`live`), a fetch every
 * few seconds because the stream is not getting through (`polling`), nothing
 * yet (`connecting`), or nothing any more (`down`).
 */
export type Feed = "connecting" | "live" | "polling" | "down";

/** The one row a session page asked for, as the stream would pick it. */
function pick(list: Session[], only: string): Session[] {
  const row = list.find((s) => s.session_id === only) ?? list.find((s) => s.session_id.startsWith(only));
  return row ? [row] : [];
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
 *
 * A stream that says nothing within `FIRST_EVENT_WITHIN` of opening, or that
 * errors, is not trusted to be coming: the table is polled from
 * `/api/sessions` instead, every `POLL_EVERY` while the page is visible, and
 * the stream keeps being retried underneath. The first event any stream
 * delivers stops the polling — the stream's own first frame is the whole
 * table, so nothing is lost in the handover. `open` alone does not count: a
 * proxy that holds the body can still pass the head.
 */
export function useSessions(only?: string): { sessions: Session[] | null; feed: Feed } {
  const [sessions, setSessions] = useState<Session[] | null>(null);
  const [feed, setFeed] = useState<Feed>("connecting");
  useEffect(() => {
    let source: EventSource | null = null;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let firstBy: ReturnType<typeof setTimeout> | undefined;
    let gone = false;
    let heard = Date.now();
    // Polling: its timer, the tag of the table last fetched, and a count that
    // marks which round of polling a reply belongs to — a reply that lands
    // after the stream came back must not put an older table on screen.
    let poller: ReturnType<typeof setInterval> | undefined;
    let round = 0;
    let etag = "";
    let asking = false;

    const poll = async () => {
      if (poller === undefined || asking || document.visibilityState !== "visible") return;
      const mine = round;
      asking = true;
      try {
        // `no-store` and the tag sent by hand: the route is never cached, so
        // the page is what remembers which table it has, and an unchanged one
        // comes back as a `304` with no body to parse.
        const response = await fetch(withToken("/api/sessions"), {
          cache: "no-store",
          headers: etag ? { "If-None-Match": etag } : {},
        });
        if (gone || mine !== round) return;
        if (response.status === 304) {
          setFeed("polling");
          return;
        }
        if (!response.ok) throw new Error(String(response.status));
        const list = JSON.parse(await response.text());
        if (gone || mine !== round || !Array.isArray(list)) return;
        etag = response.headers.get("ETag") ?? "";
        setSessions(only ? pick(list, only) : list);
        setFeed("polling");
      } catch {
        // Neither way in is working; the next tick or the stream may.
        if (!gone && mine === round) setFeed("down");
      } finally {
        asking = false;
      }
    };
    const startPolling = () => {
      if (gone || poller !== undefined) return;
      round += 1;
      etag = "";
      poller = setInterval(poll, POLL_EVERY);
      poll();
    };
    const stopPolling = () => {
      if (poller === undefined) return;
      clearInterval(poller);
      poller = undefined;
      round += 1;
    };
    const onVisible = () => {
      // Hidden, the page skipped its polls; shown, it is a table behind.
      if (document.visibilityState === "visible") poll();
    };

    // Each source's listeners are its own: a stale one being closed must not
    // have a late event of its own land on the table its replacement keeps.
    const connect = () => {
      clearTimeout(retry);
      clearTimeout(firstBy);
      source?.close();
      heard = Date.now();
      // `rows: "patch"` asks for rows as the fields that changed, which a
      // page from before that shape existed would not know to apply.
      const mine = new EventSource(withToken("/api/events", only ? { session: only } : { rows: "patch" }));
      source = mine;
      let spoke = false;
      firstBy = setTimeout(() => {
        if (source === mine && !spoke) startPolling();
      }, FIRST_EVENT_WITHIN);
      const fresh = () => {
        if (source !== mine) return false;
        heard = Date.now();
        if (!spoke) {
          spoke = true;
          clearTimeout(firstBy);
          stopPolling();
        }
        setFeed("live");
        return true;
      };
      mine.addEventListener("sessions", (event) => {
        if (!fresh()) return;
        try {
          const list = JSON.parse((event as MessageEvent).data);
          setSessions(Array.isArray(list) ? list : []);
        } catch {
          /* a bad event is skipped, not fatal */
        }
      });
      mine.addEventListener("rows", (event) => {
        if (!fresh()) return;
        try {
          const delta = JSON.parse((event as MessageEvent).data) as RowsDelta;
          setSessions((list) => (list ? applyRows(list, delta) : list));
        } catch {
          /* a bad event is skipped, not fatal */
        }
      });
      mine.addEventListener("ping", () => {
        fresh();
      });
      mine.addEventListener("error", () => {
        if (source !== mine) return;
        // Polling covers the gap; it says `polling` itself once it lands.
        if (poller === undefined) setFeed("down");
        startPolling();
        if (mine.readyState === EventSource.CLOSED && !gone) retry = setTimeout(connect, 5000);
      });
    };
    const watchdog = setInterval(() => {
      if (gone || Date.now() - heard < STALE_AFTER) return;
      if (poller === undefined) setFeed("down");
      connect();
    }, 5000);
    const onNotAsking = (event: Event) => {
      const id = (event as CustomEvent<string>).detail;
      setSessions((list) => (list ? settleAsking(list, id) : list));
      connect();
    };
    window.addEventListener(NOT_ASKING, onNotAsking);
    document.addEventListener("visibilitychange", onVisible);
    connect();
    return () => {
      gone = true;
      clearTimeout(retry);
      clearTimeout(firstBy);
      clearInterval(watchdog);
      stopPolling();
      window.removeEventListener(NOT_ASKING, onNotAsking);
      document.removeEventListener("visibilitychange", onVisible);
      source?.close();
    };
  }, [only]);
  return { sessions, feed };
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
