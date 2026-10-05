import { useState } from "react";
import { Check, ShieldAlert, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { ANSWERABLE, answerPrompt } from "@/lib/api";
import { CAN_ACT } from "@/lib/config";
import { Button } from "@/components/ui/button";
import type { Session } from "@/lib/types";

// What a waiting agent is asking, and the two answers cctop can give for it.
// Callers key it on the question, so a new prompt starts with no stale answer.
// The question is always shown: Allow without it is approving blind. A harness
// whose menu keys the server does not know gets no buttons — its own terminal
// has the full menu.
export function PromptBar({ session, className, compact }: { session: Session; className?: string; compact?: boolean }) {
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<{ ok: boolean; text: string } | null>(null);
  const answerable = CAN_ACT && ANSWERABLE.has(session.provider);
  const go = async (choice: "allow" | "deny") => {
    setBusy(true);
    try {
      setSaid({ ok: true, text: await answerPrompt(session.session_id, choice) });
    } catch (e) {
      setSaid({ ok: false, text: String((e as Error).message || e) });
    } finally {
      setBusy(false);
    }
  };
  return (
    <div
      role="group"
      aria-label="Permission prompt"
      className={cn("bg-destructive/[0.07] border-destructive/30 flex min-w-0 items-center gap-2 rounded-md border px-2.5 py-1.5", className)}
    >
      <ShieldAlert className="text-destructive size-4 shrink-0" />
      <span className="text-muted-foreground min-w-0 flex-1 truncate text-xs" title={session.asking_for ?? ""}>
        {session.asking_for ? <code className="text-foreground font-mono">{session.asking_for}</code> : "Waiting on a permission prompt"}
      </span>
      {said && <span className={cn("shrink-0 text-xs", said.ok ? "text-success" : "text-destructive")}>{said.text}</span>}
      {answerable ? (
        <>
          <Button size={compact ? "xs" : "sm"} disabled={busy} onClick={() => go("allow")} title="Press the first option — allow this once">
            <Check />
            Allow
          </Button>
          <Button size={compact ? "xs" : "sm"} variant="outline" disabled={busy} onClick={() => go("deny")} title="Press Esc — refuse it">
            <X />
            Deny
          </Button>
        </>
      ) : (
        <span className="text-muted-foreground shrink-0 text-xs">answer it in its terminal</span>
      )}
    </div>
  );
}
