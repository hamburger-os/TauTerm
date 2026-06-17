import { useTranslation } from "react-i18next";
import type { TransferConfig } from "../../../types/transfer";
import GlassButton from "../../common/GlassButton";
import ProtocolSelector from "../protocol-config/ProtocolSelector";
import ProtocolConfigForm from "../protocol-config/ProtocolConfigForm";
import ConnectionStatusDot from "../shared/ConnectionStatusDot";
import styles from "../FileTransferPanel.module.css";

interface ConfigColumnProps {
  config: TransferConfig;
  onConfigChange: (config: TransferConfig) => void;
  isConnected: boolean;
  isTransferring: boolean;
  canTransfer: boolean;
  batchCount: number;
  aggregateTotal: number;
  onSend: () => void;
  onReceive: () => void;
  onCancel: () => void;
}

/** 格式化文件大小 */
function formatSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

/**
 * 左列：操作按钮置顶 + 协议选择 + 参数配置 + 连接状态
 *
 * 发送/接收按钮并排放在最上方，方便快速点击。
 * 传输中显示取消按钮。列内容超出高度时可上下滚动。
 */
export default function ConfigColumn({
  config,
  onConfigChange,
  isConnected,
  isTransferring,
  canTransfer,
  batchCount,
  aggregateTotal,
  onSend,
  onReceive,
  onCancel,
}: ConfigColumnProps) {
  const { t } = useTranslation();

  return (
    <div className={styles.configColumn}>
      {/* Header: title + action buttons in one row */}
      <div className={styles.configHeader}>
        <span className={styles.configHeaderTitle}>
          {t("transfer.config")}
        </span>
        <div className={styles.configHeaderActions}>
          {isTransferring ? (
            <GlassButton variant="danger" size="sm" onClick={onCancel}>
              ⏹ {t("transfer.cancel")}
            </GlassButton>
          ) : (
            <>
              <GlassButton
                variant="primary"
                size="sm"
                disabled={!canTransfer}
                onClick={onSend}
              >
                📤 {t("transfer.send")}
              </GlassButton>
              <GlassButton
                variant="primary"
                size="sm"
                disabled={!canTransfer}
                onClick={onReceive}
              >
                📥 {t("transfer.receive")}
              </GlassButton>
            </>
          )}
        </div>
      </div>

      {/* Disconnected Hint */}
      {!isConnected && (
        <div className={styles.disconnectedHint}>
          {t("serial.disconnected")}
        </div>
      )}

      {/* Protocol Selector */}
      <div className={styles.section}>
        <ProtocolSelector value={config} onChange={onConfigChange} />
      </div>

      {/* Protocol-Specific Parameters */}
      <div className={styles.section}>
        <span className={styles.sectionLabel}>
          {t("transfer.configTitle")}
        </span>
        <ProtocolConfigForm config={config} onChange={onConfigChange} />
      </div>

      {/* Selected Files Summary */}
      {batchCount > 0 && (
        <div className={styles.section}>
          <span className={styles.sectionLabel}>
            {t("transfer.selectFiles")}
          </span>
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              padding: "4px 8px",
              background: "var(--glass-bg)",
              border: "1px solid var(--glass-border)",
              borderRadius: "var(--radius-sm)",
              fontSize: "var(--text-xs)",
              color: "var(--text-secondary)",
            }}
          >
            <span>
              {batchCount} {t("transfer.filesSelected")}
            </span>
            <span>{formatSize(aggregateTotal)}</span>
          </div>
        </div>
      )}

      {/* Connection Status */}
      <div className={styles.section}>
        <span className={styles.sectionLabel}>
          {t("transfer.connectionStatus")}
        </span>
        <ConnectionStatusDot isConnected={isConnected} />
      </div>
    </div>
  );
}
