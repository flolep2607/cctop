import { useState } from "react";
import { SendHorizontal } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { act } from "@/lib/api";
import { CAN_ACT } from "@/lib/config";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { PromptBar } from "@/components/prompt-bar";
import type { Session } from "@/lib/types";

// The prompt box, pinned under the conversation rather than inside it, so it
// is always on screen. One line, because that is what a pty submit is. While
// the session is stopped it is the Resume button instead.
export function Composer({ id, live }: { id: string; live: Session | null }) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  if (!CAN_ACT)
    return (
      <div className="text-muted-foreground shrink-0 border-t px-4 py-2.5 text-xs">
        View-only link — replies, Allow/Deny and resume need the “serving on” link <code>cctop serve</code> printed first.
      </div>
    );
  const running = !!live?.running;
  const asking = running && live?.state === "asking";
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      if (!running) {
        const done = await act("resume", id);
        toast.success(done.message || "Resumed");
      } else {
        if (!text.trim()) return;
        const done = await act("send", id, { text: text.trim() });
        toast.success(done.message || "Sent");
        setText("");
      }
    } catch (err) {
      toast.error(String((err as Error).message || err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="bg-background shrink-0 border-t px-4 py-2.5">
      {asking && live && <PromptBar key={live.asking_for ?? ""} session={live} className="mb-2" />}
      <form onSubmit={submit} className="flex gap-2">
        <Input
          value={text}
          onChange={(e) => setText(e.target.value)}
          disabled={!running || busy}
          maxLength={4000}
          placeholder={running ? "Answer this session…" : "Nothing is running this session"}
          className={cn("h-9", !running && "opacity-60")}
        />
        <Button type="submit" disabled={busy} className="h-9" title={running ? "" : "Start this session's harness back up on this transcript"}>
          {running ? <SendHorizontal /> : null}
          {running ? "Send" : "Resume"}
        </Button>
      </form>
    </div>
  );
}
