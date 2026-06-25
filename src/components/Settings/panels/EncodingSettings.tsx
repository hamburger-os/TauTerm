import { useState } from "react";
import { useTranslation } from "react-i18next";
import Icon from "../../common/Icon";
import styles from "../SettingsPage.module.css";

const CHARSETS = [
  { id: "utf-8", labelKey: "settings.charsetUtf8" },
  { id: "gb2312", labelKey: "settings.charsetGb2312" },
  { id: "gbk", labelKey: "settings.charsetGbk" },
  { id: "big5", labelKey: "settings.charsetBig5" },
  { id: "shift-jis", labelKey: "settings.charsetShiftJis" },
  { id: "euc-kr", labelKey: "settings.charsetEucKr" },
  { id: "iso-8859-1", labelKey: "settings.charsetLatin1" },
];

/**
 * 字符编码设置面板
 *
 * 选择会话数据的字符编码，v1 版本当前仅存储配置。
 */
export default function EncodingSettings() {
  const { t } = useTranslation();
  const [encoding, setEncoding] = useState(() => {
    return localStorage.getItem("tauterm-encoding") || "utf-8";
  });

  const handleChange = (enc: string) => {
    setEncoding(enc);
    localStorage.setItem("tauterm-encoding", enc);
  };

  return (
    <div>
      <h3 className={styles.panelTitle}>{t("settings.encoding")}</h3>

      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("settings.charsetLabel")}</span>
        <div className={styles.optionList}>
          {CHARSETS.map(cs => (
            <button
              key={cs.id}
              className={`${styles.optionItem} ${encoding === cs.id ? styles.optionItemActive : ""}`}
              onClick={() => handleChange(cs.id)}
            >
              <Icon name="check-plain" size="sm" className={styles.optionIcon} />
              <span>{t(cs.labelKey)}</span>
              <span className={styles.charsetCode}>
                {cs.id.toUpperCase()}
              </span>
            </button>
          ))}
        </div>
      </div>

      <div className={styles.encodingNote}>
        <Icon name="warning" size="sm" /> {t("settings.charsetNote")}
      </div>
    </div>
  );
}
