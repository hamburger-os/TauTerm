import { useTranslation } from "react-i18next";
import type { ZmodemTransferConfig } from "../../../../types/transfer";
import styles from "./ZmodemConfigForm.module.css";

interface ZmodemConfigFormProps {
  config: ZmodemTransferConfig;
  onChange: (config: ZmodemTransferConfig) => void;
}

/** ZModem 协议参数：窗口大小 + 续传 + 压缩 + 流式 */
export default function ZmodemConfigForm({
  config,
  onChange,
}: ZmodemConfigFormProps) {
  const { t } = useTranslation();

  return (
    <div className={styles.form}>
      {/* Window Size */}
      <div className={styles.group}>
        <label className={styles.groupLabel}>
          {t("transfer.configWindowSize")}: {config.windowSize}
        </label>
        <input
          type="range"
          min={1}
          max={16}
          value={config.windowSize}
          onChange={(e) =>
            onChange({ ...config, windowSize: Number(e.target.value) })
          }
          style={{ width: "100%", accentColor: "var(--accent-primary)" }}
        />
      </div>

      {/* Resume */}
      <div className={styles.row}>
        <span className={styles.rowLabel}>
          {t("transfer.configResumeEnabled")}
        </span>
        <label className="liquid-glass-toggle">
          <input
            type="checkbox"
            checked={config.resumeEnabled}
            onChange={() =>
              onChange({ ...config, resumeEnabled: !config.resumeEnabled })
            }
          />
          <div />
        </label>
      </div>

      {/* Compression */}
      <div className={styles.row}>
        <span className={styles.rowLabel}>
          {t("transfer.configCompression")}
        </span>
        <label className="liquid-glass-toggle">
          <input
            type="checkbox"
            checked={config.compressionEnabled}
            onChange={() =>
              onChange({
                ...config,
                compressionEnabled: !config.compressionEnabled,
              })
            }
          />
          <div />
        </label>
      </div>

      {/* Streaming */}
      <div className={styles.row}>
        <span className={styles.rowLabel}>
          {t("transfer.configStreaming")}
        </span>
        <label className="liquid-glass-toggle">
          <input
            type="checkbox"
            checked={config.streamingMode}
            onChange={() =>
              onChange({
                ...config,
                streamingMode: !config.streamingMode,
              })
            }
          />
          <div />
        </label>
      </div>
    </div>
  );
}
