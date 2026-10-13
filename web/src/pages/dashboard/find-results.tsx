import { X } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { Session } from "@/lib/types";
import { SessionRow } from "./rows";
import type { Hit } from "./use-transcript-search";

export type Find = { q: string; hits: Hit[] | null; error?: string };

// Which transcripts hold these words at all — every session the server knows,
// not only the rows on screen. A hit opens the chat with its find box seeded.
export function FindResults({ find, byId, now, onClose }: { find: Find; byId: Map<string, Session>; now: number; onClose: () => void }) {
  return (
    <section className="bg-card relative overflow-hidden rounded-xl border shadow-xs" data-rendered="">
      <h2 className="text-muted-foreground px-4 pt-3 pb-1 text-xs font-semibold tracking-wider uppercase">Transcript search — “{find.q}”</h2>
      <Button variant="ghost" size="icon-xs" className="absolute top-2 right-2" onClick={onClose} aria-label="Back to the session table">
        <X />
      </Button>
      {find.error ? (
        <div className="text-muted-foreground p-10 text-center text-sm">{find.error}</div>
      ) : find.hits === null ? (
        <div className="text-muted-foreground p-10 text-center text-sm">Searching every transcript…</div>
      ) : !find.hits.length ? (
        <div className="text-muted-foreground p-10 text-center text-sm">No transcript mentions that.</div>
      ) : (
        find.hits.map((h) => {
          // A hit names only the session; one not in the table is drawn from its id alone.
          const s = byId.get(h.session_id) ?? ({ session_id: h.session_id, provider: "", state: "idle", running: false, project: null, title: null, model: null } as unknown as Session);
          return (
            <SessionRow
              key={h.session_id}
              s={s}
              now={now}
              selected={false}
              picked={false}
              pinned={false}
              fresh={false}
              snippet={h.snippet}
              href={"/session/" + encodeURIComponent(h.session_id) + "?find=" + encodeURIComponent(find.q)}
            />
          );
        })
      )}
    </section>
  );
}
