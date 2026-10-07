import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useLocation } from "wouter";
import { Bell, BellOff, ExternalLink, FileSearch, Play, Plus, RotateCw, Search, SendHorizontal, SquareTerminal, X } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { act, ask, getJson } from "@/lib/api";
import { CAN_ACT, withToken } from "@/lib/config";
import { money, shortPath } from "@/lib/format";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Kbd } from "@/components/ui/kbd";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { AppShell } from "@/components/app-shell";
import { StateDot } from "@/components/status";
import { dotOfTab } from "@/lib/status";
import { TerminalFrame } from "@/components/terminal";
import { popOut, type Terminal } from "@/lib/terminal";
import { useSessions, useStored, useTabs, useTick } from "@/hooks/use-live";
import type { Session, Tab } from "@/lib/types";
import { SessionRow, WantingRow } from "./rows";
import { Quota } from "./quota";

type Sort = "recent" | "cost" | "tokens" | "context";
// Every order is descending: each is a "which is the most" question.
const share = (s: Session) => (s.context?.max ? s.context.used / s.context.max : -1);
const ORDER: Record<Sort, (a: Session, b: Session) => number> = {
  recent: (a, b) => String(b.last_active).localeCompare(String(a.last_active)),
  cost: (a, b) => (Number(b.cost?.total) || 0) - (Number(a.cost?.total) || 0),
  tokens: (a, b) => (Number(b.tokens?.total) || 0) - (Number(a.tokens?.total) || 0),
  // No window reported sorts last, not as empty.
  context: (a, b) => share(b) - share(a),
};

const matches = (s: Session, needle: string) => {
  if (!needle) return true;
  const hay = [s.project, s.title, s.model, s.harness, s.branch, s.provider, s.session_id, s.user, s.state].filter(Boolean).join(" ").toLowerCase();
  return needle.split(/\s+/).filter(Boolean).every((w) => hay.includes(w));
};

const typing = (t: EventTarget | null) =>
  t instanceof HTMLElement && (t.isContentEditable || /^(INPUT|SELECT|TEXTAREA)$/.test(t.tagName));

type Hit = { session_id: string; snippet?: string; project?: string; title?: string; provider?: string; model?: string };

// What the reader chose, kept across reloads. Not the bulk selection — a mark
// that survived a reload could act on a session nobody remembers choosing —
// and not the bell, whose permission belongs to the browser.
type Prefs = { filter: string; sort: Sort; running: boolean; pins: string[]; dismissed: Record<string, string> };
const DEFAULT_PREFS: Prefs = { filter: "", sort: "recent", running: false, pins: [], dismissed: {} };

function readSeen(): Record<string, { at?: number }> {
  try {
    return JSON.parse(localStorage.getItem("cctop-seen") || "{}") || {};
  } catch {
    return {};
  }
}

export function DashboardPage() {
  const { sessions, live } = useSessions();
  const [prefs, setPrefs] = useStored<Prefs>("cctop-dash", DEFAULT_PREFS, (v): v is Prefs => !!v && typeof v === "object");
  // Held across renders so `p.pins` and the rest keep their identity until the
  // stored prefs change, which is what the memos below key on.
  const p = useMemo(() => ({ ...DEFAULT_PREFS, ...prefs }), [prefs]);
  const update = (patch: Partial<Prefs>) => setPrefs({ ...p, ...patch });
  const [filterText, setFilterText] = useState(p.filter);
  const filter = filterText.trim().toLowerCase();
  const [hits, setHits] = useState<{ q: string; map: Map<string, string> }>({ q: "", map: new Map() });
  const [find, setFind] = useState<{ q: string; hits: Hit[] | null; error?: string } | null>(null);
  const [pickedRaw, setPicked] = useState<Set<string>>(new Set());
  const [selId, setSelId] = useState<string | null>(null);
  const [notify, setNotify] = useState(false);
  const filterRef = useRef<HTMLInputElement>(null);
  const [, navigate] = useLocation();
  useTick();

  // The transcript is asked once typing pauses, for a query long enough to
  // mean something; hits are gated on the query that produced them.
  useEffect(() => {
    if (filter.length < 3 || filter === hits.q) return;
    const t = setTimeout(async () => {
      try {
        const data = await getJson<{ hits?: Hit[] } | Hit[]>("/api/search", { q: filter });
        const list = Array.isArray(data) ? data : data.hits ?? [];
        const map = new Map<string, string>();
        for (const h of list) if (h?.session_id && h.snippet && !map.has(h.session_id)) map.set(h.session_id, String(h.snippet));
        setHits({ q: filter, map });
      } catch {
        /* no search route, no extra rows */
      }
    }, 400);
    return () => clearTimeout(t);
  }, [filter, hits.q]);

  const pins = useMemo(() => new Set(p.pins), [p.pins]);
  const seen = readSeen();
  const all = useMemo(() => sessions ?? [], [sessions]);
  const byId = useMemo(() => new Map(all.map((s) => [s.session_id, s])), [all]);
  const searchHits = hits.q === filter ? hits.map : new Map<string, string>();

  const shown = all
    .filter((s) => matches(s, filter) || searchHits.has(s.session_id))
    .filter((s) => !p.running || s.running)
    .sort(ORDER[p.sort] ?? ORDER.recent);
  // Live sessions only, and a dismissal holds only for the state it was made
  // in: a session that starts wanting something new is back.
  const wanting = shown.filter((s) => s.running && (s.state === "waiting" || s.state === "asking" || s.state === "error") && p.dismissed[s.session_id] !== s.state);
  const rest = shown.filter((s) => !wanting.includes(s)).sort((a, b) => Number(pins.has(b.session_id)) - Number(pins.has(a.session_id)));

  // The tab title announces the count: the page cannot ring a bell from a
  // background tab, and the title asks no permission.
  useEffect(() => {
    document.title = wanting.length ? `(${wanting.length}) cctop` : "cctop";
  }, [wanting.length]);

  // A desktop notification on the transition into waiting or asking — not for
  // a session already waiting when the page opened.
  const prev = useRef<Map<string, string> | null>(null);
  useEffect(() => {
    if (!sessions) return;
    if (prev.current && notify) {
      for (const s of sessions) {
        const was = prev.current.get(s.session_id);
        const body = (s.project || s.session_id.slice(0, 8)) + (s.title ? " · " + s.title : "");
        if (was && was !== "asking" && s.state === "asking") new Notification("Needs permission", { body, tag: s.session_id });
        if (was && was !== "waiting" && s.state === "waiting") new Notification("Waiting on you", { body, tag: s.session_id });
      }
    }
    prev.current = new Map(sessions.map((s) => [s.session_id, s.state]));
  }, [sessions, notify]);

  // Marks hold while their sessions exist, not while they are shown: a mark
  // whose session has gone is dropped here rather than acted on.
  const picked = useMemo(
    () => (sessions ? new Set([...pickedRaw].filter((id) => byId.has(id))) : pickedRaw),
    [pickedRaw, byId, sessions],
  );

  const togglePick = useCallback((id: string) => setPicked((cur) => {
    const next = new Set(cur);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    return next;
  }), []);

  const openFind = async () => {
    const q = filterText.trim();
    if (!q) return filterRef.current?.focus();
    if (q.length < 3) return setFind({ q, hits: [], error: "Three characters or more to search transcripts." });
    setFind({ q, hits: null });
    try {
      const data = await getJson<{ hits?: Hit[] } | Hit[]>("/api/search", { q });
      setFind({ q, hits: Array.isArray(data) ? data : data.hits ?? [] });
    } catch (e) {
      setFind({ q, hits: [], error: String((e as Error).message || e) });
    }
  };

  const toggleNotify = async () => {
    if (notify) return setNotify(false);
    if (!("Notification" in window)) return toast.error("This browser has no notifications");
    // Inside the click: a permission prompt no gesture asked for is refused.
    const granted = await Notification.requestPermission();
    setNotify(granted === "granted");
    if (granted !== "granted") toast.error("The browser refused notification permission for this page");
  };

  // --- keys ------------------------------------------------------------------
  const listed = find ? [] : [...wanting, ...rest];
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (e.key === "/" && !typing(e.target)) {
        e.preventDefault();
        filterRef.current?.focus();
        filterRef.current?.select();
        return;
      }
      if (typing(e.target)) return;
      const move = (dir: number) => {
        e.preventDefault();
        if (!listed.length) return;
        const at = listed.findIndex((s) => s.session_id === selId);
        const next = at < 0 ? (dir > 0 ? listed[0] : listed[listed.length - 1]) : listed[Math.min(listed.length - 1, Math.max(0, at + dir))];
        setSelId(next.session_id);
        document.querySelector(`[data-id="${CSS.escape(next.session_id)}"]`)?.scrollIntoView({ block: "nearest" });
      };
      if (e.key === "j" || e.key === "ArrowDown") move(1);
      else if (e.key === "k" || e.key === "ArrowUp") move(-1);
      else if (e.key === "Enter" && selId && !/^(A|BUTTON)$/.test((e.target as HTMLElement).tagName)) {
        e.preventDefault();
        navigate("/session/" + encodeURIComponent(selId));
      } else if (e.key === "p" && selId) {
        update({ pins: pins.has(selId) ? p.pins.filter((x) => x !== selId) : [...p.pins, selId] });
      } else if (e.key === "x" && selId && CAN_ACT) togglePick(selId);
      else if (e.key === "Escape") {
        if (find) setFind(null);
        else setSelId(null);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });

  const today = all.reduce((sum, s) => sum + (s.cost?.today || 0), 0);
  // Rolling, as the TUI's $/1H is: the clock hour reads $0 a minute past :00.
  const hour = all.reduce((sum, s) => sum + (s.cost?.last_hour || 0), 0);
  const running = all.filter((s) => s.running).length;
  const isFresh = (s: Session) => {
    const at = Number(seen[s.session_id]?.at);
    return isFinite(at) && at > 0 && Date.parse(s.last_active ?? "") > at;
  };

  return (
    <AppShell
      right={
        <div className="hidden items-baseline gap-4 font-mono text-sm md:flex">
          <span><b className="font-semibold">{money(today)}</b> <span className="text-muted-foreground font-sans text-[11px] tracking-wider uppercase">today</span></span>
          <span><b className="font-semibold">{money(hour)}</b> <span className="text-muted-foreground font-sans text-[11px] tracking-wider uppercase">last 60 min</span></span>
        </div>
      }
    >
      <div className="shrink-0 space-y-2.5 px-3 pt-3 sm:px-4">
        <Quota />
        <HostBanners />
        <Tabs />
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative min-w-52 flex-1 max-sm:basis-full">
            <Search className="text-muted-foreground pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2" />
            <Input
              ref={filterRef}
              value={filterText}
              onChange={(e) => {
                setFilterText(e.target.value);
                update({ filter: e.target.value });
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  openFind();
                } else if (e.key === "Escape") {
                  setFilterText("");
                  update({ filter: "" });
                  (e.target as HTMLInputElement).blur();
                }
              }}
              placeholder="Filter on project, model, branch, title…"
              className="h-8 pl-8"
              spellCheck={false}
              title="Type to filter · Enter to search every transcript"
            />
          </div>
          <Button variant="outline" size="sm" onClick={openFind} title="Search every transcript for the filter text">
            <FileSearch /> Search transcripts
          </Button>
          <Button variant={p.running ? "secondary" : "outline"} size="sm" aria-pressed={p.running} onClick={() => update({ running: !p.running })}>
            <Play /> Running
          </Button>
          <Button variant={notify ? "secondary" : "outline"} size="sm" aria-pressed={notify} onClick={toggleNotify} title="Notify when a session starts waiting on you">
            {notify ? <Bell /> : <BellOff />} Notify
          </Button>
          <Select value={p.sort} onValueChange={(v) => update({ sort: v as Sort })}>
            <SelectTrigger size="sm" className="w-28" aria-label="Order the table">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="recent">Recent</SelectItem>
              <SelectItem value="cost">Cost</SelectItem>
              <SelectItem value="tokens">Tokens</SelectItem>
              <SelectItem value="context">Context</SelectItem>
            </SelectContent>
          </Select>
          <Launcher />
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-3 pt-3 pb-4 sm:px-4">
        {find ? (
          <FindResults find={find} byId={byId} onClose={() => setFind(null)} />
        ) : (
          <>
            {wanting.length > 0 && (
              <section className="border-warning/60 bg-card mb-4 overflow-hidden rounded-xl border shadow-xs">
                <h2 className="text-warning px-4 pt-3 pb-1 text-xs font-semibold tracking-wider uppercase">
                  {wanting.length === 1 ? "1 session needs you" : `${wanting.length} sessions need you`}
                </h2>
                {wanting.map((s) => (
                  <WantingRow
                    key={s.session_id}
                    s={s}
                    picked={picked.has(s.session_id)}
                    onPick={togglePick}
                    onDismiss={() => update({ dismissed: { ...p.dismissed, [s.session_id]: s.state } })}
                  />
                ))}
              </section>
            )}
            <section className="bg-card overflow-hidden rounded-xl border shadow-xs" role="list">
              {sessions === null ? (
                <div className="text-muted-foreground p-10 text-center text-sm">Waiting for the first refresh…</div>
              ) : !shown.length ? (
                <div className="text-muted-foreground p-10 text-center text-sm" data-rendered="">
                  {all.length ? "Nothing matches that filter." : "No sessions yet."}
                </div>
              ) : (
                rest.map((s) => (
                  <SessionRow
                    key={s.session_id}
                    s={s}
                    selected={selId === s.session_id}
                    picked={picked.has(s.session_id)}
                    pinned={pins.has(s.session_id)}
                    fresh={isFresh(s)}
                    snippet={searchHits.get(s.session_id)}
                    onPick={togglePick}
                  />
                ))
              )}
            </section>
          </>
        )}
      </div>

      {CAN_ACT && picked.size > 0 && <BulkBar picked={picked} byId={byId} onClear={() => setPicked(new Set())} />}

      <footer className="text-muted-foreground flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 border-t px-3 py-1.5 text-xs sm:px-4">
        <span className="flex items-center gap-1.5">
          <StateDot state={live ? "working" : sessions ? "error" : "idle"} />
          {live ? "live" : sessions ? "reconnecting…" : "connecting…"}
        </span>
        <span>
          {all.length} sessions · {running} running{shown.length !== all.length ? ` · ${shown.length} shown` : ""}
        </span>
        <span className="flex-1" />
        <span className="hidden lg:inline">
          <Kbd>j</Kbd>/<Kbd>k</Kbd> select · <Kbd>Enter</Kbd> opens · <Kbd>p</Kbd> pins{CAN_ACT && <> · <Kbd>x</Kbd> marks</>} · <Kbd>/</Kbd> filters
        </span>
        <a className="hover:text-foreground underline-offset-2 hover:underline" href={withToken("/insight/optimize")}>optimize</a>
        <a className="hover:text-foreground underline-offset-2 hover:underline" href={withToken("/insight/compare")}>compare</a>
      </footer>
    </AppShell>
  );
}

// Hosts read over ssh that could not be, said once.
function HostBanners() {
  const [failed, setFailed] = useState<[string, string][]>([]);
  useEffect(() => {
    getJson<[string, string][]>("/api/hosts").then((f) => setFailed(Array.isArray(f) ? f : []), () => {});
  }, []);
  return (
    <>
      {failed.map(([host, why]) => (
        <div key={host} className="border-destructive/40 bg-destructive/5 text-destructive rounded-md border px-3 py-1.5 text-xs">
          {host} could not be read: {why}
        </div>
      ))}
    </>
  );
}

// The TUI's tab bar, read from rmux: every agent cctop has open. A tab opens
// its terminal in a drawer under the bar.
function Tabs() {
  const { tabs } = useTabs(5000);
  const [open, setOpen] = useState<{ tab: Tab; term?: Terminal; error?: string } | null>(null);
  const mint = (name: string, fresh = false): Promise<Terminal> =>
    ask("/api/tab/" + encodeURIComponent(name) + "/terminal", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ origin: location.origin, fresh }),
    }).then((r) => r.json());
  const toggle = async (t: Tab) => {
    if (open?.tab.name === t.name) return setOpen(null);
    setOpen({ tab: t });
    try {
      const res = await ask("/api/tab/" + encodeURIComponent(t.name) + "/terminal", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ origin: location.origin }),
      });
      const term = await res.json();
      setOpen((o) => (o?.tab.name === t.name ? { tab: t, term } : o));
    } catch (e) {
      setOpen((o) => (o?.tab.name === t.name ? { tab: t, error: String((e as Error).message || e) } : o));
    }
  };
  if (!tabs?.length) return null;
  return (
    <>
      <nav className="flex flex-wrap gap-1.5" aria-label="Open tabs">
        {tabs.map((t) => (
          <Button
            key={t.name}
            variant={open?.tab.name === t.name ? "secondary" : "outline"}
            size="sm"
            className="max-w-64 rounded-full"
            aria-pressed={open?.tab.name === t.name}
            disabled={!CAN_ACT}
            onClick={() => toggle(t)}
            title={(t.cwd ? shortPath(t.cwd) + " · " : "") + (CAN_ACT ? "open this tab's terminal" : "this link cannot open terminals")}
          >
            <StateDot state={dotOfTab(t.state)} />
            <span className="truncate">{t.label}</span>
          </Button>
        ))}
      </nav>
      {open && (
        <div className="bg-card overflow-hidden rounded-xl border shadow-xs">
          <div className="text-muted-foreground flex items-center gap-2 border-b px-3 py-1.5 text-xs">
            <SquareTerminal className="size-3.5" />
            {open.tab.label}
            {open.tab.cwd && <span>· {shortPath(open.tab.cwd)}</span>}
            <span className="flex-1" />
            {open.term && (
              <button
                type="button"
                className="hover:text-foreground inline-flex items-center gap-1"
                title="Open in a window of its own — the drawer closes, since a share admits one browser"
                onClick={() => {
                  const tab = open.tab;
                  popOut({ key: tab.name, title: tab.label + " — cctop", release: () => setOpen(null), mint: () => mint(tab.name, true), onClosed: () => {} });
                }}
              >
                <ExternalLink className="size-3" /> pop out
              </button>
            )}
            <Button variant="ghost" size="icon-xs" onClick={() => setOpen(null)} aria-label="Close the terminal">
              <X />
            </Button>
          </div>
          <div className="bg-terminal flex h-[min(60vh,640px)]">
            {open.term ? (
              <TerminalFrame
                url={open.term.url}
                name={open.term.name ?? open.tab.name}
                title="Terminal"
                onBroken={() => {
                  const tab = open.tab;
                  mint(tab.name, true).then((term) => setOpen((o) => (o?.tab.name === tab.name && o.term?.url !== term.url ? { tab, term } : o)), () => {});
                }}
              />
            ) : (
              <div className={cn("m-auto text-sm", open.error ? "text-red-400" : "text-neutral-400")}>{open.error || "Opening this tab's terminal…"}</div>
            )}
          </div>
        </div>
      )}
    </>
  );
}

// Which transcripts hold these words at all — every session the server knows,
// not only the rows on screen. A hit opens the chat with its find box seeded.
function FindResults({ find, byId, onClose }: { find: { q: string; hits: Hit[] | null; error?: string }; byId: Map<string, Session>; onClose: () => void }) {
  return (
    <section className="bg-card relative overflow-hidden rounded-xl border shadow-xs" data-rendered="">
      <h2 className="text-muted-foreground px-4 pt-3 pb-1 text-xs font-semibold tracking-wider uppercase">Transcript search — “{find.q}”</h2>
      <Button variant="ghost" size="icon-xs" className="absolute top-2 right-2" onClick={onClose} aria-label="Back to the session table">
        <X />
      </Button>
      {find.error ? (
        <div className="text-muted-foreground p-10 text-center text-sm">{find.error}</div>
      ) : find.hits === null ? (
        <div className="text-muted-foreground p-10 text-center text-sm">Searching every transcript…</div>
      ) : !find.hits.length ? (
        <div className="text-muted-foreground p-10 text-center text-sm">No transcript mentions that.</div>
      ) : (
        find.hits.map((h) => {
          const s = byId.get(h.session_id) ?? ({ session_id: h.session_id, provider: h.provider ?? "", state: "idle", running: false, project: h.project, title: h.title, model: h.model } as Session);
          return (
            <SessionRow
              key={h.session_id}
              s={s}
              selected={false}
              picked={false}
              pinned={false}
              fresh={false}
              snippet={h.snippet}
              href={"/session/" + encodeURIComponent(h.session_id) + "?find=" + encodeURIComponent(find.q)}
            />
          );
        })
      )}
    </section>
  );
}

// One verb against every marked session, one request at a time — these type
// at real terminals and start real agents. The summary names the failures.
function BulkBar({ picked, byId, onClear }: { picked: Set<string>; byId: Map<string, Session>; onClear: () => void }) {
  const [text, setText] = useState("");
  const [composing, setComposing] = useState(false);
  const [busy, setBusy] = useState(false);
  const run = async (verb: "send" | "resume") => {
    setBusy(true);
    const failed: string[] = [];
    let done = 0;
    for (const id of picked) {
      try {
        await act(verb, id, verb === "send" ? { text } : {});
        done++;
      } catch (e) {
        failed.push((shortPath(byId.get(id)?.project) || id.slice(0, 8)) + " — " + String((e as Error).message || e));
      }
    }
    setBusy(false);
    const summary = (verb === "send" ? "Sent to " : "Resumed ") + done;
    if (failed.length) toast.error(summary + " · failed: " + failed.map((f) => f.split(" — ")[0]).join(", "), { description: failed.join("\n") });
    else {
      toast.success(summary);
      if (verb === "send") {
        setText("");
        setComposing(false);
      }
    }
  };
  return (
    <div className="bg-background flex shrink-0 flex-wrap items-center gap-2 border-t px-3 py-2 text-sm sm:px-4">
      <span className="font-medium">{picked.size} selected</span>
      <Button size="sm" variant="outline" disabled={busy} onClick={() => setComposing(!composing)}>
        <SendHorizontal /> Send…
      </Button>
      <Button size="sm" variant="outline" disabled={busy} onClick={() => run("resume")}>
        <RotateCw /> Resume
      </Button>
      <Button size="sm" variant="ghost" disabled={busy} onClick={onClear}>
        Clear
      </Button>
      {composing && (
        <form
          className="flex min-w-64 flex-1 gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (text.trim()) run("send");
          }}
        >
          <Input autoFocus value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === "Escape" && setComposing(false)} maxLength={4000} placeholder="One line, sent to each selected session" className="h-8" />
          <Button type="submit" size="sm" disabled={busy || !text.trim()}>
            Send
          </Button>
        </form>
      )}
    </div>
  );
}

// Starting a fresh agent from the page — the TUI's launcher, carried to the
// one place a phone can reach. Only where this run can act.
function Launcher() {
  const [agents, setAgents] = useState<string[]>([]);
  const [open, setOpen] = useState(false);
  const [agent, setAgent] = useState("");
  const [cwd, setCwd] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!CAN_ACT) return;
    getJson<{ agents?: string[]; actions?: boolean }>("/api/agents").then((r) => {
      if (r.actions === false) return;
      setAgents(r.agents ?? []);
      setAgent((r.agents ?? [])[0] ?? "");
    }, () => {});
  }, []);
  if (!agents.length) return null;
  const start = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      const res = await ask("/api/launch", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ agent, cwd: cwd.trim() }) });
      const done = await res.json().catch(() => ({}));
      toast.success(done.message || "Started");
      setOpen(false);
    } catch (err) {
      toast.error(String((err as Error).message || err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <Button size="sm" onClick={() => setOpen(true)}>
        <Plus /> New agent
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="sm:max-w-md">
          <form onSubmit={start} className="grid gap-4">
            <DialogHeader>
              <DialogTitle>Start an agent</DialogTitle>
              <DialogDescription>In cctop's multiplexer, so it shows up as a tab and in the workspace.</DialogDescription>
            </DialogHeader>
            <div className="grid gap-1.5">
              <label className="text-sm font-medium" htmlFor="launch-agent">Agent</label>
              <Select value={agent} onValueChange={setAgent}>
                <SelectTrigger id="launch-agent" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {agents.map((a) => (
                    <SelectItem key={a} value={a}>{a}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-1.5">
              <label className="text-sm font-medium" htmlFor="launch-cwd">Directory</label>
              <Input id="launch-cwd" value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder="~ (the default)" spellCheck={false} autoComplete="off" />
            </div>
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setOpen(false)}>Cancel</Button>
              <Button type="submit" disabled={busy || !agent}>
                <Play /> Start
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
