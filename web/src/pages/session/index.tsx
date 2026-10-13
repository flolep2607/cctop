import { useCallback, useEffect, useMemo, useState } from "react";
import { useParams } from "wouter";
import { ExternalLink, SquareTerminal, X } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { act } from "@/lib/api";
import { CAN_ACT } from "@/lib/config";
import { ago, clock, copyText, shortModel, shortPath } from "@/lib/format";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { AppShell } from "@/components/app-shell";
import { StateBadge, StateDot } from "@/components/status";
import { dotOf } from "@/lib/status";
import { TerminalFrame } from "@/components/terminal";
import { popOut, type Terminal } from "@/lib/terminal";
import { POLL_EVERY, useSessions, useTick } from "@/hooks/use-live";
import { usePoll } from "@/hooks/use-poll";
import { YoloLog, YoloSwitch } from "@/components/yolo";
import { AgentAddress } from "@/components/address";
import type { Report } from "@/lib/types";
import { AccessView } from "./access-view";
import { ChangesView } from "./changes-view";
import { diffFiles } from "./diff-files";
import { Conversation } from "./conversation";
import { ChatSignal } from "./chat-signal";
import { ReportView } from "./report-view";
import { Actions } from "./actions";

const VIEWS = ["chat", "changes", "access", "report"] as const;
type View = (typeof VIEWS)[number];
const viewFromHash = (): View => {
  const v = location.hash.replace(/^#/, "").split("/")[0];
  return (VIEWS as readonly string[]).includes(v) ? (v as View) : "chat";
};

const typing = (t: EventTarget | null) =>
  t instanceof HTMLElement && (t.isContentEditable || /^(INPUT|SELECT|TEXTAREA)$/.test(t.tagName));

export function SessionPage() {
  const { id: rawId = "" } = useParams<{ id: string }>();
  const urlId = decodeURIComponent(rawId);
  const [view, setView] = useState<View>(viewFromHash);
  // Views are drawn on first sight and kept: each is another read, and
  // switching back must not repeat it.
  const [seen, setSeen] = useState<Set<View>>(() => new Set([viewFromHash()]));
  const [term, setTerm] = useState<Terminal | "opening" | null>(null);
  const now = useTick();

  // The page's one blocking read, retried on its own until it lands: a
  // connection that drops during it leaves nothing on screen to recover from.
  // Read whether or not the tab is shown, so a session opened in a background
  // tab is ready when it is looked at.
  const { data: r, error } = usePoll<Report>("/api/report/" + encodeURIComponent(urlId), 5000, { once: true, visibleOnly: false });
  useEffect(() => {
    if (r) document.title = "cctop — " + (r.title || r.project || r.session_id);
  }, [r]);

  const id = r?.session_id ?? urlId;
  const { sessions, feed, chat } = useSessions(urlId);
  const chatSignal = useMemo(() => ({ mark: chat, live: feed === "live" }), [chat, feed]);
  // The URL may carry any unambiguous prefix of the id.
  const live = useMemo(
    () => sessions?.find((s) => s.session_id === id) ?? sessions?.find((s) => s.session_id.startsWith(urlId)) ?? null,
    [sessions, id, urlId],
  );
  const running = live ? live.running : !!r?.running;
  const state = dotOf(live ?? (r ? { running: r.running, state: r.state } : null));

  const show = useCallback((v: View) => {
    setView(v);
    setSeen((s) => (s.has(v) ? s : new Set(s).add(v)));
    if (!location.hash.startsWith("#" + v + "/")) history.replaceState(history.state, "", location.pathname + "#" + v);
  }, []);

  // 1–4 switch views from anywhere on the page.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || typing(e.target)) return;
      if (/^[1-4]$/.test(e.key)) {
        e.preventDefault();
        show(VIEWS[Number(e.key) - 1]);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [show]);

  // The terminal link is a shell credential, minted per opening and never kept.
  const toggleTerminal = async () => {
    if (term) return setTerm(null);
    setTerm("opening");
    try {
      const t = (await act("terminal", id, { origin: location.origin })) as unknown as Terminal;
      setTerm(t);
    } catch (e) {
      setTerm(null);
      toast.error(String((e as Error).message || e));
    }
  };

  if (!r) {
    return (
      <AppShell>
        <div className="text-muted-foreground m-auto p-10 text-center text-sm" data-rendered={error ? "" : undefined}>
          {error ? error + " Trying again…" : "Reading the transcript…"}
        </div>
      </AppShell>
    );
  }

  const spans = new Date(r.started_at).toDateString() !== new Date(r.last_active).toDateString();
  const when = [clock(r.started_at, spans), r.duration, running ? "still going" : ago(live?.last_active ?? r.last_active, now)].filter(Boolean).join(" · ");
  const changes = diffFiles(r).length;

  return (
    <AppShell>
      {/* Everything above the views stays put; only the view below scrolls. */}
      <div className="shrink-0 px-4 pt-3" data-rendered="">
        <div className="flex flex-wrap items-start gap-x-4 gap-y-2">
          <div className="min-w-0 flex-1">
            <h1 className="flex items-center gap-2 text-lg font-semibold tracking-tight">
              <StateDot state={state} />
              <span className="truncate">{r.title || shortPath(r.project) || r.session_id}</span>
              <StateBadge state={state} running={running} question={!!live?.asking_question} />
              {live && <YoloSwitch session={live} className="shrink-0" />}
              {/* The state beside it is the last the stream said; say when that may be old. */}
              {feed === "polling" && (
                <span
                  className="text-muted-foreground shrink-0 text-xs font-normal"
                  title={`The live stream is not getting through, so the state is fetched every ${POLL_EVERY / 1000} seconds instead`}
                >
                  polling
                </span>
              )}
              {sessions && feed === "down" && (
                <span className="text-muted-foreground shrink-0 text-xs font-normal" title="The live connection dropped; the state shown may be out of date">
                  reconnecting…
                </span>
              )}
            </h1>
            <div className="text-muted-foreground mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs">
              {r.title && r.project && <span title={r.project}>{shortPath(r.project)}</span>}
              {r.branch && <span className="font-mono">{r.branch}</span>}
              <span>{r.provider}</span>
              {r.model && <span>{shortModel(r.model)}</span>}
              {r.profile && r.profile !== "default" && <span>profile: {r.profile}</span>}
              {r.plan && r.plan !== "retail" && <span>plan: {r.plan}</span>}
              <span>{when}</span>
              <button
                type="button"
                className="hover:text-foreground font-mono opacity-70"
                title="Copy the session id"
                onClick={() => copyText(r.session_id).then(() => toast.success("Copied the session id"))}
              >
                {r.session_id.slice(0, 8)}
              </button>
              {CAN_ACT && <AgentAddress session={r.session_id} agent={r.title || shortPath(r.project) || r.session_id.slice(0, 8)} />}
            </div>
          </div>
          <Actions r={r} id={id} running={running} term={term} onTerminal={toggleTerminal} />
        </div>
        {live?.yolo && <YoloLog session={live} className="mt-2" />}
        {r.error && (
          <div className="border-destructive/40 bg-destructive/5 text-destructive mt-2 rounded-md border px-3 py-1.5 text-xs">
            This transcript could not be fully read: {r.error}
          </div>
        )}
        <Tabs value={view} onValueChange={(v) => show(v as View)} className="mt-3">
          <TabsList>
            <TabsTrigger value="chat">Conversation</TabsTrigger>
            <TabsTrigger value="changes">
              Changes
              {changes > 0 && <span className="text-muted-foreground font-mono text-[11px]">{changes}</span>}
            </TabsTrigger>
            <TabsTrigger value="access">Access</TabsTrigger>
            <TabsTrigger value="report">Report</TabsTrigger>
          </TabsList>
        </Tabs>
      </div>

      <div className={cn("grid min-h-0 flex-1 gap-3 pt-3", term ? "grid-rows-2 lg:grid-cols-2 lg:grid-rows-1" : "grid-cols-1")}>
        <div className="flex min-h-0 min-w-0 flex-col">
          {/* Conversation stays mounted while hidden: it holds a poll, a
              scroll position and whatever is half typed in the composer. */}
          <div className={cn("min-h-0 flex-1 flex-col", view === "chat" ? "flex" : "hidden")}>
            {seen.has("chat") && (
              <ChatSignal.Provider value={chatSignal}>
                <Conversation id={id} live={live} active={view === "chat"} />
              </ChatSignal.Provider>
            )}
          </div>
          {(["changes", "access", "report"] as const).map((v) => (
            <div key={v} className={cn("min-h-0 flex-1 overflow-y-auto px-4 pb-4", view === v ? "block" : "hidden")}>
              {seen.has(v) && v === "changes" && <ChangesView r={r} />}
              {seen.has(v) && v === "access" && <AccessView id={id} />}
              {seen.has(v) && v === "report" && <ReportView r={r} />}
            </div>
          ))}
        </div>
        {term && (
          <div className="flex min-h-0 flex-col px-4 pb-3 lg:pl-0">
            <div className="text-muted-foreground mb-1.5 flex items-center gap-2 text-xs">
              <SquareTerminal className="size-3.5" />
              This session's terminal
              <span className="flex-1" />
              {term !== "opening" && (
                <button
                  type="button"
                  className="hover:text-foreground inline-flex items-center gap-1"
                  title="Open in a window of its own — the panel closes, since a share admits one browser"
                  onClick={() =>
                    popOut({
                      key: id,
                      title: (r.title || shortPath(r.project) || id) + " — terminal",
                      release: () => setTerm(null),
                      mint: () => act("terminal", id, { origin: location.origin, fresh: true }) as unknown as Promise<Terminal>,
                      onClosed: () => {},
                    })
                  }
                >
                  <ExternalLink className="size-3" /> pop out
                </button>
              )}
              <Button variant="ghost" size="icon-xs" onClick={() => setTerm(null)} aria-label="Close the terminal">
                <X />
              </Button>
            </div>
            <div className="bg-terminal flex min-h-0 flex-1 overflow-hidden rounded-lg border">
              {term === "opening" ? (
                <div className="m-auto text-sm text-neutral-400">Opening this agent's terminal…</div>
              ) : (
                <TerminalFrame
                  url={term.url}
                  name={term.name}
                  title="Terminal"
                  onBroken={() =>
                    act("terminal", id, { origin: location.origin, fresh: true }).then(
                      (t) => setTerm(t as unknown as Terminal),
                      () => {},
                    )
                  }
                />
              )}
            </div>
            {term !== "opening" && (
              <p className="text-muted-foreground mt-1.5 text-[11px]">
                {term.tunnelled ? "Through rmux's own tunnel, so it works from wherever you are. " : "Served from the machine cctop runs on. "}
                Whoever holds this page can type into this agent — it is a shell, not a prompt box.
              </p>
            )}
          </div>
        )}
      </div>
    </AppShell>
  );
}
