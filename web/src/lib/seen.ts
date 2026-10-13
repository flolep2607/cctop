// localStorage["cctop-seen"]: per session, the newest turn seq it was last
// seen at and when, `{id: {seq, at}}`. The conversation writes it and reads
// `seq` to draw the line between read and new; the dashboard reads `at` to
// mark rows that moved since.
//
// Both halves have always written that one shape, but an entry may lack
// either field — the dashboard's reader never required `seq`, and anything
// that is not a finite number is taken as absent rather than as zero.

export type Seen = { seq?: number; at?: number };

const KEY = "cctop-seen";

function readAll(): Record<string, unknown> {
  try {
    const all = JSON.parse(localStorage.getItem(KEY) || "{}");
    return all && typeof all === "object" && !Array.isArray(all) ? all : {};
  } catch {
    return {};
  }
}

const num = (v: unknown): number | undefined => {
  const n = Number(v);
  return v !== null && v !== undefined && v !== "" && isFinite(n) ? n : undefined;
};

/** Every session's entry, each reduced to the fields it has as numbers. */
export function readSeen(): Record<string, Seen> {
  const out: Record<string, Seen> = {};
  for (const [id, entry] of Object.entries(readAll())) {
    if (!entry || typeof entry !== "object") continue;
    const e = entry as Record<string, unknown>;
    out[id] = { seq: num(e.seq), at: num(e.at) };
  }
  return out;
}

/** The newest turn `id` was seen at, or null for a session never opened. */
export function seenSeq(id: string): number | null {
  return readSeen()[id]?.seq ?? null;
}

/** Record that `id` was seen up to `seq`, now. The seq never moves back. */
export function writeSeen(id: string, seq: number) {
  try {
    const all = readAll();
    const prev = num((all[id] as Record<string, unknown> | undefined)?.seq);
    all[id] = { seq: Math.max(seq, prev ?? 0), at: Date.now() };
    localStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    /* bookkeeping only */
  }
}
