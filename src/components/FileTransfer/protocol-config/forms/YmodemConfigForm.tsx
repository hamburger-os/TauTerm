import { useTranslation } from "react-i18next";
import type { YmodemTransferConfig } from "../../../../types/transfer";

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
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--spacing-sm)" }}>
      {/* Block Size */}
      <div style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
        <label style={{ fontSize: "0.6rem", color: "var(--text-muted)" }}>
          {t("transfer.configBlockSize")}
        </label>
        <div style={{ display: "flex", gap: "2px" }}>
          {([1024, 128] as const).map((bs) => (
            <button
              key={bs}
              onClick={() => onChange({ ...config, blockSize: bs })}
              style={{
                flex: 1,
                padding: "3px 6px",
                fontSize: "var(--text-xs)",
                fontFamily: "var(--font-mono)",
                textAlign: "center",
                background:
                  config.blockSize === bs
                    ? "var(--glass-bg-active)"
                    : "var(--glass-bg)",
                border:
                  config.blockSize === bs
                    ? "1px solid var(--glass-border-focus)"
                    : "1px solid var(--glass-border)",
                borderRadius: "var(--radius-sm)",
                color:
                  config.blockSize === bs
                    ? "var(--text-primary)"
                    : "var(--text-secondary)",
                cursor: "pointer",
                transition: "all var(--transition-fast, 0.15s)",
              }}
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
          {(["crc16", "crc32"] as const).map((mode) => (
            <button
              key={mode}
              onClick={() =>
                onChange({ ...config, checksumMode: mode })
              }
              style={{
                flex: 1,
                padding: "3px 6px",
                fontSize: "var(--text-xs)",
                textAlign: "center",
                background:
                  config.checksumMode === mode
                    ? "var(--glass-bg-active)"
                    : "var(--glass-bg)",
                border:
                  config.checksumMode === mode
                    ? "1px solid var(--glass-border-focus)"
                    : "1px solid var(--glass-border)",
                borderRadius: "var(--radius-sm)",
                color:
                  config.checksumMode === mode
                    ? "var(--text-primary)"
                    : "var(--text-secondary)",
                cursor: "pointer",
                transition: "all var(--transition-fast, 0.15s)",
              }}
            >
              {mode === "crc16"
                ? t("transfer.configChecksumCRC16")
                : t("transfer.configChecksumCRC32")}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
