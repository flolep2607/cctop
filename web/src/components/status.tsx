import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
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

export function StateDot({ state, className }: { state: Dot; className?: string }) {
  return (
    <span
      aria-hidden
      className={cn(
        "inline-block size-2 shrink-0 rounded-full",
        state === "working" && "bg-success",
        state === "waiting" && "bg-warning",
        (state === "asking" || state === "error") && "bg-destructive",
        state === "asking" && "ring-2 ring-destructive/25 animate-pulse",
        (state === "idle" || state === "gone") && "bg-muted-foreground/50",
        className,
      )}
    />
  );
}

const WORDS: Partial<Record<Dot, [string, string]>> = {
  working: ["working", "text-success border-success/40"],
  waiting: ["waiting on you", "text-warning border-warning/40"],
  asking: ["needs permission", "text-destructive border-destructive/40 bg-destructive/5"],
  error: ["api error", "text-destructive border-destructive/40"],
};

export function StateBadge({ state, running }: { state: Dot; running?: boolean }) {
  const w = WORDS[state];
  if (!w) return null;
  // Present tense is a claim about now; a stopped session's error is history.
  const text = state === "error" && running === false ? "ended on an api error" : w[0];
  return (
    <Badge variant="outline" className={cn("font-normal", w[1])}>
      {text}
    </Badge>
  );
}
