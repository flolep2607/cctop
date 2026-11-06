// A file's diff laid out the way GitHub lays out a split diff — the same
// layout the terminal's diff view draws (crates/ui/src/diffview.rs), from the
// same answer: git's hunks, and the file's two versions to open the unchanged
// stretches between them.
//
// Pure, so `npm test` covers it without a browser.

/** One hunk of git's diff. */
export interface Hunk {
  /** First line and count, on each side. */
  old: [number, number];
  new: [number, number];
  /** What git prints after the second `@@`: the enclosing function, if any. */
  heading: string;
  /** ' ', '-' or '+', and the line. */
  lines: [string, string][];
}

/** Char ranges of a line that a comparison found changed. */
export type Marks = [number, number][];

export interface Cell { no: number; text: string; marks: Marks }

export type Row =
  | { kind: "hunk"; header: string }
  | { kind: "fold"; id: number; old: number; new: number; count: number; canOpen: boolean }
  | { kind: "same"; old: number; new: number; text: string }
  | { kind: "pair"; left?: Cell; right?: Cell };

const HEADER = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@ ?(.*)$/;

/** git's diff, from the first hunk on, as hunks. One with no header — an untracked file — is one hunk of all of it. */
export function parseHunks(diff: string[]): Hunk[] {
  const hunks: Hunk[] = [];
  let cur: Hunk | null = null;
  for (const line of diff) {
    const m = HEADER.exec(line);
    if (m) {
      cur = {
        old: [+m[1], m[2] === undefined ? 1 : +m[2]],
        new: [+m[3], m[4] === undefined ? 1 : +m[4]],
        heading: m[5] ?? "",
        lines: [],
      };
      hunks.push(cur);
      continue;
    }
    if (line.startsWith("\\")) continue; // "\ No newline at end of file"
    if (!cur) {
      cur = { old: [0, 0], new: [1, 0], heading: "", lines: [] };
      hunks.push(cur);
    }
    const sign = line[0] === "+" || line[0] === "-" ? line[0] : " ";
    cur.lines.push([sign, line.slice(1)]);
  }
  // The headerless hunk's size is its lines: git gave no header to say so.
  const bare = hunks[0];
  if (bare && !HEADER.test(diff[0] ?? "")) bare.new[1] = bare.lines.length;
  return hunks;
}

/** A hunk's header as git prints it. */
export function hunkHeader(h: Hunk): string {
  const range = ([at, len]: [number, number]) => (len === 1 ? `${at}` : `${at},${len}`);
  return `@@ -${range(h.old)} +${range(h.new)} @@${h.heading ? " " + h.heading : ""}`;
}

/** The first line of a side, counting an empty side's 0 as before line 1. */
const first = ([at, len]: [number, number]) => (len === 0 ? at + 1 : at);

/** Unchanged lines folded into one row unless there are at least this many. */
const FOLD_MIN = 4;

/**
 * The rows of a file's diff. Folds in `opened` (or all, with `all`) are
 * opened from the new version's text when it was given.
 */
export function buildRows(
  hunks: Hunk[],
  oldLines: string[] | null,
  newLines: string[] | null,
  opened: Set<number>,
  all: boolean,
): Row[] {
  const rows: Row[] = [];
  let fold = 0;
  // Where the previous hunk left each side.
  let atOld = 1;
  let atNew = 1;
  const gap = (old: number, nw: number, count: number) => {
    if (count <= 0) return;
    const id = fold++;
    const text = newLines ?? oldLines;
    const canOpen = !!text && (newLines ? nw - 1 + count <= newLines.length : old - 1 + count <= (oldLines?.length ?? 0));
    if (canOpen && (all || opened.has(id) || count < FOLD_MIN)) {
      for (let i = 0; i < count; i++) {
        const t = newLines ? newLines[nw - 1 + i] : oldLines![old - 1 + i];
        rows.push({ kind: "same", old: old + i, new: nw + i, text: t ?? "" });
      }
    } else {
      rows.push({ kind: "fold", id, old, new: nw, count, canOpen });
    }
  };
  for (const h of hunks) {
    const startOld = first(h.old);
    const startNew = first(h.new);
    gap(atOld, atNew, Math.min(startOld - atOld, startNew - atNew));
    if (h.old[0] !== 0 || h.lines.some(([s]) => s !== "+")) rows.push({ kind: "hunk", header: hunkHeader(h) });
    let o = startOld;
    let n = startNew;
    let removed: Cell[] = [];
    let added: Cell[] = [];
    const flush = () => {
      const len = Math.max(removed.length, added.length);
      for (let i = 0; i < len; i++) {
        const left = removed[i];
        const right = added[i];
        if (left && right) {
          const [lm, rm] = wordMarks(left.text, right.text);
          left.marks = lm;
          right.marks = rm;
        }
        rows.push({ kind: "pair", left, right });
      }
      removed = [];
      added = [];
    };
    for (const [sign, text] of h.lines) {
      if (sign === "-") removed.push({ no: o++, text, marks: [] });
      else if (sign === "+") added.push({ no: n++, text, marks: [] });
      else {
        flush();
        rows.push({ kind: "same", old: o++, new: n++, text });
      }
    }
    flush();
    atOld = o;
    atNew = n;
  }
  // What follows the last hunk, as far as the versions say the file goes.
  const tail = newLines ? newLines.length - (atNew - 1) : oldLines ? oldLines.length - (atOld - 1) : 0;
  if (hunks.length) gap(atOld, atNew, tail);
  return rows;
}

/** Words, runs of space, and single other chars, each with the char (not UTF-16) offset it starts at. */
function tokens(line: string): [number, string][] {
  const out: [number, string][] = [];
  let at = 0;
  for (const m of line.matchAll(/[\p{L}\p{N}_]+|\s+|./gsu)) {
    out.push([at, m[0]]);
    at += [...m[0]].length;
  }
  return out;
}

/** Tokens per side past which a pair is not compared: the comparison is quadratic. */
const WORDS_MAX = 400;

/**
 * The char ranges of `a` and `b` that differ, by the longest common run of
 * tokens: what GitHub washes stronger inside a changed line.
 */
export function wordMarks(a: string, b: string): [Marks, Marks] {
  const ta = tokens(a);
  const tb = tokens(b);
  if (ta.length > WORDS_MAX || tb.length > WORDS_MAX) return [[], []];
  const n = ta.length;
  const m = tb.length;
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      lcs[i][j] = ta[i][1] === tb[j][1] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  const ka = new Array<boolean>(n).fill(false);
  const kb = new Array<boolean>(m).fill(false);
  for (let i = 0, j = 0; i < n && j < m; ) {
    if (ta[i][1] === tb[j][1]) {
      ka[i++] = true;
      kb[j++] = true;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) i++;
    else j++;
  }
  // A line that shares nothing is a rewrite: washing all of it says nothing more.
  if (!ka.some(Boolean)) return [[], []];
  const ranges = (t: [number, string][], kept: boolean[]): Marks => {
    const out: Marks = [];
    t.forEach(([at, text], i) => {
      if (kept[i] || !text.trim()) return;
      const start = at;
      const end = start + [...text].length;
      const last = out[out.length - 1];
      if (last && last[1] >= start - 1) last[1] = end;
      else out.push([start, end]);
    });
    return out;
  };
  return [ranges(ta, ka), ranges(tb, kb)];
}

/** A changed file, as the tree lists it. */
export interface TreeFile { path: string; added: number | null; removed: number | null; untracked: boolean }

export interface Folder { name: string; path: string; added: number; removed: number; count: number; folders: Folder[]; files: TreeFile[] }

/** The files grouped by folder, chains of single folders joined (`docs/explanation/`), folders first, each sorted. */
export function buildTree(files: TreeFile[]): Folder {
  const root: Folder = { name: "", path: "", added: 0, removed: 0, count: 0, folders: [], files: [] };
  for (const f of files) {
    const parts = f.path.split("/");
    let dir = root;
    const add = (d: Folder) => {
      d.added += f.added ?? 0;
      d.removed += f.removed ?? 0;
      d.count++;
    };
    add(dir);
    for (const part of parts.slice(0, -1)) {
      let next = dir.folders.find((d) => d.name === part);
      if (!next) dir.folders.push((next = { name: part, path: dir.path ? `${dir.path}/${part}` : part, added: 0, removed: 0, count: 0, folders: [], files: [] }));
      dir = next;
      add(dir);
    }
    dir.files.push(f);
  }
  const tidy = (d: Folder): Folder => {
    d.folders = d.folders.map(tidy);
    // One folder and nothing else: shown as one row.
    while (d.name && d.folders.length === 1 && d.files.length === 0) {
      const only = d.folders[0];
      d = { ...only, name: `${d.name}/${only.name}` };
    }
    d.folders.sort((a, b) => a.name.localeCompare(b.name));
    d.files.sort((a, b) => a.path.localeCompare(b.path));
    return d;
  };
  return tidy(root);
}

/** The files in the tree's order: what next and previous step through. */
export function treeOrder(root: Folder): TreeFile[] {
  return [...root.folders.flatMap(treeOrder), ...root.files];
}

/** GitHub's five squares: how many of them are green, the rest red. */
export function squares(added: number, removed: number): number {
  const total = added + removed;
  return total === 0 ? 0 : Math.min(5, Math.round((added * 5) / total));
}
