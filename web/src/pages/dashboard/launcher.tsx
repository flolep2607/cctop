import { useEffect, useRef, useState } from "react";
import { CircleAlert, CircleCheck, Loader2, Play, Plus, Server } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { ask, getJson } from "@/lib/api";
import { CAN_ACT } from "@/lib/config";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";

type SshHost = { name: string; aliases: string[] };
type Hit = { path: string; problem: string | null };
type Offline = { ready: false; why: string; prompt: boolean };
type Completion = Offline | { ready: true; dir: string; missing: boolean; failed: string | null; scanning: boolean; hits: Hit[] };
type Verdict = Offline | { ready: true; problem: string | null };

// The server's own reading of `host:path` (`remote_launch::parse`): a colon
// before any slash makes a host, unless the text starts like a path.
function parseLocation(text: string): { host: string; path: string } | null {
  const t = text.trim();
  if (!t || /^[/~.]/.test(t)) return null;
  if (t.startsWith("[")) {
    const end = t.indexOf("]:");
    return end > 1 ? { host: t.slice(0, end + 1), path: t.slice(end + 2) } : null;
  }
  const colon = t.indexOf(":");
  if (colon < 1) return null;
  const host = t.slice(0, colon);
  if (host.includes("/") || /\s/.test(host) || host.startsWith("-")) return null;
  return { host, path: t.slice(colon + 1) };
}

async function post<T>(path: string, body: object, signal?: AbortSignal): Promise<T> {
  const res = await ask(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body), signal });
  return (await res.json()) as T;
}

// What the host says while it is not ready, in the field's words.
function offlineNote(host: string, o: Offline): string {
  return o.prompt ? `${host}: ${o.why} — the session will ask` : `${host} offline: ${o.why}`;
}

// Hosts to offer while the field holds no host: those whose name or alias
// contains what is typed, the way the terminal's field filters them.
function hostHits(hosts: SshHost[], word: string): SshHost[] {
  const w = word.trim().toLowerCase();
  if (w.includes("/") || /^[~.]/.test(w)) return [];
  return hosts.filter((h) => [h.name, ...h.aliases].some((n) => n.toLowerCase().includes(w))).slice(0, 5);
}

// Starting a fresh agent from the page — the TUI's launcher, carried to the
// one place a phone can reach. Only where this run can act. The Directory
// field takes `host:path` as the terminal's does; the hosts, completion and
// folder check are asked of routes only the full token reaches.
export function Launcher() {
  const [agents, setAgents] = useState<string[]>([]);
  const [localOnly, setLocalOnly] = useState<Record<string, string>>({});
  const [open, setOpen] = useState(false);
  const [agent, setAgent] = useState("");
  const [cwd, setCwd] = useState("");
  const [busy, setBusy] = useState(false);
  const [hosts, setHosts] = useState<SshHost[]>([]);
  // The host's last answer, and the `host:path` it was for.
  const [answer, setAnswer] = useState<{ key: string; completion: Completion | null; verdict: Verdict | null }>({ key: "", completion: null, verdict: null });
  const [pick, setPick] = useState(-1);
  const [focused, setFocused] = useState(false);
  const field = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!CAN_ACT) return;
    getJson<{ agents?: string[]; actions?: boolean; local_only?: Record<string, string> }>("/api/agents").then((r) => {
      if (r.actions === false) return;
      setAgents(r.agents ?? []);
      setLocalOnly(r.local_only ?? {});
      setAgent((r.agents ?? [])[0] ?? "");
    }, () => {});
  }, []);

  // The host list once per opening: a host added to ~/.ssh/config shows up
  // the next time the dialog does.
  useEffect(() => {
    if (!open) return;
    post<{ hosts: SshHost[] }>("/api/ssh/hosts", {}).then((r) => setHosts(r.hosts ?? []), () => setHosts([]));
  }, [open]);

  const remote = parseLocation(cwd);
  const host = remote?.host ?? "";
  const path = remote?.path ?? "";
  const key = host ? `${host}\n${path}` : "";
  const asking = !!host && answer.key !== key;
  // The last listing stays up while the next is asked for, as long as it is
  // the same host's, so the list does not blink at every keystroke.
  const completion = host && answer.key.startsWith(`${host}\n`) ? answer.completion : null;
  const verdict = host && !asking ? answer.verdict : null;

  // Completion and the folder check, a moment after typing stops. An
  // abandoned request is aborted, so an older answer never lands on newer text.
  useEffect(() => {
    if (!host) return;
    const abort = new AbortController();
    const timer = window.setTimeout(() => {
      const body = { host, path };
      Promise.all([
        post<Completion>("/api/ssh/complete", body, abort.signal),
        post<Verdict>("/api/ssh/check", body, abort.signal),
      ]).then(
        ([completion, verdict]) => setAnswer({ key, completion, verdict }),
        (err) => {
          if (abort.signal.aborted) return;
          setAnswer({ key, completion: { ready: false, why: String((err as Error).message || err), prompt: false }, verdict: null });
        },
      );
    }, 250);
    return () => {
      window.clearTimeout(timer);
      abort.abort();
    };
  }, [host, path, key]);

  // While a scan of the host's repositories is still running, ask again so
  // they arrive without another keystroke.
  const scanning = !asking && !!completion?.ready && completion.scanning;
  useEffect(() => {
    if (!scanning) return;
    const abort = new AbortController();
    const timer = window.setTimeout(() => {
      post<Completion>("/api/ssh/complete", { host, path }, abort.signal).then(
        (completion) => setAnswer((a) => (a.key === key ? { ...a, completion } : a)),
        () => {},
      );
    }, 1500);
    return () => {
      window.clearTimeout(timer);
      abort.abort();
    };
  }, [scanning, host, path, key, answer]);

  if (!agents.length) return null;

  type Row = { text: string; fill: string; label: string; note?: string; bad?: boolean; host?: boolean };
  const rows: Row[] = remote
    ? completion?.ready
      ? completion.hits
          // The folder already typed out in full is not a suggestion.
          .filter((h) => `${host}:${h.path}` !== cwd.trim())
          .map((h) => ({ text: `${host}:${h.path}`, fill: `${host}:${h.path.replace(/\/$/, "")}/`, label: h.path, note: h.problem ?? undefined, bad: !!h.problem }))
      : []
    : hostHits(hosts, cwd).map((h) => ({ text: `${h.name}:`, fill: `${h.name}:`, label: `${h.name}:`, note: h.aliases.join(" "), host: true }));

  const take = (row: Row) => {
    setCwd(row.fill);
    setPick(-1);
    field.current?.focus();
  };

  const blocked = remote ? localOnly[agent] : undefined;
  const unusable = remote && verdict?.ready ? verdict.problem : null;
  const note: { text: string; warn: boolean } | null = !remote
    ? null
    : asking && !completion
      ? { text: `connecting to ${host}…`, warn: false }
      : completion && !completion.ready
        ? { text: offlineNote(host, completion), warn: true }
        : completion?.ready && completion.missing
          ? { text: `${completion.dir} isn't there on ${host}`, warn: true }
          : completion?.ready && completion.failed
            ? { text: `couldn't list ${completion.dir}: ${completion.failed}`, warn: true }
            : null;

  const start = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      const res = await ask("/api/launch", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ agent, cwd: cwd.trim() }) });
      const done = await res.json().catch(() => ({}));
      toast.success(done.message || "Started");
      setOpen(false);
    } catch (err) {
      toast.error(String((err as Error).message || err));
    } finally {
      setBusy(false);
    }
  };

  const onKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (!rows.length) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      // -1 is the field itself, so the arrows go round through it.
      setPick((p) => {
        const next = p + step;
        return next < -1 ? rows.length - 1 : next >= rows.length ? -1 : next;
      });
    } else if ((e.key === "Enter" || e.key === "Tab") && pick >= 0 && pick < rows.length) {
      e.preventDefault();
      take(rows[pick]);
    } else if (e.key === "Tab" && rows.length === 1) {
      e.preventDefault();
      take(rows[0]);
    } else if (e.key === "Escape" && pick >= 0) {
      e.preventDefault();
      e.stopPropagation();
      setPick(-1);
    }
  };

  return (
    <>
      <Button size="sm" onClick={() => setOpen(true)}>
        <Plus /> New agent
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="sm:max-w-md">
          <form onSubmit={start} className="grid gap-4">
            <DialogHeader>
              <DialogTitle>Start an agent</DialogTitle>
              <DialogDescription>In cctop's multiplexer, so it shows up as a tab and in the workspace.</DialogDescription>
            </DialogHeader>
            <div className="grid gap-1.5">
              <label className="text-sm font-medium" htmlFor="launch-agent">Agent</label>
              <Select value={agent} onValueChange={setAgent}>
                <SelectTrigger id="launch-agent" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {agents.map((a) => (
                    <SelectItem key={a} value={a} disabled={!!remote && !!localOnly[a]}>
                      {a}
                      {remote && localOnly[a] && <span className="text-muted-foreground text-xs">— not on a remote host yet</span>}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {blocked && <p className="text-destructive text-xs">{blocked}</p>}
            </div>
            <div className="grid gap-1.5">
              <label className="text-sm font-medium" htmlFor="launch-cwd">Directory</label>
              <Input
                id="launch-cwd"
                ref={field}
                value={cwd}
                onChange={(e) => {
                  setCwd(e.target.value);
                  setPick(-1);
                }}
                onKeyDown={onKey}
                onFocus={() => setFocused(true)}
                onBlur={() => window.setTimeout(() => setFocused(false), 150)}
                placeholder={hosts.length ? "~ (the default), or host:path" : "~ (the default)"}
                spellCheck={false}
                autoComplete="off"
                autoCapitalize="off"
                role="combobox"
                aria-expanded={focused && rows.length > 0}
                aria-controls="launch-cwd-hits"
              />
              {focused && rows.length > 0 && (
                <ul id="launch-cwd-hits" role="listbox" className="bg-popover max-h-56 overflow-y-auto rounded-md border p-1 text-sm">
                  {rows.map((row, i) => (
                    <li key={row.text} role="option" aria-selected={i === pick}>
                      <button
                        type="button"
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={() => take(row)}
                        className={cn(
                          "flex w-full min-w-0 items-center gap-2 rounded-sm px-2 py-1.5 text-left",
                          i === pick ? "bg-accent text-accent-foreground" : "hover:bg-accent/60",
                        )}
                      >
                        {row.host && <Server className="text-muted-foreground size-3.5 shrink-0" />}
                        <span className={cn("truncate font-mono text-xs", row.bad && "text-muted-foreground")}>{row.label}</span>
                        {row.note && <span className={cn("ml-auto shrink-0 truncate text-xs", row.bad ? "text-destructive" : "text-muted-foreground")}>{row.note}</span>}
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              {note && (
                <p className={cn("flex items-center gap-1.5 text-xs", note.warn ? "text-amber-600 dark:text-amber-400" : "text-muted-foreground")}>
                  {note.warn ? <CircleAlert className="size-3.5 shrink-0" /> : <Loader2 className="size-3.5 shrink-0 animate-spin" />}
                  {note.text}
                </p>
              )}
              {remote && !note && verdict?.ready && (
                <p className={cn("flex items-center gap-1.5 text-xs", unusable ? "text-destructive" : "text-muted-foreground")}>
                  {unusable ? <CircleAlert className="size-3.5 shrink-0" /> : <CircleCheck className="size-3.5 shrink-0 text-emerald-600 dark:text-emerald-400" />}
                  {unusable ? `can't work in ${path || "~"}: ${unusable}` : `${host}:${path || "~"} can be worked in`}
                </p>
              )}
            </div>
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setOpen(false)}>Cancel</Button>
              <Button type="submit" disabled={busy || !agent || !!blocked || !!unusable || (!!remote && asking)}>
                <Play /> Start
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
