import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import styles from "../Modbus.module.css";
import { hex, STANDARD_EXCEPTIONS, type ModbusStatus, type TransactionRecord } from "../model";

const VIEW_LIMIT = 1000;

export default function TransactionsPanel({ sessionId, connected }: { sessionId: string; connected: boolean }) {
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

  return <div className={styles.panelPage}>
    <section className={styles.workbenchSection}>
      <div className={styles.panelHeading}>
        <div><strong>事务历史</strong><span className={styles.hint}>前端仅增量读取新事务，不再周期性复制完整历史。暂停只冻结当前视图，不影响 Watch 或协议运行时。</span></div>
        <div className={styles.actions}>
          <button className="liquid-glass-button" disabled={!connected} onClick={() => setPaused(value => !value)}>{paused ? "继续刷新" : "暂停刷新"}</button>
          <button className="liquid-glass-button" disabled={records.length === 0} onClick={() => setRecords([])}>清空视图</button>
        </div>
      </div>
      {!connected && <div className={styles.emptyState}>会话未连接。连接后将实时显示事务历史。</div>}
    </section>
    <section className={styles.workbenchSection}>
      <div className={styles.tableWrap}>
        <table className={styles.table}>
          <thead><tr><th>时间</th><th>结果</th><th>Unit</th><th>功能</th><th>TID</th><th>耗时</th><th>尝试</th><th>帧</th></tr></thead>
          <tbody>{items.length === 0 ? <tr><td colSpan={8} className={styles.empty}>尚无事务</td></tr> : items.map(record => {
            const item = record.result;
            return <tr key={record.sequence}>
              <td>{new Date(item.timestamp_ms).toLocaleTimeString()}</td>
              <td title={item.message ?? ""}>{item.status}{item.exception_code != null ? ` · ${STANDARD_EXCEPTIONS[item.exception_code] ?? `0x${item.exception_code.toString(16)}`}` : ""}{item.write_outcome_unknown ? " · 写入结果未知" : ""}</td>
              <td>{item.unit_id}</td>
              <td>{item.function === 0 ? "Raw ADU" : `0x${item.function.toString(16).padStart(2, "0").toUpperCase()}`}</td>
              <td>{item.transaction_id ?? "—"}</td>
              <td>{item.latency_ms} ms</td>
              <td>{item.attempt + 1}</td>
              <td><details className={styles.rawDetails}><summary className={styles.mono}>TX {hex(item.raw_tx).slice(0, 48)}{item.raw_tx.length > 24 ? "…" : ""}</summary><div className={styles.rawFrames}><div className={styles.mono}>TX {hex(item.raw_tx) || "—"}</div><div className={styles.mono}>RX {hex(item.raw_rx) || "—"}</div>{item.response_pdu.length > 0 && <div className={styles.mono}>PDU {hex(item.response_pdu)}</div>}<div className={styles.actions}><button className="liquid-glass-button" onClick={() => copy(item.raw_tx)}>复制 TX</button><button className="liquid-glass-button" onClick={() => copy(item.raw_rx)}>复制 RX</button>{item.response_pdu.length > 0 && <button className="liquid-glass-button" onClick={() => copy(item.response_pdu)}>复制 PDU</button>}</div></div></details></td>
            </tr>;
          })}</tbody>
        </table>
      </div>
    </section>
  </div>;
}
