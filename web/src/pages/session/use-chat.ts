import { useCallback, useEffect, useRef, useState } from "react";
import { getJson } from "@/lib/api";
import type { Chat, Turn } from "@/lib/types";

// The conversation, as the server pages it out.
//
// Every turn carries `seq`, its transcript-wide number, and that is the only
// name that stays true across a poll (which re-reads the tail) and a page-back
// (which prepends older turns) — so turns are merged by seq, fresh copy wins,
// and seq is what DOM ids, deep links and the find box use.
//
// Once something is held, a poll asks only for what it lacks: `since` the
// stamp of the transcript as last read (nothing at all comes back if it has
// not moved) and `after` the newest held turn (which a landing tool result
// changes) and anything after it.
export function useChat(id: string, running: boolean) {
  const [turns, setTurns] = useState<Turn[]>([]);
  const [meta, setMeta] = useState<Pick<Chat, "supported" | "note" | "earlier"> | null>(null);
  const [error, setError] = useState("");
  const stamp = useRef<string | null>(null);
  const held = useRef<Turn[]>([]);

  const merge = useCallback((fresh: Turn[]) => {
    const bySeq = new Map(fresh.map((t) => [t.seq, t]));
    const next = held.current.filter((t) => !bySeq.has(t.seq)).concat(fresh).sort((a, b) => a.seq - b.seq);
    held.current = next;
    setTurns(next);
  }, []);

  const refresh = useCallback(async () => {
    const newest = held.current.length ? held.current[held.current.length - 1].seq : null;
    const narrow = stamp.current !== null && newest !== null;
    try {
      const chat = await getJson<Chat>("/api/chat/" + encodeURIComponent(id), narrow ? { since: stamp.current, after: newest } : undefined);
      if (chat.stamp) stamp.current = chat.stamp;
      setError("");
      if (chat.unchanged) return;
      if (!narrow) setMeta({ supported: chat.supported, note: chat.note, earlier: chat.earlier });
      merge(chat.turns ?? []);
    } catch (e) {
      setError(String((e as Error).message || e));
    }
  }, [id, merge]);

  // Turns the transcript holds before everything fetched: the seq of the
  // oldest held turn is exactly the count of what came before it.
  const earlier = turns.length ? turns[0].seq : meta?.earlier ?? 0;

  const loadEarlier = useCallback(async () => {
    const first = held.current.length ? held.current[0].seq : 0;
    if (!first) return;
    const chat = await getJson<Chat>("/api/chat/" + encodeURIComponent(id), { before: first });
    merge(chat.turns ?? []);
  }, [id, merge]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Followed while the stream says the session runs — not the once-read
  // report, which would poll a stopped session forever or never follow one
  // that started. One last read on stopping: the final turns can postdate the
  // last poll.
  const wasRunning = useRef(running);
  useEffect(() => {
    if (wasRunning.current && !running) refresh();
    wasRunning.current = running;
    if (!running) return;
    const t = setInterval(refresh, 5000);
    return () => clearInterval(t);
  }, [running, refresh]);

  return { turns, meta, error, earlier, loadEarlier, refresh, held };
}
