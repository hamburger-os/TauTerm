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
  { id: "performance", labelKey: "settings.performancePerformance", descKey: "settings.performancePerformanceDesc" },
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
    systemReducedMotion,
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

      {/* 两档视觉性能：效果优先保留动态液态玻璃；性能优先使用静态四色 + 低成本半透明表面。 */}
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
          {t(PERFORMANCE_OPTIONS.find(option => option.id === performanceMode)?.descKey ?? "settings.performanceQualityDesc")}
        </p>
        <p className={styles.settingDesc} aria-live="polite">
          {t("settings.systemMotionStatus")}:{" "}
          {systemReducedMotion
            ? t("settings.systemMotionReduced")
            : t("settings.systemMotionAllowed")}
        </p>
        {systemReducedMotion && performanceMode === "quality" && (
          <p className={styles.settingDesc}>
            {t("settings.systemMotionReducedHint")}
          </p>
        )}
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
