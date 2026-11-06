// The session page's diff layout: the same cases the terminal's diff view
// pins (crates/ui/src/diffview.rs), on the web's port of it.
import { test } from "node:test";
import assert from "node:assert/strict";
import { buildRows, buildTree, parseHunks, squares, treeOrder, wordMarks } from "../src/lib/diff.ts";

const file = Array.from({ length: 20 }, (_, i) => `line ${i + 1}`);

test("a hunk lines up removed beside added, with the unchanged around it folded", () => {
  const diff = ["@@ -9,3 +9,4 @@ fn send() {", " line 9", "-    retry(3)", "+    retry(5)", "+    log()", " line 11"];
  const after = [...file.slice(0, 9), "    retry(5)", "    log()", ...file.slice(10)];
  const rows = buildRows(parseHunks(diff), file, after, new Set(), false);
  const kinds = rows.map((r) => r.kind);
  assert.deepEqual(kinds, ["fold", "hunk", "same", "pair", "pair", "same", "fold"]);
  assert.deepEqual(rows[0], { kind: "fold", id: 0, old: 1, new: 1, count: 8, canOpen: true });
  const pair = rows[3] as Extract<(typeof rows)[number], { kind: "pair" }>;
  assert.equal(pair.left?.no, 10);
  assert.equal(pair.right?.no, 10);
  assert.deepEqual(pair.right?.marks, [[10, 11]], "only the 5 changed");
  const lone = rows[4] as Extract<(typeof rows)[number], { kind: "pair" }>;
  assert.equal(lone.left, undefined);
  assert.equal(lone.right?.text, "    log()");
  assert.match((rows[1] as { header: string }).header, /^@@ -9,3 \+9,4 @@ fn send\(\) \{$/);
});

test("a fold opens into its lines, and stays shut without the file's text", () => {
  const diff = ["@@ -9,1 +9,1 @@", "-line 9", "+line nine"];
  const open = buildRows(parseHunks(diff), file, file, new Set([0]), false);
  assert.equal(open.filter((r) => r.kind === "same").length, 8);
  const blind = buildRows(parseHunks(diff), null, null, new Set([0]), true);
  assert.equal(blind[0].kind, "fold");
  assert.equal((blind[0] as { canOpen: boolean }).canOpen, false);
});

test("an untracked file is one run of added lines with no header", () => {
  const rows = buildRows(parseHunks(["+a", "+b"]), null, ["a", "b"], new Set(), false);
  assert.deepEqual(rows.map((r) => r.kind), ["pair", "pair"]);
  assert.equal((rows[1] as { right?: { no: number } }).right?.no, 2);
});

test("word marks are char ranges, past a two-byte char", () => {
  const [a, b] = wordMarks('let s = "é"; f(1)', 'let s = "é"; f(2)');
  assert.deepEqual(a, [[15, 16]]);
  assert.deepEqual(b, [[15, 16]]);
  assert.deepEqual(wordMarks("abc", "xyz"), [[], []], "a rewrite washes nothing extra");
});

test("the tree joins single-folder chains, adds up, and steps folders first", () => {
  const f = (path: string, added: number) => ({ path, added, removed: 1, untracked: false });
  const root = buildTree([f("docs/explanation/a.md", 2), f("docs/explanation/b.md", 3), f("README.md", 1)]);
  assert.equal(root.folders[0].name, "docs/explanation");
  assert.equal(root.folders[0].added, 5);
  assert.equal(root.folders[0].count, 2);
  assert.deepEqual(treeOrder(root).map((x) => x.path), ["docs/explanation/a.md", "docs/explanation/b.md", "README.md"]);
});

test("five squares, green for the added share", () => {
  assert.equal(squares(3, 3), 3);
  assert.equal(squares(10, 0), 5);
  assert.equal(squares(0, 0), 0);
});

test("highlight.js's spans come back as per-line char ranges, innermost class first", async () => {
  const { fromHtml, highlightLines, languageOf } = await import("../src/lib/highlight.ts");
  const lines = fromHtml('<span class="hljs-comment">/* a\nb */</span> x <span class="hljs-string">&quot;é&quot;</span>', 2);
  assert.deepEqual(lines[0], [[0, 4, "comment"]]);
  assert.deepEqual(lines[1], [[0, 4, "comment"], [7, 10, "string"]]);
  assert.equal(languageOf("src/main.rs"), "rust");
  assert.equal(languageOf("bin/run", "#!/usr/bin/env python3"), "python");
  assert.equal(languageOf("notes.unknown"), undefined);
  const rust = highlightLines("a.rs", ["fn main() {", '    let s = "x";', "}"]);
  assert.ok(rust?.[0].some(([, , c]) => c === "keyword"), JSON.stringify(rust));
  assert.ok(rust?.[1].some(([, , c]) => c === "string"), JSON.stringify(rust));
});
