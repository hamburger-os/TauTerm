import styles from "../Modbus.module.css";
import { hex, STANDARD_EXCEPTIONS, type TransactionResult } from "../model";

export default function ResultCard({ result }: { result: TransactionResult }) {
  const className = result.status === "success" || result.status === "broadcast"
    ? styles.success
    : result.status === "timeout"
      ? styles.warning
      : styles.error;
  const exception = result.exception_code == null
    ? null
    : `${STANDARD_EXCEPTIONS[result.exception_code] ?? "Unknown Exception"} (0x${result.exception_code.toString(16).padStart(2, "0").toUpperCase()})`;

  return (
    <div className={styles.resultSummary}>
      <div className={styles.resultMeta}>
        <strong className={className}>{result.status}</strong>
        <span>{result.latency_ms} ms</span>
        <span>attempt {result.attempt + 1}</span>
        {result.write_outcome_unknown && <span className={styles.warning}>响应超时，写入结果未知</span>}
        {exception && <span className={styles.error}>{exception}</span>}
        {result.message && <span>{result.message}</span>}
      </div>
      <details className={styles.rawDetails}>
        <summary>原始帧</summary>
        <div className={styles.rawFrames}>
          <div className={styles.mono}>TX {hex(result.raw_tx) || "—"}</div>
          <div className={styles.mono}>RX {hex(result.raw_rx) || "—"}</div>
          {result.response_pdu.length > 0 && <div className={styles.mono}>PDU {hex(result.response_pdu)}</div>}
        </div>
      </details>
    </div>
  );
}
