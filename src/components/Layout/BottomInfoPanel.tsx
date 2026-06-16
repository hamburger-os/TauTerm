import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import styles from "./BottomInfoPanel.module.css";

/**
 * 底部信息面板
 *
 * 始终可见的固定高度面板，显示当前活跃会话的信息。
 * 替换了原先可切换的文件传输面板。
 */
/** 连接类型 i18n 键映射 */
const CONNECTION_TYPE_KEYS: Record<string, string> = {
  serial: "connectionType.serial",
  ssh: "connectionType.ssh",
  telnet: "connectionType.telnet",
  tftp: "connectionType.tftp",
};

export default function BottomInfoPanel() {
  const { t } = useTranslation();
  const { state } = useSession();

  const activeTab = state.tabs.find(t => t.id === state.activeTabId);
  const connTypeLabel = activeTab
    ? t(CONNECTION_TYPE_KEYS[activeTab.connection_type] || activeTab.connection_type)
    : "";

  return (
    <div className={styles.panel}>
      {activeTab ? (
        <div className={styles.content}>
          <div className={styles.infoItem}>
            <span className={styles.label}>{t("session.renameSession")}</span>
            <span className={styles.value}>{activeTab.name}</span>
          </div>
          <div className={styles.infoItem}>
            <span className={styles.label}>{t("connectionType.label")}</span>
            <span className={styles.value}>{connTypeLabel}</span>
          </div>
          <div className={styles.infoItem}>
            <span className={styles.label}>{t("serial.port")}</span>
            <span className={styles.value}>{activeTab.endpoint}</span>
          </div>
          <div className={styles.infoItem}>
            <span className={styles.label}>{t("serial.connected")}</span>
            {activeTab.state === "transferring" ? (
              <span className={`${styles.value} ${styles.transferring}`}>
                📤 {t("transfer.transferringStatus") || "Transferring..."}
              </span>
            ) : (
              <span className={`${styles.value} ${activeTab.state === "connected" ? styles.connected : styles.disconnected}`}>
                {activeTab.state === "connected" ? t("serial.connected") : t("serial.disconnected")}
              </span>
            )}
          </div>
        </div>
      ) : (
        <div className={styles.emptyState}>
          <span className={styles.emptyIcon}>⚡</span>
          <span>{t("bottomPanel.noSession") || "No active session"}</span>
        </div>
      )}
    </div>
  );
}
