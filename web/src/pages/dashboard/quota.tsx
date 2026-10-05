import { useEffect, useState } from "react";
import { getJson } from "@/lib/api";
import { ContextBar } from "./rows";

type Window = { label: string; pct: number; limit_reached?: boolean; resets_at?: number };
type Entry = { status: string; profile?: string; detail?: string; windows?: Window[] };

// A reset as a wall-clock time — "resets 14:05" reads at a glance where
// "in 40m" asks for a sum against a clock.
const hhmm = (secs?: number) => {
  if (!secs) return "";
  const d = new Date(Number(secs) * 1000);
  return isFinite(d.getTime()) ? String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0") : "";
};

// How much of each provider's subscription window is already gone. It moves in
// minutes, so it polls on its own slow clock; a server without the route
// leaves the strip empty, and empty draws nothing.
export function Quota() {
  const [data, setData] = useState<Record<string, Entry[]>>({});
  useEffect(() => {
    const poll = () => getJson<Record<string, Entry[]>>("/api/quota").then((d) => setData(d ?? {}), () => {});
    poll();
    const t = setInterval(poll, 60000);
    return () => clearInterval(t);
  }, []);
  const items: React.ReactNode[] = [];
  for (const [provider, entries] of Object.entries(data)) {
    for (const e of entries ?? []) {
      const who = provider + (e.profile && e.profile !== "default" ? "·" + e.profile : "");
      if (e.status === "ok") {
        items.push(
          <span key={who} className="inline-flex flex-wrap items-baseline gap-2.5">
            <span className="text-[10px] tracking-wider uppercase">{who}</span>
            {(e.windows ?? []).map((w) => {
              const pct = Math.max(0, Math.min(100, Number(w.pct) || 0));
              const at = hhmm(w.resets_at);
              return (
                <span key={w.label} className="whitespace-nowrap" title={at ? `${w.label} resets ${at}` : undefined}>
                  {w.label} <b className="text-foreground/80 font-mono font-semibold tabular-nums">{Math.round(pct)}%</b>{" "}
                  <ContextBar used={w.limit_reached ? 100 : pct} max={100} className="w-7" />
                </span>
              );
            })}
          </span>,
        );
      } else {
        // Only a status that asks something of the reader earns a mention.
        const resets = (e.windows ?? []).map((w) => w.resets_at).find(Boolean);
        const note =
          e.status === "expired" ? "sign-in expired"
          : e.status === "not_signed_in" ? "not signed in"
          : e.status === "rate_limited" ? "rate-limited" + (resets ? " until " + hhmm(resets) : "")
          : "";
        if (note) items.push(<span key={who} title={e.detail}>{who}: {note}</span>);
      }
    }
  }
  if (!items.length) return null;
  return <div className="text-muted-foreground flex flex-wrap items-center gap-x-4 gap-y-1 text-[11px]">{items}</div>;
}
