import { useEffect, useRef, useCallback, useMemo, useState, forwardRef, useImperativeHandle } from "react";
import { useTranslation } from "react-i18next";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { invoke } from "@tauri-apps/api/core";
import "@xterm/xterm/css/xterm.css";
import { useTheme } from "../../context/ThemeContext";
import { shortcutRegistry } from "../../shortcuts/registry";
import { copyToClipboard, readFromClipboard } from "../../utils/clipboard";
import { analyzeTerminalPaste } from "../../utils/terminalClipboard";
import ContextMenu from "../common/ContextMenu";
import type { ContextMenuItem } from "../common/ContextMenu";
import type { ContextMenuState } from "../../hooks/useContextMenu";
import ScrollToBottomButton from "./ScrollToBottomButton";
import PasteSafetyDialog from "./PasteSafetyDialog";
import { createManagedXTermHost, type ManagedXTermHost } from "./xtermLifecycle";
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
    background: "transparent",
    foreground: "#e8eaf0",
    cursor: blue,
    cursorAccent: "#060709",
    selectionBackground: withAlpha(blue, 0.3),
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
  background: "transparent",
  foreground: "#e6e7eb",
  cursor: "#60a5fa",
  cursorAccent: "#050608",
  selectionBackground: "rgba(96, 165, 250, 0.24)",
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
  background: "transparent",
  foreground: "#1e293b",
  cursor: "#3b82f6",
  cursorAccent: "#f8fafc",
  selectionBackground: "rgba(59, 130, 246, 0.2)",
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
  /** 当用户在终端输入时回调 */
  onData?: (data: string) => void;
  /** 是否已连接 */
  isConnected?: boolean;
  /** 是否为当前活跃标签页 */
  isActive?: boolean;
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
}

/**
 * 终端实例组件
 *
 * 每个标签页渲染一个独立的 xterm.js 实例。
 * 接受 sessionId 以区分数据路由。
 */
const TerminalInstance = forwardRef<any, TerminalInstanceProps>(function TerminalInstance(
  { sessionId, onData, isConnected = false, isActive = true, onTermReady, onCleanup, fontSize, bufferLines, onShowSearch, onDisconnectSession, allowProtocolInspect = true },
  ref
) {
  const containerRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XTerm | null>(null);
  const xtermLayoutRef = useRef<ManagedXTermHost | null>(null);
  // PTY resize 防抖定时器：避免拖拽 resize 时 IPC 风暴
  const resizeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 使用 ref 持有最新的回调，避免初始化 effect 中的闭包过期问题
  const [isAtBottom, setIsAtBottom] = useState(true);
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

  const restoreTerminalFocus = useCallback(() => {
    requestAnimationFrame(() => {
      xtermRef.current?.focus();
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
    const text = await readFromClipboard();
    if (!text) {
      restoreTerminalFocus();
      return;
    }
    requestPasteText(text);
  }, [requestPasteText, restoreTerminalFocus]);

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

  const { t } = useTranslation();
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
      if (term && sessionId) {
        invoke("resize_pty", { sessionId, cols: term.cols, rows: term.rows }).catch(() => {});
      }
    }, RESIZE_DEBOUNCE_MS);
  }, [sessionId]);

  /**
   * 所有 xterm fit 都委托给共享 lifecycle；ResizeObserver、StrictMode cleanup、
   * hidden Pane 和 imperative fit 因此使用同一套销毁/RAF 约束。
   */
  const scheduleFit = useCallback(() => {
    xtermLayoutRef.current?.scheduleFit();
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

  // xterm 的 DOM 生命周期由共享 host 管理器负责：仅在宿主仍连接且尺寸非零时 open/fit，
  // cleanup 会先取消所有 ResizeObserver / RAF，再销毁 renderer。
  useEffect(() => {
    const container = containerRef.current;
    if (!container || xtermLayoutRef.current) return;

    let opened = false;
    const layout = createManagedXTermHost({
      host: container,
      create: () => {
        const terminal = new XTerm({
          convertEol: true,
          fontSize: fontSizeRef.current ?? Number(localStorage.getItem("tauterm-font-size") || "14"),
          fontFamily: '"JetBrains Mono", "Cascadia Code", "Fira Code", "Consolas", "Courier New", monospace',
          theme: terminalThemeRef.current,
          allowTransparency: true,
          cursorBlink: true,
          cursorStyle: "underline",
          allowProposedApi: true,
          scrollback: bufferLinesRef.current ?? Number(localStorage.getItem("tauterm-buffer-lines") || "10000"),
          cols: 80,
          rows: 24,
        });
        const fitAddon = new FitAddon();
        terminal.loadAddon(fitAddon);
        terminal.loadAddon(new WebLinksAddon());

        terminal.attachCustomKeyEventHandler((e) => {
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

        return { terminal, fitAddon };
      },
      onOpen: ({ terminal }) => {
        opened = true;
        xtermRef.current = terminal;

        const inputDisposable = terminal.onData((data) => {
          onDataRef.current?.(data);
        });
        const scrollDisposable = terminal.onScroll((viewportY: number) => {
          const buffer = terminal.buffer.active;
          const viewportBottom = viewportY + terminal.rows;
          setIsAtBottom(viewportBottom >= buffer.baseY - SCROLL_BOTTOM_TOLERANCE);
        });

        onTermReadyRef.current?.((data: Uint8Array | string) => {
          if (xtermRef.current === terminal) terminal.write(data);
        });

        return () => {
          inputDisposable.dispose();
          scrollDisposable.dispose();
          if (xtermRef.current === terminal) xtermRef.current = null;
        };
      },
      onFit: notifyResize,
    });
    xtermLayoutRef.current = layout;

    return () => {
      if (xtermLayoutRef.current === layout) xtermLayoutRef.current = null;
      layout.dispose();
      if (resizeTimerRef.current) {
        clearTimeout(resizeTimerRef.current);
        resizeTimerRef.current = null;
      }
      if (opened) {
        // React StrictMode 的探测性 cleanup 若尚未真正 open，不通知父层清理 startup buffer。
        onCleanupRef.current?.(sessionId);
      }
    };
  }, []);

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
    const raf = requestAnimationFrame(scheduleFit);
    return () => cancelAnimationFrame(raf);
  }, [isActive, scheduleFit]);

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
      case "disconnect":
        onDisconnectSessionRef.current?.();
        break;
    }
  }, [restoreTerminalFocus, sessionId]);

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
