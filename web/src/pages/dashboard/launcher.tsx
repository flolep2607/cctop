import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronRight, CircleAlert, CircleCheck, Loader2, Play, Plus, Server } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { ask, getJson } from "@/lib/api";
import { CAN_ACT } from "@/lib/config";
import { shortModel } from "@/lib/format";
import { mintTab, popOut } from "@/lib/terminal";
import type { Session } from "@/lib/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";

type SshHost = { name: string; aliases: string[] };
type Hit = { path: string; problem: string | null };
type Offline = { ready: false; why: string; prompt: boolean };
type Completion = Offline | { ready: true; dir: string; missing: boolean; failed: string | null; scanning: boolean; hits: Hit[] };
type Verdict = Offline | { ready: true; problem: string | null };

// What a harness's options offer (`launch_offer` on the server): the models
// after its default, its permission settings with the default first, and the
// machine's accounts for it.
type Offer = { models: string[] | null; permissions: { word: string; label: string }[]; accounts: string[] };
type Machine = { name: string; offline: string | null };
type AgentsDoc = {
  agents?: string[];
  actions?: boolean;
  local_only?: Record<string, string>;
  // Absent from a cctop older than the options: it gets a bare launch, never
  // settings it would drop without a word.
  options?: Record<string, Offer>;
  machines?: Machine[];
};
// What was set for one harness on one machine; empty is its default.
type Picked = { model: string; permissions: string; account: string; branch: string };
const BLANK: Picked = { model: "", permissions: "", account: "", branch: "" };
// The Select's value for this machine: Radix keeps "" for "nothing chosen".
const HERE = ".";
const BYPASS = "bypass";
const LOUD = "text-amber-600 dark:text-amber-400";

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

// The options row's one line: what differs from a bare start, as the
// terminal's launcher row says it.
function summary(p: Picked): string {
  return [p.model && shortModel(p.model), p.permissions && (p.permissions === BYPASS ? "YOLO" : p.permissions), p.account, p.branch && `⑂ ${p.branch}`]
    .filter(Boolean)
    .join(" · ");
}

// Starting a fresh agent from the page — the TUI's launcher, carried to the
// one place a phone can reach. Only where this run can act. The Directory
// field takes `host:path` as the terminal's does; the hosts, completion and
// folder check are asked of routes only the full token reaches.
//
// A sibling on the Cloudflare account is a place to start one too: its own
// /api/agents and /api/launch, through this server's relay, so the sibling
// checks the launch as it checks its own page's, and this server draws the
// acting line as it does for every relayed action.
export function Launcher({ sessions }: { sessions: Session[] }) {
  const [here, setHere] = useState<AgentsDoc>({});
  const [open, setOpen] = useState(false);
  const [machine, setMachine] = useState("");
  // The picked sibling's answer, and the machine it was for.
  const [there, setThere] = useState<{ machine: string; doc: AgentsDoc | null; error: string }>({ machine: "", doc: null, error: "" });
  const [wanted, setWanted] = useState("");
  const [cwd, setCwd] = useState("");
  const [busy, setBusy] = useState(false);
  const [hosts, setHosts] = useState<SshHost[]>([]);
  // Per machine and harness, and kept for the page's life, as the terminal's
  // launcher keeps them for its run.
  const [picked, setPicked] = useState<Record<string, Picked>>({});
  const [showOpts, setShowOpts] = useState(false);
  // The host's last answer, and the `host:path` it was for.
  const [answer, setAnswer] = useState<{ key: string; completion: Completion | null; verdict: Verdict | null }>({ key: "", completion: null, verdict: null });
  const [pick, setPick] = useState(-1);
  const [focused, setFocused] = useState(false);
  const field = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!CAN_ACT) return;
    getJson<AgentsDoc>("/api/agents").then((r) => {
      if (r.actions === false) return;
      setHere(r);
    }, () => {});
  }, []);

  // The host list once per opening: a host added to ~/.ssh/config shows up
  // the next time the dialog does.
  useEffect(() => {
    if (!open) return;
    post<{ hosts: SshHost[] }>("/api/ssh/hosts", {}).then((r) => setHosts(r.hosts ?? []), () => setHosts([]));
  }, [open]);

  // The sibling's agents and options, asked of it each time it is picked:
  // what is installed there is its own to say.
  useEffect(() => {
    if (!open || !machine) return;
    let live = true;
    getJson<AgentsDoc>("/api/peer/" + encodeURIComponent(machine) + "/api/agents").then(
      (doc) => live && setThere({ machine, doc, error: doc.actions === false ? "actions off" : "" }),
      (e) => live && setThere({ machine, doc: null, error: String((e as Error).message || e) }),
    );
    return () => {
      live = false;
    };
  }, [open, machine]);

  const machines = here.machines ?? [];
  const askingThere = !!machine && there.machine !== machine;
  const doc: AgentsDoc | null = machine ? (askingThere ? null : there.doc) : here;
  const agents = doc?.actions === false ? [] : (doc?.agents ?? []);
  // The agent picked last, while the machine has it; its first otherwise.
  const agent = agents.includes(wanted) ? wanted : (agents[0] ?? "");
  const localOnly = doc?.local_only ?? {};
  const offer = doc?.options?.[agent];
  const key = `${machine}\n${agent}`;
  const opts = picked[key] ?? BLANK;
  const setOpt = (patch: Partial<Picked>) => setPicked((all) => ({ ...all, [key]: { ...(all[key] ?? BLANK), ...patch } }));

  // A sibling's directory is a path there: no `host:path`, which would name
  // its ssh hosts, not this machine's.
  const remote = machine ? null : parseLocation(cwd);
  const host = remote?.host ?? "";
  const path = remote?.path ?? "";
  const hostKey = host ? `${host}\n${path}` : "";
  const asking = !!host && answer.key !== hostKey;
  // The last listing stays up while the next is asked for, as long as it is
  // the same host's, so the list does not blink at every keystroke.
  const completion = host && answer.key.startsWith(`${host}\n`) ? answer.completion : null;
  const verdict = host && !asking ? answer.verdict : null;

  // A sibling's projects, from the rows it reports: most recent first.
  const projects = useMemo(() => {
    if (!machine) return [];
    const seen = new Set<string>();
    return sessions
      .filter((s) => s.host === machine && s.project)
      .sort((a, b) => b.last_active.localeCompare(a.last_active))
      .map((s) => s.project as string)
      .filter((p) => !seen.has(p) && !!seen.add(p));
  }, [sessions, machine]);

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
        ([completion, verdict]) => setAnswer({ key: hostKey, completion, verdict }),
        (err) => {
          if (abort.signal.aborted) return;
          setAnswer({ key: hostKey, completion: { ready: false, why: String((err as Error).message || err), prompt: false }, verdict: null });
        },
      );
    }, 250);
    return () => {
      window.clearTimeout(timer);
      abort.abort();
    };
  }, [host, path, hostKey]);

  // While a scan of the host's repositories is still running, ask again so
  // they arrive without another keystroke.
  const scanning = !asking && !!completion?.ready && completion.scanning;
  useEffect(() => {
    if (!scanning) return;
    const abort = new AbortController();
    const timer = window.setTimeout(() => {
      post<Completion>("/api/ssh/complete", { host, path }, abort.signal).then(
        (completion) => setAnswer((a) => (a.key === hostKey ? { ...a, completion } : a)),
        () => {},
      );
    }, 1500);
    return () => {
      window.clearTimeout(timer);
      abort.abort();
    };
  }, [scanning, host, path, hostKey, answer]);

  if (!here.agents?.length) return null;

  type Row = { text: string; fill: string; label: string; note?: string; bad?: boolean; host?: boolean };
  const typed = cwd.trim();
  const rows: Row[] = machine
    ? projects
        .filter((p) => p !== typed && p.toLowerCase().includes(typed.toLowerCase()))
        .slice(0, 6)
        .map((p) => ({ text: p, fill: p, label: p }))
    : remote
      ? completion?.ready
        ? completion.hits
            // The folder already typed out in full is not a suggestion.
            .filter((h) => `${host}:${h.path}` !== typed)
            .map((h) => ({ text: `${host}:${h.path}`, fill: `${host}:${h.path.replace(/\/$/, "")}/`, label: h.path, note: h.problem ?? undefined, bad: !!h.problem }))
        : []
      : hostHits(hosts, cwd).map((h) => ({ text: `${h.name}:`, fill: `${h.name}:`, label: `${h.name}:`, note: h.aliases.join(" "), host: true }));

  const take = (row: Row) => {
    setCwd(row.fill);
    setPick(-1);
    field.current?.focus();
  };

  // Where each option applies, as in the terminal's launcher: an account is
  // this machine's, so not a sibling's; a worktree is made here, so not on an
  // ssh host either. What does not apply is neither shown nor sent.
  const accounts = !machine && offer && offer.accounts.length > 1 ? offer.accounts : [];
  const canBranch = !!offer && !machine && !remote;
  const applied: Picked = {
    model: offer?.models ? opts.model.trim() : "",
    permissions: offer?.permissions.some((p) => p.word === opts.permissions) ? opts.permissions : "",
    account: accounts.includes(opts.account) ? opts.account : "",
    branch: canBranch ? opts.branch.trim() : "",
  };
  const hasOptions = !!offer && (!!offer.models || offer.permissions.length > 0 || accounts.length > 0 || canBranch);
  const said = summary(applied);

  const blocked = remote ? localOnly[agent] : undefined;
  const unusable = remote && verdict?.ready ? verdict.problem : null;
  const note: { text: string; warn: boolean } | null = machine
    ? askingThere
      ? { text: `${machine}…`, warn: false }
      : there.error
        ? { text: `${machine}: ${there.error}`, warn: true }
        : null
    : !remote
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

  // Every launch goes to /api/launch — this server's, or the sibling's
  // through the relay — and its tab's terminal is asked of the same place.
  const base = machine ? "/api/peer/" + encodeURIComponent(machine) : "";

  const start = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!agent) return;
    setBusy(true);
    const body: Record<string, string> = { agent, cwd: typed };
    for (const [name, value] of Object.entries(applied)) if (value) body[name] = value;
    try {
      const res = await ask(base + "/api/launch", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
      const done = (await res.json().catch(() => ({}))) as { rmux?: string | null };
      // A branch is spent by the launch that made its worktree.
      if (body.branch) setOpt({ branch: "" });
      const where = machine || host;
      const title = where ? `${agent} · ${where}` : agent;
      const tab = done.rmux;
      toast.success(`Started ${title}`, {
        action: tab
          ? {
              label: "Open",
              onClick: () =>
                popOut({
                  key: (machine ? machine + "-" : "") + tab,
                  title: title + " — cctop",
                  release: () => {},
                  mint: () => mintTab(tab, { base, fresh: true }),
                  onClosed: () => {},
                }),
            }
          : undefined,
      });
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

  const permission = applied.permissions || offer?.permissions[0]?.word || "";
  // YOLO in the colour of a warning wherever it shows, as in the terminal.
  const loud = permission === BYPASS;

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
              <DialogDescription>A cctop tab · in the workspace</DialogDescription>
            </DialogHeader>
            {machines.length > 0 && (
              <div className="grid gap-1.5">
                <label className="text-sm font-medium" htmlFor="launch-machine">Machine</label>
                <Select
                  value={machine || HERE}
                  onValueChange={(v) => {
                    setMachine(v === HERE ? "" : v);
                    setCwd("");
                    setPick(-1);
                  }}
                >
                  <SelectTrigger id="launch-machine" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={HERE}>
                      <span className="size-2 shrink-0 rounded-full bg-emerald-500" />
                      this machine
                    </SelectItem>
                    {machines.map((m) => (
                      <SelectItem key={m.name} value={m.name} disabled={!!m.offline}>
                        <span className={cn("size-2 shrink-0 rounded-full", m.offline ? "bg-muted-foreground/40" : "bg-emerald-500")} />
                        {m.name}
                        {m.offline && <span className="text-muted-foreground min-w-0 truncate text-xs">offline · {m.offline}</span>}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}
            <div className="grid gap-1.5">
              <label className="text-sm font-medium" htmlFor="launch-agent">Agent</label>
              <Select value={agent} onValueChange={setWanted} disabled={!agents.length}>
                <SelectTrigger id="launch-agent" className="w-full">
                  <SelectValue placeholder="—" />
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
                placeholder={machine ? `~ on ${machine}` : hosts.length ? "~ (the default), or host:path" : "~ (the default)"}
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
                <p className={cn("flex items-center gap-1.5 text-xs", note.warn ? LOUD : "text-muted-foreground")}>
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
            {hasOptions && offer && (
              <div className="grid gap-3">
                <button
                  type="button"
                  onClick={() => setShowOpts((s) => !s)}
                  aria-expanded={showOpts}
                  className="text-muted-foreground hover:text-foreground flex min-w-0 items-center gap-1.5 text-left text-sm"
                >
                  <ChevronRight className={cn("size-4 shrink-0 transition-transform", showOpts && "rotate-90")} />
                  <span className="text-foreground font-medium">Options</span>
                  {said && <span className={cn("truncate text-xs", loud && LOUD)}>{said}</span>}
                </button>
                {showOpts && (
                  <div className="grid grid-cols-[auto_minmax(0,1fr)] items-center gap-x-3 gap-y-2 pl-5">
                    {offer.models && (
                      <>
                        <label className="text-muted-foreground text-xs" htmlFor="launch-model">Model</label>
                        <Input
                          id="launch-model"
                          value={opts.model}
                          onChange={(e) => setOpt({ model: e.target.value })}
                          list="launch-models"
                          placeholder="default"
                          spellCheck={false}
                          autoComplete="off"
                          autoCapitalize="off"
                          className="h-8"
                        />
                        <datalist id="launch-models">
                          {offer.models.map((m) => (
                            <option key={m} value={m} />
                          ))}
                        </datalist>
                      </>
                    )}
                    {offer.permissions.length > 0 && (
                      <>
                        <label className="text-muted-foreground text-xs" htmlFor="launch-perms">Permissions</label>
                        <Select value={permission} onValueChange={(v) => setOpt({ permissions: v === offer.permissions[0]?.word ? "" : v })}>
                          <SelectTrigger id="launch-perms" size="sm" className={cn("w-full", loud && LOUD)}>
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {offer.permissions.map((p) => (
                              <SelectItem key={p.word} value={p.word} className={cn(p.word === BYPASS && LOUD)}>
                                {p.label}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </>
                    )}
                    {accounts.length > 0 && (
                      <>
                        <label className="text-muted-foreground text-xs" htmlFor="launch-account">Account</label>
                        <Select value={applied.account || accounts[0]} onValueChange={(v) => setOpt({ account: v === accounts[0] ? "" : v })}>
                          <SelectTrigger id="launch-account" size="sm" className="w-full">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {accounts.map((a) => (
                              <SelectItem key={a} value={a}>{a}</SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </>
                    )}
                    {canBranch && (
                      <>
                        <label className="text-muted-foreground text-xs" htmlFor="launch-branch">Worktree</label>
                        <Input
                          id="launch-branch"
                          value={opts.branch}
                          onChange={(e) => setOpt({ branch: e.target.value })}
                          placeholder="branch"
                          spellCheck={false}
                          autoComplete="off"
                          autoCapitalize="off"
                          className="h-8"
                        />
                      </>
                    )}
                  </div>
                )}
              </div>
            )}
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
