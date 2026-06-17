import { useTranslation } from "react-i18next";
import type { BatchFileEntry } from "../../../types/transfer";
import AggregateProgress from "../progress/AggregateProgress";
import PerFileList from "../progress/PerFileList";
import styles from "../FileTransferPanel.module.css";

interface ProgressColumnProps {
  isTransferring: boolean;
  batchEntries: BatchFileEntry[];
  currentFileIndex: number;
  totalFiles: number;
  aggregateBytesTransferred: number;
  aggregateTotalBytes: number;
  error: string | null;
  onClearError: () => void;
}

/**
 * 中列：聚合进度 + 逐文件列表 + 错误/跳过汇总
 */
export default function ProgressColumn({
  isTransferring,
  batchEntries,
  currentFileIndex,
  totalFiles,
  aggregateBytesTransferred,
  aggregateTotalBytes,
  error,
  onClearError,
}: ProgressColumnProps) {
  const { t } = useTranslation();

  const showActiveTransfer = batchEntries.length > 0 || isTransferring;
  const failedCount = batchEntries.filter((e) => e.status === "failed").length;
  const skippedCount = batchEntries.filter(
    (e) => e.status === "skipped",
  ).length;

  if (!showActiveTransfer) {
    return (
      <div className={styles.progressColumn}>
        <h4 className={styles.columnTitle}>
          {t("transfer.transferProgress")}
        </h4>
        <div className={styles.placeholder}>
          {t("transfer.noActiveTransfer")}
        </div>
      </div>
    );
  }

  return (
    <div className={styles.progressColumn}>
      <h4 className={styles.columnTitle}>
        {t("transfer.transferProgress")}
      </h4>

      {/* Aggregate Progress */}
      <AggregateProgress
        currentFileIndex={currentFileIndex}
        totalFiles={totalFiles}
        aggregateBytesTransferred={aggregateBytesTransferred}
        aggregateTotalBytes={aggregateTotalBytes}
      />

      {/* Per-File List (scrollable) */}
      <div className={styles.fileListScroll}>
        <PerFileList entries={batchEntries} />
      </div>

      {/* Error */}
      {error && (
        <div
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: "var(--spacing-sm)",
            padding: "var(--spacing-xs) var(--spacing-sm)",
            background: "rgba(255, 71, 87, 0.1)",
            border: "1px solid rgba(255, 71, 87, 0.25)",
            borderRadius: "var(--radius-sm)",
            fontSize: "var(--text-xs)",
            color: "var(--color-error)",
            flexShrink: 0,
          }}
        >
          <span style={{ flex: 1 }}>{error}</span>
          <button
            onClick={onClearError}
            style={{
              background: "none",
              border: "none",
              color: "var(--color-error)",
              cursor: "pointer",
              fontSize: "var(--text-md)",
              lineHeight: 1,
              padding: 0,
              flexShrink: 0,
              opacity: 0.6,
            }}
          >
            ×
          </button>
        </div>
      )}

      {/* Failure Summary */}
      {failedCount > 0 && !isTransferring && (
        <div
          style={{
            padding: "var(--spacing-xs) var(--spacing-sm)",
            background: "rgba(255, 71, 87, 0.08)",
            border: "1px solid rgba(255, 71, 87, 0.2)",
            borderRadius: "var(--radius-sm)",
            fontSize: "var(--text-xs)",
            color: "var(--color-error)",
            flexShrink: 0,
          }}
        >
          ⚠ {failedCount} {t("transfer.filesFailed")}{" "}
          {t("transfer.partialSuccess")}
        </div>
      )}

      {/* Skip Summary */}
      {skippedCount > 0 && !isTransferring && (
        <div
          style={{
            padding: "var(--spacing-xs) var(--spacing-sm)",
            background: "rgba(255, 165, 2, 0.08)",
            border: "1px solid rgba(255, 165, 2, 0.2)",
            borderRadius: "var(--radius-sm)",
            fontSize: "var(--text-xs)",
            color: "var(--color-warning)",
            flexShrink: 0,
          }}
        >
          ⏭ {skippedCount} {t("transfer.filesSkipped")}{" "}
          {t("transfer.filesSkippedMsg")}
        </div>
      )}
    </div>
  );
}
