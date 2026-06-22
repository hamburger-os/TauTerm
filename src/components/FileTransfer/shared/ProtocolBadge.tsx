import type { ProtocolType } from "../../../types/transfer";
import { PROTOCOL_REGISTRY } from "../../../types/transfer";
import styles from "./ProtocolBadge.module.css";

interface ProtocolBadgeProps {
  protocol: ProtocolType;
}

/** 协议彩色小标签 */
export default function ProtocolBadge({ protocol }: ProtocolBadgeProps) {
  const meta = PROTOCOL_REGISTRY[protocol];
  if (!meta) return null;

  return (
    <span className={styles.badge}>
      {meta.icon} {protocol.toUpperCase()}
    </span>
  );
}
