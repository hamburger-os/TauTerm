import { useTranslation } from "react-i18next";
import ProgressBar from "./ProgressBar";

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
    <div
      style={{
        padding: "var(--spacing-xs) var(--spacing-sm)",
        background: "var(--glass-bg)",
        border: "1px solid var(--glass-border)",
        borderRadius: "var(--radius-md)",
        flexShrink: 0,
      }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          marginBottom: "4px",
          fontSize: "var(--text-xs)",
        }}
      >
        <span
          style={{
            color: "var(--text-primary)",
            fontWeight: 500,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            flex: 1,
            marginRight: "8px",
          }}
        >
          {currentFileName ||
            t("transfer.fileXOfY", {
              current: currentFileIndex + 1,
              total: totalFiles,
            })}
        </span>
        <span style={{ color: "var(--text-secondary)", whiteSpace: "nowrap" }}>
          {formatSize(aggregateBytesTransferred)} /{" "}
          {formatSize(aggregateTotalBytes)} ({percent}%)
        </span>
      </div>
      <ProgressBar percent={percent} />
    </div>
  );
}
