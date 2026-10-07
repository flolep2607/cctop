import type { Report } from "@/lib/types";

export interface FileDiff { file: string; added: number; removed: number; edits: number; hunks: string[]; truncated: boolean }

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
