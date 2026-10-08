import type { Session } from "./types";

// The table stream's two shapes, folded into one list. Pure, and kept apart
// from the hook, so what a `rows` event does to the table can be read — and
// checked — without an EventSource behind it.

/** One row's changed fields, and the ones it no longer has (`table_patch` in crates/serve/src/lib.rs). */
export interface RowPatch {
  id: string;
  to: Partial<Session>;
  drop?: string[];
}

/**
 * A `rows` event: rows sent whole, rows sent as the fields that changed, and
 * the order when it moved — either whole, or as the ids to put first and the
 * ids that are gone (`table_patch` in crates/serve/src/lib.rs).
 */
export interface RowsDelta {
  set?: Session[];
  patch?: RowPatch[];
  order?: string[];
  head?: string[];
  gone?: string[];
}

/** `row` with `patch` laid over it. */
function patched(row: Session, patch: RowPatch): Session {
  const out = { ...row, ...patch.to } as Record<string, unknown>;
  for (const key of patch.drop ?? []) delete out[key];
  return out as unknown as Session;
}

/**
 * The table after `delta`. Without an order, rows are replaced where they
 * stand; with one, the table is rebuilt in it — which is also how a row that
 * went away is said, by its id no longer being listed. `head` and `gone` say
 * the same as an order more briefly: `head` first, then every other row the
 * table had that is not `gone`, where it was.
 */
export function applyRows(list: Session[], delta: RowsDelta): Session[] {
  const set = new Map((delta.set ?? []).map((s) => [s.session_id, s]));
  const patches = new Map((delta.patch ?? []).map((p) => [p.id, p]));
  const fresh = (s: Session): Session => {
    const whole = set.get(s.session_id);
    if (whole) return whole;
    const p = patches.get(s.session_id);
    return p ? patched(s, p) : s;
  };
  let order = delta.order;
  if (!order && delta.head) {
    const first = new Set(delta.head);
    const gone = new Set(delta.gone ?? []);
    order = [...delta.head, ...list.map((s) => s.session_id).filter((id) => !first.has(id) && !gone.has(id))];
  }
  if (!order) return set.size || patches.size ? list.map(fresh) : list;
  const had = new Map(list.map((s) => [s.session_id, s]));
  const out: Session[] = [];
  for (const id of order) {
    const row = had.get(id);
    const now = row ? fresh(row) : set.get(id);
    if (now) out.push(now);
  }
  return out;
}

/**
 * The table with `id`'s prompt taken down, for when the server has said it is
 * not asking. "working" because an answered prompt is a tool running, and the
 * next event for the row replaces this guess with what the server knows.
 */
export function settleAsking(list: Session[], id: string): Session[] {
  return list.map((s) => (s.session_id === id && s.state === "asking" ? { ...s, state: "working", asking_for: null, asking_question: false } : s));
}
