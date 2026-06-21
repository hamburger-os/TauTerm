import { useEffect, useRef, useCallback, forwardRef, useImperativeHandle } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { useTheme } from "../../context/ThemeContext";
import styles from "./Terminal.module.css";

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
  brightBlack: "#94a3b8",
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
}

/**
 * 终端实例组件
 *
 * 每个标签页渲染一个独立的 xterm.js 实例。
 * 接受 sessionId 以区分数据路由。
 */
const TerminalInstance = forwardRef<any, TerminalInstanceProps>(function TerminalInstance(
  { sessionId, onData, isConnected = false, isActive = true, onTermReady, onCleanup },
  ref
) {
  const containerRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  // 使用 ref 持有最新的回调，避免初始化 effect 中的闭包过期问题
  const onTermReadyRef = useRef(onTermReady);
  onTermReadyRef.current = onTermReady;
  const onCleanupRef = useRef(onCleanup);
  onCleanupRef.current = onCleanup;

  const { theme } = useTheme();
  const isDark = theme === "google-glow" || theme === "obsidian";
  const terminalTheme = isDark ? DARK_TERMINAL_THEME : LIGHT_TERMINAL_THEME;

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

  // 初始化 xterm.js
  useEffect(() => {
    if (!containerRef.current || xtermRef.current) return;

    const term = new XTerm({
      convertEol: true,
      fontSize: Number(localStorage.getItem("tauterm-font-size") || "14"),
      fontFamily: '"JetBrains Mono", "Cascadia Code", "Fira Code", "Consolas", "Courier New", monospace',
      theme: terminalTheme,
      cursorBlink: true,
      cursorStyle: "bar",
      allowProposedApi: true,
      scrollback: 50000,
      cols: 80,
      rows: 24,
    });

    const fitAddon = new FitAddon();
    const webLinksAddon = new WebLinksAddon();

    term.loadAddon(fitAddon);
    term.loadAddon(webLinksAddon);

    term.open(containerRef.current);
    fitAddon.fit();

    xtermRef.current = term;
    fitAddonRef.current = fitAddon;

    // 终端初始化完成后立即注册写函数，不依赖外部重渲染触发
    onTermReadyRef.current?.((data: Uint8Array | string) => {
      term.write(data);
    });

    const handleResize = () => {
      try { fitAddon.fit(); } catch { /* ignore */ }
    };

    const observer = new ResizeObserver(handleResize);
    observer.observe(containerRef.current);

    return () => {
      observer.disconnect();
      term.dispose();
      xtermRef.current = null;
      fitAddonRef.current = null;
      // 通知父组件清理此会话的 writeRefs 条目
      onCleanupRef.current?.(sessionId);
    };
  }, []);

  // 主题变化时动态更新终端配色，无需销毁重建
  useEffect(() => {
    if (!xtermRef.current) return;
    xtermRef.current.options.theme = terminalTheme;
  }, [theme, terminalTheme]);

  // 当标签页变为活跃时重新调整终端尺寸
  // 使用双 rAF 确保 DOM 已完成 opacity 过渡和布局计算
  useEffect(() => {
    if (!isActive || !containerRef.current || !fitAddonRef.current) return;
    let raf1: number;
    let raf2: number;
    raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => {
        try { fitAddonRef.current?.fit(); } catch { /* ignore */ }
      });
    });
    return () => {
      cancelAnimationFrame(raf1);
      cancelAnimationFrame(raf2);
    };
  }, [isActive]);

  // 捕获终端输入
  useEffect(() => {
    if (!xtermRef.current || !onData) return;
    const term = xtermRef.current;
    const handler = term.onData(onData);
    return () => { handler.dispose(); };
  }, [onData, sessionId]);

  // 处理粘贴
  const handlePaste = useCallback(async (e: React.ClipboardEvent) => {
    if (!onData || !isConnected) return;
    const text = e.clipboardData.getData("text");
    if (text) onData(text);
  }, [onData, isConnected]);

  // 右键粘贴
  const handleContextMenu = useCallback(async (e: React.MouseEvent) => {
    if (!onData || !isConnected) return;
    try {
      const text = await navigator.clipboard.readText();
      if (text) onData(text);
    } catch { /* clipboard read failed */ }
    e.preventDefault();
  }, [onData, isConnected]);

  return (
    <div
      ref={containerRef}
      className={styles.terminal}
      onPaste={handlePaste}
      onContextMenu={handleContextMenu}
    />
  );
});

export default TerminalInstance;
