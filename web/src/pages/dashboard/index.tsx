import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useLocation } from "wouter";
import { Bell, BellOff, FileSearch, Play, Search } from "lucide-react";
import { CAN_ACT, withToken } from "@/lib/config";
import { money } from "@/lib/format";
import { readSeen } from "@/lib/seen";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Kbd } from "@/components/ui/kbd";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { AppShell } from "@/components/app-shell";
import { StateDot } from "@/components/status";
import { POLL_EVERY, useSessions, useStored, useTabs, useTick } from "@/hooks/use-live";
import type { Session } from "@/lib/types";
import { SessionRow, WantingRow } from "./rows";
import { Quota } from "./quota";
import { ProviderStatus } from "./provider-status";
import { Launcher } from "./launcher";
import { HostBanners } from "./host-banners";
import { Tabs } from "./tabs";
import { FindResults, type Find } from "./find-results";
import { BulkBar } from "./bulk-bar";
import { searchTranscripts, useTranscriptSearch } from "./use-transcript-search";
import { useWantingNotifications } from "./use-wanting-notifications";

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

// What the reader chose, kept across reloads. Not the bulk selection — a mark
// that survived a reload could act on a session nobody remembers choosing —
// and not the bell, whose permission belongs to the browser.
type Prefs = { filter: string; sort: Sort; running: boolean; pins: string[]; dismissed: Record<string, string> };
const DEFAULT_PREFS: Prefs = { filter: "", sort: "recent", running: false, pins: [], dismissed: {} };

export function DashboardPage() {
  const { sessions, feed } = useSessions();
  // Here rather than in the tab bar, so the palette lists the same tabs.
  const { tabs } = useTabs(5000);
  const [prefs, setPrefs] = useStored<Prefs>("cctop-dash", DEFAULT_PREFS, (v): v is Prefs => !!v && typeof v === "object");
  // Held across renders so `p.pins` and the rest keep their identity until the
  // stored prefs change, which is what the memos below key on.
  const p = useMemo(() => ({ ...DEFAULT_PREFS, ...prefs }), [prefs]);
  const update = (patch: Partial<Prefs>) => setPrefs({ ...p, ...patch });
  const [filterText, setFilterText] = useState(p.filter);
  const filter = filterText.trim().toLowerCase();
  const searchHits = useTranscriptSearch(filter);
  const [find, setFind] = useState<Find | null>(null);
  const [pickedRaw, setPicked] = useState<Set<string>>(new Set());
  const [selId, setSelId] = useState<string | null>(null);
  const { notify, toggle: toggleNotify } = useWantingNotifications(sessions);
  const filterRef = useRef<HTMLInputElement>(null);
  const [, navigate] = useLocation();
  const now = useTick();

  const pins = useMemo(() => new Set(p.pins), [p.pins]);
  const seen = readSeen();
  const all = useMemo(() => sessions ?? [], [sessions]);
  const byId = useMemo(() => new Map(all.map((s) => [s.session_id, s])), [all]);

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
      setFind({ q, hits: await searchTranscripts(q) });
    } catch (e) {
      setFind({ q, hits: [], error: String((e as Error).message || e) });
    }
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
    const at = seen[s.session_id]?.at;
    return at !== undefined && at > 0 && Date.parse(s.last_active ?? "") > at;
  };

  return (
    <AppShell
      live={{ sessions, tabs }}
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
        <ProviderStatus />
        <Tabs tabs={tabs} />
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
          <FindResults find={find} byId={byId} now={now} onClose={() => setFind(null)} />
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
                    now={now}
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
                    now={now}
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
        <span
          className="flex items-center gap-1.5"
          title={
            feed === "polling"
              ? `The live stream is not getting through — a quick tunnel does not carry it — so the table is fetched every ${POLL_EVERY / 1000} seconds instead. It goes back to live on its own.`
              : undefined
          }
        >
          <StateDot state={feed === "live" ? "working" : feed === "polling" ? "waiting" : sessions ? "error" : "idle"} />
          {feed === "live" ? "live" : feed === "polling" ? "polling" : sessions ? "reconnecting…" : "connecting…"}
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
