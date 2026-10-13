import { useEffect, useState } from "react";
import { Check, ChevronDown, Copy, Download, FileText, Forward, LayoutGrid, MessagesSquare, MoreHorizontal, Play, SquareTerminal, Wrench } from "lucide-react";
import { toast } from "sonner";
import { act, getJson } from "@/lib/api";
import { CAN_ACT, withToken } from "@/lib/config";
import { copyText, costLabel } from "@/lib/format";
import { copyMarkdown, downloadMarkdown } from "@/lib/export";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { Report } from "@/lib/types";
import { reportMarkdown } from "./report-markdown";

/** Where a session can be handed: an agent, under one of its accounts. */
type HandoffTarget = { agent: string; account?: string | null; label: string };

// The buttons that are not a prompt: the terminal, put the session back, give
// its work to another agent. Absent entirely when this run serves no actions.
export function Actions({
  r, id, running, term, onTerminal,
}: {
  r: Report; id: string; running: boolean; term: unknown; onTerminal: () => void;
}) {
  // Every (agent, account) this session can go to — the server leaves out the
  // pair it is already on, so the menu never offers a session back to itself.
  const [targets, setTargets] = useState<HandoffTarget[]>([]);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!CAN_ACT) return;
    getJson<{ targets?: HandoffTarget[] }>("/api/handoff/" + encodeURIComponent(id)).then(
      (b) => setTargets(b.targets ?? []),
      () => {},
    );
  }, [id]);
  const run = async (verb: string, body: object, label: string) => {
    setBusy(true);
    try {
      const done = await act(verb, id, body);
      toast.success(done.message || label + " done");
    } catch (e) {
      toast.error(String((e as Error).message || e));
    } finally {
      setBusy(false);
    }
  };
  const copyMd = () =>
    copyText(reportMarkdown(r))
      .then(() => toast.success("Copied this report as markdown"))
      .catch((e) => toast.error("Copy failed: " + String(e?.message || e)));
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {CAN_ACT && (
        <Tooltip>
          <TooltipTrigger asChild>
            <span>
              <Button variant={term ? "secondary" : "outline"} size="sm" disabled={!r.terminal} onClick={onTerminal} aria-pressed={!!term}>
                <SquareTerminal />
                Terminal
              </Button>
            </span>
          </TooltipTrigger>
          <TooltipContent>
            {r.terminal ? "This agent's own terminal, beside the conversation" : "Not running in a multiplexer cctop can reach"}
          </TooltipContent>
        </Tooltip>
      )}
      {CAN_ACT && (
        <Button variant="outline" size="sm" disabled={busy} onClick={() => run("resume", {}, "Resume")} title="Start this session's harness back up on this transcript">
          <Play />
          {running ? "Reattach" : "Resume"}
        </Button>
      )}
      {CAN_ACT && targets.length > 0 && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" size="sm" disabled={busy}>
              <Forward />
              Hand off
              <ChevronDown />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-60">
            <DropdownMenuLabel className="text-muted-foreground text-xs font-normal">Hand this session to another agent or account</DropdownMenuLabel>
            {targets.map((t) => (
              <DropdownMenuItem
                key={t.label}
                onSelect={() => run("handoff", { agent: t.agent, account: t.account ?? "" }, "Handoff")}
              >
                <span>{t.agent}</span>
                {/* Named whenever there is one, as the terminal's picker does: it is
                    the half of the pair that says whose subscription is spent. */}
                {t.account && (
                  <span className="text-muted-foreground ml-auto pl-4 text-xs whitespace-nowrap">as {t.account}</span>
                )}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      )}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label="More">
            <MoreHorizontal />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-64">
          <DropdownMenuItem onSelect={copyMd}>
            <Copy /> Copy report as markdown
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          {/* The conversation itself, for pasting into another agent or an
              issue. Flat rather than a submenu: a submenu is a hover target,
              and this menu is opened on phones too. */}
          <DropdownMenuItem onSelect={() => copyMarkdown(r.session_id, "conversation")}>
            <MessagesSquare /> Copy conversation as markdown
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => copyMarkdown(r.session_id, "tools")}>
            <Wrench /> Copy with tool output
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => copyMarkdown(r.session_id, "brief")}>
            <FileText /> Copy handoff brief
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => downloadMarkdown(r.session_id, "tools")}>
            <Download /> Download .md with tool output
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => copyText(r.session_id).then(() => toast.success("Copied the session id"))}>
            <Check /> Copy session id
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem asChild>
            <a href={withToken("/workspace")}>
              <LayoutGrid /> Open the workspace
            </a>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {r.cost && (
        <Badge variant="outline" className="font-mono font-normal">
          {costLabel(r.cost)}
        </Badge>
      )}
    </div>
  );
}
