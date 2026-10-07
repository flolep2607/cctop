import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link } from "wouter";
import { ExternalLink, GripVertical, Maximize2, MessageSquareText, Minimize2, Plus, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { ask } from "@/lib/api";
import { CAN_ACT } from "@/lib/config";
import { money, shortModel, shortPath } from "@/lib/format";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Kbd } from "@/components/ui/kbd";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { AppShell } from "@/components/app-shell";
import { PromptBar } from "@/components/prompt-bar";
import { StateDot } from "@/components/status";
import { dotOfTab } from "@/lib/status";
import { TerminalFrame } from "@/components/terminal";
import { popOut, type Terminal } from "@/lib/terminal";
import { useSessions, useStored, useTabs } from "@/hooks/use-live";
import type { Session, Tab } from "@/lib/types";

// Every agent's terminal on one screen.
//
// One rule shapes this page: moving an iframe in the DOM reloads it — a new
// socket, a blank screen, the agent's scrollback gone. So tiles are rendered in
// the order they were first opened, which only ever appends, and the order the
// reader sees is CSS `order` on a grid. Dragging, maximising, closing a
// neighbour: none of them moves a frame. A `.map()` over the display order
// would look right and reload every terminal on each drag.

type Cols = "auto" | "1" | "2" | "3" | "4";
const autoCols = (n: number) => (n <= 1 ? 1 : n <= 4 ? 2 : n <= 9 ? 3 : 4);

// Opening a terminal asks rmux for a share, so it happens once per tile per
// page load — kept by name so a re-render does not ask again, forgotten on
// failure so Retry does.
const opening = new Map<string, Promise<Terminal>>();
const mintShare = (name: string, fresh = false): Promise<Terminal> =>
  ask("/api/tab/" + encodeURIComponent(name) + "/terminal", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ origin: location.origin, fresh }),
  }).then((r) => r.json());
function openTerminal(name: string) {
  if (!opening.has(name)) {
    opening.set(
      name,
      mintShare(name).catch((e) => {
        opening.delete(name);
        throw e;
      }),
    );
  }
  return opening.get(name)!;
}

const Tile = memo(function Tile({
  name, tab, session, position, maximized, focused, dropping, onClose, onMaximize, onFocus, dragProps,
}: {
  name: string; tab?: Tab; session?: Session; position: number; maximized: boolean; focused: boolean; dropping: boolean;
  onClose: () => void; onMaximize: () => void; onFocus: () => void;
  dragProps: React.HTMLAttributes<HTMLElement>;
}) {
  const [terminal, setTerminal] = useState<Terminal | null>(null);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  // In a window of its own: the tile lets go of its frame (one browser per
  // share) and takes a fresh one back when that window closes.
  const [popped, setPopped] = useState(false);
  const retries = useRef(0);
  const pop = () =>
    popOut({
      key: name,
      title: label + " — cctop",
      release: () => {
        setPopped(true);
        setTerminal(null);
        opening.delete(name);
      },
      mint: () => mintShare(name, true),
      onClosed: () => {
        setPopped(false);
        setAttempt((a) => a + 1);
      },
    });
  const exists = !!tab;
  const state = dotOfTab(tab?.state);
  const label = tab?.label ?? name.replace(/^cctop-/, "");

  useEffect(() => {
    if (!CAN_ACT || !exists || popped) return;
    let live = true;
    openTerminal(name).then(
      (t) => {
        if (!live) return;
        setError("");
        setTerminal(t);
      },
      (e) => live && setError(String(e.message || e)),
    );
    return () => {
      live = false;
    };
  }, [name, attempt, exists, popped]);

  // An open frame outranks everything: a tab missing from one poll must not
  // cost the reader their terminal. The header says "closed"; the frame stays.
  let body: React.ReactNode;
  if (popped) body = <Empty>In a window of its own. Close that window to bring the terminal back here.</Empty>;
  else if (terminal)
    body = (
      <TerminalFrame
        url={terminal.url}
        name={terminal.name ?? name}
        title={"Terminal — " + label}
        onBroken={() => {
          // Twice at most: a share that will not connect when fresh is not
          // fixed by a third.
          if (retries.current >= 2) return;
          retries.current += 1;
          mintShare(name, true).then(
            (t) => {
              opening.set(name, Promise.resolve(t));
              setTerminal(t);
            },
            (e) => setError(String(e.message || e)),
          );
        }}
      />
    );
  else if (!CAN_ACT) body = <Empty>This is the view-only link, which cannot open terminals. Open the first link <code className="text-neutral-300">cctop serve</code> printed — “serving on …” — to type into agents here.</Empty>;
  else if (!exists) body = <Empty>This tab has closed — its agent exited or was moved.<Button size="sm" variant="secondary" onClick={onClose}>Remove</Button></Empty>;
  else if (error) body = <Empty bad>{error}<Button size="sm" variant="secondary" onClick={() => setAttempt(attempt + 1)}>Retry</Button></Empty>;
  else body = <Empty>Opening the terminal…</Empty>;

  const cost = session?.cost?.available && !session.cost.included ? money(session.cost.total) : null;
  return (
    <section
      onPointerDown={onFocus}
      className={cn(
        "@container bg-card flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-xl border shadow-xs transition-[border-color,box-shadow]",
        focused && "border-primary/50",
        dropping && "ring-primary ring-2",
        state === "asking" && "border-destructive animate-[ask_2.4s_ease-in-out_infinite]",
        state === "gone" && "opacity-75",
      )}
    >
      <header
        {...dragProps}
        onDoubleClick={onMaximize}
        title="Drag to rearrange · double-click to maximise"
        className="flex min-w-0 cursor-grab items-center gap-2 border-b py-1.5 pr-1.5 pl-2 text-[13px] select-none active:cursor-grabbing"
      >
        <GripVertical className="text-muted-foreground size-3.5 shrink-0 @max-[440px]:hidden" />
        <StateDot state={state} />
        <span className="shrink-0 font-semibold">{label}</span>
        {position < 9 && <Kbd className="@max-[440px]:hidden">{position + 1}</Kbd>}
        <span className="text-muted-foreground min-w-0 truncate text-xs @max-[360px]:hidden">{shortPath(tab?.cwd)}</span>
        {state === "asking" && <Badge variant="destructive" className="shrink-0">needs you</Badge>}
        {state === "gone" && <Badge variant="outline" className="shrink-0">closed</Badge>}
        <span className="flex-1" />
        {session?.model && <span className="text-muted-foreground font-mono text-xs whitespace-nowrap @max-[560px]:hidden">{shortModel(session.model)}</span>}
        {cost && <span className="text-muted-foreground font-mono text-xs whitespace-nowrap @max-[360px]:hidden">{cost}</span>}
        <div className="flex shrink-0 items-center">
          {tab?.session_id && (
            <Button variant="ghost" size="icon-xs" asChild title="The conversation, the changes and the report">
              <Link href={"/session/" + encodeURIComponent(tab.session_id)} aria-label="Open the session page">
                <MessageSquareText />
              </Link>
            </Button>
          )}
          {terminal && !popped && (
            <Button variant="ghost" size="icon-xs" onClick={pop} title="Open in a window of its own — it leaves the grid until that window closes" aria-label="Pop out">
              <ExternalLink />
            </Button>
          )}
          <Button variant="ghost" size="icon-xs" onClick={onMaximize} title={maximized ? "Back to the grid (Esc)" : "Maximise (Alt+Enter)"} aria-label={maximized ? "Restore" : "Maximise"}>
            {maximized ? <Minimize2 /> : <Maximize2 />}
          </Button>
          <Button variant="ghost" size="icon-xs" onClick={onClose} title="Close this tile — the agent keeps running" aria-label="Close tile">
            <X />
          </Button>
        </div>
      </header>
      {session?.running && session.state === "asking" && (
        <PromptBar key={session.asking_for ?? ""} session={session} compact className="m-1.5 rounded-md" />
      )}
      <div className="bg-terminal flex min-h-0 flex-1">{body}</div>
    </section>
  );
});

function Empty({ children, bad }: { children: React.ReactNode; bad?: boolean }) {
  return <div className={cn("m-auto grid max-w-sm justify-items-center gap-3 p-6 text-center text-sm", bad ? "text-red-400" : "text-neutral-400")}>{children}</div>;
}

export function WorkspacePage() {
  const { tabs, error } = useTabs();
  const { sessions } = useSessions();
  const [order, setOrder] = useStored<string[]>("cctop-workspace-order", [], (v): v is string[] => Array.isArray(v));
  const [cols, setCols] = useStored<Cols>("cctop-workspace-cols", "auto");
  const [maximized, setMaximized] = useState<string | null>(null);
  const [focused, setFocused] = useState<string | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const [over, setOver] = useState<string | null>(null);
  // Every tile opened this page load, in opening order — the DOM order, which
  // must never change (see the top of the file). Adjusted during render, the
  // way React wants state derived from props kept: new tiles append, closed
  // ones drop out, and nothing already there moves.
  const [mounted, setMounted] = useState<string[]>(order);
  const nextMounted = [...mounted.filter((n) => order.includes(n)), ...order.filter((n) => !mounted.includes(n))];
  if (nextMounted.length !== mounted.length || nextMounted.some((n, i) => n !== mounted[i])) setMounted(nextMounted);

  const byName = useMemo(() => new Map((tabs ?? []).map((t) => [t.name, t])), [tabs]);
  const byId = useMemo(() => new Map((sessions ?? []).map((s) => [s.session_id, s])), [sessions]);

  const add = (name: string) => !order.includes(name) && setOrder([...order, name]);
  const close = useCallback(
    (name: string) => {
      setOrder(order.filter((n) => n !== name));
      setMaximized((m) => (m === name ? null : m));
    },
    [order, setOrder],
  );
  const toggleMax = (name: string) => setMaximized((m) => (m === name ? null : name));
  const openAll = () => setOrder([...order, ...(tabs ?? []).map((t) => t.name).filter((n) => !order.includes(n))]);

  // Keys, for when the page rather than a terminal has focus — a terminal keeps
  // every key it is given, as it must.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.closest?.("input, textarea, select, [role=dialog]")) return;
      if (e.key === "Escape" && maximized) return setMaximized(null);
      if (!e.altKey || e.ctrlKey || e.metaKey) return;
      if (/^[1-9]$/.test(e.key)) {
        const name = order[Number(e.key) - 1];
        if (!name) return;
        e.preventDefault();
        setFocused(name);
        if (maximized) setMaximized(name);
        document.querySelector<HTMLIFrameElement>(`[data-tile="${CSS.escape(name)}"] iframe`)?.focus();
      } else if (e.key === "Enter" && (focused || order[0])) {
        e.preventDefault();
        toggleMax(focused || order[0]);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });

  // The tab title counts agents waiting on you, so a workspace in a background
  // browser tab still says when it wants attention.
  const asking = (tabs ?? []).filter((t) => t.state === "needs-input").length;
  useEffect(() => {
    document.title = (asking ? `(${asking}) ` : "") + "cctop — workspace";
  }, [asking]);

  const dragFor = (name: string): React.HTMLAttributes<HTMLElement> => ({
    draggable: true,
    onDragStart: (e) => {
      setDragging(name);
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", name);
    },
    onDragEnd: () => {
      setDragging(null);
      setOver(null);
    },
  });
  const dropFor = (target: string): React.HTMLAttributes<HTMLElement> => ({
    onDragOver: (e) => {
      if (!dragging) return;
      e.preventDefault();
      if (over !== target) setOver(target);
    },
    onDrop: (e) => {
      e.preventDefault();
      if (dragging && dragging !== target) {
        const next = order.filter((n) => n !== dragging);
        next.splice(next.indexOf(target) + (order.indexOf(dragging) < order.indexOf(target) ? 1 : 0), 0, dragging);
        setOrder(next);
      }
      setDragging(null);
      setOver(null);
    },
  });

  const closed = (tabs ?? []).filter((t) => !order.includes(t.name));
  const n = cols === "auto" ? autoCols(order.length) : Number(cols);

  return (
    <AppShell>
      <div className="flex shrink-0 flex-wrap items-center gap-2 px-4 py-2.5" data-rendered={tabs !== null || error ? "" : undefined}>
        <div className="flex min-w-0 flex-wrap items-center gap-1.5" aria-label="Tabs not on screen">
          {tabs === null ? (
            <span className="text-muted-foreground text-sm">Reading the tabs…</span>
          ) : closed.length ? (
            closed.map((t) => (
              <Button key={t.name} variant="outline" size="sm" className="max-w-60 rounded-full" onClick={() => add(t.name)} title={"Add to the workspace · " + shortPath(t.cwd)}>
                <StateDot state={dotOfTab(t.state)} />
                <span className="truncate">{t.label}</span>
                <Plus className="text-muted-foreground" />
              </Button>
            ))
          ) : (
            <span className="text-muted-foreground text-sm">{tabs.length ? "Every tab is on screen." : ""}</span>
          )}
        </div>
        <span className="flex-1" />
        {closed.length > 1 && (
          <Button variant="outline" size="sm" onClick={openAll}>
            Open all
          </Button>
        )}
        <ToggleGroup type="single" variant="outline" size="sm" value={cols} onValueChange={(v) => v && setCols(v as Cols)} className="max-sm:hidden" aria-label="Columns">
          {(["auto", "1", "2", "3", "4"] as Cols[]).map((c) => (
            <ToggleGroupItem key={c} value={c} className="px-2.5 text-xs" title={c === "auto" ? "As square as the count allows" : c + " columns"}>
              {c === "auto" ? "Auto" : c}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>
      {error && <div className="border-destructive/40 bg-destructive/5 text-destructive mx-4 mb-2 rounded-md border px-3 py-1.5 text-xs">{error}</div>}

      {order.length === 0 ? (
        <div className="bg-card text-muted-foreground mx-4 mb-4 flex flex-1 flex-col items-center justify-center gap-3 rounded-xl border p-10 text-center text-sm">
          {(tabs ?? []).length ? (
            <>
              Pick a tab above to put its terminal here.
              <Button onClick={openAll}>Open all {tabs!.length}</Button>
            </>
          ) : (
            "No agent is open in cctop's multiplexer. Start one from the dashboard or the TUI and it appears here."
          )}
        </div>
      ) : (
        <div
          className={cn(
            "grid min-h-0 flex-1 gap-2.5 px-4 pb-2 max-sm:auto-rows-[70vh] max-sm:grid-cols-1 max-sm:overflow-y-auto sm:auto-rows-[minmax(240px,1fr)]",
            dragging && "[&_iframe]:pointer-events-none",
          )}
          style={{ gridTemplateColumns: maximized ? "minmax(0,1fr)" : `repeat(${n}, minmax(0, 1fr))` }}
        >
          {nextMounted.map((name) => {
            const tab = byName.get(name);
            return (
              <div
                key={name}
                data-tile={name}
                className={cn("flex min-h-0 min-w-0", maximized && maximized !== name && "hidden")}
                style={{ order: order.indexOf(name) }}
                {...dropFor(name)}
              >
                <Tile
                  name={name}
                  tab={tab}
                  session={tab?.session_id ? byId.get(tab.session_id) : undefined}
                  position={order.indexOf(name)}
                  maximized={maximized === name}
                  focused={focused === name}
                  dropping={over === name && dragging !== name}
                  onClose={() => close(name)}
                  onMaximize={() => toggleMax(name)}
                  onFocus={() => setFocused(name)}
                  dragProps={dragFor(name)}
                />
              </div>
            );
          })}
        </div>
      )}
      <p className="text-muted-foreground shrink-0 px-4 pb-2 text-[11px] max-sm:hidden">
        <Kbd>Alt</Kbd>+<Kbd>1–9</Kbd> jumps to a tile · <Kbd>Alt</Kbd>+<Kbd>Enter</Kbd> maximises · <Kbd>Esc</Kbd> restores · drag a header to rearrange. Closing a tile leaves its agent running.
      </p>
    </AppShell>
  );
}
