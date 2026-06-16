import { useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import { useSession } from "../../context/SessionContext";
import { useTransfer } from "../../context/TransferContext";
import BottomInfoPanel from "./BottomInfoPanel";
import FileTransferPanel from "../FileTransfer/FileTransferPanel";
import styles from "./BottomPanel.module.css";

interface BottomPanelProps {
  onSendFiles: () => void;
  onReceiveFiles: () => void;
}

type TabId = "info" | "transfer";

/**
 * 底部面板容器
 *
 * 信息标签页 + 文件传输标签页，可拖拽调整高度。
 */
export default function BottomPanel({ onSendFiles, onReceiveFiles }: BottomPanelProps) {
  const { t } = useTranslation();
  const { state: sessionState } = useSession();
  const { state: transferState, cancelTransfer, clearError, clearHistory } = useTransfer();
  const [activeTab, setActiveTab] = useState<TabId>("info");

  const tabs: { id: TabId; label: string; icon: string }[] = [
    { id: "info", label: t("bottomPanel.sessionInfo") || "Info", icon: "📊" },
    { id: "transfer", label: t("transfer.title") || "Transfer", icon: "📂" },
  ];

  const handleTabClick = useCallback((tabId: TabId) => {
    setActiveTab(tabId);
  }, []);

  const isConnected = sessionState.tabs.some(t => t.state === "connected");

  return (
    <div className={styles.panel}>
      {/* 标签页栏 */}
      <div className={styles.tabBar}>
        {tabs.map(tab => (
          <button
            key={tab.id}
            className={`${styles.tab} ${activeTab === tab.id ? styles.tabActive : ""}`}
            onClick={() => handleTabClick(tab.id)}
          >
            <span className={styles.tabIcon}>{tab.icon}</span>
            <span>{tab.label}</span>
          </button>
        ))}
      </div>

      {/* 标签页内容 */}
      <div className={styles.tabContent}>
        <motion.div
          key={activeTab}
          className={styles.tabPanel}
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.15 }}
        >
          {activeTab === "info" ? (
            <BottomInfoPanel />
          ) : (
            <FileTransferPanel
              status={transferState.status}
              progress={transferState.progress}
              history={transferState.history}
              error={transferState.error}
              isConnected={isConnected}
              onSendFiles={onSendFiles}
              onReceiveFiles={onReceiveFiles}
              onCancel={() => sessionState.activeTabId && cancelTransfer(sessionState.activeTabId)}
              onClearError={clearError}
              onClearHistory={clearHistory}
            />
          )}
        </motion.div>
      </div>
    </div>
  );
}
