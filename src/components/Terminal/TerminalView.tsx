import { useEffect, useRef, useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { AnimatePresence, motion } from "framer-motion";
import { useSession } from "../../context/SessionContext";
import { useKeyboard } from "../../hooks/useKeyboard";
import { ACTION_IDS } from "../../shortcuts/actionIds";
import TerminalInstance from "./Terminal";
import SearchBar from "./SearchBar";
import styles from "./Terminal.module.css";

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
      const writeFn = writeRefs.current.get(sessionId);
      writeFn?.(data);
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
    }
  }, [connectedTabs]);

  const handleTermReady = useCallback((sessionId: string, writeFn: (data: Uint8Array | string) => void) => {
    writeRefs.current.set(sessionId, writeFn);
  }, []);

  const handleTermCleanup = useCallback((sessionId: string) => {
    writeRefs.current.delete(sessionId);
  }, []);

  const handleData = useCallback((sessionId: string, data: string) => {
    sendData(sessionId, data);
  }, [sendData]);

  const isActiveTransferring = activeTab?.state === "transferring";

  return (
    <div className={styles.viewport}>
      <div className={styles.terminalArea}>
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
