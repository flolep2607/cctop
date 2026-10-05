"use strict";

// A reset timestamp as a wall-clock time — "resets 14:05" is read at a glance,
// where "in 40m" asks the reader to do the sum against a clock they may not be
// looking at.
const hhmm = (secs) => {
  if (!secs) return "";
  const d = new Date(Number(secs) * 1000);
  if (!isFinite(d.getTime())) return "";
  return String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0");
};

// The cost a row shows. `total` arrives as a six-decimal string — the JSON
// document is exact on purpose — so it is reformatted here rather than printed,
// which would put `$0.000000` in a column four characters wide.
//
// `incl`, `—` and a figure are three different claims: the plan bundles this,
// the provider records no usage at all, and here is what it cost. None of them
// may be rendered as either of the others.
const rowCost = (s) => {
  if (s.cost.included) return "incl";
  if (!s.cost.available) return "—";
  if (s.cost.total === null || s.cost.total === undefined) return "—";
  return money(Number(s.cost.total));
};

// When each session's chat was last opened: `{id: {seq, at}}` written by
// report.html, read here to mark rows with activity newer than the visit.
// Every access is behind try/catch — a private window throws on the read, and
// a shape from a version that stored it differently must not kill the render.
const readSeen = () => {
  try {
    return JSON.parse(localStorage.getItem("cctop-seen")) || {};
  } catch (e) {
    return {};
  }
};

let sessions = [];
let filter = "";
let runningOnly = false;
let sortBy = "recent";
// Transcript-search hits, kept with the query that produced them. The gate
// `searchFor === filter` wherever they are read means a stale answer — or one
// for a query the box no longer holds — shows nothing.
let searchFor = "";
let searchHits = new Map();
let searchTimer = 0;
// The keyboard selection is a session id, not a node: render() rebuilds every
// row, so the mark is re-applied to whichever new row carries the id — and
// dropped when none does.
let selId = null;
// Sessions pinned to the top of the table, and the state each attention row
// was last dismissed in — a dismissed session returns to the card on its own
// once it wants something new. Both persist through the cctop-dash store.
let pins = new Set();
let dismissed = {};
// The bulk-selection set: session ids marked for a send/resume across several
// rows at once. Deliberately not in the store — a mark that survived a reload
// could act on a session the reader no longer remembers choosing.
let picked = new Set();
// Sessions by id, for the two places that know an id and want its row's
// words: transcript hits joining back to their metadata, and the bulk bar
// naming the sessions it acted on.
let sessionById = new Map();
// report.html's "you last opened this chat at" record, refreshed every render
// because a chat open in another tab keeps rewriting it. Read, never written.
let seenNow = {};

// How the table is ordered, by what the reader picked.
//
// Every one of these is descending, because every one of them is a question of
// the form "which is the most" — the most recent, the most expensive, the
// fullest window. An ascending sort would answer a question nobody asks of this
// table.
//
// `recent` is what the server already sends, and re-sorting it here rather than
// leaning on that keeps the order a property of the page: a payload that ever
// arrives in a different order does not silently change what "Recent" means.
const ORDER = {
  recent: (a, b) => String(b.last_active).localeCompare(String(a.last_active)),
  cost: (a, b) => (Number(b.cost.total) || 0) - (Number(a.cost.total) || 0),
  tokens: (a, b) => (Number(b.tokens.total) || 0) - (Number(a.tokens.total) || 0),
  // A session whose harness reports no window sorts last rather than as zero:
  // "nothing is known" is not "the window is empty".
  context: (a, b) => share(b) - share(a),
};
const share = (s) => (s.context && s.context.max ? s.context.used / s.context.max : -1);
// What each session's state was last render, so a *transition* into waiting can
// be told from a session that has been waiting since before the page opened.
// Without it every refresh would re-notify about the same idle agent.
let previousState = new Map();
let firstRender = true;

const matches = (s, needle) => {
  if (!needle) return true;
  const hay = [
    s.project, s.title, s.model, s.harness, s.branch, s.provider,
    s.session_id, s.user, s.state,
  ].filter(Boolean).join(" ").toLowerCase();
  return needle.split(/\s+/).filter(Boolean).every((word) => hay.includes(word));
};

function contextBar(s) {
  const box = el("span", "ctx");
  const ctx = s.context;
  if (!ctx || !ctx.max) return null;
  const pct = Math.min(100, (ctx.used / ctx.max) * 100);
  const fill = el("i");
  fill.style.width = pct.toFixed(1) + "%";
  if (pct >= 90) fill.className = "full";
  else if (pct >= 70) fill.className = "hot";
  box.appendChild(fill);
  box.title = Math.round(pct) + "% of the context window";
  return box;
}

function rowFor(s) {
  const row = el("a", "row");
  row.href = "/session/" + encodeURIComponent(s.session_id) + QUERY;
  row.setAttribute("role", "listitem");
  // The id the keyboard selection navigates by — the only handle left once
  // the node itself is thrown away on the next render.
  row.dataset.id = s.session_id;
  if (s.session_id === selId) row.classList.add("sel");
  if (picked.has(s.session_id)) row.classList.add("picked");

  row.appendChild(el("span", "dot " + (s.running || s.state === "error" ? s.state : "idle")));

  const name = el("div", "name trunc");
  if (pins.has(s.session_id)) name.appendChild(el("span", "pin", "★"));
  if (picked.has(s.session_id)) name.appendChild(el("span", "tick", "✓"));
  name.appendChild(el("span", null, s.project ? shortPath(s.project) : s.session_id.slice(0, 8)));
  if (s.title) {
    name.appendChild(el("span", "title", " · " + s.title));
  }
  // New activity since the chat was last opened — the marker needs both a
  // visit on record and something after it, so a session never opened and one
  // read to the end both stay unmarked.
  const seen = seenNow[s.session_id] || seenNow[s.provider + ":" + s.session_id];
  if (seen && Date.parse(s.last_active) > Number(seen.at)) {
    const dot = el("span", "newdot");
    dot.title = "activity since you last opened this session";
    name.appendChild(dot);
  }
  // The full path is worth having, but not worth the width. `title` is also
  // what a screen reader reads out, which is the same trade.
  if (s.project) name.title = s.project;
  row.appendChild(name);

  const meta = el("div", "meta");
  const tag = (text, cls) => { if (text) meta.appendChild(el("span", cls || null, text)); };
  // Harness first, then the model: on a machine running several agents the
  // model alone does not say who ran the session — `opus-5` under claude and
  // under a devin row are different sessions.
  tag(s.provider);
  if (s.model) tag(shortModel(s.model));
  if (s.branch) tag(s.branch);
  tag(ago(s.last_active));
  if (s.running && s.state === "waiting") tag("waiting on you", "pill warn");
  // Louder than "waiting on you", and deliberately so: that one is your move
  // whenever you get to it, this one is an agent stopped mid-tool until you say.
  if (s.running && s.state === "asking") tag("needs permission", "pill bad");
  // "api error" in the present tense is a claim about now. For a stopped
  // session it is something that happened, and the row's last-active time is
  // already saying when.
  if (s.state === "error") tag(s.running ? "api error" : "ended on an api error", "pill bad");
  // A session with a quarter of its tool calls failing is retrying something
  // that will not work, and paying for every attempt.
  if (s.activity.tool_errors > 0 && s.activity.tool_count > 0) {
    const rate = s.activity.tool_errors / s.activity.tool_count;
    if (rate >= 0.25) tag(Math.round(rate * 100) + "% tool errors", "pill bad");
  }
  if (s.conflict) tag(s.conflict.level === "file" ? "same file as another agent" : "same repo as another agent", "pill warn");
  if (s.user) tag(s.user);
  // Which Claude login this ran under. Absent for a machine with one profile
  // and for every harness that has no such concept, so the tag appears exactly
  // where it distinguishes something.
  if (s.profile && s.profile !== "default") tag(s.profile);
  const bar = contextBar(s);
  if (bar) meta.appendChild(bar);
  // A transcript-search hit, quoted last and on its own line. It shows for a
  // session the metadata filter matched too — the snippet says what the
  // transcript holds, which the metadata never could.
  if (searchFor === filter && searchHits.has(s.session_id)) {
    meta.appendChild(el("span", "snip trunc", "“" + searchHits.get(s.session_id) + "”"));
  }
  row.appendChild(meta);

  const figures = el("div", "figures");
  figures.appendChild(el("div", null, rowCost(s)));
  figures.appendChild(el("div", "sub", tokens(s.tokens.total) + " tok"));
  row.appendChild(figures);

  return row;
}

// --- talking to the server -------------------------------------------------

// Answering a waiting agent from the list, which is the whole point of the
// card it sits in: a page that says "this one needs you" and cannot be replied
// to has shown someone a problem and kept the fix.
//
// One line, because that is what submitting to a pty is — the server refuses a
// newline rather than sending half of a paragraph.
function answerBox(s) {
  if (!CAN_ACT || !s.running) return null;
  const form = el("form", "say");
  const input = el("input");
  input.type = "text";
  input.maxLength = 4000;
  input.autocomplete = "off";
  input.placeholder = "Answer this session…";
  const send = el("button", "primary", "Send");
  send.type = "submit";
  const said = el("span", "said", "");
  // The one POST, shared by the typed answer and the one-tap chips below: a
  // chip is the Send button with the word already chosen — same route, same
  // disabled-while-in-flight, same error line.
  const post = async (verb, payload, button) => {
    button.disabled = true;
    said.className = "said";
    said.textContent = "…";
    try {
      const response = await ask(
        "/api/act/" + verb + "/" + encodeURIComponent(s.session_id) + QUERY,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(payload),
        },
      );
      const done = await response.json().catch(() => ({}));
      said.textContent = (done.message || "sent").toLowerCase();
      return true;
    } catch (e) {
      said.className = "said bad";
      said.textContent = String(e.message || e);
      return false;
    } finally {
      button.disabled = false;
    }
  };
  // The commonest answers to a waiting agent are single words, and one tap on
  // a phone beats typing any of them. Only the states with an obvious word
  // get chips — anything else is a real answer and wants the box.
  //
  // A permission prompt is not answered with a word: it is a menu, and these
  // once typed "yes" or "no" and pressed Enter — which Claude Code reads as
  // the highlighted option, "Yes", either way. So those two go to the answer
  // route, which presses the key that harness's own menu names, and only for
  // the harnesses it knows the keys of.
  const QUICK = {
    asking: ANSWERABLE.has(s.provider)
      ? [["Allow", "answer", { choice: "allow" }], ["Deny", "answer", { choice: "deny" }]]
      : [],
    waiting: [["Continue", "send", { text: "continue" }]],
  };
  for (const [label, verb, payload] of QUICK[s.state] || []) {
    const chip = el("button", "chip", label);
    chip.type = "button";
    chip.addEventListener("click", () => { post(verb, payload, chip); });
    form.appendChild(chip);
  }
  form.appendChild(input);
  form.appendChild(send);
  form.appendChild(said);
  // A picture pasted into the box, which is the one way an image reaches an
  // agent on a machine you are only sshed into: a terminal carries text and
  // never an image, but a browser reads a real one off the clipboard. The
  // bytes go over this connection, land in a file on the agent's machine, and
  // the box is filled with the path — which is how every one of these agents
  // reads an image. Text pastes are left entirely alone.
  input.addEventListener("paste", (event) => {
    const items = Array.from(event.clipboardData?.items || []);
    const picture = items.find((i) => i.type && i.type.startsWith("image/"));
    if (!picture) return;
    const file = picture.getAsFile();
    if (!file) return;
    event.preventDefault();
    said.className = "said";
    said.textContent = "filing the image…";
    const reader = new FileReader();
    reader.onerror = () => {
      said.className = "said bad";
      said.textContent = "could not read that image";
    };
    reader.onload = async () => {
      try {
        const response = await ask(
          "/api/act/image/" + encodeURIComponent(s.session_id) + QUERY,
          {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ data: String(reader.result) }),
          },
        );
        const filed = await response.json();
        // Appended rather than replacing: the sentence about the picture is
        // usually already half typed by the time it is pasted.
        const room = input.value && !input.value.endsWith(" ") ? " " : "";
        input.value = input.value + room + filed.path + " ";
        input.focus();
        said.textContent = "image filed on that machine";
      } catch (e) {
        said.className = "said bad";
        said.textContent = String(e.message || e);
      }
    };
    // A data: URL, which is the spelling the server already recognises from
    // the terminal side.
    reader.readAsDataURL(file);
  });
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const text = input.value.trim();
    if (!text) return;
    if (await post("send", { text }, send)) input.value = "";
  });
  return form;
}

// A row in the attention card: the link, the box for replying to it, and a
// way to wave it away.
function wantingNode(s) {
  const box = el("div", "wanting");
  box.appendChild(pickableRow(s));
  // What Allow would allow. Without it the page asks for a yes to a question
  // it has not shown, which is approving blind.
  if (s.running && s.state === "asking" && s.asking_for) {
    const asked = el("div", "asked");
    asked.appendChild(el("code", null, s.asking_for));
    box.appendChild(asked);
  }
  const say = answerBox(s);
  if (say) box.appendChild(say);
  // Dismissal is recorded against the state that was dismissed, not the
  // session — so the row is back the moment it wants something new, and the
  // session's place in the main table is never touched.
  const dismiss = el("button", "dismiss", "×");
  dismiss.type = "button";
  dismiss.title = "Dismiss until its state changes";
  dismiss.setAttribute("aria-label", dismiss.title);
  dismiss.addEventListener("click", () => {
    dismissed[s.session_id] = s.state;
    store.write();
    render();
  });
  box.appendChild(dismiss);
  return box;
}

function render() {
  const shown = sessions
    // A session whose metadata matches nothing still belongs when the
    // transcript search says the words are inside it — the snippet is how the
    // row explains itself.
    .filter((s) => matches(s, filter) || (searchFor === filter && searchHits.has(s.session_id)))
    .filter((s) => !runningOnly || s.running)
    .sort(ORDER[sortBy] || ORDER.recent);

  // The selection outlives the rebuild below only while its session is still
  // on screen — a session that filtered away or ended takes the mark with it.
  if (selId && !shown.some((s) => s.session_id === selId)) selId = null;
  // The bulk set holds while its sessions exist, not while they are shown:
  // filtering is a view, and a mark that a keystroke wiped would make picking
  // a row and then looking for the next one impossible. A session that leaves
  // the list entirely — ended, or walked away on the server's side — drops
  // its mark, since acting on it is no longer possible anyway.
  for (const id of picked) {
    if (!sessions.some((s) => s.session_id === id)) picked.delete(id);
  }
  // Re-read on every render rather than once at load: a chat opened in
  // another tab since the last pass writes the record this reads.
  seenNow = readSeen();

  // Anything a person has to answer, first and on its own.
  //
  // Live sessions only. A session that hit an API error in May and has not run
  // since is history, not a call to action — promoting it puts four dead rows
  // above the one agent actually waiting, which is the opposite of what this
  // block is for. The row keeps its red dot either way; that is the table
  // saying what happened, which is a different claim from "answer me".
  // A dismissal holds only for the state it was made in: waiting → asking →
  // working → waiting again is a new call to action, and an old × never
  // silences it.
  const wanting = shown.filter(
    (s) =>
      s.running &&
      (s.state === "waiting" || s.state === "asking" || s.state === "error") &&
      dismissed[s.session_id] !== s.state,
  );
  // The whole point of keeping this page in a pinned or background tab is
  // noticing when a session starts waiting. The tab cannot ring a bell, but
  // its title can announce the count — and it asks for no notification
  // permission to do it.
  document.title = wanting.length ? "(" + wanting.length + ") cctop" : "cctop";
  const rest = shown.filter((s) => !wanting.includes(s));
  // Pins lift a session within the table only — the card's order belongs to
  // who needs answering, not to who was starred. The sort is stable, so the
  // chosen order holds inside the pinned half and the unpinned alike.
  rest.sort((a, b) => Number(pins.has(b.session_id)) - Number(pins.has(a.session_id)));

  const attention = document.getElementById("attention");
  const attentionRows = document.getElementById("attention-rows");
  // Rebuilt on every refresh, so a half-typed answer would be thrown away
  // twice a second — along with the focus and the cursor, which is worse: the
  // rest of the word goes into a box that no longer exists. What is typed, what
  // was focused and where the caret sat all cross over.
  const typed = new Map();
  let focused = null;
  let caret = 0;
  for (const form of attentionRows.querySelectorAll("form.say")) {
    const input = form.querySelector("input");
    if (!form.dataset.session) continue;
    if (input.value) typed.set(form.dataset.session, input.value);
    if (document.activeElement === input) {
      focused = form.dataset.session;
      caret = input.selectionStart;
    }
  }
  attentionRows.replaceChildren(...wanting.map((s) => {
    const node = wantingNode(s);
    const form = node.querySelector("form.say");
    if (!form) return node;
    form.dataset.session = s.session_id;
    const input = form.querySelector("input");
    const carried = typed.get(s.session_id);
    if (carried) input.value = carried;
    if (focused === s.session_id) {
      input.focus();
      input.setSelectionRange(caret, caret);
    }
    return node;
  }));
  attention.hidden = wanting.length === 0;
  document.getElementById("attention-heading").textContent =
    wanting.length === 1 ? "1 session needs you" : wanting.length + " sessions need you";

  document.getElementById("rows").replaceChildren(...rest.map(pickableRow));
  const empty = document.getElementById("empty");
  empty.hidden = shown.length > 0;
  if (shown.length === 0 && sessions.length > 0) empty.textContent = "Nothing matches that filter.";

  const running = sessions.filter((s) => s.running).length;
  document.getElementById("counts").textContent =
    sessions.length + " sessions · " + running + " running" +
    (shown.length !== sessions.length ? " · " + shown.length + " shown" : "");

  const totals = document.getElementById("totals");
  const today = sessions.reduce((sum, s) => sum + (s.cost.today || 0), 0);
  // Rolling, as the TUI's $/1H column is: the clock-hour `this_hour` reads
  // $0.00 a minute after every :00, however busy the minutes before it were.
  const hour = sessions.reduce((sum, s) => sum + (s.cost.last_hour || 0), 0);
  totals.replaceChildren();
  for (const [label, value] of [["today", money(today)], ["last 60 min", money(hour)]]) {
    const box = el("span");
    box.appendChild(el("b", null, value));
    box.appendChild(document.createTextNode(" "));
    box.appendChild(el("span", "k", label));
    totals.appendChild(box);
  }

  renderBulk();
}

// A desktop notification for the moment a session crosses into waiting — the
// same event the terminal's `w` rings a bell for, and the reason this page is
// worth having open on a second screen at all.
const bell = document.getElementById("bell");
let notifying = false;
bell.addEventListener("click", async () => {
  if (notifying) {
    notifying = false;
    bell.setAttribute("aria-pressed", "false");
    return;
  }
  if (!("Notification" in window)) {
    bell.textContent = "No notifications";
    bell.disabled = true;
    return;
  }
  // Must be inside the click: browsers refuse a permission prompt that no
  // gesture asked for, and refusing it once is remembered.
  const granted = await Notification.requestPermission();
  notifying = granted === "granted";
  bell.setAttribute("aria-pressed", String(notifying));
  if (!notifying) bell.title = "Your browser refused notification permission for this page";
});

function announce(next) {
  if (!notifying || firstRender) return;
  for (const s of next) {
    const was = previousState.get(s.session_id);
    if (was && was !== "asking" && s.state === "asking") {
      new Notification("Needs permission", {
        body: (s.project || s.session_id.slice(0, 8)) + (s.title ? " · " + s.title : ""),
        tag: s.session_id,
      });
    }
    if (was && was !== "waiting" && s.state === "waiting") {
      new Notification("Waiting on you", {
        body: (s.project || s.session_id.slice(0, 8)) + (s.title ? " · " + s.title : ""),
        tag: s.session_id,
      });
    }
  }
}

function apply(next) {
  announce(next);
  previousState = new Map(next.map((s) => [s.session_id, s.state]));
  sessions = next;
  sessionById = new Map(next.map((s) => [s.session_id, s]));
  firstRender = false;
  render();
}

// --- subscription windows --------------------------------------------------

// The other half of "can I afford to let this run": not what the sessions
// spent, but how much of each provider's window is already gone. It moves in
// minutes rather than milliseconds, so it polls on its own slow clock instead
// of riding the session stream — and a server old enough to lack the route
// leaves the strip empty, which the CSS folds away entirely.
function renderQuota(data) {
  const out = [];
  for (const provider of Object.keys(data || {})) {
    for (const entry of data[provider] || []) {
      // The provider name plus the profile when there is more than the one —
      // the same distinction the session rows make.
      const who = provider + (entry.profile && entry.profile !== "default" ? "·" + entry.profile : "");
      if (entry.status === "ok") {
        const box = el("span", "qp");
        box.appendChild(el("span", "qwho", who));
        for (const w of entry.windows || []) {
          const item = el("span", "qw");
          const pct = Math.max(0, Math.min(100, Number(w.pct) || 0));
          item.appendChild(el("span", null, w.label + " "));
          item.appendChild(el("b", null, Math.round(pct) + "%"));
          const bar = el("span", "ctx");
          const fill = el("i");
          fill.style.width = pct.toFixed(1) + "%";
          // The context bar's thresholds, reused: a window nearly spent reads
          // exactly like a context nearly full.
          if (w.limit_reached || pct >= 90) fill.className = "full";
          else if (pct >= 70) fill.className = "hot";
          bar.appendChild(fill);
          item.appendChild(document.createTextNode(" "));
          item.appendChild(bar);
          const at = hhmm(w.resets_at);
          if (at) item.title = w.label + " resets " + at;
          box.appendChild(item);
        }
        out.push(box);
      } else {
        // A status earns a mention only when it asks something of the reader:
        // "sign in again" and "wait until" can be acted on, while a billing
        // state or a provider that never answered is just noise in a header.
        const resets = (entry.windows || []).map((w) => w.resets_at).find(Boolean);
        const note =
          entry.status === "expired" ? "sign-in expired" :
          entry.status === "not_signed_in" ? "not signed in" :
          entry.status === "rate_limited" ? "rate-limited" + (resets ? " until " + hhmm(resets) : "") :
          "";
        if (!note) continue;
        const box = el("span", null, who + ": " + note);
        if (entry.detail) box.title = entry.detail;
        out.push(box);
      }
    }
  }
  document.getElementById("quota").replaceChildren(...out);
}

async function pollQuota() {
  try {
    renderQuota(await ask("/api/quota" + QUERY).then(asJson));
  } catch (e) {
    // A failed poll leaves whatever was last shown. The strip answers a
    // nice-to-have question; it never earns a banner.
  }
}
pollQuota();
setInterval(pollQuota, 60000);

// --- tabs --------------------------------------------------------------------

// The TUI's tab bar, read from rmux by /api/tabs: every agent cctop has open,
// in the order the bar shows them, whether or not a dashboard is running
// anywhere. A tab opens its terminal in the drawer below, drawn inside the page
// from cctop's own copy of rmux's terminal app (src/serve/term.rs).
let openTab = null;

function tabState(t) {
  return t.state === "needs-input" ? "asking" : t.state === "working" ? "working" : "idle";
}

function renderTabs(list) {
  const bar = document.getElementById("tabs");
  bar.replaceChildren();
  bar.hidden = !list.length;
  for (const t of list) {
    const tab = el("button", "tab");
    tab.type = "button";
    tab.setAttribute("aria-pressed", String(openTab === t.name));
    tab.title = (t.cwd ? shortPath(t.cwd) + " · " : "") +
      (CAN_ACT ? "open this tab's terminal" : "this link cannot open terminals");
    tab.appendChild(el("span", "dot " + tabState(t)));
    tab.appendChild(el("span", "name", t.label));
    if (t.session_id) {
      // The session's own page, for the conversation rather than the screen.
      const page = el("a", null, "page");
      page.href = "/session/" + encodeURIComponent(t.session_id) + QUERY;
      page.addEventListener("click", (ev) => ev.stopPropagation());
      tab.appendChild(page);
    }
    if (CAN_ACT) tab.addEventListener("click", () => openTerminal(t));
    else tab.disabled = !t.session_id;
    bar.appendChild(tab);
  }
}

async function pollTabs() {
  try {
    renderTabs((await ask("/api/tabs" + QUERY).then(asJson)).tabs || []);
  } catch (e) {
    // An older cctop has no /api/tabs; the bar simply stays hidden.
  }
}

// The share's fragment on our copy of the app — see terminalFrame in
// report.js, which this mirrors.
function terminalFrame(url) {
  const at = url.indexOf("#");
  const frame = document.createElement("iframe");
  frame.className = "termframe";
  frame.title = "Terminal";
  frame.allow = "clipboard-read; clipboard-write";
  frame.src = "/term/" + QUERY + (at >= 0 ? url.slice(at) : "");
  return frame;
}

function closeTerminal() {
  // Removed rather than hidden: the frame holds a live socket to the agent.
  document.getElementById("term-body").replaceChildren();
  document.getElementById("termdrawer").hidden = true;
  openTab = null;
  pollTabs();
}

async function openTerminal(t) {
  if (openTab === t.name) return closeTerminal();
  const drawer = document.getElementById("termdrawer");
  const body = document.getElementById("term-body");
  const popout = document.getElementById("term-popout");
  document.getElementById("term-title").textContent = t.label + (t.cwd ? " · " + shortPath(t.cwd) : "");
  popout.hidden = true;
  body.replaceChildren(el("div", "empty", "Opening this tab's terminal…"));
  drawer.hidden = false;
  openTab = t.name;
  pollTabs();
  try {
    const terminal = await ask("/api/tab/" + encodeURIComponent(t.name) + "/terminal" + QUERY, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ origin: location.origin }),
    }).then(asJson);
    if (openTab !== t.name) return;
    popout.href = terminal.url;
    popout.hidden = false;
    body.replaceChildren(
      terminalFrame(terminal.url),
      el("div", "why",
        (terminal.tunnelled
          ? "Through rmux's own tunnel, so it works from wherever you are. "
          : "Served from the machine cctop runs on, so it only opens from a browser on it. ") +
        "Whoever holds this page can type into this agent — it is a shell, not a prompt box."),
    );
  } catch (e) {
    if (openTab === t.name) body.replaceChildren(el("div", "empty", String(e.message || e)));
  }
}

document.getElementById("term-close").addEventListener("click", closeTerminal);
pollTabs();
setInterval(pollTabs, 5000);

// --- transcript search -------------------------------------------------------

// Rides next to the metadata filter, never in place of it: a query of a few
// characters asks the server which transcripts mention it, and a session whose
// row matched nothing visible still shows, quoting the line that did. Any
// failure just means no extra rows — the filter keeps working against a server
// that has no search route at all.
async function transcriptSearch(q) {
  let data;
  try {
    data = await ask(
      "/api/search?q=" + encodeURIComponent(q) + (TOKEN ? "&t=" + encodeURIComponent(TOKEN) : ""),
    ).then(asJson);
  } catch (e) {
    return;
  }
  // The box may have moved on while the request was in flight; hits for a
  // query it no longer holds would surface rows that match nothing.
  if (q !== filter) return;
  searchFor = q;
  searchHits = new Map();
  for (const hit of data.hits || []) {
    if (hit && hit.session_id && hit.snippet && !searchHits.has(hit.session_id)) {
      searchHits.set(hit.session_id, String(hit.snippet));
    }
  }
  render();
}

// --- the transcript-results card ---------------------------------------------

// The filter asks "which of these sessions"; the card answers "which
// transcripts hold these words at all" — every session the server knows, not
// only the rows on screen. Enter in the filter or the button beside it opens
// it; Escape or its × puts the table back, which was only hidden, never torn
// down — so the attention card, the marks and a half-typed answer all survive.
const findCard = document.getElementById("find");
const findRows = document.getElementById("find-rows");
const findHeading = document.getElementById("find-heading");
const tableview = document.getElementById("tableview");
const findOpen = () => !findCard.hidden;

function closeFind() {
  findCard.hidden = true;
  tableview.hidden = false;
}

// One hit, drawn like a session row but linked to the chat with its find box
// already holding the query — `?find=` is the report page's spelling for that.
// The route's answer carries only the id and the snippet; the project, title
// and provider are the session's own, joined back from the table's data.
function findRowFor(hit, q) {
  const id = hit.session_id || hit.id || "";
  const s = sessionById.get(id);
  const row = el("a", "row");
  row.href =
    "/session/" + encodeURIComponent(id) +
    (QUERY ? QUERY + "&find=" : "?find=") + encodeURIComponent(q);
  row.setAttribute("role", "listitem");
  row.dataset.id = id;
  row.appendChild(
    el("span", "dot " + (s && (s.running || s.state === "error") ? s.state : "idle")),
  );
  const name = el("div", "name trunc");
  const project = (s && s.project) || hit.project;
  name.appendChild(el("span", null, project ? shortPath(project) : id.slice(0, 8)));
  const title = (s && s.title) || hit.title;
  if (title) name.appendChild(el("span", "title", " · " + title));
  if (project) name.title = project;
  row.appendChild(name);
  const meta = el("div", "meta");
  const provider = (s && s.provider) || hit.provider;
  const model = (s && s.model) || hit.model;
  if (provider) meta.appendChild(el("span", null, provider));
  if (model) meta.appendChild(el("span", null, shortModel(model)));
  if (s && s.last_active) meta.appendChild(el("span", null, ago(s.last_active)));
  if (hit.snippet) {
    meta.appendChild(el("span", "snip trunc", "“" + hit.snippet + "”"));
  }
  row.appendChild(meta);
  return row;
}

async function openFind() {
  const q = filterBox.value.trim();
  if (!q) {
    filterBox.focus();
    return;
  }
  tableview.hidden = true;
  findCard.hidden = false;
  findHeading.textContent = "Transcript search — “" + q + "”";
  // The server floors short queries rather than answering them; the card says
  // so instead of asking and reporting an empty answer as "nothing mentions".
  if (q.length < 3) {
    findRows.replaceChildren(
      el("div", "empty", "Three characters or more to search transcripts."),
    );
    return;
  }
  findRows.replaceChildren(el("div", "empty", "Searching every transcript…"));
  let hits;
  try {
    const data = await ask(
      "/api/search?q=" + encodeURIComponent(q) +
        (TOKEN ? "&t=" + encodeURIComponent(TOKEN) : ""),
    ).then(asJson);
    // The route answers {"hits": [...]}; a bare array is accepted too — it is
    // the shape an earlier version of the route was described as having.
    hits = Array.isArray(data) ? data : data.hits || [];
  } catch (e) {
    if (findOpen()) {
      findRows.replaceChildren(el("div", "empty bad", String(e.message || e)));
    }
    return;
  }
  // Closed while the search was in flight: the card's state belongs to the
  // view that asked for it, and that view is gone.
  if (!findOpen()) return;
  if (!hits.length) {
    findRows.replaceChildren(el("div", "empty", "No transcript mentions that."));
    return;
  }
  findRows.replaceChildren(...hits.map((h) => findRowFor(h, q)));
}

// --- bulk selection ------------------------------------------------------------

// `x` on the keyboard-selected row, or the checkbox in the row's gutter, marks
// a session for a verb that takes several at once. A serve that may not act
// gets none of this: no marks, no bar, no wiring — there would be nothing
// behind it to run.
function pickableRow(s) {
  const row = rowFor(s);
  if (!CAN_ACT) return row;
  const wrap = el("div", "pickrow");
  const box = el("input", "pick");
  box.type = "checkbox";
  box.checked = picked.has(s.session_id);
  box.title = "Select for a bulk action";
  box.setAttribute(
    "aria-label",
    box.title + " — " + (s.project ? shortPath(s.project) : s.session_id.slice(0, 8)),
  );
  box.addEventListener("click", () => togglePick(s.session_id));
  wrap.appendChild(box);
  wrap.appendChild(row);
  return wrap;
}

function togglePick(id) {
  if (picked.has(id)) picked.delete(id);
  else picked.add(id);
  render();
}

const bulkBar = document.getElementById("bulk");
const bulkForm = document.getElementById("bulk-form");
const bulkText = document.getElementById("bulk-text");
const bulkSaid = document.getElementById("bulk-said");
// One bulk run at a time; the buttons are disabled for the flight, so this is
// a guard against a keypress landing between the click and the disable.
let bulkBusy = false;

function renderBulk() {
  if (!CAN_ACT) return;
  bulkBar.hidden = picked.size === 0;
  document.getElementById("bulk-count").textContent = picked.size + " selected";
}

// How a selected row is named in the summary — its project, or the id's head
// when there is none, the same choice the row itself makes.
const nameOf = (id) => {
  const s = sessionById.get(id);
  return s && s.project ? shortPath(s.project) : id.slice(0, 8);
};

// One verb against every marked session, one request at a time: these type at
// real terminals and start real agents, and a parallel burst is the mistake
// the answer box's one-POST-at-a-time already avoids. The summary names the
// failures rather than counting them — "2 failed" still has to be answered by
// hand, while the names are the answer.
async function bulkAct(verb, text) {
  if (bulkBusy || picked.size === 0) return;
  bulkBusy = true;
  const buttons = bulkBar.querySelectorAll("button");
  for (const b of buttons) b.disabled = true;
  bulkSaid.className = "said";
  bulkSaid.textContent = "…";
  const done = [];
  const failed = [];
  for (const id of picked) {
    try {
      await ask("/api/act/" + verb + "/" + encodeURIComponent(id) + QUERY, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        // The route insists on a JSON object even when the verb takes no
        // arguments — a resume carries an empty one.
        body: JSON.stringify(verb === "send" ? { text } : {}),
      });
      done.push(id);
    } catch (e) {
      failed.push(nameOf(id) + " — " + String(e.message || e));
    }
  }
  bulkSaid.textContent =
    (verb === "send" ? "sent to " : "resumed ") + done.length +
    (failed.length
      ? " · failed: " + failed.map((f) => f.split(" — ")[0]).join(", ")
      : "");
  bulkSaid.title = failed.join("\n");
  bulkSaid.className = failed.length ? "said bad" : "said";
  if (!failed.length && verb === "send") {
    bulkText.value = "";
    bulkForm.hidden = true;
  }
  for (const b of buttons) b.disabled = false;
  bulkBusy = false;
}

if (CAN_ACT) {
  document.getElementById("bulk-send").addEventListener("click", () => {
    bulkForm.hidden = !bulkForm.hidden;
    if (!bulkForm.hidden) bulkText.focus();
  });
  document.getElementById("bulk-resume").addEventListener("click", () => bulkAct("resume"));
  document.getElementById("bulk-clear").addEventListener("click", () => {
    picked.clear();
    bulkForm.hidden = true;
    bulkSaid.textContent = "";
    render();
  });
  bulkForm.addEventListener("submit", (event) => {
    event.preventDefault();
    const text = bulkText.value.trim();
    if (text) bulkAct("send", text);
  });
  bulkText.addEventListener("keydown", (event) => {
    if (event.key === "Escape") bulkForm.hidden = true;
  });
}

// --- the live connection ---------------------------------------------------

const link = document.getElementById("link");
const linkText = document.getElementById("link-text");
function setLink(state, text) {
  link.className = "dot " + state;
  linkText.textContent = text;
}

let source;
function connect() {
  source = new EventSource("/api/events" + QUERY);
  source.addEventListener("sessions", (event) => {
    setLink("working", "live");
    try { apply(JSON.parse(event.data)); } catch (e) { setLink("error", "bad payload"); }
  });
  // EventSource reconnects on its own, but only from a stream that broke. An
  // answer that was never a stream — the HTML error page a tunnel serves once
  // its far end is gone — closes it for good, and the page would sit on
  // "reconnecting…" for ever. So a closed source is reopened here, on a timer,
  // because a stale table that looks live is the one failure this page must
  // not have.
  source.addEventListener("error", () => {
    if (source.readyState !== EventSource.CLOSED) {
      setLink("waiting", "reconnecting…");
      return;
    }
    setLink("error", "cctop is unreachable");
    setTimeout(connect, 5000);
  });
  source.addEventListener("open", () => setLink("working", "live"));
}

ask("/api/hosts" + QUERY)
  .then(asJson)
  .then((failed) => {
    const banners = document.getElementById("banners");
    for (const [host, why] of failed) {
      banners.appendChild(el("div", "banner", host + " could not be read: " + why));
    }
  })
  .catch(() => {});

// The launcher, offered only where this run can act — a read-only link or a
// --no-actions serve has nothing behind the button, so it is never drawn.
if (CAN_ACT) {
  ask("/api/agents" + QUERY)
    .then(asJson)
    .then((r) => {
      const known = r.agents || [];
      if (!r.actions || !known.length) return;
      const form = document.getElementById("launch");
      const pick = document.getElementById("launch-agent");
      const cwd = document.getElementById("launch-cwd");
      const said = document.getElementById("launch-said");
      const go = form.querySelector("button");
      for (const agent of known) pick.appendChild(el("option", null, agent));
      form.hidden = false;
      form.addEventListener("submit", async (event) => {
        event.preventDefault();
        go.disabled = true;
        said.className = "said";
        said.textContent = "…";
        try {
          const response = await ask("/api/launch" + QUERY, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ agent: pick.value, cwd: cwd.value.trim() }),
          });
          const done = await asJson(response);
          said.textContent = done.message || "started";
        } catch (e) {
          said.className = "said bad";
          said.textContent = String(e.message || e);
        } finally {
          go.disabled = false;
        }
      });
    })
    .catch(() => {});
}

// --- the controls ------------------------------------------------------------

const filterBox = document.getElementById("filter");
const sortBox = document.getElementById("sort");
const runningBtn = document.getElementById("running");

// The controls survive a reload: a filter someone typed is the question they
// were asking, and a refresh that forgets it makes them ask again. One key,
// one small object, every access behind try/catch — private windows and
// embedded contexts throw on localStorage, and the page must not die on it.
// The bell is not in here on purpose: notification permission belongs to the
// browser, and re-arming it silently would claim a yes the reader gave for
// one sitting, not for ever.
const store = {
  read() {
    try {
      return JSON.parse(localStorage.getItem("cctop-dash")) || {};
    } catch (e) {
      return {};
    }
  },
  write() {
    try {
      localStorage.setItem(
        "cctop-dash",
        JSON.stringify({
          filter: filterBox.value,
          sort: sortBy,
          running: runningOnly,
          pins: [...pins],
          dismissed,
        }),
      );
    } catch (e) {}
  },
};

// Restored as if it had just been typed and picked: the box and the variable
// it feeds get the value together, and a long-enough query goes back to the
// transcript search so its snippet rows come back with it.
const prefs = store.read();
if (typeof prefs.filter === "string" && prefs.filter) {
  filterBox.value = prefs.filter;
  filter = prefs.filter.trim().toLowerCase();
  if (filter.length >= 3) transcriptSearch(filter);
}
if (prefs.sort && ORDER[prefs.sort]) {
  sortBy = prefs.sort;
  sortBox.value = sortBy;
}
if (prefs.running) {
  runningOnly = true;
  runningBtn.setAttribute("aria-pressed", "true");
}
// Stored objects from before pins and dismissals existed have neither key;
// both simply stay empty.
if (Array.isArray(prefs.pins)) pins = new Set(prefs.pins);
if (prefs.dismissed && typeof prefs.dismissed === "object") {
  dismissed = prefs.dismissed;
}

filterBox.addEventListener("input", (e) => {
  filter = e.target.value.trim().toLowerCase();
  // The metadata filter answers instantly; the transcript is asked only once
  // typing pauses, and only for a query long enough to mean something. A box
  // back under three characters just stops showing hits — the cached ones are
  // gated on the query anyway, so nothing needs clearing.
  clearTimeout(searchTimer);
  if (filter.length >= 3 && filter !== searchFor) {
    searchTimer = setTimeout(() => transcriptSearch(filter), 400);
  }
  render();
  store.write();
});
sortBox.addEventListener("change", (e) => {
  sortBy = e.target.value;
  store.write();
  render();
});
runningBtn.addEventListener("click", (e) => {
  runningOnly = !runningOnly;
  e.currentTarget.setAttribute("aria-pressed", String(runningOnly));
  store.write();
  render();
});
document.getElementById("gsearch").addEventListener("click", openFind);
document.getElementById("find-close").addEventListener("click", closeFind);

// Every page-level key shares this guard: none of them may fire while the
// reader is typing — in the filter itself, the sort select, an answer box, or
// anything editable.
const typing = (t) =>
  t instanceof HTMLElement &&
  (t.isContentEditable || /^(INPUT|SELECT|TEXTAREA)$/.test(t.tagName));

// `/` reaches the filter from anywhere, Escape inside it clears and leaves.
document.addEventListener("keydown", (e) => {
  if (e.key !== "/" || e.ctrlKey || e.metaKey || e.altKey) return;
  if (typing(e.target)) return;
  e.preventDefault();
  filterBox.focus();
  if (filterBox.value) filterBox.select();
});
filterBox.addEventListener("keydown", (e) => {
  // Enter in the filter is the transcript search: the metadata match keeps
  // answering as typing happens, but asking every transcript is deliberate —
  // a keypress, not a debounce.
  if (e.key === "Enter") {
    e.preventDefault();
    openFind();
    return;
  }
  if (e.key !== "Escape") return;
  e.preventDefault();
  // Cleared by running the same path as typing it empty: `filter`, the
  // debounce and the transcript-hit gating all follow the box, so nothing —
  // snippets included — lingers after it.
  filterBox.value = "";
  filterBox.dispatchEvent(new Event("input"));
  filterBox.blur();
});

// j/k walk the rows in the order the page shows them — the attention card
// first, then the table — and Enter follows whichever is marked. The rows
// are anchors, so "open" is just a click.
// Only rows on screen count: while the search-results card is open the table
// is display:none, and its rows must not take j/k or Enter behind its back.
const listedRows = () =>
  [...document.querySelectorAll(".row[data-id]")].filter((r) => r.offsetParent !== null);

function moveSelection(dir) {
  const rows = listedRows();
  if (!rows.length) return;
  const at = rows.findIndex((row) => row.dataset.id === selId);
  // From nothing, j lands on the first row and k on the last; past either
  // end the mark just stays where it is.
  const next =
    at < 0
      ? (dir > 0 ? rows[0] : rows[rows.length - 1])
      : rows[Math.min(rows.length - 1, Math.max(0, at + dir))];
  selId = next.dataset.id;
  for (const row of rows) row.classList.toggle("sel", row === next);
  next.scrollIntoView({ block: "nearest" });
}

document.addEventListener("keydown", (e) => {
  if (e.ctrlKey || e.metaKey || e.altKey || typing(e.target)) return;
  if (e.key === "j" || e.key === "ArrowDown") {
    e.preventDefault();
    moveSelection(1);
  } else if (e.key === "k" || e.key === "ArrowUp") {
    e.preventDefault();
    moveSelection(-1);
  } else if (e.key === "Enter" && selId) {
    // A focused link or button already owns Enter; the selection borrows it
    // only when nothing else is answering.
    if (/^(A|BUTTON)$/.test(e.target.tagName)) return;
    const row = listedRows().find((r) => r.dataset.id === selId);
    if (row) {
      e.preventDefault();
      row.click();
    }
  } else if (e.key === "p" && selId) {
    if (pins.has(selId)) pins.delete(selId);
    else pins.add(selId);
    store.write();
    render();
  } else if (e.key === "x" && selId && CAN_ACT) {
    togglePick(selId);
  } else if (e.key === "Escape") {
    // The results card takes Escape first — it is a view over the table, and
    // closing it is the smaller undo. Only then does Escape unmark the row.
    if (findOpen()) {
      closeFind();
    } else if (selId) {
      selId = null;
      for (const row of listedRows()) row.classList.remove("sel");
    }
  }
});

// The header's last control is not in the markup: the button comes from the
// shared theme script inlined above, so a page served without it has one
// fewer button rather than an error. It lands after the totals — #quota
// claims a whole flex line, so this closes the first.
placeThemeButton(document.getElementById("quota"));

// The hint names only keys that do something on this page: a read-only serve
// never draws the bulk bar, so it never mentions the mark key either.
document.getElementById("keys").textContent =
  "j/k select · Enter opens · p pins · / filters" +
  (CAN_ACT ? " · x marks" : "") +
  " · Enter in the filter searches transcripts";

// Ages are relative and nothing else changes between snapshots, so a slow tick
// keeps "4m ago" honest without waiting on the next refresh.
setInterval(() => { if (sessions.length) render(); }, 15000);


connect();
