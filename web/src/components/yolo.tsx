import { useEffect, useRef, useState } from "react";
import { Zap } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { ANSWERABLE, setYolo } from "@/lib/api";
import { CAN_ACT } from "@/lib/config";
import { ago } from "@/lib/format";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import type { Session } from "@/lib/types";

// YOLO: cctop allowing every permission prompt a session raises, until it is
// switched off or the agent process exits (crates/core/src/yolo.rs). Claude Code
// is answered by `cctop yolo-hook` before its dialog goes up; other agents, and
// a Claude whose hook did not answer, get Allow pressed by the server. This page
// only flips the switch and shows what was waved through.

const TITLE = "cctop is allowing every permission prompt this session raises";

/** Whether this page may switch YOLO for `s` — the same harnesses Allow is offered for. */
const switchable = (s: Session) => CAN_ACT && s.running && ANSWERABLE.has(s.provider);

export function YoloBadge({ className }: { className?: string }) {
  return (
    <Badge variant="outline" className={cn("text-destructive border-destructive/50 bg-destructive/10 gap-1 font-semibold tracking-wide", className)} title={TITLE}>
      <Zap className="fill-current" />
      YOLO
    </Badge>
  );
}

/**
 * The switch as this page last set it, until the stream says the same. The
 * row only moves on the server's next refresh, and a button that snaps back
 * for a second after you pressed it reads as a refusal.
 */
function useYolo(s: Session) {
  const [busy, setBusy] = useState(false);
  // What was asked for, and what the row said when it was asked: once the row
  // says anything else the stream has caught up, and the row is the truth.
  const [pending, setPending] = useState<{ want: boolean; from: boolean } | null>(null);
  const actual = !!s.yolo;
  const set = async (on: boolean) => {
    setBusy(true);
    try {
      toast.success(await setYolo(s.session_id, on));
      setPending({ want: on, from: actual });
      // Never for long: a row that goes back to how it was later is news, not lag.
      setTimeout(() => setPending(null), 8000);
    } catch (e) {
      toast.error(String((e as Error).message || e));
    } finally {
      setBusy(false);
    }
  };
  return { on: pending && pending.from === actual ? pending.want : actual, busy, set };
}

/** The confirmation: what turning it on means, said before it is on. */
function Confirm({ open, onOpenChange, onConfirm }: { open: boolean; onOpenChange: (o: boolean) => void; onConfirm: () => void }) {
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle className="flex items-center gap-2">
            <Zap className="text-destructive size-4" />
            Allow everything this session asks?
          </AlertDialogTitle>
          <AlertDialogDescription>
            Every tool call in this session will be allowed without asking you — commands, edits, fetches, all of it. Claude Code&apos;s prompts are
            answered before they appear; other agents get Allow pressed for them.
          </AlertDialogDescription>
          <AlertDialogDescription>
            It lasts until you turn it off or this agent process exits, so a resumed session needs it turned on again. A question with choices still
            waits for you.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction className="bg-destructive hover:bg-destructive/90 text-white" onClick={onConfirm}>
            Allow everything
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/**
 * The session header's switch. Off, a quiet button that asks first; on, a loud
 * badge that is itself the one-click way off. Where it cannot be switched — a
 * read-only page, a harness cctop cannot answer for — it is only the badge.
 */
export function YoloSwitch({ session, className }: { session: Session; className?: string }) {
  const { on, busy, set } = useYolo(session);
  const [asking, setAsking] = useState(false);
  if (!switchable(session)) return on ? <YoloBadge className={className} /> : null;
  if (on) {
    return (
      <Button
        size="xs"
        variant="destructive"
        disabled={busy}
        className={cn("border-destructive/50 gap-1 font-semibold tracking-wide", className)}
        onClick={() => set(false)}
        title={TITLE + " — click to stop"}
      >
        <Zap className="fill-current" />
        YOLO
        <span className="font-normal opacity-80">· turn off</span>
      </Button>
    );
  }
  return (
    <>
      <Button
        size="xs"
        variant="ghost"
        disabled={busy}
        className={cn("text-muted-foreground font-normal", className)}
        onClick={() => setAsking(true)}
        title="Allow every permission prompt this session raises"
      >
        <Zap />
        YOLO
      </Button>
      <Confirm open={asking} onOpenChange={setAsking} onConfirm={() => set(true)} />
    </>
  );
}

/** The prompt bar's way in: this prompt and every one after it. */
export function AllowAllButton({ session, compact }: { session: Session; compact?: boolean }) {
  const { on, busy, set } = useYolo(session);
  const [asking, setAsking] = useState(false);
  if (on || !switchable(session)) return null;
  return (
    <>
      <Button
        size={compact ? "xs" : "sm"}
        variant="ghost"
        disabled={busy}
        className="text-muted-foreground"
        onClick={() => setAsking(true)}
        title="Turn on YOLO: this prompt and every later one allowed"
      >
        <Zap />
        {/* The long label does not fit a phone's prompt bar beside Allow and Deny. */}
        <span className={compact ? "hidden" : "max-sm:hidden"}>Allow all from now on</span>
        <span className={cn(!compact && "sm:hidden")}>Allow all</span>
      </Button>
      <Confirm open={asking} onOpenChange={setAsking} onConfirm={() => set(true)} />
    </>
  );
}

/**
 * What YOLO has allowed, newest first, and a toast for each as it lands.
 * The server keeps the last twenty (`KEEP` in crates/core/src/yolo.rs); the event log has
 * the rest.
 */
export function YoloLog({ session, className }: { session: Session; className?: string }) {
  const allowed = session.yolo?.allowed ?? [];
  const newest = allowed.at(-1)?.at ?? "";
  const newestAsk = allowed.at(-1)?.ask ?? "";
  const seen = useRef<string | null>(null);
  useEffect(() => {
    // The first look is history, not news.
    if (seen.current !== null && newest && newest !== seen.current) toast.message("Auto-allowed", { description: newestAsk });
    seen.current = newest;
  }, [newest, newestAsk]);
  if (!session.yolo) return null;
  return (
    <div className={cn("border-destructive/30 bg-destructive/[0.04] rounded-md border px-3 py-1.5 text-xs", className)}>
      <div className="text-muted-foreground flex items-center gap-1.5">
        <Zap className="text-destructive size-3 fill-current" />
        {allowed.length ? (
          <span>
            Auto-allowed {allowed.length === 1 ? "one prompt" : allowed.length + " prompts"} since YOLO went on {ago(session.yolo.since)}
          </span>
        ) : (
          <span>YOLO is on — nothing asked yet since {ago(session.yolo.since)}</span>
        )}
      </div>
      {allowed.length > 0 && (
        <ul className="mt-1 max-h-24 space-y-0.5 overflow-y-auto">
          {[...allowed].reverse().map((a, i) => (
            <li key={a.at + i} className="flex min-w-0 items-baseline gap-2">
              <span className="text-muted-foreground w-14 shrink-0 tabular-nums">{ago(a.at)}</span>
              <code className="text-foreground min-w-0 truncate font-mono" title={a.ask}>
                {a.ask}
              </code>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
