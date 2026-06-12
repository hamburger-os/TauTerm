import { useEffect, useRef, useCallback, forwardRef, useImperativeHandle } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import styles from "./Terminal.module.css";

interface TerminalInstanceProps {
  /** 会话 ID，用于关联数据和命令 */
  sessionId: string;
  /** 当用户在终端输入时回调 */
  onData?: (data: string) => void;
  /** 是否已连接 */
  isConnected?: boolean;
  /** 当终端就绪时回调 */
  onTermReady?: (writeFn: (data: Uint8Array | string) => void) => void;
}

/**
 * 终端实例组件
 *
 * 每个标签页渲染一个独立的 xterm.js 实例。
 * 接受 sessionId 以区分数据路由。
 */
const TerminalInstance = forwardRef<any, TerminalInstanceProps>(function TerminalInstance(
  { sessionId, onData, isConnected = false, onTermReady },
  ref
) {
  const containerRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);

  // 暴露 direct write 方法
  useImperativeHandle(ref, () => ({
    write: (data: Uint8Array | string) => {
      xtermRef.current?.write(data);
    },
  }));

  // 通知父组件写函数就绪
  useEffect(() => {
    if (xtermRef.current && onTermReady) {
      onTermReady((data: Uint8Array | string) => {
        xtermRef.current?.write(data);
      });
    }
  }, [onTermReady]);

  // 初始化 xterm.js
  useEffect(() => {
    if (!containerRef.current || xtermRef.current) return;

    const term = new XTerm({
      convertEol: true,
      fontSize: 14,
      fontFamily: '"JetBrains Mono", "Cascadia Code", "Fira Code", "Consolas", "Courier New", monospace',
      theme: {
        background: "rgba(0, 0, 0, 0.25)",
        foreground: "#e0e0ff",
        cursor: "#00d4aa",
        cursorAccent: "#060610",
        selectionBackground: "rgba(0, 212, 170, 0.3)",
        black: "#1a1a2e",
        red: "#ff4757",
        green: "#00d4aa",
        yellow: "#ffa502",
        blue: "#00a3ff",
        magenta: "#a855f7",
        cyan: "#00d4aa",
        white: "#e0e0ff",
        brightBlack: "#555577",
        brightRed: "#ff6b81",
        brightGreen: "#00ffcc",
        brightYellow: "#ffbe76",
        brightBlue: "#45aaf2",
        brightMagenta: "#c084fc",
        brightCyan: "#00ffcc",
        brightWhite: "#ffffff",
      },
      cursorBlink: true,
      cursorStyle: "bar",
      allowProposedApi: true,
      scrollback: 5000,
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
    };
  }, []);

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
