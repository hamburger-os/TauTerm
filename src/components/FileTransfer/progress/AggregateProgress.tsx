import { useTranslation } from "react-i18next";
import { formatBytes, formatRate } from "../../../utils/format";
import ProgressBar from "./ProgressBar";
import styles from "./AggregateProgress.module.css";

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
  const speedText = speed && speed > 0 ? formatRate(speed) : "";

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
          {formatBytes(aggregateBytesTransferred)} /{" "}
          {formatBytes(aggregateTotalBytes)} ({percent}%)
          {speedText && (
            <span className={styles.speed}> · {speedText}</span>
          )}
        </span>
      </div>
      <ProgressBar percent={percent} />
    </div>
  );
}
