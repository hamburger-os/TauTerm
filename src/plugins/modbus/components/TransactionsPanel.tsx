import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import styles from "../Modbus.module.css";
import { hex, STANDARD_EXCEPTIONS, type ModbusStatus, type TransactionResult } from "../model";

export default function TransactionsPanel({ sessionId, fallback, connected }: { sessionId: string; fallback: TransactionResult[]; connected: boolean }) {
  const [items, setItems] = useState<TransactionResult[]>(fallback);

  useEffect(() => {
    setItems(fallback);
  }, [fallback, sessionId]);

  useEffect(() => {
    if (!connected) return;
    let mounted = true;
    const refresh = () => void invoke<ModbusStatus>("modbus_status", { sessionId })
      .then(status => { if (mounted) setItems(status.transactions); })
      .catch(() => undefined);
    refresh();
    const timer = window.setInterval(refresh, 500);
    return () => { mounted = false; window.clearInterval(timer); };
  }, [connected, sessionId]);

  const copy = (bytes: number[]) => void navigator.clipboard?.writeText(hex(bytes));

  return <div className={styles.panelPage}>
    <section className={styles.workbenchSection}>
      <div className={styles.panelHeading}>
        <div><strong>事务历史</strong><span className={styles.hint}>按时间查看请求结果、耗时、尝试次数和原始帧，用于定位超时、异常响应和链路问题。</span></div>
      </div>
      {!connected && <div className={styles.emptyState}>会话未连接。连接后将实时显示事务历史。</div>}
    </section>
    <section className={styles.workbenchSection}>
      <div className={styles.tableWrap}>
        <table className={styles.table}>
          <thead><tr><th>时间</th><th>结果</th><th>Unit</th><th>功能</th><th>TID</th><th>耗时</th><th>尝试</th><th>帧</th></tr></thead>
          <tbody>{items.length === 0 ? <tr><td colSpan={8} className={styles.empty}>尚无事务</td></tr> : items.map((item, index) => <tr key={`${item.timestamp_ms}-${item.transaction_id}-${index}`}>
            <td>{new Date(item.timestamp_ms).toLocaleTimeString()}</td>
            <td title={item.message ?? ""}>{item.status}{item.exception_code != null ? ` · ${STANDARD_EXCEPTIONS[item.exception_code] ?? `0x${item.exception_code.toString(16)}`}` : ""}{item.write_outcome_unknown ? " · 写入结果未知" : ""}</td>
            <td>{item.unit_id}</td>
            <td>{item.function === 0 ? "Raw ADU" : `0x${item.function.toString(16).padStart(2, "0").toUpperCase()}`}</td>
            <td>{item.transaction_id ?? "—"}</td>
            <td>{item.latency_ms} ms</td>
            <td>{item.attempt + 1}</td>
            <td><details className={styles.rawDetails}><summary className={styles.mono}>TX {hex(item.raw_tx).slice(0, 48)}{item.raw_tx.length > 24 ? "…" : ""}</summary><div className={styles.rawFrames}><div className={styles.mono}>TX {hex(item.raw_tx) || "—"}</div><div className={styles.mono}>RX {hex(item.raw_rx) || "—"}</div>{item.response_pdu.length > 0 && <div className={styles.mono}>PDU {hex(item.response_pdu)}</div>}<div className={styles.actions}><button className="liquid-glass-button" onClick={() => copy(item.raw_tx)}>复制 TX</button><button className="liquid-glass-button" onClick={() => copy(item.raw_rx)}>复制 RX</button>{item.response_pdu.length > 0 && <button className="liquid-glass-button" onClick={() => copy(item.response_pdu)}>复制 PDU</button>}</div></div></details></td>
          </tr>)}</tbody>
        </table>
      </div>
    </section>
  </div>;
}
