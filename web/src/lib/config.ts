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
