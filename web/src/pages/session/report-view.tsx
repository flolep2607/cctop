import { Badge } from "@/components/ui/badge";
import { DataTable, Panel, Section, Stat } from "@/components/section";
import { clock, money, secs, tokens } from "@/lib/format";
import { Ansi, stripAnsi } from "@/components/ansi";
import type { Report } from "@/lib/types";

const when = (ts?: string) => (ts || "").replace("T", " ").replace(/\..*$/, "");

function Tiles({ r }: { r: Report }) {
  // "incl" and "—" are different claims: the plan bundles this, or the
  // transcript records no billable usage at all.
  const cost = r.cost.included ? "incl" : r.cost.available ? money(r.cost.total) : "—";
  const pct = r.activity.error_rate != null ? Math.round(r.activity.error_rate * 100) : null;
  const ctx = r.context ? Math.round(r.context.percent_to_compact) : null;
  return (
    <div className="mb-7 grid grid-cols-2 gap-2.5 sm:grid-cols-[repeat(auto-fit,minmax(140px,1fr))]">
      <Stat label="Cost" value={cost} note={r.cost.included ? "bundled by this plan" : undefined} />
      <Stat
        label="Tokens"
        value={tokens(r.tokens.total || r.tokens.input + r.tokens.output)}
        note={`${tokens(r.tokens.input)} in · ${tokens(r.tokens.output)} out`}
      />
      <Stat label="Tool calls" value={(r.activity.tool_count || 0).toLocaleString()} />
      {pct !== null && (
        <Stat label="Tool errors" value={pct + "%"} note={(r.activity.tool_errors || 0) + " failed"} tone={pct >= 25 ? "bad" : pct >= 10 ? "warn" : null} />
      )}
      {ctx !== null && (
        <Stat label="Context" value={ctx + "%"} note={`${tokens(r.context.used)} of ${tokens(r.context.max)}`} tone={ctx >= 90 ? "bad" : ctx >= 70 ? "warn" : null} />
      )}
      {r.context?.compactions > 0 && <Stat label="Compactions" value={r.context.compactions} tone={r.context.compactions >= 3 ? "warn" : null} />}
      {(r.activity.lines_added || r.activity.lines_removed) ? (
        <Stat label="Lines" value={"+" + r.activity.lines_added} note={"−" + r.activity.lines_removed} />
      ) : null}
    </div>
  );
}

function Failures({ r }: { r: Report }) {
  const found = r.activity.failures ?? [];
  if (!found.length) return null;
  const repeated = found.some((f: { count: number }) => f.count > 1);
  // The heading follows the content: "Repeated failures" over a list of
  // one-offs teaches the reader to stop believing headings.
  return (
    <Section
      title={repeated ? "Repeated failures" : "Failed tool calls"}
      note={
        repeated
          ? "Grouped by tool and argument. A count above one is the same call retried — the agent paid for every attempt."
          : "Grouped by tool and argument. Nothing repeated, so these are one-offs rather than a loop."
      }
    >
      <Panel pad={false}>
        {found.map((f: { count: number; tool: string; detail: string; samples?: { ts: string }[] }, i: number) => (
          <div key={i} className="border-t px-4 py-3 first:border-t-0">
            <div className="flex flex-wrap items-baseline gap-2.5">
              {f.count > 1 && <span className="text-destructive font-mono font-semibold">×{f.count}</span>}
              <span className="text-muted-foreground font-mono text-xs">{f.tool}</span>
              {f.samples?.[0]?.ts && <span className="text-muted-foreground text-xs">first at {when(f.samples[0].ts)}</span>}
            </div>
            <pre className="bg-muted/50 mt-1.5 overflow-x-auto rounded-md border px-2.5 py-2 font-mono text-xs break-words whitespace-pre-wrap"><Ansi text={f.detail} /></pre>
          </div>
        ))}
      </Panel>
    </Section>
  );
}

const PARTS: [string, string, string][] = [
  ["startup", "Startup", "var(--muted-foreground)"],
  ["tool_output", "Tool output", "var(--chart-1)"],
  ["tool_input", "Tool input", "var(--chart-4)"],
  ["attachments", "Attachments", "var(--chart-3)"],
  ["user_text", "You", "var(--chart-5)"],
  ["assistant_text", "Assistant", "var(--chart-2)"],
];

function Breakdown({ r }: { r: Report }) {
  const b = r.context?.breakdown;
  if (!b?.total) return null;
  const parts: [string, number, string][] = PARTS.map(([k, label, color]) => [label, b[k] || 0, color]);
  if (b.unaccounted > 0) parts.push(["Unaccounted", b.unaccounted, "var(--border)"]);
  const shown = parts.reduce((s, [, v]) => s + v, 0) || 1;
  return (
    <Section
      title="What is in the context window"
      note={
        "Startup and the window total are exact; the rest is estimated from transcript characters, so read them as proportions. " +
        (b.unaccounted >= 0
          ? "Unaccounted is thinking — stored stripped — plus the harness's per-turn reminders and estimation error."
          : "The estimate overshoots the measured window, which happens when the harness has dropped old tool results the transcript still holds.") +
        (b.superseded ? " A compaction has since replaced this segment, so these describe the window before it." : "")
      }
    >
      <Panel>
        <div className="bg-muted flex h-5 overflow-hidden rounded-md">
          {parts.filter(([, v]) => v > 0).map(([label, v, color]) => (
            <span key={label} style={{ width: ((v / shown) * 100).toFixed(2) + "%", background: color }} title={label + ": " + tokens(v)} />
          ))}
        </div>
        <div className="mt-3 flex flex-wrap gap-x-5 gap-y-2 text-xs">
          {parts.filter(([, v]) => v > 0).map(([label, v, color]) => (
            <span key={label} className="flex items-center gap-1.5">
              <span className="size-2.5 rounded-sm" style={{ background: color }} />
              {label}
              <span className="text-muted-foreground font-mono">{tokens(v)}</span>
            </span>
          ))}
        </div>
      </Panel>
    </Section>
  );
}

// The window at every request, oldest first, evenly spaced per request — not a
// time axis, which the note says, because a reader who assumes one reads
// every flat stretch as a pause.
function WindowChart({ r }: { r: Report }) {
  const series: { ts: string; window: number; after_compaction?: boolean }[] = r.context?.series ?? [];
  if (series.length < 3) return null;
  const first = series[0].ts, last = series[series.length - 1].ts;
  const spans = new Date(first).toDateString() !== new Date(last).toDateString();
  const W = 900, H = 205, L = 46, B = 32, T = 8;
  const max = Math.max(...series.map((p) => p.window), 1);
  const x = (i: number) => L + (i / (series.length - 1)) * (W - L - 8);
  const y = (v: number) => T + (1 - v / max) * (H - T - B);
  const line = series.map((p, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(p.window).toFixed(1)}`).join("");
  return (
    <Section
      title="How the window filled"
      note="One point per request, evenly spaced — not a time axis: a wide flat stretch is many requests, not a long pause. Dashed marks are compactions."
    >
      <Panel className="overflow-x-auto">
        <svg viewBox={`0 0 ${W} ${H}`} className="block h-auto w-full min-w-[560px]" role="img" aria-label={`Context window across ${series.length} requests`}>
          {[0, 0.5, 1].map((f) => (
            <g key={f}>
              <line x1={L} x2={W - 8} y1={y(max * f)} y2={y(max * f)} stroke="var(--border)" />
              <text x={L - 6} y={y(max * f) + 3} textAnchor="end" className="fill-muted-foreground font-mono text-[10px]">
                {tokens(Math.round(max * f))}
              </text>
            </g>
          ))}
          <path d={`${line}L${x(series.length - 1)},${y(0)}L${x(0)},${y(0)}Z`} fill="var(--primary)" fillOpacity={0.13} />
          <path d={line} fill="none" stroke="var(--primary)" strokeWidth={1.5} />
          {series.map((p, i) =>
            p.after_compaction ? <line key={i} x1={x(i)} x2={x(i)} y1={T} y2={y(0)} stroke="var(--warning)" strokeDasharray="3 3" /> : null,
          )}
          <text x={L} y={H - 14} className="fill-muted-foreground font-mono text-[10px]">{clock(first, spans)}</text>
          <text x={(L + W - 8) / 2} y={H - 14} textAnchor="middle" className="fill-muted-foreground font-mono text-[10px]">{series.length} requests →</text>
          <text x={W - 8} y={H - 14} textAnchor="end" className="fill-muted-foreground font-mono text-[10px]">{clock(last, spans)}</text>
        </svg>
      </Panel>
    </Section>
  );
}

function Spend({ r }: { r: Report }) {
  const byModel = r.cost.by_model ?? [];
  const byHour: [string, number][] = r.cost.by_hour ?? [];
  if (!byModel.length && !byHour.length) return null;
  const W = 900, H = 110, L = 46, B = 16;
  const max = Math.max(...byHour.map(([, v]) => v), 0.0001);
  const bw = Math.max(1, (W - L - 8) / Math.max(1, byHour.length) - 1);
  return (
    <Section title="What it cost">
      <Panel>
        {byHour.length > 1 && (
          <div className="mb-3 overflow-x-auto">
            <svg viewBox={`0 0 ${W} ${H}`} className="block h-auto w-full min-w-[560px]" role="img" aria-label="Spend per hour">
              {byHour.map(([key, v], i) => {
                const h = Math.max(1, (v / max) * (H - B - 4));
                return (
                  <rect key={key} x={L + i * (bw + 1)} y={H - B - h} width={bw} height={h} rx={1} fill="var(--primary)">
                    <title>{key.replace("T", " ") + ":00 — " + money(v)}</title>
                  </rect>
                );
              })}
              <text x={L - 6} y={12} textAnchor="end" className="fill-muted-foreground font-mono text-[10px]">{money(max)}</text>
            </svg>
            <div className="text-muted-foreground text-xs">Per hour, most recent at the right.</div>
          </div>
        )}
        {byModel.length > 0 && (
          <DataTable
            head={[["Model"], ["Input", "num"], ["Output", "num"], ["Cost", "num"]]}
            rows={byModel.map((m: { model: string; tokens: { input: number; output: number }; total: number }) => [
              m.model, tokens(m.tokens.input), tokens(m.tokens.output), r.cost.included ? "incl" : money(m.total),
            ])}
          />
        )}
      </Panel>
    </Section>
  );
}

type Call = { tool: string; detail: string; failed?: boolean; origin?: string; duration_ms?: number | null; window_growth?: number | null; shared?: number; ts?: string };

const callCell = (c: Call, extra?: string | null) => (
  <span className="flex flex-wrap items-baseline gap-1.5">
    <span className="font-mono text-xs break-all">{stripAnsi(c.detail)}</span>
    {c.failed && <Badge variant="destructive">failed</Badge>}
    {extra && <Badge variant="outline">{extra}</Badge>}
    {c.origin && <Badge variant="outline">{c.origin}</Badge>}
  </span>
);

function Heaviest({ r }: { r: Report }) {
  const calls: Call[] = r.activity.heaviest ?? [];
  if (!calls.length) return null;
  return (
    <Section
      title="What filled the window"
      note="How much the context window grew after each call — which is what its result added. Largest first. Measured from the prompt each request was billed for; a turn that issued several calls grew it by all of their results together, and those are marked."
    >
      <Panel>
        <DataTable
          head={[["Tool"], ["Call"], ["Grew by", "num"]]}
          rows={calls.map((c) => [<span className="font-mono text-xs">{c.tool}</span>, callCell(c, (c.shared ?? 0) > 1 ? `with ${(c.shared ?? 0) - 1} more` : null), tokens(c.window_growth)])}
        />
      </Panel>
    </Section>
  );
}

function Slowest({ r }: { r: Report }) {
  const calls: Call[] = r.activity.slowest ?? [];
  if (!calls.length) return null;
  return (
    <Section title="Slowest calls" note="Wall time from the call being issued to its result arriving.">
      <Panel>
        <DataTable head={[["Tool"], ["Call"], ["Took", "num"]]} rows={calls.map((c) => [<span className="font-mono text-xs">{c.tool}</span>, callCell(c), secs(c.duration_ms)])} />
      </Panel>
    </Section>
  );
}

function Tools({ r }: { r: Report }) {
  const tools: { name: string; calls: number; failed: number }[] = r.activity.tools ?? [];
  if (!tools.length) return null;
  return (
    <Section title="Tools">
      <Panel>
        <DataTable
          head={[["Tool"], ["Calls", "num"], ["Failed", "num"]]}
          rows={tools.map((t) => [<span className="font-mono text-xs">{t.name}</span>, t.calls.toLocaleString(), t.failed ? String(t.failed) : "—"])}
        />
      </Panel>
    </Section>
  );
}

function CallLog({ r }: { r: Report }) {
  const calls: Call[] = r.activity.calls ?? [];
  if (!calls.length) return null;
  const made = r.activity.tool_count || 0;
  // Never presented as the whole record when it is not.
  const note =
    (calls.length < made
      ? `Newest first. The transcript keeps only its most recent ${calls.length} calls, of ${made} the session made.`
      : `Newest first. All ${calls.length} calls.`) +
    (calls.some((c) => c.origin) ? " A call a subagent made carries its name." : "");
  return (
    <Section title="Every tool call" note={note}>
      <Panel pad={false} className="max-h-[60vh] overflow-y-auto">
        <div className="p-3">
          <DataTable
            head={[["Tool"], ["Call"], ["Took", "num"], ["Grew", "num"], ["When"]]}
            rows={calls.map((c) => [
              <span className={c.failed ? "text-destructive font-mono text-xs" : "font-mono text-xs"}>{c.tool}</span>,
              callCell(c),
              c.duration_ms == null ? "—" : secs(c.duration_ms),
              c.window_growth == null ? "—" : "+" + tokens(c.window_growth),
              <span className="text-muted-foreground font-mono text-xs whitespace-nowrap">{when(c.ts)}</span>,
            ])}
          />
        </div>
      </Panel>
    </Section>
  );
}

function Subagents({ r }: { r: Report }) {
  if (!r.subagents?.length) return null;
  return (
    <Section title="Subagents">
      <Panel>
        <DataTable
          head={[["Type"], ["Description"], ["Tools", "num"], ["Cost", "num"]]}
          rows={r.subagents.map((a: { type: string; description: string; tool_count: number; cost: number }) => [
            <span className="font-mono text-xs">{a.type}</span>, a.description, a.tool_count, r.cost.included ? "incl" : money(a.cost),
          ])}
        />
      </Panel>
    </Section>
  );
}

export function ReportView({ r }: { r: Report }) {
  return (
    <>
      <Tiles r={r} />
      <Failures r={r} />
      <Breakdown r={r} />
      <WindowChart r={r} />
      <Heaviest r={r} />
      <Spend r={r} />
      <Slowest r={r} />
      <Tools r={r} />
      <CallLog r={r} />
      <Subagents r={r} />
    </>
  );
}

/** The report as something to paste: the figures a summary would type. */
export function reportMarkdown(r: Report): string {
  const out: string[] = ["# " + (r.title || r.project || r.session_id)];
  const meta = [r.project, r.branch, r.model || r.provider].filter(Boolean).join(" · ");
  if (meta) out.push(meta);
  out.push("");
  out.push("- Cost: " + (r.cost.included ? "included in the plan" : r.cost.available ? money(r.cost.total) : "not recorded"));
  out.push(`- Tokens: ${tokens(r.tokens.total || r.tokens.input + r.tokens.output)} (${tokens(r.tokens.input)} in · ${tokens(r.tokens.output)} out)`);
  if (r.context?.max)
    out.push(
      `- Context: ${Math.round(r.context.percent_to_compact)}% of the window (${tokens(r.context.used)} of ${tokens(r.context.max)})` +
        (r.context.compactions ? `, ${r.context.compactions} compactions` : ""),
    );
  if (r.duration) out.push("- Duration: " + r.duration + (r.running ? ", still running" : ""));
  if (r.activity?.error_rate != null)
    out.push(`- Tool errors: ${Math.round(r.activity.error_rate * 100)}% (${r.activity.tool_errors || 0} of ${r.activity.tool_count || 0})`);
  const loops = (r.activity.failures ?? []).filter((f: { count: number }) => f.count > 1);
  if (loops.length) {
    out.push("", "## Repeated failures");
    for (const f of loops.slice(0, 6)) {
      let d = String(f.detail || "").split("\n")[0];
      if (d.length > 120) d = d.slice(0, 117) + "…";
      out.push(`- ×${f.count} \`${f.tool}\`` + (d ? ` — \`${d}\`` : ""));
    }
  }
  if (r.files?.length) {
    out.push("", "## Files it wrote");
    for (const f of r.files) out.push("- " + f);
  }
  return out.join("\n");
}
