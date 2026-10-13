import type { Analytics, AnalyticsSession } from "@/lib/types";
// The analytics page's data and the small arithmetic every section shares.

// One row of `/api/analytics`, and the document — generated from the server's structs.
export type A = AnalyticsSession;
export type Data = Analytics;

export const pad2 = (n: number) => String(n).padStart(2, "0");
export const dayKey = (d: Date) => `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
export const hourKey = (d: Date) => dayKey(d) + "T" + pad2(d.getHours());
// A day key as a local date: `new Date("2026-09-15")` would be UTC midnight,
// the previous evening for anyone west of Greenwich.
export const parseDay = (day: string) => {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, m - 1, d);
};
export const dayLabel = (day: string) => parseDay(day).toLocaleDateString(undefined, { month: "short", day: "numeric" });
export function daysBetween(a: string, b: string): string[] {
  const out: string[] = [];
  const end = parseDay(b);
  for (let d = parseDay(a); d <= end && out.length <= 400; d.setDate(d.getDate() + 1)) out.push(dayKey(d));
  return out;
}

// Whose session: the person, the login, the machine, or nobody in particular.
export const who = (s: A) => s.user || s.account || s.host || "local";
export const projKey = (s: A) => s.project_name || s.project || "—";
// Three distinct claims: the provider records cost at all, the plan bundles
// it (the figure is a retail equivalent), and a number was recorded.
export const measurable = (s: A) => !!s.cost_available && !s.cost_included && typeof s.cost === "number";
export const hasRecorded = (s: A) => !!s.cost_available && typeof s.cost === "number";
export const count = (v: unknown) => (Number(v) || 0).toLocaleString();
export const PALETTE = ["var(--chart-1)", "var(--chart-2)", "var(--chart-3)", "var(--chart-4)", "var(--chart-5)", "var(--muted-foreground)"];

export type Series = { name: string; color: string; byDay: Map<string, number> };
export type ByDay = (src: A[], field: "by_day" | "tokens_by_day") => { span: string[]; series: Series[]; any: boolean };
