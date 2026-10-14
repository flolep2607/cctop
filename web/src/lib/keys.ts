// Keys the framed terminal (rmux's xterm.js, under /term/) cannot spell on its
// own. `components/terminal.tsx` listens inside the frame and re-presses them.

type Press = { key: string; shiftKey: boolean; altKey: boolean; ctrlKey: boolean; metaKey: boolean; isComposing?: boolean };

/**
 * Shift+Enter, and nothing held with it.
 *
 * xterm.js sends Enter as a bare carriage return whatever Shift says, so in
 * the browser Shift+Enter submits an agent's prompt instead of breaking the
 * line. A terminal on the desktop sends Claude Code something distinct — the
 * kitty `CSI 13;2u`, which the TUI's own tabs send (`extended` in
 * crates/core/src/attach.rs) — but xterm.js has no kitty keyboard, so the
 * frame sends the other spelling every agent reads as a newline: `ESC CR`,
 * which is what xterm.js already sends for Alt+Enter. A press mid-composition
 * is the input method's to finish.
 */
export const isShiftEnter = (e: Press) =>
  e.key === "Enter" && e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey && !e.isComposing;

/**
 * The Alt+Enter press that stands in for Shift+Enter, built in the frame's own
 * window so xterm.js takes it as one of its own.
 *
 * xterm.js reads `keyCode`, which the `KeyboardEvent` constructor cannot set
 * (it is a legacy field, always 0 on a synthetic event), so it is pinned on the
 * instance. Nothing in xterm.js asks whether an event is trusted.
 */
export function altEnter(win: Window & typeof globalThis): KeyboardEvent {
  const press = new win.KeyboardEvent("keydown", {
    key: "Enter",
    code: "Enter",
    altKey: true,
    bubbles: true,
    cancelable: true,
  });
  for (const field of ["keyCode", "which"]) Object.defineProperty(press, field, { get: () => 13 });
  return press;
}
