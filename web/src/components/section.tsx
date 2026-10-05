import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

// A titled section: a small heading, an optional note saying how to read what
// follows, then usually a card.
export function Section({ title, note, children, className }: { title: string; note?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cn("mb-7", className)}>
      <h3 className="text-muted-foreground mb-1 text-xs font-semibold tracking-wider uppercase">{title}</h3>
      {note && <p className="text-muted-foreground mb-2.5 max-w-[70ch] text-xs leading-relaxed">{note}</p>}
      {!note && <div className="mb-2" />}
      {children}
    </section>
  );
}

export function Panel({ children, className, pad = true }: { children: ReactNode; className?: string; pad?: boolean }) {
  return <div className={cn("bg-card overflow-hidden rounded-xl border shadow-xs", pad && "p-4", className)}>{children}</div>;
}

/** A plain data table: faint uppercase headers, figures right-aligned in mono. */
export function DataTable({ head, rows, className }: { head: [string, ("num" | "")?][]; rows: ReactNode[][]; className?: string }) {
  return (
    <div className={cn("overflow-x-auto", className)}>
      <table className="w-full text-[13px]">
        <thead>
          <tr>
            {head.map(([label, kind], i) => (
              <th key={i} className={cn("text-muted-foreground px-2.5 pb-2 text-left text-[11px] font-medium tracking-wider whitespace-nowrap uppercase", kind === "num" && "text-right")}>
                {label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((cells, r) => (
            <tr key={r} className="hover:bg-muted/40 border-t">
              {cells.map((cell, i) => (
                <td key={i} className={cn("px-2.5 py-1.5 align-top", head[i]?.[1] === "num" && "text-right font-mono tabular-nums")}>
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function Stat({ label, value, note, tone }: { label: string; value: ReactNode; note?: ReactNode; tone?: "bad" | "warn" | null }) {
  return (
    <div className="bg-card rounded-xl border p-3.5 shadow-xs">
      <div className="text-muted-foreground text-[11px] font-medium tracking-wider uppercase">{label}</div>
      <div className={cn("mt-1 font-mono text-xl font-semibold tabular-nums", tone === "bad" && "text-destructive", tone === "warn" && "text-warning")}>{value}</div>
      {note && <div className="text-muted-foreground mt-0.5 text-[11px]">{note}</div>}
    </div>
  );
}
