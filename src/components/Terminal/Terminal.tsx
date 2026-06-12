import { useEffect, useRef, useCallback } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import styles from "./Terminal.module.css";

interface TerminalProps {
  /** 当用户在终端输入时回调，将数据发送到串口 */
  onData?: (data: string) => void;
  /** 外部写入终端的数据（来自串口） */
  writeData?: Uint8Array | string | null;
  /** 是否已连接串口 */
  isConnected?: boolean;
}

/**
 * 终端仿真组件
 * 集成 xterm.js 并启用 FitAddon 和 WebLinksAddon
 */
export default function Terminal({
  onData,
  writeData,
  isConnected = false,
}: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);

  // 初始化 xterm.js
  useEffect(() => {
    if (!containerRef.current || xtermRef.current) return;

    const term = new XTerm({
      fontSize: 14,
      fontFamily: '"JetBrains Mono", "Cascadia Code", "Fira Code", "Consolas", "Courier New", monospace',
      theme: {
        background: "rgba(0, 0, 0, 0.35)",
        foreground: "#e0e0e0",
        cursor: "#00d4aa",
        cursorAccent: "#0a0a1a",
        selectionBackground: "rgba(0, 212, 170, 0.3)",
        black: "#1a1a2e",
        red: "#ff4757",
        green: "#00d4aa",
        yellow: "#ffa502",
        blue: "#00a3ff",
        magenta: "#a855f7",
        cyan: "#00d4aa",
        white: "#e0e0e0",
        brightBlack: "#555555",
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

    // 处理窗口大小调整
    const handleResize = () => {
      try {
        fitAddon.fit();
      } catch {
        /* ignore */
      }
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

  // 监听串口数据写入终端
  useEffect(() => {
    if (!xtermRef.current || writeData == null) return;
    const term = xtermRef.current;
    if (typeof writeData === "string") {
      term.write(writeData);
    } else {
      term.write(writeData);
    }
  }, [writeData]);

  // 捕获终端输入
  useEffect(() => {
    if (!xtermRef.current || !onData) return;
    const term = xtermRef.current;
    const handler = term.onData(onData);
    return () => {
      handler.dispose();
    };
  }, [onData]);

  // 处理粘贴事件
  const handlePaste = useCallback(
    async (e: React.ClipboardEvent) => {
      if (!onData || !isConnected) return;
      const text = e.clipboardData.getData("text");
      if (text) {
        onData(text);
      }
    },
    [onData, isConnected]
  );

  // 处理右键粘贴
  const handleContextMenu = useCallback(
    async (e: React.MouseEvent) => {
      if (!onData || !isConnected) return;
      try {
        const text = await navigator.clipboard.readText();
        if (text) {
          onData(text);
        }
      } catch {
        /* 剪贴板读取失败 */
      }
      e.preventDefault();
    },
    [onData, isConnected]
  );

  return (
    <div
      ref={containerRef}
      className={styles.terminal}
      onPaste={handlePaste}
      onContextMenu={handleContextMenu}
    />
  );
}
