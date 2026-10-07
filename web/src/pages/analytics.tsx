import { useEffect, useMemo, useState } from "react";
import { Link } from "wouter";
import { cn } from "@/lib/utils";
import { getJson } from "@/lib/api";
import { ago, money, shortModel, shortPath, tokens } from "@/lib/format";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { AppShell } from "@/components/app-shell";
import { DataTable, Panel, Section, Stat } from "@/components/section";
import { useStored, useTick } from "@/hooks/use-live";

// The fleet as figures: what sessions cost, when they ran, and who, what and
// where they belong to. Every section reads the same filtered set, so the page
// tells one story. All filtering is client-side: the whole fleet already
// arrives in one payload, and re-asking the server per filter change would add
// a route for nothing.

type A = {
  id: string; provider: string; label?: string; project?: string; project_name?: string; title?: string;
  model?: string; models?: string[]; user?: string; account?: string; host?: string;
  started?: string; last_active?: string; running?: boolean; state?: string;
  cost?: number | null; cost_available?: boolean; cost_included?: boolean;
  tokens?: { total?: number }; tools?: number; tool_errors?: number | null; writes?: string[];
  by_day?: Record<string, Record<string, number>>; tokens_by_day?: Record<string, Record<string, number>>;
  by_hour?: Record<string, Record<string, number>>; tokens_by_hour?: Record<string, Record<string, number>>;
};
type Data = { sessions: A[]; plan?: string; generated?: string };

const pad2 = (n: number) => String(n).padStart(2, "0");
const dayKey = (d: Date) => `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
const hourKey = (d: Date) => dayKey(d) + "T" + pad2(d.getHours());
// A day key as a local date: `new Date("2026-09-15")` would be UTC midnight,
// the previous evening for anyone west of Greenwich.
const parseDay = (day: string) => {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, m - 1, d);
};
const dayLabel = (day: string) => parseDay(day).toLocaleDateString(undefined, { month: "short", day: "numeric" });
function daysBetween(a: string, b: string): string[] {
  const out: string[] = [];
  const end = parseDay(b);
  for (let d = parseDay(a); d <= end && out.length <= 400; d.setDate(d.getDate() + 1)) out.push(dayKey(d));
  return out;
}

// Whose session: the person, the login, the machine, or nobody in particular.
const who = (s: A) => s.user || s.account || s.host || "local";
const projKey = (s: A) => s.project_name || s.project || "—";
// Three distinct claims: the provider records cost at all, the plan bundles
// it (the figure is a retail equivalent), and a number was recorded.
const measurable = (s: A) => !!s.cost_available && !s.cost_included && typeof s.cost === "number";
const hasRecorded = (s: A) => !!s.cost_available && typeof s.cost === "number";
const count = (v: unknown) => (Number(v) || 0).toLocaleString();
const PALETTE = ["var(--chart-1)", "var(--chart-2)", "var(--chart-3)", "var(--chart-4)", "var(--chart-5)", "var(--muted-foreground)"];

const RANGES = { "24h": 864e5, "7d": 7 * 864e5, "30d": 30 * 864e5, all: Infinity } as const;
type Range = keyof typeof RANGES;
type Sel = { provider: string; project: string; model: string; who: string; range: Range };
const ALL = "__all";

type Series = { name: string; color: string; byDay: Map<string, number> };

function StackedDays({ days, series, fmt, label }: { days: string[]; series: Series[]; fmt: (v: number) => string; label: string }) {
  const W = 920, H = 210, L = 52, B = 30, T = 10, R = 8;
  const totals = days.map((d) => series.reduce((a, x) => a + (x.byDay.get(d) || 0), 0));
  const max = Math.max(...totals, 1e-9);
  const y = (v: number) => T + (1 - v / max) * (H - T - B);
  const slot = (W - L - R) / days.length;
  const bw = Math.max(1, Math.min(34, slot - Math.max(1, slot * 0.18)));
  return (
    <Panel className="overflow-x-auto">
      <svg viewBox={`0 0 ${W} ${H}`} className="block h-auto w-full min-w-[600px]" role="img" aria-label={label}>
        {[0, 0.5, 1].map((f) => (
          <g key={f}>
            <line x1={L} x2={W - R} y1={y(max * f)} y2={y(max * f)} stroke="var(--border)" />
            <text x={L - 6} y={y(max * f) + 3} textAnchor="end" className="fill-muted-foreground font-mono text-[10px]">{fmt(max * f)}</text>
          </g>
        ))}
        <text x={L} y={H - 8} className="fill-muted-foreground font-mono text-[10px]">{dayLabel(days[0])}</text>
        <text x={(L + W - R) / 2} y={H - 8} textAnchor="middle" className="fill-muted-foreground font-mono text-[10px]">{days.length} {days.length === 1 ? "day" : "days"}</text>
        <text x={W - R} y={H - 8} textAnchor="end" className="fill-muted-foreground font-mono text-[10px]">{dayLabel(days[days.length - 1])}</text>
        {days.map((day, i) => {
          let base = y(0);
          const cx = L + i * slot + (slot - bw) / 2;
          return series.map((ser) => {
            const v = ser.byDay.get(day) || 0;
            if (v <= 0) return null;
            const h = Math.max(0, y(0) - y(v));
            base -= h;
            return (
              <rect key={day + ser.name} x={cx} y={base} width={bw} height={h} rx={1.5} fill={ser.color} className="transition-opacity hover:opacity-80">
                <title>{`${dayLabel(day)} · ${ser.name} — ${fmt(v)}`}</title>
              </rect>
            );
          });
        })}
      </svg>
      <div className="mt-3 flex flex-wrap gap-x-5 gap-y-2 text-xs">
        {series.map((ser) => {
          const total = [...ser.byDay.values()].reduce((a, v) => a + v, 0);
          return total > 0 ? (
            <span key={ser.name} className="flex items-center gap-1.5">
              <span className="size-2.5 rounded-sm" style={{ background: ser.color }} />
              {ser.name}
              <span className="text-muted-foreground font-mono">{fmt(total)}</span>
            </span>
          ) : null;
        })}
      </div>
    </Panel>
  );
}

const Empty = ({ children }: { children: React.ReactNode }) => <Panel className="text-muted-foreground p-8 text-center text-sm">{children}</Panel>;

export function AnalyticsPage() {
  const [data, setData] = useState<Data | null>(null);
  const [error, setError] = useState("");
  const [updated, setUpdated] = useState(0);
  const [sel, setSel] = useStored<Sel>("cctop-analytics", { provider: "", project: "", model: "", who: "", range: "30d" });
  const now = useTick(10000);

  useEffect(() => {
    let live = true;
    const refresh = async () => {
      try {
        const d = await getJson<Data>("/api/analytics");
        if (!live) return;
        if (!Array.isArray(d.sessions)) d.sessions = [];
        setData(d);
        const t = Date.parse(d.generated ?? "");
        setUpdated(isFinite(t) ? t : Date.now());
        setError("");
      } catch (e) {
        if (live) setError(String((e as Error).message || e));
      }
    };
    refresh();
    const t = setInterval(refresh, 15000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, []);

  const range = (Object.hasOwn(RANGES, sel.range) ? sel.range : "30d") as Range;
  const ms = RANGES[range];
  const cut = isFinite(ms) ? new Date(now - ms) : null;
  const MIN_DAY = cut ? dayKey(cut) : "";
  const MIN_HOUR = cut ? hourKey(cut) : "";

  // The choices read the unfiltered fleet: filtering must not shrink the
  // options that produced it.
  const options = useMemo(() => {
    const providers = new Map<string, string>();
    const projects = new Set<string>(), models = new Set<string>(), whos = new Set<string>();
    for (const s of data?.sessions ?? []) {
      if (s.provider) providers.set(s.provider, s.label || s.provider);
      projects.add(projKey(s));
      if (s.model) models.add(s.model);
      for (const m of s.models ?? []) models.add(m);
      whos.add(who(s));
    }
    return {
      provider: [...providers.entries()].sort((a, b) => a[1].localeCompare(b[1])),
      project: [...projects].sort().map((p) => [p, p] as [string, string]),
      model: [...models].sort().map((m) => [m, m] as [string, string]),
      who: [...whos].sort().map((w) => [w, w] as [string, string]),
    };
  }, [data]);
  // A stored choice the fleet no longer offers reads as "all".
  const eff = (k: "provider" | "project" | "model" | "who") => (options[k].some(([v]) => v === sel[k]) ? sel[k] : "");
  const f = { provider: eff("provider"), project: eff("project"), model: eff("model"), who: eff("who") };

  // A session is in range if it did something, or began, inside it.
  const list = (data?.sessions ?? []).filter((s) => {
    if (f.provider && s.provider !== f.provider) return false;
    if (f.project && projKey(s) !== f.project) return false;
    if (f.model && s.model !== f.model && !(s.models ?? []).includes(f.model)) return false;
    if (f.who && who(s) !== f.who) return false;
    if (!cut) return true;
    const last = Date.parse(s.last_active ?? ""), start = Date.parse(s.started ?? "");
    return (isFinite(last) && last >= cut.getTime()) || (isFinite(start) && start >= cut.getTime());
  });

  const today = dayKey(new Date(now));
  const domain = (present: Iterable<string>) => {
    let first: string | null = MIN_DAY || null;
    for (const d of present) if (!first || d < first) first = d;
    return !first || first > today ? [] : daysBetween(first, today);
  };
  const byDay = (src: A[], field: "by_day" | "tokens_by_day") => {
    const days = new Map<string, Map<string, number>>();
    const names = new Map<string, number>();
    for (const s of src) {
      const label = s.label || s.provider;
      for (const [day, m] of Object.entries(s[field] ?? {})) {
        if (day < MIN_DAY) continue;
        let row = days.get(day);
        if (!row) days.set(day, (row = new Map()));
        for (const v of Object.values(m ?? {})) {
          const n = Number(v) || 0;
          row.set(label, (row.get(label) || 0) + n);
          names.set(label, (names.get(label) || 0) + n);
        }
      }
    }
    const span = domain(days.keys());
    const series: Series[] = [...names.entries()].sort((a, b) => b[1] - a[1]).map(([name], i) => ({
      name,
      color: PALETTE[i % PALETTE.length],
      // Only the days the axis shows, so the legend's totals agree with it.
      byDay: new Map(span.map((d) => [d, days.get(d)?.get(name) || 0])),
    }));
    return { span, series, any: days.size > 0 };
  };

  const update = (k: keyof Sel, v: string) => setSel({ ...sel, [k]: v === ALL ? "" : v } as Sel);
  const filterSelect = (k: "provider" | "project" | "model" | "who", all: string) => (
    <Select value={f[k] || ALL} onValueChange={(v) => update(k, v)}>
      <SelectTrigger size="sm" className="max-w-56 min-w-32" aria-label={all}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={ALL}>{all}</SelectItem>
        {options[k].map(([v, label]) => (
          <SelectItem key={v} value={v}>{label}</SelectItem>
        ))}
      </SelectContent>
    </Select>
  );

  const stamp = updated ? `updated ${new Date(updated).toLocaleTimeString()} · ${ago(new Date(updated).toISOString())}` : "reading…";

  return (
    <AppShell right={<span className="text-muted-foreground hidden text-xs md:inline">{stamp}</span>}>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[1280px] px-3 pt-3 pb-8 sm:px-4" data-rendered={data || error ? "" : undefined}>
          {error && <div className="border-destructive/40 bg-destructive/5 text-destructive mb-3 rounded-md border px-3 py-1.5 text-xs">{error} Still trying.</div>}
          {!data ? (
            !error && <Empty>Reading the sessions…</Empty>
          ) : (
            <>
              <Kpis list={list} />
              <div className="mb-6 flex flex-wrap gap-2">
                {filterSelect("provider", "All providers")}
                {filterSelect("project", "All projects")}
                {filterSelect("model", "All models")}
                {filterSelect("who", "Who: all")}
                <Select value={range} onValueChange={(v) => update("range", v)}>
                  <SelectTrigger size="sm" className="w-36" aria-label="How far back">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="24h">Last 24h</SelectItem>
                    <SelectItem value="7d">Last 7 days</SelectItem>
                    <SelectItem value="30d">Last 30 days</SelectItem>
                    <SelectItem value="all">All time</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              {!list.length ? (
                <Empty>{data.sessions.length ? "No sessions match that filter and range." : "No sessions yet — when cctop has seen some, this page reads them."}</Empty>
              ) : (
                <>
                  <SpendChart list={list} byDay={byDay} />
                  <TokensChart list={list} byDay={byDay} />
                  <ActiveChart list={list} today={today} minDay={MIN_DAY} domain={domain} />
                  <Heat list={list} minHour={MIN_HOUR} />
                  <Files list={list} />
                  <Breakdowns list={list} />
                  <Sessions list={list} />
                </>
              )}
              <Footnotes list={list} plan={data.plan} />
            </>
          )}
        </div>
      </div>
    </AppShell>
  );
}

type ByDay = (src: A[], field: "by_day" | "tokens_by_day") => { span: string[]; series: Series[]; any: boolean };

function Kpis({ list }: { list: A[] }) {
  // "Spend" is money spent; a bundled session's retail equivalent is a
  // different claim and rides as the note.
  const spent = list.reduce((a, s) => a + (measurable(s) ? (s.cost as number) : 0), 0);
  const bundled = list.reduce((a, s) => a + (s.cost_available && s.cost_included && typeof s.cost === "number" ? s.cost : 0), 0);
  const anySpent = list.some(measurable);
  // tool_errors is null where a harness records none — not zero.
  const reporting = list.filter((s) => typeof s.tool_errors === "number");
  const errs = reporting.reduce((a, s) => a + (s.tool_errors as number), 0);
  const calls = reporting.reduce((a, s) => a + (Number(s.tools) || 0), 0);
  const rate = calls ? errs / calls : 0;
  return (
    <div className="mb-4 grid grid-cols-2 gap-2.5 sm:grid-cols-3 lg:grid-cols-6">
      <Stat label="Spend" value={anySpent ? money(spent) : "—"} note={bundled > 0 ? "+" + money(bundled) + " bundled by a plan" : anySpent ? undefined : "nothing measured"} />
      <Stat label="Sessions" value={count(list.length)} />
      <Stat label="Running now" value={count(list.filter((s) => s.running).length)} />
      <Stat label="Tokens" value={tokens(list.reduce((a, s) => a + (Number(s.tokens?.total) || 0), 0))} />
      <Stat label="Tool calls" value={count(list.reduce((a, s) => a + (Number(s.tools) || 0), 0))} />
      {reporting.length > 0 && (
        <Stat label="Tool errors" value={count(errs)} note={reporting.length < list.length ? "of the sessions that record them" : undefined} tone={rate >= 0.25 ? "bad" : rate >= 0.1 ? "warn" : null} />
      )}
    </div>
  );
}

// Bundled and unrecorded costs are not in this chart: a $0 drawn for them would
// be a claim the data does not make. The foot of the page says where they went.
function SpendChart({ list, byDay }: { list: A[]; byDay: ByDay }) {
  const src = list.filter((s) => s.cost_available && !s.cost_included);
  const { span, series, any } = byDay(src, "by_day");
  return (
    <Section title="Spend per day" note="What the filtered sessions spent each day, stacked by agent. Bundled and unrecorded costs are not in it — the foot of the page says which.">
      {!src.length ? <Empty>No measured spend in this set — every session's cost is bundled by a plan or not recorded at all.</Empty>
        : !span.length || !any ? <Empty>No per-day cost buckets recorded in this range.</Empty>
        : <StackedDays days={span} series={series} fmt={money} label="Spend per day, stacked by agent" />}
    </Section>
  );
}

function TokensChart({ list, byDay }: { list: A[]; byDay: ByDay }) {
  const { span, series, any } = byDay(list, "tokens_by_day");
  return (
    <Section title="Tokens per day" note="Input, output and cache traffic together, stacked by agent.">
      {!span.length || !any ? <Empty>No per-day token buckets recorded in this range.</Empty> : <StackedDays days={span} series={series} fmt={tokens} label="Tokens per day, stacked by agent" />}
    </Section>
  );
}

// A session counts on every day its [started, last_active] span covers.
function ActiveChart({ list, today, minDay, domain }: { list: A[]; today: string; minDay: string; domain: (d: Iterable<string>) => string[] }) {
  const covered = new Map<string, number>();
  const present: string[] = [];
  for (const s of list) {
    const a = Date.parse(s.started ?? ""), b = Date.parse(s.last_active ?? "");
    let from = isFinite(a) ? dayKey(new Date(a)) : isFinite(b) ? dayKey(new Date(b)) : "";
    let to = isFinite(b) ? dayKey(new Date(b)) : s.running ? today : from;
    if (!from || !to) continue;
    if (from > to) [from, to] = [to, from];
    if (to > today) to = today;
    for (const day of daysBetween(from, to)) {
      if (day < minDay || day > today) continue;
      covered.set(day, (covered.get(day) || 0) + 1);
      present.push(day);
    }
  }
  const span = minDay ? domain([minDay]) : domain(present);
  return (
    <Section title="Sessions active per day" note="A session counts on every day it spans, not only the day it started.">
      {!span.length ? <Empty>No session has a timestamp inside this range.</Empty>
        : <StackedDays days={span} series={[{ name: "sessions", color: "var(--chart-1)", byDay: covered }]} fmt={(v) => String(Math.round(v))} label="Sessions active per day" />}
    </Section>
  );
}

const DOWS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

// Tokens wherever any session records them; per-hour cost only when none do,
// and the heading says which — they are different claims.
function Heat({ list, minHour }: { list: A[]; minHour: string }) {
  const gather = (field: "tokens_by_hour" | "by_hour", src: A[]) => {
    const cells = Array.from({ length: 7 }, () => new Array(24).fill(0) as number[]);
    for (const s of src)
      for (const [key, m] of Object.entries(s[field] ?? {})) {
        if (key < minHour) continue;
        const h = Number(key.slice(11, 13));
        if (!(h >= 0 && h < 24)) continue;
        cells[(parseDay(key.slice(0, 10)).getDay() + 6) % 7][h] += Object.values(m ?? {}).reduce((a, v) => a + (Number(v) || 0), 0);
      }
    return cells;
  };
  let metric: "tokens" | "spend" = "tokens";
  let cells = gather("tokens_by_hour", list);
  let max = Math.max(...cells.flat());
  if (max <= 0) {
    metric = "spend";
    cells = gather("by_hour", list.filter((s) => s.cost_available));
    max = Math.max(...cells.flat());
  }
  const fmt = metric === "tokens" ? (v: number) => tokens(v) + " tok" : money;
  return (
    <Section
      title={metric === "tokens" ? "Activity heat" : "Spend heat"}
      note={metric === "tokens" ? "Tokens recorded in each hour of the week — when the fleet works, not how much of it." : "No per-hour token data in this set, so this shows recorded spend per hour of the week instead."}
    >
      {max <= 0 ? <Empty>No per-hour activity recorded in this range.</Empty> : (
        <Panel className="overflow-x-auto">
          <div className="grid min-w-[560px] grid-cols-[32px_repeat(24,minmax(10px,1fr))] items-center gap-0.5">
            <span />
            {Array.from({ length: 24 }, (_, h) => (
              <span key={h} className="text-muted-foreground text-center font-mono text-[9px]">{h % 6 === 0 ? h : ""}</span>
            ))}
            {cells.map((row, dow) => (
              <div key={dow} className="contents">
                <span className="text-muted-foreground font-mono text-[10px]">{DOWS[dow]}</span>
                {row.map((v, h) => (
                  <div
                    key={h}
                    className={cn("aspect-square min-h-2.5 rounded-[3px]", v <= 0 && "bg-muted/60")}
                    style={v > 0 ? { background: `color-mix(in oklch, var(--primary) ${Math.max(8, Math.round((v / max) * 100))}%, transparent)` } : undefined}
                    title={`${DOWS[dow]} ${pad2(h)}:00 — ${fmt(v)}`}
                  />
                ))}
              </div>
            ))}
          </div>
        </Panel>
      )}
    </Section>
  );
}

// Each session's writes are a bounded recent list: "what is being edited
// lately", not an all-time record.
function Files({ list }: { list: A[] }) {
  const counts = new Map<string, number>();
  for (const s of list) for (const w of s.writes ?? []) counts.set(w, (counts.get(w) || 0) + 1);
  const top = [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, 15);
  return (
    <Section title="Most-written files" note="Recent writes across the filtered sessions — each keeps a bounded list, so this is what they still remember writing, not an all-time record.">
      {!top.length ? <Empty>No session in this set has recorded a written file.</Empty> : (
        <Panel className="grid gap-1">
          {top.map(([path, n]) => (
            <div key={path} className="grid grid-cols-[minmax(0,1fr)_minmax(60px,140px)_auto] items-center gap-3 text-xs">
              <span className="text-muted-foreground truncate font-mono" title={path}>{shortPath(path)}</span>
              <span className="bg-muted h-2 overflow-hidden rounded-full">
                <i className="bg-primary block h-full rounded-full" style={{ width: ((n / top[0][1]) * 100).toFixed(1) + "%" }} />
              </span>
              <span className="text-muted-foreground text-right font-mono tabular-nums">×{n}</span>
            </div>
          ))}
        </Panel>
      )}
    </Section>
  );
}

function groups(list: A[], key: (s: A) => string) {
  const map = new Map<string, A[]>();
  for (const s of list) {
    const k = key(s);
    if (!map.has(k)) map.set(k, []);
    map.get(k)!.push(s);
  }
  return [...map.entries()].map(([name, ss]) => ({
    name, ss,
    spend: ss.reduce((a, s) => a + (hasRecorded(s) ? (s.cost as number) : 0), 0),
    anyCost: ss.some(hasRecorded),
    tokens: ss.reduce((a, s) => a + (Number(s.tokens?.total) || 0), 0),
    tools: ss.reduce((a, s) => a + (Number(s.tools) || 0), 0),
    running: ss.filter((s) => s.running).length,
  })).sort((a, b) => b.spend - a.spend || b.tokens - a.tokens);
}

function Breakdowns({ list }: { list: A[] }) {
  const spend = (g: ReturnType<typeof groups>[number]) => (g.anyCost ? money(g.spend) : "—");
  const deck = (title: string, key: (s: A) => string, withTools: boolean, withRunning: boolean, mono = false) => {
    const gs = groups(list, key);
    return (
      <Panel>
        <h4 className="text-muted-foreground mb-2 text-xs font-semibold tracking-wider uppercase">{title}</h4>
        <DataTable
          head={[[title.replace("By ", "").replace(/^./, (c) => c.toUpperCase())], ["Sessions", "num"], ["Spend", "num"], ["Tokens", "num"], ...(withTools ? [["Tools", "num"] as [string, "num"]] : []), ...(withRunning ? [["Running", "num"] as [string, "num"]] : [])]}
          rows={gs.map((g) => [
            <span key="n" className={cn("break-all", mono && "font-mono text-xs")}>{g.name}</span>,
            g.ss.length, spend(g), tokens(g.tokens),
            ...(withTools ? [count(g.tools)] : []),
            ...(withRunning ? [g.running ? String(g.running) : "—"] : []),
          ])}
        />
      </Panel>
    );
  };
  return (
    <Section title="Breakdowns">
      <div className="grid gap-3 [grid-template-columns:repeat(auto-fill,minmax(min(100%,420px),1fr))]">
        {deck("By who", who, true, true)}
        {deck("By provider", (s) => s.label || s.provider || "—", true, true)}
        {deck("By model", (s) => s.model || "—", false, false, true)}
        {deck("By project", projKey, true, false)}
      </div>
    </Section>
  );
}

const MAX_ROWS = 50;

// The drill-down: the rows the figures were summed from. Cost ranks them where
// the set mostly records it; tokens where it does not. No recorded cost sorts
// last rather than as zero.
function Sessions({ list }: { list: A[] }) {
  const tok = (s: A) => Number(s.tokens?.total) || 0;
  const costful = list.filter(hasRecorded).length * 2 >= list.length;
  const ranked = [...list].sort((a, b) => (costful ? (hasRecorded(b) ? (b.cost as number) : -Infinity) - (hasRecorded(a) ? (a.cost as number) : -Infinity) : 0) || tok(b) - tok(a));
  const anyHost = list.some((x) => x.host);
  const cost = (s: A) => (s.cost_included ? "incl" : !s.cost_available ? "—" : typeof s.cost === "number" ? money(s.cost) : "—");
  return (
    <Section title="Sessions" note={costful ? "The filtered set, most expensive first. Each row opens its session report." : "The filtered set, most tokens first — this set mostly records no cost."}>
      <Panel>
        <DataTable
          head={[["Session"], ...(anyHost ? [["Host"] as [string]] : []), ["Agent"], ["Model"], ["Cost", "num"], ["Tokens", "num"], ["Last active", "num"]]}
          rows={ranked.slice(0, MAX_ROWS).map((s) => [
            <Link key="l" href={"/session/" + encodeURIComponent(s.id)} className="hover:text-primary inline-block max-w-[46ch] truncate align-bottom" title={s.project}>
              {projKey(s)}
              {s.title && <span className="text-muted-foreground"> · {s.title}</span>}
            </Link>,
            ...(anyHost ? [<span key="h" className="text-muted-foreground font-mono text-xs">{s.host}</span>] : []),
            s.label || s.provider || "—",
            <span key="m" className="font-mono text-xs">{s.model ? shortModel(s.model) : "—"}</span>,
            cost(s),
            tokens(tok(s)),
            // A running session's last_active is always "now"; its state is the honest cell.
            s.running ? s.state || "running" : ago(s.last_active) || "—",
          ])}
        />
        {list.length > MAX_ROWS && <div className="text-muted-foreground px-2.5 pt-2 text-xs">…and {list.length - MAX_ROWS} more</div>}
      </Panel>
    </Section>
  );
}

// Where the figures cannot speak for themselves: which agents' costs a plan
// bundles, and which record no cost at all.
function Footnotes({ list, plan }: { list: A[]; plan?: string }) {
  const lines: string[] = [];
  const bundled = [...new Set(list.filter((s) => s.cost_included).map((s) => s.label || s.provider))];
  if (bundled.length) lines.push(`Spend for ${bundled.join(" and ")} is bundled in the ${plan ? plan + " " : ""}plan; figures shown are the recorded retail equivalent.`);
  const byProvider = new Map<string, A[]>();
  for (const s of list) {
    const k = s.label || s.provider || "—";
    if (!byProvider.has(k)) byProvider.set(k, []);
    byProvider.get(k)!.push(s);
  }
  const silent = [...byProvider.entries()].filter(([, ss]) => ss.every((s) => !s.cost_available)).map(([k]) => k);
  if (silent.length) lines.push(`${silent.join(" and ")} ${silent.length === 1 ? "records" : "record"} no cost data — its sessions contribute tokens and activity only.`);
  if (!lines.length) return null;
  return (
    <footer className="text-muted-foreground mt-2 grid gap-1 text-xs">
      {lines.map((l) => <div key={l}>{l}</div>)}
    </footer>
  );
}
