import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { useTranslation } from "react-i18next";
import "@xterm/xterm/css/xterm.css";
import { useTheme } from "../../context/ThemeContext";
import { useToast } from "../../context/ToastContext";
import { shortcutRegistry } from "../../shortcuts/registry";
import { copyToClipboard, readClipboardText } from "../../utils/clipboard";
import { analyzeTerminalPaste } from "../../utils/terminalClipboard";
import ContextMenu from "../common/ContextMenu";
import type { ContextMenuItem } from "../common/ContextMenu";
import type { ContextMenuState } from "../../hooks/useContextMenu";
import ScrollToBottomButton from "./ScrollToBottomButton";
import SearchBar from "./SearchBar";
import PasteSafetyDialog from "./PasteSafetyDialog";
import { mountXtermHost, type XtermHostLifecycle } from "./xtermHostLifecycle";
import { createTerminalOptions, createTerminalTheme } from "./terminalTheme";
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
import styles from "./terminalSurface.module.css";

/** 视口底部容差行数：视口底边与缓冲区底部的间距小于此值即视为"在底部" */
const SCROLL_BOTTOM_TOLERANCE = 5;

/**
 * 数据源适配器：终端 surface 只负责呈现，字节从哪来、往哪去由调用方提供。
 *
 * - `input`：xterm 解析出的输入（键盘、粘贴、终端响应）交给宿主；
 * - `attach`：宿主在此注册输出写入函数，并在挂载时回放自己已有的缓存；
 *   返回的函数在 surface 销毁时调用，宿主必须在这里停止向该实例写入。
 *
 * 序列/去重/历史回放属于数据源，xterm 只消费已经排好序的字节。
 *
 * 是否接受输入不是数据源状态而是 surface prop（`inputEnabled`）：它必须随会话
 * 连接状态变化触发重新渲染，否则“断开时取消待确认粘贴”这类 effect 永远不会重跑。
 */
export interface TerminalSurfaceSource {
  input(data: string): void;
  attach(write: (data: Uint8Array | string) => void): () => void;
}

export interface TerminalSurfaceHandle {
  readonly terminal: XTerm | null;
  write(data: Uint8Array | string): void;
  fit(): void;
  focus(): void;
  copySelection(): void;
  requestPaste(): void;
  openSearch(): void;
  toggleSearch(): void;
}

interface TerminalSurfaceProps {
  /** 会话 ID：只用于 protocol inspector 事件，不参与数据路由 */
  sessionId: string;
  source: TerminalSurfaceSource;
  /**
   * 当前是否接受用户输入（断开的会话保持只读现场）。必须是响应式 prop：
   * 粘贴确认框的取消、右键菜单 Paste 的可用性都依赖它在断连时重新求值。
   */
  inputEnabled?: boolean;
  /** 终端当前是否真的显示在 Workspace 中（分屏 Pane 或当前标签） */
  isVisible?: boolean;
  /** 是否为当前活跃标签页 */
  isActive?: boolean;
  /**
   * 是否为每一行 LF 追加 CR（xterm `convertEol`）。
   *
   * 只对**不是 PTY** 的字节流开启：PTY（Local Shell / SSH）的行结束翻译由
   * 远端 termios / ConPTY 自己负责；随包的 ConPTY 引擎还会用「裸 LF + 相对光标
   * 移动」重绘行首，把 LF 改写成 CR+LF 会让那类相对移动失效、整行贴到第 0 列。
   */
  convertEol?: boolean;
  /** 当前 Session 是否提供 Protocol Inspector 右侧栏 */
  allowProtocolInspect?: boolean;
  /** 提供后右键菜单出现「断开连接」 */
  onDisconnectSession?: () => void;
  /** fit 后回调真实行列数（PTY 会话用于通知远端窗口尺寸） */
  onFit?: (cols: number, rows: number) => void;
}

/** Font-box height measured exactly the way xterm measures a cell. */
function measureFontBoxHeight(fontFamily: string, fontSize: number): number {
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
  onFallback: () => void,
): TerminalRendererKind {
  if (visible) {
    if (addonRef.current) return "webgl";
    try {
      const addon = new WebglAddon();
      addon.onContextLoss(() => {
        // Disposing the addon restores xterm's DOM renderer. A lost GPU context is
        // a downgrade too, so report it instead of leaving the state header
        // claiming a WebGL surface (see `warnRendererFallback`).
        try {
          addon.dispose();
        } catch {
          // Renderer already gone; the terminal keeps rendering through the DOM renderer.
        }
        if (addonRef.current !== addon) return;
        addonRef.current = null;
        warnRendererFallback(term, "WebGL context lost");
        onFallback();
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

/**
 * The single terminal presentation surface.
 *
 * Owns everything that makes a terminal look and behave like a TauTerm terminal —
 * xterm construction and theme, glass transparency CSS, renderer policy, cell-grid
 * alignment, search, clipboard safety, context menu and scroll-to-bottom — so a
 * second surface cannot re-derive a slightly different terminal. Data flow is the
 * caller's job via `TerminalSurfaceSource`.
 */
const TerminalSurface = forwardRef<TerminalSurfaceHandle, TerminalSurfaceProps>(
  function TerminalSurface({
    sessionId,
    source,
    inputEnabled = true,
    isVisible = true,
    isActive = true,
    convertEol = true,
    allowProtocolInspect = false,
    onDisconnectSession,
    onFit,
  }, ref) {
    const { t } = useTranslation();
    const { showToast } = useToast();
    const { theme, fontSize, bufferLines } = useTheme();

    const wrapperRef = useRef<HTMLDivElement>(null);
    const hostRef = useRef<HTMLDivElement>(null);
    const xtermRef = useRef<XTerm | null>(null);
    const webglAddonRef = useRef<WebglAddon | null>(null);
    const hostLifecycleRef = useRef<XtermHostLifecycle | null>(null);
    const [isAtBottom, setIsAtBottom] = useState(true);
    const [presentation, setPresentation] = useState<TerminalPresentation>(IDLE_TERMINAL_PRESENTATION);
    const [rendererKind, setRendererKind] = useState<TerminalRendererKind>(DOM_TERMINAL_RENDERER);
    const [pendingPaste, setPendingPaste] = useState<string | null>(null);
    const [contextMenu, setContextMenu] = useState<ContextMenuState>({ x: 0, y: 0, visible: false, session: null });
    const [searchOpen, setSearchOpen] = useState(false);

    const sourceRef = useRef(source);
    sourceRef.current = source;
    const inputEnabledRef = useRef(inputEnabled);
    inputEnabledRef.current = inputEnabled;
    const [hostMeasurable, setHostMeasurable] = useState(false);
    const hostMeasurableRef = useRef(hostMeasurable);
    hostMeasurableRef.current = hostMeasurable;
    const [searchState, setSearchState] = useState({ query: "", caseSensitive: false });
    const isVisibleRef = useRef(isVisible);
    isVisibleRef.current = isVisible;
    const onFitRef = useRef(onFit);
    onFitRef.current = onFit;
    const sessionIdRef = useRef(sessionId);
    sessionIdRef.current = sessionId;
    // Surface construction happens once; these refs make the first open use the
    // settings that are current at open time even if they changed during mount.
    const themeRef = useRef(theme);
    themeRef.current = theme;
    const fontSizeRef = useRef(fontSize);
    fontSizeRef.current = fontSize;
    const bufferLinesRef = useRef(bufferLines);
    bufferLinesRef.current = bufferLines;
    const [surfaceTerminal, setSurfaceTerminal] = useState<XTerm | null>(null);

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
      if (term && text && inputEnabledRef.current) {
        // xterm fires the normal input path, so pasted bytes reach the host exactly
        // like typed bytes (including bracketed-paste framing).
        term.paste(text);
      }
      restoreTerminalFocus();
    }, [restoreTerminalFocus]);

    const requestPasteText = useCallback((text: string) => {
      const term = xtermRef.current;
      if (!text || !term || !inputEnabledRef.current) {
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
      if (!inputEnabledRef.current) {
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
      if (clipboard.kind === "empty" || !clipboard.text) {
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

    const scheduleFit = useCallback(() => {
      hostLifecycleRef.current?.scheduleFit();
    }, []);

    useImperativeHandle(ref, () => ({
      get terminal() {
        return xtermRef.current;
      },
      write: (data: Uint8Array | string) => {
        xtermRef.current?.write(data);
      },
      fit: scheduleFit,
      focus: () => xtermRef.current?.focus(),
      copySelection: () => copySelectionRef.current(),
      requestPaste: () => { void requestClipboardPasteRef.current(); },
      openSearch: () => setSearchOpen(true),
      toggleSearch: () => setSearchOpen(open => !open),
    }), [scheduleFit]);

    // 共享 xterm host lifecycle 统一负责可测量后 open、RAF fit 合并与 dispose 顺序。
    useEffect(() => {
      const host = hostRef.current;
      if (!host || hostLifecycleRef.current) return;

      let inputDisposable: { dispose(): void } | null = null;
      let scrollDisposable: { dispose(): void } | null = null;
      let presentationDisposable: { dispose(): void } | null = null;
      let detachSource: (() => void) | null = null;

      const lifecycle = mountXtermHost(host, {
        create: () => {
          const terminal = new XTerm(createTerminalOptions({
            theme: themeRef.current,
            convertEol,
            fontSize: fontSizeRef.current,
            scrollback: bufferLinesRef.current,
          }));
          const fitAddon = new FitAddon();
          terminal.loadAddon(fitAddon);
          terminal.loadAddon(new WebLinksAddon());

          terminal.attachCustomKeyEventHandler((e) => {
            // Terminal compatibility aliases remain explicit at the xterm boundary:
            // Ctrl+Insert = copy, Shift+Insert = paste, Meta+C = copy, Meta+V = paste.
            // Ctrl+C / Ctrl+V keep their PTY meanings and are never remapped here.
            const isKeyDown = e.type === "keydown";
            const lowerKey = e.key.toLowerCase();
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
            terminal,
            fitAddon,
            onOpened: () => {
              xtermRef.current = terminal;
              setSurfaceTerminal(terminal);

              // Prefer the WebGL renderer for displayed terminals: it draws
              // block/box glyphs as exact quads (see `customGlyphs`), which keeps
              // pixel-art logos, TUI frames and character scrollbars free of DOM
              // cell seams. Hidden pooled terminals stay on the DOM renderer.
              setRendererKind(syncWebglRenderer(
                terminal,
                webglAddonRef,
                isVisibleRef.current && hostMeasurableRef.current,
                () => { setRendererKind(DOM_TERMINAL_RENDERER); },
              ));

              // Align rows to whole device pixels after the font is actually loaded;
              // a refit is required because a taller cell changes the row count.
              const alignCellGrid = () => {
                if (xtermRef.current !== terminal) return;
                if (applyCellGrid(terminal)) scheduleFit();
              };
              const fontsReady = document.fonts?.ready;
              if (fontsReady) {
                void fontsReady
                  .then(() => requestAnimationFrame(alignCellGrid))
                  .catch(() => requestAnimationFrame(alignCellGrid));
              } else {
                requestAnimationFrame(alignCellGrid);
              }

              // Input must be subscribed before the host replays its buffered output:
              // shells may answer DSR cursor-position queries while that replay is being
              // parsed, and a missing subscriber blocks them.
              inputDisposable = terminal.onData((data) => {
                if (!inputEnabledRef.current) return;
                sourceRef.current.input(data);
              });
              scrollDisposable = terminal.onScroll((viewportY: number) => {
                const buffer = terminal.buffer.active;
                const viewportBottom = viewportY + terminal.rows;
                setIsAtBottom(viewportBottom >= buffer.baseY - SCROLL_BOTTOM_TOLERANCE);
              });
              setPresentation(readTerminalPresentation(terminal));
              presentationDisposable = terminal.onWriteParsed(() => {
                setPresentation(current => {
                  const next = readTerminalPresentation(terminal);
                  return isSameTerminalPresentation(current, next) ? current : next;
                });
              });

              detachSource = sourceRef.current.attach((data) => {
                if (xtermRef.current === terminal) terminal.write(data);
              });
            },
            onDispose: () => {
              detachSource?.();
              detachSource = null;
              inputDisposable?.dispose();
              scrollDisposable?.dispose();
              presentationDisposable?.dispose();
              inputDisposable = null;
              scrollDisposable = null;
              presentationDisposable = null;
              webglAddonRef.current = null;
              setPresentation(IDLE_TERMINAL_PRESENTATION);
              setRendererKind(DOM_TERMINAL_RENDERER);
              if (xtermRef.current === terminal) xtermRef.current = null;
              setSurfaceTerminal(current => (current === terminal ? null : current));
            },
          };
        },
        onFit: (terminal) => onFitRef.current?.(terminal.cols, terminal.rows),
        onMeasurable: (measurable) => setHostMeasurable(measurable),
      });
      hostLifecycleRef.current = lifecycle;

      return () => {
        if (hostLifecycleRef.current === lifecycle) {
          hostLifecycleRef.current = null;
        }
        lifecycle.dispose();
      };
      // Surface construction happens once per mount: theme/font/visibility are
      // live-updated by the effects below, and `convertEol` is a per-plugin constant.
    }, []);

    // 主题变化时动态更新终端配色，无需销毁重建
    useEffect(() => {
      const term = xtermRef.current;
      if (!term) return;
      term.options.theme = createTerminalTheme(theme);
    }, [theme]);

    // 字体大小 / 行缓冲实时更新：通过 context 驱动，设置页滑块拖动时即时生效
    useEffect(() => {
      const term = xtermRef.current;
      if (!term) return;
      term.options.fontSize = fontSize;
      applyCellGrid(term);
      term.options.scrollback = bufferLines;
      scheduleFit();
    }, [fontSize, bufferLines, scheduleFit]);

    // 当标签页变为活跃时重新调整终端尺寸。
    // 外层 rAF 等待 Pane 布局提交，scheduleFit 再合并到统一的下一帧 fit。
    useEffect(() => {
      if (!isActive) return;
      const raf = requestAnimationFrame(() => {
        const term = xtermRef.current;
        if (term) applyCellGrid(term);
        scheduleFit();
      });
      return () => cancelAnimationFrame(raf);
    }, [isActive, scheduleFit]);

    // Renderer follows display state: GPU rendering only while the terminal is
    // actually shown, DOM renderer while pooled/hidden.
    useEffect(() => {
      const term = xtermRef.current;
      if (!term) return;
      setRendererKind(syncWebglRenderer(term, webglAddonRef, isVisible && hostMeasurable, () => {
        setRendererKind(DOM_TERMINAL_RENDERER);
      }));
    }, [hostMeasurable, isVisible]);

    // 接管系统 paste 事件的 capture 阶段，确保不会先被 xterm 默认处理后再重复发送。
    // 所有入口（系统 paste / 快捷键 / 右键菜单）最终统一进入 requestPasteText → term.paste。
    const handlePasteCapture = useCallback((e: React.ClipboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const text = e.clipboardData.getData("text");
      if (text) requestPasteText(text);
    }, [requestPasteText]);

    // 右键上下文菜单：始终显示自定义菜单，避免浏览器默认菜单弹出
    const handleContextMenu = useCallback((e: React.MouseEvent) => {
      e.preventDefault();
      const { clientX, clientY } = e;
      // 只打开菜单，不预读系统剪贴板。剪贴板访问必须由明确的 Paste 动作触发。
      setContextMenu({ x: clientX, y: clientY, visible: true, session: null });
    }, []);

    const closeContextMenu = useCallback(() => {
      setContextMenu(prev => ({ ...prev, visible: false }));
    }, []);

    const contextMenuItems = useMemo((): ContextMenuItem[] => {
      const term = xtermRef.current;
      const hasSel = term ? term.hasSelection() : false;

      const items: ContextMenuItem[] = [
        { id: "copy", label: t("terminal.copy", "Copy"), icon: "clipboard", disabled: !hasSel },
        ...(allowProtocolInspect
          ? [{
              id: "inspectProtocol",
              label: t("terminal.inspectProtocol", "Inspect selection"),
              icon: "search" as const,
              disabled: !hasSel,
            }]
          : []),
        { id: "paste", label: t("terminal.paste", "Paste"), icon: "paste", disabled: !inputEnabled },
        { id: "sep1", label: "", type: "separator" },
        { id: "selectAll", label: t("terminal.selectAll", "Select All"), icon: "edit" },
        { id: "sep2", label: "", type: "separator" },
        { id: "search", label: t("terminal.search", "Search..."), icon: "search" },
        { id: "clear", label: t("terminal.clear", "Clear"), icon: "trash" },
        { id: "resetTerminal", label: t("terminal.reset", "Reset Terminal"), icon: "refresh" },
      ];

      // 仅连接中显示「断开连接」（破坏性操作，danger 样式，放在末尾）
      if (onDisconnectSession) {
        items.push({ id: "sep3", label: "", type: "separator" });
        items.push({
          id: "disconnect",
          label: t("contextMenu.disconnect", "Disconnect"),
          icon: "stop",
          danger: true,
        });
      }
      return items;
    }, [allowProtocolInspect, t, onDisconnectSession, inputEnabled, contextMenu]);

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
                detail: { sessionId: sessionIdRef.current, input: selection },
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
          setSearchOpen(true);
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
          onDisconnectSession?.();
          break;
      }
    }, [onDisconnectSession, refreshPresentation, restoreTerminalFocus]);

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
      if ((!inputEnabled || !isActive) && pendingPaste !== null) {
        setPendingPaste(null);
      }
    }, [inputEnabled, isActive, pendingPaste]);

    useEffect(() => {
      // The search overlay is positioned from this surface's box, so it must not stay
      // open once the Pane/session is hidden behind another one. Non-terminal Session
      // views are kept mounted and hidden with `display: none`, which only the host's
      // measurable size can observe.
      if (!isActive || !isVisible || !hostMeasurable) setSearchOpen(false);
    }, [hostMeasurable, isActive, isVisible]);

    const handlePasteConfirm = useCallback(() => {
      const text = pendingPaste;
      setPendingPaste(null);
      if (text) commitPaste(text);
    }, [commitPaste, pendingPaste]);

    const handlePasteCancel = useCallback(() => {
      setPendingPaste(null);
      restoreTerminalFocus();
    }, [restoreTerminalFocus]);

    const closeSearch = useCallback(() => {
      setSearchOpen(false);
      restoreTerminalFocus();
    }, [restoreTerminalFocus]);

    return (
      <div className={styles.wrapper} ref={wrapperRef}>
        <div
          ref={hostRef}
          className={styles.host}
          onPasteCapture={handlePasteCapture}
          onContextMenu={handleContextMenu}
        />
        <ScrollToBottomButton
          visible={!isAtBottom}
          onClick={() => {
            xtermRef.current?.scrollToBottom();
            setIsAtBottom(true);
          }}
        />
        <SearchBar
          isOpen={searchOpen}
          onClose={closeSearch}
          terminal={surfaceTerminal}
          savedQuery={searchState.query}
          savedCaseSensitive={searchState.caseSensitive}
          onSaveState={(query, caseSensitive) => setSearchState({ query, caseSensitive })}
          viewportRef={wrapperRef}
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
  },
);

export default TerminalSurface;
