import { memo, useState, type ReactNode } from "react";
import { ArrowUpRight, Bot, ChevronRight, Hash, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { clock, money, secs, tokens } from "@/lib/format";
import { Badge } from "@/components/ui/badge";
import { Markdown } from "@/components/markdown";
import { Patch } from "@/components/patch";
import { Ansi } from "@/components/ansi";
import { stripAnsi } from "@/lib/ansi";
import type { AgentCall, Tool, Turn } from "@/lib/types";
import { LAST_MESSAGE, agentTitle, useAgents } from "./agents";
import { useChat } from "./use-chat";

const WHO: Record<string, string> = { user: "you", assistant: "agent", system: "harness" };

// Whose words a turn is. Another agent's report is filed as `system` so it never
// reads as the person's, but "harness" would be wrong too: it names its sender.
// `asked` names whoever plays the user's part: the person in the main
// conversation, the main agent inside a subagent's.
function who(turn: Turn, asked: string): string {
  if (turn.kind === "reasoning") return "thinking";
  if (turn.kind === "agent-message") return turn.from ? `from an agent · ${turn.from}` : "from an agent";
  if (turn.role === "user") return asked;
  return WHO[turn.role] || turn.role;
}

// A tool call: one line saying what it did, opening onto what came back.
// `openAll` is the conversation-wide fold; each call can still be toggled.
// Keyed on the fold by its caller, so flipping the fold resets every call.
function ToolCall({ tool, openAll }: { tool: Tool; openAll: boolean }) {
  // An agent's block is not opened by the conversation-wide fold: opening one
  // reads that agent's whole transcript, and the fold would read them all.
  if (tool.agent) return <AgentBlock tool={tool} agent={tool.agent} />;
  return <PlainCall tool={tool} openAll={openAll} />;
}

function PlainCall({ tool, openAll }: { tool: Tool; openAll: boolean }) {
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
  turn, isNew, current, selected, hit, openTools, onLink, idPrefix = "turn-", asked = "you",
}: {
  turn: Turn; isNew: boolean; current: boolean; selected: boolean; hit: boolean; openTools: boolean;
  /** Absent inside an agent's block: its turns have no link of their own. */
  onLink?: (seq: number) => void;
  idPrefix?: string;
  asked?: string;
}) {
  const marks = cn(
    hit && "shadow-[inset_3px_0_0_var(--primary)]",
    current && "bg-primary/10",
    selected && "ring-primary ring-1 ring-inset",
  );
  if (turn.kind === "compaction") {
    return (
      <div id={idPrefix + turn.seq} className={cn("turn text-muted-foreground border-t px-4 py-3 text-center text-xs", marks)}>
        — context compacted here —
      </div>
    );
  }
  if (turn.kind === "agent-message" && onLink) return <HandBack turn={turn} marks={marks} isNew={isNew} />;
  const quiet = turn.role === "system" || turn.kind === "reasoning";
  return (
    <div
      id={idPrefix + turn.seq}
      className={cn(
        "turn group border-t px-4 py-3 first:border-t-0 [content-visibility:auto] [contain-intrinsic-size:auto_140px]",
        turn.role === "user" && "bg-primary/[0.045]",
        marks,
      )}
    >
      <div className="text-muted-foreground mb-1.5 flex items-center gap-2 text-[11px] tracking-wider uppercase">
        <span className={cn(turn.role === "user" && "text-primary font-medium")}>
          {who(turn, asked)}
        </span>
        {turn.ts && <span className="normal-case tracking-normal opacity-80">{clock(turn.ts, true)}</span>}
        {isNew && <span className="bg-primary size-1.5 rounded-full" title="New since your last visit" />}
        {turn.agent && onLink && <AgentLink id={turn.agent} />}
        {onLink && (
          <button
            type="button"
            onClick={() => onLink(turn.seq)}
            className="hover:text-primary ml-auto opacity-0 transition-opacity group-hover:opacity-70 focus-visible:opacity-100"
            title="Copy a link to this turn"
            aria-label="Copy a link to this turn"
          >
            <Hash className="size-3.5" />
          </button>
        )}
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

// A link from something about an agent — its hand-back, a notice — up to the
// call that started it, where its block is.
function AgentLink({ id }: { id: string }) {
  const { calls, jump } = useAgents();
  const call = calls.get(id);
  if (!call) return null;
  return (
    <button
      type="button"
      onClick={() => jump(call.seq)}
      className="hover:text-primary inline-flex min-w-0 items-center gap-0.5 normal-case tracking-normal"
      title="Go to the call that started this agent"
    >
      <ArrowUpRight className="size-3 shrink-0" />
      <span className="truncate">{call.agent.description || call.agent.type}</span>
    </button>
  );
}

// A subagent's hand-back in the main timeline: one line naming the agent,
// opening onto its report. It stays at its time, because the main agent's next
// reply answers it, but it is the agent's words, so it is drawn small.
function HandBack({ turn, marks, isNew }: { turn: Turn; marks: string; isNew: boolean }) {
  const [open, setOpen] = useState(false);
  const { calls, jump } = useAgents();
  const call = turn.agent ? calls.get(turn.agent) : undefined;
  const name = call ? agentTitle(call.agent) : turn.from || "";
  return (
    <div id={"turn-" + turn.seq} className={cn("turn border-t px-4 py-2 first:border-t-0", marks)}>
      <div className="text-muted-foreground flex min-w-0 items-center gap-2 text-xs">
        <button type="button" onClick={() => setOpen(!open)} className="hover:text-foreground flex min-w-0 flex-1 items-center gap-1.5 text-left" aria-expanded={open}>
          <ChevronRight className={cn("size-3 shrink-0 transition-transform", open && "rotate-90")} />
          <Bot className="size-3.5 shrink-0" />
          <span className="shrink-0 tracking-wider uppercase text-[11px]">from an agent</span>
          {name && <span className="min-w-0 truncate">· {name}</span>}
        </button>
        {isNew && <span className="bg-primary size-1.5 shrink-0 rounded-full" title="New since your last visit" />}
        {call && (
          <button type="button" onClick={() => jump(call.seq)} className="hover:text-primary inline-flex shrink-0 items-center gap-0.5" title="Go to the call that started this agent">
            <ArrowUpRight className="size-3" />
            its call
          </button>
        )}
        {turn.ts && <span className="shrink-0 opacity-80">{clock(turn.ts, true)}</span>}
      </div>
      {open && turn.text && (
        <div className="text-muted-foreground mt-2 pl-5 [&_.md]:text-[13px]">
          <Markdown text={turn.text} />
        </div>
      )}
    </div>
  );
}

const STATUS_BADGE: Record<string, ReactNode> = {
  running: (
    <Badge variant="outline" className="text-warning border-warning/40 gap-1">
      <Loader2 className="size-3 animate-spin" />
      running
    </Badge>
  ),
  failed: <Badge variant="destructive">failed</Badge>,
};

// What a subagent did, in place of the call that started it: which agent, what
// for, how far it got, and — opened — its own turns, read from its own
// transcript the first time the block opens and followed while it runs.
function AgentBlock({ tool, agent }: { tool: Tool; agent: AgentCall }) {
  const [open, setOpen] = useState(false);
  const { reported, everything, jump, included } = useAgents();
  const plural = (n: number, what: string) => `${n} ${what}${n === 1 ? "" : "s"}`;
  const summary = [
    agent.turns ? plural(agent.turns, "turn") : "",
    agent.tool_count ? plural(agent.tool_count, "tool") : "",
    agent.duration_ms ? secs(agent.duration_ms) : "",
    // As the report tab's subagent table reads it: a bundled plan's agent is
    // "incl", and an unpriced one says nothing rather than "$0".
    agent.cost ? (included ? "incl" : money(agent.cost)) : "",
    agent.started_at ? "started " + clock(agent.started_at) : "",
  ].filter(Boolean);
  const spent = agent.tokens ? `${tokens(agent.tokens)} tokens` : undefined;
  const reportedAt = agent.handback !== undefined ? reported.get(agent.handback) : undefined;
  return (
    <div className={cn("bg-muted/40 mt-2 overflow-hidden rounded-md border", agent.status === "failed" && "border-destructive/50")}>
      <button type="button" onClick={() => setOpen(!open)} className="hover:bg-muted/70 flex w-full min-w-0 items-start gap-2 px-2.5 py-2 text-left" aria-expanded={open}>
        <ChevronRight className={cn("text-muted-foreground mt-1 size-3 shrink-0 transition-transform", open && "rotate-90")} />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <Badge variant="secondary" className="font-mono">
              <Bot />
              {agent.type}
            </Badge>
            <span className="min-w-0 text-[13px] font-medium break-words">{agent.description || tool.detail}</span>
            {STATUS_BADGE[agent.status]}
          </div>
          {summary.length > 0 && (
            <div className="text-muted-foreground mt-1 text-xs break-words" title={spent}>
              {summary.join(" · ")}
            </div>
          )}
          {!open && agent.report && (
            <div className={cn("text-muted-foreground mt-1.5 text-xs break-words", agent.last_message && "border-muted-foreground/40 border-l-2 border-dashed pl-2 italic")}>
              {agent.last_message && <div className="mb-0.5 not-italic text-[11px] tracking-wider uppercase">{LAST_MESSAGE}</div>}
              <div className="line-clamp-3 whitespace-pre-line">{agent.report}</div>
            </div>
          )}
        </div>
      </button>
      {reportedAt && everything && (
        <button
          type="button"
          onClick={() => jump(agent.handback!)}
          className="text-muted-foreground hover:text-primary flex w-full items-center gap-1 border-t px-2.5 py-1.5 text-left text-xs"
        >
          <ArrowUpRight className="size-3 rotate-90" />
          reported back at {clock(reportedAt)}
        </button>
      )}
      {open && <AgentBody tool={tool} agent={agent} />}
    </div>
  );
}

function AgentBody({ tool, agent }: { tool: Tool; agent: AgentCall }) {
  const { session } = useAgents();
  const [brief, setBrief] = useState(false);
  const text = tool.full || tool.detail || "";
  return (
    <div className="border-t">
      {text && (
        <div className="border-b px-3 py-2">
          <button type="button" onClick={() => setBrief(!brief)} className="text-muted-foreground hover:text-foreground flex items-center gap-1 text-xs" aria-expanded={brief}>
            <ChevronRight className={cn("size-3 transition-transform", brief && "rotate-90")} />
            The brief
          </button>
          {brief && <pre className="text-muted-foreground mt-1.5 max-h-72 overflow-y-auto font-mono text-xs break-words whitespace-pre-wrap">{text}</pre>}
        </div>
      )}
      {agent.ghost ? (
        <div className="text-muted-foreground px-3 py-3 text-xs">This agent's transcript is gone — Claude Code purged it. What the main conversation recorded is all that is left.</div>
      ) : (
        <AgentTurns session={session} agent={agent} brief={text} />
      )}
      {agent.report && (
        <div className="border-t px-3 py-2">
          <div className="text-muted-foreground mb-1 text-[11px] tracking-wider uppercase">{agent.last_message ? LAST_MESSAGE : "Report"}</div>
          <div className={cn("[&_.md]:text-[13px]", agent.last_message && "border-muted-foreground/40 text-muted-foreground border-l-2 border-dashed pl-2")}>
            <Markdown text={agent.report} />
          </div>
        </div>
      )}
    </div>
  );
}

// A subagent's own turns, under a rule that says they are its. The brief is
// already shown above, so its copy as the agent's first message is not repeated.
function AgentTurns({ session, agent, brief }: { session: string; agent: AgentCall; brief: string }) {
  const { turns, meta, error, earlier, loadEarlier } = useChat(session, agent.status === "running", agent.id);
  const shown = turns.filter((t) => !(t.role === "user" && brief && (t.text ?? "").trim() === brief.trim()));
  if (error && !turns.length)
    return (
      <div className="text-muted-foreground px-3 py-3 text-xs">
        Could not read this agent's turns: {error}. On a session from another machine, the cctop there may be too old to answer — update it.
      </div>
    );
  if (meta && !meta.supported) return <div className="text-muted-foreground px-3 py-3 text-xs">{meta.note}</div>;
  if (!meta) return <div className="text-muted-foreground px-3 py-3 text-xs">Reading this agent's turns…</div>;
  return (
    <div className="border-primary/30 ml-3 border-l-2">
      {earlier > 0 && (
        <button type="button" onClick={() => loadEarlier()} className="text-muted-foreground hover:text-primary w-full px-4 py-2 text-left text-xs">
          Show earlier turns — {earlier} not drawn
        </button>
      )}
      {!shown.length && <div className="text-muted-foreground px-4 py-3 text-xs">Nothing yet.</div>}
      {shown.map((t) => (
        <TurnView
          key={t.seq}
          turn={t}
          isNew={false}
          current={false}
          selected={false}
          hit={false}
          openTools={false}
          idPrefix={`agent-${agent.id}-turn-`}
          asked="main agent"
        />
      ))}
    </div>
  );
}
