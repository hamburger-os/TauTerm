import { useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { useTransfer } from "../../context/TransferContext";
import type { TransferConfig } from "../../types/transfer";
import { PROTOCOL_REGISTRY } from "../../types/transfer";
import type { ProtocolType } from "../../types/transfer";
import ProtocolSelector from "../FileTransfer/protocol-config/ProtocolSelector";
import ProtocolConfigForm from "../FileTransfer/protocol-config/ProtocolConfigForm";
import AggregateProgress from "../FileTransfer/progress/AggregateProgress";
import PerFileList from "../FileTransfer/progress/PerFileList";
import ConnectionStatusDot from "../FileTransfer/shared/ConnectionStatusDot";
import GlassButton from "../common/GlassButton";
import { formatBytes } from "../../utils/format";
import styles from "./TransmissionPanel.module.css";

interface TransmissionPanelProps {
  sessionId: string;
  isConnected: boolean;
  /** 会话创建时选定的传输协议 */
  initialProtocol?: ProtocolType;
  style?: React.CSSProperties;
}

/**
 * 传输子系统面板 (竖向布局)
 *
 * 位于终端右侧，条件显示。包含文件传输的配置、
 * 进度和操作按钮，采用竖向排列适合侧面板显示。
 */
export default function TransmissionPanel({ sessionId, isConnected, initialProtocol, style }: TransmissionPanelProps) {
  const { t } = useTranslation();
  const {
    state: transferState,
    startTransfer,
    cancelTransfer,
    clearError,
  } = useTransfer();

  const initialConfig = PROTOCOL_REGISTRY[initialProtocol || "ymodem"]?.defaultConfig ?? PROTOCOL_REGISTRY.ymodem.defaultConfig;
  const [config, setConfig] = useState<TransferConfig>(initialConfig);

  const {
    status,
    error,
    batchFiles,
    aggregateBytesTransferred,
    aggregateTotalBytes,
    currentFileIndex,
    totalFiles,
  } = transferState;

  const isTransferring = status === "transferring";
  const canTransfer = isConnected && !isTransferring;
  const batchEntries = Object.values(batchFiles);

  // 发送文件
  const handleSend = useCallback(async () => {
    if (!sessionId) return;
    try {
      const selected = await open({ multiple: true, filters: [{ name: t("transmission.allFiles") || "All Files", extensions: ["*"] }] });
      if (selected) {
        const paths = Array.isArray(selected) ? selected : [selected];
        startTransfer(config, sessionId, "send", paths);
      }
    } catch (e) {
      // errors are handled by TransferContext
    }
  }, [sessionId, startTransfer, config]);

  // 接收文件
  const handleReceive = useCallback(async () => {
    if (!sessionId) return;
    try {
      const selected = await open({ directory: true, multiple: false });
      if (selected && typeof selected === "string") {
        startTransfer(config, sessionId, "receive", undefined, selected);
      }
    } catch (e) {
      // errors are handled by TransferContext
    }
  }, [sessionId, startTransfer, config]);

  const handleCancel = useCallback(() => {
    cancelTransfer(sessionId);
  }, [sessionId, cancelTransfer]);

  const showActiveTransfer = batchEntries.length > 0 || isTransferring;
  const failedCount = batchEntries.filter(e => e.status === "failed").length;
  const skippedCount = batchEntries.filter(e => e.status === "skipped").length;

  return (
    <div className={`${styles.panel} liquid-glass`} style={style}>
      {/* 标题栏 */}
      <div className={styles.header}>
        <span className={styles.title}>{t("transmission.title")}</span>
      </div>

      <div className={styles.body}>
          {/* 操作按钮 */}
          <div className={styles.actionRow}>
            {isTransferring ? (
              <GlassButton variant="danger" size="sm" onClick={handleCancel}>
                ⏹ {t("transmission.cancel")}
              </GlassButton>
            ) : (
              <>
                <GlassButton
                  variant="primary"
                  size="sm"
                  disabled={!canTransfer}
                  onClick={handleSend}
                >
                  📤 {t("transmission.send")}
                </GlassButton>
                <GlassButton
                  variant="primary"
                  size="sm"
                  disabled={!canTransfer}
                  onClick={handleReceive}
                >
                  📥 {t("transmission.receive")}
                </GlassButton>
              </>
            )}
          </div>

          {/* 连接状态 */}
          <div className={styles.section}>
            <ConnectionStatusDot isConnected={isConnected} />
          </div>

          {/* 协议配置区 */}
          <div className={styles.section}>
            <span className={styles.sectionLabel}>{t("transmission.config")}</span>
            <ProtocolSelector value={config} onChange={setConfig} />
            <ProtocolConfigForm config={config} onChange={setConfig} />
          </div>

          {/* 已选文件 */}
          {batchEntries.length > 0 && (
            <div className={styles.section}>
              <span className={styles.sectionLabel}>{t("transmission.selectFiles")}</span>
              <div className={styles.fileSummary}>
                <span>{batchEntries.length} {t("transfer.filesSelected")}</span>
                <span>{formatBytes(aggregateTotalBytes)}</span>
              </div>
            </div>
          )}

          {/* 传输进度区 */}
          <div className={styles.progressSection}>
            {showActiveTransfer ? (
              <>
                <AggregateProgress
                  currentFileIndex={currentFileIndex}
                  totalFiles={totalFiles}
                  aggregateBytesTransferred={aggregateBytesTransferred}
                  aggregateTotalBytes={aggregateTotalBytes}
                />
                <div className={styles.fileListScroll}>
                  <PerFileList entries={batchEntries} />
                </div>

                {/* 错误 */}
                {error && (
                  <div className={styles.errorBox}>
                    <span className={styles.errorText}>{error}</span>
                    <button className={styles.errorClose} onClick={clearError}>×</button>
                  </div>
                )}

                {/* 失败汇总 */}
                {failedCount > 0 && !isTransferring && (
                  <div className={styles.failSummary}>
                    ⚠ {failedCount} {t("transfer.filesFailed")}
                  </div>
                )}

                {/* 跳过汇总 */}
                {skippedCount > 0 && !isTransferring && (
                  <div className={styles.skipSummary}>
                    ⏭ {skippedCount} {t("transfer.filesSkipped")}
                  </div>
                )}
              </>
            ) : (
              <div className={styles.placeholder}>
                {t("transmission.noActiveTransfer")}
              </div>
            )}
          </div>
        </div>
    </div>
  );
}
