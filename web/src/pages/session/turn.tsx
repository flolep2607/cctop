import { memo, useState } from "react";
import { ChevronRight, Hash, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { clock } from "@/lib/format";
import { Badge } from "@/components/ui/badge";
import { Markdown } from "@/components/markdown";
import { Patch } from "@/components/patch";
import { Ansi } from "@/components/ansi";
import { stripAnsi } from "@/lib/ansi";
import type { Tool, Turn } from "@/lib/types";

const WHO: Record<string, string> = { user: "you", assistant: "agent", system: "harness" };

// A tool call: one line saying what it did, opening onto what came back.
// `openAll` is the conversation-wide fold; each call can still be toggled.
// Keyed on the fold by its caller, so flipping the fold resets every call.
function ToolCall({ tool, openAll }: { tool: Tool; openAll: boolean }) {
  const [open, setOpen] = useState(openAll);
  const running = tool.result === undefined || tool.result === null;
  return (
    <div className={cn("bg-muted/40 mt-2 overflow-hidden rounded-md border", tool.failed && "border-destructive/50")}>
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="hover:bg-muted/70 flex w-full min-w-0 items-baseline gap-2 px-2.5 py-1.5 text-left text-[12.5px]"
        aria-expanded={open}
      >
        <ChevronRight className={cn("text-muted-foreground size-3 shrink-0 self-center transition-transform", open && "rotate-90")} />
        <span className={cn("font-mono font-semibold", tool.failed && "text-destructive", running && "text-warning")}>{tool.name}</span>
        {tool.detail && <span className="text-muted-foreground min-w-0 truncate font-mono">{stripAnsi(tool.detail)}</span>}
        {(tool.added || tool.removed) ? (
          <span className="shrink-0 font-mono text-xs">
            <span className="text-success">+{tool.added || 0}</span> <span className="text-destructive">−{tool.removed || 0}</span>
          </span>
        ) : null}
        <span className="flex-1" />
        {tool.failed && <Badge variant="destructive">failed</Badge>}
        {running && !tool.failed && (
          <Badge variant="outline" className="text-warning border-warning/40 gap-1">
            <Loader2 className="size-3 animate-spin" />
            running
          </Badge>
        )}
      </button>
      {open && (
        <div className="border-t">
          {tool.full && tool.full !== tool.detail && (
            <pre className="text-muted-foreground px-3 py-2 font-mono text-xs break-words whitespace-pre-wrap"><Ansi text={tool.full} /></pre>
          )}
          {tool.diff && tool.diff.length > 0 && <Patch lines={tool.diff} className="border-t first:border-t-0" />}
          {!running && tool.result !== "" && (
            <pre className="text-muted-foreground max-h-96 overflow-y-auto border-t px-3 py-2 font-mono text-xs break-words whitespace-pre-wrap first:border-t-0">
              <Ansi text={tool.result ?? ""} />
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

export const TurnView = memo(function TurnView({
  turn, isNew, current, selected, hit, openTools, onLink,
}: {
  turn: Turn; isNew: boolean; current: boolean; selected: boolean; hit: boolean; openTools: boolean;
  onLink: (seq: number) => void;
}) {
  const marks = cn(
    hit && "shadow-[inset_3px_0_0_var(--primary)]",
    current && "bg-primary/10",
    selected && "ring-primary ring-1 ring-inset",
  );
  if (turn.kind === "compaction") {
    return (
      <div id={"turn-" + turn.seq} className={cn("turn text-muted-foreground border-t px-4 py-3 text-center text-xs", marks)}>
        — context compacted here —
      </div>
    );
  }
  const quiet = turn.role === "system" || turn.kind === "reasoning";
  return (
    <div
      id={"turn-" + turn.seq}
      className={cn(
        "turn group border-t px-4 py-3 first:border-t-0 [content-visibility:auto] [contain-intrinsic-size:auto_140px]",
        turn.role === "user" && "bg-primary/[0.045]",
        marks,
      )}
    >
      <div className="text-muted-foreground mb-1.5 flex items-center gap-2 text-[11px] tracking-wider uppercase">
        <span className={cn(turn.role === "user" && "text-primary font-medium")}>
          {turn.kind === "reasoning" ? "thinking" : WHO[turn.role] || turn.role}
        </span>
        {turn.ts && <span className="normal-case tracking-normal opacity-80">{clock(turn.ts, true)}</span>}
        {isNew && <span className="bg-primary size-1.5 rounded-full" title="New since your last visit" />}
        <button
          type="button"
          onClick={() => onLink(turn.seq)}
          className="hover:text-primary ml-auto opacity-0 transition-opacity group-hover:opacity-70 focus-visible:opacity-100"
          title="Copy a link to this turn"
          aria-label="Copy a link to this turn"
        >
          <Hash className="size-3.5" />
        </button>
      </div>
      {turn.text && (
        <div className={cn(quiet && "text-muted-foreground [&_.md]:text-[13px]")}>
          <Markdown text={turn.text} />
        </div>
      )}
      {turn.clipped && <div className="text-muted-foreground mt-1 text-[11px]">… cut for length; the whole of it is in the transcript</div>}
      {(turn.tools ?? []).map((tool, i) => (
        <ToolCall key={i + ":" + openTools} tool={tool} openAll={openTools} />
      ))}
    </div>
  );
});
