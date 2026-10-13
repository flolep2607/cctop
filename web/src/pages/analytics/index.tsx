import { useMemo } from "react";
import { ago } from "@/lib/format";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { AppShell } from "@/components/app-shell";
import { Empty } from "@/components/empty";
import { useStored, useTick } from "@/hooks/use-live";
import { usePoll } from "@/hooks/use-poll";
import { dayKey, daysBetween, hourKey, PALETTE, projKey, who, type A, type Data, type Series } from "./model";
import { ActiveChart, Heat, SpendChart, TokensChart } from "./charts";
import { Breakdowns, Files, Footnotes, Kpis, Sessions } from "./tables";

// The fleet as figures: what sessions cost, when they ran, and who, what and
// where they belong to. Every section reads the same filtered set, so the page
// tells one story. All filtering is client-side: the whole fleet already
// arrives in one payload, and re-asking the server per filter change would add
// a route for nothing.

const RANGES = { "24h": 864e5, "7d": 7 * 864e5, "30d": 30 * 864e5, all: Infinity } as const;
type Range = keyof typeof RANGES;
type Sel = { provider: string; project: string; model: string; who: string; range: Range };
const ALL = "__all";

export function AnalyticsPage() {
  const { data, error, at } = usePoll<Data>("/api/analytics", 15000);
  const [sel, setSel] = useStored<Sel>("cctop-analytics", { provider: "", project: "", model: "", who: "", range: "30d" });
  const now = useTick(10000);
  const sessions = useMemo(() => (Array.isArray(data?.sessions) ? data.sessions : []), [data]);
  // When the server built the document, or failing that when it arrived.
  const generated = Date.parse(data?.generated ?? "");
  const updated = data ? (isFinite(generated) ? generated : at) : 0;

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
    for (const s of sessions) {
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
  }, [sessions]);
  // A stored choice the fleet no longer offers reads as "all".
  const eff = (k: "provider" | "project" | "model" | "who") => (options[k].some(([v]) => v === sel[k]) ? sel[k] : "");
  const f = { provider: eff("provider"), project: eff("project"), model: eff("model"), who: eff("who") };

  // A session is in range if it did something, or began, inside it.
  const list = sessions.filter((s) => {
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

  const stamp = updated ? `updated ${new Date(updated).toLocaleTimeString()} · ${ago(new Date(updated).toISOString(), now)}` : "reading…";

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
                <Empty>{sessions.length ? "No sessions match that filter and range." : "No sessions yet — when cctop has seen some, this page reads them."}</Empty>
              ) : (
                <>
                  <SpendChart list={list} byDay={byDay} />
                  <TokensChart list={list} byDay={byDay} />
                  <ActiveChart list={list} today={today} minDay={MIN_DAY} domain={domain} />
                  <Heat list={list} minHour={MIN_HOUR} />
                  <Files list={list} />
                  <Breakdowns list={list} />
                  <Sessions list={list} now={now} />
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
