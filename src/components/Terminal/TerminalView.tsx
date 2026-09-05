import { useEffect, useRef, useCallback, useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import { useSession } from "../../context/SessionContext";
import { useTheme } from "../../context/ThemeContext";
import { useKeyboard } from "../../hooks/useKeyboard";
import { ACTION_IDS } from "../../shortcuts/actionIds";
import { PendingSessionData } from "../../core/pending-session-data";
import type { PaneRect } from "../../core/split-layout";
import Icon from "../common/Icon";
import TerminalInstance from "./Terminal";
import DualPane from "./DualPane";
import type { DualLine } from "./DualPane";
import SearchBar from "./SearchBar";
import styles from "./Terminal.module.css";

const BYTES_PER_LINE = 16;
const HEX_HALF_WIDTH = 8 * 3 - 1; // 8 字节 hex 列最大宽度 (8×2 位 + 7 个空格 = 23)
const FULL_DOCKED_RECT: PaneRect = { left: 0, top: 0, width: 1, height: 1 };

/** Dual 模式帧超时默认值：50ms 内未收到新数据则视为一帧结束，可在连接配置中覆盖 */
const DUAL_FRAME_TIMEOUT_DEFAULT_MS = 50;

interface TerminalViewProps {
  /** 分屏模式下：sessionId → 归一化 Pane 几何。缺省时保持原单视图行为。 */
  dockedPlacements?: Record<string, PaneRect>;
  /** Split Workspace 为每个终端计算的真实外轮廓圆角；内部交点必须保持直角。 */
  dockedBorderRadii?: Record<string, string>;
  /** Split Workspace 的 Pane 总数；单 Pane 时不绘制 active/inactive 材质差异。 */
  paneCount?: number;
  /** 用户在可见终端内操作时，请求选中其所属 Pane。 */
  onActivateSession?: (sessionId: string) => void;
}

function dockedRectStyle(rect: PaneRect): React.CSSProperties {
  return {
    left: `${rect.left * 100}%`,
    top: `${rect.top * 100}%`,
    width: `${rect.width * 100}%`,
    height: `${rect.height * 100}%`,
  };
}

/** 拼接多个 Uint8Array */
function concatBytes(chunks: Uint8Array[]): Uint8Array {
  const totalLen = chunks.reduce((s, c) => s + c.length, 0);
  const out = new Uint8Array(totalLen);
  let off = 0;
  for (const c of chunks) { out.set(c, off); off += c.length; }
  return out;
}

/**
 * 格式化单行 Hex Dump（最多 16 字节）
 *
 * 固定宽度 78 字符，保证多次输出 `\r` 覆盖对齐。
 * 格式：8位偏移量 + 2空格 + 左8字节hex + 2空格 + 右8字节hex + 2空格 + |ASCII|
 */
function formatHexLine(data: Uint8Array, offset: number): string {
  const chunk = data.slice(0, BYTES_PER_LINE);
  const offsetStr = offset.toString(16).padStart(8, "0");

  const leftHexParts: string[] = [];
  for (let j = 0; j < 8 && j < chunk.length; j++) {
    leftHexParts.push(chunk[j].toString(16).padStart(2, "0"));
  }
  const leftHex = leftHexParts.join(" ").padEnd(HEX_HALF_WIDTH, " ");

  const rightHexParts: string[] = [];
  for (let j = 8; j < BYTES_PER_LINE && j < chunk.length; j++) {
    rightHexParts.push(chunk[j].toString(16).padStart(2, "0"));
  }
  const rightHex = rightHexParts.join(" ").padEnd(HEX_HALF_WIDTH, " ");
  const hex = `${leftHex}  ${rightHex}`;

  const asciiParts: string[] = [];
  for (let j = 0; j < BYTES_PER_LINE; j++) {
    if (j < chunk.length) {
      const b = chunk[j];
      asciiParts.push((b >= 32 && b <= 126) ? String.fromCharCode(b) : ".");
    } else {
      asciiParts.push(" ");
    }
  }
  const ascii = asciiParts.join("");

  return `${offsetStr}  ${hex}  |${ascii}|`;
}

/** 将原始字节数据转换为 DualLine 基础结构（id 由调用方分配以保证单调递增） */
function dataToDualLine(data: Uint8Array, direction: "RX" | "TX"): Omit<DualLine, "id"> {
  const now = new Date();
  const hh = String(now.getHours()).padStart(2, "0");
  const mm = String(now.getMinutes()).padStart(2, "0");
  const ss = String(now.getSeconds()).padStart(2, "0");
  const ms = String(now.getMilliseconds()).padStart(3, "0");
  const timestamp = `${hh}:${mm}:${ss}.${ms}`;

  let cleanLen = data.length;
  while (cleanLen > 0) {
    const b = data[cleanLen - 1];
    if (b === 0x0d || b === 0x0a) cleanLen--;
    else break;
  }
  const cleanData = cleanLen === data.length ? data : data.slice(0, cleanLen);

  const textParts: string[] = [];
  for (let i = 0; i < cleanData.length; i++) {
    const b = cleanData[i];
    if (b === 0x0d) { textParts.push("␍"); continue; }
    if (b === 0x0a) { textParts.push("␊"); continue; }
    if (b >= 32 && b <= 126) textParts.push(String.fromCharCode(b));
    else textParts.push(".");
  }

  const hexParts: string[] = [];
  for (let i = 0; i < cleanData.length; i++) {
    if (i > 0) hexParts.push(" ");
    if (i > 0 && i % 8 === 0) hexParts.push(" ");
    hexParts.push(cleanData[i].toString(16).padStart(2, "0"));
  }

  return { timestamp, direction, text: textParts.join(""), hex: hexParts.join("") };
}

function normalizeDecodedText(text: string): string {
  let end = text.length;
  while (end > 0) {
    const c = text.charCodeAt(end - 1);
    if (c !== 0x0d && c !== 0x0a) break;
    end--;
  }
  let out = "";
  for (let i = 0; i < end; i++) {
    const c = text.charCodeAt(i);
    if (c === 0x0d) out += "␍";
    else if (c === 0x0a) out += "␊";
    else if (c < 0x20) out += ".";
    else out += text[i];
  }
  return out;
}

/**
 * 终端区域管理器。
 *
 * 单视图模式沿用原来的 opacity 切换；分屏模式下所有终端实例仍只有一份，
 * 通过归一化几何放置到多个 Pane。未被放入 Pane 的终端保持隐藏挂载并保留
 * 最后一次尺寸，避免切换 Pane 导致 xterm dispose / scrollback 丢失 / PTY resize 到 0。
 */
export default function TerminalView({
  dockedPlacements,
  dockedBorderRadii,
  paneCount = 1,
  onActivateSession,
}: TerminalViewProps = {}) {
  const { t } = useTranslation();
  const { state, sendData, disconnect, closeChannel, onSessionData, onDataSent } = useSession();
  const { fontSize, bufferLines } = useTheme();
  const { registerAction } = useKeyboard();
  const writeRefs = useRef<Map<string, (data: Uint8Array | string) => void>>(new Map());
  const terminalRefs = useRef<Map<string, any>>(new Map());
  const terminalViewportRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const lastDockedRectsRef = useRef<Map<string, PaneRect>>(new Map());
  const dockedViewportRef = useRef<HTMLDivElement | null>(null);
  const [searchVisible, setSearchVisible] = useState(false);
  const searchStateRef = useRef<Map<string, { query: string; caseSensitive: boolean }>>(new Map());
  const currentSearchState = state.activeTabId ? (searchStateRef.current.get(state.activeTabId) ?? { query: "", caseSensitive: false }) : { query: "", caseSensitive: false };

  const handleSearchClose = useCallback(() => {
    setSearchVisible(false);
    setTimeout(() => {
      activeTermRef.current?.terminal?.focus();
    }, 0);
  }, []);

  const handleSearchSaveState = useCallback((query: string, caseSensitive: boolean) => {
    if (state.activeTabId) searchStateRef.current.set(state.activeTabId, { query, caseSensitive });
  }, [state.activeTabId]);
  const activeTermRef = useRef<any>(null);
  const hexOffsetsRef = useRef<Map<string, number>>(new Map());
  const hexPendingRef = useRef<Map<string, { offset: number; data: Uint8Array }>>(new Map());
  const frameBufRef = useRef<Map<string, { buffer: Uint8Array[]; timer: ReturnType<typeof setTimeout> | null; generation: number }>>(new Map());
  const [dualLines, setDualLines] = useState<Map<string, DualLine[]>>(new Map());
  const pendingDualRef = useRef<Map<string, DualLine[]>>(new Map());
  const pendingTextRef = useRef<Map<string, string[]>>(new Map());
  const pendingStartupDataRef = useRef(new PendingSessionData());
  const sessionDataHandlerRef = useRef<(sessionId: string, data: Uint8Array) => void>(() => {});
  const textRafIdRef = useRef<number | null>(null);
  const decodersRef = useRef<Map<string, { label: string; decoder: TextDecoder }>>(new Map());
  const rafIdRef = useRef<number | null>(null);
  const lineIdCounterRef = useRef<Map<string, number>>(new Map());
  const dualLinesKeysRef = useRef<Set<string>>(new Set());
  const bufferLinesRef = useRef(bufferLines);
  bufferLinesRef.current = bufferLines;
  const tabsRef = useRef(state.tabs);
  const viewportRef = useRef<HTMLDivElement>(null);
  tabsRef.current = state.tabs;

  const terminalTabs = state.tabs.filter(
    t => t.state === "connected" || t.state === "transferring" || t.disconnectInfo?.retain_terminal
  );
  const activeTab = state.tabs.find(t => t.id === state.activeTabId);

  const connectedTabIds = useMemo(
    () => state.tabs
      .filter(t => t.state === "connected" || t.state === "transferring")
      .concat(state.tabs.filter(t => t.state === "disconnected" && t.disconnectInfo?.retain_terminal))
      .map(t => t.id)
      .sort()
      .join(","),
    [state.tabs]
  );

  useEffect(() => {
    activeTermRef.current = state.activeTabId
      ? terminalRefs.current.get(state.activeTabId) ?? null
      : null;
    dockedViewportRef.current = state.activeTabId
      ? terminalViewportRefs.current.get(state.activeTabId) ?? null
      : null;
  }, [state.activeTabId, dockedPlacements]);

  useEffect(() => {
    registerAction(ACTION_IDS.TERMINAL_SEARCH, () => setSearchVisible(v => !v));
  }, [registerAction]);

  useEffect(() => {
    registerAction(ACTION_IDS.TERMINAL_SELECT_ALL, () => {
      activeTermRef.current?.terminal?.selectAll();
    });
  }, [registerAction]);

  useEffect(() => {
    setSearchVisible(false);
  }, [state.activeTabId]);

  useEffect(() => {
    return () => {
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }
      if (textRafIdRef.current !== null) {
        cancelAnimationFrame(textRafIdRef.current);
        textRafIdRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    dualLinesKeysRef.current = new Set(dualLines.keys());
  }, [dualLines]);

  const processIncomingFrame = useCallback((
    sessionId: string,
    data: Uint8Array,
    frameTimeoutMs: number,
    onFrame: (frameData: Uint8Array) => void,
  ) => {
    const frame = frameBufRef.current.get(sessionId);
    if (frame?.timer) clearTimeout(frame.timer);
    const generation = (frame?.generation ?? 0) + 1;
    const chunks = frame?.buffer ? [...frame.buffer, data] : [data];
    let remaining = concatBytes(chunks);

    while (remaining.length > 0) {
      let delimIdx = -1;
      let delimLen = 0;
      for (let i = 0; i < remaining.length; i++) {
        if (remaining[i] === 0x0d && i + 1 < remaining.length && remaining[i + 1] === 0x0a) {
          delimIdx = i; delimLen = 2; break;
        }
        if (remaining[i] === 0x0a) { delimIdx = i; delimLen = 1; break; }
        if (remaining[i] === 0x0d) { delimIdx = i; delimLen = 1; break; }
      }
      if (delimIdx < 0) break;
      const lineEnd = delimIdx + delimLen;
      const lineData = remaining.slice(0, lineEnd);
      remaining = remaining.slice(lineEnd);
      onFrame(lineData);
    }

    if (remaining.length > 0) {
      const capturedGen = generation;
      const timer = setTimeout(() => {
        const f = frameBufRef.current.get(sessionId);
        if (f && f.generation === capturedGen && f.buffer.length > 0) {
          const all = concatBytes(f.buffer);
          if (all.length > 0) onFrame(all);
        }
        if (f && f.generation === capturedGen) frameBufRef.current.delete(sessionId);
      }, frameTimeoutMs);
      frameBufRef.current.set(sessionId, { buffer: [remaining], timer, generation });
    } else {
      frameBufRef.current.delete(sessionId);
    }
  }, []);

  const flushDualLines = useCallback(() => {
    rafIdRef.current = null;
    const pending = pendingDualRef.current;
    if (pending.size === 0) return;
    const snapshot = new Map(pending);
    pending.clear();

    setDualLines(prev => {
      const next = new Map(prev);
      const maxLines = bufferLinesRef.current;
      for (const [sessionId, newLines] of snapshot) {
        const lines = [...(prev.get(sessionId) ?? []), ...newLines];
        if (lines.length > maxLines) lines.splice(0, lines.length - maxLines);
        next.set(sessionId, lines);
      }
      return next;
    });
  }, []);

  const flushTextBuffers = useCallback(() => {
    textRafIdRef.current = null;
    const pending = pendingTextRef.current;
    if (pending.size === 0) return;
    const snapshot = new Map(pending);
    pending.clear();

    for (const [sessionId, chunks] of snapshot) {
      const writeFn = writeRefs.current.get(sessionId);
      if (!writeFn) continue;
      const merged = chunks.join("");
      if (merged.length === 0) continue;
      writeFn(merged);
    }
  }, []);

  const pushDualLine = useCallback((
    sessionId: string,
    direction: "RX" | "TX",
    data: Uint8Array,
    decodedText?: string,
  ) => {
    if (data.length === 0) return;
    const base = decodedText !== undefined
      ? { ...dataToDualLine(data, direction), text: normalizeDecodedText(decodedText) }
      : dataToDualLine(data, direction);
    if (base.text.length === 0 && base.hex.length === 0) return;

    const lineId = (lineIdCounterRef.current.get(sessionId) ?? 0) + 1;
    lineIdCounterRef.current.set(sessionId, lineId);
    const newLine: DualLine = { ...base, id: lineId };
    const pending = pendingDualRef.current;
    if (!pending.has(sessionId)) pending.set(sessionId, []);
    pending.get(sessionId)!.push(newLine);
    if (rafIdRef.current === null) rafIdRef.current = requestAnimationFrame(flushDualLines);
  }, [flushDualLines]);

  const getSessionDecoder = useCallback((sessionId: string): TextDecoder => {
    const tab = tabsRef.current.find(t => t.id === sessionId);
    const label = typeof tab?.params?.encoding === "string" ? tab.params.encoding : "utf-8";
    const cached = decodersRef.current.get(sessionId);
    if (cached && cached.label === label) return cached.decoder;
    let decoder: TextDecoder;
    try { decoder = new TextDecoder(label); }
    catch { decoder = new TextDecoder("utf-8"); }
    decodersRef.current.set(sessionId, { label, decoder });
    return decoder;
  }, []);

  useEffect(() => {
    const handleSessionData = (sessionId: string, data: Uint8Array) => {
      const tab = tabsRef.current.find(t => t.id === sessionId);
      if (!tab) {
        pendingStartupDataRef.current.push(sessionId, data);
        return;
      }

      const isDual = tab.params?.data_mode === "dual";
      const writeFn = writeRefs.current.get(sessionId);
      if (!isDual && !writeFn) {
        pendingStartupDataRef.current.push(sessionId, data);
        return;
      }

      if (tab.params?.data_mode === "hex") {
        const pending = hexPendingRef.current.get(sessionId);
        const prev = pending?.data ?? new Uint8Array(0);
        const baseOffset = pending?.offset ?? (hexOffsetsRef.current.get(sessionId) ?? 0);
        const prevHadPending = prev.length > 0;
        const combined = new Uint8Array(prev.length + data.length);
        combined.set(prev);
        combined.set(data, prev.length);
        let pos = 0;
        let curOffset = baseOffset;
        const w = writeFn!;

        while (pos + BYTES_PER_LINE <= combined.length) {
          const line = formatHexLine(combined.slice(pos, pos + BYTES_PER_LINE), curOffset);
          if (pos === 0 && prevHadPending) w("\r" + line + "\r\n");
          else w(line + "\r\n");
          pos += BYTES_PER_LINE;
          curOffset += BYTES_PER_LINE;
        }

        hexOffsetsRef.current.set(sessionId, curOffset);
        const remainder = combined.slice(pos);
        if (remainder.length > 0) {
          const line = formatHexLine(remainder, curOffset);
          w("\r" + line);
          hexPendingRef.current.set(sessionId, { offset: curOffset, data: remainder });
        } else {
          hexPendingRef.current.delete(sessionId);
        }
      } else if (tab.params?.data_mode === "dual") {
        const timeout = typeof tab.params?.dual_frame_timeout_ms === "number"
          ? tab.params.dual_frame_timeout_ms
          : DUAL_FRAME_TIMEOUT_DEFAULT_MS;
        processIncomingFrame(sessionId, data, timeout, (frameData) => {
          const decoded = getSessionDecoder(sessionId).decode(frameData, { stream: true });
          pushDualLine(sessionId, "RX", frameData, decoded);
        });
      } else {
        const text = getSessionDecoder(sessionId).decode(data, { stream: true });
        const pending = pendingTextRef.current;
        if (!pending.has(sessionId)) pending.set(sessionId, []);
        pending.get(sessionId)!.push(text);
        if (textRafIdRef.current === null) textRafIdRef.current = requestAnimationFrame(flushTextBuffers);
      }
    };
    sessionDataHandlerRef.current = handleSessionData;
    onSessionData(handleSessionData);
    return () => {
      sessionDataHandlerRef.current = () => {};
      onSessionData(() => {});
    };
  }, [onSessionData, pushDualLine, processIncomingFrame, flushTextBuffers, getSessionDecoder]);

  useEffect(() => {
    for (const tab of state.tabs) {
      if (tab.params?.data_mode !== "dual") continue;
      for (const chunk of pendingStartupDataRef.current.drain(tab.id)) {
        sessionDataHandlerRef.current(tab.id, chunk);
      }
    }
  }, [state.tabs]);

  const flushSessionDecoder = useCallback((sessionId: string) => {
    const entry = decodersRef.current.get(sessionId);
    if (entry) {
      try { entry.decoder.decode(); } catch { /* ignore */ }
      decodersRef.current.delete(sessionId);
    }
  }, []);

  useEffect(() => {
    onDataSent((sessionId, data) => {
      const tab = tabsRef.current.find(t => t.id === sessionId);
      if (!tab) return;
      if (tab.params?.data_mode === "dual") {
        const label = typeof tab?.params?.encoding === "string" ? tab.params.encoding : "utf-8";
        let decoded: string;
        try { decoded = new TextDecoder(label).decode(data); }
        catch { decoded = new TextDecoder("utf-8").decode(data); }
        pushDualLine(sessionId, "TX", data, decoded);
        return;
      }
      if (tab.localEcho) {
        const writeFn = writeRefs.current.get(sessionId);
        if (writeFn) {
          const label = typeof tab?.params?.encoding === "string" ? tab.params.encoding : "utf-8";
          let echoText: string;
          try { echoText = new TextDecoder(label).decode(data); }
          catch { echoText = new TextDecoder("utf-8").decode(data); }
          writeFn(echoText);
        }
      }
    });
    return () => { onDataSent(() => {}); };
  }, [onDataSent, pushDualLine]);

  useEffect(() => {
    const connectedIds = new Set(
      connectedTabIds ? connectedTabIds.split(",").filter(Boolean) : []
    );
    const toRemove: string[] = [];
    writeRefs.current.forEach((_, id) => {
      if (!connectedIds.has(id)) toRemove.push(id);
    });
    const dualDisconnected: string[] = [];
    for (const id of dualLinesKeysRef.current) {
      if (!connectedIds.has(id) && !toRemove.includes(id)) dualDisconnected.push(id);
    }

    for (const id of toRemove) {
      writeRefs.current.delete(id);
      terminalRefs.current.delete(id);
      terminalViewportRefs.current.delete(id);
      lastDockedRectsRef.current.delete(id);
      hexOffsetsRef.current.delete(id);
      hexPendingRef.current.delete(id);
      const fb = frameBufRef.current.get(id);
      if (fb?.timer) clearTimeout(fb.timer);
      frameBufRef.current.delete(id);
      pendingDualRef.current.delete(id);
      lineIdCounterRef.current.delete(id);
      flushSessionDecoder(id);
    }
    for (const id of dualDisconnected) {
      terminalViewportRefs.current.delete(id);
      lastDockedRectsRef.current.delete(id);
      const fb = frameBufRef.current.get(id);
      if (fb?.timer) clearTimeout(fb.timer);
      frameBufRef.current.delete(id);
      pendingDualRef.current.delete(id);
      lineIdCounterRef.current.delete(id);
      flushSessionDecoder(id);
    }

    if (toRemove.length > 0 || dualDisconnected.length > 0) {
      setDualLines(prev => {
        const next = new Map(prev);
        for (const id of toRemove) next.delete(id);
        for (const id of dualDisconnected) next.delete(id);
        return next;
      });
    }
  }, [connectedTabIds, flushSessionDecoder]);

  const handleTermReady = useCallback((sessionId: string, writeFn: (data: Uint8Array | string) => void) => {
    writeRefs.current.set(sessionId, writeFn);
    for (const chunk of pendingStartupDataRef.current.drain(sessionId)) {
      sessionDataHandlerRef.current(sessionId, chunk);
    }
  }, []);

  const handleTermCleanup = useCallback((sessionId: string) => {
    writeRefs.current.delete(sessionId);
    terminalViewportRefs.current.delete(sessionId);
    lastDockedRectsRef.current.delete(sessionId);
    hexOffsetsRef.current.delete(sessionId);
    hexPendingRef.current.delete(sessionId);
    const fb = frameBufRef.current.get(sessionId);
    if (fb?.timer) clearTimeout(fb.timer);
    frameBufRef.current.delete(sessionId);
    pendingDualRef.current.delete(sessionId);
    pendingTextRef.current.delete(sessionId);
    pendingStartupDataRef.current.delete(sessionId);
    lineIdCounterRef.current.delete(sessionId);
    flushSessionDecoder(sessionId);
    setDualLines(prev => {
      const next = new Map(prev);
      next.delete(sessionId);
      return next;
    });
  }, [flushSessionDecoder]);

  const handleData = useCallback((sessionId: string, data: string) => {
    sendData(sessionId, data);
  }, [sendData]);

  const renderTerminalBody = (tab: (typeof terminalTabs)[number], isActive: boolean) => {
    const isDual = tab.params?.data_mode === "dual";
    return isDual ? (
      <DualPane
        key={tab.id}
        lines={dualLines.get(tab.id) ?? []}
        fontSize={fontSize}
        bufferLines={bufferLines}
      />
    ) : (
      <TerminalInstance
        sessionId={tab.id}
        onData={(data) => handleData(tab.id, data)}
        isConnected={tab.state === "connected" || tab.state === "transferring"}
        isActive={isActive}
        onTermReady={(writeFn) => handleTermReady(tab.id, writeFn)}
        onCleanup={handleTermCleanup}
        fontSize={fontSize}
        bufferLines={bufferLines}
        onShowSearch={() => setSearchVisible(true)}
        onDisconnectSession={() => {
          if (tab.parentId) {
            void closeChannel(tab.id, tab.parentId);
          } else {
            void disconnect(tab.id);
          }
        }}
        ref={(node) => {
          if (node) terminalRefs.current.set(tab.id, node);
          else terminalRefs.current.delete(tab.id);
        }}
      />
    );
  };

  if (dockedPlacements) {
    return (
      <div className={styles.dockedRoot}>
        {terminalTabs.map(tab => {
          const placedRect = dockedPlacements[tab.id];
          if (placedRect) lastDockedRectsRef.current.set(tab.id, placedRect);
          const rect = placedRect ?? lastDockedRectsRef.current.get(tab.id) ?? FULL_DOCKED_RECT;
          const visible = Boolean(placedRect);
          const isActive = tab.id === state.activeTabId;
          const paneMaterial = paneCount > 1
            ? (isActive ? "liquid-glass-content-active" : "liquid-glass-content-inactive")
            : "";
          const retainedDisconnect = tab.state === "disconnected" && tab.disconnectInfo?.retain_terminal
            ? tab.disconnectInfo
            : null;

          return (
            <div
              key={tab.id}
              ref={(node) => {
                if (node) terminalViewportRefs.current.set(tab.id, node);
                else terminalViewportRefs.current.delete(tab.id);
              }}
              className={`${styles.terminalWrapper} ${styles.dockedTerminalWrapper} liquid-glass-content ${paneMaterial}`}
              style={{
                ...dockedRectStyle(rect),
                borderRadius: dockedBorderRadii?.[tab.id] ?? "0",
                opacity: visible ? 1 : 0,
                visibility: visible ? "visible" : "hidden",
                pointerEvents: visible ? "auto" : "none",
              }}
              onMouseDownCapture={() => {
                if (visible) onActivateSession?.(tab.id);
              }}
            >
              {tab.state === "transferring" && (
                <div className={styles.transferBanner}>
                  <Icon name="transfer-active" size="sm" className={styles.transferBannerIcon} />
                  <span>{t("transfer.transferringBanner", "File transfer in progress...")}</span>
                </div>
              )}
              {retainedDisconnect && (
                <div className={`${styles.disconnectBanner} liquid-glass-alert-banner`}>
                  <Icon name="warning" size="sm" />
                  <span>
                    {t("localShell.unexpectedExit")}: {retainedDisconnect.reason}
                    {typeof retainedDisconnect.exit_code === "number"
                      ? ` (${t("localShell.exitCode", { code: retainedDisconnect.exit_code })})`
                      : ""}
                  </span>
                </div>
              )}
              <div className={styles.dockedTerminalBody}>
                {renderTerminalBody(tab, isActive)}
              </div>
            </div>
          );
        })}

        <SearchBar
          isOpen={searchVisible}
          onClose={handleSearchClose}
          terminal={activeTermRef.current?.terminal ?? null}
          savedQuery={currentSearchState.query}
          savedCaseSensitive={currentSearchState.caseSensitive}
          onSaveState={handleSearchSaveState}
          viewportRef={dockedViewportRef}
        />
      </div>
    );
  }

  const isActiveTransferring = activeTab?.state === "transferring";
  const retainedDisconnect = activeTab?.state === "disconnected" && activeTab.disconnectInfo?.retain_terminal
    ? activeTab.disconnectInfo
    : null;

  return (
    <div className={styles.viewport} ref={viewportRef}>
      <div className={`${styles.terminalArea} liquid-glass-content`}>
        {isActiveTransferring && (
          <motion.div
            className={styles.transferBanner}
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, y: 0 }}
          >
            <Icon name="transfer-active" size="sm" className={styles.transferBannerIcon} />
            <span>{t("transfer.transferringBanner", "File transfer in progress...")}</span>
          </motion.div>
        )}

        {retainedDisconnect && (
          <motion.div
            className={`${styles.disconnectBanner} liquid-glass-alert-banner`}
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, y: 0 }}
          >
            <Icon name="warning" size="sm" />
            <span>
              {t("localShell.unexpectedExit")}: {retainedDisconnect.reason}
              {typeof retainedDisconnect.exit_code === "number"
                ? ` (${t("localShell.exitCode", { code: retainedDisconnect.exit_code })})`
                : ""}
            </span>
          </motion.div>
        )}

        <div className={styles.terminalsContainer}>
          {terminalTabs.map(tab => {
            const isActive = tab.id === state.activeTabId;
            return (
              <div
                key={tab.id}
                className={`${styles.terminalWrapper} ${styles.stackedTerminalWrapper} ${isActive ? styles.stackedTerminalActive : ""}`}
                aria-hidden={!isActive}
              >
                {renderTerminalBody(tab, isActive)}
              </div>
            );
          })}

          {terminalTabs.length === 0 && (
            <div className={styles.emptyState}>
              <Icon name="logo" size="2xl" className={styles.emptyIcon} />
              <div>{t("session.noSessions")}</div>
              <div className={styles.emptyHint}>
                {t("session.emptyHint") || "Use Ctrl+Shift+N to create a new session"}
              </div>
            </div>
          )}
        </div>
      </div>

      <SearchBar
        isOpen={searchVisible}
        onClose={handleSearchClose}
        terminal={activeTermRef.current?.terminal ?? null}
        savedQuery={currentSearchState.query}
        savedCaseSensitive={currentSearchState.caseSensitive}
        onSaveState={handleSearchSaveState}
        viewportRef={viewportRef}
      />
    </div>
  );
}
