import { useState } from "react";
import { ExternalLink, SquareTerminal, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { ask } from "@/lib/api";
import { CAN_ACT } from "@/lib/config";
import { shortPath } from "@/lib/format";
import { Button } from "@/components/ui/button";
import { StateDot } from "@/components/status";
import { dotOfTab } from "@/lib/status";
import { TerminalFrame } from "@/components/terminal";
import { popOut, type Terminal } from "@/lib/terminal";
import type { Tab } from "@/lib/types";

// The TUI's tab bar, read from rmux: every agent cctop has open. A tab opens
// its terminal in a drawer under the bar. The list is the page's, which also
// hands it to the palette.
export function Tabs({ tabs }: { tabs: Tab[] | null }) {
  const [open, setOpen] = useState<{ tab: Tab; term?: Terminal; error?: string } | null>(null);
  const mint = (name: string, fresh = false): Promise<Terminal> =>
    ask("/api/tab/" + encodeURIComponent(name) + "/terminal", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ origin: location.origin, fresh }),
    }).then((r) => r.json());
  const toggle = async (t: Tab) => {
    if (open?.tab.name === t.name) return setOpen(null);
    setOpen({ tab: t });
    try {
      const term = await mint(t.name);
      setOpen((o) => (o?.tab.name === t.name ? { tab: t, term } : o));
    } catch (e) {
      setOpen((o) => (o?.tab.name === t.name ? { tab: t, error: String((e as Error).message || e) } : o));
    }
  };
  if (!tabs?.length) return null;
  return (
    <>
      <nav className="flex flex-wrap gap-1.5" aria-label="Open tabs">
        {tabs.map((t) => (
          <Button
            key={t.name}
            variant={open?.tab.name === t.name ? "secondary" : "outline"}
            size="sm"
            className="max-w-64 rounded-full"
            aria-pressed={open?.tab.name === t.name}
            disabled={!CAN_ACT}
            onClick={() => toggle(t)}
            title={(t.cwd ? shortPath(t.cwd) + " · " : "") + (CAN_ACT ? "open this tab's terminal" : "this link cannot open terminals")}
          >
            <StateDot state={dotOfTab(t.state)} />
            <span className="truncate">{t.label}</span>
          </Button>
        ))}
      </nav>
      {open && (
        <div className="bg-card overflow-hidden rounded-xl border shadow-xs">
          <div className="text-muted-foreground flex items-center gap-2 border-b px-3 py-1.5 text-xs">
            <SquareTerminal className="size-3.5" />
            {open.tab.label}
            {open.tab.cwd && <span>· {shortPath(open.tab.cwd)}</span>}
            <span className="flex-1" />
            {open.term && (
              <button
                type="button"
                className="hover:text-foreground inline-flex items-center gap-1"
                title="Open in a window of its own — the drawer closes, since a share admits one browser"
                onClick={() => {
                  const tab = open.tab;
                  popOut({ key: tab.name, title: tab.label + " — cctop", release: () => setOpen(null), mint: () => mint(tab.name, true), onClosed: () => {} });
                }}
              >
                <ExternalLink className="size-3" /> pop out
              </button>
            )}
            <Button variant="ghost" size="icon-xs" onClick={() => setOpen(null)} aria-label="Close the terminal">
              <X />
            </Button>
          </div>
          <div className="bg-terminal flex h-[min(60vh,640px)]">
            {open.term ? (
              <TerminalFrame
                url={open.term.url}
                name={open.term.name ?? open.tab.name}
                title="Terminal"
                onBroken={() => {
                  const tab = open.tab;
                  mint(tab.name, true).then((term) => setOpen((o) => (o?.tab.name === tab.name && o.term?.url !== term.url ? { tab, term } : o)), () => {});
                }}
              />
            ) : (
              <div className={cn("m-auto text-sm", open.error ? "text-terminal-error" : "text-terminal-muted")}>{open.error || "Opening this tab's terminal…"}</div>
            )}
          </div>
        </div>
      )}
    </>
  );
}
