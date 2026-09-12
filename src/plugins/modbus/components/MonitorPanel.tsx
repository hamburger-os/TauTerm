import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import styles from "../Modbus.module.css";
import type { ModbusRequest, ModbusStatus, ValueFormat, WatchRow, WatchValue } from "../model";

const DEFAULT_FORMAT: ValueFormat = {
  value_type: "uint16",
  byte_order: "ABCD",
  scale: 1,
  offset: 0,
  unit: "",
  bit: null,
};

function readRequest(functionCode: number, address: number, quantity: number): ModbusRequest {
  if (functionCode === 1 || functionCode === 2) return { kind: "read_bits", function: functionCode, address, quantity };
  return { kind: "read_registers", function: functionCode === 4 ? 4 : 3, address, quantity };
}

function requestFields(request: ModbusRequest): { functionCode: number; address: number; quantity: number } {
  if (request.kind === "read_bits" || request.kind === "read_registers") {
    return { functionCode: request.function, address: request.address, quantity: request.quantity };
  }
  return { functionCode: 3, address: 0, quantity: 1 };
}

function defaultWatchRow(): WatchRow {
  return {
    id: crypto.randomUUID(),
    enabled: true,
    name: "Holding 0",
    request: { kind: "read_registers", function: 3, address: 0, quantity: 1 },
    period_ms: 1000,
    format: DEFAULT_FORMAT,
  };
}

export default function MonitorPanel({ sessionId }: { sessionId: string }) {
  const [rows, setRows] = useState<WatchRow[]>(() => [defaultWatchRow()]);
  const [values, setValues] = useState<Record<string, WatchValue>>({});
  const [running, setRunning] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let mounted = true;
    setRows([defaultWatchRow()]);
    setValues({});
    setRunning(false);
    setError("");
    void invoke<ModbusStatus>("modbus_status", { sessionId })
      .then(status => {
        if (mounted && status.watch_rows.length > 0) setRows(status.watch_rows);
      })
      .catch(cause => {
        if (mounted) setError(String(cause));
      });
    return () => { mounted = false; };
  }, [sessionId]);

  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => {
      void invoke<WatchValue[]>("modbus_watch_values", { sessionId })
        .then(items => setValues(Object.fromEntries(items.map(item => [item.row_id, item]))))
        .catch(() => undefined);
    }, 300);
    return () => window.clearInterval(timer);
  }, [running, sessionId]);

  useEffect(() => () => { void invoke("modbus_watch_stop", { sessionId }).catch(() => undefined); }, [sessionId]);

  const patchRow = (id: string, patch: Partial<WatchRow>) => setRows(current => current.map(row => row.id === id ? { ...row, ...patch } : row));
  const patchFormat = (row: WatchRow, patch: Partial<ValueFormat>) => patchRow(row.id, { format: { ...DEFAULT_FORMAT, ...row.format, ...patch } });
  const setReadField = (row: WatchRow, patch: Partial<{ functionCode: number; address: number; quantity: number }>) => {
    const current = requestFields(row.request);
    const next = { ...current, ...patch };
    patchRow(row.id, { request: readRequest(next.functionCode, next.address, next.quantity) });
  };
  const add = () => setRows(current => [...current, {
    id: crypto.randomUUID(),
    enabled: true,
    name: `Watch ${current.length + 1}`,
    request: { kind: "read_registers", function: 3, address: 0, quantity: 1 },
    period_ms: 1000,
    format: DEFAULT_FORMAT,
  }]);
  const apply = async (start: boolean) => {
    setError("");
    try {
      await invoke("modbus_watch_set", { sessionId, rows });
      await invoke(start ? "modbus_watch_start" : "modbus_watch_stop", { sessionId });
      setRunning(start);
    } catch (cause) {
      setError(String(cause));
    }
  };

  return (
    <div className={`${styles.card} liquid-glass-card`}>
      <div className={styles.actions}>
        <button className="liquid-glass-button" onClick={add}>添加监控项</button>
        <button className="liquid-glass-button" onClick={() => void apply(!running)}>{running ? "停止轮询" : "开始轮询"}</button>
        <span className={styles.hint}>慢请求完成后再安排下一周期，不积压、不重入。</span>
        {error && <span className={styles.error}>{error}</span>}
      </div>
      <div className={styles.tableWrap}>
        <table className={styles.table}>
          <thead><tr><th>启用</th><th>名称</th><th>区域</th><th>地址</th><th>数量</th><th>类型</th><th>字节/字序</th><th>Scale</th><th>Offset</th><th>Unit</th><th>Bit</th><th>周期 ms</th><th>当前值</th><th>状态</th><th /></tr></thead>
          <tbody>{rows.map(row => {
            const request = requestFields(row.request);
            const format = { ...DEFAULT_FORMAT, ...row.format };
            const current = values[row.id];
            const maxQuantity = request.functionCode <= 2 ? 2000 : 125;
            return <tr key={row.id}>
              <td><input type="checkbox" checked={row.enabled} onChange={event => patchRow(row.id, { enabled: event.target.checked })} /></td>
              <td><input className="liquid-glass-input" value={row.name} onChange={event => patchRow(row.id, { name: event.target.value })} /></td>
              <td><select className="liquid-glass-input liquid-glass-select" value={request.functionCode} onChange={event => setReadField(row, { functionCode: Number(event.target.value), quantity: 1 })}><option value={1}>Coils</option><option value={2}>Discrete Inputs</option><option value={3}>Holding Registers</option><option value={4}>Input Registers</option></select></td>
              <td><input className="liquid-glass-input" type="number" min={0} max={65535} value={request.address} onChange={event => setReadField(row, { address: Number(event.target.value) })} /></td>
              <td><input className="liquid-glass-input" type="number" min={1} max={maxQuantity} value={request.quantity} onChange={event => setReadField(row, { quantity: Number(event.target.value) })} /></td>
              <td><select className="liquid-glass-input liquid-glass-select" value={format.value_type} disabled={request.functionCode <= 2} onChange={event => patchFormat(row, { value_type: event.target.value as ValueFormat["value_type"] })}>{["bool","uint16","int16","uint32","int32","float32","uint64","int64","float64","hex","binary","ascii","utf8"].map(value => <option key={value}>{value}</option>)}</select></td>
              <td><select className="liquid-glass-input liquid-glass-select" value={format.byte_order} disabled={request.functionCode <= 2} onChange={event => patchFormat(row, { byte_order: event.target.value as ValueFormat["byte_order"] })}>{["ABCD","BADC","CDAB","DCBA"].map(value => <option key={value}>{value}</option>)}</select></td>
              <td><input className="liquid-glass-input" type="number" step="any" value={format.scale} disabled={request.functionCode <= 2} onChange={event => patchFormat(row, { scale: Number(event.target.value) })} /></td>
              <td><input className="liquid-glass-input" type="number" step="any" value={format.offset} disabled={request.functionCode <= 2} onChange={event => patchFormat(row, { offset: Number(event.target.value) })} /></td>
              <td><input className="liquid-glass-input" value={format.unit} disabled={request.functionCode <= 2} onChange={event => patchFormat(row, { unit: event.target.value })} /></td>
              <td><input className="liquid-glass-input" type="number" min={0} max={15} value={format.bit ?? ""} disabled={request.functionCode <= 2} onChange={event => patchFormat(row, { bit: event.target.value === "" ? null : Number(event.target.value) })} /></td>
              <td><input className="liquid-glass-input" type="number" min={20} value={row.period_ms} onChange={event => patchRow(row.id, { period_ms: Number(event.target.value) })} /></td>
              <td className={styles.mono}>{current?.value == null ? "—" : `${String(current.value)}${format.unit ? ` ${format.unit}` : ""}`}</td>
              <td title={current?.message ?? ""}>{current?.status ?? "—"}</td>
              <td><button className="liquid-glass-button" onClick={() => setRows(list => list.filter(item => item.id !== row.id))}>×</button></td>
            </tr>;
          })}</tbody>
        </table>
      </div>
    </div>
  );
}
