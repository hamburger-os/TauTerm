import { useTranslation } from "react-i18next";
import styles from "../Modbus.module.css";
import { hex, STANDARD_EXCEPTIONS, type TransactionResult, type TransactionStatus } from "../model";

const STATUS_KEY: Partial<Record<TransactionStatus, string>> = {
  broadcast: "modbus.statusBroadcast",
  modbus_exception: "modbus.statusException",
  protocol_error: "modbus.statusProtocol",
  malformed_response: "modbus.statusMalformed",
  timeout: "modbus.statusTimeout",
  transport_error: "modbus.statusTransport",
  cancelled: "modbus.statusCancelled",
  fault_injected: "modbus.statusFault",
};

export default function ResultCard({ result }: { result: TransactionResult }) {
  const { t } = useTranslation();
  const className = result.status === "success" || result.status === "broadcast"
    ? styles.success
    : result.status === "timeout"
      ? styles.warning
      : styles.error;
  const exception = result.exception_code == null
    ? null
    : `${STANDARD_EXCEPTIONS[result.exception_code] ?? "Unknown Exception"} (0x${result.exception_code.toString(16).padStart(2, "0").toUpperCase()})`;
  const status = result.status === "success" ? "OK" : t(STATUS_KEY[result.status] ?? "modbus.statusProtocol");

  return (
    <div className={styles.resultSummary}>
      <div className={styles.resultMeta}>
        <strong className={className}>{status}</strong>
        <span>{result.latency_ms} ms</span>
        <span>{t("modbus.resultAttempt", { attempt: result.attempt + 1 })}</span>
        {result.write_outcome_unknown && <span className={styles.warning}>{t("modbus.resultOutcomeUnknown")}</span>}
        {exception && <span className={styles.error}>{exception}</span>}
        {result.message && <span>{result.message}</span>}
      </div>
      <details className={styles.rawDetails}>
        <summary>{t("modbus.resultRawFrames")}</summary>
        <div className={styles.rawFrames}>
          <div className={styles.mono}>TX {hex(result.raw_tx) || "—"}</div>
          <div className={styles.mono}>RX {hex(result.raw_rx) || "—"}</div>
          {result.response_pdu.length > 0 && <div className={styles.mono}>PDU {hex(result.response_pdu)}</div>}
        </div>
      </details>
    </div>
  );
}
