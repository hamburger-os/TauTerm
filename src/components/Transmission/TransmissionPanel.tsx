import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { useTransfer } from "../../context/TransferContext";
import Icon from "../common/Icon";
import type { ProtocolType, TransferConfig } from "../../types/transfer";
import { PROTOCOL_REGISTRY } from "../../types/transfer";
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
 * 传输子系统面板。
 *
 * 面板只投影 TransferContext 中属于当前 Session 的任务，不再拥有或读取全局
 * “active transfer” 状态，因此多个 Session 的传输不会互相覆盖 UI 所有权。
 */
export default function TransmissionPanel({
  sessionId,
  isConnected,
  initialProtocol,
  style,
}: TransmissionPanelProps) {
  const { t } = useTranslation();
  const {
    state,
    startTransfer,
    cancelTransfer,
    clearError,
    getTaskForSession,
    getActiveTaskForSession,
  } = useTransfer();

  const initialConfig =
    PROTOCOL_REGISTRY[initialProtocol || "ymodem"]?.defaultConfig
    ?? PROTOCOL_REGISTRY.ymodem.defaultConfig;
  const [config, setConfig] = useState<TransferConfig>(initialConfig);

  const task = getTaskForSession(sessionId);
  const activeTask = getActiveTaskForSession(sessionId);
  const isTransferring = Boolean(activeTask);
  const isCancelling = activeTask?.phase === "cancelling";
  const canTransfer = isConnected && !isTransferring;
  const batchEntries = task?.files ?? [];
  const error = task?.error ?? state.startErrorsBySession[sessionId] ?? null;

  const handleSend = useCallback(async () => {
    if (!sessionId) return;
    try {
      const selected = await open({
        multiple: true,
        filters: [{
          name: t("transmission.allFiles") || "All Files",
          extensions: ["*"],
        }],
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      await startTransfer(config, sessionId, "send", paths);
    } catch {
      // 启动错误由 TransferContext 按 Session 保存并展示。
    }
  }, [config, sessionId, startTransfer, t]);

  const handleReceive = useCallback(async () => {
    if (!sessionId) return;
    try {
      const selected = await open({ directory: true, multiple: false });
      if (selected && typeof selected === "string") {
        await startTransfer(config, sessionId, "receive", undefined, selected);
      }
    } catch {
      // 启动错误由 TransferContext 按 Session 保存并展示。
    }
  }, [config, sessionId, startTransfer]);

  const handleCancel = useCallback(async () => {
    try {
      await cancelTransfer(sessionId);
    } catch {
      // 取消错误会回写到精确 transfer_id 的任务卡。
    }
  }, [cancelTransfer, sessionId]);

  const showActiveTransfer = Boolean(task);
  const failedCount = batchEntries.filter((entry) => entry.status === "failed").length;
  const skippedCount = batchEntries.filter((entry) => entry.status === "skipped").length;

  return (
    <div className={styles.panel} style={style}>
      <div className={styles.body}>
        <div className={styles.actionRow}>
          {isTransferring ? (
            <GlassButton
              variant="danger"
              size="sm"
              disabled={isCancelling}
              onClick={handleCancel}
            >
              <Icon name="stop" size="sm" /> {t("transmission.cancel")}
            </GlassButton>
          ) : (
            <>
              <GlassButton
                variant="primary"
                size="sm"
                disabled={!canTransfer}
                onClick={handleSend}
              >
                <Icon name="upload" size="sm" /> {t("transmission.send")}
              </GlassButton>
              <GlassButton
                variant="primary"
                size="sm"
                disabled={!canTransfer}
                onClick={handleReceive}
              >
                <Icon name="download" size="sm" /> {t("transmission.receive")}
              </GlassButton>
            </>
          )}
        </div>

        <div className={styles.section}>
          <ConnectionStatusDot isConnected={isConnected} />
        </div>

        <div className={styles.section}>
          <span className={styles.sectionLabel}>{t("transmission.config")}</span>
          <ProtocolSelector value={config} onChange={setConfig} />
          <ProtocolConfigForm config={config} onChange={setConfig} />
        </div>

        {batchEntries.length > 0 && (
          <div className={styles.section}>
            <span className={styles.sectionLabel}>{t("transmission.selectFiles")}</span>
            <div className={styles.fileSummary}>
              <span>{batchEntries.length} {t("transfer.filesSelected")}</span>
              <span>{formatBytes(task?.aggregateTotal ?? 0)}</span>
            </div>
          </div>
        )}

        <div className={styles.progressSection}>
          {showActiveTransfer && task ? (
            <>
              <AggregateProgress
                currentFileIndex={task.fileIndex}
                totalFiles={task.totalFiles}
                aggregateBytesTransferred={task.aggregateBytes}
                aggregateTotalBytes={task.aggregateTotal}
                currentFileName={task.fileName || undefined}
                speed={task.speed ?? undefined}
              />
              <div className={styles.fileListScroll}>
                <PerFileList entries={batchEntries} />
              </div>

              {error && (
                <div className={styles.errorBox}>
                  <span className={styles.errorText}>{error}</span>
                  <button
                    className={styles.errorClose}
                    onClick={() => clearError(sessionId)}
                    aria-label={t("common.close")}
                  >
                    <Icon name="close" size="sm" />
                  </button>
                </div>
              )}

              {failedCount > 0 && !isTransferring && (
                <div className={styles.failSummary}>
                  <Icon name="warning" size="sm" /> {failedCount} {t("transfer.filesFailed")}
                </div>
              )}

              {skippedCount > 0 && !isTransferring && (
                <div className={styles.skipSummary}>
                  <Icon name="status-skipped" size="sm" /> {skippedCount} {t("transfer.filesSkipped")}
                </div>
              )}
            </>
          ) : error ? (
            <div className={styles.errorBox}>
              <span className={styles.errorText}>{error}</span>
              <button
                className={styles.errorClose}
                onClick={() => clearError(sessionId)}
                aria-label={t("common.close")}
              >
                <Icon name="close" size="sm" />
              </button>
            </div>
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
