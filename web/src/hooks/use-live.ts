import { useEffect, useState } from "react";
import { withToken } from "@/lib/config";
import { getJson } from "@/lib/api";
import type { Session, Tab } from "@/lib/types";

/**
 * The session table, live, over the same `sessions` event stream every cctop
 * page reads. With `only`, the server sends that session's row alone — the
 * whole table re-sent on every refresh would be paid for by a page that
 * reads one row.
 *
 * A named event: `onmessage` only ever sees unnamed ones. And a stream that
 * was never a stream (a tunnel's error page) closes for good rather than
 * reconnecting, so a closed source is reopened on a timer.
 */
export function useSessions(only?: string): { sessions: Session[] | null; live: boolean } {
  const [sessions, setSessions] = useState<Session[] | null>(null);
  const [live, setLive] = useState(false);
  useEffect(() => {
    let source: EventSource | null = null;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let gone = false;
    const connect = () => {
      source = new EventSource(withToken("/api/events", only ? { session: only } : undefined));
      source.addEventListener("sessions", (event) => {
        try {
          const list = JSON.parse((event as MessageEvent).data);
          setSessions(Array.isArray(list) ? list : []);
          setLive(true);
        } catch {
          /* a bad event is skipped, not fatal */
        }
      });
      source.addEventListener("open", () => setLive(true));
      source.addEventListener("error", () => {
        setLive(false);
        if (source?.readyState === EventSource.CLOSED && !gone) retry = setTimeout(connect, 5000);
      });
    };
    connect();
    return () => {
      gone = true;
      clearTimeout(retry);
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

/** Re-render on a slow clock, so "4m ago" stays honest between events. */
export function useTick(ms = 15000): number {
  const [n, setN] = useState(0);
  useEffect(() => {
    const t = setInterval(() => setN((x) => x + 1), ms);
    return () => clearInterval(t);
  }, [ms]);
  return n;
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
