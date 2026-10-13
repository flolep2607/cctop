import { useState } from "react";
import { ExternalLink, TriangleAlert } from "lucide-react";
import { since } from "@/lib/format";
import { usePoll } from "@/hooks/use-poll";
import { useTick } from "@/hooks/use-live";
import { cn } from "@/lib/utils";
import type { PageDocument as PageDoc, ProviderStatus } from "@/lib/types";
import type { Incident } from "@/lib/gen/Incident";
import type { StatusLevel } from "@/lib/gen/StatusLevel";
import type { StatusTone } from "@/lib/gen/StatusTone";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";

// `/api/provider-status`, as `cctop_core::provider_status::document` writes it.
// The wording — the line and the "is it me?" sentence — arrives finished, so
// this page and the TUI's footer cannot say two different things.
type Doc = ProviderStatus;
type Level = StatusLevel;
type Tone = StatusTone;

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

// The TUI's outage footer, above the table: one line when a vendor's page
// explains — or pointedly fails to explain — the sessions failing here, and
// nothing otherwise, offline included. Clicking it opens the `!` panel.
export function ProviderStatus() {
  const read = usePoll<Doc>("/api/provider-status", REREAD_MS);
  const [open, setOpen] = useState(false);
  const now = useTick();
  // A failed read clears the line rather than keeping one that can no longer
  // be vouched for; it is not worth a console message, since the table's own
  // connection state already says the server went away.
  const doc = !read.error && read.data && Array.isArray(read.data.pages) ? read.data : null;
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
              <PageSection key={p.page} page={p} now={now} />
            ))}
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}

function PageSection({ page: p, now }: { page: PageDoc; now: number }) {
  const summary =
    p.state === "pending" ? "checking…"
    // Kept apart from an outage: cctop not reaching the page says nothing
    // about the provider.
    : p.state === "unavailable" ? `status page unreachable (${p.reason ?? "no answer"})`
    : p.description;
  const incidents = p.state === "ok" ? p.incidents : [];
  const degraded = p.state === "ok" ? p.degraded : [];
  const counted = incidents.filter((i) => i.standing !== "other");
  const other = incidents.filter((i) => i.standing === "other");
  const affected = degraded.filter((c) => c.standing !== "other");
  const otherComponents = degraded.filter((c) => c.standing === "other");
  return (
    <section className="space-y-2 border-t pt-3 first:border-t-0 first:pt-0">
      <div className="flex flex-wrap items-baseline gap-x-2">
        <h3 className="font-semibold">{p.label}</h3>
        <span className={p.state === "ok" ? LEVEL[p.level] : "text-muted-foreground"}>{summary}</span>
        {p.erroring > 0 && <span className="text-muted-foreground text-xs">({p.erroring} of yours failing)</span>}
        <a href={p.site} target="_blank" rel="noreferrer noopener" className="text-muted-foreground hover:text-foreground ml-auto inline-flex items-center gap-1 text-xs underline-offset-2 hover:underline">
          {p.site.replace(/^https?:\/\//, "")} <ExternalLink className="size-3" aria-hidden />
        </a>
      </div>
      {counted.map((i, n) => (
        <IncidentBlock key={n} incident={i} now={now} />
      ))}
      {affected.length > 0 && (
        <p className="text-xs">
          <span className="text-muted-foreground">affected </span>
          <span className="text-warning">{affected.map((c) => c.name).join(", ")}</span>
        </p>
      )}
      {(other.length > 0 || otherComponents.length > 0) && (
        <div className="space-y-2">
          <h4 className="text-muted-foreground text-xs font-medium">other, not affecting agents</h4>
          {other.map((i, n) => (
            <IncidentBlock key={n} incident={i} now={now} />
          ))}
          {otherComponents.length > 0 && (
            <p className="text-muted-foreground text-xs">affected {otherComponents.map((c) => c.name).join(", ")}</p>
          )}
        </div>
      )}
      {p.state === "ok" && incidents.length === 0 && degraded.length === 0 && <p className="text-muted-foreground text-xs">nothing open</p>}
    </section>
  );
}

// Live incidents at full strength; stale and unrelated ones dimmed, since they
// are there to be recognised and dismissed. A stale one is told by its age —
// "since 21:44" on a two-day-old incident would read as this evening.
function IncidentBlock({ incident: i, now }: { incident: Incident; now: number }) {
  const live = i.standing === "live";
  const meta = i.note ?? [i.stage, i.started_at ? since(i.started_at, now) : ""].filter(Boolean).join(" · ");
  return (
    <div
      className={cn(
        "space-y-1 border-l-2 pl-3",
        !live ? "border-muted text-muted-foreground" : i.level === "major" || i.level === "critical" ? "border-destructive" : "border-warning",
      )}
    >
      <div className="font-medium">{i.name}</div>
      {meta && <div className="text-muted-foreground text-xs">{meta}</div>}
      {i.update && <p className={live ? "text-foreground/90" : undefined}>{i.update}</p>}
      {i.components.length > 0 && (
        <p className="text-xs">
          <span className="text-muted-foreground">on </span>
          {i.components.join(", ")}
        </p>
      )}
    </div>
  );
}
