interface ConnectionStatusDotProps {
  isConnected: boolean;
  portName?: string;
  baudRate?: number;
}

/** 连接状态指示点 */
export default function ConnectionStatusDot({
  isConnected,
  portName,
  baudRate,
}: ConnectionStatusDotProps) {
  const color = isConnected ? "var(--color-success)" : "var(--color-error)";
  const label = isConnected
    ? [portName, baudRate ? `${baudRate}` : null]
        .filter(Boolean)
        .join(" · ")
    : "未连接";

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: "6px",
        fontSize: "var(--text-xs)",
        color: "var(--text-secondary)",
      }}
    >
      <span
        style={{
          width: "8px",
          height: "8px",
          borderRadius: "50%",
          background: color,
          boxShadow: `0 0 6px ${color}`,
          flexShrink: 0,
        }}
      />
      <span
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {label}
      </span>
    </div>
  );
}
