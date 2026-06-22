import styles from "./ConnectionStatusDot.module.css";

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
  const label = isConnected
    ? [portName, baudRate ? `${baudRate}` : null]
        .filter(Boolean)
        .join(" · ")
    : "未连接";

  return (
    <div className={styles.container}>
      <span
        className={`${styles.dot} ${isConnected ? styles.dotConnected : styles.dotDisconnected}`}
      />
      <span className={styles.label}>{label}</span>
    </div>
  );
}
