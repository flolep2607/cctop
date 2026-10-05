import { BarChart3, LayoutGrid, List, type LucideIcon } from "lucide-react";
import { withToken } from "@/lib/config";

// The pages, and which of them this app renders. A page still served by the
// old assets is reached with a full load — the router cannot draw it.
export const SPA_PREFIXES = ["/session/", "/workspace"];
export const isSpa = (path: string) => SPA_PREFIXES.some((p) => path === p.replace(/\/$/, "") || path.startsWith(p));

export const PAGES: { path: string; label: string; hint: string; icon: LucideIcon }[] = [
  { path: "/", label: "Sessions", hint: "the live table", icon: List },
  { path: "/workspace", label: "Workspace", hint: "every terminal, tiled", icon: LayoutGrid },
  { path: "/analytics", label: "Analytics", hint: "spend, tokens and activity over time", icon: BarChart3 },
];

export function go(navigate: (to: string) => void, path: string) {
  if (isSpa(path.split(/[?#]/)[0])) navigate(path);
  else location.href = withToken(path);
}
