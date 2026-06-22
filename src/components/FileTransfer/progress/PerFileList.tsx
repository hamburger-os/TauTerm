import type { BatchFileEntry } from "../../../types/transfer";
import ProgressBar from "./ProgressBar";
import styles from "./PerFileList.module.css";

/** 格式化文件大小 */
function formatSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

function getStatusIcon(status: string): string {
  switch (status) {
    case "pending":
      return "⏳";
    case "transferring":
      return "⬆️";
    case "completed":
      return "✅";
    case "failed":
      return "❌";
    case "skipped":
      return "⏭️";
    default:
      return "";
  }
}

interface PerFileListProps {
  entries: BatchFileEntry[];
}

/** 逐文件列表：状态图标 + 文件名 + 迷你进度条 + 大小 */
export default function PerFileList({ entries }: PerFileListProps) {
  if (entries.length === 0) return null;

  return (
    <div className={styles.list}>
      {entries.map((entry) => {
        const filePercent =
          entry.totalBytes > 0
            ? Math.round((entry.bytesTransferred / entry.totalBytes) * 100)
            : 0;
        const isError =
          entry.status === "failed" || entry.status === "skipped";
        return (
          <div
            key={entry.fileName}
            className={`${styles.row} liquid-glass`}
          >
            <span className={styles.iconCell}>
              {getStatusIcon(entry.status)}
            </span>
            <div className={styles.fileInfo}>
              <span title={entry.fileName} className={styles.fileName}>
                {entry.fileName}
              </span>
              {entry.status === "transferring" && entry.totalBytes > 0 && (
                <ProgressBar percent={filePercent} height={2} />
              )}
              {isError && entry.error && (
                <span
                  className={`${styles.errorText} ${
                    entry.status === "skipped"
                      ? styles.errorSkipped
                      : styles.errorFailed
                  }`}
                >
                  {entry.error}
                </span>
              )}
            </div>
            <span className={styles.fileSize}>
              {entry.status === "pending"
                ? "—"
                : formatSize(entry.bytesTransferred)}
            </span>
          </div>
        );
      })}
    </div>
  );
}
