import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import styles from "../Modbus.module.css";
import type { ModbusStatus, ServerSnapshot } from "../model";

type Area = "coil" | "discrete_input" | "holding_register" | "input_register";

export default function ServerPanel({ sessionId, connected }: { sessionId: string; connected: boolean }) {
  const [snapshot, setSnapshot] = useState<ServerSnapshot | null>(null);
  const [area, setArea] = useState<Area>("holding_register");
  const [address, setAddress] = useState(0);
  const [value, setValue] = useState(0);
  const [error, setError] = useState("");
  const cursorRef = useRef(0);

  const refreshSnapshot = useCallback(() => {
    if (!connected) return;
    void invoke<ServerSnapshot>("modbus_server_snapshot", { sessionId })
      .then(setSnapshot)
      .catch(cause => setError(String(cause)));
  }, [connected, sessionId]);

  useEffect(() => {
    setError("");
    cursorRef.current = 0;
    if (!connected) {
      setSnapshot(null);
      return;
    }
    refreshSnapshot();
    let mounted = true;
    const refreshIfChanged = async () => {
      try {
        const status = await invoke<ModbusStatus>("modbus_status", {
          sessionId,
          afterSequence: cursorRef.current,
          transactionLimit: 100,
        });
        if (!mounted) return;
        const records = status.transactions.records;
        if (records.length > 0) {
          cursorRef.current = records[records.length - 1].sequence;
          refreshSnapshot();
        }
      } catch {
        // Runtime/session state presents connection failures; retry the lightweight cursor poll.
      }
    };
    const timer = window.setInterval(() => { void refreshIfChanged(); }, 750);
    return () => { mounted = false; window.clearInterval(timer); };
  }, [connected, refreshSnapshot, sessionId]);

  const entries = useMemo(() => {
    if (!snapshot) return [] as [number, boolean | number][];
    if (area === "coil") return snapshot.coils;
    if (area === "discrete_input") return snapshot.discrete_inputs;
    if (area === "input_register") return snapshot.input_registers;
    return snapshot.holding_registers;
  }, [area, snapshot]);

  const definePoint = async () => {
    if (!connected) return;
    setError("");
    try {
      await invoke("modbus_server_set_value", { sessionId, area, address, value, fault: null });
      refreshSnapshot();
    } catch (cause) {
      setError(String(cause));
    }
  };

  return <div className={styles.panelPage}>
    <section className={styles.workbenchSection}>
      <div className={styles.panelHeading}>
        <div><strong>模拟地址空间</strong><span className={styles.hint}>工作台负责定义和初始化数据点；协议侧 FC05/06/0F/10/16/17 只能写已定义地址，未定义地址返回 Illegal Data Address。</span></div>
      </div>
    </section>
    <section className={`${styles.workbenchSection} ${styles.serverSplit}`}>
      <div className={styles.serverEditor}>
        <div className={styles.subHeading}><strong>定义 / 更新数据点</strong><span className={styles.hint}>这里属于模拟器管理操作，不等同于发送一条 Modbus 写请求。</span></div>
        <label className={styles.field}><span className={styles.label}>区域</span><select className="liquid-glass-input liquid-glass-select" value={area} onChange={event => setArea(event.target.value as Area)}><option value="coil">Coils</option><option value="discrete_input">Discrete Inputs</option><option value="holding_register">Holding Registers</option><option value="input_register">Input Registers</option></select></label>
        <label className={styles.field}><span className={styles.label}>协议地址</span><input className="liquid-glass-input" type="number" min={0} max={65535} value={address} onChange={event => setAddress(Number(event.target.value))} /></label>
        <label className={styles.field}><span className={styles.label}>初始 / 当前值</span><input className="liquid-glass-input" type="number" min={0} max={area === "coil" || area === "discrete_input" ? 1 : 65535} value={value} onChange={event => setValue(Number(event.target.value))} /></label>
        <div className={styles.actions}>
          <button className="liquid-glass-button" disabled={!connected} onClick={() => void definePoint()}>应用数据点</button>
          <button className="liquid-glass-button" disabled={!connected} onClick={refreshSnapshot}>刷新</button>
        </div>
        {!connected && <span className={styles.hint}>连接 Server 会话后才能修改运行中的地址空间。</span>}
        {error && <span className={styles.error}>{error}</span>}
      </div>
      <div className={styles.serverTablePane}>
        <div className={styles.subHeading}><strong>当前区域</strong><span className={styles.hint}>协议事务发生后按事务游标刷新，不再每秒无条件搬运完整地址空间。</span></div>
        {!connected ? <div className={styles.emptyState}>会话未连接。连接后将显示当前数据点。</div> : <div className={styles.tableWrap}><table className={styles.table}><thead><tr><th>协议地址</th><th>传统引用</th><th>值</th></tr></thead><tbody>{entries.length === 0 ? <tr><td colSpan={3} className={styles.empty}>尚未定义数据点</td></tr> : entries.map(([entryAddress, entryValue]) => <tr key={entryAddress}><td>{entryAddress}</td><td>{reference(area, entryAddress)}</td><td className={styles.mono}>{String(entryValue)}</td></tr>)}</tbody></table></div>}
      </div>
    </section>
  </div>;
}

function reference(area: Area, address: number): string {
  const base = area === "coil" ? 1 : area === "discrete_input" ? 10001 : area === "input_register" ? 30001 : 40001;
  return String(base + address).padStart(5, "0");
}
