import { useState } from "react";
import { ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { Patch } from "@/components/patch";
import { clock } from "@/lib/format";
import { Panel, Section } from "@/components/section";
import type { Report } from "@/lib/types";
import { diffFiles, type FileDiff } from "./diff-files";

function FileRow({ f }: { f: FileDiff }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="border-t first:border-t-0">
      <button type="button" onClick={() => setOpen(!open)} className="hover:bg-muted/50 flex w-full flex-wrap items-baseline gap-2.5 px-4 py-2 text-left" aria-expanded={open}>
        <ChevronRight className={cn("text-muted-foreground size-3.5 shrink-0 self-center transition-transform", open && "rotate-90")} />
        <span className="min-w-0 font-mono text-[12.5px] break-all">{f.file}</span>
        {f.added > 0 && <span className="text-success font-mono text-xs">+{f.added}</span>}
        {f.removed > 0 && <span className="text-destructive font-mono text-xs">−{f.removed}</span>}
        {f.edits > 1 && <span className="text-muted-foreground text-xs">{f.edits} edits</span>}
      </button>
      {open && <Patch lines={f.hunks} truncated={f.truncated} className="border-t" />}
    </div>
  );
}

// Shell commands whose shape says they may write files. The transcript keeps
// the command, not what it did to the disk, so these are listed rather than
// diffed — a heuristic that errs towards listing: a redirect, an in-place
// edit, a script fed on stdin, a move, a delete, a git command that rewrites
// the tree. A redirect to /dev/null, or of one stream into another, is not a
// write, and neither is a numbered one (`2>file` is a log, not an edit).
const WRITES = /(^|[\s;&|(])(sed\s+-i|perl\s+-p?i|tee\s|cp\s|mv\s|rm\s|mkdir\s|touch\s|truncate\s|patch\s|python3?\s+-\s*<<|python3?\s+-c|node\s+-e|npx\s+\S*(prettier|shadcn|create)|git\s+(rm|mv|apply|checkout|restore|reset|stash|commit|merge|rebase|cherry-pick|am)\b|cargo\s+(fmt|add|update)|npm\s+(install|i|uninstall|update|create))|(?<![0-9&])>>?\s*(?!\/dev\/|&)[^\s&|>]/;

function shellWrites(r: Report): { detail: string; ts?: string }[] {
  return (r.activity?.calls ?? [])
    .filter((c: { tool: string; detail?: string }) => /^(Bash|bash|shell|exec_command|run_shell_command)$/.test(c.tool) && WRITES.test(c.detail ?? ""))
    .map((c: { detail: string; ts?: string }) => ({ detail: c.detail, ts: c.ts }));
}

export function ChangesView({ r }: { r: Report }) {
  const files = diffFiles(r);
  const shell = shellWrites(r);
  const shellCount = (r.activity?.tools ?? [])
    .filter((t: { name: string }) => /^(Bash|bash|shell|exec_command|run_shell_command)$/.test(t.name))
    .reduce((a: number, t: { calls: number }) => a + t.calls, 0);
  const stats = new Map(files.map((d) => [d.file, d]));
  const total = files.reduce((a, f) => a + f.added + f.removed, 0);
  return (
    <>
      {files.length > 0 ? (
        <Section
          title="What it changed"
          note={`${files.length} ${files.length === 1 ? "file" : "files"}, ${total} changed lines, from the agent's edit and write tools. Click a file for its diff. It is what the agent applied rather than what the file holds now${shellCount ? " — and not what its shell commands did, which are listed below." : "."}`}
        >
          <Panel pad={false}>
            {files.map((f) => (
              <FileRow key={f.file} f={f} />
            ))}
          </Panel>
        </Section>
      ) : (
        <Panel className="text-muted-foreground p-10 text-center text-sm">
          {shellCount ? "No edit or write tool calls — anything this session changed, it changed through the shell." : "This session recorded no edits."}
        </Panel>
      )}
      {shell.length > 0 && (
        <Section
          title="Shell commands that may have changed files"
          note={`${shell.length} of the ${shellCount} shell commands look like they write — an in-place edit, a redirect, a script, a move or a git command. The transcript keeps the command, not its effect on the disk, so these are listed rather than diffed. Newest first${(r.activity?.calls?.length ?? 0) < (r.activity?.tool_count ?? 0) ? ", from the calls the transcript still keeps" : ""}.`}
        >
          <Panel pad={false} className="max-h-[50vh] overflow-y-auto">
            {shell.map((c, i) => (
              <div key={i} className="flex gap-3 border-t px-4 py-1.5 first:border-t-0">
                <span className="text-muted-foreground shrink-0 font-mono text-[11px] leading-5">{clock(c.ts)}</span>
                <code className="min-w-0 font-mono text-xs leading-5 break-all whitespace-pre-wrap">{c.detail}</code>
              </div>
            ))}
          </Panel>
        </Section>
      )}
      {r.files?.length > 0 && (
        <Section title="Files it wrote" note="Most recent first. Line counts where the transcript kept the edits.">
          <Panel>
            <div className="columns-1 gap-6 font-mono text-xs md:columns-2">
              {r.files.map((f: string) => {
                const d = stats.get(f);
                return (
                  <div key={f} className="text-muted-foreground break-inside-avoid py-0.5 break-all">
                    {f}
                    {d && (d.added || d.removed) ? (
                      <>
                        {" "}
                        <span className="text-success">+{d.added}</span> <span className="text-destructive">−{d.removed}</span>
                      </>
                    ) : null}
                  </div>
                );
              })}
            </div>
          </Panel>
        </Section>
      )}
    </>
  );
}
