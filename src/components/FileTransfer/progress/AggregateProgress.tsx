import { useTranslation } from "react-i18next";
import ProgressBar from "./ProgressBar";
import styles from "./AggregateProgress.module.css";

/** 格式化文件大小 */
function formatSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

interface AggregateProgressProps {
  currentFileIndex: number;
  totalFiles: number;
  aggregateBytesTransferred: number;
  aggregateTotalBytes: number;
  currentFileName?: string;
}

/** 聚合进度卡片：总体进度条 + 文件计数 + 字节统计 */
export default function AggregateProgress({
  currentFileIndex,
  totalFiles,
  aggregateBytesTransferred,
  aggregateTotalBytes,
  currentFileName,
}: AggregateProgressProps) {
  const { t } = useTranslation();
  const percent =
    aggregateTotalBytes > 0
      ? Math.round((aggregateBytesTransferred / aggregateTotalBytes) * 100)
      : 0;

  return (
    <div className={`${styles.container} liquid-glass`}>
      <div className={styles.header}>
        <span className={styles.fileName}>
          {currentFileName ||
            t("transfer.fileXOfY", {
              current: currentFileIndex + 1,
              total: totalFiles,
            })}
        </span>
        <span className={styles.stats}>
          {formatSize(aggregateBytesTransferred)} /{" "}
          {formatSize(aggregateTotalBytes)} ({percent}%)
        </span>
      </div>
      <ProgressBar percent={percent} />
    </div>
  );
}
