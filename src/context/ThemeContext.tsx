import { createContext, useContext, useState, useCallback, useEffect, type ReactNode } from "react";

export type ThemeId = "neon-dark" | "ocean" | "sunset";

interface ThemeInfo {
  id: ThemeId;
  name: string;
  nameEn: string;
}

export const THEMES: ThemeInfo[] = [
  { id: "neon-dark", name: "霓虹暗黑", nameEn: "Neon Dark" },
  { id: "ocean", name: "深海蓝", nameEn: "Ocean Blue" },
  { id: "sunset", name: "日落琥珀", nameEn: "Sunset Amber" },
];

interface ThemeContextValue {
  theme: ThemeId;
  themes: ThemeInfo[];
  setTheme: (theme: ThemeId) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<ThemeId>(() => {
    return (localStorage.getItem("tauterm-theme") as ThemeId) || "neon-dark";
  });

  const setTheme = useCallback((newTheme: ThemeId) => {
    setThemeState(newTheme);
    localStorage.setItem("tauterm-theme", newTheme);
    document.documentElement.dataset.theme = newTheme;
  }, []);

  // Apply theme on mount and changes
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  return (
    <ThemeContext.Provider value={{ theme, themes: THEMES, setTheme }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme() {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used within ThemeProvider");
  return ctx;
}
