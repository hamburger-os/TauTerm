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
    <div className={styles.notice}>
      <strong className={className}>{result.status}</strong> · {result.latency_ms} ms · attempt {result.attempt + 1}
      {result.write_outcome_unknown && <span className={styles.warning}> · 响应超时，写入结果未知</span>}
      <div className={styles.mono}>TX {hex(result.raw_tx) || "—"}</div>
      {result.raw_rx.length > 0 && <div className={styles.mono}>RX {hex(result.raw_rx)}</div>}
      {result.response_pdu.length > 0 && <div className={styles.mono}>PDU {hex(result.response_pdu)}</div>}
      {exception && <div className={styles.error}>{exception}</div>}
      {result.message && <div>{result.message}</div>}
    </div>
  );
}
