import { createContext, useContext, useState, useCallback, useEffect, type ReactNode } from "react";

export type ThemeId = "spectrum-flow" | "obsidian" | "frosted";
export type VisualPerformanceMode = "quality" | "performance";

interface ThemeInfo {
  id: ThemeId;
  name: string;
  nameEn: string;
}

export const THEMES: ThemeInfo[] = [
  { id: "spectrum-flow", name: "炫彩流光", nameEn: "Spectrum Flow" },
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

const PERFORMANCE_MODE_DEFAULT: VisualPerformanceMode = "quality";

function isPerformanceMode(value: string | null): value is VisualPerformanceMode {
  return value === "quality" || value === "performance";
}

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
  /** 视觉性能档：quality / performance，默认 quality */
  performanceMode: VisualPerformanceMode;
  setPerformanceMode: (mode: VisualPerformanceMode) => void;
  /** 系统是否请求减少动态效果 */
  systemReducedMotion: boolean;
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
    return stored === "spectrum-flow" || stored === "obsidian" || stored === "frosted"
      ? stored
      : "spectrum-flow";
  });

  const [performanceMode, setPerformanceModeState] = useState<VisualPerformanceMode>(() => {
    const stored = localStorage.getItem("tauterm-performance-mode");
    return isPerformanceMode(stored) ? stored : PERFORMANCE_MODE_DEFAULT;
  });

  const [systemReducedMotion, setSystemReducedMotion] = useState<boolean>(() =>
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );

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

  const setPerformanceMode = useCallback((mode: VisualPerformanceMode) => {
    setPerformanceModeState(mode);
    localStorage.setItem("tauterm-performance-mode", mode);
    document.documentElement.dataset.performance = mode;
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

  // Apply theme / performance tier on mount and changes.
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    document.documentElement.dataset.performance = performanceMode;
  }, [performanceMode]);

  // Decorative ambient motion pauses only when the document is actually hidden.
  // Losing focus (screenshot tools, task switcher, another window) must not reset/freeze
  // the visual field; semantic loading/status animations are never controlled here.
  useEffect(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");

    const applyMotionState = () => {
      setSystemReducedMotion(media.matches);
      const state = document.hidden
        ? "paused"
        : media.matches
          ? "reduced"
          : "full";
      document.documentElement.dataset.motion = state;
    };

    applyMotionState();
    document.addEventListener("visibilitychange", applyMotionState);

    if (typeof media.addEventListener === "function") {
      media.addEventListener("change", applyMotionState);
    } else {
      media.addListener(applyMotionState);
    }

    return () => {
      document.removeEventListener("visibilitychange", applyMotionState);
      if (typeof media.removeEventListener === "function") {
        media.removeEventListener("change", applyMotionState);
      } else {
        media.removeListener(applyMotionState);
      }
    };
  }, []);

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
        if (v === "spectrum-flow" || v === "obsidian" || v === "frosted") {
          setThemeState(v);
        }
      }
      if (e.key === "tauterm-performance-mode" && isPerformanceMode(e.newValue)) {
        setPerformanceModeState(e.newValue);
      }
    };
    window.addEventListener("storage", handler);
    return () => window.removeEventListener("storage", handler);
  }, []);

  return (
    <ThemeContext.Provider
      value={{
        theme,
        themes: THEMES,
        setTheme,
        performanceMode,
        setPerformanceMode,
        systemReducedMotion,
        fontSize,
        setFontSize,
        bufferLines,
        setBufferLines,
      }}
    >
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme() {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used within ThemeProvider");
  return ctx;
}
