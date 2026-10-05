"use strict";
// workspace.html — every agent's terminal on one screen, live.
//
// The first page written with Preact (vendored beside this file, see
// vendor/SOURCE.md) rather than by rebuilding the DOM on each refresh, because
// it is the first page whose DOM must not be rebuilt: each tile holds an
// iframe with a socket to an agent, and the header above it changes every few
// seconds. Preact diffs the header and leaves the frame alone.
//
// One thing a diffing renderer would still get wrong on its own: moving an
// iframe in the DOM reloads it — a new socket, a blank screen, the agent's
// scrollback gone. So tiles are rendered in the order they were opened, which
// only ever appends, and the order the reader sees is CSS `order` on a grid.
// Dragging a tile, maximising one, closing a neighbour: none of them moves a
// frame. Keep it that way; a `.map()` over the *display* order would look right
// and reload every terminal on each drag.

const { h, render } = preact;
const { useState, useEffect, useRef, useMemo, useCallback } = preactHooks;
const html = htm.bind(h);

// --- what the reader chose, kept across reloads ------------------------------

const LAYOUT_KEY = "cctop-workspace";
const loadLayout = () => {
  try {
    const v = JSON.parse(localStorage.getItem(LAYOUT_KEY) || "{}");
    return {
      order: Array.isArray(v.order) ? v.order.filter((n) => typeof n === "string") : [],
      cols: ["auto", 1, 2, 3, 4].includes(v.cols) ? v.cols : "auto",
    };
  } catch (e) {
    return { order: [], cols: "auto" };
  }
};
const saveLayout = (layout) => {
  try { localStorage.setItem(LAYOUT_KEY, JSON.stringify(layout)); } catch (e) {}
};

// Columns for `n` tiles when the reader has not picked: as square a grid as
// the count allows, since a terminal is about as wide as it is useful tall.
const autoCols = (n) => (n <= 1 ? 1 : n <= 4 ? 2 : n <= 9 ? 3 : 4);

const stateClass = (t) =>
  !t ? "gone" : t.state === "needs-input" ? "asking" : t.state === "working" ? "working" : "idle";
const stateWords = { asking: "needs you", working: "working", idle: "idle", gone: "closed" };

// --- the server --------------------------------------------------------------

// The tab bar, every few seconds. An older cctop has no route: that is the
// same as no tabs, said once rather than as an error on every poll.
function useTabs() {
  const [tabs, setTabs] = useState(null);
  const [problemText, setProblem] = useState("");
  useEffect(() => {
    let live = true;
    const poll = async () => {
      try {
        const body = await ask("/api/tabs" + QUERY).then(asJson);
        if (!live) return;
        setTabs(body.tabs || []);
        setProblem("");
      } catch (e) {
        if (live) setProblem(String(e.message || e));
      }
    };
    poll();
    const timer = setInterval(poll, 3000);
    return () => { live = false; clearInterval(timer); };
  }, []);
  return [tabs, problemText];
}

// The session table over the same stream the dashboard reads, so a tile can
// say what its agent has cost without a request of its own.
function useSessions() {
  const [byId, setById] = useState(new Map());
  useEffect(() => {
    let source;
    let retry;
    const connect = () => {
      source = new EventSource("/api/events" + QUERY);
      // A named event: `onmessage` only ever sees unnamed ones.
      source.addEventListener("sessions", (event) => {
        try {
          const list = JSON.parse(event.data);
          setById(new Map((Array.isArray(list) ? list : []).map((s) => [s.session_id, s])));
        } catch (e) { /* a bad event is skipped, not fatal */ }
      });
      // A stream that was never a stream (a tunnel's error page) closes for
      // good; reopened on a timer, as the dashboard does.
      source.onerror = () => {
        if (source.readyState === EventSource.CLOSED) retry = setTimeout(connect, 5000);
      };
    };
    connect();
    return () => { clearTimeout(retry); source && source.close(); };
  }, []);
  return byId;
}

// Opening a terminal is an action — it asks rmux for a share — so it happens
// once per tile per page load, not per render. The promise is kept by name so
// a tile that re-renders while its share is on the way does not ask again.
const opening = new Map();
function openTerminal(name) {
  if (!opening.has(name)) {
    opening.set(name, ask("/api/tab/" + encodeURIComponent(name) + "/terminal" + QUERY, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ origin: location.origin }),
    }).then(asJson).catch((e) => {
      // Forgotten on failure so Retry asks again rather than replaying it.
      opening.delete(name);
      throw e;
    }));
  }
  return opening.get(name);
}

// --- one tile ----------------------------------------------------------------

// The frame. Its own component with no props that change, so the header's
// re-renders never reach it — and memoised by Preact's diff on `src` alone.
function Frame({ src, label }) {
  return html`<iframe class="termframe" title=${"Terminal — " + label}
    allow="clipboard-read; clipboard-write" src=${src}></iframe>`;
}

// What the agent is asking, and the two answers cctop can give for it, between
// the tile's header and its terminal. The question is shown because Allow
// without it is approving blind — and the terminal right below has the full
// menu for anything the two buttons do not cover.
function PromptBar({ session }) {
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState(null);
  // A new question is a new prompt: what the last answer said no longer applies.
  useEffect(() => { setSaid(null); }, [session.asking_for]);
  const answerable = ANSWERABLE.has(session.provider);
  const go = async (choice) => {
    setBusy(true);
    try {
      setSaid({ ok: true, text: await answerPrompt(session.session_id, choice) });
    } catch (e) {
      setSaid({ ok: false, text: String(e.message || e) });
    } finally {
      setBusy(false);
    }
  };
  return html`
    <div class="promptbar" role="group" aria-label="Permission prompt">
      <span class="ask trunc" title=${session.asking_for || ""}>
        ${session.asking_for ? html`<code>${session.asking_for}</code>` : "Waiting on a permission prompt"}
      </span>
      ${said && html`<span class=${"said " + (said.ok ? "ok" : "bad")}>${said.text}</span>`}
      ${answerable && CAN_ACT
        ? html`
          <button type="button" class="primary" disabled=${busy} onClick=${() => go("allow")}
            title="Press the first option — allow this once">Allow</button>
          <button type="button" disabled=${busy} onClick=${() => go("deny")}
            title="Press Esc — refuse it">Deny</button>`
        : html`<span class="faint">answer in the terminal below</span>`}
    </div>`;
}

function Tile({ name, tab, session, position, maximized, focused,
                onClose, onMaximize, onFocus, drag }) {
  const [terminal, setTerminal] = useState(null);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const cls = stateClass(tab);
  const label = tab ? tab.label : name.replace(/^cctop-/, "");

  useEffect(() => {
    if (!CAN_ACT || !tab) return;
    let live = true;
    setError("");
    openTerminal(name).then(
      (t) => { if (live) setTerminal(t); },
      (e) => { if (live) setError(String(e.message || e)); },
    );
    return () => { live = false; };
    // `tab` only matters for whether it exists: a closed tab is not reopened,
    // and a header update must not ask for a second share.
  }, [name, attempt, Boolean(tab)]);

  const src = terminal && (() => {
    const at = terminal.url.indexOf("#");
    return "/term/" + QUERY + (at >= 0 ? terminal.url.slice(at) : "");
  })();

  // An open frame outranks everything below it: a tab missing from one poll
  // — rmux busy, a rename mid-flight — must not cost the reader their
  // terminal. The header says "closed"; the frame stays until they remove it.
  let body;
  if (src) {
    body = html`<${Frame} src=${src} label=${label} />`;
  } else if (!CAN_ACT) {
    body = html`<div class="empty">This link is read-only. Terminals open from the full link, which can type into agents.</div>`;
  } else if (!tab) {
    body = html`<div class="empty">This tab has closed — its agent exited or was moved.
      <div><button type="button" onClick=${onClose}>Remove</button></div></div>`;
  } else if (error) {
    body = html`<div class="empty bad">${error}
      <div><button type="button" onClick=${() => setAttempt(attempt + 1)}>Retry</button></div></div>`;
  } else {
    body = html`<div class="empty">Opening the terminal…</div>`;
  }

  const cost = session && session.cost && session.cost.available && !session.cost.included
    ? money(session.cost.total) : null;
  const model = session && session.model ? shortModel(session.model) : null;

  return html`
    <section class=${"tile " + cls + (maximized ? " max" : "") + (focused ? " focused" : "") +
                     (drag.over === name ? " dropping" : "")}
      onDragOver=${(e) => drag.over !== undefined && drag.onOver(e, name)}
      onDrop=${(e) => drag.onDrop(e, name)}
      onPointerDown=${() => onFocus(name)}>
      <header class="tilehead" draggable="true"
        onDragStart=${(e) => drag.onStart(e, name)} onDragEnd=${drag.onEnd}
        onDblClick=${onMaximize} title="Drag to rearrange · double-click to maximise">
        <span class="grip" aria-hidden="true">⠿</span>
        <span class=${"dot " + cls}></span>
        <span class="label">${label}</span>
        ${position < 9 && html`<kbd class="slot" title=${"Alt+" + (position + 1)}>${position + 1}</kbd>`}
        <span class="where trunc">${tab && tab.cwd ? shortPath(tab.cwd) : ""}</span>
        ${(cls === "asking" || cls === "gone") &&
          html`<span class=${"pill " + (cls === "asking" ? "bad" : "")}>${stateWords[cls]}</span>`}
        <span class="spacer"></span>
        ${model && html`<span class="fig model mono">${model}</span>`}
        ${cost && html`<span class="fig mono">${cost}</span>`}
        <span class="acts">
          ${tab && tab.session_id && html`<a class="act" href=${"/session/" + encodeURIComponent(tab.session_id) + QUERY}
            title="The conversation, the changes and the report" aria-label="Open the session page">≡</a>`}
          ${terminal && html`<a class="act" href=${terminal.url} target="_blank" rel="noopener"
            title="Open this terminal in a window of its own" aria-label="Pop out">↗</a>`}
          <button type="button" class="quiet act" onClick=${onMaximize}
            title=${maximized ? "Back to the grid (Esc)" : "Maximise (Alt+Enter)"}
            aria-label=${maximized ? "Restore" : "Maximise"}>${maximized ? "▣" : "□"}</button>
          <button type="button" class="quiet act" onClick=${onClose}
            title="Close this tile — the agent keeps running" aria-label="Close tile">×</button>
        </span>
      </header>
      ${session && session.running && session.state === "asking" && html`<${PromptBar} session=${session} />`}
      <div class="tilebody">${body}</div>
    </section>`;
}

// --- the page ----------------------------------------------------------------

function Workspace() {
  const [tabs, tabsProblem] = useTabs();
  const sessions = useSessions();
  const [layout, setLayout] = useState(loadLayout);
  const [maximized, setMaximized] = useState(null);
  const [focused, setFocused] = useState(null);
  const [dragging, setDragging] = useState(null);
  const [over, setOver] = useState(null);
  // Every tile ever opened this page load, in the order it was opened — the
  // order the frames sit in the DOM, which must never change (see the top).
  const mounted = useRef([]);

  const update = useCallback((next) => {
    setLayout((prev) => {
      const v = { ...prev, ...next };
      saveLayout(v);
      return v;
    });
  }, []);

  const byName = useMemo(() => new Map((tabs || []).map((t) => [t.name, t])), [tabs]);
  const order = layout.order;
  for (const n of order) if (!mounted.current.includes(n)) mounted.current.push(n);
  mounted.current = mounted.current.filter((n) => order.includes(n));

  const add = (name) => !order.includes(name) && update({ order: [...order, name] });
  const close = (name) => {
    update({ order: order.filter((n) => n !== name) });
    if (maximized === name) setMaximized(null);
  };
  const toggleMax = (name) => setMaximized((m) => (m === name ? null : name));
  const openAll = () => update({
    order: [...order, ...(tabs || []).map((t) => t.name).filter((n) => !order.includes(n))],
  });

  // Keyboard, for when the page rather than a terminal has focus — a terminal
  // keeps every key it is given, as it must.
  useEffect(() => {
    const onKey = (e) => {
      if (e.target.closest && e.target.closest("input, textarea, select, dialog")) return;
      if (e.key === "Escape" && maximized) { setMaximized(null); return; }
      if (!e.altKey || e.ctrlKey || e.metaKey) return;
      if (/^[1-9]$/.test(e.key)) {
        const name = order[Number(e.key) - 1];
        if (!name) return;
        e.preventDefault();
        setFocused(name);
        if (maximized) setMaximized(name);
        const frame = document.querySelector(`[data-tile="${CSS.escape(name)}"] iframe`);
        if (frame) frame.focus();
      } else if (e.key === "Enter" && (focused || order[0])) {
        e.preventDefault();
        toggleMax(focused || order[0]);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [order, maximized, focused]);

  // The tab title counts the agents waiting on you, so a workspace left in a
  // background tab still says when it wants attention.
  const asking = (tabs || []).filter((t) => t.state === "needs-input").length;
  useEffect(() => {
    document.title = (asking ? "(" + asking + ") " : "") + "cctop — workspace";
  }, [asking]);

  const drag = {
    over: dragging ? over : undefined,
    onStart: (e, name) => {
      setDragging(name);
      e.dataTransfer.effectAllowed = "move";
      // Some browsers start no drag without data.
      e.dataTransfer.setData("text/plain", name);
    },
    onOver: (e, name) => { e.preventDefault(); if (name !== over) setOver(name); },
    onDrop: (e, target) => {
      e.preventDefault();
      if (dragging && dragging !== target) {
        const next = order.filter((n) => n !== dragging);
        next.splice(next.indexOf(target) + (order.indexOf(dragging) < order.indexOf(target) ? 1 : 0), 0, dragging);
        update({ order: next });
      }
      setDragging(null);
      setOver(null);
    },
    onEnd: () => { setDragging(null); setOver(null); },
  };

  if (tabs === null && !tabsProblem) return html`<div class="empty">Reading the tabs…</div>`;

  const closed = (tabs || []).filter((t) => !order.includes(t.name));
  const cols = layout.cols === "auto" ? autoCols(order.length) : layout.cols;

  return html`
    ${tabsProblem && html`<div class="banner">${tabsProblem}</div>`}
    <div class="wsbar">
      <div class="dock" aria-label="Tabs not on screen">
        ${closed.length
          ? closed.map((t) => html`
              <button type="button" class="tab" key=${t.name} onClick=${() => add(t.name)}
                title=${"Add to the workspace" + (t.cwd ? " · " + shortPath(t.cwd) : "")}>
                <span class=${"dot " + stateClass(t)}></span>
                <span class="name">${t.label}</span><span class="plus" aria-hidden="true">+</span>
              </button>`)
          : html`<span class="faint">${(tabs || []).length ? "Every tab is on screen." : ""}</span>`}
      </div>
      <span class="spacer"></span>
      ${closed.length > 1 && html`<button type="button" onClick=${openAll}>Open all</button>`}
      <div class="seg" role="group" aria-label="Columns">
        ${["auto", 1, 2, 3, 4].map((c) => html`
          <button type="button" key=${c} aria-pressed=${String(layout.cols === c)}
            onClick=${() => update({ cols: c })}
            title=${c === "auto" ? "As square as the count allows" : c + (c === 1 ? " column" : " columns")}>
            ${c === "auto" ? "Auto" : c}</button>`)}
      </div>
    </div>
    ${order.length === 0
      ? html`<div class="card empty">
          ${(tabs || []).length
            ? html`Pick a tab above to put its terminal here, or <button type="button" class="primary" onClick=${openAll}>open all ${tabs.length}</button>`
            : html`No agent is open in cctop's multiplexer. Start one from the dashboard or the TUI and it appears here.`}
        </div>`
      : html`<div class=${"grid" + (dragging ? " dragging" : "") + (maximized ? " maxed" : "")}
          style=${{ "--cols": cols }}>
          ${mounted.current.map((name) => html`
            <div key=${name} data-tile=${name} class="slotwrap" style=${{ order: order.indexOf(name) }}
              hidden=${Boolean(maximized && maximized !== name)}>
              <${Tile} name=${name} tab=${byName.get(name)}
                session=${byName.get(name) && sessions.get(byName.get(name).session_id)}
                position=${order.indexOf(name)} maximized=${maximized === name}
                focused=${focused === name} drag=${drag}
                onClose=${() => close(name)} onMaximize=${() => toggleMax(name)} onFocus=${setFocused} />
            </div>`)}
        </div>`}
    <p class="hint faint">Alt+1–9 jumps to a tile · Alt+Enter maximises · Esc restores · drag a header to rearrange.
      Closing a tile leaves its agent running.</p>`;
}

placeThemeButton();
render(html`<${Workspace} />`, document.getElementById("app"));
