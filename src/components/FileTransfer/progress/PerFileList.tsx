import type { BatchFileEntry } from "../../../types/transfer";
import ProgressBar from "./ProgressBar";

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
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "2px",
      }}
    >
      {entries.map((entry) => {
        const filePercent =
          entry.totalBytes > 0
            ? Math.round((entry.bytesTransferred / entry.totalBytes) * 100)
            : 0;
        return (
          <div
            key={entry.fileName}
            style={{
              display: "flex",
              alignItems: "center",
              gap: "var(--spacing-sm)",
              padding: "2px var(--spacing-sm)",
              background: "var(--glass-bg)",
              border: "1px solid var(--glass-border)",
              borderRadius: "var(--radius-sm)",
              fontSize: "var(--text-xs)",
            }}
          >
            <span style={{ flexShrink: 0, width: "18px", textAlign: "center" }}>
              {getStatusIcon(entry.status)}
            </span>
            <div
              style={{
                flex: 1,
                overflow: "hidden",
                display: "flex",
                flexDirection: "column",
                gap: "2px",
              }}
            >
              <span
                title={entry.fileName}
                style={{
                  color: "var(--text-primary)",
                  fontFamily: "var(--font-mono)",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {entry.fileName}
              </span>
              {entry.status === "transferring" && entry.totalBytes > 0 && (
                <ProgressBar percent={filePercent} height={2} />
              )}
              {(entry.status === "failed" || entry.status === "skipped") &&
                entry.error && (
                  <span
                    style={{
                      color:
                        entry.status === "skipped"
                          ? "var(--color-warning)"
                          : "var(--color-error)",
                      fontSize: "0.6rem",
                    }}
                  >
                    {entry.error}
                  </span>
                )}
            </div>
            <span
              style={{
                flexShrink: 0,
                color: "var(--text-muted)",
                fontSize: "0.6rem",
                whiteSpace: "nowrap",
              }}
            >
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
