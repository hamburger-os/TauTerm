import type { ProtocolType } from "../../../types/transfer";
import { PROTOCOL_REGISTRY } from "../../../types/transfer";

interface ProtocolBadgeProps {
  protocol: ProtocolType;
}

/** 协议彩色小标签 */
export default function ProtocolBadge({ protocol }: ProtocolBadgeProps) {
  const meta = PROTOCOL_REGISTRY[protocol];
  if (!meta) return null;

  return (
    <span
      style={{
        display: "inline-block",
        padding: "0 4px",
        fontSize: "0.6rem",
        fontWeight: 600,
        borderRadius: "3px",
        background: "rgba(0, 163, 255, 0.12)",
        color: "var(--color-info)",
        lineHeight: "1.4",
      }}
    >
      {meta.icon} {protocol.toUpperCase()}
    </span>
  );
}
