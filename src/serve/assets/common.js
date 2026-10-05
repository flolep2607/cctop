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
