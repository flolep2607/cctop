import { useState } from "react";
import { RotateCw, SendHorizontal } from "lucide-react";
import { toast } from "sonner";
import { act } from "@/lib/api";
import { shortPath } from "@/lib/format";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { Session } from "@/lib/types";

// One verb against every marked session, one request at a time — these type
// at real terminals and start real agents. The summary names the failures.
export function BulkBar({ picked, byId, onClear }: { picked: Set<string>; byId: Map<string, Session>; onClear: () => void }) {
  const [text, setText] = useState("");
  const [composing, setComposing] = useState(false);
  const [busy, setBusy] = useState(false);
  const run = async (verb: "send" | "resume") => {
    setBusy(true);
    const failed: string[] = [];
    let done = 0;
    for (const id of picked) {
      try {
        await act(verb, id, verb === "send" ? { text } : {});
        done++;
      } catch (e) {
        failed.push((shortPath(byId.get(id)?.project) || id.slice(0, 8)) + " — " + String((e as Error).message || e));
      }
    }
    setBusy(false);
    const summary = (verb === "send" ? "Sent to " : "Resumed ") + done;
    if (failed.length) toast.error(summary + " · failed: " + failed.map((f) => f.split(" — ")[0]).join(", "), { description: failed.join("\n") });
    else {
      toast.success(summary);
      if (verb === "send") {
        setText("");
        setComposing(false);
      }
    }
  };
  return (
    <div className="bg-background flex shrink-0 flex-wrap items-center gap-2 border-t px-3 py-2 text-sm sm:px-4">
      <span className="font-medium">{picked.size} selected</span>
      <Button size="sm" variant="outline" disabled={busy} onClick={() => setComposing(!composing)}>
        <SendHorizontal /> Send…
      </Button>
      <Button size="sm" variant="outline" disabled={busy} onClick={() => run("resume")}>
        <RotateCw /> Resume
      </Button>
      <Button size="sm" variant="ghost" disabled={busy} onClick={onClear}>
        Clear
      </Button>
      {composing && (
        <form
          className="flex min-w-64 flex-1 gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (text.trim()) run("send");
          }}
        >
          <Input autoFocus value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === "Escape" && setComposing(false)} maxLength={4000} placeholder="One line, sent to each selected session" className="h-8" />
          <Button type="submit" size="sm" disabled={busy || !text.trim()}>
            Send
          </Button>
        </form>
      )}
    </div>
  );
}
