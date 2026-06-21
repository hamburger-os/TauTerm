import { useTranslation } from "react-i18next";
import type { ZmodemTransferConfig } from "../../../../types/transfer";

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

  const toggleStyle = (active: boolean): React.CSSProperties => ({
    width: "32px",
    height: "18px",
    borderRadius: "var(--radius-full)",
    background: active
      ? "var(--accent-gradient)"
      : "var(--glass-button-bg)",
    border: "1px solid var(--glass-border-default)",
    cursor: "pointer",
    position: "relative",
    transition: "background 0.2s",
  });

  return (
    <div
      style={{ display: "flex", flexDirection: "column", gap: "var(--spacing-sm)" }}
    >
      {/* Window Size */}
      <div style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
        <label style={{ fontSize: "0.6rem", color: "var(--text-muted)" }}>
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
          style={{ width: "100%", accentColor: "var(--color-info)" }}
        />
      </div>

      {/* Resume */}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
        }}
      >
        <span style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)" }}>
          {t("transfer.configResumeEnabled")}
        </span>
        <button
          style={toggleStyle(config.resumeEnabled)}
          onClick={() =>
            onChange({ ...config, resumeEnabled: !config.resumeEnabled })
          }
        >
          <span
            style={{
              position: "absolute",
              top: "2px",
              left: config.resumeEnabled ? "16px" : "2px",
              width: "14px",
              height: "14px",
              borderRadius: "50%",
              background: "var(--text-primary)",
              transition: "left 0.2s",
            }}
          />
        </button>
      </div>

      {/* Compression */}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
        }}
      >
        <span style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)" }}>
          {t("transfer.configCompression")}
        </span>
        <button
          style={toggleStyle(config.compressionEnabled)}
          onClick={() =>
            onChange({
              ...config,
              compressionEnabled: !config.compressionEnabled,
            })
          }
        >
          <span
            style={{
              position: "absolute",
              top: "2px",
              left: config.compressionEnabled ? "16px" : "2px",
              width: "14px",
              height: "14px",
              borderRadius: "50%",
              background: "var(--text-primary)",
              transition: "left 0.2s",
            }}
          />
        </button>
      </div>

      {/* Streaming */}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
        }}
      >
        <span style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)" }}>
          {t("transfer.configStreaming")}
        </span>
        <button
          style={toggleStyle(config.streamingMode)}
          onClick={() =>
            onChange({
              ...config,
              streamingMode: !config.streamingMode,
            })
          }
        >
          <span
            style={{
              position: "absolute",
              top: "2px",
              left: config.streamingMode ? "16px" : "2px",
              width: "14px",
              height: "14px",
              borderRadius: "50%",
              background: "var(--text-primary)",
              transition: "left 0.2s",
            }}
          />
        </button>
      </div>
    </div>
  );
}
