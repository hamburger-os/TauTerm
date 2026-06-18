import { useTranslation } from "react-i18next";
import styles from "../SettingsPage.module.css";

/**
 * 语言设置面板
 *
 * 从状态栏移入，提供界面语言切换。
 */
export default function LanguageSettings() {
  const { t, i18n } = useTranslation();

  const handleLanguageChange = (lang: string) => {
    i18n.changeLanguage(lang);
    localStorage.setItem("tauterm-language", lang);
  };

  return (
    <div>
      <h3 className={styles.panelTitle}>{t("settings.language")}</h3>

      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("settings.languageLabel")}</span>
        <div className={styles.optionList}>
          <button
            className={`${styles.optionItem} ${i18n.language === "zh-CN" ? styles.optionItemActive : ""}`}
            onClick={() => handleLanguageChange("zh-CN")}
          >
            <span className={styles.optionIcon}>✓</span>
            {t("settings.languageZh")}
          </button>
          <button
            className={`${styles.optionItem} ${i18n.language === "en-US" ? styles.optionItemActive : ""}`}
            onClick={() => handleLanguageChange("en-US")}
          >
            <span className={styles.optionIcon}>✓</span>
            {t("settings.languageEn")}
          </button>
        </div>
      </div>
    </div>
  );
}
