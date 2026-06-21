import { createContext, useContext, useState, useCallback, useEffect, type ReactNode } from "react";

export type ThemeId = "google-glow" | "obsidian" | "frosted";

interface ThemeInfo {
  id: ThemeId;
  name: string;
  nameEn: string;
}

export const THEMES: ThemeInfo[] = [
  { id: "google-glow", name: "炫彩流光", nameEn: "Google Glow" },
  { id: "obsidian", name: "黑曜石", nameEn: "Obsidian" },
  { id: "frosted", name: "白霜", nameEn: "Frosted" },
];

interface ThemeContextValue {
  theme: ThemeId;
  themes: ThemeInfo[];
  setTheme: (theme: ThemeId) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<ThemeId>(() => {
    const stored = localStorage.getItem("tauterm-theme");
    // 迁移旧主题 ID 到新主题
    if (stored === "neon-dark" || stored === "ocean" || stored === "sunset") {
      return "google-glow";
    }
    if (stored === "google-glow" || stored === "obsidian" || stored === "frosted") {
      return stored;
    }
    return "google-glow";
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
