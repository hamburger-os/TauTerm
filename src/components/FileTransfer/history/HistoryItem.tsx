import { useTranslation } from "react-i18next";
import type { TransferHistoryItem as THItem } from "../../../types/transfer";
import ProtocolBadge from "../shared/ProtocolBadge";

/** 格式化文件大小 */
function formatSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

function formatTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

interface HistoryItemProps {
  item: THItem;
}

export default function HistoryItem({ item }: HistoryItemProps) {
  const { t } = useTranslation();

  const statusColors: Record<string, string> = {
    completed: "var(--color-success)",
    failed: "var(--color-error)",
    cancelled: "var(--color-warning)",
  };

  const statusLabels: Record<string, string> = {
    completed: t("transfer.complete"),
    failed: t("transfer.failed"),
    cancelled: t("transfer.cancelled"),
  };

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        padding: "3px var(--spacing-sm)",
        background: "var(--glass-bg)",
        border: "1px solid var(--glass-border)",
        borderRadius: "var(--radius-sm)",
        fontSize: "var(--text-xs)",
      }}
    >
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: "1px",
          flex: 1,
          overflow: "hidden",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "6px",
          }}
        >
          <span
            style={{
              color: "var(--text-primary)",
              fontFamily: "var(--font-mono)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {item.file_name}
          </span>
          <ProtocolBadge protocol={item.protocol} />
        </div>
        <div
          style={{
            display: "flex",
            gap: "8px",
            color: "var(--text-muted)",
            fontSize: "0.6rem",
          }}
        >
          <span>
            {item.direction === "send"
              ? t("transfer.directionLabel_send")
              : t("transfer.directionLabel_receive")}
          </span>
          <span>{formatSize(item.size)}</span>
          <span>{formatTime(item.timestamp)}</span>
        </div>
      </div>
      <span
        style={{
          flexShrink: 0,
          marginLeft: "8px",
          padding: "1px 5px",
          borderRadius: "3px",
          fontSize: "0.6rem",
          background: `${statusColors[item.status] || "var(--color-info)"}22`,
          color: statusColors[item.status] || "var(--color-info)",
        }}
      >
        {statusLabels[item.status] || item.status}
      </span>
    </div>
  );
}
