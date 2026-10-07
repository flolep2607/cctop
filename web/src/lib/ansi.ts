// The plain-text half of `components/ansi.tsx`: finding and removing a
// terminal's escape sequences. It lives apart from the component so a page
// that only wants clean text imports no React, and so the component module
// exports nothing but components (which fast refresh needs).

// SGR, any other CSI, OSC (ended by BEL or ST), and lone two-byte escapes.
// eslint-disable-next-line no-control-regex
export const ESCAPE = /\x1b\[([0-9;:?]*)([@-~])|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)?|\x1b[@-Z\\-_]/g;

// The other control characters a terminal swallows: Shift In/Out (`tput sgr0`
// ends a reset with \x0f), bell, backspace and the rest of C0 — tab, newline
// and ESC itself excepted. A browser draws each as a tofu box.
// eslint-disable-next-line no-control-regex
export const CONTROL = /[\x00-\x08\x0b\x0c\x0e-\x1a\x1c-\x1f\x7f]/g;

// A progress bar redraws its line with \r, and a terminal shows the last
// drawing; so does this. A trailing \r (a CRLF line end) is just dropped.
export function settle(text: string): string {
  if (!text.includes("\r")) return text;
  return text
    .split("\n")
    .map((line) => {
      if (!line.includes("\r")) return line;
      const drawn = line.split("\r").filter((seg) => seg !== "");
      return drawn.length ? drawn[drawn.length - 1] : "";
    })
    .join("\n");
}

/** Whether text needs any of this — the common case is none. */
// eslint-disable-next-line no-control-regex
export const hasAnsi = (text: string) => /[\x00-\x08\x0b-\x1f\x7f]/.test(text);

/** The text with every escape sequence and control character removed. */
export const stripAnsi = (text: string) => (hasAnsi(text) ? settle(text).replace(ESCAPE, "").replace(CONTROL, "") : text);
