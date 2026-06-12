import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import styles from "./StatusBar.module.css";

/**
 * 底部状态栏
 *
 * 显示连接状态、Rx/Tx 统计、快捷操作按钮。
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

  return (
    <div className={styles.bar}>
      <div className={styles.left}>
        <div className={styles.indicator}>
          <span className={`${styles.dot} ${activeTab?.state === "connected" ? styles.connected : ""}`} />
          <span className={styles.text}>
            {activeTab?.state === "connected"
              ? `${t("serial.connected")}: ${activeTab.endpoint}`
              : t("serial.disconnected")}
          </span>
        </div>
      </div>

      <div className={styles.right}>
        <span className={styles.meta}>
          {activeTab?.state === "connected" ? `${activeTab.endpoint}` : ""}
        </span>
        <button className={styles.langBtn} onClick={toggleLanguage}>
          {i18n.language === "zh-CN" ? "EN" : "中"}
        </button>
        <span className={styles.version}>v0.2.0</span>
      </div>
    </div>
  );
}
