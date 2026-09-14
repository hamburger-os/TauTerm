import { useTranslation } from "react-i18next";
import type { YmodemTransferConfig } from "../../../../types/transfer";
import styles from "./shared/ProtocolOptionForm.module.css";

interface YmodemConfigFormProps {
  config: YmodemTransferConfig;
  onChange: (config: YmodemTransferConfig) => void;
}

/** YMODEM 仅暴露发送方可控制的数据块大小；接收方固定按标准 CRC16 流程工作。 */
export default function YmodemConfigForm({ config, onChange }: YmodemConfigFormProps) {
  const { t } = useTranslation();
  return (
    <div className={styles.form}>
      <div className={styles.group}>
        <label className={styles.groupLabel}>
          {t("transfer.configSendSettings")} · {t("transfer.configBlockSize")}
        </label>
        <div className={styles.btnRow}>
          {([1024, 128] as const).map((blockSize) => (
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
    </div>
  );
}
