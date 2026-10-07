import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import type { Dot } from "@/lib/status";

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

export function StateBadge({ state, running, question }: { state: Dot; running?: boolean; question?: boolean }) {
  const w = question && state === "asking" ? (["has a question", "text-warning border-warning/40 bg-warning/5"] as [string, string]) : WORDS[state];
  if (!w) return null;
  // Present tense is a claim about now; a stopped session's error is history.
  const text = state === "error" && running === false ? "ended on an api error" : w[0];
  return (
    <Badge variant="outline" className={cn("font-normal", w[1])}>
      {text}
    </Badge>
  );
}
