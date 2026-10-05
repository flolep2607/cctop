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

export function tokens(v: unknown): string {
  const n = Number(v) || 0;
  if (n >= 1e9) return (n / 1e9).toFixed(2) + "G";
  if (n >= 1e6) return (n / 1e6).toFixed(1) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + "k";
  return String(Math.round(n));
}

export function ago(iso: string | null | undefined): string {
  const then = Date.parse(iso ?? "");
  if (!isFinite(then)) return "";
  const s = Math.max(0, (Date.now() - then) / 1000);
  if (s < 60) return Math.floor(s) + "s ago";
  if (s < 3600) return Math.floor(s / 60) + "m ago";
  if (s < 86400) return Math.floor(s / 3600) + "h ago";
  return Math.floor(s / 86400) + "d ago";
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

export function bytes(n: unknown): string {
  const v = Number(n) || 0;
  if (v >= 1048576) return (v / 1048576).toFixed(1) + " MB";
  if (v >= 1024) return Math.round(v / 1024) + " KB";
  return v + " B";
}

export function shortPath(path: string | null | undefined): string {
  if (!path) return "";
  const home = config.home;
  if (home && path.startsWith(home + "/")) return "~" + path.slice(home.length);
  if (home && path === home) return "~";
  const parts = path.split("/").filter(Boolean);
  return parts.length <= 2 ? path : "…/" + parts.slice(-2).join("/");
}

/** `anthropic/claude-opus-5` and `claude-opus-5` are one model to a reader. */
export function shortModel(model: string | null | undefined): string {
  if (!model) return "";
  const parts = model.split("/").filter(Boolean);
  return parts.length ? parts[parts.length - 1] : model;
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
