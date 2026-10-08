import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ArrowDown, ChevronDown, ChevronUp, ChevronsDownUp, ChevronsUpDown, Rows3, Rows4, Search, SendHorizontal } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { act } from "@/lib/api";
import { CAN_ACT, OPENING_QUERY, withToken } from "@/lib/config";
import { copyText } from "@/lib/format";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Kbd } from "@/components/ui/kbd";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { PromptBar } from "@/components/prompt-bar";
import { useStored } from "@/hooks/use-live";
import type { Session, Turn } from "@/lib/types";
import { TurnView } from "./turn";
import { AgentsContext, aboutAgent, agentCalls, type Agents } from "./agents";
import { useChat } from "./use-chat";

// How many of a long conversation's turns are drawn at once: the newest
// CHUNK, one chunk more per click. The tail is what is live; drawing all of a
// 2,000-turn session is the stall, not the poll.
const CHUNK = 120;

// What a turn can be found by: its text and its tools' names, arguments,
// results and diffs.
const turnText = (t: Turn) =>
  [
    t.text,
    ...(t.tools ?? []).flatMap((x) => [x.name, x.detail, x.full, x.result, x.agent?.type, x.agent?.description, x.agent?.report, ...(x.diff ?? [])]),
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();

// localStorage["cctop-seen"], shared with the dashboard: {id: {seq, at}} — the
// newest turn seq this session was last seen at. Read once per visit, so the
// line between read and new does not move under the reader.
function readSeen(id: string): number | null {
  try {
    const seq = Number(JSON.parse(localStorage.getItem("cctop-seen") || "{}")[id]?.seq);
    return isFinite(seq) ? seq : null;
  } catch {
    return null;
  }
}
function writeSeen(id: string, seq: number) {
  try {
    const all = JSON.parse(localStorage.getItem("cctop-seen") || "{}");
    const prev = Number(all[id]?.seq);
    all[id] = { seq: Math.max(seq, isFinite(prev) ? prev : 0), at: Date.now() };
    localStorage.setItem("cctop-seen", JSON.stringify(all));
  } catch {
    /* bookkeeping only */
  }
}

const typing = (t: EventTarget | null) =>
  t instanceof HTMLElement && (t.isContentEditable || /^(INPUT|SELECT|TEXTAREA)$/.test(t.tagName));

export function Conversation({ id, live, active }: { id: string; live: Session | null; active: boolean }) {
  const running = !!live?.running;
  const { turns, meta, error, earlier, loadEarlier, held } = useChat(id, running);
  const [shown, setShown] = useState(CHUNK);
  const [query, setQuery] = useState(OPENING_QUERY.get("find") ?? "");
  const [seekAt, setSeekAt] = useState(-1);
  const [current, setCurrent] = useState<number | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [openTools, setOpenTools] = useState(false);
  const [density, setDensity] = useStored<"comfortable" | "compact">("cctop-density-v2", "comfortable");
  // "main": the main agent's own timeline, each subagent's report left in its
  // block. Nothing becomes unreachable — the hand-backs are what is hidden.
  const [scope, setScope] = useStored<"everything" | "main">("cctop-chat-scope", "everything", (v): v is "everything" | "main" => v === "everything" || v === "main");
  const [more, setMore] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const findRef = useRef<HTMLInputElement>(null);
  const near = useRef(true);
  const seenBase = useRef<number | null | undefined>(undefined);
  const firstFill = useRef(true);
  const wantTurn = useRef<number | null>(null);
  const [scrollTo, setScrollTo] = useState<number | null>(null);

  if (seenBase.current === undefined) seenBase.current = readSeen(id);
  const base = seenBase.current;

  // A "#chat/turn-12" link opens on that turn once the conversation is drawn.
  useEffect(() => {
    const m = location.hash.match(/^#chat\/turn-(\d+)$/);
    if (m) wantTurn.current = Number(m[1]);
  }, []);

  const visible = useMemo(() => (scope === "main" ? turns.filter((t) => !aboutAgent(t)) : turns), [turns, scope]);
  const hidden = Math.max(0, visible.length - shown);
  const drawn = useMemo(() => visible.slice(hidden), [visible, hidden]);

  // Find runs over every fetched turn, not just the drawn ones: a hit the
  // chunking hides is still a hit, and walking to it draws its window. What
  // "Main only" leaves out is not searched, nor is an agent's own transcript.
  const q = query.trim().toLowerCase();
  const hits = useMemo(() => (q ? visible.filter((t) => turnText(t).includes(q)).map((t) => t.seq) : []), [visible, q]);
  const hitSet = useMemo(() => new Set(hits), [hits]);

  // --- following the tail ----------------------------------------------------
  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    near.current = el.scrollTop + el.clientHeight >= el.scrollHeight - 80;
    if (near.current) setMore(false);
  };
  const toBottom = () => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
    setMore(false);
  };
  const last = turns[turns.length - 1];
  const tailKey = last ? `${turns.length}:${last.seq}:${(last.text ?? "").length}:${(last.tools ?? []).length}` : "";
  useLayoutEffect(() => {
    if (!turns.length) return;
    if (firstFill.current) {
      // The first fill is the whole conversation arriving, not new activity.
      firstFill.current = false;
      if (wantTurn.current === null) toBottom();
    } else if (near.current) {
      // Pinned to the tail: new turns must not scroll the newest out from
      // under someone watching it arrive.
      toBottom();
    } else {
      setMore(true);
    }
    writeSeen(id, turns[turns.length - 1].seq);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tailKey]);

  // --- walking to a turn -----------------------------------------------------
  const gotoTurn = useCallback(
    async (seq: number) => {
      // Each page-back is a transcript parse on the server, so it is bounded.
      let guard = 8;
      while (held.current.length && held.current[0].seq > seq && guard-- > 0) {
        try {
          await loadEarlier();
        } catch {
          break;
        }
      }
      const list = held.current;
      const pos = list.findIndex((t) => t.seq >= seq);
      if (pos < 0) {
        toast.error("That turn is not in this conversation");
        return;
      }
      setShown((s) => Math.max(s, list.length - pos));
      setCurrent(list[pos].seq);
      setScrollTo(list[pos].seq);
    },
    [held, loadEarlier],
  );
  useEffect(() => {
    if (scrollTo === null) return;
    const node = document.getElementById("turn-" + scrollTo);
    if (node) {
      node.scrollIntoView({ block: "center" });
      setScrollTo(null);
    }
  }, [scrollTo, drawn]);
  useEffect(() => {
    if (turns.length && wantTurn.current !== null) {
      const seq = wantTurn.current;
      wantTurn.current = null;
      gotoTurn(seq);
    }
  }, [turns.length, gotoTurn]);

  const seek = (dir: 1 | -1) => {
    if (!hits.length) return;
    const next = seekAt < 0 ? (dir > 0 ? 0 : hits.length - 1) : (seekAt + dir + hits.length) % hits.length;
    setSeekAt(next);
    gotoTurn(hits[next]);
  };
  // A seeded "?find=" walks to its first hit once there is something to search.
  const seeded = useRef(false);
  useEffect(() => {
    if (!seeded.current && q && turns.length) {
      seeded.current = true;
      if (hits.length) {
        setSeekAt(0);
        gotoTurn(hits[0]);
      }
    }
  }, [q, turns.length, hits, gotoTurn]);

  const linkTurn = useCallback((seq: number) => {
    const hash = "#chat/turn-" + seq;
    try {
      history.replaceState(history.state, "", location.pathname + hash);
    } catch {
      /* keep the URL it had */
    }
    copyText(location.origin + withToken(location.pathname + hash))
      .then(() => toast.success("Copied a link to this turn"))
      .catch((e) => toast.error("Copy failed: " + String(e?.message || e)));
  }, []);

  // Rebuilt only when what it says changes, so a poll that adds an unrelated
  // turn does not redraw every memoised turn through the context.
  const callKey = turns.map((t) => (t.tools ?? []).filter((x) => x.agent).map((x) => `${t.seq}:${JSON.stringify(x.agent)}`).join()).join("|");
  const reportKey = turns.filter((t) => t.kind === "agent-message").map((t) => t.seq + t.ts!).join();
  const agents = useMemo<Agents>(
    () => ({
      session: id,
      calls: agentCalls(turns),
      reported: new Map(turns.filter((t) => t.kind === "agent-message").map((t) => [t.seq, t.ts ?? ""])),
      everything: scope === "everything",
      jump: gotoTurn,
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [id, callKey, reportKey, scope, gotoTurn],
  );

  const showEarlier = async () => {
    const el = scrollRef.current;
    const before = el?.scrollHeight ?? 0;
    if (hidden > 0) setShown((s) => s + CHUNK);
    else {
      try {
        await loadEarlier();
        setShown((s) => s + CHUNK);
      } catch (e) {
        toast.error(String((e as Error).message || e));
        return;
      }
    }
    // Held on the same turn rather than jumping to wherever the new top put it.
    requestAnimationFrame(() => {
      if (el) el.scrollTop += el.scrollHeight - before;
    });
  };

  // --- keys --------------------------------------------------------------------
  useEffect(() => {
    if (!active) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || typing(e.target)) return;
      const move = (dir: 1 | -1) => {
        e.preventDefault();
        if (!drawn.length) return;
        const at = drawn.findIndex((t) => t.seq === selected);
        const next = at < 0 ? (dir > 0 ? drawn[0] : drawn[drawn.length - 1]) : drawn[Math.min(drawn.length - 1, Math.max(0, at + dir))];
        setSelected(next.seq);
        document.getElementById("turn-" + next.seq)?.scrollIntoView({ block: "nearest" });
      };
      if (e.key === "j" || e.key === "ArrowDown") move(1);
      else if (e.key === "k" || e.key === "ArrowUp") move(-1);
      else if (e.key === "n") seek(1);
      else if (e.key === "N") seek(-1);
      else if (e.key === "t") setOpenTools((o) => !o);
      else if (e.key === "a") setScope(scope === "main" ? "everything" : "main");
      else if (e.key === "/") {
        e.preventDefault();
        findRef.current?.focus();
        findRef.current?.select();
      } else if (e.key === "Escape") {
        if (query) setQuery("");
        setSelected(null);
        setCurrent(null);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });

  const unsupported = meta && !meta.supported;
  const fresh = base !== null ? turns.filter((t) => t.seq > base).length : 0;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* The find bar and the conversation-wide toggles. */}
      <div className="flex flex-wrap items-center gap-2 px-4 pb-2">
        <div className="relative min-w-40 flex-1">
          <Search className="text-muted-foreground pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2" />
          <Input
            ref={findRef}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSeekAt(-1);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                seek(e.shiftKey ? -1 : 1);
              } else if (e.key === "Escape") {
                setQuery("");
                (e.target as HTMLInputElement).blur();
              }
            }}
            placeholder="Find in the conversation…"
            className="h-8 pl-8"
            aria-label="Find in the conversation"
          />
        </div>
        {q && (
          <span className="text-muted-foreground text-xs whitespace-nowrap">
            {hits.length ? `${seekAt + 1 > 0 ? seekAt + 1 + " of " : ""}${hits.length} turn${hits.length === 1 ? "" : "s"}` : "no matches"}
            {earlier > 0 && " · earlier not searched"}
          </span>
        )}
        <Button variant="outline" size="icon-sm" disabled={!hits.length} onClick={() => seek(-1)} aria-label="Previous match">
          <ChevronUp />
        </Button>
        <Button variant="outline" size="icon-sm" disabled={!hits.length} onClick={() => seek(1)} aria-label="Next match">
          <ChevronDown />
        </Button>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="outline" size="sm" onClick={() => setOpenTools(!openTools)} aria-pressed={openTools}>
              {openTools ? <ChevronsDownUp /> : <ChevronsUpDown />}
              Tools
            </Button>
          </TooltipTrigger>
          <TooltipContent>{openTools ? "Close" : "Open"} every tool call <Kbd>t</Kbd></TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="outline"
              size="icon-sm"
              onClick={() => setDensity(density === "compact" ? "comfortable" : "compact")}
              aria-label={"Density: " + density}
            >
              {density === "compact" ? <Rows4 /> : <Rows3 />}
            </Button>
          </TooltipTrigger>
          <TooltipContent>Density: {density}</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              value={scope}
              onValueChange={(v) => v && setScope(v as "everything" | "main")}
              aria-label="Which turns to show"
            >
              <ToggleGroupItem value="everything" className="px-2.5 text-xs">
                Everything
              </ToggleGroupItem>
              <ToggleGroupItem value="main" className="px-2.5 text-xs">
                Main only
              </ToggleGroupItem>
            </ToggleGroup>
          </TooltipTrigger>
          <TooltipContent>
            Main only leaves subagents' hand-backs in their blocks <Kbd>a</Kbd>
          </TooltipContent>
        </Tooltip>
        <span
          className="text-muted-foreground hidden text-[11px] lg:inline"
          title="j/k move · n/N walk matches · t folds tools · a main only / everything · 1–4 views · / find (what is shown, not agents' own turns) · Esc clears"
        >
          <Kbd>j</Kbd>/<Kbd>k</Kbd> <Kbd>n</Kbd> <Kbd>t</Kbd> <Kbd>a</Kbd> <Kbd>/</Kbd>
        </span>
      </div>

      {/* The conversation — the only thing on this page that scrolls. */}
      <div className="relative min-h-0 flex-1">
        <div ref={scrollRef} onScroll={onScroll} className="absolute inset-0 overflow-y-auto overscroll-contain px-4 pb-4" data-density={density}>
          <div className="bg-card overflow-hidden rounded-xl border shadow-xs">
            {(hidden > 0 || earlier > 0) && (
              <button type="button" onClick={showEarlier} className="text-muted-foreground hover:text-primary w-full border-b px-4 py-2.5 text-left text-xs">
                Show {Math.min(CHUNK, hidden + earlier)} earlier turns — {hidden + earlier} in all not drawn
              </button>
            )}
            {fresh > 0 && base !== null && !firstFill.current && (
              <button type="button" onClick={() => gotoTurn(base + 1)} className="text-primary w-full border-b px-4 py-2.5 text-left text-xs">
                {fresh}
                {earlier > base ? "+" : ""} new since your last visit — jump to the first
              </button>
            )}
            {unsupported ? (
              <div className="text-muted-foreground p-10 text-center text-sm">{meta?.note || "no reader for this harness"}</div>
            ) : !meta && !error ? (
              <div className="text-muted-foreground p-10 text-center text-sm">Reading the conversation…</div>
            ) : !turns.length ? (
              <div className="text-muted-foreground p-10 text-center text-sm">{error || "Nothing has been said in this session yet."}</div>
            ) : (
              <AgentsContext.Provider value={agents}>
                {drawn.map((t) => (
                <TurnView
                  key={t.seq}
                  turn={t}
                  isNew={base !== null && t.seq > base}
                  current={current === t.seq}
                  selected={selected === t.seq}
                  hit={hitSet.has(t.seq)}
                  openTools={openTools || current === t.seq}
                  onLink={linkTurn}
                />
                ))}
              </AgentsContext.Provider>
            )}
          </div>
          {error && turns.length > 0 && <div className="text-destructive mt-2 text-xs">{error} Still trying.</div>}
        </div>
        {more && (
          <Button size="sm" variant="secondary" className="absolute bottom-3 left-1/2 -translate-x-1/2 shadow-md" onClick={toBottom}>
            <ArrowDown />
            New activity
          </Button>
        )}
      </div>

      <Composer id={id} live={live} />
    </div>
  );
}

// The prompt box, pinned under the conversation rather than inside it, so it
// is always on screen. One line, because that is what a pty submit is. While
// the session is stopped it is the Resume button instead.
function Composer({ id, live }: { id: string; live: Session | null }) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  if (!CAN_ACT)
    return (
      <div className="text-muted-foreground shrink-0 border-t px-4 py-2.5 text-xs">
        View-only link — replies, Allow/Deny and resume need the “serving on” link <code>cctop serve</code> printed first.
      </div>
    );
  const running = !!live?.running;
  const asking = running && live?.state === "asking";
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      if (!running) {
        const done = await act("resume", id);
        toast.success(done.message || "Resumed");
      } else {
        if (!text.trim()) return;
        const done = await act("send", id, { text: text.trim() });
        toast.success(done.message || "Sent");
        setText("");
      }
    } catch (err) {
      toast.error(String((err as Error).message || err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="bg-background shrink-0 border-t px-4 py-2.5">
      {asking && live && <PromptBar key={live.asking_for ?? ""} session={live} className="mb-2" />}
      <form onSubmit={submit} className="flex gap-2">
        <Input
          value={text}
          onChange={(e) => setText(e.target.value)}
          disabled={!running || busy}
          maxLength={4000}
          placeholder={running ? "Answer this session…" : "Nothing is running this session"}
          className={cn("h-9", !running && "opacity-60")}
        />
        <Button type="submit" disabled={busy} className="h-9" title={running ? "" : "Start this session's harness back up on this transcript"}>
          {running ? <SendHorizontal /> : null}
          {running ? "Send" : "Resume"}
        </Button>
      </form>
    </div>
  );
}
