import { config } from "./config";

// A cost. Missing is not free: no price recorded is a dash, never "$0".
export function money(v: unknown): string {
  if (v === null || v === undefined || v === "") return "—";
  const n = Number(v);
  if (!isFinite(n)) return "—";
  if (n === 0) return "$0";
  if (n < 0.01) return "<$0.01";
  return "$" + (n < 100 ? n.toFixed(2) : Math.round(n).toLocaleString());
}

/**
 * A cell's cost, from a cost record. `incl`, `—` and a figure are three
 * different claims — the plan bundles this, nothing billable was recorded,
 * here is what it cost — and none may be drawn as another. The figure goes
 * through `money`, so a sub-cent spend reads `<$0.01` wherever it is shown.
 */
export function costLabel(cost: { total?: number | string | null; included?: boolean; available?: boolean } | null | undefined): string {
  if (!cost) return "—";
  if (cost.included) return "incl";
  if (cost.available === false) return "—";
  return money(cost.total);
}

// `tokens`, `age`, `bytes`, `shortModel` and `compact` are `cctop_core::util`'s
// (`compact_tokens`, `relative_age`, `compact_bytes`, `short_model`,
// `compact_duration`), ported line for line so the page and the TUI print the
// same string for the same figure.

/** `compact_tokens`: `999`, `1.5K`, `2.4M`, `5.6G`. */
export function tokens(v: unknown): string {
  const n = Math.max(0, Math.floor(Number(v) || 0));
  if (n >= 1e9) return (n / 1e9).toFixed(1) + "G";
  if (n >= 1e6) return (n / 1e6).toFixed(1) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + "K";
  return String(n);
}

/**
 * `relative_age`, htop-style: `now`, `5m`, `3h`, `2d`, `4w`, `7mo`, `1y`.
 * Empty where the TUI says `n/a`: on a page an unknown age is better left out
 * than spelled. `now` is the caller's — `useTick`'s — because reading the clock
 * during render makes two renders of one state draw different pages.
 */
export function age(iso: string | null | undefined, now: number): string {
  const then = Date.parse(iso ?? "");
  if (!isFinite(then)) return "";
  const s = Math.max(0, Math.floor(now / 1000) - Math.floor(then / 1000));
  if (s < 60) return "now";
  if (s < 3600) return Math.floor(s / 60) + "m";
  if (s < 86400) return Math.floor(s / 3600) + "h";
  if (s < 604800) return Math.floor(s / 86400) + "d";
  if (s < 2592000) return Math.floor(s / 604800) + "w";
  if (s < 31536000) return Math.floor(s / 2592000) + "mo";
  return Math.floor(s / 31536000) + "y";
}

/** `age` as prose: `5m ago`, `just now`. */
export function ago(iso: string | null | undefined, now: number): string {
  const a = age(iso, now);
  return !a ? "" : a === "now" ? "just now" : a + " ago";
}

/** A clock time, with the date when it is asked for. */
export function clock(iso: string | null | undefined, withDate = false): string {
  const at = new Date(iso ?? "");
  if (isNaN(at.getTime())) return "";
  const hm = String(at.getHours()).padStart(2, "0") + ":" + String(at.getMinutes()).padStart(2, "0");
  if (!withDate) return hm;
  return at.toLocaleDateString(undefined, { month: "short", day: "numeric" }) + " " + hm;
}

export function secs(ms: unknown): string {
  const n = Number(ms) || 0;
  if (n >= 60000) return Math.round(n / 60000) + "m";
  if (n >= 1000) return (n / 1000).toFixed(1) + "s";
  return n + "ms";
}

/**
 * `compact_bytes`: `512B`, `40K`, `1.5M`, `2.1G`. The TUI leaves zero blank in
 * its column; a page saying a file is there owes it a size, so zero is `0B`.
 */
export function bytes(n: unknown): string {
  const v = Math.max(0, Math.floor(Number(n) || 0));
  const K = 1024;
  if (v >= K * K * K) return (v / (K * K * K)).toFixed(1) + "G";
  if (v >= K * K) return (v / (K * K)).toFixed(1) + "M";
  if (v >= K) return (v / K).toFixed(0) + "K";
  return v + "B";
}

export function shortPath(path: string | null | undefined): string {
  if (!path) return "";
  const home = config.home;
  if (home && path.startsWith(home + "/")) return "~" + path.slice(home.length);
  if (home && path === home) return "~";
  const parts = path.split("/").filter(Boolean);
  return parts.length <= 2 ? path : "…/" + parts.slice(-2).join("/");
}

/**
 * `short_model`: the route's leaf, without `claude-` or a `-YYYYMMDD` release
 * stamp — `anthropic/claude-opus-4-5-20251101` is `opus-4-5`. `gpt-` stays:
 * without it a model is a bare version number.
 */
export function shortModel(model: string | null | undefined): string {
  if (!model) return "";
  const leaf = model.slice(model.lastIndexOf("/") + 1) || model;
  const m = leaf.startsWith("claude-") ? leaf.slice("claude-".length) : leaf;
  const stamped = /^(.+)-\d{8}$/.exec(m);
  return stamped ? stamped[1] : m;
}

/** `compact_duration`: `45s`, `3m12s`, `2h05m`. */
export function compact(secs: number): string {
  const s = Math.round(secs);
  if (s <= 0) return "—";
  if (s < 60) return s + "s";
  const two = (n: number) => String(n).padStart(2, "0");
  if (s < 3600) return Math.floor(s / 60) + "m" + two(s % 60) + "s";
  return Math.floor(s / 3600) + "h" + two(Math.floor((s % 3600) / 60)) + "m";
}

/** "since 13:55 (1h20m)" for a Unix time, in the browser's local time. */
export function since(startedAt: number, now: number): string {
  const at = new Date(startedAt * 1000);
  const hm = isNaN(at.getTime()) ? "?" : String(at.getHours()).padStart(2, "0") + ":" + String(at.getMinutes()).padStart(2, "0");
  const elapsed = now / 1000 - startedAt;
  return elapsed > 0 ? `since ${hm} (${compact(elapsed)})` : `since ${hm}`;
}

// navigator.clipboard only answers in a secure context, and cctop is usually
// plain http on localhost or a LAN address — so the textarea detour stays.
export async function copyText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      /* refused — try the detour */
    }
  }
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.style.cssText = "position:fixed;top:0;left:0;opacity:0";
  document.body.appendChild(ta);
  ta.select();
  try {
    if (!document.execCommand("copy")) throw new Error("the browser refused");
  } finally {
    ta.remove();
  }
}
