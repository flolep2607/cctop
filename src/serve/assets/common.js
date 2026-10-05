"use strict";
// What every page's script starts from, inlined ahead of it the way common.css
// is inlined ahead of each page's own styles.
//
// These were three copies once, one per page, and the copies had already
// drifted: the dashboard rounded a cost to whole dollars from $10 and the other
// two from $100, and only one of them knew a missing cost from a zero. A helper
// that exists once cannot disagree with itself, so anything two pages both want
// belongs here rather than in either of them.
//
// A classic script, not a module: the content policy allows inline script and
// nothing else, and top-level declarations in one classic script are visible
// to the next. That is also the constraint on this file — a page must not
// declare a name this file already has, or the page's script fails to parse
// and the page renders and does nothing.

// The values the server substitutes. JSON-encoded on the way in, so these are
// literals by the time this runs; a page opened as a bare file keeps the
// placeholders, which read as "no token" and "cannot act".
const TOKEN = "__CCTOP_TOKEN__";
const QUERY = TOKEN ? "?t=" + encodeURIComponent(TOKEN) : "";
// Whether this run serves the routes that act on a session — substituted rather
// than discovered, so no page draws a control the server would refuse.
const CAN_ACT = "__CCTOP_ACTIONS__";
// The operator's home, so paths read `~/…` as they do in the TUI. Substituted
// because a browser cannot know it: it may not be on the machine cctop is.
const HOME = "__CCTOP_HOME__";

// The token reaches the script embedded in the page, so once it is running the
// `?t=` in the address bar is only a credential sitting in history, in
// screenshots, and in any link copied without thinking. Drop it as soon as the
// page is up. Some embedded contexts refuse replaceState — then the URL stays
// as it arrived, which is the most that can be done there anyway.
//
// A page that wants something else from its query string reads it before this
// line runs, which means at the top of this file or not at all: the report's
// `?find=` is captured here for that reason.
const OPENING_QUERY = new URLSearchParams(location.search);
try {
  const here = new URL(location.href);
  if (here.searchParams.has("t")) {
    here.searchParams.delete("t");
    history.replaceState(null, "", here.pathname + here.search + here.hash);
  }
} catch (e) {}

// Every string from a transcript — titles, branches, paths, model names — is
// written through textContent or this. None of it is trusted markup, and a
// project directory is perfectly free to be called `<img onerror=…>`.
const el = (tag, cls, text) => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined && text !== null) node.textContent = String(text);
  return node;
};
const svg = (tag, attrs) => {
  const node = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [k, v] of Object.entries(attrs || {})) node.setAttribute(k, String(v));
  return node;
};

// A titled section — the small uppercase heading and the note under it that
// common.css styles as section.block. The caller fills it.
const block = (heading, note) => {
  const s = el("section", "block");
  s.appendChild(el("h3", null, heading));
  if (note) s.appendChild(el("p", "note", note));
  return s;
};

// --- figures ---------------------------------------------------------------

// A cost. Missing is not free: a session whose harness records no price shows
// a dash, never "$0", which would claim something nobody measured.
const money = (v) => {
  if (v === null || v === undefined) return "—";
  const n = Number(v);
  if (!isFinite(n)) return "—";
  if (n === 0) return "$0";
  if (n < 0.01) return "<$0.01";
  return "$" + (n < 100 ? n.toFixed(2) : Math.round(n).toLocaleString());
};
const tokens = (v) => {
  const n = Number(v) || 0;
  if (n >= 1e9) return (n / 1e9).toFixed(2) + "G";
  if (n >= 1e6) return (n / 1e6).toFixed(1) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + "k";
  return String(Math.round(n));
};
// "4m ago", or nothing for a timestamp that does not parse — never " ago" on
// its own, which is what appending the suffix at each call site used to give.
const ago = (iso) => {
  const then = Date.parse(iso);
  if (!isFinite(then)) return "";
  const s = Math.max(0, (Date.now() - then) / 1000);
  if (s < 60) return Math.floor(s) + "s ago";
  if (s < 3600) return Math.floor(s / 60) + "m ago";
  if (s < 86400) return Math.floor(s / 3600) + "h ago";
  return Math.floor(s / 86400) + "d ago";
};
const shortPath = (path) => {
  const full = String(path);
  if (HOME && full.startsWith(HOME + "/")) return "~" + full.slice(HOME.length);
  if (HOME && full === HOME) return "~";
  const parts = full.split("/").filter(Boolean);
  return parts.length <= 2 ? full : "…/" + parts.slice(-2).join("/");
};
// `anthropic/claude-opus-5` and `claude-opus-5` are one model to a reader.
const shortModel = (model) => {
  const parts = String(model).split("/").filter(Boolean);
  return parts.length ? parts[parts.length - 1] : model;
};

// --- talking to the server -------------------------------------------------

// What went wrong, in words worth showing. A cctop error is short and arrives
// as text/plain; anything else in the body was written by something between
// this page and the server — a tunnel whose far end has gone answers with a
// whole HTML error page, and that page used to land on screen verbatim.
async function problem(response) {
  const kind = (response.headers.get("content-type") || "").split(";")[0].trim();
  const said = kind === "text/plain" ? (await response.text()).trim() : "";
  if (said) return said.length > 400 ? said.slice(0, 400) + "…" : said;
  if (response.status >= 502 && response.status <= 504) return "cctop is not answering. The link is up but nothing is behind it — the terminal it runs in may have stopped.";
  if (response.status === 401 || response.status === 403) return "This link is no longer authorised. Open a fresh one from the terminal.";
  return "The server answered " + response.status + (response.statusText ? " " + response.statusText : "") + ".";
}

// The body as JSON, or a sentence saying why it is not.
//
// A 200 is not a promise of JSON. A captive portal, a proxy, or a tunnel that
// has been repointed all answer with an HTML page and a perfectly good status,
// and `response.json()` then throws a parser's complaint — `Unexpected token
// '<', "<html><bod"... is not valid JSON` — which is machinery, shown to
// somebody who wanted to read a conversation.
async function asJson(response) {
  const text = await response.text();
  try {
    return JSON.parse(text);
  } catch (e) {
    throw new Error("Whatever answered this page, it was not cctop.");
  }
}

// Every request the page makes. A dropped connection rejects the fetch itself
// with nothing in it worth reading, so it is named here instead.
async function ask(url, init) {
  let response;
  try {
    response = await fetch(url, init);
  } catch (e) {
    throw new Error("cctop is unreachable — the connection dropped.");
  }
  if (!response.ok) throw new Error(await problem(response));
  return response;
}

// --- answering a permission prompt ----------------------------------------

// The harnesses whose prompt menus the server knows the keys of — see
// `actions::answer`: Allow presses the first option, Deny presses Esc, and for
// anything else a guessed key could mean the opposite. Every page that draws
// Allow and Deny asks this first, so a button is never offered that would
// answer 409.
const ANSWERABLE = new Set(["claude", "codex"]);

// Allow or deny what a session is asking. Resolves to what the server said
// ("Allowed", "Denied"); rejects with a sentence worth showing.
async function answerPrompt(sessionId, choice) {
  const response = await ask("/api/act/answer/" + encodeURIComponent(sessionId) + QUERY, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ choice }),
  });
  const done = await response.json().catch(() => ({}));
  return done.message || (choice === "allow" ? "Allowed" : "Denied");
}

// --- the header every page shares ------------------------------------------

// The page links in the header carry the credential this page was opened with,
// or each of them lands on a 403. Done here rather than by each page so a page
// added to the nav later cannot forget it.
for (const link of document.querySelectorAll("a[data-nav]")) {
  link.href = link.getAttribute("data-nav") + QUERY;
}

// The light/dark/system switch, appended to the header's right end. theme.js
// defines window.themeToggle when the page is served; a copy opened as a bare
// file has none, so it is guarded rather than assumed. `before`, when a page
// passes one, is the element the button goes in front of instead.
function placeThemeButton(before) {
  const button = window.themeToggle && window.themeToggle();
  if (!button) return;
  if (before) before.before(button);
  else document.querySelector("header.top")?.appendChild(button);
}

// --- the command palette ---------------------------------------------------
//
// Ctrl+K (⌘K on a Mac) on every page: one box that reaches every page, every
// session and every open tab by typing a few letters of it. It is the answer
// to a nav that grows a link per page — the header keeps the three a reader
// visits, and everything else is a keystroke away rather than a click deep.
//
// Built here, in plain DOM, because it is the same on every page and owns no
// state that outlives it: it is rebuilt from what the server says each time it
// opens, and thrown away when it closes.

// How well `query` matches `text`: the letters in order, not necessarily
// together, scored so a run of them and a match at a word's start rank above a
// scatter. -1 is no match. Small enough to read, which matters more here than
// the last few percent of ranking quality.
function fuzzy(query, text) {
  if (!query) return 0;
  const t = text.toLowerCase();
  let score = 0;
  let at = -1;
  let run = 0;
  for (const ch of query.toLowerCase()) {
    if (ch === " ") continue;
    const next = t.indexOf(ch, at + 1);
    if (next < 0) return -1;
    run = next === at + 1 ? run + 1 : 0;
    const wordStart = next === 0 || /[\s/._-]/.test(t[next - 1]);
    score += 1 + run * 2 + (wordStart ? 3 : 0);
    at = next;
  }
  // Shorter text wins a tie: "Analytics" over "analytics of a long title".
  return score - t.length / 100;
}

const palette = (() => {
  let dialog = null;
  let items = [];
  let shown = [];
  let active = 0;

  const pages = () => [
    { group: "Pages", label: "Sessions", hint: "the live table", href: "/" },
    { group: "Pages", label: "Workspace", hint: "every terminal, tiled", href: "/workspace" },
    { group: "Pages", label: "Analytics", hint: "spend, tokens and activity over time", href: "/analytics" },
    { group: "Pages", label: "Optimize", hint: "what sessions were handed and never used", href: "/insight/optimize" },
    { group: "Pages", label: "Compare", hint: "how the agents differ on the same kind of work", href: "/insight/compare" },
    {
      group: "Actions", label: "Switch theme", hint: "light, dark, or the system's",
      run: () => document.querySelector("button.theme")?.click(),
    },
  ];

  // Sessions and tabs, fetched as the palette opens. Each source fails on its
  // own: an older cctop without /api/tabs still lists its sessions.
  async function load(render) {
    const [sessions, tabs] = await Promise.all([
      ask("/api/sessions" + QUERY).then(asJson).catch(() => []),
      ask("/api/tabs" + QUERY).then(asJson).then((b) => b.tabs || []).catch(() => []),
    ]);
    const live = [];
    for (const t of tabs) {
      live.push({
        group: "Tabs", label: t.label, hint: t.cwd ? shortPath(t.cwd) : "",
        state: t.state === "needs-input" ? "asking" : t.state === "working" ? "working" : "idle",
        href: t.session_id ? "/session/" + encodeURIComponent(t.session_id) : "/workspace",
      });
    }
    const recent = (Array.isArray(sessions) ? sessions : [])
      .slice()
      .sort((a, b) => (Date.parse(b.last_active) || 0) - (Date.parse(a.last_active) || 0));
    for (const s of recent) {
      const where = s.project ? shortPath(s.project) : "";
      live.push({
        group: "Sessions",
        label: s.title || where || s.session_id,
        hint: [s.title ? where : "", s.harness, s.model ? shortModel(s.model) : "", ago(s.last_active)]
          .filter(Boolean).join(" · "),
        state: s.state === "needs-input" ? "asking" : s.running ? "working" : "idle",
        href: "/session/" + encodeURIComponent(s.session_id),
      });
    }
    items = pages().concat(live);
    render();
  }

  function go(item, newTab) {
    close();
    if (item.run) return item.run();
    const url = item.href + QUERY;
    if (newTab) window.open(url, "_blank", "noopener");
    else location.href = url;
  }

  function build() {
    dialog = el("dialog", "palette");
    dialog.setAttribute("aria-label", "Go to");
    const input = el("input");
    input.type = "search";
    input.placeholder = "Go to a page, session or tab…";
    input.autocomplete = "off";
    input.spellcheck = false;
    input.setAttribute("role", "combobox");
    input.setAttribute("aria-controls", "palette-list");
    const list = el("div", "palette-list");
    list.id = "palette-list";
    list.setAttribute("role", "listbox");
    const foot = el("div", "palette-foot",
      "↑↓ to move · Enter to open · Ctrl+Enter in a new tab · Esc to close");
    dialog.append(input, list, foot);

    const render = () => {
      const q = input.value.trim();
      shown = items
        .map((it) => ({ it, score: fuzzy(q, it.label + " " + (q ? it.hint : "")) }))
        .filter((x) => x.score >= 0)
        // With no query the groups keep their own order, which is the useful
        // one: pages, tabs, then sessions by recency.
        .sort((a, b) => (q ? b.score - a.score : 0))
        .slice(0, 60)
        .map((x) => x.it);
      active = Math.min(active, Math.max(0, shown.length - 1));
      list.replaceChildren();
      let group = null;
      shown.forEach((it, i) => {
        // Group headings only while browsing; a ranked result list is one list.
        if (!q && it.group !== group) {
          group = it.group;
          list.appendChild(el("div", "palette-group", group));
        }
        const row = el("div", "palette-item");
        row.setAttribute("role", "option");
        row.setAttribute("aria-selected", String(i === active));
        row.id = "palette-opt-" + i;
        if (it.state) row.appendChild(el("span", "dot " + it.state));
        row.appendChild(el("span", "label", it.label));
        if (it.hint) row.appendChild(el("span", "hint", it.hint));
        if (q) row.appendChild(el("span", "kind", it.group.replace(/s$/, "")));
        row.addEventListener("mousemove", () => {
          if (active !== i) { active = i; paintActive(); }
        });
        row.addEventListener("click", (e) => go(it, e.ctrlKey || e.metaKey));
        list.appendChild(row);
      });
      if (!shown.length) list.appendChild(el("div", "palette-empty", q ? "Nothing matches." : "Loading…"));
      paintActive();
    };
    const paintActive = () => {
      list.querySelectorAll(".palette-item").forEach((row, i) => {
        row.setAttribute("aria-selected", String(i === active));
        if (i === active) row.scrollIntoView({ block: "nearest" });
      });
      input.setAttribute("aria-activedescendant", shown.length ? "palette-opt-" + active : "");
    };

    input.addEventListener("input", () => { active = 0; render(); });
    input.addEventListener("keydown", (e) => {
      const move = (d) => {
        e.preventDefault();
        if (!shown.length) return;
        active = (active + d + shown.length) % shown.length;
        paintActive();
      };
      if (e.key === "ArrowDown" || (e.ctrlKey && e.key === "n")) move(1);
      else if (e.key === "ArrowUp" || (e.ctrlKey && e.key === "p")) move(-1);
      else if (e.key === "Enter" && shown[active]) {
        e.preventDefault();
        go(shown[active], e.ctrlKey || e.metaKey);
      }
      // The page's own shortcuts must not see what is typed here.
      e.stopPropagation();
    });
    // A click on the backdrop is a click on the dialog itself, outside its box.
    dialog.addEventListener("click", (e) => { if (e.target === dialog) close(); });
    dialog.addEventListener("close", () => { input.value = ""; });
    document.body.appendChild(dialog);
    return { input, render };
  }

  let parts = null;
  function open() {
    if (!parts) parts = build();
    if (dialog.open) return;
    items = pages();
    active = 0;
    parts.render();
    dialog.showModal();
    parts.input.focus();
    load(parts.render);
  }
  function close() { if (dialog && dialog.open) dialog.close(); }

  document.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "k") {
      e.preventDefault();
      dialog && dialog.open ? close() : open();
    }
  });

  // The way in for someone who does not know the shortcut: a quiet search
  // field in the header that is really a button.
  const header = document.querySelector("header.top");
  if (header) {
    const mac = /Mac|iPhone|iPad/.test(navigator.platform || "");
    const button = el("button", "palette-open");
    button.type = "button";
    button.title = "Go to a page, session or tab";
    button.append(el("span", null, "Go to…"), el("kbd", null, mac ? "⌘K" : "Ctrl K"));
    button.addEventListener("click", open);
    const spacer = header.querySelector(".spacer");
    if (spacer) spacer.after(button);
    else header.appendChild(button);
  }

  return { open, close };
})();
