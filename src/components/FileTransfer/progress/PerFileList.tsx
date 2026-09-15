import type { BatchFileEntry } from "../../../types/transfer";
import { formatBytes } from "../../../utils/format";
import Icon from "../../common/Icon";
import type { IconName } from "../../common/Icon";
import ProgressBar from "./ProgressBar";
import styles from "./PerFileList.module.css";

function getStatusIconName(status: string): IconName {
  switch (status) {
    case "pending":
      return "hourglass";
    case "transferring":
      return "transfer-active";
    case "completed":
      return "check-circle";
    case "failed":
      return "x-circle";
    case "skipped":
      return "status-skipped";
    default:
      return "info";
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
            className={styles.row}
          >
            <span className={styles.iconCell}>
              <Icon name={getStatusIconName(entry.status)} size="sm" />
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
                : formatBytes(entry.bytesTransferred)}
            </span>
          </div>
        );
      })}
    </div>
  );
}
