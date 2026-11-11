import { useEffect, useRef, useCallback, useMemo, useState, forwardRef, useImperativeHandle } from "react";
import { useTranslation } from "react-i18next";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { invoke } from "@tauri-apps/api/core";
import "@xterm/xterm/css/xterm.css";
import { useTheme } from "../../context/ThemeContext";
import { shortcutRegistry } from "../../shortcuts/registry";
import { copyToClipboard, readClipboardText } from "../../utils/clipboard";
import { useToast } from "../../context/ToastContext";
import { analyzeTerminalPaste } from "../../utils/terminalClipboard";
import ContextMenu from "../common/ContextMenu";
import type { ContextMenuItem } from "../common/ContextMenu";
import type { ContextMenuState } from "../../hooks/useContextMenu";
import ScrollToBottomButton from "./ScrollToBottomButton";
import PasteSafetyDialog from "./PasteSafetyDialog";
import { mountXtermHost, type XtermHostLifecycle } from "./xtermHostLifecycle";
import { planIntegerCellHeight } from "../../core/terminal-cell-grid";
import {
  IDLE_TERMINAL_PRESENTATION,
  isSameTerminalPresentation,
  readTerminalPresentation,
  resetTerminalSurface,
  type TerminalPresentation,
} from "../../core/terminal-presentation";
import {
  DOM_TERMINAL_RENDERER,
  describeRendererFailure,
  warnRendererFallback,
  type TerminalRendererKind,
} from "../../core/terminal-renderer";
import styles from "./Terminal.module.css";

/** 视口底部容差行数：视口底边与缓冲区底部的间距小于此值即视为"在底部" */
const SCROLL_BOTTOM_TOLERANCE = 5;

/** PTY resize 防抖间隔 (ms)：避免拖拽 resize 时 IPC 风暴 */
const RESIZE_DEBOUNCE_MS = 150;

/** 从 CSS canonical spectrum 读取 TauTerm 四色环境锚点。
 * 不在 TS 中复制十六进制品牌值，避免 Ambient / Button / Terminal 三套颜色漂移。 */
function readRequiredCssColor(token: string): string {
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

/**
 * Enable the WebGL renderer only for terminals that are actually displayed.
 * xterm keeps the DOM renderer as its fallback, so an unsupported WebGL2 context
 * or a lost GPU context degrades to the DOM renderer instead of breaking. Pooled
 * hidden terminals stay on the DOM renderer, which keeps the number of live
 * WebGL contexts bounded by the visible Pane count instead of the tab count.
 *
 * Returns the renderer that is now driving the surface. The downgrade is
 * reported instead of swallowed: `customGlyphs` only exists on the WebGL path,
 * so a silent fallback changes how block and box-drawing characters are drawn.
 */
function syncWebglRenderer(
  term: XTerm,
  addonRef: { current: WebglAddon | null },
  visible: boolean,
): TerminalRendererKind {
  if (visible) {
    if (addonRef.current) return "webgl";
    try {
      const addon = new WebglAddon();
      addon.onContextLoss(() => {
        addon.dispose();
        if (addonRef.current === addon) addonRef.current = null;
      });
      term.loadAddon(addon);
      addonRef.current = addon;
      return "webgl";
    } catch (error) {
      addonRef.current = null;
      warnRendererFallback(term, describeRendererFailure(error));
      return DOM_TERMINAL_RENDERER;
    }
  }
  if (addonRef.current) {
    try {
      addonRef.current.dispose();
    } catch {
      // Renderer already gone; the terminal keeps rendering through the DOM renderer.
    }
    addonRef.current = null;
  }
  return DOM_TERMINAL_RENDERER;
}

/** Font-box height measured exactly the way xterm measures a cell. */function measureFontBoxHeight(fontFamily: string, fontSize: number): number {
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d");
  if (!context) return 0;
  context.font = `${fontSize}px ${fontFamily}`;
  const metrics = context.measureText("W");
  const ascent = metrics.fontBoundingBoxAscent;
  const descent = metrics.fontBoundingBoxDescent;
  if (typeof ascent === "number" && typeof descent === "number" && ascent + descent > 0) {
    return ascent + descent;
  }
  return 0;
}

/**
 * Align DOM rows to whole device pixels: xterm derives the CSS row height from
 * an integer device cell divided by the display scale, so fractional scales can
 * leave box-drawing columns on a half-pixel phase and clip them into dashes (see
 * `src/core/terminal-cell-grid.ts`). Returns true when the line height changed
 * and a refit is required.
 */
function applyCellGrid(term: XTerm): boolean {
  const fontFamily = term.options.fontFamily;
  const fontSize = term.options.fontSize;
  if (!fontFamily || !fontSize) return false;
  const fontBoxHeight = measureFontBoxHeight(fontFamily, fontSize);
  if (!(fontBoxHeight > 0)) return false;
  const plan = planIntegerCellHeight({
    lineHeight: term.options.lineHeight ?? 1,
    fontBoxHeight,
    devicePixelRatio: window.devicePixelRatio || 1,
  });
  if (!plan) return false;
  term.options.lineHeight = plan.lineHeight;
  return true;
}

/** 炫彩流光终端：所有彩色 ANSI 基色均来自 canonical four-color spectrum。
 * Purple/Cyan/bright variants 只是标准四色之间或与白色的派生混色，不是新的品牌 token。 */
function createSpectrumTerminalTheme() {
  const blue = readRequiredCssColor("--spectrum-blue");
  const red = readRequiredCssColor("--spectrum-red");
  const yellow = readRequiredCssColor("--spectrum-yellow");
  const green = readRequiredCssColor("--spectrum-green");
  const white = "#ffffff";
  const magenta = mixHex(blue, red, 0.5);
  const cyan = mixHex(blue, green, 0.48);

  return {
    background: "rgba(0, 0, 0, 0)",
    foreground: "#e8eaf0",
    cursor: blue,
    cursorAccent: "#060709",
    selectionBackground: withAlpha(blue, 0.3),
    // Glass terminal: the scrollbar is a hint, not a chrome bar.
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
    brightBlue: mixHex(blue, white, 0.20),
    brightMagenta: mixHex(magenta, white, 0.22),
    brightCyan: mixHex(cyan, white, 0.22),
    brightWhite: white,
  } as const;
}

/** 黑曜石：更中性的烟晶终端配色，与炫彩流光拉开材质身份 */
const OBSIDIAN_TERMINAL_THEME = {
  background: "rgba(0, 0, 0, 0)",
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
} as const;

/** 浅色主题终端配色 (frosted) */
const LIGHT_TERMINAL_THEME = {
  background: "rgba(0, 0, 0, 0)",
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
} as const;

interface TerminalInstanceProps {
  /** 会话 ID，用于关联数据和命令 */
  sessionId: string;
  runtimeId: string | null;
  /** 当用户在终端输入时回调 */
  onData?: (data: string) => void;
  /** 是否已连接 */
  isConnected?: boolean;
  /** 是否为当前活跃标签页 */
  isActive?: boolean;
  /** 终端当前是否真的显示在 Workspace 中（分屏 Pane 或当前标签）。用于按需启用 GPU 渲染器。 */
  isVisible?: boolean;
  /** 当终端就绪时回调，传入 write 函数供父组件注册数据路由 */
  onTermReady?: (writeFn: (data: Uint8Array | string) => void) => void;
  /** 当终端实例卸载时回调，供父组件清理数据路由 */
  onCleanup?: (sessionId: string) => void;
  /** 终端字体大小 (px)，来自 context，实时更新 */
  fontSize?: number;
  /** 终端行缓冲上限（所有模式统一），来自 context，实时更新 */
  bufferLines?: number;
  /** 触发搜索面板显示 */
  onShowSearch?: () => void;
  /** 触发断开当前会话 */
  onDisconnectSession?: () => void;
  /** 当前 Session 是否提供 Protocol Inspector 右侧栏 */
  allowProtocolInspect?: boolean;
  /**
   * 是否为每一行 LF 追加 CR（xterm `convertEol`）。
   *
   * 只对**不是 PTY** 的字节流开启：PTY（Local Shell / SSH）的行结束翻译由
   * 远端 termios / ConPTY 自己负责；随包的 ConPTY 引擎还会用「裸 LF + 相对光标
   * 移动」重绘行首，把 LF 改写成 CR+LF 会让那类相对移动失效、整行贴到第 0 列。
   * 默认开启以保持 Serial / RTT 等原始字节流的现有呈现。
   */
  convertEol?: boolean;
}

/**
 * 终端实例组件
 *
 * 每个标签页渲染一个独立的 xterm.js 实例。
 * 接受 sessionId 以区分数据路由。
 */
const TerminalInstance = forwardRef<any, TerminalInstanceProps>(function TerminalInstance(
  { sessionId, runtimeId, onData, isConnected = false, isActive = true, isVisible = true, onTermReady, onCleanup, fontSize, bufferLines, onShowSearch, onDisconnectSession, allowProtocolInspect = true, convertEol = true },
  ref
) {
  const containerRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XTerm | null>(null);
  const webglAddonRef = useRef<WebglAddon | null>(null);
  const isVisibleRef = useRef(isVisible);
  isVisibleRef.current = isVisible;
  const hostLifecycleRef = useRef<XtermHostLifecycle | null>(null);
  // PTY resize 防抖定时器：避免拖拽 resize 时 IPC 风暴
  const resizeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 使用 ref 持有最新的回调，避免初始化 effect 中的闭包过期问题
  const [isAtBottom, setIsAtBottom] = useState(true);
  // Host-visible emulation state (alt screen / mouse capture). xterm owns the
  // mode bits; this mirror only feeds the context-menu summary and recovery affordances.
  const [presentation, setPresentation] = useState<TerminalPresentation>(IDLE_TERMINAL_PRESENTATION);
  // Host-visible renderer state: `customGlyphs` is WebGL-only, so a downgraded
  // surface renders block art differently and must be explainable (see
  // `syncWebglRenderer`).
  const [rendererKind, setRendererKind] = useState<TerminalRendererKind>(DOM_TERMINAL_RENDERER);
  const onTermReadyRef = useRef(onTermReady);
  onTermReadyRef.current = onTermReady;
  const onCleanupRef = useRef(onCleanup);
  onCleanupRef.current = onCleanup;
  // xterm 可能在解析首批 shell 输出时立即产生终端响应（例如 DSR 光标位置报告）。
  // 用 ref 保持最新输入回调，并在首批输出回放前完成 onData 订阅，避免响应丢失后 shell 阻塞。
  const onDataRef = useRef(onData);
  onDataRef.current = onData;
  const isConnectedRef = useRef(isConnected);
  isConnectedRef.current = isConnected;
  const [pendingPaste, setPendingPaste] = useState<string | null>(null);
  const { t } = useTranslation();
  const { showToast } = useToast();

  const restoreTerminalFocus = useCallback(() => {
    requestAnimationFrame(() => {
      xtermRef.current?.focus();
    });
  }, []);

  /** Re-read the terminal's mode mirror after a host-initiated reset. */
  const refreshPresentation = useCallback(() => {
    const term = xtermRef.current;
    if (!term) return;
    setPresentation(current => {
      const next = readTerminalPresentation(term);
      return isSameTerminalPresentation(current, next) ? current : next;
    });
  }, []);

  const commitPaste = useCallback((text: string) => {
    const term = xtermRef.current;
    if (term && text && isConnectedRef.current) {
      term.paste(text);
    }
    restoreTerminalFocus();
  }, [restoreTerminalFocus]);

  const requestPasteText = useCallback((text: string) => {
    const term = xtermRef.current;
    if (!text || !term || !isConnectedRef.current) {
      restoreTerminalFocus();
      return;
    }
    const pasteAnalysis = analyzeTerminalPaste(text);
    // A line break can submit input immediately when Bracketed Paste Mode (DECSET 2004)
    // is absent. Very large pastes remain confirmable regardless of bracketed mode to
    // avoid accidentally flooding a remote/serial target or making the UI unresponsive.
    const shouldWarnForLineBreak = pasteAnalysis.hasLineBreak && !term.modes.bracketedPasteMode;
    if (shouldWarnForLineBreak || pasteAnalysis.isLargePaste) {
      setPendingPaste(text);
      return;
    }
    commitPaste(text);
  }, [commitPaste, restoreTerminalFocus]);

  const requestClipboardPaste = useCallback(async () => {
    if (!isConnectedRef.current) {
      restoreTerminalFocus();
      return;
    }
    const clipboard = await readClipboardText();
    if (clipboard.kind === "unavailable") {
      // A denied/unavailable clipboard used to be indistinguishable from an empty
      // one, which made paste look broken. Always tell the user which happened.
      showToast("error", t("terminal.clipboardUnavailable"));
      restoreTerminalFocus();
      return;
    }
    if (clipboard.kind === "empty") {
      restoreTerminalFocus();
      return;
    }
    requestPasteText(clipboard.text);
  }, [requestPasteText, restoreTerminalFocus, showToast, t]);

  const copySelection = useCallback(() => {
    const term = xtermRef.current;
    const selection = term?.getSelection() ?? "";
    if (!selection) {
      restoreTerminalFocus();
      return;
    }
    void copyToClipboard(selection).finally(restoreTerminalFocus);
  }, [restoreTerminalFocus]);

  const copySelectionRef = useRef(copySelection);
  copySelectionRef.current = copySelection;
  const requestClipboardPasteRef = useRef(requestClipboardPaste);
  requestClipboardPasteRef.current = requestClipboardPaste;

  const { theme } = useTheme();
  const terminalTheme = useMemo(() => (
    theme === "frosted"
      ? LIGHT_TERMINAL_THEME
      : theme === "obsidian"
        ? OBSIDIAN_TERMINAL_THEME
        : createSpectrumTerminalTheme()
  ), [theme]);
  const terminalThemeRef = useRef(terminalTheme);
  terminalThemeRef.current = terminalTheme;
  const fontSizeRef = useRef(fontSize);
  fontSizeRef.current = fontSize;
  const bufferLinesRef = useRef(bufferLines);
  bufferLinesRef.current = bufferLines;
  const convertEolRef = useRef(convertEol);
  convertEolRef.current = convertEol;

  // 右键上下文菜单状态
  // 直接用 useState 管理，而非 useContextMenu hook——后者面向 Tab 标签右键菜单，强依赖 session 参数，此处不适用
  const [contextMenu, setContextMenu] = useState<ContextMenuState>({ x: 0, y: 0, visible: false, session: null });
  // 回调 refs：避免 context menu handler 持有过期闭包
  const onShowSearchRef = useRef(onShowSearch);
  onShowSearchRef.current = onShowSearch;
  const onDisconnectSessionRef = useRef(onDisconnectSession);
  onDisconnectSessionRef.current = onDisconnectSession;

  /** 通知后端 PTY 窗口尺寸已变更（带 150ms 防抖） */
  const notifyResize = useCallback(() => {
    if (resizeTimerRef.current) {
      clearTimeout(resizeTimerRef.current);
    }
    resizeTimerRef.current = setTimeout(() => {
      const term = xtermRef.current;
      if (term && runtimeId) {
        invoke("resize_pty", { sessionId, runtimeId, cols: term.cols, rows: term.rows }).catch(() => {});
      }
    }, RESIZE_DEBOUNCE_MS);
  }, [sessionId, runtimeId]);

  /** 所有 xterm fit 都由共享 host lifecycle 合并并做 dispose 保护。 */
  const scheduleFit = useCallback(() => {
    hostLifecycleRef.current?.scheduleFit();
  }, []);

  // 暴露 xterm 实例和 write 方法
  useImperativeHandle(ref, () => ({
    write: (data: Uint8Array | string) => {
      xtermRef.current?.write(data);
    },
    fit: scheduleFit,
    copySelection: () => copySelectionRef.current(),
    requestPaste: () => requestClipboardPasteRef.current(),
    focus: () => xtermRef.current?.focus(),
    get terminal() {
      return xtermRef.current;
    },
  }), [scheduleFit]);

  // 共享 xterm host lifecycle 统一负责可测量后 open、RAF fit 合并与 dispose 顺序。
  // 终端业务事件仍由本组件持有，避免把 PTY/剪贴板语义耦合进通用宿主。
  useEffect(() => {
    const container = containerRef.current;
    if (!container || hostLifecycleRef.current) return;

    let inputDisposable: { dispose(): void } | null = null;
    let scrollDisposable: { dispose(): void } | null = null;
    let presentationDisposable: { dispose(): void } | null = null;
    let opened = false;

    const lifecycle = mountXtermHost(container, {
      create: () => {
        const nextTerm = new XTerm({
          // PTY-backed sessions translate line endings in the PTY, not here (see `convertEol`).
          convertEol: convertEolRef.current ?? true,
          fontSize: fontSizeRef.current ?? Number(localStorage.getItem("tauterm-font-size") || "14"),
          fontFamily: '"JetBrains Mono", "Cascadia Code", "Fira Code", "Consolas", "Courier New", monospace',
          theme: terminalThemeRef.current,
          allowTransparency: true,
          // Block, quadrant and box-drawing characters are drawn as exact cell
          // quads instead of font glyphs (WebGL renderer). The DOM renderer paints
          // every cell as its own text run, so half-block logos, TUI borders and
          // character scrollbars end up with 1px seams between adjacent cells.
          customGlyphs: true,
          cursorBlink: true,
          cursorStyle: "underline",
          allowProposedApi: true,
          scrollback: bufferLinesRef.current ?? Number(localStorage.getItem("tauterm-buffer-lines") || "10000"),
          cols: 80,
          rows: 24,
        });
        const fitAddon = new FitAddon();
        nextTerm.loadAddon(fitAddon);
        nextTerm.loadAddon(new WebLinksAddon());

        nextTerm.attachCustomKeyEventHandler((e) => {
          const isKeyDown = e.type === "keydown";
          const lowerKey = e.key.toLowerCase();
          // Terminal compatibility aliases remain explicit at the xterm boundary:
          // Ctrl+Insert = copy, Shift+Insert = paste, Meta+C = copy, Meta+V = paste.
          // Ctrl+C / Ctrl+V keep their PTY meanings and are never remapped here.
          const compatibilityCopy = e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && e.key === "Insert";
          const compatibilityPaste = e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey && e.key === "Insert";
          const macCopy = e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey && lowerKey === "c";
          const macPaste = e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey && lowerKey === "v";

          if (compatibilityCopy || macCopy) {
            e.preventDefault();
            e.stopPropagation();
            if (isKeyDown) copySelectionRef.current();
            return false;
          }
          if (compatibilityPaste || macPaste) {
            e.preventDefault();
            e.stopPropagation();
            if (isKeyDown) void requestClipboardPasteRef.current();
            return false;
          }

          const matched = shortcutRegistry.match(e);
          if (matched) return false;
          return true;
        });

        return {
          terminal: nextTerm,
          fitAddon,
          onOpened: () => {
            opened = true;
            xtermRef.current = nextTerm;

            // Prefer the WebGL renderer for displayed terminals: it draws
            // block/box glyphs as exact quads (see `customGlyphs`), which keeps
            // pixel-art logos, TUI frames and character scrollbars free of DOM
            // cell seams. Hidden pooled terminals stay on the DOM renderer.
            setRendererKind(syncWebglRenderer(nextTerm, webglAddonRef, isVisibleRef.current));

            // Align rows to whole device pixels after the font is actually loaded;
            // a refit is required because a taller cell changes the row count.
            const alignCellGrid = () => {
              if (xtermRef.current !== nextTerm) return;
              if (applyCellGrid(nextTerm)) scheduleFit();
            };
            const fontsReady = document.fonts?.ready;
            if (fontsReady) {
              void fontsReady
                .then(() => requestAnimationFrame(alignCellGrid))
                .catch(() => requestAnimationFrame(alignCellGrid));
            } else {
              requestAnimationFrame(alignCellGrid);
            }

            inputDisposable = nextTerm.onData((data) => {
              onDataRef.current?.(data);
            });
            scrollDisposable = nextTerm.onScroll((viewportY: number) => {
              const buffer = nextTerm.buffer.active;
              const viewportBottom = viewportY + nextTerm.rows;
              setIsAtBottom(viewportBottom >= buffer.baseY - SCROLL_BOTTOM_TOLERANCE);
            });

            setPresentation(readTerminalPresentation(nextTerm));
            presentationDisposable = nextTerm.onWriteParsed(() => {
              setPresentation(current => {
                const next = readTerminalPresentation(nextTerm);
                return isSameTerminalPresentation(current, next) ? current : next;
              });
            });

            // Input must be subscribed before startup data is replayed. Shells may answer DSR
            // immediately while that replay is being parsed.
            onTermReadyRef.current?.((data: Uint8Array | string) => {
              if (xtermRef.current === nextTerm) nextTerm.write(data);
            });
          },
          onDispose: () => {
            inputDisposable?.dispose();
            scrollDisposable?.dispose();
            presentationDisposable?.dispose();
            inputDisposable = null;
            scrollDisposable = null;
            presentationDisposable = null;
            setPresentation(IDLE_TERMINAL_PRESENTATION);
            setRendererKind(DOM_TERMINAL_RENDERER);
            if (resizeTimerRef.current) {
              clearTimeout(resizeTimerRef.current);
              resizeTimerRef.current = null;
            }
            if (xtermRef.current === nextTerm) xtermRef.current = null;
            if (opened) onCleanupRef.current?.(sessionId);
          },
        };
      },
      onFit: () => notifyResize(),
    });
    hostLifecycleRef.current = lifecycle;

    return () => {
      if (hostLifecycleRef.current === lifecycle) {
        hostLifecycleRef.current = null;
      }
      lifecycle.dispose();
    };
  }, [notifyResize, sessionId]);

  // 主题变化时动态更新终端配色，无需销毁重建
  useEffect(() => {
    if (!xtermRef.current) return;
    xtermRef.current.options.theme = terminalTheme;
  }, [terminalTheme]);

  // 字体大小 / 行缓冲实时更新：通过 context 驱动，设置页滑块拖动时即时生效
  useEffect(() => {
    const term = xtermRef.current;
    if (!term) return;
    if (fontSize !== undefined) {
      term.options.fontSize = fontSize;
      // A new font size can leave the row on a fractional device-pixel phase again.
      applyCellGrid(term);
    }
    if (bufferLines !== undefined) {
      term.options.scrollback = bufferLines;
    }
    if (fontSize !== undefined) {
      scheduleFit();
    }
  }, [fontSize, bufferLines, scheduleFit]);

  // 当标签页变为活跃时重新调整终端尺寸。
  // 外层 rAF 等待 Pane 布局提交，scheduleFit 再合并到统一的下一帧 fit。
  useEffect(() => {
    if (!isActive) return;
    const raf = requestAnimationFrame(() => {
      const term = xtermRef.current;
      // Re-check the device-pixel grid: the window may have moved to a monitor
      // with a different display scale while this terminal was inactive.
      if (term) applyCellGrid(term);
      scheduleFit();
    });
    return () => cancelAnimationFrame(raf);
  }, [isActive, scheduleFit]);

  // Renderer follows display state: GPU rendering only while the terminal is
  // actually shown, DOM renderer while pooled/hidden (see `syncWebglRenderer`).
  useEffect(() => {
    const term = xtermRef.current;
    if (!term) return;
    setRendererKind(syncWebglRenderer(term, webglAddonRef, isVisible));
  }, [isVisible]);

  // 接管系统 paste 事件的 capture 阶段，确保不会先被 xterm 默认处理后再重复发送。
  // 所有入口（系统 paste / 快捷键 / 右键菜单）最终统一进入 requestPasteText → term.paste。
  const handlePaste = useCallback((e: React.ClipboardEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const text = e.clipboardData.getData("text");
    if (text) requestPasteText(text);
  }, [requestPasteText]);

  // 右键上下文菜单
  // 始终显示自定义菜单（与 isConnected 无关），避免浏览器默认菜单弹出
  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const { clientX, clientY } = e;

    // 只打开菜单，不预读系统剪贴板。剪贴板访问必须由明确的 Paste 动作触发。
    setContextMenu({
      x: clientX,
      y: clientY,
      visible: true,
      session: null,
    });
  }, []);

  // 关闭右键菜单
  const closeContextMenu = useCallback(() => {
    setContextMenu(prev => ({ ...prev, visible: false }));
  }, []);

  // 构建菜单项：根据 isConnected / selection 动态控制 disabled
  const contextMenuItems = useMemo((): ContextMenuItem[] => {
    const term = xtermRef.current;
    const hasSel = term ? term.hasSelection() : false;

    const items: ContextMenuItem[] = [
      {
        id: "copy",
        label: t("terminal.copy", "Copy"),
        icon: "clipboard",
        disabled: !hasSel,
      },
      ...(allowProtocolInspect
        ? [{
            id: "inspectProtocol",
            label: t("terminal.inspectProtocol", "Inspect selection"),
            icon: "search" as const,
            disabled: !hasSel,
          }]
        : []),
      {
        id: "paste",
        label: t("terminal.paste", "Paste"),
        icon: "paste",
        disabled: !isConnected,
      },
      { id: "sep1", label: "", type: "separator" },
      {
        id: "selectAll",
        label: t("terminal.selectAll", "Select All"),
        icon: "edit",
      },
      { id: "sep2", label: "", type: "separator" },
      {
        id: "search",
        label: t("terminal.search", "Search..."),
        icon: "search",
      },
      {
        id: "clear",
        label: t("terminal.clear", "Clear"),
        icon: "trash",
      },
      {
        id: "resetTerminal",
        label: t("terminal.reset", "Reset Terminal"),
        icon: "refresh",
      },
    ];

    // 仅连接中显示「断开连接」（破坏性操作，danger 样式，放在末尾）
    if (isConnected) {
      items.push({ id: "sep3", label: "", type: "separator" });
      items.push({
        id: "disconnect",
        label: t("contextMenu.disconnect", "Disconnect"),
        icon: "stop",
        danger: true,
      });
    }

    return items;
  }, [allowProtocolInspect, t, isConnected, contextMenu]);

  // 菜单项点击处理
  const handleContextMenuSelect = useCallback((itemId: string) => {
    const term = xtermRef.current;
    if (!term) return;

    switch (itemId) {
      case "copy":
        copySelectionRef.current();
        break;
      case "inspectProtocol": {
        const selection = term.getSelection();
        if (selection) {
          window.dispatchEvent(
            new CustomEvent("tauterm:protocol-inspect", {
              detail: { sessionId, input: selection },
            }),
          );
        }
        restoreTerminalFocus();
        break;
      }
      case "paste":
        void requestClipboardPasteRef.current();
        break;
      case "selectAll":
        term.selectAll();
        restoreTerminalFocus();
        break;
      case "search":
        onShowSearchRef.current?.();
        break;
      case "clear":
        term.clear();
        restoreTerminalFocus();
        break;
      case "resetTerminal":
        resetTerminalSurface(term);
        refreshPresentation();
        restoreTerminalFocus();
        break;
      case "disconnect":
        onDisconnectSessionRef.current?.();
        break;
    }
  }, [refreshPresentation, restoreTerminalFocus, sessionId]);

  // Read-only mode summary for the context-menu header. It explains why a
  // full-screen app may behave differently (and why Shift is needed to select)
  // without overlaying the terminal content with a permanent badge.
  const modeHeader = useMemo(() => {
    const parts: string[] = [];
    if (presentation.alternateScreen) {
      parts.push(t("terminal.fullscreenApp", "Full-screen app"));
    }
    if (presentation.alternateScreen || presentation.mouseTrackingMode !== "none") {
      parts.push(t("terminal.mouseReporting", "Mouse reporting: {{mode}}", {
        mode: presentation.mouseTrackingMode,
      }));
    }
    // Only the degraded case is worth surfacing: a WebGL surface draws block and
    // box-drawing glyphs as exact quads, the DOM fallback uses the font and can
    // fragment pixel-art logos (the app must never hide that difference again).
    if (rendererKind === DOM_TERMINAL_RENDERER) {
      parts.push(t("terminal.rendererFallback", "Renderer: DOM fallback (no WebGL)"));
    }
    return parts.length > 0 ? parts.join(" · ") : null;
  }, [presentation, rendererKind, t]);

  useEffect(() => {
    // A pending confirmation belongs to exactly one active terminal. If the
    // session disconnects or the user switches Pane/Session, cancel it instead
    // of allowing a later confirmation to target a hidden/inactive terminal.
    if ((!isConnected || !isActive) && pendingPaste !== null) {
      setPendingPaste(null);
    }
  }, [isActive, isConnected, pendingPaste]);

  const handlePasteConfirm = useCallback(() => {
    const text = pendingPaste;
    setPendingPaste(null);
    if (text) commitPaste(text);
  }, [commitPaste, pendingPaste]);

  const handlePasteCancel = useCallback(() => {
    setPendingPaste(null);
    restoreTerminalFocus();
  }, [restoreTerminalFocus]);

  return (
    <div className={styles.terminalInstanceWrapper}>
      <div
        ref={containerRef}
        className={styles.terminal}
        onPasteCapture={handlePaste}
        onContextMenu={handleContextMenu}
      />
      <ScrollToBottomButton
        visible={!isAtBottom}
        onClick={() => {
          xtermRef.current?.scrollToBottom();
          setIsAtBottom(true);
        }}
      />
      <ContextMenu
        state={contextMenu}
        items={contextMenuItems}
        header={modeHeader ? { label: modeHeader } : null}
        onSelect={handleContextMenuSelect}
        onClose={closeContextMenu}
      />
      <PasteSafetyDialog
        text={pendingPaste}
        onConfirm={handlePasteConfirm}
        onCancel={handlePasteCancel}
      />
    </div>
  );
});

export default TerminalInstance;
