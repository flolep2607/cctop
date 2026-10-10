// What the server knows and the page cannot: fetched from `/api/config` by the
// script at the top of index.html, which starts it before this one is parsed.
export interface Config {
  token: string;
  /** Whether this run serves the routes that act on a session. */
  actions: boolean;
  /** The operator's home directory, so paths read `~/…`. */
  home: string;
  version: string;
}

declare global {
  interface Window {
    cctopConfig?: Promise<Partial<Config>>;
  }
}

async function read(): Promise<Config> {
  const fallback: Config = { token: "", actions: false, home: "", version: "dev" };
  try {
    return { ...fallback, ...(await window.cctopConfig) };
  } catch {
    return fallback;
  }
}

// The script in index.html has already asked for the config with the token,
// so from here the `?t=` in the address bar is only a credential in history
// and screenshots — and it goes before the wait for the answer, not after.
// Captured first: a page that wants something else from the query (`?find=`)
// reads it from here, not from a URL that has already been rewritten.
//
// This is the client's half of a decision that was looked at properly and made
// this way. The server could answer `/` with a 303 to the same path without
// `?t=`, having set the cookie on the way out, and the address bar would be
// clean before any script ran. It does not, because the read-only link has to
// beat a full cookie the browser is already holding: a 303 drops the `?t=`,
// the follow-up request arrives with no token in the query, and the cookie is
// then the only thing that decides — so opening a read-only link in a browser
// holding the full cookie would quietly hand over the full credential. That
// ordering is the whole point of the read-only link (see `app_page` in
// `crates/serve/src/lib.rs`), and the try/catch below costs nothing when it
// fails: the worst case is the URL it opened with, which is what the page was
// going to show anyway.
export const OPENING_QUERY = new URLSearchParams(location.search);
try {
  const here = new URL(location.href);
  if (here.searchParams.has("t")) {
    here.searchParams.delete("t");
    history.replaceState(history.state, "", here.pathname + here.search + here.hash);
  }
} catch {
  /* a context that forbids it keeps the URL it opened with */
}

// Awaited here, at the top level, so every module that imports these reads
// them as the plain values they always were: nothing renders before it lands.
export const config = await read();
export const TOKEN = config.token;
export const QUERY = TOKEN ? "?t=" + encodeURIComponent(TOKEN) : "";
export const CAN_ACT = config.actions;

/** A URL with the token appended — every request, link and frame needs it. */
export function withToken(path: string, extra?: Record<string, string | number | null | undefined>): string {
  const params = new URLSearchParams();
  if (TOKEN) params.set("t", TOKEN);
  for (const [k, v] of Object.entries(extra ?? {})) if (v !== null && v !== undefined) params.set(k, String(v));
  const q = params.toString();
  const hash = path.indexOf("#");
  const [base, frag] = hash >= 0 ? [path.slice(0, hash), path.slice(hash)] : [path, ""];
  return base + (q ? (base.includes("?") ? "&" : "?") + q : "") + frag;
}
