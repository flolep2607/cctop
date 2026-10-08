import { useEffect, useState } from "react";
import { ExternalLink, TriangleAlert } from "lucide-react";
import { getJson } from "@/lib/api";
import { cn } from "@/lib/utils";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";

// `/api/provider-status`, as `cctop_core::provider_status::document` writes it.
// The wording — the line and the "is it me?" sentence — arrives finished, so
// this page and the TUI's footer cannot say two different things.
type Level = "operational" | "maintenance" | "minor" | "major" | "critical";
type Tone = "outage" | "warning" | "quiet";
type Incident = { name: string; stage: string; level: Level; started_at: number | null; update: string | null; components: string[] };
type PageDoc = {
  page: string;
  label: string;
  site: string;
  erroring: number;
  state: "pending" | "ok" | "unavailable";
  reason?: string;
  level?: Level;
  description?: string;
  incidents?: Incident[];
  degraded?: string[];
};
type Doc = { pages: PageDoc[]; line: { text: string; tone: Tone } | null; verdict: { text: string; detail: string | null; tone: Tone } };

// The server answers from memory and polls the vendor on the TUI's two-minute
// schedule whatever this does, so re-reading more often than that buys nothing
// but staleness bounded a little tighter.
const REREAD_MS = 30000;

const TONE: Record<Tone, string> = {
  outage: "text-destructive",
  warning: "text-warning",
  quiet: "text-muted-foreground",
};

const LEVEL: Record<Level, string> = {
  operational: "text-success",
  maintenance: "text-warning",
  minor: "text-warning",
  major: "text-destructive",
  critical: "text-destructive",
};

// `cctop_core::util::compact_duration`, which the TUI's panel uses.
function compact(secs: number): string {
  const s = Math.round(secs);
  if (s <= 0) return "—";
  if (s < 60) return s + "s";
  const two = (n: number) => String(n).padStart(2, "0");
  if (s < 3600) return Math.floor(s / 60) + "m" + two(s % 60) + "s";
  return Math.floor(s / 3600) + "h" + two(Math.floor((s % 3600) / 60)) + "m";
}

// "since 13:55 (1h20m)", in the browser's local time.
function since(startedAt: number): string {
  const at = new Date(startedAt * 1000);
  const hm = isNaN(at.getTime()) ? "?" : String(at.getHours()).padStart(2, "0") + ":" + String(at.getMinutes()).padStart(2, "0");
  const elapsed = Date.now() / 1000 - startedAt;
  return elapsed > 0 ? `since ${hm} (${compact(elapsed)})` : `since ${hm}`;
}

// The TUI's outage footer, above the table: one line when a vendor's page
// explains — or pointedly fails to explain — the sessions failing here, and
// nothing otherwise, offline included. Clicking it opens the `!` panel.
export function ProviderStatus() {
  const [doc, setDoc] = useState<Doc | null>(null);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    // A failed read clears the line rather than keeping one that can no longer
    // be vouched for; it is not worth a console message, since the table's own
    // connection state already says the server went away.
    const read = () => getJson<Doc>("/api/provider-status").then((d) => setDoc(d && Array.isArray(d.pages) ? d : null), () => setDoc(null));
    read();
    const t = setInterval(read, REREAD_MS);
    return () => clearInterval(t);
  }, []);
  if (!doc?.line) return null;
  const { line } = doc;
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className={cn(
          "flex w-full items-start gap-2 rounded-md border px-3 py-1.5 text-left text-xs font-medium",
          "focus-visible:ring-ring/50 outline-none focus-visible:ring-[3px]",
          line.tone === "outage" ? "border-destructive/40 bg-destructive/5 text-destructive hover:bg-destructive/10" : "border-warning/40 bg-warning/5 text-warning hover:bg-warning/10",
        )}
        aria-haspopup="dialog"
      >
        <TriangleAlert className="mt-px size-3.5 shrink-0" aria-hidden />
        {/* The ⚠ is the TUI's; the icon says it here. */}
        <span className="min-w-0 break-words">{line.text.replace(/^⚠\s*/, "")}</span>
        <span className="text-muted-foreground ml-auto shrink-0 pl-2 font-normal max-sm:hidden">details</span>
      </button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="flex max-h-[85vh] flex-col gap-4 sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>Provider status</DialogTitle>
            <DialogDescription className={cn("font-medium", TONE[doc.verdict.tone])}>{doc.verdict.text}</DialogDescription>
            {doc.verdict.detail && <p className="text-muted-foreground text-sm">{doc.verdict.detail}</p>}
          </DialogHeader>
          <div className="-mx-1 min-h-0 flex-1 space-y-4 overflow-y-auto px-1 text-sm">
            {doc.pages.map((p) => (
              <PageSection key={p.page} page={p} />
            ))}
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}

function PageSection({ page: p }: { page: PageDoc }) {
  const summary =
    p.state === "pending" ? "checking…"
    // Kept apart from an outage: cctop not reaching the page says nothing
    // about the provider.
    : p.state === "unavailable" ? `status page unreachable (${p.reason ?? "no answer"})`
    : p.description || "";
  const incidents = p.incidents ?? [];
  const degraded = p.degraded ?? [];
  return (
    <section className="space-y-2 border-t pt-3 first:border-t-0 first:pt-0">
      <div className="flex flex-wrap items-baseline gap-x-2">
        <h3 className="font-semibold">{p.label}</h3>
        <span className={p.state === "ok" && p.level ? LEVEL[p.level] : "text-muted-foreground"}>{summary}</span>
        {p.erroring > 0 && <span className="text-muted-foreground text-xs">({p.erroring} of yours failing)</span>}
        <a href={p.site} target="_blank" rel="noreferrer noopener" className="text-muted-foreground hover:text-foreground ml-auto inline-flex items-center gap-1 text-xs underline-offset-2 hover:underline">
          {p.site.replace(/^https?:\/\//, "")} <ExternalLink className="size-3" aria-hidden />
        </a>
      </div>
      {incidents.map((i, n) => {
        const meta = [i.stage, i.started_at ? since(i.started_at) : ""].filter(Boolean).join(" · ");
        return (
          <div key={n} className={cn("space-y-1 border-l-2 pl-3", i.level === "major" || i.level === "critical" ? "border-destructive" : "border-warning")}>
            <div className="font-medium">{i.name}</div>
            {meta && <div className="text-muted-foreground text-xs">{meta}</div>}
            {i.update && <p className="text-foreground/90">{i.update}</p>}
            {i.components.length > 0 && (
              <p className="text-xs">
                <span className="text-muted-foreground">on </span>
                {i.components.join(", ")}
              </p>
            )}
          </div>
        );
      })}
      {degraded.length > 0 && (
        <p className="text-xs">
          <span className="text-muted-foreground">affected </span>
          <span className="text-warning">{degraded.join(", ")}</span>
        </p>
      )}
      {p.state === "ok" && incidents.length === 0 && degraded.length === 0 && <p className="text-muted-foreground text-xs">nothing open</p>}
    </section>
  );
}
