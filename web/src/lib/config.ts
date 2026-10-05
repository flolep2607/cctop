// What the server substituted into the page (see index.html). Read once.
export interface Config {
  token: string;
  /** Whether this run serves the routes that act on a session. */
  actions: boolean;
  /** The operator's home directory, so paths read `~/…`. */
  home: string;
  version: string;
}

function read(): Config {
  const fallback: Config = { token: "", actions: false, home: "", version: "dev" };
  try {
    const raw = document.getElementById("cctop-config")?.textContent ?? "";
    return { ...fallback, ...JSON.parse(raw) };
  } catch {
    return fallback;
  }
}

export const config = read();
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

// The token reaches the script inside the page, so once it is running the
// `?t=` in the address bar is only a credential in history and screenshots.
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
