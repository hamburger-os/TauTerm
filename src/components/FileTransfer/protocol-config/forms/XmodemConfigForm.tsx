import { useTranslation } from "react-i18next";
import type { XmodemTransferConfig } from "../../../../types/transfer";

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
  const btnStyle = (active: boolean) =>
    ({
      flex: 1,
      padding: "3px 6px",
      fontSize: "var(--text-xs)",
      textAlign: "center" as const,
      background: active ? "var(--glass-bg-active)" : "var(--glass-bg)",
      border: active
        ? "1px solid var(--glass-border-focus)"
        : "1px solid var(--glass-border)",
      borderRadius: "var(--radius-sm)",
      color: active ? "var(--text-primary)" : "var(--text-secondary)",
      cursor: "pointer",
      transition: "all var(--transition-fast, 0.15s)",
    }) as React.CSSProperties;

  return (
    <div
      style={{ display: "flex", flexDirection: "column", gap: "var(--spacing-sm)" }}
    >
      {/* Block Size */}
      <div style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
        <label style={{ fontSize: "0.6rem", color: "var(--text-muted)" }}>
          {t("transfer.configBlockSize")}
        </label>
        <div style={{ display: "flex", gap: "2px" }}>
          {([128, 1024] as const).map((bs) => (
            <button
              key={bs}
              style={btnStyle(config.blockSize === bs)}
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
      <div style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
        <label style={{ fontSize: "0.6rem", color: "var(--text-muted)" }}>
          {t("transfer.configChecksumMode")}
        </label>
        <div style={{ display: "flex", gap: "2px" }}>
          {(["checksum", "crc16"] as const).map((mode) => (
            <button
              key={mode}
              style={btnStyle(config.checksumMode === mode)}
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
      <div style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
        <label style={{ fontSize: "0.6rem", color: "var(--text-muted)" }}>
          {t("transfer.configInitChar")}
        </label>
        <div style={{ display: "flex", gap: "2px" }}>
          {(["nak", "crc"] as const).map((init) => (
            <button
              key={init}
              style={btnStyle(config.initChar === init)}
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
