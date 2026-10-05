import { Route, Switch } from "wouter";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ThemeProvider } from "@/components/theme";
import { AppShell } from "@/components/app-shell";
import { SessionPage } from "@/pages/session";
import { WorkspacePage } from "@/pages/workspace";
import { DashboardPage } from "@/pages/dashboard";
import { AnalyticsPage } from "@/pages/analytics";

// Four routes, so a 2 KB router rather than a framework's: everything here is
// inlined into one page and paid for on every load.
export default function App() {
  return (
    <ThemeProvider>
      <TooltipProvider delayDuration={300}>
        <Switch>
          <Route path="/" component={DashboardPage} />
          <Route path="/analytics" component={AnalyticsPage} />
          <Route path="/session/:id" component={SessionPage} />
          <Route path="/workspace" component={WorkspacePage} />
          <Route>
            <AppShell>
              <div className="text-muted-foreground m-auto text-sm">No such page.</div>
            </AppShell>
          </Route>
        </Switch>
        <Toaster position="bottom-right" />
      </TooltipProvider>
    </ThemeProvider>
  );
}
