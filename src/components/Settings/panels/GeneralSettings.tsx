import { useState } from "react";
import { useTranslation } from "react-i18next";
import OptionButton from "../../common/OptionButton";
import styles from "../SettingsPage.module.css";

/**
 * 通用设置面板
 *
 * 默认数据模式选择。
 */
export default function GeneralSettings() {
  const { t } = useTranslation();

  const [currentMode, setCurrentMode] = useState<string>(
    () => localStorage.getItem("tauterm-default-data-mode") || "text"
  );

  const handleModeChange = (mode: string) => {
    localStorage.setItem("tauterm-default-data-mode", mode);
    setCurrentMode(mode);
  };

  return (
    <div>
      <h3 className={styles.panelTitle}>{t("settings.general")}</h3>

      <h4 className={styles.categoryTitle}>{t("settings.defaultDataMode")}</h4>
      <div className={styles.settingGroup}>
        <div className={styles.optionList}>
          <OptionButton selected={currentMode === "text"} onClick={() => handleModeChange("text")}>
            {t("serial.dataModeText")}
          </OptionButton>
          <OptionButton selected={currentMode === "hex"} onClick={() => handleModeChange("hex")}>
            {t("serial.dataModeHex")}
          </OptionButton>
          <OptionButton selected={currentMode === "dual"} onClick={() => handleModeChange("dual")}>
            {t("serial.dataModeDual")}
          </OptionButton>
        </div>
        <p className={styles.settingDesc}>
          {t("settings.defaultDataMode")}: {
            currentMode === "text" ? t("serial.dataModeText") :
            currentMode === "hex" ? t("serial.dataModeHex") :
            t("serial.dataModeDual")
          }
        </p>
      </div>
    </div>
  );
}
