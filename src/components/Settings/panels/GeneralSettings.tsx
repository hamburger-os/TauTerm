import { useTranslation } from "react-i18next";
import Icon from "../../common/Icon";
import styles from "../SettingsPage.module.css";

/**
 * 通用设置面板
 *
 * 默认数据模式选择。
 */
export default function GeneralSettings() {
  const { t } = useTranslation();

  const currentMode = localStorage.getItem("tauterm-default-data-mode") || "text";

  const handleModeChange = (mode: string) => {
    localStorage.setItem("tauterm-default-data-mode", mode);
  };

  return (
    <div>
      <h3 className={styles.panelTitle}>{t("settings.general")}</h3>

      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("settings.defaultDataMode")}</span>
        <div className={styles.optionList}>
          <button
            className={`${styles.optionItem} ${currentMode === "text" ? styles.optionItemActive : ""}`}
            onClick={() => handleModeChange("text")}
          >
            <Icon name="check-plain" size="sm" className={styles.optionIcon} />
            {t("serial.dataModeText")}
          </button>
          <button
            className={`${styles.optionItem} ${currentMode === "hex" ? styles.optionItemActive : ""}`}
            onClick={() => handleModeChange("hex")}
          >
            <Icon name="check-plain" size="sm" className={styles.optionIcon} />
            {t("serial.dataModeHex")}
          </button>
        </div>
        <p className={styles.settingDesc}>
          {t("settings.defaultDataMode")}: {currentMode === "text" ? t("serial.dataModeText") : t("serial.dataModeHex")}
        </p>
      </div>
    </div>
  );
}
