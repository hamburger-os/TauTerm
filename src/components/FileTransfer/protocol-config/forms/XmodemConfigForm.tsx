import { useTranslation } from "react-i18next";
import type { XmodemReceiveCheckMode, XmodemTransferConfig } from "../../../../types/transfer";
import styles from "./shared/ProtocolOptionForm.module.css";

interface XmodemConfigFormProps {
  config: XmodemTransferConfig;
  onChange: (config: XmodemTransferConfig) => void;
}

const receiveModes: readonly XmodemReceiveCheckMode[] = ["auto", "crc16", "checksum"];

export default function XmodemConfigForm({ config, onChange }: XmodemConfigFormProps) {
  const { t } = useTranslation();
  return (
    <div className={styles.form}>
      <div className={styles.group}>
        <label className={styles.groupLabel}>
          {t("transfer.configSendSettings")} · {t("transfer.configBlockSize")}
        </label>
        <div className={styles.btnRow}>
          {([128, 1024] as const).map((blockSize) => (
            <button
              key={blockSize}
              className={`${styles.optionBtn} liquid-glass-button ${config.send.blockSize === blockSize ? "active" : ""}`}
              onClick={() => onChange({ ...config, send: { blockSize } })}
            >
              {blockSize === 1024 ? t("transfer.configBlockSize1K") : t("transfer.configBlockSize128")}
            </button>
          ))}
        </div>
      </div>
      <div className={styles.group}>
        <label className={styles.groupLabel}>
          {t("transfer.configReceiveSettings")} · {t("transfer.configChecksumRequest")}
        </label>
        <div className={styles.btnRow}>
          {receiveModes.map((checkMode) => (
            <button
              key={checkMode}
              className={`${styles.optionBtn} liquid-glass-button ${config.receive.checkMode === checkMode ? "active" : ""}`}
              onClick={() => onChange({ ...config, receive: { checkMode } })}
            >
              {t(`transfer.configMode.${checkMode}`)}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
