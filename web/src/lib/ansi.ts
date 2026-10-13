// Terminal output as styled lines: the plain-data half of `components/ansi.tsx`.
// It lives apart from the component so a page that only wants clean text
// imports no React, so the component module exports nothing but components
// (which fast refresh needs), and so `node --test` can run it with no bundler.
//
// This is a port of the TUI's `crates/ui/src/ansi.rs`, state machine and all,
// and the two are held to the same cases in `crates/ui/src/ansi_cases.json`.
// It used to be a regular expression, and a regular expression cannot see what
// a terminal sees: an OSC cut off by a size limit took every line after it, a
// DCS left its payload in the text, a CSI with a `<` or an intermediate left
// its tail, a colon underline style (`4:3`) read as italic, and the progress
// bar's `\r` took the colour the bar was printed in with it.

/** `n0`–`n15` for the sixteen by SGR number, `i0`–`i255` for `38;5`, `#rrggbb` for 24-bit. */
export type Colour = string;

export type Style = {
  fg?: Colour;
  bg?: Colour;
  bold?: true;
  dim?: true;
  italic?: true;
  underline?: true;
  inverse?: true;
  strike?: true;
};

export type Span = { text: string; style: Style };

const ESC = "\x1b";

/** `text` as lines of spans, adjacent spans of one style merged. */
export function parse(text: string): Span[][] {
  const out: Span[][] = [];
  let line: [string, Style][] = [];
  let style: Style = {};
  // Code points, as Rust's `chars()` sees them, so a backspace takes back a
  // whole emoji rather than half of its surrogate pair.
  const cs = Array.from(text);
  let i = 0;
  const peek = () => cs[i] as string | undefined;
  while (i < cs.length) {
    const c = cs[i++];
    if (c === ESC) {
      // Peeked rather than taken, so an ESC with nothing sensible after it
      // costs only itself — never the newline or letter that follows.
      const n = peek();
      if (n === "[") {
        i++;
        i = csi(cs, i, (params) => (style = sgr(params, style)));
      } else if (n !== undefined && "]PX^_".includes(n)) {
        // OSC, and the strings that end the same way: DCS, SOS, PM, APC.
        i = osc(cs, i + 1);
      } else if (n !== undefined && n >= " " && n <= "/") {
        // `ESC ( B` and the like: intermediates, then one final byte.
        while (i < cs.length && cs[i] >= " " && cs[i] <= "/") i++;
        if (i < cs.length && cs[i] >= "0" && cs[i] <= "~") i++;
      } else if (n !== undefined && n >= "0" && n <= "~") {
        // `ESC 7`, `ESC M`, `ESC =`: complete in two bytes.
        i++;
      }
    } else if (c === "\u009b") {
      i = csi(cs, i, (params) => (style = sgr(params, style)));
    } else if (c === "\n") {
      out.push(finish(line));
      line = [];
    } else if (c === "\r") {
      // A return with nothing drawn after it leaves the line as it was: CRLF,
      // or a last frame that ended on its `\r`.
      const n = peek();
      if (n !== undefined && n !== "\n" && n !== "\r") line = [];
    } else if (c === "\b") {
      // What `man` does for bold: a character, a backspace, the same again.
      line.pop();
    } else if (c === "\t") {
      // Kept, unlike the TUI, which has to expand it: a browser lays a tab out
      // in columns itself, and copying the output gives back the tab.
      line.push([c, style]);
    } else if (!isControl(c)) {
      line.push([c, style]);
    }
  }
  if (line.length || !out.length) out.push(finish(line));
  return out;
}

// Rust's `char::is_control`: C0, DEL and C1.
const isControl = (c: string) => c <= "\x1f" || (c >= "\x7f" && c <= "\x9f");

function finish(line: [string, Style][]): Span[] {
  const spans: Span[] = [];
  for (const [c, style] of line) {
    const last = spans[spans.length - 1];
    if (last && same(last.style, style)) last.text += c;
    else spans.push({ text: c, style });
  }
  return spans;
}

const KEYS = ["fg", "bg", "bold", "dim", "italic", "underline", "inverse", "strike"] as const;
const same = (a: Style, b: Style) => a === b || KEYS.every((k) => a[k] === b[k]);

/**
 * Consume a control sequence after its introducer, handing the parameters on
 * when it is SGR. Returns where the text resumes.
 *
 * Parameter bytes, then intermediates, then one final byte, per ECMA-48. A
 * byte outside those ranges ends the sequence where it stands and is left for
 * the text — which is how a sequence cut off by a truncated result stops at
 * the newline rather than eating the next line.
 */
function csi(cs: string[], i: number, onSgr: (params: string) => void): number {
  let params = "";
  while (i < cs.length && cs[i] >= "0" && cs[i] <= "?") params += cs[i++];
  let intermediates = false;
  while (i < cs.length && cs[i] >= " " && cs[i] <= "/") {
    intermediates = true;
    i++;
  }
  if (i >= cs.length || cs[i] < "@" || cs[i] > "~") return i;
  const end = cs[i++];
  // `ESC [ ? … m` and its kin are private modes that happen to end in `m`.
  const priv = /^[<=>?]/.test(params);
  if (end === "m" && !intermediates && !priv) onSgr(params);
  return i;
}

/**
 * Consume an OSC-style string up to BEL, ST (`ESC \`) or the eight-bit ST.
 * A newline stops it too and is left for the text: a terminal would keep
 * swallowing, but a terminal is not reading a result cut off at a size limit.
 */
function osc(cs: string[], i: number): number {
  while (i < cs.length && cs[i] !== "\n") {
    const c = cs[i++];
    if (c === "\x07" || c === "\u009c") return i;
    if (c === ESC) return cs[i] === "\\" ? i + 1 : i;
  }
  return i;
}

// The integer parses Rust does: `u16` (a group that is not one reads as 0, so
// an empty parameter is a reset) and `u8` (one that is not is no colour).
const u16 = (s: string) => (/^\d+$/.test(s) && Number(s) <= 0xffff ? Number(s) : 0);
const u8 = (s: string | undefined) => (s !== undefined && /^\d+$/.test(s) && Number(s) <= 255 ? Number(s) : undefined);
const hex = (r: number, g: number, b: number) => "#" + [r, g, b].map((x) => (x & 255).toString(16).padStart(2, "0")).join("");

/**
 * Apply one SGR parameter list. Colon sub-parameters (`38:2::r:g:b`, `4:3` for
 * a curly underline) are the newer spelling of the same things, and are read
 * as one group each.
 */
function sgr(params: string, prev: Style): Style {
  if (!params) return {};
  let s: Style = { ...prev };
  const groups = params.split(";");
  const paint = (fg: boolean, c: Colour | undefined) => {
    if (c !== undefined) s[fg ? "fg" : "bg"] = c;
  };
  for (let i = 0; i < groups.length; ) {
    const group = groups[i++];
    if (group.includes(":")) {
      const parts = group.split(":").map(u16);
      if (parts[0] === 4) {
        if (parts[1] === 0) delete s.underline;
        else s.underline = true;
      } else if (parts[0] === 38 || parts[0] === 48) {
        // `38:2:<colourspace>:r:g:b`, with the colourspace often left empty —
        // and sometimes left out altogether.
        if (parts[1] === 5 && parts[2] !== undefined) paint(parts[0] === 38, `i${parts[2] & 255}`);
        if (parts[1] === 2) {
          let rgb = parts.slice(2);
          if (rgb.length >= 4) rgb = rgb.slice(1);
          if (rgb.length >= 3) paint(parts[0] === 38, hex(rgb[0], rgb[1], rgb[2]));
        }
      }
      continue;
    }
    const code = u16(group);
    if (code === 0) s = {};
    else if (code === 1) s.bold = true;
    else if (code === 2) s.dim = true;
    else if (code === 3) s.italic = true;
    else if (code === 4) s.underline = true;
    else if (code === 7) s.inverse = true;
    else if (code === 9) s.strike = true;
    else if (code === 22) {
      delete s.bold;
      delete s.dim;
    }
    else if (code === 23) delete s.italic;
    else if (code === 24) delete s.underline;
    else if (code === 27) delete s.inverse;
    else if (code === 29) delete s.strike;
    else if (code >= 30 && code <= 37) s.fg = `n${code - 30}`;
    else if (code >= 40 && code <= 47) s.bg = `n${code - 40}`;
    else if (code >= 90 && code <= 97) s.fg = `n${code - 90 + 8}`;
    else if (code >= 100 && code <= 107) s.bg = `n${code - 100 + 8}`;
    else if (code === 39) delete s.fg;
    else if (code === 49) delete s.bg;
    else if (code === 38 || code === 48 || code === 58) {
      // Underline colour (58) is spelled the same and skipped the same way.
      const kind = groups[i] !== undefined && /^\d+$/.test(groups[i]) ? Number(groups[i]) : undefined;
      if (kind === 5) {
        const n = u8(groups[i + 1]);
        if (code !== 58 && n !== undefined) paint(code === 38, `i${n}`);
        i += 2;
      } else if (kind === 2) {
        const rgb = groups.slice(i + 1, i + 4).map(u8).filter((x) => x !== undefined);
        if (code !== 58 && rgb.length === 3) paint(code === 38, hex(rgb[0], rgb[1], rgb[2]));
        i += 4;
      } else i += 1;
    }
    // Blink and conceal are left out on purpose, as in the TUI.
  }
  return s;
}

/** Whether text needs any of this — the common case is none. */
// eslint-disable-next-line no-control-regex
export const hasAnsi = (text: string) => /[\x00-\x08\x0b-\x1f\x7f-\x9f]/.test(text);

/** The text with every escape sequence and control character removed. */
export const stripAnsi = (text: string) =>
  hasAnsi(text)
    ? parse(text)
        .map((line) => line.map((s) => s.text).join(""))
        .join("\n")
    : text;
