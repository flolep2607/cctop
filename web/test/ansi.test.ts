// The web half of the shared escape-sequence cases; the TUI's half is
// `the_shared_cases_read_as_the_web_reads_them` in crates/ui/src/ansi.rs.
//
// Run by `npm test` with Node's own runner and type stripping: no test
// framework to install, and nothing here reaches the bundle, since only
// src/ is built.
import { readFileSync } from "node:fs";
import { test } from "node:test";
import assert from "node:assert/strict";
import { hasAnsi, parse, stripAnsi, type Span } from "../src/lib/ansi.ts";

type Case = { name: string; input: string; lines: ({ text: string } & Record<string, unknown>)[][] };
const { cases } = JSON.parse(
  readFileSync(new URL("../../crates/ui/src/ansi_cases.json", import.meta.url), "utf8"),
) as { cases: Case[] };

const flat = (span: Span) => ({ text: span.text, ...span.style });

for (const c of cases) {
  test(c.name, () => {
    assert.deepEqual(parse(c.input).map((line) => line.map(flat)), c.lines, JSON.stringify(c.input));
    // Text with no control character in it is handed back as it was, so the
    // trailing-newline case is only the parser's to agree on.
    if (!hasAnsi(c.input)) return;
    const plain = c.lines.map((line) => line.map((s) => s.text).join("")).join("\n");
    assert.equal(stripAnsi(c.input), plain);
  });
}
