import { useTranslation } from "react-i18next";
import ProgressBar from "./ProgressBar";
import styles from "./AggregateProgress.module.css";

/** 通用字节格式化（size/speed 共享） */
function formatBytes(bytes: number, suffix: string): string {
  if (bytes <= 0) return "";
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    units.length - 1,
  );
  const rounded =
    bytes >= 1024 * 1024
      ? (bytes / Math.pow(1024, i)).toFixed(1)
      : String(Math.round(bytes / Math.pow(1024, i)));
  return `${rounded} ${units[i]}${suffix}`;
}

/** 格式化文件大小 */
function formatSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  return formatBytes(bytes, "");
}

/** 格式化传输速度 */
function formatSpeed(bytesPerSec: number): string {
  return formatBytes(bytesPerSec, "/s");
}

interface AggregateProgressProps {
  currentFileIndex: number;
  totalFiles: number;
  aggregateBytesTransferred: number;
  aggregateTotalBytes: number;
  currentFileName?: string;
  /** 当前传输速度（bytes/s），0 或 undefined 时不显示 */
  speed?: number;
}

/** 聚合进度卡片：总体进度条 + 文件计数 + 字节统计 */
export default function AggregateProgress({
  currentFileIndex,
  totalFiles,
  aggregateBytesTransferred,
  aggregateTotalBytes,
  currentFileName,
  speed,
}: AggregateProgressProps) {
  const { t } = useTranslation();
  const percent =
    aggregateTotalBytes > 0
      ? Math.round((aggregateBytesTransferred / aggregateTotalBytes) * 100)
      : 0;
  const speedText = formatSpeed(speed ?? 0);

  return (
    <div className={`${styles.container} liquid-glass-card`}>
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
          {speedText && (
            <span className={styles.speed}> · {speedText}</span>
          )}
        </span>
      </div>
      <ProgressBar percent={percent} />
    </div>
  );
}
