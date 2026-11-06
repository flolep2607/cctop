// Syntax colours for the session page's diff, as char ranges per line, so the
// diff can lay its word marks over them without touching HTML.
//
// highlight.js's core with a handful of languages registered by hand: the page
// is one inlined file paid for on every load, and the full build is most of a
// megabyte. Measured on the built page: these ten cost 69 KB (23 KB gzipped);
// adding C, C++, Java, SQL, CSS and HTML cost 31 KB more and were left out. A
// file in a language not below is drawn without colours, its washes intact.
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import go from "highlight.js/lib/languages/go";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import markdown from "highlight.js/lib/languages/markdown";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import typescript from "highlight.js/lib/languages/typescript";
import yaml from "highlight.js/lib/languages/yaml";
import ini from "highlight.js/lib/languages/ini";

const LANGUAGES = { bash, go, javascript, json, markdown, python, rust, typescript, yaml, ini };
for (const [name, lang] of Object.entries(LANGUAGES)) hljs.registerLanguage(name, lang);

const BY_EXTENSION: Record<string, string> = {
  sh: "bash", bash: "bash", zsh: "bash",
  go: "go",
  js: "javascript", jsx: "javascript", mjs: "javascript", cjs: "javascript",
  ts: "typescript", tsx: "typescript", mts: "typescript",
  json: "json", jsonl: "json",
  md: "markdown", mdx: "markdown",
  py: "python",
  rs: "rust",
  yml: "yaml", yaml: "yaml",
  toml: "ini", ini: "ini", cfg: "ini",
};

/** The language for `path`, by its extension, then a shebang. */
export function languageOf(path: string, firstLine?: string): string | undefined {
  const name = path.split("/").pop() ?? path;
  const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  if (BY_EXTENSION[ext]) return BY_EXTENSION[ext];
  if (name === "Dockerfile" || name === "Makefile") return "bash";
  const bang = firstLine?.match(/^#!.*\b(bash|sh|zsh|python3?|node)\b/);
  if (bang) return { bash: "bash", sh: "bash", zsh: "bash", python: "python", python3: "python", node: "javascript" }[bang[1]];
  return undefined;
}

/** A coloured stretch of a line: char range and highlight.js's class, `keyword` for `hljs-keyword`. */
export type Colour = [number, number, string];

/** A file bigger than this is drawn without colours. */
const MAX_CHARS = 1 << 20;

/**
 * Each line's coloured stretches, the file highlighted whole so a string or
 * a comment opened above a line still colours it. `null` when there is no
 * language for it, or it is too big.
 */
export function highlightLines(path: string, lines: string[]): Colour[][] | null {
  const lang = languageOf(path, lines[0]);
  const text = lines.join("\n");
  if (!lang || text.length > MAX_CHARS) return null;
  let html: string;
  try {
    html = hljs.highlight(text, { language: lang, ignoreIllegals: true }).value;
  } catch {
    return null;
  }
  return fromHtml(html, lines.length);
}

const ENTITIES: Record<string, string> = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#x27;": "'", "&#39;": "'" };

/**
 * highlight.js's HTML — only `<span class="hljs-…">`, `</span>` and escaped
 * text — read back into per-line char ranges, the innermost class winning.
 */
export function fromHtml(html: string, count: number): Colour[][] {
  const out: Colour[][] = Array.from({ length: count }, () => []);
  const stack: string[] = [];
  let line = 0;
  let col = 0;
  const emit = (text: string) => {
    for (const ch of text) {
      if (ch === "\n") {
        line++;
        col = 0;
        continue;
      }
      const cls = stack[stack.length - 1];
      if (cls && line < count) {
        const row = out[line];
        const last = row[row.length - 1];
        if (last && last[2] === cls && last[1] === col) last[1] = col + 1;
        else row.push([col, col + 1, cls]);
      }
      col++;
    }
  };
  for (const m of html.matchAll(/<span class="([^"]*)">|<\/span>|([^<]+)/g)) {
    if (m[1] !== undefined) stack.push(m[1].split(" ")[0].replace(/^hljs-/, ""));
    else if (m[0] === "</span>") stack.pop();
    else emit(m[2].replace(/&(amp|lt|gt|quot|#x27|#39);/g, (e) => ENTITIES[e] ?? e));
  }
  return out;
}
