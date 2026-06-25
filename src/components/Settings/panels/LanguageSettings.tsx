import { useTranslation } from "react-i18next";
import Icon from "../../common/Icon";
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
            <Icon name="check-plain" size="sm" className={styles.optionIcon} />
            {t("settings.languageZh")}
          </button>
          <button
            className={`${styles.optionItem} ${i18n.language === "en-US" ? styles.optionItemActive : ""}`}
            onClick={() => handleLanguageChange("en-US")}
          >
            <Icon name="check-plain" size="sm" className={styles.optionIcon} />
            {t("settings.languageEn")}
          </button>
        </div>
      </div>
    </div>
  );
}
