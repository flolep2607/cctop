import { BarChart3, LayoutGrid, List, type LucideIcon } from "lucide-react";
import { withToken } from "@/lib/config";

// The pages this app renders. Anything else (the plain-text insight reports)
// is a full load, with the token, since the router cannot draw it.
export const isSpa = (path: string) => path === "/" || path === "/analytics" || path === "/workspace" || path.startsWith("/session/");

export const PAGES: { path: string; label: string; hint: string; icon: LucideIcon }[] = [
  { path: "/", label: "Sessions", hint: "the live table", icon: List },
  { path: "/workspace", label: "Workspace", hint: "every terminal, tiled", icon: LayoutGrid },
  { path: "/analytics", label: "Analytics", hint: "spend, tokens and activity over time", icon: BarChart3 },
];

export function go(navigate: (to: string) => void, path: string) {
  if (isSpa(path.split(/[?#]/)[0])) navigate(path);
  else location.href = withToken(path);
}
