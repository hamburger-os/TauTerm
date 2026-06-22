import { useTranslation } from "react-i18next";
import type { XmodemTransferConfig } from "../../../../types/transfer";
import styles from "./shared/ProtocolOptionForm.module.css";

interface XmodemConfigFormProps {
  config: XmodemTransferConfig;
  onChange: (config: XmodemTransferConfig) => void;
}

/** XModem 协议参数：块大小 + 校验 + 启动模式 */
export default function XmodemConfigForm({
  config,
  onChange,
}: XmodemConfigFormProps) {
  const { t } = useTranslation();

  return (
    <div className={styles.form}>
      {/* Block Size */}
      <div className={styles.group}>
        <label className={styles.groupLabel}>
          {t("transfer.configBlockSize")}
        </label>
        <div className={styles.btnRow}>
          {([128, 1024] as const).map((bs) => (
            <button
              key={bs}
              className={`${styles.optionBtn} liquid-glass-button ${config.blockSize === bs ? styles.optionBtnActive : ""}`}
              onClick={() => onChange({ ...config, blockSize: bs })}
            >
              {bs === 1024
                ? t("transfer.configBlockSize1K")
                : t("transfer.configBlockSize128")}
            </button>
          ))}
        </div>
      </div>

      {/* Checksum Mode */}
      <div className={styles.group}>
        <label className={styles.groupLabel}>
          {t("transfer.configChecksumMode")}
        </label>
        <div className={styles.btnRow}>
          {(["checksum", "crc16"] as const).map((mode) => (
            <button
              key={mode}
              className={`${styles.optionBtn} liquid-glass-button ${config.checksumMode === mode ? styles.optionBtnActive : ""}`}
              onClick={() => onChange({ ...config, checksumMode: mode })}
            >
              {mode === "checksum"
                ? t("transfer.configChecksumStandard")
                : t("transfer.configChecksumCRC16")}
            </button>
          ))}
        </div>
      </div>

      {/* Init Char */}
      <div className={styles.group}>
        <label className={styles.groupLabel}>
          {t("transfer.configInitChar")}
        </label>
        <div className={styles.btnRow}>
          {(["nak", "crc"] as const).map((init) => (
            <button
              key={init}
              className={`${styles.optionBtn} liquid-glass-button ${config.initChar === init ? styles.optionBtnActive : ""}`}
              onClick={() => onChange({ ...config, initChar: init })}
            >
              {init === "nak"
                ? t("transfer.configInitNak")
                : t("transfer.configInitCRC")}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
