import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useTheme } from "../../context/ThemeContext";
import { base64ToBytes, type RttChunk } from "./model";
import styles from "./RttSessionView.module.css";

interface Props {
  chunks: readonly RttChunk[];
  connected: boolean;
  onData: (data: Uint8Array) => void;
}

function readToken(name: string, fallback: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

export default function RttTerminalView({ chunks, connected, onData }: Props) {
  const { theme } = useTheme();
  const hostRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<XTerm | null>(null);
  const lastSequenceRef = useRef(0);
  const chunksRef = useRef(chunks);
  const onDataRef = useRef(onData);
  const connectedRef = useRef(connected);
  chunksRef.current = chunks;
  onDataRef.current = onData;
  connectedRef.current = connected;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const terminal = new XTerm({
      convertEol: true,
      allowTransparency: true,
      cursorBlink: true,
      cursorStyle: "underline",
      scrollback: Number(localStorage.getItem("tauterm-buffer-lines") || "10000"),
      fontSize: Number(localStorage.getItem("tauterm-font-size") || "14"),
      fontFamily: '"JetBrains Mono", "Cascadia Code", "Fira Code", "Consolas", monospace',
      theme: {
        background: "transparent",
        foreground: readToken("--text-primary", "#f1f3f6"),
        cursor: readToken("--accent-primary", "#0b8aff"),
        selectionBackground: "rgba(128, 128, 128, 0.28)",
      },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host);
    terminalRef.current = terminal;

    const input = terminal.onData(data => {
      if (connectedRef.current) onDataRef.current(new TextEncoder().encode(data));
    });
    for (const chunk of chunksRef.current) {
      terminal.write(base64ToBytes(chunk.data_b64));
      lastSequenceRef.current = Math.max(lastSequenceRef.current, chunk.sequence);
    }

    const fitTerminal = () => {
      if (host.clientWidth > 0 && host.clientHeight > 0) {
        try { fit.fit(); } catch { /* hidden pane */ }
      }
    };
    const observer = new ResizeObserver(fitTerminal);
    observer.observe(host);
    requestAnimationFrame(fitTerminal);
    return () => {
      observer.disconnect();
      input.dispose();
      terminal.dispose();
      terminalRef.current = null;
      lastSequenceRef.current = 0;
    };
  }, []);

  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    terminal.options.theme = {
      background: "transparent",
      foreground: readToken("--text-primary", theme === "frosted" ? "#1e293b" : "#f1f3f6"),
      cursor: readToken("--accent-primary", "#0b8aff"),
      selectionBackground: "rgba(128, 128, 128, 0.28)",
    };
  }, [theme]);

  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    for (const chunk of chunks) {
      if (chunk.sequence <= lastSequenceRef.current) continue;
      terminal.write(base64ToBytes(chunk.data_b64));
      lastSequenceRef.current = chunk.sequence;
    }
  }, [chunks]);

  return <div ref={hostRef} className={styles.terminalHost} />;
}
