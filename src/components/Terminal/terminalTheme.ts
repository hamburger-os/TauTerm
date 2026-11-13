import type { ITheme, ITerminalOptions } from "@xterm/xterm";

/**
 * The single owner of terminal *skin*: theme palette and xterm construction options.
 *
 * Every terminal surface in TauTerm (PTY sessions, Serial, Network Debug, RTT) renders
 * through `TerminalSurface`, which is the only place allowed to instantiate xterm.
 * Theme colors and xterm options therefore live here instead of being re-declared by
 * each consumer — a second hand-written theme silently drifts from the four-color
 * spectrum, and a second hand-written background silently stops being transparent.
 *
 * `npm run check:terminal-surface` enforces both invariants.
 */

export type TerminalThemeMode = "spectrum-flow" | "obsidian" | "frosted";

/**
 * xterm only parses `#rgb[a]`, `#rrggbb[aa]`, `rgb()` and `rgba()`; its canvas
 * fallback explicitly rejects any color with alpha !== 255. The keyword
 * `transparent` therefore does not mean "see through": `ThemeService` treats it as
 * unparseable and falls back to opaque `#000000`, which used to paint RTT's viewer
 * as a black block. Transparency must always be alpha-0 rgba.
 */
export const TERMINAL_SURFACE_BACKGROUND = "rgba(0, 0, 0, 0)";

/** Shared font stack: a cell-grid change must not differ between two surfaces. */
export const TERMINAL_FONT_FAMILY =
  '"JetBrains Mono", "Cascadia Code", "Fira Code", "Consolas", "Courier New", monospace';

/** Reads a canonical theme token from the document root; throwing keeps drift loud. */
export type TokenReader = (token: string) => string;

export function readTerminalCssToken(token: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(token).trim();
  if (!value) throw new Error(`Missing required theme color token: ${token}`);
  return value;
}

function hexToRgb(hex: string): [number, number, number] {
  const normalized = hex.trim().replace(/^#/, "");
  if (!/^[0-9a-fA-F]{6}$/.test(normalized)) {
    throw new Error(`Expected #RRGGBB theme color, got: ${hex}`);
  }
  return [
    Number.parseInt(normalized.slice(0, 2), 16),
    Number.parseInt(normalized.slice(2, 4), 16),
    Number.parseInt(normalized.slice(4, 6), 16),
  ];
}

function withAlpha(hex: string, alpha: number): string {
  const [r, g, b] = hexToRgb(hex);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

function mixHex(a: string, b: string, amount: number): string {
  const [ar, ag, ab] = hexToRgb(a);
  const [br, bg, bb] = hexToRgb(b);
  const mix = (x: number, y: number) => Math.round(x + (y - x) * amount);
  return `#${[mix(ar, br), mix(ag, bg), mix(ab, bb)]
    .map(channel => channel.toString(16).padStart(2, "0"))
    .join("")}`;
}

/** 炫彩流光终端：所有彩色 ANSI 基色均来自 canonical four-color spectrum。
 * Purple/Cyan/bright variants 只是标准四色之间或与白色的派生混色，不是新的品牌 token。 */
function createSpectrumTerminalTheme(readToken: TokenReader): ITheme {
  const blue = readToken("--spectrum-blue");
  const red = readToken("--spectrum-red");
  const yellow = readToken("--spectrum-yellow");
  const green = readToken("--spectrum-green");
  const white = "#ffffff";
  const magenta = mixHex(blue, red, 0.5);
  const cyan = mixHex(blue, green, 0.48);

  return {
    background: TERMINAL_SURFACE_BACKGROUND,
    foreground: "#e8eaf0",
    cursor: blue,
    cursorAccent: "#060709",
    selectionBackground: withAlpha(blue, 0.3),
    // Glass terminal: the scrollbar is a hint, not a decorative bar.
    scrollbarSliderBackground: "rgba(232, 234, 240, 0.16)",
    scrollbarSliderHoverBackground: "rgba(232, 234, 240, 0.28)",
    scrollbarSliderActiveBackground: "rgba(232, 234, 240, 0.4)",
    black: "#17191d",
    red,
    green,
    yellow,
    blue,
    magenta,
    cyan,
    white: "#e8eaf0",
    brightBlack: "#626873",
    brightRed: mixHex(red, white, 0.24),
    brightGreen: mixHex(green, white, 0.22),
    brightYellow: mixHex(yellow, white, 0.18),
    brightBlue: mixHex(blue, white, 0.2),
    brightMagenta: mixHex(magenta, white, 0.22),
    brightCyan: mixHex(cyan, white, 0.22),
    brightWhite: white,
  };
}

/** 黑曜石：更中性的烟晶终端配色，与炫彩流光拉开材质身份 */
const OBSIDIAN_TERMINAL_THEME: ITheme = {
  background: TERMINAL_SURFACE_BACKGROUND,
  foreground: "#e6e7eb",
  cursor: "#60a5fa",
  cursorAccent: "#050608",
  selectionBackground: "rgba(96, 165, 250, 0.24)",
  scrollbarSliderBackground: "rgba(230, 231, 235, 0.14)",
  scrollbarSliderHoverBackground: "rgba(230, 231, 235, 0.26)",
  scrollbarSliderActiveBackground: "rgba(230, 231, 235, 0.38)",
  black: "#17191d",
  red: "#fb5b6b",
  green: "#3fd49a",
  yellow: "#e9a93a",
  blue: "#5b8def",
  magenta: "#a78bfa",
  cyan: "#67c7d9",
  white: "#d8dbe2",
  brightBlack: "#626873",
  brightRed: "#ff7a88",
  brightGreen: "#65e6ae",
  brightYellow: "#f3bf5b",
  brightBlue: "#7aa7ff",
  brightMagenta: "#c4a7ff",
  brightCyan: "#8adbea",
  brightWhite: "#ffffff",
};

/** 浅色主题终端配色 (frosted) */
const FROSTED_TERMINAL_THEME: ITheme = {
  background: TERMINAL_SURFACE_BACKGROUND,
  foreground: "#1e293b",
  cursor: "#3b82f6",
  cursorAccent: "#f8fafc",
  selectionBackground: "rgba(59, 130, 246, 0.2)",
  scrollbarSliderBackground: "rgba(30, 41, 59, 0.18)",
  scrollbarSliderHoverBackground: "rgba(30, 41, 59, 0.3)",
  scrollbarSliderActiveBackground: "rgba(30, 41, 59, 0.42)",
  black: "#f1f5f9",
  red: "#dc2626",
  green: "#16a34a",
  yellow: "#d97706",
  blue: "#2563eb",
  magenta: "#9333ea",
  cyan: "#0891b2",
  white: "#1e293b",
  brightBlack: "#64748b",
  brightRed: "#ef4444",
  brightGreen: "#22c55e",
  brightYellow: "#f59e0b",
  brightBlue: "#3b82f6",
  brightMagenta: "#a855f7",
  brightCyan: "#06b6d4",
  brightWhite: "#0f172a",
};

/**
 * Builds the xterm theme for a TauTerm theme. `readToken` is injected so the module
 * stays DOM-free at import time and the palette stays unit-testable; callers default
 * to the document-root token reader.
 */
export function createTerminalTheme(
  mode: TerminalThemeMode,
  readToken: TokenReader = readTerminalCssToken,
): ITheme {
  switch (mode) {
    case "obsidian":
      return OBSIDIAN_TERMINAL_THEME;
    case "frosted":
      return FROSTED_TERMINAL_THEME;
    case "spectrum-flow":
      return createSpectrumTerminalTheme(readToken);
    default:
      // Returning undefined would hand xterm its opaque default theme; an unknown
      // mode must fail loudly instead of silently painting a black terminal.
      throw new Error(`Unknown terminal theme mode: ${String(mode)}`);
  }
}

export interface TerminalOptionsInput {
  theme: TerminalThemeMode;
  /** 主题 token 读取器；默认读取 document root */
  readToken?: TokenReader;
  /** xterm `convertEol`：只对非 PTY 字节流开启（见 TerminalView 注释） */
  convertEol: boolean;
  fontSize: number;
  scrollback: number;
}

/**
 * The single xterm option set. Surfaces that need a different *presentation* must
 * change this function, not pass a divergent option object from outside.
 */
export function createTerminalOptions(input: TerminalOptionsInput): ITerminalOptions {
  return {
    convertEol: input.convertEol,
    fontSize: input.fontSize,
    fontFamily: TERMINAL_FONT_FAMILY,
    theme: createTerminalTheme(input.theme, input.readToken),
    allowTransparency: true,
    // Block/quadrant/box-drawing characters are drawn as exact cell quads instead of
    // font glyphs (WebGL path). The DOM renderer paints every cell as its own text run,
    // so half-block logos, TUI borders and character scrollbars end up with 1px seams.
    customGlyphs: true,
    cursorBlink: true,
    cursorStyle: "underline",
    allowProposedApi: true,
    scrollback: input.scrollback,
  };
}
