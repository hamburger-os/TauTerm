import { useTranslation } from "react-i18next";
import { useTheme, THEMES, type VisualPerformanceMode } from "../../../context/ThemeContext";
import OptionButton from "../../common/OptionButton";
import styles from "../SettingsPage.module.css";

/** 行缓冲滑块步长：1,000 行一档，避免拖动时频繁重设 xterm scrollback */
const BUFFER_LINES_STEP = 1000;

const PERFORMANCE_OPTIONS: Array<{
  id: VisualPerformanceMode;
  labelKey: string;
  descKey: string;
}> = [
  { id: "quality", labelKey: "settings.performanceQuality", descKey: "settings.performanceQualityDesc" },
  { id: "balanced", labelKey: "settings.performanceBalanced", descKey: "settings.performanceBalancedDesc" },
  { id: "compat", labelKey: "settings.performanceCompat", descKey: "settings.performanceCompatDesc" },
];

/**
 * 外观设置面板
 *
 * 主题选择 + 终端字体大小 + 行缓冲上限（所有数据模式统一）。
 */
export default function AppearanceSettings() {
  const { t } = useTranslation();
  const {
    theme,
    setTheme,
    performanceMode,
    setPerformanceMode,
    fontSize,
    setFontSize,
    bufferLines,
    setBufferLines,
  } = useTheme();

  return (
    <div>
      <h3 className={styles.panelTitle}>{t("settings.appearance")}</h3>

      {/* 主题选择 */}
      <h4 className={styles.categoryTitle}>{t("settings.theme")}</h4>
      <div className={styles.settingGroup}>
        <div className={styles.optionList}>
          {THEMES.map(tm => (
            <OptionButton
              key={tm.id}
              selected={theme === tm.id}
              onClick={() => setTheme(tm.id)}
            >
              {tm.name}
            </OptionButton>
          ))}
        </div>
      </div>

      {/* 视觉性能档：主题身份不变；Quality/Balanced 调整 Ambient 丰富度，Compat 静态化并关闭 backdrop blur */}
      <h4 className={styles.categoryTitle}>{t("settings.performanceMode")}</h4>
      <div className={styles.settingGroup}>
        <div className={styles.optionList}>
          {PERFORMANCE_OPTIONS.map(option => (
            <OptionButton
              key={option.id}
              selected={performanceMode === option.id}
              onClick={() => setPerformanceMode(option.id)}
            >
              {t(option.labelKey)}
            </OptionButton>
          ))}
        </div>
        <p className={styles.settingDesc}>
          {t(PERFORMANCE_OPTIONS.find(option => option.id === performanceMode)?.descKey ?? "settings.performanceBalancedDesc")}
        </p>
      </div>

      {/* 字体大小 */}
      <h4 className={styles.categoryTitle}>{t("settings.fontSize")}</h4>
      <div className={styles.settingGroup}>
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
      <h4 className={styles.categoryTitle}>{t("settings.bufferLines")}</h4>
      <div className={styles.settingGroup}>
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
