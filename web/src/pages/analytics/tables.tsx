import { Link } from "wouter";
import { cn } from "@/lib/utils";
import { ago, costLabel, money, shortModel, shortPath, tokens } from "@/lib/format";
import { DataTable, Panel, Section, Stat } from "@/components/section";
import { Empty } from "@/components/empty";
import { count, hasRecorded, measurable, projKey, who, type A } from "./model";

// The page's figures and lists: the headline numbers, the most-written files,
// the breakdowns, the sessions themselves and the footnotes.

export function Kpis({ list }: { list: A[] }) {
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

// Each session's writes are a bounded recent list: "what is being edited
// lately", not an all-time record.
export function Files({ list }: { list: A[] }) {
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

export function Breakdowns({ list }: { list: A[] }) {
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
export function Sessions({ list, now }: { list: A[]; now: number }) {
  const tok = (s: A) => Number(s.tokens?.total) || 0;
  const costful = list.filter(hasRecorded).length * 2 >= list.length;
  const ranked = [...list].sort((a, b) => (costful ? (hasRecorded(b) ? (b.cost as number) : -Infinity) - (hasRecorded(a) ? (a.cost as number) : -Infinity) : 0) || tok(b) - tok(a));
  const anyHost = list.some((x) => x.host);
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
            costLabel({ total: s.cost, included: s.cost_included, available: !!s.cost_available }),
            tokens(tok(s)),
            // A running session's last_active is always "now"; its state is the honest cell.
            s.running ? s.state || "running" : ago(s.last_active, now) || "—",
          ])}
        />
        {list.length > MAX_ROWS && <div className="text-muted-foreground px-2.5 pt-2 text-xs">…and {list.length - MAX_ROWS} more</div>}
      </Panel>
    </Section>
  );
}

// Where the figures cannot speak for themselves: which agents' costs a plan
// bundles, and which record no cost at all.
export function Footnotes({ list, plan }: { list: A[]; plan?: string }) {
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
