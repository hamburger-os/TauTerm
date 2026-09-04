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
import ContextMenu from "../common/ContextMenu";
import type { ContextMenuItem } from "../common/ContextMenu";
import type { ContextMenuState } from "../../hooks/useContextMenu";
import ScrollToBottomButton from "./ScrollToBottomButton";
import styles from "./Terminal.module.css";

/** 视口底部容差行数：视口底边与缓冲区底部的间距小于此值即视为"在底部" */
const SCROLL_BOTTOM_TOLERANCE = 5;

/** PTY resize 防抖间隔 (ms)：避免拖拽 resize 时 IPC 风暴 */
const RESIZE_DEBOUNCE_MS = 150;

/** 深色主题终端配色 (google-glow / obsidian) */
const DARK_TERMINAL_THEME = {
  background: "transparent",
  foreground: "#e0e0ff",
  cursor: "#4285F4",
  cursorAccent: "#060610",
  selectionBackground: "rgba(66, 133, 244, 0.3)",
  black: "#1a1a2e",
  red: "#ff4757",
  green: "#34d399",
  yellow: "#ffa502",
  blue: "#4285F4",
  magenta: "#a855f7",
  cyan: "#60a5fa",
  white: "#e0e0ff",
  brightBlack: "#555577",
  brightRed: "#ff6b81",
  brightGreen: "#4ade80",
  brightYellow: "#ffbe76",
  brightBlue: "#60a5fa",
  brightMagenta: "#c084fc",
  brightCyan: "#67e8f9",
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
}

/**
 * 终端实例组件
 *
 * 每个标签页渲染一个独立的 xterm.js 实例。
 * 接受 sessionId 以区分数据路由。
 */
const TerminalInstance = forwardRef<any, TerminalInstanceProps>(function TerminalInstance(
  { sessionId, onData, isConnected = false, isActive = true, onTermReady, onCleanup, fontSize, bufferLines, onShowSearch, onDisconnectSession },
  ref
) {
  const containerRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
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

  const { t } = useTranslation();
  const { theme } = useTheme();
  const isDark = theme === "google-glow" || theme === "obsidian";
  const terminalTheme = isDark ? DARK_TERMINAL_THEME : LIGHT_TERMINAL_THEME;

  // 右键上下文菜单状态
  // 直接用 useState 管理，而非 useContextMenu hook——后者面向 Tab 标签右键菜单，强依赖 session 参数，此处不适用
  const [contextMenu, setContextMenu] = useState<ContextMenuState>({ x: 0, y: 0, visible: false, session: null });
  // 剪贴板是否为空（异步检测）
  const [clipboardHasText, setClipboardHasText] = useState(false);
  // 回调 refs：避免 context menu handler 持有过期闭包
  const onShowSearchRef = useRef(onShowSearch);
  onShowSearchRef.current = onShowSearch;
  const onDisconnectSessionRef = useRef(onDisconnectSession);
  onDisconnectSessionRef.current = onDisconnectSession;

  // 暴露 xterm 实例和 write 方法
  useImperativeHandle(ref, () => ({
    write: (data: Uint8Array | string) => {
      xtermRef.current?.write(data);
    },
    fit: () => {
      fitAddonRef.current?.fit();
    },
    get terminal() {
      return xtermRef.current;
    },
  }));

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

  // 初始化 xterm.js
  useEffect(() => {
    if (!containerRef.current || xtermRef.current) return;

    const term = new XTerm({
      convertEol: true,
      fontSize: fontSize ?? Number(localStorage.getItem("tauterm-font-size") || "14"),
      fontFamily: '"JetBrains Mono", "Cascadia Code", "Fira Code", "Consolas", "Courier New", monospace',
      theme: terminalTheme,
      cursorBlink: true,
      cursorStyle: "underline", // 下划线光标：不遮挡字符内容，串口/TUI 场景可读性优于 bar/block
      allowProposedApi: true,
      scrollback: bufferLines ?? Number(localStorage.getItem("tauterm-buffer-lines") || "10000"),
      cols: 80,
      rows: 24,
    });

    const fitAddon = new FitAddon();
    const webLinksAddon = new WebLinksAddon();

    term.loadAddon(fitAddon);
    term.loadAddon(webLinksAddon);

    // 拦截终端内键盘事件：已注册的全局快捷键穿透到浏览器，其余由 xterm 正常处理
    // 这使 Ctrl+F / Ctrl+Tab / Ctrl+Shift+P 等快捷键在终端聚焦时也能正常工作
    term.attachCustomKeyEventHandler((e) => {
      const matched = shortcutRegistry.match(e);
      if (matched) return false; // 穿透 → document keydown → useKeyboard hook
      return true;               // xterm 正常处理（→ onData → PTY）
    });

    term.open(containerRef.current);
    fitAddon.fit();

    xtermRef.current = term;
    fitAddonRef.current = fitAddon;

    // 必须先订阅 xterm 输入，再向父层暴露 write。
    // 父层会在 onTermReady 中同步回放连接初期缓存的 PTY 数据；PowerShell / cmd /
    // Git Bash 等启动阶段可能输出 ESC[6n（DSR）并等待终端应答。如果此时 onData 尚未
    // 注册，xterm 生成的 ESC[row;colR 响应会被丢掉，表现为“连接成功但终端空白、回车无反应”。
    const inputDisposable = term.onData((data) => {
      onDataRef.current?.(data);
    });

    // 终端初始化完成后立即注册写函数，不依赖外部重渲染触发
    onTermReadyRef.current?.((data: Uint8Array | string) => {
      term.write(data);
    });

    const handleResize = () => {
      try { fitAddon.fit(); } catch { /* ignore */ }
      notifyResize();
    };

    const observer = new ResizeObserver(handleResize);
    observer.observe(containerRef.current);

    return () => {
      observer.disconnect();
      inputDisposable.dispose();
      if (resizeTimerRef.current) {
        clearTimeout(resizeTimerRef.current);
        resizeTimerRef.current = null;
      }
      term.dispose();
      xtermRef.current = null;
      fitAddonRef.current = null;
      // 通知父组件清理此会话的 writeRefs 条目
      onCleanupRef.current?.(sessionId);
    };
  }, []);

  // 跟踪 xterm.js 视口滚动位置，用于自动滚动检测和浮动按钮
  useEffect(() => {
    const term = xtermRef.current;
    if (!term) return;

    const disposable = term.onScroll((viewportY: number) => {
      const buffer = term.buffer.active;
      // 视口底部行号 = 视口顶部行号 + 可见行数
      // baseY 是缓冲区历史底部（最大行号），视口底部 >= baseY - 5 即视为"在底部"
      const viewportBottom = viewportY + term.rows;
      const atBottom = viewportBottom >= buffer.baseY - SCROLL_BOTTOM_TOLERANCE;
      setIsAtBottom(atBottom);
    });

    return () => {
      disposable.dispose();
    };
  }, []);

  // 主题变化时动态更新终端配色，无需销毁重建
  useEffect(() => {
    if (!xtermRef.current) return;
    xtermRef.current.options.theme = terminalTheme;
  }, [theme, terminalTheme]);

  // 字体大小 / 行缓冲实时更新：通过 context 驱动，设置页滑块拖动时即时生效
  useEffect(() => {
    if (!xtermRef.current) return;
    if (fontSize !== undefined) {
      xtermRef.current.options.fontSize = fontSize;
    }
    if (bufferLines !== undefined) {
      xtermRef.current.options.scrollback = bufferLines;
    }
    // 字体变化后重新 fit 以适配新的单元格尺寸
    if (fontSize !== undefined) {
      try { fitAddonRef.current?.fit(); } catch { /* ignore */ }
      notifyResize();
    }
  }, [fontSize, bufferLines]);

  // 当标签页变为活跃时重新调整终端尺寸
  // 使用双 rAF 确保 DOM 已完成 opacity 过渡和布局计算
  useEffect(() => {
    if (!isActive || !containerRef.current || !fitAddonRef.current) return;
    let raf1: number;
    let raf2: number;
    raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => {
        try { fitAddonRef.current?.fit(); } catch { /* ignore */ }
        notifyResize();
      });
    });
    return () => {
      cancelAnimationFrame(raf1);
      cancelAnimationFrame(raf2);
    };
  }, [isActive]);

  // 处理粘贴
  const handlePaste = useCallback(async (e: React.ClipboardEvent) => {
    if (!onData || !isConnected) return;
    const text = e.clipboardData.getData("text");
    if (text) onData(text);
  }, [onData, isConnected]);

  // 右键上下文菜单
  // 始终显示自定义菜单（与 isConnected 无关），避免浏览器默认菜单弹出
  const handleContextMenu = useCallback(async (e: React.MouseEvent) => {
    e.preventDefault();
    const { clientX, clientY } = e;

    // 先立刻显示菜单（确保 useMemo 读取最新的 hasSelection() 状态）
    setContextMenu({
      x: clientX,
      y: clientY,
      visible: true,
      session: null,
    });

    // 异步检测剪贴板内容，用于控制「粘贴」菜单项的 disabled 状态
    const text = await readFromClipboard();
    setClipboardHasText(text.length > 0);
  }, []);

  // 关闭右键菜单
  const closeContextMenu = useCallback(() => {
    setContextMenu(prev => ({ ...prev, visible: false }));
  }, []);

  // 构建菜单项：根据 isConnected / selection / clipboard 动态控制 disabled
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
      {
        id: "paste",
        label: t("terminal.paste", "Paste"),
        icon: "paste",
        disabled: !isConnected || !clipboardHasText,
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
  }, [t, isConnected, clipboardHasText, contextMenu]);

  // 菜单项点击处理
  const handleContextMenuSelect = useCallback((itemId: string) => {
    const term = xtermRef.current;
    if (!term) return;

    switch (itemId) {
      case "copy": {
        const selection = term.getSelection();
        if (selection) copyToClipboard(selection);
        break;
      }
      case "paste":
        readFromClipboard().then(text => {
          if (text) {
            term.paste(text);
          }
        }).catch(() => {});
        break;
      case "selectAll":
        term.selectAll();
        break;
      case "search":
        onShowSearchRef.current?.();
        break;
      case "clear":
        term.clear();
        break;
      case "disconnect":
        onDisconnectSessionRef.current?.();
        break;
    }
  }, []);

  return (
    <div className={styles.terminalInstanceWrapper}>
      <div
        ref={containerRef}
        className={styles.terminal}
        onPaste={handlePaste}
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
    </div>
  );
});

export default TerminalInstance;
