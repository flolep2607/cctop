import type { Session } from "./types";

// The table stream's two shapes, folded into one list. Pure, and kept apart
// from the hook, so what a `rows` event does to the table can be read — and
// checked — without an EventSource behind it.

/** A `rows` event: the rows that changed, and the order of ids when it moved (`table_delta` in src/serve/mod.rs). */
export interface RowsDelta {
  set?: Session[];
  order?: string[];
}

/**
 * The table after `delta`. Without an order, rows are replaced where they
 * stand; with one, the table is rebuilt in it — which is also how a row that
 * went away is said, by its id no longer being listed.
 */
export function applyRows(list: Session[], delta: RowsDelta): Session[] {
  const set = new Map((delta.set ?? []).map((s) => [s.session_id, s]));
  if (!delta.order) return set.size ? list.map((s) => set.get(s.session_id) ?? s) : list;
  const had = new Map(list.map((s) => [s.session_id, s]));
  const out: Session[] = [];
  for (const id of delta.order) {
    const row = set.get(id) ?? had.get(id);
    if (row) out.push(row);
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
