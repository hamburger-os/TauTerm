import { useTranslation } from "react-i18next";
import type { YmodemTransferConfig } from "../../../../types/transfer";
import styles from "./shared/ProtocolOptionForm.module.css";

interface YmodemConfigFormProps {
  config: YmodemTransferConfig;
  onChange: (config: YmodemTransferConfig) => void;
}

/** YModem 协议参数：块大小 + 校验模式 */
export default function YmodemConfigForm({
  config,
  onChange,
}: YmodemConfigFormProps) {
  const { t } = useTranslation();

  return (
    <div className={styles.form}>
      {/* Block Size */}
      <div className={styles.group}>
        <label className={styles.groupLabel}>
          {t("transfer.configBlockSize")}
        </label>
        <div className={styles.btnRow}>
          {([1024, 128] as const).map((bs) => (
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
          {(["crc16", "checksum8"] as const).map((mode) => (
            <button
              key={mode}
              className={`${styles.optionBtn} liquid-glass-button ${config.checksumMode === mode ? styles.optionBtnActive : ""}`}
              onClick={() =>
                onChange({ ...config, checksumMode: mode })
              }
            >
              {mode === "crc16"
                ? t("transfer.configChecksumCRC16")
                : t("transfer.configChecksumStandard")}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
