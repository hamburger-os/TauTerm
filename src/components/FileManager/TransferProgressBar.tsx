/**
 * 文件管理器传输状态条。
 *
 * 视觉层只负责响应式布局和状态表达；传输生命周期、自动收起与事件过滤
 * 由 useSftpProgress 统一管理。
 */
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import type { TransferPhase } from "./hooks/useSftpProgress";
import { isTransferTerminalPhase } from "./hooks/useSftpProgress";
import styles from "./TransferProgressBar.module.css";

function formatSpeed(bytesPerSec: number | null): string {
  if (bytesPerSec === null || !Number.isFinite(bytesPerSec) || bytesPerSec <= 0) return "—";
  if (bytesPerSec < 1024) return `${bytesPerSec.toFixed(0)} B/s`;
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} MB/s`;
}

interface TransferProgressBarProps {
  visible: boolean;
  fileName: string;
  direction: "upload" | "download";
  percent: number;
  phase: TransferPhase;
  /** 后端 SFTP I/O 层测得的速率；null 表示暂无可靠样本。 */
  speed: number | null;
  error?: string | null;
  fileIndex?: number;
  totalFiles?: number;
  aggregatePercent?: number;
  onClose: () => void;
  onMouseEnter?: () => void;
  onMouseLeave?: () => void;
}

export default function TransferProgressBar({
  visible,
  fileName,
  direction,
  percent,
  phase,
  speed,
  error,
  fileIndex = 0,
  totalFiles = 1,
  aggregatePercent,
  onClose,
  onMouseEnter,
  onMouseLeave,
}: TransferProgressBarProps) {
  const { t } = useTranslation();

  if (!visible) return null;

  const directionIcon = direction === "upload" ? "upload" : "download";
  const clampedPercent = Math.min(100, Math.max(0, percent));
  const isBatch = totalFiles > 1;
  const clampedAgg = Math.min(100, Math.max(0, aggregatePercent ?? percent));
  const terminal = isTransferTerminalPhase(phase);
  const cancelling = phase === "cancelling";

  const detail = (() => {
    if (error && phase !== "completed" && phase !== "cancelled") {
      return error;
    }
    switch (phase) {
      case "preparing":
        return t("fileManager.transferPreparing");
      case "transferring":
        return formatSpeed(speed);
      case "finalizing":
        return t("fileManager.transferFinalizing");
      case "cancelling":
        return t("fileManager.transferCancelling");
      case "completed":
        return speed && speed > 0
          ? `${t("fileManager.transferCompleted")} · ${formatSpeed(speed)}`
          : t("fileManager.transferCompleted");
      case "failed":
        return t("fileManager.transferFailed");
      case "cancelled":
        return t("fileManager.transferCancelled");
    }
  })();

  const displayName = fileName || (
    direction === "upload"
      ? t("fileManager.uploading")
      : t("fileManager.downloading")
  );

  return (
    <div
      className={`${styles.bar} liquid-glass-float`}
      data-phase={phase}
      data-has-error={Boolean(error) ? "true" : "false"}
      role="status"
      aria-live="polite"
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      <span className={styles.left}>
        <Icon name={directionIcon} size="sm" className={styles.dirIcon} />
        <span className={styles.fileName} title={displayName}>
          {displayName}
          {isBatch && (
            <span className={styles.batchLabel}>
              {" "}({fileIndex + 1}/{totalFiles})
            </span>
          )}
        </span>
      </span>

      <div className={styles.progressArea} aria-hidden="true">
        <div className={styles.progressTrack}>
          <div
            className={styles.progressFill}
            style={{ width: `${clampedPercent}%` }}
          />
        </div>
        {isBatch && (
          <div className={`${styles.progressTrack} ${styles.aggregateTrack}`}>
            <div
              className={`${styles.progressFill} ${styles.aggregateFill}`}
              style={{ width: `${clampedAgg}%` }}
            />
          </div>
        )}
      </div>

      <span className={styles.percentText}>
        {clampedPercent.toFixed(0)}%
      </span>

      <span
        className={`${styles.detailText} ${phase === "transferring" ? styles.liveSpeed : ""}`}
        title={error || detail}
      >
        {detail}
      </span>

      <button
        className={`${styles.closeBtn} liquid-glass-ghost-button`}
        onClick={onClose}
        disabled={cancelling}
        title={
          terminal
            ? t("common.close")
            : cancelling
              ? t("fileManager.transferCancelling")
              : t("fileManager.cancelTransfer")
        }
        aria-label={
          terminal
            ? t("common.close")
            : cancelling
              ? t("fileManager.transferCancelling")
              : t("fileManager.cancelTransfer")
        }
        type="button"
      >
        <Icon name="close" size="sm" />
      </button>
    </div>
  );
}
