import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import { pluginRegistry } from "../../core/plugin-registry";
import styles from "./StatusBar.module.css";

/**
 * 底部状态栏
 *
 * 显示连接状态、Tx/Rx 实时速率、插件状态项。
 * 插件通过 registerPlugin() 注册 statusBarItems。
 */
export default function StatusBar() {
  const { t, i18n } = useTranslation();
  const { state } = useSession();
  const activeTab = state.tabs.find(t => t.id === state.activeTabId);

  const toggleLanguage = () => {
    const newLang = i18n.language === "zh-CN" ? "en-US" : "zh-CN";
    i18n.changeLanguage(newLang);
    localStorage.setItem("tauterm-language", newLang);
  };

  // 获取活跃插件的状态栏项
  const pluginStatusItems = activeTab
    ? pluginRegistry.get(activeTab.pluginId)?.statusBarItems ?? []
    : [];

  // 格式化字节数
  const formatBytes = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  return (
    <div className={styles.bar}>
      <div className={styles.left}>
        {/* 连接状态指示器 */}
        <div className={styles.indicator}>
          <span className={`${styles.dot} ${
            activeTab?.state === "connected" ? styles.connected :
            activeTab?.state === "transferring" ? styles.transferring : ""
          }`} />
          <span className={styles.text}>
            {activeTab?.state === "connected"
              ? `${t("serial.connected")}: ${activeTab.endpoint}`
              : activeTab?.state === "transferring"
                ? `${t("transfer.transferringStatus") || "Transferring..."}: ${activeTab.endpoint}`
                : t("serial.disconnected")}
          </span>
        </div>

        {/* I/O 吞吐量 */}
        {activeTab && (activeTab.state === "connected" || activeTab.state === "transferring") && (
          <div className={styles.stats}>
            <span className={styles.statItem} title="TX">
              ↑ {formatBytes(activeTab.stats.txBytes)}
            </span>
            <span className={styles.statItem} title="RX">
              ↓ {formatBytes(activeTab.stats.rxBytes)}
            </span>
          </div>
        )}

        {/* 插件状态项 */}
        {pluginStatusItems.map(item => (
          <div key={item.id} className={styles.pluginItem}>
            {item.render({ sessionId: activeTab?.id ?? "" })}
          </div>
        ))}
      </div>

      <div className={styles.right}>
        <span className={styles.meta}>
          {(activeTab?.state === "connected" || activeTab?.state === "transferring") ? `${activeTab.endpoint}` : ""}
        </span>
        <button className={styles.langBtn} onClick={toggleLanguage}>
          {i18n.language === "zh-CN" ? "EN" : "中"}
        </button>
        <span className={styles.version}>v0.2.0</span>
      </div>
    </div>
  );
}
