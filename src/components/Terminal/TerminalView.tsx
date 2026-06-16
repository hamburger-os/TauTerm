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
 * 渲染当前活跃标签页的终端实例。
 * 非活跃标签页的终端保持 DOM 但不渲染（节省资源但保留状态可选）。
 */
export default function TerminalView() {
  const { t } = useTranslation();
  const { state, sendData, onSessionData } = useSession();
  const { registerAction } = useKeyboard();
  const writeRefs = useRef<Map<string, (data: Uint8Array | string) => void>>(new Map());
  const [searchVisible, setSearchVisible] = useState(false);
  const activeTermRef = useRef<any>(null);

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

  // 清理已删除会话的 writeRefs 条目（处理非活跃标签页被删除的情况）
  useEffect(() => {
    const validIds = new Set(state.tabs.map(t => t.id));
    const toRemove: string[] = [];
    writeRefs.current.forEach((_, id) => {
      if (!validIds.has(id)) {
        toRemove.push(id);
      }
    });
    for (const id of toRemove) {
      writeRefs.current.delete(id);
    }
  }, [state.tabs]);

  const handleTermReady = useCallback((sessionId: string, writeFn: (data: Uint8Array | string) => void) => {
    writeRefs.current.set(sessionId, writeFn);
  }, []);

  const handleTermCleanup = useCallback((sessionId: string) => {
    writeRefs.current.delete(sessionId);
  }, []);

  const handleData = useCallback((sessionId: string, data: string) => {
    sendData(sessionId, data);
  }, [sendData]);

  const activeTab = state.tabs.find(t => t.id === state.activeTabId);
  const isTermActive = activeTab?.state === "connected" || activeTab?.state === "transferring";
  const activeTerm = activeTab ? {
    id: activeTab.id,
    isConnected: isTermActive,
    isTransferring: activeTab.state === "transferring",
  } : null;

  return (
    <div className={styles.viewport}>
      <div className={styles.terminalArea}>
        {activeTerm?.isTransferring && (
          <motion.div
            className={styles.transferBanner}
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, y: 0 }}
          >
            <span className={styles.transferBannerIcon}>📤</span>
            <span>{t("transfer.transferringBanner", "File transfer in progress – terminal paused")}</span>
          </motion.div>
        )}
        <AnimatePresence mode="wait">
          {activeTerm && activeTerm.isConnected ? (
            <motion.div
              key={`${activeTerm.id}-${activeTerm.isConnected}`}
              className={styles.terminalWrapper}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.15 }}
            >
              <TerminalInstance
                sessionId={activeTerm.id}
                onData={(data) => handleData(activeTerm.id, data)}
                isConnected={activeTerm.isConnected}
                onTermReady={(writeFn) => handleTermReady(activeTerm.id, writeFn)}
                onCleanup={handleTermCleanup}
                ref={activeTermRef}
              />
            </motion.div>
          ) : (
            <div className={styles.emptyState}>
              <div className={styles.emptyIcon}>⚡</div>
              <div>{t("session.noSessions")}</div>
              <div className={styles.emptyHint}>
                {t("session.emptyHint") || "Use Ctrl+Shift+N to create a new session"}
              </div>
            </div>
          )}
        </AnimatePresence>
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
