import { memo, type CSSProperties, type ReactNode } from "react";
import { hasAnsi, parse, type Colour, type Style } from "@/lib/ansi";

// Terminal output, coloured the way the terminal coloured it.
//
// Tool results are whatever a command printed, and plenty of commands print
// colour: cargo's diffs, test runners, git, ls. Shown as plain text the escape
// sequences come out as `[31m` litter through the output. Here the SGR codes
// (`ESC [ … m`) become styled spans and every other control sequence — cursor
// moves, `?25l`, window titles — is dropped, since a page has no cursor to
// move. Reading the sequences is `lib/ansi.ts`; this only turns its styles
// into CSS.

// The sixteen named colours, as colours that read on both themes rather than
// xterm's literal values (pure blue on a dark page is unreadable). The bright
// eight fold onto the same eight: the themes have no second set to give them.
const NAMED = [
  "var(--ansi-black)", "var(--ansi-red)", "var(--ansi-green)", "var(--ansi-yellow)",
  "var(--ansi-blue)", "var(--ansi-magenta)", "var(--ansi-cyan)", "var(--ansi-white)",
];

// A parsed colour as CSS: the 256-colour cube and grey ramp for `i…`.
function colour(c: Colour): string {
  if (c.startsWith("#")) return c;
  const n = Number(c.slice(1));
  if (n < 16) return NAMED[n % 8];
  if (n >= 232) {
    const v = 8 + (n - 232) * 10;
    return `rgb(${v},${v},${v})`;
  }
  const i = n - 16;
  const x = (k: number) => (k ? 55 + k * 40 : 0);
  return `rgb(${x(Math.floor(i / 36))},${x(Math.floor(i / 6) % 6)},${x(i % 6)})`;
}

function css(s: Style): CSSProperties | undefined {
  const out: CSSProperties = {};
  const fg = s.inverse ? (s.bg ? colour(s.bg) : "var(--background)") : s.fg && colour(s.fg);
  const bg = s.inverse ? (s.fg ? colour(s.fg) : "var(--foreground)") : s.bg && colour(s.bg);
  if (fg) out.color = fg;
  if (bg) out.backgroundColor = bg;
  if (s.bold) out.fontWeight = 600;
  if (s.dim) out.opacity = 0.65;
  if (s.italic) out.fontStyle = "italic";
  const lines = [s.underline && "underline", s.strike && "line-through"].filter(Boolean);
  if (lines.length) out.textDecoration = lines.join(" ");
  return Object.keys(out).length ? out : undefined;
}

export const Ansi = memo(function Ansi({ text }: { text: string }) {
  if (!hasAnsi(text)) return <>{text}</>;
  const out: ReactNode[] = [];
  let key = 0;
  parse(text).forEach((line, n) => {
    if (n) out.push("\n");
    for (const span of line) {
      const st = css(span.style);
      out.push(st ? <span key={key++} style={st}>{span.text}</span> : span.text);
    }
  });
  return <>{out}</>;
});
