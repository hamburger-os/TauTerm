import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useTheme, THEMES } from "../../../context/ThemeContext";
import Icon from "../../common/Icon";
import styles from "../SettingsPage.module.css";

/**
 * 外观设置面板
 *
 * 主题选择 + 终端字体大小。
 */
export default function AppearanceSettings() {
  const { t } = useTranslation();
  const { theme, setTheme } = useTheme();
  const [fontSize, setFontSize] = useState(() => {
    return Number(localStorage.getItem("tauterm-font-size") || "14");
  });

  const handleFontSizeChange = (val: number) => {
    setFontSize(val);
    localStorage.setItem("tauterm-font-size", String(val));
  };

  return (
    <div>
      <h3 className={styles.panelTitle}>{t("settings.appearance")}</h3>

      {/* 主题选择 */}
      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("settings.theme")}</span>
        <div className={styles.optionList}>
          {THEMES.map(tm => (
            <button
              key={tm.id}
              className={`${styles.optionItem} ${theme === tm.id ? styles.optionItemActive : ""}`}
              onClick={() => setTheme(tm.id)}
            >
              <Icon name="check-plain" size="sm" className={styles.optionIcon} />
              {tm.name}
            </button>
          ))}
        </div>
      </div>

      {/* 字体大小 */}
      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("settings.fontSize")}</span>
        <div className={styles.fontSlider}>
          <input
            type="range"
            className={styles.fontSliderInput}
            min={10}
            max={24}
            step={1}
            value={fontSize}
            onChange={(e) => handleFontSizeChange(Number(e.target.value))}
          />
          <span className={styles.fontSliderValue}>{fontSize}px</span>
        </div>
        <p className={styles.settingDesc}>
          {t("settings.fontSize")}: {fontSize}px ({t("settings.fontSizeNote")})
        </p>
      </div>
    </div>
  );
}
