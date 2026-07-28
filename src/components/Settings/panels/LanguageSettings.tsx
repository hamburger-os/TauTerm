import { useTranslation } from "react-i18next";
import OptionButton from "../../common/OptionButton";
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

      <h4 className={styles.categoryTitle}>{t("settings.languageLabel")}</h4>
      <div className={styles.settingGroup}>
        <div className={styles.optionList}>
          <OptionButton selected={i18n.language === "zh-CN"} onClick={() => handleLanguageChange("zh-CN")}>
            {t("settings.languageZh")}
          </OptionButton>
          <OptionButton selected={i18n.language === "en-US"} onClick={() => handleLanguageChange("en-US")}>
            {t("settings.languageEn")}
          </OptionButton>
        </div>
      </div>
    </div>
  );
}
