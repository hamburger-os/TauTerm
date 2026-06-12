import { useTranslation } from "react-i18next";
import GlassButton from "../common/GlassButton";
import type {
  TransferStatus,
  TransferProgress,
  TransferHistoryItem,
} from "../../hooks/useFileTransfer";
import styles from "./FileTransferPanel.module.css";

interface FileTransferPanelProps {
  /** 传输状态 */
  status: TransferStatus;
  /** 传输进度 */
  progress: TransferProgress | null;
  /** 传输历史 */
  history: TransferHistoryItem[];
  /** 错误信息 */
  error: string | null;
  /** 取消传输 */
  onCancel: () => void;
  /** 清除错误 */
  onClearError: () => void;
  /** 清除历史 */
  onClearHistory: () => void;
}

/** 格式化文件大小 */
function formatSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

/** 格式化时间戳 */
function formatTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/**
 * 文件传输面板
 *
 * 显示传输进度、历史记录和操作按钮。
 */
export default function FileTransferPanel({
  status,
  progress,
  history,
  error,
  onCancel,
  onClearError,
  onClearHistory,
}: FileTransferPanelProps) {
  const { t } = useTranslation();

  const isTransferring = status === "transferring";
  const progressPercent =
    progress && progress.total_bytes > 0
      ? Math.round((progress.bytes_transferred / progress.total_bytes) * 100)
      : 0;

  const getStatusLabel = (s: TransferStatus): string => {
    switch (s) {
      case "transferring":
        return t("transfer.sending");
      case "completed":
        return t("transfer.complete");
      case "failed":
        return t("transfer.failed");
      case "cancelled":
        return t("transfer.cancelled");
      default:
        return "";
    }
  };

  const getDirectionLabel = (d: string): string => {
    return d === "send" ? t("transfer.send") : t("transfer.receive");
  };

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <h3 className={styles.title}>{t("transfer.title")}</h3>
        {isTransferring && (
          <GlassButton variant="danger" size="sm" onClick={onCancel}>
            {t("transfer.cancel")}
          </GlassButton>
        )}
      </div>

      {/* 当前传输进度 */}
      {progress && isTransferring && (
        <div className={styles.progressSection}>
          <div className={styles.progressInfo}>
            <span className={styles.fileName}>
              {progress.file_name || t("transfer.receiving")}
            </span>
            <span className={styles.progressText}>
              {formatSize(progress.bytes_transferred)} /{" "}
              {formatSize(progress.total_bytes)} ({progressPercent}%)
            </span>
          </div>
          <div className={styles.progressBar}>
            <div
              className={styles.progressFill}
              style={{ width: `${progressPercent}%` }}
            />
          </div>
          <div className={styles.progressStatus}>
            {getStatusLabel(status)}
          </div>
        </div>
      )}

      {/* 错误信息 */}
      {error && (
        <div className={styles.errorBox}>
          <span>{error}</span>
          <button className={styles.errorClose} onClick={onClearError}>
            ×
          </button>
        </div>
      )}

      {/* 传输历史 */}
      <div className={styles.historySection}>
        <div className={styles.historyHeader}>
          <span className={styles.historyTitle}>
            {t("transfer.history")}
          </span>
          {history.length > 0 && (
            <GlassButton
              variant="ghost"
              size="sm"
              onClick={onClearHistory}
            >
              {t("common.close")}
            </GlassButton>
          )}
        </div>

        {history.length === 0 ? (
          <div className={styles.emptyHistory}>
            {t("transfer.noHistory")}
          </div>
        ) : (
          <div className={styles.historyList}>
            {history.map((item) => (
              <div key={item.id} className={styles.historyItem}>
                <div className={styles.historyItemInfo}>
                  <span className={styles.historyFileName}>
                    {item.file_name}
                  </span>
                  <span className={styles.historyMeta}>
                    {getDirectionLabel(item.direction)} ·{" "}
                    {formatSize(item.size)} · {formatTime(item.timestamp)}
                  </span>
                </div>
                <span
                  className={`${styles.historyStatus} ${
                    styles[`status_${item.status}`]
                  }`}
                >
                  {getStatusLabel(item.status)}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
