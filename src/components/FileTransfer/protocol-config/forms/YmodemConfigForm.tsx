import { useTranslation } from "react-i18next";
import type { YmodemTransferConfig } from "../../../../types/transfer";
import styles from "./shared/ProtocolOptionForm.module.css";

interface YmodemConfigFormProps {
  config: YmodemTransferConfig;
  onChange: (config: YmodemTransferConfig) => void;
}

/** YModem 只暴露真正由本端控制的发送块大小；校验/流模式由握手自动协商。 */
export default function YmodemConfigForm({
  config,
  onChange,
}: YmodemConfigFormProps) {
  const { t } = useTranslation();

  return (
    <div className={styles.form}>
      <div className={styles.group}>
        <label className={styles.groupLabel}>
          {t("transfer.configBlockSize")}
        </label>
        <div className={styles.btnRow}>
          {([1024, 128] as const).map((blockSize) => (
            <button
              key={blockSize}
              className={`${styles.optionBtn} liquid-glass-button ${config.blockSize === blockSize ? "active" : ""}`}
              onClick={() => onChange({ ...config, blockSize })}
            >
              {blockSize === 1024
                ? t("transfer.configBlockSize1K")
                : t("transfer.configBlockSize128")}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
