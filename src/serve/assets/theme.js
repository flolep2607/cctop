// The reader's choice of light, dark, or whatever the system is doing —
// shared by every page, inlined the way the stylesheet is.
//
// Two jobs. The first runs now, synchronously in the head, so the attribute
// is on <html> before the first paint: a reader who chose dark must never see
// a light frame on the way in. The second is `themeToggle`, which each page's
// own script calls to place the button — the pages have no shared header to
// put one in.
//
// `data-theme` is the whole mechanism: common.css keeps dark under
// prefers-color-scheme for anyone who has never touched the button, and reads
// the attribute as an override for anyone who has. "System" is the attribute
// being absent, which is why clearing it — not writing "system" — is how the
// cycle returns to following the OS.
(() => {
  const KEY = "cctop-theme";
  const stored = (() => {
    try { return localStorage.getItem(KEY); } catch (e) { return null; }
  })();
  if (stored === "light" || stored === "dark") {
    document.documentElement.dataset.theme = stored;
  }
  window.themeToggle = () => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "theme";
    // A glyph rather than "Theme: system": the words took a tenth of a phone's
    // header to say something the reader looks for once. The name is still
    // there for anyone who hovers or uses a screen reader.
    const GLYPH = { light: "☀", dark: "☾", system: "◐" };
    const paint = () => {
      const set = document.documentElement.dataset.theme || "system";
      button.textContent = GLYPH[set];
      button.setAttribute("aria-label", "Theme: " + set);
      button.title =
        "Theme: " + set + " — light, dark, or the system's choice, for every cctop page";
    };
    button.addEventListener("click", () => {
      const now = document.documentElement.dataset.theme;
      // The stored value, not the computed one: system counts as unset, so a
      // reader on a dark OS who wants light still reaches it in one click.
      const next = now === "light" ? "dark" : now === "dark" ? null : "light";
      if (next) document.documentElement.dataset.theme = next;
      else delete document.documentElement.dataset.theme;
      try {
        if (next) localStorage.setItem(KEY, next);
        else localStorage.removeItem(KEY);
      } catch (e) {}
      paint();
    });
    paint();
    return button;
  };
})();
