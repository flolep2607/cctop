import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { Monitor, Moon, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

// Light, dark, or the system's — stored under the same key every cctop page
// reads, so a choice made here holds on the pages not yet moved to this app.
// "system" is the key being absent.
type Theme = "light" | "dark" | "system";
const KEY = "cctop-theme";

const ThemeContext = createContext<{ theme: Theme; setTheme: (t: Theme) => void; cycle: () => void }>({
  theme: "system",
  setTheme: () => {},
  cycle: () => {},
});

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(() => {
    try {
      const v = localStorage.getItem(KEY);
      return v === "light" || v === "dark" ? v : "system";
    } catch {
      return "system";
    }
  });
  useEffect(() => {
    const media = matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && media.matches);
      document.documentElement.classList.toggle("dark", dark);
      // The old pages read data-theme; keep them in step until they are gone.
      if (theme === "system") delete document.documentElement.dataset.theme;
      else document.documentElement.dataset.theme = theme;
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
  const setTheme = (t: Theme) => {
    setThemeState(t);
    try {
      if (t === "system") localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, t);
    } catch {
      /* best effort */
    }
  };
  const cycle = () => setTheme(theme === "light" ? "dark" : theme === "dark" ? "system" : "light");
  return <ThemeContext.Provider value={{ theme, setTheme, cycle }}>{children}</ThemeContext.Provider>;
}

export const useTheme = () => useContext(ThemeContext);

export function ThemeToggle() {
  const { theme, cycle } = useTheme();
  const Icon = theme === "light" ? Sun : theme === "dark" ? Moon : Monitor;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="ghost" size="icon" onClick={cycle} aria-label={"Theme: " + theme}>
          <Icon />
        </Button>
      </TooltipTrigger>
      <TooltipContent>Theme: {theme}</TooltipContent>
    </Tooltip>
  );
}
