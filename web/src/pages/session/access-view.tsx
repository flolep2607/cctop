import { useEffect, useState } from "react";
import { ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { getJson } from "@/lib/api";
import { bytes } from "@/lib/format";
import { Badge } from "@/components/ui/badge";
import { DataTable, Panel, Section } from "@/components/section";
import type { Report } from "@/lib/types";

type RuleFile = { path: string; scope: string; present: boolean; bytes: number; head?: string; clipped?: boolean };

function Rule({ file }: { file: RuleFile }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="border-t first:border-t-0">
      <button
        type="button"
        disabled={!file.present}
        onClick={() => setOpen(!open)}
        className={cn("flex w-full flex-wrap items-baseline gap-2.5 px-4 py-2 text-left text-[13px]", file.present ? "hover:bg-muted/50" : "text-muted-foreground cursor-default")}
      >
        <ChevronRight className={cn("size-3.5 shrink-0 self-center transition-transform", open && "rotate-90", !file.present && "opacity-0")} />
        <span className="font-mono text-[12.5px] break-all">{file.path}</span>
        <Badge variant="outline">{file.scope}</Badge>
        <span className="text-muted-foreground text-xs">{file.present ? bytes(file.bytes) : "not there"}</span>
      </button>
      {open && file.present && (
        <pre className="max-h-[50vh] overflow-y-auto border-t px-4 py-2.5 font-mono text-xs break-words whitespace-pre-wrap">
          {(file.head || "") + (file.clipped ? "\n\n… cut for length" : "")}
        </pre>
      )}
    </div>
  );
}

function Rules({ title, files }: { title: string; files?: RuleFile[] }) {
  if (!files?.length) return null;
  return (
    <Section title={title}>
      <Panel pad={false}>
        {files.map((f) => (
          <Rule key={f.path} file={f} />
        ))}
      </Panel>
    </Section>
  );
}

// What a session in this directory can reach. Read on first sight of the view
// — another pass over the disk — and read again if that read failed.
export function AccessView({ id }: { id: string }) {
  const [a, setA] = useState<Report | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    getJson<Report>("/api/access/" + encodeURIComponent(id)).then(setA, (e) => setError(String(e.message || e)));
  }, [id]);
  if (error) return <Panel className="text-destructive p-10 text-center text-sm">{error}</Panel>;
  if (!a) return <Panel className="text-muted-foreground p-10 text-center text-sm">Reading what it can reach…</Panel>;
  const kv: [string, string | null | undefined][] = [
    ["Directory", a.cwd + (a.cwd_exists ? "" : " (gone)")],
    ["Branch", a.branch],
    ["Harness", a.harness],
    ["Model", a.model],
    ["Permission", a.permission ? a.permission + " — " + (a.permission_detail || "") : null],
    ["Process", a.pid ? "pid " + a.pid : null],
  ];
  return (
    <>
      <Section title="In scope" note="The files and servers that apply to a session in this directory — which is checkable. What the harness actually loaded is its own business, and this does not claim to know it.">
        <Panel>
          <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1.5 text-[13px]">
            {kv.filter(([, v]) => v).map(([k, v]) => (
              <div key={k} className="contents">
                <dt className="text-muted-foreground">{k}</dt>
                <dd className="break-all">{v}</dd>
              </div>
            ))}
          </dl>
          {a.note && <p className="text-muted-foreground mt-3 text-xs">{a.note}</p>}
        </Panel>
      </Section>
      <Rules title="Instructions" files={a.instructions} />
      <Rules title="Settings" files={a.configs} />
      {a.mcp?.length > 0 && (
        <Section title="MCP servers" note="Tools reaching outside this machine's filesystem come from here.">
          <Panel>
            <DataTable
              head={[["Server"], ["Scope"], ["Command"]]}
              rows={a.mcp.map((s: { name: string; scope: string; command?: string }) => [
                <span key="name" className="font-mono text-xs">{s.name}</span>, s.scope, <span key="command" className="text-muted-foreground font-mono text-xs break-all">{s.command || "—"}</span>,
              ])}
            />
          </Panel>
        </Section>
      )}
      {a.skills?.length > 0 && (
        <Section title="Skills" note={(a.skills_dirs ?? []).join("  ")}>
          <Panel pad={false}>
            {a.skills.map((s: { name: string; description?: string }) => (
              <div key={s.name} className="border-t px-4 py-2 text-[13px] first:border-t-0">
                <span className="font-mono">{s.name}</span>
                {s.description && <span className="text-muted-foreground"> {s.description}</span>}
              </div>
            ))}
          </Panel>
        </Section>
      )}
      {a.tools?.length > 0 && (
        <Section title="Tools it has used" note="What it reached for, not what it was offered.">
          <Panel>
            <div className="flex flex-wrap gap-1.5">
              {a.tools.map((t: { name: string; count: number }) => (
                <Badge key={t.name} variant="outline" className="gap-1.5 font-normal">
                  {t.name}
                  <span className="text-muted-foreground font-mono">{t.count}</span>
                </Badge>
              ))}
            </div>
          </Panel>
        </Section>
      )}
      {a.hooks?.length > 0 && (
        <Section title="cctop's own hooks" note="Whether this harness reports its state to cctop as it happens, rather than being read off disk after the fact.">
          <Panel>
            <DataTable
              head={[["Harness"], ["Scope"], ["State"], ["File"]]}
              rows={a.hooks.map((h: { harness: string; scope: string; state: string; detail?: string; path: string }) => [
                h.harness,
                h.scope,
                <span key="state">
                  <Badge variant={h.state === "broken" ? "destructive" : "outline"}>{h.state}</Badge>
                  {h.detail && <span className="text-muted-foreground text-xs"> {h.detail}</span>}
                </span>,
                <span key="path" className="text-muted-foreground font-mono text-xs break-all">{h.path}</span>,
              ])}
            />
          </Panel>
        </Section>
      )}
      {a.writes?.length > 0 && (
        <Section title="Written lately">
          <Panel>
            <div className="columns-1 gap-6 font-mono text-xs md:columns-2">
              {a.writes.map((p: string) => (
                <div key={p} className="text-muted-foreground break-inside-avoid py-0.5 break-all">{p}</div>
              ))}
            </div>
          </Panel>
        </Section>
      )}
    </>
  );
}
