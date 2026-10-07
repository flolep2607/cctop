import { createContext, useContext } from "react";

// Light, dark, or the system's — stored under the same key every cctop page
// reads, so a choice made here holds on the pages not yet moved to this app.
// "system" is the key being absent.
export type Theme = "light" | "dark" | "system";
export const THEME_KEY = "cctop-theme";

export const ThemeContext = createContext<{ theme: Theme; setTheme: (t: Theme) => void; cycle: () => void }>({
  theme: "system",
  setTheme: () => {},
  cycle: () => {},
});

/** The theme in force, and the setters; `ThemeProvider` in components/theme.tsx supplies it. */
export const useTheme = () => useContext(ThemeContext);
