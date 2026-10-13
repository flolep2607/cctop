import { cn } from "@/lib/utils";
import { money, tokens } from "@/lib/format";
import { Panel, Section } from "@/components/section";
import { Empty } from "@/components/empty";
import { dayKey, dayLabel, daysBetween, pad2, parseDay, type A, type ByDay, type Series } from "./model";

// The page's charts: spend, tokens and activity per day, and the week's heat.

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

// Bundled and unrecorded costs are not in this chart: a $0 drawn for them would
// be a claim the data does not make. The foot of the page says where they went.
export function SpendChart({ list, byDay }: { list: A[]; byDay: ByDay }) {
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

export function TokensChart({ list, byDay }: { list: A[]; byDay: ByDay }) {
  const { span, series, any } = byDay(list, "tokens_by_day");
  return (
    <Section title="Tokens per day" note="Input, output and cache traffic together, stacked by agent.">
      {!span.length || !any ? <Empty>No per-day token buckets recorded in this range.</Empty> : <StackedDays days={span} series={series} fmt={tokens} label="Tokens per day, stacked by agent" />}
    </Section>
  );
}

// A session counts on every day its [started, last_active] span covers.
export function ActiveChart({ list, today, minDay, domain }: { list: A[]; today: string; minDay: string; domain: (d: Iterable<string>) => string[] }) {
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
export function Heat({ list, minHour }: { list: A[]; minHour: string }) {
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
