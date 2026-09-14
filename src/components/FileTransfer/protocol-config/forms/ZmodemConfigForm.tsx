import { useTranslation } from "react-i18next";
import type {
  ZmodemCrcPolicy,
  ZmodemMaxBlockSize,
  ZmodemReceiveCrcCapability,
  ZmodemTransferConfig,
} from "../../../../types/transfer";
import styles from "./shared/ProtocolOptionForm.module.css";

interface ZmodemConfigFormProps {
  config: ZmodemTransferConfig;
  onChange: (config: ZmodemTransferConfig) => void;
}

const crcPolicies: readonly ZmodemCrcPolicy[] = ["auto", "crc16", "crc32-required"];
const crcCapabilities: readonly ZmodemReceiveCrcCapability[] = ["auto", "crc16-only"];
const maxBlockSizes: readonly ZmodemMaxBlockSize[] = [1024, 2048, 4096, 8192];

export default function ZmodemConfigForm({ config, onChange }: ZmodemConfigFormProps) {
  const { t } = useTranslation();
  return (
    <div className={styles.form}>
      <div className={styles.group}>
        <label className={styles.groupLabel}>
          {t("transfer.configSendSettings")} · {t("transfer.configCrcPolicy")}
        </label>
        <div className={styles.btnRow}>
          {crcPolicies.map((crcPolicy) => (
            <button
              key={crcPolicy}
              className={`${styles.optionBtn} liquid-glass-button ${config.send.crcPolicy === crcPolicy ? "active" : ""}`}
              onClick={() => onChange({ ...config, send: { ...config.send, crcPolicy } })}
            >
              {t(`transfer.configMode.${crcPolicy}`)}
            </button>
          ))}
        </div>
      </div>
      <div className={styles.group}>
        <label className={styles.groupLabel}>
          {t("transfer.configSendSettings")} · {t("transfer.configMaxBlockSize")}
        </label>
        <div className={styles.btnRow}>
          {maxBlockSizes.map((maxBlockSize) => (
            <button
              key={maxBlockSize}
              className={`${styles.optionBtn} liquid-glass-button ${config.send.maxBlockSize === maxBlockSize ? "active" : ""}`}
              onClick={() => onChange({ ...config, send: { ...config.send, maxBlockSize } })}
            >
              {maxBlockSize / 1024}K
            </button>
          ))}
        </div>
      </div>
      <div className={styles.group}>
        <label className={styles.groupLabel}>
          {t("transfer.configReceiveSettings")} · {t("transfer.configCrcCapability")}
        </label>
        <div className={styles.btnRow}>
          {crcCapabilities.map((crcCapability) => (
            <button
              key={crcCapability}
              className={`${styles.optionBtn} liquid-glass-button ${config.receive.crcCapability === crcCapability ? "active" : ""}`}
              onClick={() => onChange({ ...config, receive: { crcCapability } })}
            >
              {t(`transfer.configMode.${crcCapability}`)}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
