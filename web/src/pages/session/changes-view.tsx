import { useState } from "react";
import { ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { Patch } from "@/components/patch";
import { Panel, Section } from "@/components/section";
import type { Report } from "@/lib/types";

interface FileDiff { file: string; added: number; removed: number; edits: number; hunks: string[]; truncated: boolean }

// One entry per file. Line counts come from the transcript's own tally; an
// entry with none is counted back out of its hunk lines.
export function diffFiles(r: Report): FileDiff[] {
  const byFile = new Map<string, FileDiff>();
  for (const f of r.diffs ?? []) {
    let got = byFile.get(f.file);
    if (!got) byFile.set(f.file, (got = { file: f.file, added: 0, removed: 0, edits: 0, hunks: [], truncated: false }));
    got.added += f.added || 0;
    got.removed += f.removed || 0;
    got.edits += f.edits || 1;
    got.hunks.push(...(f.hunks ?? []));
    got.truncated ||= !!f.truncated;
  }
  const files = [...byFile.values()];
  for (const f of files) {
    if (f.added || f.removed || !f.hunks.length) continue;
    for (const line of f.hunks) {
      if (line.startsWith("+") && !line.startsWith("+++")) f.added++;
      else if (line.startsWith("-") && !line.startsWith("---")) f.removed++;
    }
  }
  return files;
}

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

export function ChangesView({ r }: { r: Report }) {
  const files = diffFiles(r);
  const stats = new Map(files.map((d) => [d.file, d]));
  const total = files.reduce((a, f) => a + f.added + f.removed, 0);
  return (
    <>
      {files.length > 0 ? (
        <Section
          title="What it changed"
          note={`${files.length} ${files.length === 1 ? "file" : "files"}, ${total} changed lines. Click a file for its diff. Reconstructed from the patches the transcript recorded, so it is what the agent applied rather than what the file holds now.`}
        >
          <Panel pad={false}>
            {files.map((f) => (
              <FileRow key={f.file} f={f} />
            ))}
          </Panel>
        </Section>
      ) : (
        <Panel className="text-muted-foreground p-10 text-center text-sm">This session recorded no edits.</Panel>
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
