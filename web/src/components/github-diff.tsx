import { useMemo, useState, type ReactNode } from "react";
import { ChevronsUpDown } from "lucide-react";
import { cn } from "@/lib/utils";
import { buildRows, parseHunks, squares, type Cell, type Marks, type Row } from "@/lib/diff";
import { highlightLines, type Colour } from "@/lib/highlight";

// One file's diff, drawn as GitHub draws it: the old file on the left and the
// new on the right on a wide screen, one above the other on a phone; both line
// numbers in the gutters; removed and added rows washed red and green, the
// words a changed line actually changed washed stronger; hunk headers with the
// function git named; the unchanged stretches between hunks folded to a row
// that opens them. The same layout as the terminal's diff view.

export interface DiffFile {
  path: string;
  added: number;
  removed: number;
  diff: string[];
  old: string[] | null;
  new: string[] | null;
  binary: boolean;
  tooBig: boolean;
}

/** A line's text, split where its syntax colour or its word mark changes. */
function Code({ text, marks, colours, mark }: { text: string; marks: Marks; colours: Colour[] | undefined; mark: string }) {
  const chars = [...text];
  if (!chars.length) return <>{" "}</>;
  const marked = (i: number) => marks.some(([a, b]) => i >= a && i < b);
  const colourAt = (i: number) => colours?.find(([a, b]) => i >= a && i < b)?.[2];
  const out: { text: string; marked: boolean; cls?: string }[] = [];
  chars.forEach((ch, i) => {
    const m = marked(i);
    const cls = colourAt(i);
    const last = out[out.length - 1];
    if (last && last.marked === m && last.cls === cls) last.text += ch;
    else out.push({ text: ch, marked: m, cls });
  });
  return (
    <>
      {out.map((s, i) => (
        <span key={i} className={cn(s.cls && `hl-${s.cls}`, s.marked && mark)}>
          {s.text}
        </span>
      ))}
    </>
  );
}

const gutter = "text-muted-foreground/70 select-none whitespace-nowrap px-2 text-right align-top tabular-nums";
const code = "whitespace-pre-wrap break-all pr-3 align-top";
const addRow = "bg-success/12";
const delRow = "bg-destructive/12";
const addMark = "rounded-[2px] bg-success/35";
const delMark = "rounded-[2px] bg-destructive/35";

function Squares({ added, removed }: { added: number; removed: number }) {
  const green = squares(added, removed);
  const total = added + removed;
  return (
    <span className="inline-flex gap-px" aria-hidden>
      {Array.from({ length: 5 }, (_, i) => (
        <span key={i} className={cn("size-2 rounded-[1px]", total === 0 ? "bg-muted" : i < green ? "bg-success" : "bg-destructive")} />
      ))}
    </span>
  );
}

export function GithubDiff({ file, split }: { file: DiffFile; split: boolean }) {
  const [opened, setOpened] = useState<Set<number>>(new Set());
  const [all, setAll] = useState(false);
  const hunks = useMemo(() => parseHunks(file.diff), [file.diff]);
  const rows = useMemo(() => buildRows(hunks, file.old, file.new, opened, all), [hunks, file.old, file.new, opened, all]);
  // Highlighted once per file, whole, from each version — or, without the
  // versions, from the hunk lines alone, which colours less and still helps.
  const colours = useMemo(() => {
    const side = (lines: string[] | null, fallback: (h: (typeof hunks)[number]) => [number, string][]) => {
      if (lines) return highlightLines(file.path, lines);
      const byNo = new Map<number, string>();
      for (const h of hunks) for (const [no, text] of fallback(h)) byNo.set(no, text);
      if (!byNo.size) return null;
      const max = Math.max(...byNo.keys());
      const sparse = Array.from({ length: max }, (_, i) => byNo.get(i + 1) ?? "");
      return highlightLines(file.path, sparse);
    };
    // One side's lines of a hunk, by their number on that side.
    const numbered = (sign: string) => (h: (typeof hunks)[number]) => {
      let no = sign === "-" ? h.old[0] : h.new[0];
      const out: [number, string][] = [];
      for (const [s, text] of h.lines) if (s === sign || s === " ") out.push([no++, text]);
      return out;
    };
    return { old: side(file.old, numbered("-")), new: side(file.new, numbered("+")) };
  }, [file.path, file.old, file.new, hunks]);
  const lineColours = (isNew: boolean, no: number) => (isNew ? colours.new : colours.old)?.[no - 1];
  const foldCount = rows.filter((r) => r.kind === "fold" && r.canOpen).length;

  const header = (
    <div className="bg-muted/40 flex flex-wrap items-center gap-2.5 border-b px-3 py-2">
      <span className="min-w-0 font-mono text-[12.5px] font-medium break-all">{file.path}</span>
      <span className="text-success font-mono text-xs">+{file.added}</span>
      <span className="text-destructive font-mono text-xs">−{file.removed}</span>
      <Squares added={file.added} removed={file.removed} />
      {foldCount > 0 && (
        <button type="button" onClick={() => setAll(!all)} className="text-muted-foreground hover:text-foreground ml-auto inline-flex items-center gap-1 text-xs">
          <ChevronsUpDown className="size-3.5" />
          {all ? "Fold unchanged" : "Show all lines"}
        </button>
      )}
    </div>
  );

  if (file.binary || file.tooBig) {
    return (
      <div className="overflow-hidden rounded-lg border">
        {header}
        <p className="text-muted-foreground px-4 py-6 text-center text-sm">
          {file.binary ? "A binary file: there are no lines to show." : "This diff is too big to draw here — open it in the terminal (Alt+g)."}
        </p>
      </div>
    );
  }

  const open = (id: number) => setOpened((s) => new Set(s).add(id));
  const cells = (cell: Cell | undefined, isNew: boolean) =>
    cell ? (
      <>
        <td className={cn(gutter, isNew ? addRow : delRow)}>{cell.no}</td>
        <td className={cn(code, isNew ? addRow : delRow)}>
          <span className={cn("select-none pr-1.5", isNew ? "text-success" : "text-destructive")}>{isNew ? "+" : "−"}</span>
          <Code text={cell.text} marks={cell.marks} colours={lineColours(isNew, cell.no)} mark={isNew ? addMark : delMark} />
        </td>
      </>
    ) : (
      <td colSpan={2} className="bg-muted/30" />
    );

  const rowFor = (r: Row, i: number) => {
    switch (r.kind) {
      case "hunk":
        return (
          <tr key={i} className="bg-primary/6 text-muted-foreground">
            <td colSpan={split ? 4 : 3} className="px-3 py-1 font-mono">{r.header}</td>
          </tr>
        );
      case "fold":
        return (
          <tr key={i} className="bg-muted/40 text-muted-foreground">
            <td colSpan={split ? 4 : 3} className="px-3 py-0.5">
              {r.canOpen ? (
                <button type="button" onClick={() => open(r.id)} className="hover:text-foreground w-full text-left">
                  ⋯ {r.count} unchanged lines · show
                </button>
              ) : (
                <span>⋯ {r.count} unchanged lines</span>
              )}
            </td>
          </tr>
        );
      case "same":
        return split ? (
          <tr key={i}>
            <td className={gutter}>{r.old}</td>
            <td className={code}>
              <span className="pr-1.5 select-none"> </span>
              <Code text={r.text} marks={[]} colours={lineColours(false, r.old) ?? lineColours(true, r.new)} mark="" />
            </td>
            <td className={cn(gutter, "border-l")}>{r.new}</td>
            <td className={code}>
              <span className="pr-1.5 select-none"> </span>
              <Code text={r.text} marks={[]} colours={lineColours(true, r.new) ?? lineColours(false, r.old)} mark="" />
            </td>
          </tr>
        ) : (
          <tr key={i}>
            <td className={gutter}>{r.old}</td>
            <td className={gutter}>{r.new}</td>
            <td className={code}>
              <span className="pr-1.5 select-none"> </span>
              <Code text={r.text} marks={[]} colours={lineColours(true, r.new) ?? lineColours(false, r.old)} mark="" />
            </td>
          </tr>
        );
      case "pair":
        return split ? (
          <tr key={i}>
            {cells(r.left, false)}
            {r.right ? (
              <>
                <td className={cn(gutter, addRow, "border-l")}>{r.right.no}</td>
                <td className={cn(code, addRow)}>
                  <span className="text-success pr-1.5 select-none">+</span>
                  <Code text={r.right.text} marks={r.right.marks} colours={lineColours(true, r.right.no)} mark={addMark} />
                </td>
              </>
            ) : (
              <td colSpan={2} className="bg-muted/30 border-l" />
            )}
          </tr>
        ) : null;
    }
  };

  // Unified: a change's removed lines above its added ones, as git prints them.
  const unified = () => {
    const out: ReactNode[] = [];
    let removed: Cell[] = [];
    let added: Cell[] = [];
    const flush = (key: string) => {
      removed.forEach((c, j) =>
        out.push(
          <tr key={`${key}-r${j}`}>
            <td className={cn(gutter, delRow)}>{c.no}</td>
            <td className={cn(gutter, delRow)} />
            <td className={cn(code, delRow)}>
              <span className="text-destructive pr-1.5 select-none">−</span>
              <Code text={c.text} marks={c.marks} colours={lineColours(false, c.no)} mark={delMark} />
            </td>
          </tr>,
        ),
      );
      added.forEach((c, j) =>
        out.push(
          <tr key={`${key}-a${j}`}>
            <td className={cn(gutter, addRow)} />
            <td className={cn(gutter, addRow)}>{c.no}</td>
            <td className={cn(code, addRow)}>
              <span className="text-success pr-1.5 select-none">+</span>
              <Code text={c.text} marks={c.marks} colours={lineColours(true, c.no)} mark={addMark} />
            </td>
          </tr>,
        ),
      );
      removed = [];
      added = [];
    };
    rows.forEach((r, i) => {
      if (r.kind === "pair") {
        if (r.left) removed.push(r.left);
        if (r.right) added.push(r.right);
        return;
      }
      flush(`f${i}`);
      out.push(rowFor(r, i));
    });
    flush("end");
    return out;
  };

  return (
    <div className="overflow-hidden rounded-lg border">
      {header}
      <div className="overflow-x-auto">
        <table className="diff-code w-full table-fixed border-collapse font-mono text-[12px] leading-[1.5]">
          {split ? (
            <colgroup>
              <col className="w-12" />
              <col />
              <col className="w-12" />
              <col />
            </colgroup>
          ) : (
            <colgroup>
              <col className="w-12" />
              <col className="w-12" />
              <col />
            </colgroup>
          )}
          <tbody>{split ? rows.map(rowFor) : unified()}</tbody>
        </table>
      </div>
    </div>
  );
}
