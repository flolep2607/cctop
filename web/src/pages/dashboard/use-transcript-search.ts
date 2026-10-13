import { useEffect, useState } from "react";
import { getJson } from "@/lib/api";
import type { SearchHit } from "@/lib/types";

// What a hit carries. Older servers answered with a bare list; the current one
// is `{ hits: SearchHit[] }`.
export type Hit = Pick<SearchHit, "session_id" | "snippet">;

/** Every transcript the server knows, searched for `q`: `/api/search`, in either of the shapes it has answered in. */
export async function searchTranscripts(q: string): Promise<Hit[]> {
  const data = await getJson<{ hits?: Hit[] } | Hit[]>("/api/search", { q });
  return Array.isArray(data) ? data : data.hits ?? [];
}

/**
 * The table filter's extra rows: sessions whose transcript holds `filter`,
 * each with the snippet that matched.
 *
 * The transcript is asked once typing pauses, for a query long enough to mean
 * something; hits are gated on the query that produced them, so a reply that
 * lands after the filter moved on adds nothing.
 */
export function useTranscriptSearch(filter: string): Map<string, string> {
  const [hits, setHits] = useState<{ q: string; map: Map<string, string> }>({ q: "", map: new Map() });
  useEffect(() => {
    if (filter.length < 3 || filter === hits.q) return;
    const t = setTimeout(async () => {
      try {
        const map = new Map<string, string>();
        for (const h of await searchTranscripts(filter)) if (h?.session_id && h.snippet && !map.has(h.session_id)) map.set(h.session_id, String(h.snippet));
        setHits({ q: filter, map });
      } catch {
        /* no search route, no extra rows */
      }
    }, 400);
    return () => clearTimeout(t);
  }, [filter, hits.q]);
  return hits.q === filter ? hits.map : EMPTY;
}

const EMPTY = new Map<string, string>();
