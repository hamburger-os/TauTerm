import { useTranslation } from "react-i18next";
import { useTheme, THEMES } from "../../../context/ThemeContext";
import Icon from "../../common/Icon";
import styles from "../SettingsPage.module.css";

/** 行缓冲滑块步长：1,000 行一档，避免拖动时频繁重设 xterm scrollback */
const BUFFER_LINES_STEP = 1000;

/**
 * 外观设置面板
 *
 * 主题选择 + 终端字体大小 + 行缓冲上限（所有数据模式统一）。
 */
export default function AppearanceSettings() {
  const { t } = useTranslation();
  const { theme, setTheme, fontSize, setFontSize, bufferLines, setBufferLines } = useTheme();

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
            onChange={(e) => setFontSize(Number(e.target.value))}
          />
          <span className={styles.fontSliderValue}>{fontSize}px</span>
        </div>
        <p className={styles.settingDesc}>
          {t("settings.fontSize")}: {fontSize}px ({t("settings.fontSizeNote")})
        </p>
      </div>

      {/* 行缓冲上限（统一：Text / HEX / Dual） */}
      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("settings.bufferLines")}</span>
        <div className={styles.fontSlider}>
          <input
            type="range"
            className={styles.fontSliderInput}
            min={1000}
            max={100000}
            step={BUFFER_LINES_STEP}
            value={bufferLines}
            onChange={(e) => setBufferLines(Number(e.target.value))}
          />
          <span className={styles.fontSliderValue}>{bufferLines.toLocaleString()}</span>
        </div>
        <p className={styles.settingDesc}>
          {t("settings.bufferLines")}: {bufferLines.toLocaleString()} {t("settings.bufferLinesNote")}
        </p>
      </div>
    </div>
  );
}
