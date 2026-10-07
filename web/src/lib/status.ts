import type { Session } from "@/lib/types";

// The state of an agent, the way every cctop surface draws it: one dot, one
// word, the same four colours. "asking" is louder than "waiting" — that one is
// your move whenever, this one is an agent stopped mid-tool until you say.
export type Dot = "working" | "waiting" | "asking" | "error" | "idle" | "gone";

export function dotOf(s: Pick<Session, "running" | "state"> | null | undefined): Dot {
  if (!s) return "idle";
  if (s.state === "error") return "error";
  if (!s.running) return "idle";
  if (s.state === "working" || s.state === "waiting" || s.state === "asking") return s.state;
  return "idle";
}

export function dotOfTab(state: string | undefined | null): Dot {
  return state === "needs-input" ? "asking" : state === "working" ? "working" : state ? "idle" : "gone";
}
