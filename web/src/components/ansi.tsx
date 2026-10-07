import { memo, type CSSProperties, type ReactNode } from "react";
import { CONTROL, ESCAPE, hasAnsi, settle } from "@/lib/ansi";

// Terminal output, coloured the way the terminal coloured it.
//
// Tool results are whatever a command printed, and plenty of commands print
// colour: cargo's diffs, test runners, git, ls. Shown as plain text the escape
// sequences come out as `[31m` litter through the output. Here the SGR codes
// (`ESC [ … m`) become styled spans and every other control sequence — cursor
// moves, `?25l`, window titles — is dropped, since a page has no cursor to
// move. The text itself is never touched, so copying it gives clean output.

// The sixteen named colours, as colours that read on both themes rather than
// xterm's literal values (pure blue on a dark page is unreadable).
const NAMED = [
  "var(--ansi-black)", "var(--ansi-red)", "var(--ansi-green)", "var(--ansi-yellow)",
  "var(--ansi-blue)", "var(--ansi-magenta)", "var(--ansi-cyan)", "var(--ansi-white)",
];

type Style = { fg?: string; bg?: string; bold?: boolean; dim?: boolean; italic?: boolean; underline?: boolean; inverse?: boolean };

// The 256-colour cube and grey ramp, for `38;5;n`.
function xterm256(n: number): string {
  if (n < 8) return NAMED[n];
  if (n < 16) return NAMED[n - 8];
  if (n >= 232) {
    const v = 8 + (n - 232) * 10;
    return `rgb(${v},${v},${v})`;
  }
  const i = n - 16;
  const c = (x: number) => (x ? 55 + x * 40 : 0);
  return `rgb(${c(Math.floor(i / 36))},${c(Math.floor(i / 6) % 6)},${c(i % 6)})`;
}

function apply(style: Style, params: number[]): Style {
  const s = { ...style };
  if (!params.length) params = [0];
  for (let i = 0; i < params.length; i++) {
    const p = params[i];
    if (p === 0) Object.keys(s).forEach((k) => delete s[k as keyof Style]);
    else if (p === 1) s.bold = true;
    else if (p === 2) s.dim = true;
    else if (p === 3) s.italic = true;
    else if (p === 4) s.underline = true;
    else if (p === 7) s.inverse = true;
    else if (p === 22) s.bold = s.dim = false;
    else if (p === 23) s.italic = false;
    else if (p === 24) s.underline = false;
    else if (p === 27) s.inverse = false;
    else if (p >= 30 && p <= 37) s.fg = NAMED[p - 30];
    else if (p >= 90 && p <= 97) s.fg = NAMED[p - 90];
    else if (p === 39) delete s.fg;
    else if (p >= 40 && p <= 47) s.bg = NAMED[p - 40];
    else if (p >= 100 && p <= 107) s.bg = NAMED[p - 100];
    else if (p === 49) delete s.bg;
    else if ((p === 38 || p === 48) && params[i + 1] === 5 && params[i + 2] !== undefined) {
      s[p === 38 ? "fg" : "bg"] = xterm256(params[i + 2]);
      i += 2;
    } else if ((p === 38 || p === 48) && params[i + 1] === 2 && params[i + 4] !== undefined) {
      s[p === 38 ? "fg" : "bg"] = `rgb(${params[i + 2]},${params[i + 3]},${params[i + 4]})`;
      i += 4;
    }
  }
  return s;
}

function css(s: Style): CSSProperties | undefined {
  const out: CSSProperties = {};
  const fg = s.inverse ? s.bg ?? "var(--background)" : s.fg;
  const bg = s.inverse ? s.fg ?? "var(--foreground)" : s.bg;
  if (fg) out.color = fg;
  if (bg) out.backgroundColor = bg;
  if (s.bold) out.fontWeight = 600;
  if (s.dim) out.opacity = 0.65;
  if (s.italic) out.fontStyle = "italic";
  if (s.underline) out.textDecoration = "underline";
  return Object.keys(out).length ? out : undefined;
}

export const Ansi = memo(function Ansi({ text }: { text: string }) {
  if (!hasAnsi(text)) return <>{text}</>;
  text = settle(text);
  const out: ReactNode[] = [];
  let style: Style = {};
  let at = 0;
  let key = 0;
  const push = (raw: string) => {
    const chunk = raw.replace(CONTROL, "");
    if (!chunk) return;
    const st = css(style);
    out.push(st ? <span key={key++} style={st}>{chunk}</span> : chunk);
  };
  for (const m of text.matchAll(ESCAPE)) {
    push(text.slice(at, m.index));
    at = m.index! + m[0].length;
    // Only SGR changes anything visible; every other sequence just vanishes.
    if (m[2] === "m" && !m[1].includes("?")) {
      style = apply(style, m[1].split(/[;:]/).filter((x) => x !== "").map(Number));
    }
  }
  push(text.slice(at));
  return <>{out}</>;
});
