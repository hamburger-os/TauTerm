import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import styles from "../Modbus.module.css";
import { hex, STANDARD_EXCEPTIONS, type ModbusStatus, type TransactionRecord, type TransactionStatus } from "../model";

const VIEW_LIMIT = 1000;
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

function formatTimestamp(timestampMs: number): string {
  const date = new Date(timestampMs);
  const base = date.toLocaleTimeString(undefined, { hour12: false });
  return `${base}.${String(date.getMilliseconds()).padStart(3, "0")}`;
}

export default function TransactionsPanel({ sessionId, connected }: { sessionId: string; connected: boolean }) {
  const { t } = useTranslation();
  const [records, setRecords] = useState<TransactionRecord[]>([]);
  const [paused, setPaused] = useState(false);
  const cursorRef = useRef(0);
  const refreshingRef = useRef(false);

  useEffect(() => {
    cursorRef.current = 0;
    setRecords([]);
    setPaused(false);
  }, [sessionId]);

  useEffect(() => {
    if (!connected || paused) return;
    let mounted = true;
    const refresh = async () => {
      if (refreshingRef.current) return;
      refreshingRef.current = true;
      try {
        const status = await invoke<ModbusStatus>("modbus_status", {
          sessionId,
          afterSequence: cursorRef.current,
          transactionLimit: 250,
        });
        if (!mounted) return;
        const incoming = status.transactions.records;
        if (incoming.length > 0) {
          cursorRef.current = incoming[incoming.length - 1].sequence;
          setRecords(current => {
            const known = new Set(current.map(record => record.sequence));
            const merged = [...current, ...incoming.filter(record => !known.has(record.sequence))];
            return merged.slice(-VIEW_LIMIT);
          });
        }
      } catch {
        // Session-level connection state owns error presentation; transient history refresh failures retry.
      } finally {
        refreshingRef.current = false;
      }
    };
    void refresh();
    const timer = window.setInterval(() => { void refresh(); }, 500);
    return () => { mounted = false; window.clearInterval(timer); };
  }, [connected, paused, sessionId]);

  const copy = (bytes: number[]) => void navigator.clipboard?.writeText(hex(bytes));
  const items = records.slice().reverse();
  const statusLabel = (status: TransactionStatus) => status === "success" ? "OK" : t(STATUS_KEY[status] ?? "modbus.statusProtocol");

  return <div className={styles.panelPage}>
    <section className={styles.workbenchSection}>
      <div className={styles.panelHeading}>
        <div><strong>{t("modbus.transactionsTitle")}</strong><span className={styles.hint}>{t("modbus.transactionsHint")}</span></div>
        <div className={styles.actions}>
          <button className="liquid-glass-button" disabled={!connected} onClick={() => setPaused(value => !value)}>{paused ? t("modbus.transactionsResume") : t("modbus.transactionsPause")}</button>
          <button className="liquid-glass-button" disabled={records.length === 0} onClick={() => setRecords([])}>{t("modbus.transactionsClear")}</button>
        </div>
      </div>
      {!connected && <div className={styles.emptyState}>{t("modbus.transactionsDisconnected")}</div>}
    </section>
    <section className={styles.workbenchSection}>
      <div className={styles.tableWrap}>
        <table className={styles.table}>
          <thead><tr><th>{t("modbus.columnTime")}</th><th>{t("modbus.columnResult")}</th><th>{t("modbus.columnUnit")}</th><th>{t("modbus.columnFunction")}</th><th>TID</th><th>{t("modbus.columnLatency")}</th><th>{t("modbus.columnAttempt")}</th><th>{t("modbus.columnFrames")}</th></tr></thead>
          <tbody>{items.length === 0 ? <tr><td colSpan={8} className={styles.empty}>{t("modbus.transactionsEmpty")}</td></tr> : items.map(record => {
            const item = record.result;
            return <tr key={record.sequence}>
              <td className={styles.mono}>{formatTimestamp(item.timestamp_ms)}</td>
              <td title={item.message ?? ""}>{statusLabel(item.status)}{item.exception_code != null ? ` · ${STANDARD_EXCEPTIONS[item.exception_code] ?? `0x${item.exception_code.toString(16)}`}` : ""}{item.write_outcome_unknown ? ` · ${t("modbus.outcomeUnknown")}` : ""}</td>
              <td>{item.unit_id}</td>
              <td>{item.function === 0 ? "Raw ADU" : `0x${item.function.toString(16).padStart(2, "0").toUpperCase()}`}</td>
              <td>{item.transaction_id ?? "—"}</td>
              <td>{item.latency_ms} ms</td>
              <td>{item.attempt + 1}</td>
              <td><details className={styles.rawDetails}><summary className={styles.mono}>TX {hex(item.raw_tx).slice(0, 48)}{item.raw_tx.length > 24 ? "…" : ""}</summary><div className={styles.rawFrames}><div className={styles.mono}>TX {hex(item.raw_tx) || "—"}</div><div className={styles.mono}>RX {hex(item.raw_rx) || "—"}</div>{item.response_pdu.length > 0 && <div className={styles.mono}>PDU {hex(item.response_pdu)}</div>}<div className={styles.actions}><button className="liquid-glass-button" onClick={() => copy(item.raw_tx)}>{t("modbus.copyTx")}</button><button className="liquid-glass-button" onClick={() => copy(item.raw_rx)}>{t("modbus.copyRx")}</button>{item.response_pdu.length > 0 && <button className="liquid-glass-button" onClick={() => copy(item.response_pdu)}>{t("modbus.copyPdu")}</button>}</div></div></details></td>
            </tr>;
          })}</tbody>
        </table>
      </div>
    </section>
  </div>;
}
