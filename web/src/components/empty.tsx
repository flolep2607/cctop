import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Panel } from "./section";

/**
 * What stands where content would be: nothing yet, nothing matches, gone.
 *
 * Two surfaces. On a card it is a quiet panel of its own. On a terminal — a
 * workspace tile — it sits on the terminal's black, which is black in both
 * themes, so it takes the terminal's own text colours rather than the
 * theme's, whose muted grey is drawn for a light page.
 */
export function Empty({ children, on = "card", bad, className }: { children: ReactNode; on?: "card" | "terminal"; bad?: boolean; className?: string }) {
  if (on === "terminal")
    return (
      <div className={cn("m-auto grid max-w-sm justify-items-center gap-3 p-6 text-center text-sm", bad ? "text-terminal-error" : "text-terminal-muted", className)}>
        {children}
      </div>
    );
  return <Panel className={cn("p-8 text-center text-sm", bad ? "text-destructive" : "text-muted-foreground", className)}>{children}</Panel>;
}
