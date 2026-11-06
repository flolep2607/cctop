import { useEffect, useMemo, useState } from "react";
import { ChevronRight, Folder as FolderIcon, RefreshCw } from "lucide-react";
import { cn } from "@/lib/utils";
import { getJson } from "@/lib/api";
import { usePoll } from "@/hooks/use-poll";
import { buildTree, treeOrder, type Folder, type TreeFile } from "@/lib/diff";
import { GithubDiff, type DiffFile } from "@/components/github-diff";
import { Panel, Section } from "@/components/section";
import { Button } from "@/components/ui/button";
import type { CheckoutChanges } from "@/lib/gen/CheckoutChanges";
import type { CheckoutDiff } from "@/lib/gen/CheckoutDiff";

// What the session changed in its checkout, as git sees it now: the same list
// and the same base as the terminal's sidebar, and a file's diff laid out the
// way GitHub lays one out.

/** Wide enough for the two halves side by side, as on GitHub. */
const SPLIT_QUERY = "(min-width: 1100px)";

function useWide(): boolean {
  const [wide, setWide] = useState(() => typeof window !== "undefined" && window.matchMedia(SPLIT_QUERY).matches);
  useEffect(() => {
    const q = window.matchMedia(SPLIT_QUERY);
    const on = () => setWide(q.matches);
    q.addEventListener("change", on);
    return () => q.removeEventListener("change", on);
  }, []);
  return wide;
}

function Counts({ added, removed }: { added: number | null; removed: number | null }) {
  if (added === null && removed === null) return <span className="text-muted-foreground text-[11px]">binary</span>;
  return (
    <span className="shrink-0 font-mono text-[11px]">
      {!!added && <span className="text-success">+{added}</span>} {!!removed && <span className="text-destructive">−{removed}</span>}
    </span>
  );
}

function Tree({ dir, depth, at, pick, shut, turn }: { dir: Folder; depth: number; at: string; pick: (p: string) => void; shut: Set<string>; turn: (p: string) => void }) {
  const pad = { paddingLeft: `${0.5 + depth * 0.85}rem` };
  return (
    <>
      {dir.folders.map((d) => (
        <div key={d.path}>
          <button type="button" onClick={() => turn(d.path)} style={pad} className="hover:bg-muted/60 flex w-full items-center gap-1.5 py-1 pr-2 text-left" aria-expanded={!shut.has(d.path)}>
            <ChevronRight className={cn("text-muted-foreground size-3 shrink-0 transition-transform", !shut.has(d.path) && "rotate-90")} />
            <FolderIcon className="text-muted-foreground size-3.5 shrink-0" />
            <span className="min-w-0 flex-1 truncate font-mono text-xs">{d.name}/</span>
            <span className="text-muted-foreground text-[11px]">{d.count}</span>
            <Counts added={d.added} removed={d.removed} />
          </button>
          {!shut.has(d.path) && <Tree dir={d} depth={depth + 1} at={at} pick={pick} shut={shut} turn={turn} />}
        </div>
      ))}
      {dir.files.map((f) => (
        <button
          type="button"
          key={f.path}
          onClick={() => pick(f.path)}
          style={{ paddingLeft: `${1.6 + depth * 0.85}rem` }}
          className={cn("hover:bg-muted/60 flex w-full items-center gap-2 py-1 pr-2 text-left", f.path === at && "bg-primary/10")}
          aria-current={f.path === at}
        >
          <span className={cn("min-w-0 flex-1 truncate font-mono text-xs", f.untracked && "italic")}>{f.path.split("/").pop()}</span>
          <Counts added={f.added} removed={f.removed} />
        </button>
      ))}
    </>
  );
}

export function CheckoutView({ id }: { id: string }) {
  const { data, error, refresh } = usePoll<CheckoutChanges | null>("/api/changes/" + encodeURIComponent(id), 15000);
  const wide = useWide();
  const [split, setSplit] = useState<boolean | null>(null);
  const [at, setAt] = useState<string>("");
  const [shut, setShut] = useState<Set<string>>(new Set());
  const [file, setFile] = useState<{ key: string; diff?: DiffFile; error?: string } | null>(null);

  const tree = useMemo(() => (data ? buildTree(data.files as TreeFile[]) : null), [data]);
  const order = useMemo(() => (tree ? treeOrder(tree) : []), [tree]);
  const chosen = order.find((f) => f.path === at) ?? order[0];

  // The chosen file's diff, read when it is chosen and again when the list
  // says it changed.
  const key = data && chosen ? `${data.base}\n${chosen.path}\n${chosen.added}\n${chosen.removed}` : "";
  useEffect(() => {
    if (!data || !chosen || !key || file?.key === key) return;
    let live = true;
    getJson<CheckoutDiff>("/api/diff/" + encodeURIComponent(id), { path: chosen.path, base: data.base, untracked: chosen.untracked ? 1 : null })
      .then((d) => {
        if (!live) return;
        setFile({
          key,
          diff: { path: chosen.path, added: chosen.added ?? 0, removed: chosen.removed ?? 0, diff: d.diff, old: d.old, new: d.new, binary: d.binary, tooBig: d.too_big },
        });
      })
      .catch((e: unknown) => live && setFile({ key, error: String(e instanceof Error ? e.message : e) }));
    return () => {
      live = false;
    };
  }, [data, chosen, id, key, file?.key]);
  // A read for another file, or none yet, is not this file's.
  const shown = file?.key === key ? file : null;

  if (data === null && !error) return null;
  if (error && !data) {
    return (
      <Section title="In its checkout">
        <Panel className="text-muted-foreground p-6 text-center text-sm">{error}</Panel>
      </Section>
    );
  }
  if (!data || !tree) return null;
  const added = data.files.reduce((a, f) => a + (f.added ?? 0), 0);
  const removed = data.files.reduce((a, f) => a + (f.removed ?? 0), 0);
  const step = (by: number) => {
    const i = order.findIndex((f) => f.path === chosen?.path);
    const next = order[(i + by + order.length) % order.length];
    if (next) setAt(next.path);
  };
  const isSplit = split ?? wide;

  return (
    <Section
      title="In its checkout"
      note={
        <>
          {data.files.length} {data.files.length === 1 ? "file" : "files"}, <span className="text-success">+{added}</span>{" "}
          <span className="text-destructive">−{removed}</span> in <span className="font-mono">{data.root}</span>, against {data.base_note} (
          <span className="font-mono">{data.base.slice(0, 8)}</span>){data.more_files ? `, and ${data.more_files} more not listed` : ""}.
        </>
      }
    >
      {data.files.length === 0 ? (
        <Panel className="text-muted-foreground p-8 text-center text-sm">Nothing changed in the checkout since the session started.</Panel>
      ) : (
        <div className="grid gap-3 lg:grid-cols-[18rem_minmax(0,1fr)]">
          <Panel pad={false} className="max-h-[70vh] overflow-y-auto py-1 lg:sticky lg:top-3 lg:self-start">
            <Tree
              dir={tree}
              depth={0}
              at={chosen?.path ?? ""}
              pick={setAt}
              shut={shut}
              turn={(p) =>
                setShut((s) => {
                  const n = new Set(s);
                  if (n.has(p)) n.delete(p);
                  else n.add(p);
                  return n;
                })
              }
            />
          </Panel>
          <div className="min-w-0 space-y-2">
            <div className="flex flex-wrap items-center gap-1.5">
              <Button variant="outline" size="xs" onClick={() => step(-1)} disabled={order.length < 2}>
                ← Previous
              </Button>
              <Button variant="outline" size="xs" onClick={() => step(1)} disabled={order.length < 2}>
                Next →
              </Button>
              <Button variant="ghost" size="xs" onClick={() => setSplit(!isSplit)}>
                {isSplit ? "Unified" : "Side by side"}
              </Button>
              <Button variant="ghost" size="xs" className="ml-auto" onClick={refresh} aria-label="Read the checkout again">
                <RefreshCw />
              </Button>
            </div>
            {shown?.error ? (
              <Panel className="text-muted-foreground p-6 text-center text-sm">{shown.error}</Panel>
            ) : shown?.diff ? (
              <>
                <GithubDiff key={shown.key} file={shown.diff} split={isSplit} />
                {!shown.diff.new && !shown.diff.old && !shown.diff.binary && (
                  <p className="text-muted-foreground text-xs">This link can see the changed lines, not the whole file, so the unchanged stretches stay folded.</p>
                )}
              </>
            ) : (
              <Panel className="text-muted-foreground p-6 text-center text-sm">Reading {chosen?.path}…</Panel>
            )}
          </div>
        </div>
      )}
    </Section>
  );
}
