import { useEffect, useState } from "react";
import { useLocation } from "wouter";
import { Moon, Sparkles, Scale } from "lucide-react";
import {
  Command, CommandDialog, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandShortcut,
} from "@/components/ui/command";
import { getJson } from "@/lib/api";
import { ago, shortModel, shortPath } from "@/lib/format";
import { withToken } from "@/lib/config";
import type { Session, Tab } from "@/lib/types";
import { StateDot } from "./status";
import { dotOf, dotOfTab } from "@/lib/status";
import { useTheme } from "@/hooks/use-theme";
import { useTick } from "@/hooks/use-live";
import { go, PAGES } from "./nav";

/** What a page already holds live: the whole session table, the tabs. Null while it is still arriving. */
export type Shared = { sessions?: Session[] | null; tabs?: Tab[] | null };

// Ctrl+K / ⌘K on every page: every page, open tab and session, a few letters
// away. A page that already follows the table or the tabs — the dashboard,
// the workspace — hands them in, and the palette lists those as they move. On
// any other page they are fetched as it opens: the session page follows one
// row, and a second, whole-table stream on every page for a palette that is
// mostly closed would cost more than one read per opening.
export function CommandPalette({ open, setOpen, shared }: { open: boolean; setOpen: (o: boolean) => void; shared?: Shared }) {
  const [, navigate] = useLocation();
  const { cycle } = useTheme();
  const [fetched, setFetched] = useState<{ sessions: Session[]; tabs: Tab[] }>({ sessions: [], tabs: [] });
  const sessions = shared?.sessions ?? fetched.sessions;
  const tabs = shared?.tabs ?? fetched.tabs;
  const now = useTick(60000);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen(!open);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, setOpen]);

  const ownSessions = !shared?.sessions;
  const ownTabs = !shared?.tabs;
  useEffect(() => {
    if (!open) return;
    if (ownSessions) getJson<Session[]>("/api/sessions").then((s) => setFetched((f) => ({ ...f, sessions: Array.isArray(s) ? s : [] })), () => {});
    if (ownTabs) getJson<{ tabs?: Tab[] }>("/api/tabs").then((b) => setFetched((f) => ({ ...f, tabs: b.tabs ?? [] })), () => {});
  }, [open, ownSessions, ownTabs]);

  const run = (fn: () => void) => {
    setOpen(false);
    fn();
  };
  const recent = [...sessions].sort((a, b) => (Date.parse(b.last_active ?? "") || 0) - (Date.parse(a.last_active ?? "") || 0));

  return (
    <CommandDialog open={open} onOpenChange={setOpen} title="Go to" description="A page, a tab or a session">
      <Command>
      <CommandInput placeholder="Go to a page, session or tab…" />
      <CommandList>
        <CommandEmpty>Nothing matches.</CommandEmpty>
        <CommandGroup heading="Pages">
          {PAGES.map((p) => (
            <CommandItem key={p.path} value={"page " + p.label + " " + p.hint} onSelect={() => run(() => go(navigate, p.path))}>
              <p.icon />
              <span>{p.label}</span>
              <span className="text-muted-foreground truncate text-xs">{p.hint}</span>
            </CommandItem>
          ))}
          <CommandItem value="page optimize what sessions were handed and never used" onSelect={() => run(() => (location.href = withToken("/insight/optimize")))}>
            <Sparkles />
            <span>Optimize</span>
            <span className="text-muted-foreground truncate text-xs">what sessions were handed and never used</span>
          </CommandItem>
          <CommandItem value="page compare how the agents differ" onSelect={() => run(() => (location.href = withToken("/insight/compare")))}>
            <Scale />
            <span>Compare</span>
            <span className="text-muted-foreground truncate text-xs">how the agents differ on the same work</span>
          </CommandItem>
          <CommandItem value="action switch theme light dark" onSelect={() => run(cycle)}>
            <Moon />
            <span>Switch theme</span>
          </CommandItem>
        </CommandGroup>
        {tabs.length > 0 && (
          <CommandGroup heading="Tabs">
            {tabs.map((t) => (
              <CommandItem
                key={t.name}
                value={"tab " + t.label + " " + (t.cwd ?? "") + " " + t.name}
                onSelect={() => run(() => go(navigate, t.session_id ? "/session/" + encodeURIComponent(t.session_id) : "/workspace"))}
              >
                <StateDot state={dotOfTab(t.state)} />
                <span>{t.label}</span>
                <span className="text-muted-foreground truncate text-xs">{shortPath(t.cwd)}</span>
                <CommandShortcut>tab</CommandShortcut>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        {recent.length > 0 && (
          <CommandGroup heading="Sessions">
            {recent.slice(0, 200).map((s) => {
              const where = shortPath(s.project);
              return (
                <CommandItem
                  key={s.session_id}
                  value={["session", s.title, where, s.harness, s.model, s.session_id].filter(Boolean).join(" ")}
                  onSelect={() => run(() => go(navigate, "/session/" + encodeURIComponent(s.session_id)))}
                >
                  <StateDot state={dotOf(s)} />
                  <span className="truncate">{s.title || where || s.session_id}</span>
                  <span className="text-muted-foreground truncate text-xs">
                    {[s.title ? where : "", s.harness, shortModel(s.model), ago(s.last_active, now)].filter(Boolean).join(" · ")}
                  </span>
                </CommandItem>
              );
            })}
          </CommandGroup>
        )}
      </CommandList>
      </Command>
    </CommandDialog>
  );
}

