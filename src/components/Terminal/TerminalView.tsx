import { useEffect, useRef, useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { AnimatePresence, motion } from "framer-motion";
import { useSession } from "../../context/SessionContext";
import { useKeyboard } from "../../hooks/useKeyboard";
import { ACTION_IDS } from "../../shortcuts/actionIds";
import TerminalInstance from "./Terminal";
import SearchBar from "./SearchBar";
import styles from "./Terminal.module.css";

const BYTES_PER_LINE = 16;
const HEX_HALF_WIDTH = 8 * 3 - 1; // 8 字节 hex 列最大宽度 (8×2 位 + 7 个空格 = 23)

/**
 * 格式化单行 Hex Dump（最多 16 字节）
 *
 * 固定宽度 78 字符，保证多次输出 `\r` 覆盖对齐。
 * 格式：8位偏移量 + 2空格 + 左8字节hex + 2空格 + 右8字节hex + 2空格 + |ASCII|
 */
function formatHexLine(data: Uint8Array, offset: number): string {
  const chunk = data.slice(0, BYTES_PER_LINE);
  const offsetStr = offset.toString(16).padStart(8, "0");

  // Hex 列：前 8 字节
  const leftHexParts: string[] = [];
  for (let j = 0; j < 8 && j < chunk.length; j++) {
    leftHexParts.push(chunk[j].toString(16).padStart(2, "0"));
  }
  const leftHex = leftHexParts.join(" ").padEnd(HEX_HALF_WIDTH, " ");

  // Hex 列：后 8 字节
  const rightHexParts: string[] = [];
  for (let j = 8; j < BYTES_PER_LINE && j < chunk.length; j++) {
    rightHexParts.push(chunk[j].toString(16).padStart(2, "0"));
  }
  const rightHex = rightHexParts.join(" ").padEnd(HEX_HALF_WIDTH, " ");

  // 第 8/9 字节之间额外空格
  const hex = `${leftHex}  ${rightHex}`;

  // ASCII 列：固定 16 字符宽，不足补空格
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

/**
 * 终端区域管理器
 *
 * 同时渲染所有已连接标签页的终端实例，使用 CSS opacity 控制可见性。
 * 非活跃标签页的终端保持在 DOM 中并继续接收数据，切换时无需重建。
 */
export default function TerminalView() {
  const { t } = useTranslation();
  const { state, sendData, onSessionData } = useSession();
  const { registerAction } = useKeyboard();
  const writeRefs = useRef<Map<string, (data: Uint8Array | string) => void>>(new Map());
  const terminalRefs = useRef<Map<string, any>>(new Map());
  const [searchVisible, setSearchVisible] = useState(false);
  const activeTermRef = useRef<any>(null);
  const hexOffsetsRef = useRef<Map<string, number>>(new Map());
  /** HEX 模式待补行缓冲区：存不够 16 字节的剩余字节，凑满 16 后输出完整行并换行 */
  const hexPendingRef = useRef<Map<string, { offset: number; data: Uint8Array }>>(new Map());
  const tabsRef = useRef(state.tabs);
  tabsRef.current = state.tabs;

  // 所有已连接的标签页（需要保持终端实例存活）
  const connectedTabs = state.tabs.filter(
    t => t.state === "connected" || t.state === "transferring"
  );
  const activeTab = state.tabs.find(t => t.id === state.activeTabId);

  // 同步 activeTermRef 指向当前活跃标签页的终端引用
  useEffect(() => {
    activeTermRef.current = state.activeTabId
      ? terminalRefs.current.get(state.activeTabId) ?? null
      : null;
  }, [state.activeTabId]);

  // 注册 Ctrl+F 搜索快捷键
  useEffect(() => {
    registerAction(ACTION_IDS.TERMINAL_SEARCH, () => setSearchVisible(v => !v));
  }, [registerAction]);

  // 注册数据回调，将每个 session 的数据路由到对应终端
  useEffect(() => {
    onSessionData((sessionId, data) => {
      const tab = tabsRef.current.find(t => t.id === sessionId);
      const writeFn = writeRefs.current.get(sessionId);
      if (!writeFn) return;

      if (tab?.params?.data_mode === "hex") {
        // ── HEX 模式：逐行实时更新 ──
        // 取出上一轮未满 16 字节的待补数据，与新数据拼接
        const pending = hexPendingRef.current.get(sessionId);
        const prev = pending?.data ?? new Uint8Array(0);
        const baseOffset = pending?.offset ?? (hexOffsetsRef.current.get(sessionId) ?? 0);
        const prevHadPending = prev.length > 0;

        const combined = new Uint8Array(prev.length + data.length);
        combined.set(prev);
        combined.set(data, prev.length);

        let pos = 0;
        let curOffset = baseOffset;

        // 输出所有已满 16 字节的行
        while (pos + BYTES_PER_LINE <= combined.length) {
          const line = formatHexLine(combined.slice(pos, pos + BYTES_PER_LINE), curOffset);
          if (pos === 0 && prevHadPending) {
            // 替换上一轮显示的待补行：\r 回行首，写入完整行，\r\n 换行
            writeFn("\r" + line + "\r\n");
          } else {
            writeFn(line + "\r\n");
          }
          pos += BYTES_PER_LINE;
          curOffset += BYTES_PER_LINE;
        }

        hexOffsetsRef.current.set(sessionId, curOffset);

        // 不满 16 字节的剩余：先输出（无换行），存起来等下一轮拼接后覆盖
        const remainder = combined.slice(pos);
        if (remainder.length > 0) {
          const line = formatHexLine(remainder, curOffset);
          writeFn("\r" + line); // \r 回到行首准备覆盖，无尾随 \n 所以光标在该行末尾
          hexPendingRef.current.set(sessionId, { offset: curOffset, data: remainder });
        } else {
          hexPendingRef.current.delete(sessionId);
        }
      } else {
        writeFn(data);
      }
    });
  }, [onSessionData]);

  // 清理已断开/已删除会话的 writeRefs 和 terminalRefs 条目
  useEffect(() => {
    const connectedIds = new Set(connectedTabs.map(t => t.id));
    const toRemove: string[] = [];
    writeRefs.current.forEach((_, id) => {
      if (!connectedIds.has(id)) {
        toRemove.push(id);
      }
    });
    for (const id of toRemove) {
      writeRefs.current.delete(id);
      terminalRefs.current.delete(id);
      hexOffsetsRef.current.delete(id);
      hexPendingRef.current.delete(id);
    }
  }, [connectedTabs]);

  const handleTermReady = useCallback((sessionId: string, writeFn: (data: Uint8Array | string) => void) => {
    writeRefs.current.set(sessionId, writeFn);
  }, []);

  const handleTermCleanup = useCallback((sessionId: string) => {
    writeRefs.current.delete(sessionId);
    hexOffsetsRef.current.delete(sessionId);
    hexPendingRef.current.delete(sessionId);
  }, []);

  const handleData = useCallback((sessionId: string, data: string) => {
    sendData(sessionId, data);
  }, [sendData]);

  const isActiveTransferring = activeTab?.state === "transferring";

  return (
    <div className={styles.viewport}>
      <div className={`${styles.terminalArea} liquid-glass`}>
        {isActiveTransferring && (
          <motion.div
            className={styles.transferBanner}
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, y: 0 }}
          >
            <span className={styles.transferBannerIcon}>📤</span>
            <span>{t("transfer.transferringBanner", "File transfer in progress – terminal paused")}</span>
          </motion.div>
        )}

        <div className={styles.terminalsContainer}>
          <AnimatePresence>
            {connectedTabs.map(tab => {
              const isActive = tab.id === state.activeTabId;
              return (
                <motion.div
                  key={tab.id}
                  className={styles.terminalWrapper}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: isActive ? 1 : 0 }}
                  exit={{ opacity: 0 }}
                  style={{ pointerEvents: isActive ? "auto" : "none" }}
                  transition={{ duration: 0.15 }}
                >
                  <TerminalInstance
                    sessionId={tab.id}
                    onData={(data) => handleData(tab.id, data)}
                    isConnected={tab.state === "connected" || tab.state === "transferring"}
                    isActive={isActive}
                    onTermReady={(writeFn) => handleTermReady(tab.id, writeFn)}
                    onCleanup={handleTermCleanup}
                    ref={(node) => {
                      if (node) {
                        terminalRefs.current.set(tab.id, node);
                      } else {
                        terminalRefs.current.delete(tab.id);
                      }
                    }}
                  />
                </motion.div>
              );
            })}
          </AnimatePresence>

          {connectedTabs.length === 0 && (
            <div className={styles.emptyState}>
              <div className={styles.emptyIcon}>⚡</div>
              <div>{t("session.noSessions")}</div>
              <div className={styles.emptyHint}>
                {t("session.emptyHint") || "Use Ctrl+Shift+N to create a new session"}
              </div>
            </div>
          )}
        </div>
      </div>

      {searchVisible && (
        <SearchBar
          onClose={() => setSearchVisible(false)}
          terminal={activeTermRef.current?.terminal ?? null}
        />
      )}
    </div>
  );
}
