import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import styles from "../Modbus.module.css";
import type { ServerSnapshot } from "../model";

type Area = "coil" | "discrete_input" | "holding_register" | "input_register";

export default function ServerPanel({ sessionId }: { sessionId: string }) {
  const [snapshot, setSnapshot] = useState<ServerSnapshot | null>(null);
  const [area, setArea] = useState<Area>("holding_register");
  const [address, setAddress] = useState(0);
  const [value, setValue] = useState(0);
  const [error, setError] = useState("");

  const refresh = () => void invoke<ServerSnapshot>("modbus_server_snapshot", { sessionId }).then(setSnapshot).catch(cause => setError(String(cause)));
  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, 1000);
    return () => window.clearInterval(timer);
  }, [sessionId]);

  const entries = useMemo(() => {
    if (!snapshot) return [] as [number, boolean | number][];
    if (area === "coil") return snapshot.coils;
    if (area === "discrete_input") return snapshot.discrete_inputs;
    if (area === "input_register") return snapshot.input_registers;
    return snapshot.holding_registers;
  }, [area, snapshot]);

  const write = async () => {
    setError("");
    try {
      await invoke("modbus_server_set_value", { sessionId, area, address, value });
      refresh();
    } catch (cause) {
      setError(String(cause));
    }
  };

  return <div className={styles.split}>
    <div className={`${styles.card} liquid-glass-card`}>
      <strong>模拟数据模型</strong>
      <span className={styles.hint}>Coils / Discrete Inputs / Holding Registers / Input Registers 使用协议地址 0..65535。Client 写请求会实时反映到可写区域。</span>
      <label className={styles.field}><span className={styles.label}>区域</span><select className="liquid-glass-input liquid-glass-select" value={area} onChange={event => setArea(event.target.value as Area)}><option value="coil">Coils</option><option value="discrete_input">Discrete Inputs</option><option value="holding_register">Holding Registers</option><option value="input_register">Input Registers</option></select></label>
      <label className={styles.field}><span className={styles.label}>协议地址</span><input className="liquid-glass-input" type="number" min={0} max={65535} value={address} onChange={event => setAddress(Number(event.target.value))} /></label>
      <label className={styles.field}><span className={styles.label}>值</span><input className="liquid-glass-input" type="number" min={0} max={area === "coil" || area === "discrete_input" ? 1 : 65535} value={value} onChange={event => setValue(Number(event.target.value))} /></label>
      <button className="liquid-glass-button" onClick={() => void write()}>写入模拟数据</button>
      {error && <span className={styles.error}>{error}</span>}
    </div>
    <div className={`${styles.card} liquid-glass-card`}>
      <strong>当前区域</strong>
      <div className={styles.tableWrap}><table className={styles.table}><thead><tr><th>协议地址</th><th>传统引用</th><th>值</th></tr></thead><tbody>{entries.length === 0 ? <tr><td colSpan={3} className={styles.empty}>尚未定义数据点</td></tr> : entries.map(([entryAddress, entryValue]) => <tr key={entryAddress}><td>{entryAddress}</td><td>{reference(area, entryAddress)}</td><td>{String(entryValue)}</td></tr>)}</tbody></table></div>
    </div>
  </div>;
}

function reference(area: Area, address: number): string {
  const base = area === "coil" ? 1 : area === "discrete_input" ? 10001 : area === "input_register" ? 30001 : 40001;
  return String(base + address).padStart(5, "0");
}
