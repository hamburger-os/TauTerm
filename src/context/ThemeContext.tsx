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

/** 字体大小范围常量 */
const FONT_SIZE_MIN = 8;
const FONT_SIZE_MAX = 32;
const FONT_SIZE_DEFAULT = 14;

/** 行缓冲范围常量 */
const BUFFER_LINES_MIN = 1000;
const BUFFER_LINES_MAX = 100000;
const BUFFER_LINES_DEFAULT = 10000;

/** 从 localStorage 读取数值，带校验和默认值回退 */
function readStoredNumber(key: string, min: number, max: number, fallback: number): number {
  const raw = localStorage.getItem(key);
  if (raw) {
    const n = Number(raw);
    if (Number.isFinite(n) && n >= min && n <= max) return n;
  }
  return fallback;
}

interface ThemeContextValue {
  theme: ThemeId;
  themes: ThemeInfo[];
  setTheme: (theme: ThemeId) => void;
  /** 终端字体大小 (px)，范围 8–32，默认 14 */
  fontSize: number;
  setFontSize: (n: number) => void;
  /** 终端行缓冲上限（所有模式统一），范围 1,000–100,000，默认 10,000 */
  bufferLines: number;
  setBufferLines: (n: number) => void;
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

  const [fontSize, setFontSizeState] = useState<number>(() =>
    readStoredNumber("tauterm-font-size", FONT_SIZE_MIN, FONT_SIZE_MAX, FONT_SIZE_DEFAULT),
  );

  const [bufferLines, setBufferLinesState] = useState<number>(() =>
    readStoredNumber("tauterm-buffer-lines", BUFFER_LINES_MIN, BUFFER_LINES_MAX, BUFFER_LINES_DEFAULT),
  );

  const setTheme = useCallback((newTheme: ThemeId) => {
    setThemeState(newTheme);
    localStorage.setItem("tauterm-theme", newTheme);
    document.documentElement.dataset.theme = newTheme;
  }, []);

  const setFontSize = useCallback((n: number) => {
    if (!Number.isFinite(n) || n < FONT_SIZE_MIN || n > FONT_SIZE_MAX) return;
    setFontSizeState(n);
    localStorage.setItem("tauterm-font-size", String(n));
  }, []);

  const setBufferLines = useCallback((n: number) => {
    if (!Number.isFinite(n) || n < BUFFER_LINES_MIN || n > BUFFER_LINES_MAX) return;
    setBufferLinesState(n);
    localStorage.setItem("tauterm-buffer-lines", String(n));
  }, []);

  // Apply theme on mount and changes
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  // 跨标签页同步：监听其他标签页的 localStorage 变更
  useEffect(() => {
    const handler = (e: StorageEvent) => {
      if (e.key === "tauterm-font-size" && e.newValue) {
        const n = Number(e.newValue);
        if (Number.isFinite(n) && n >= FONT_SIZE_MIN && n <= FONT_SIZE_MAX) {
          setFontSizeState(n);
        }
      }
      if (e.key === "tauterm-buffer-lines" && e.newValue) {
        const n = Number(e.newValue);
        if (Number.isFinite(n) && n >= BUFFER_LINES_MIN && n <= BUFFER_LINES_MAX) {
          setBufferLinesState(n);
        }
      }
      if (e.key === "tauterm-theme" && e.newValue) {
        const v = e.newValue as ThemeId;
        if (v === "google-glow" || v === "obsidian" || v === "frosted") {
          setThemeState(v);
        }
      }
    };
    window.addEventListener("storage", handler);
    return () => window.removeEventListener("storage", handler);
  }, []);

  return (
    <ThemeContext.Provider value={{ theme, themes: THEMES, setTheme, fontSize, setFontSize, bufferLines, setBufferLines }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme() {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used within ThemeProvider");
  return ctx;
}
