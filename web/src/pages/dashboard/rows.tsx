import { memo, useState } from "react";
import { Link } from "wouter";
import { Check, ImageUp, SendHorizontal, Star, X } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { act, ANSWERABLE, ask } from "@/lib/api";
import { CAN_ACT } from "@/lib/config";
import { ago, money, shortModel, shortPath, tokens } from "@/lib/format";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { StateDot, dotOf } from "@/components/status";
import { stripAnsi } from "@/components/ansi";
import type { Session } from "@/lib/types";

// The cost a row shows. `incl`, `—` and a figure are three different claims —
// the plan bundles this, nothing billable was recorded, here is what it cost —
// and none may be drawn as another.
export function rowCost(s: Session): string {
  if (s.cost?.included) return "incl";
  if (!s.cost?.available || s.cost.total == null) return "—";
  return money(Number(s.cost.total));
}

/** How full the context window is: the one figure that is a proportion. */
export function ContextBar({ used, max, className }: { used: number; max: number; className?: string }) {
  const pct = Math.min(100, (used / max) * 100);
  return (
    <span className={cn("bg-muted inline-block h-1.5 w-12 overflow-hidden rounded-full align-middle", className)} title={Math.round(pct) + "% of the context window"}>
      <i className={cn("block h-full", pct >= 90 ? "bg-destructive" : pct >= 70 ? "bg-warning" : "bg-muted-foreground/60")} style={{ width: pct.toFixed(1) + "%" }} />
    </span>
  );
}

export const SessionRow = memo(function SessionRow({
  s, selected, picked, pinned, fresh, snippet, href, onPick,
}: {
  s: Session; selected: boolean; picked: boolean; pinned: boolean; fresh: boolean; snippet?: string; href?: string;
  onPick?: (id: string) => void;
}) {
  const state = dotOf(s);
  const errRate = s.activity && s.activity.tool_count > 0 ? s.activity.tool_errors / s.activity.tool_count : 0;
  return (
    <div
      data-id={s.session_id}
      className={cn(
        "group relative flex items-start gap-3 border-t px-3 py-2.5 transition-colors first:border-t-0 sm:px-4",
        "hover:bg-primary/[0.05]",
        selected && "bg-primary/10 hover:bg-primary/[0.14]",
        picked && "shadow-[inset_3px_0_0_var(--primary)]",
      )}
    >
      {CAN_ACT && onPick && (
        <input
          type="checkbox"
          checked={picked}
          onChange={() => onPick(s.session_id)}
          className={cn("accent-primary relative z-10 mt-1.5 size-3.5 shrink-0 cursor-pointer transition-opacity", !picked && "opacity-30 group-hover:opacity-100 focus-visible:opacity-100")}
          aria-label={"Select for a bulk action — " + (shortPath(s.project) || s.session_id.slice(0, 8))}
        />
      )}
      <StateDot state={state} className="mt-2" />
      <Link href={href ?? "/session/" + encodeURIComponent(s.session_id)} className="min-w-0 flex-1 outline-none after:absolute after:inset-0 focus-visible:after:ring-2 focus-visible:after:ring-ring focus-visible:after:ring-inset">
        <div className="flex min-w-0 items-baseline gap-1.5" title={s.project ?? undefined}>
          {pinned && <Star className="text-warning size-3 shrink-0 fill-current self-center" />}
          {picked && <Check className="text-primary size-3.5 shrink-0 self-center" />}
          <span className="truncate font-medium">{s.project ? shortPath(s.project) : s.session_id.slice(0, 8)}</span>
          {s.title && <span className="text-muted-foreground truncate">· {s.title}</span>}
          {fresh && <span className="bg-primary size-1.5 shrink-0 self-center rounded-full" title="Activity since you last opened this session" />}
        </div>
        <div className="text-muted-foreground mt-0.5 flex flex-wrap items-center gap-x-2.5 gap-y-1 text-xs">
          <span>{s.provider}</span>
          {s.model && <span>{shortModel(s.model)}</span>}
          {s.branch && <span className="font-mono text-[11px]">{s.branch}</span>}
          <span>{ago(s.last_active)}</span>
          {/* The figures column is gone at phone width; the cost is the one
              figure worth keeping, so it moves into this line there. */}
          <span className="text-foreground font-mono sm:hidden">{rowCost(s)}</span>
          {s.running && s.state === "waiting" && <Badge variant="outline" className="text-warning border-warning/40 font-normal">waiting on you</Badge>}
          {s.running && s.state === "asking" &&
            (s.asking_question ? (
              <Badge variant="outline" className="text-warning border-warning/40 bg-warning/5 font-normal">has a question</Badge>
            ) : (
              <Badge variant="outline" className="text-destructive border-destructive/40 bg-destructive/5 font-normal">needs permission</Badge>
            ))}
          {s.state === "error" && <Badge variant="outline" className="text-destructive border-destructive/40 font-normal">{s.running ? "api error" : "ended on an api error"}</Badge>}
          {errRate >= 0.25 && <Badge variant="outline" className="text-destructive border-destructive/40 font-normal">{Math.round(errRate * 100)}% tool errors</Badge>}
          {s.conflict && (
            <Badge variant="outline" className="text-warning border-warning/40 font-normal">
              {s.conflict.level === "file" ? "same file as another agent" : "same repo as another agent"}
            </Badge>
          )}
          {s.sandbox && (
            <span className="font-mono" title={`Bash runs on ${s.sandbox.host}; ${s.sandbox.path} is mounted here`}>
              {s.sandbox.host}⇄
            </span>
          )}
          {s.user && <span>{s.user}</span>}
          {s.profile && s.profile !== "default" && <span>{s.profile}</span>}
          {s.context?.max ? <ContextBar used={s.context.used} max={s.context.max} /> : null}
          {snippet && <span className="basis-full truncate italic">“{stripAnsi(snippet)}”</span>}
        </div>
      </Link>
      <div className="shrink-0 text-right font-mono text-[13px] tabular-nums max-sm:hidden">
        <div>{rowCost(s)}</div>
        <div className="text-muted-foreground text-[11px]">{tokens(s.tokens?.total)} tok</div>
      </div>
    </div>
  );
});

// A session that wants a person: its row, what it is asking, and the box for
// answering it — one line, since that is what a pty submit is. The one-tap
// answers are the commonest replies: Allow/Deny for a permission menu (pressing
// the key the harness's own menu names), Continue for a turn that ended.
export function WantingRow({ s, picked, onPick, onDismiss }: { s: Session; picked: boolean; onPick: (id: string) => void; onDismiss: () => void }) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const post = async (verb: string, body: object, label?: string) => {
    setBusy(true);
    try {
      const done = await act(verb, s.session_id, body);
      toast.success(done.message || label || "Sent");
      return true;
    } catch (e) {
      toast.error(String((e as Error).message || e));
      return false;
    } finally {
      setBusy(false);
    }
  };
  // A picture pasted into the box reaches an agent on a machine you are only
  // sshed into: the bytes land in a file over there and the box gets its path,
  // which is how every one of these agents reads an image.
  const onPaste = (e: React.ClipboardEvent<HTMLInputElement>) => {
    const picture = Array.from(e.clipboardData?.items ?? []).find((i) => i.type?.startsWith("image/"));
    const file = picture?.getAsFile();
    if (!file) return;
    e.preventDefault();
    const reader = new FileReader();
    reader.onerror = () => toast.error("Could not read that image");
    reader.onload = async () => {
      try {
        const res = await ask("/api/act/image/" + encodeURIComponent(s.session_id), {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ data: String(reader.result) }),
        });
        const filed = await res.json();
        setText((t) => t + (t && !t.endsWith(" ") ? " " : "") + filed.path + " ");
        toast.success("Image filed on that machine");
      } catch (err) {
        toast.error(String((err as Error).message || err));
      }
    };
    reader.readAsDataURL(file);
  };
  const asking = s.running && s.state === "asking";
  return (
    <div className="relative border-t first:border-t-0">
      <SessionRow s={s} selected={false} picked={picked} pinned={false} fresh={false} onPick={onPick} />
      <Button variant="ghost" size="icon-xs" className="text-muted-foreground absolute top-2 right-2 z-10" onClick={onDismiss} title="Dismiss until its state changes" aria-label="Dismiss until its state changes">
        <X />
      </Button>
      {asking && s.asking_for && (
        <div className="px-4 pb-2 sm:pl-12">
          {s.asking_question ? (
            <p className="bg-warning/5 border-warning/30 rounded-md border px-2.5 py-1.5 text-sm">
              {s.asking_for}
              <span className="text-muted-foreground block text-xs">A question with choices — pick an answer in its terminal or session page.</span>
            </p>
          ) : (
            <code className="bg-muted/60 block rounded-md border px-2.5 py-1.5 font-mono text-xs break-words whitespace-pre-wrap">{s.asking_for}</code>
          )}
        </div>
      )}
      {CAN_ACT && s.running && (
        <form
          className="flex flex-wrap gap-1.5 px-3 pb-3 sm:pl-12"
          onSubmit={async (e) => {
            e.preventDefault();
            if (text.trim() && (await post("send", { text: text.trim() }))) setText("");
          }}
        >
          {asking && ANSWERABLE.has(s.provider) && !s.asking_question && (
            <>
              <Button type="button" size="sm" disabled={busy} onClick={() => post("answer", { choice: "allow" }, "Allowed")}>
                <Check /> Allow
              </Button>
              <Button type="button" size="sm" variant="outline" disabled={busy} onClick={() => post("answer", { choice: "deny" }, "Denied")}>
                <X /> Deny
              </Button>
            </>
          )}
          {s.state === "waiting" && (
            <Button type="button" size="sm" variant="outline" disabled={busy} onClick={() => post("send", { text: "continue" })}>
              Continue
            </Button>
          )}
          <div className="relative min-w-40 flex-1">
            <Input value={text} onChange={(e) => setText(e.target.value)} onPaste={onPaste} maxLength={4000} placeholder="Answer this session…" className="h-7 pr-7 text-[13px]" />
            <ImageUp className="text-muted-foreground/60 pointer-events-none absolute top-1/2 right-2 size-3.5 -translate-y-1/2" aria-label="Paste an image to send it" />
          </div>
          <Button type="submit" size="sm" disabled={busy}>
            <SendHorizontal /> Send
          </Button>
        </form>
      )}
    </div>
  );
}
