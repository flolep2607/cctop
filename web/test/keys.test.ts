// Shift+Enter in the framed terminal: which presses are re-spelled, and that
// the stand-in is the one xterm.js turns into `ESC CR`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { altEnter, isShiftEnter } from "../src/lib/keys.ts";

const press = (key: string, held: Partial<Record<"shiftKey" | "altKey" | "ctrlKey" | "metaKey" | "isComposing", boolean>> = {}) => ({
  key,
  shiftKey: false,
  altKey: false,
  ctrlKey: false,
  metaKey: false,
  ...held,
});

test("only Shift+Enter alone is re-spelled", () => {
  assert.ok(isShiftEnter(press("Enter", { shiftKey: true })));
  for (const other of [
    press("Enter"),
    press("Enter", { shiftKey: true, ctrlKey: true }),
    press("Enter", { shiftKey: true, altKey: true }),
    press("Enter", { shiftKey: true, metaKey: true }),
    press("Enter", { shiftKey: true, isComposing: true }),
    press("a", { shiftKey: true }),
  ]) {
    assert.ok(!isShiftEnter(other), JSON.stringify(other));
  }
});

// Node has no KeyboardEvent; a stand-in with the same constructor shape, whose
// `keyCode` is the prototype's 0 as a browser's synthetic event's is.
class FakeKeyboardEvent extends Event {
  key: string;
  code: string;
  altKey: boolean;
  shiftKey: boolean;
  constructor(type: string, init: { key: string; code: string; altKey: boolean; shiftKey?: boolean; bubbles: boolean; cancelable: boolean }) {
    super(type, init);
    this.key = init.key;
    this.code = init.code;
    this.altKey = init.altKey;
    this.shiftKey = init.shiftKey ?? false;
  }
  get keyCode() {
    return 0;
  }
  get which() {
    return 0;
  }
}

test("the stand-in is Alt+Enter with the keyCode xterm.js reads", () => {
  const e = altEnter({ KeyboardEvent: FakeKeyboardEvent } as unknown as Window & typeof globalThis);
  assert.equal(e.type, "keydown");
  assert.equal(e.key, "Enter");
  assert.equal(e.altKey, true);
  assert.equal(e.shiftKey, false);
  assert.equal(e.keyCode, 13);
  assert.equal(e.which, 13);
  assert.equal(e.bubbles, true);
  // Not a Shift+Enter itself, or the listener would take its own press.
  assert.ok(!isShiftEnter(e));
});
