import { useState, type ReactNode } from "react";
import { Link, useLocation } from "wouter";
import { Eye, Search } from "lucide-react";
import { cn } from "@/lib/utils";
import { CAN_ACT, config } from "@/lib/config";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Badge } from "@/components/ui/badge";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { CommandPalette } from "./command-palette";
import { DashboardAddress } from "./address";
import { ThemeToggle } from "./theme";
import { go, PAGES } from "./nav";

const mac = /Mac|iPhone|iPad/.test(navigator.platform || "");

// The frame every page sits in: one row of header — name, pages, the palette,
// the theme — and the page below it filling the rest of the window. Pages
// decide for themselves what scrolls; the header never does.
export function AppShell({ children, right }: { children: ReactNode; right?: ReactNode }) {
  const [open, setOpen] = useState(false);
  const [pathname, navigate] = useLocation();
  const current = (p: string) => (p === "/" ? pathname === "/" : pathname.startsWith(p));
  return (
    <div className="flex h-dvh flex-col">
      <header className="bg-background/80 supports-backdrop-filter:backdrop-blur flex h-12 shrink-0 items-center gap-1 border-b px-3 sm:gap-3 sm:px-4">
        <Link
          href="/"
          onClick={(e) => {
            e.preventDefault();
            go(navigate, "/");
          }}
          className="flex items-baseline gap-1.5 pr-1 font-semibold tracking-tight"
        >
          cctop
          <span className="text-muted-foreground hidden font-mono text-[11px] font-normal sm:inline">{config.version}</span>
        </Link>
        <nav className="flex items-center gap-0.5" aria-label="Pages">
          {PAGES.map((p) => (
            <a
              key={p.path}
              href={p.path}
              onClick={(e) => {
                e.preventDefault();
                go(navigate, p.path);
              }}
              aria-current={current(p.path) ? "page" : undefined}
              className={cn(
                "text-muted-foreground hover:text-foreground hover:bg-muted rounded-md px-2 py-1 text-sm transition-colors",
                current(p.path) && "bg-muted text-foreground font-medium",
              )}
            >
              <p.icon className="mr-1 inline size-3.5 align-[-2px] max-sm:hidden" />
              {p.label}
            </a>
          ))}
        </nav>
        <div className="flex-1" />
        {right}
        {/* The full link only: the read-only page has no address to choose. */}
        {CAN_ACT && <DashboardAddress />}
        {/* Said once, in the header, rather than discovered per button: the
            read-only link is the second one `cctop serve` prints and the
            easier one to click by mistake. */}
        {!CAN_ACT && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Badge variant="outline" className="text-warning border-warning/40 cursor-help gap-1 font-normal">
                <Eye className="size-3" />
                View only
              </Badge>
            </TooltipTrigger>
            <TooltipContent className="max-w-72 text-xs leading-relaxed">
              This link can watch but not act: no answers, terminals or launches. To act, open the first link <code>cctop serve</code> printed — the
              “serving on” one, not the “read-only link”.
            </TooltipContent>
          </Tooltip>
        )}
        <Button variant="outline" size="sm" className="text-muted-foreground gap-2 max-sm:hidden" onClick={() => setOpen(true)}>
          <Search />
          Go to…
          <Kbd>{mac ? "⌘K" : "Ctrl K"}</Kbd>
        </Button>
        <Button variant="ghost" size="icon" className="sm:hidden" onClick={() => setOpen(true)} aria-label="Go to">
          <Search />
        </Button>
        <ThemeToggle />
      </header>
      <main className="flex min-h-0 flex-1 flex-col">{children}</main>
      <CommandPalette open={open} setOpen={setOpen} />
    </div>
  );
}
