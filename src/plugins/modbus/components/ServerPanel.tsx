import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import styles from "../Modbus.module.css";
import type { ModbusStatus, ServerSnapshot } from "../model";

type Area = "coil" | "discrete_input" | "holding_register" | "input_register";

export default function ServerPanel({ sessionId, connected }: { sessionId: string; connected: boolean }) {
  const { t } = useTranslation();
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
          includeWatchRows: false,
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
        <div><strong>{t("modbus.serverAddressSpace")}</strong><span className={styles.hint}>{t("modbus.serverAddressSpaceHint")}</span></div>
      </div>
    </section>
    <section className={`${styles.workbenchSection} ${styles.serverSplit}`}>
      <div className={styles.serverEditor}>
        <div className={styles.subHeading}><strong>{t("modbus.serverDefinePoint")}</strong><span className={styles.hint}>{t("modbus.serverDefineHint")}</span></div>
        <label className={styles.field}><span className={styles.label}>{t("modbus.columnArea")}</span><select className="liquid-glass-input liquid-glass-select" value={area} onChange={event => setArea(event.target.value as Area)}><option value="coil">Coils</option><option value="discrete_input">Discrete Inputs</option><option value="holding_register">Holding Registers</option><option value="input_register">Input Registers</option></select></label>
        <label className={styles.field}><span className={styles.label}>{t("modbus.columnProtocolAddress")}</span><input className="liquid-glass-input" type="number" min={0} max={65535} value={address} onChange={event => setAddress(Number(event.target.value))} /></label>
        <label className={styles.field}><span className={styles.label}>{t("modbus.serverInitialCurrentValue")}</span><input className="liquid-glass-input" type="number" min={0} max={area === "coil" || area === "discrete_input" ? 1 : 65535} value={value} onChange={event => setValue(Number(event.target.value))} /></label>
        <div className={styles.actions}>
          <button className="liquid-glass-button" disabled={!connected} onClick={() => void definePoint()}>{t("modbus.serverApplyPoint")}</button>
          <button className="liquid-glass-button" disabled={!connected} onClick={refreshSnapshot}>{t("modbus.refresh")}</button>
        </div>
        {!connected && <span className={styles.hint}>{t("modbus.serverConnectHint")}</span>}
        {error && <span className={styles.error}>{error}</span>}
      </div>
      <div className={styles.serverTablePane}>
        <div className={styles.subHeading}><strong>{t("modbus.serverCurrentArea")}</strong><span className={styles.hint}>{t("modbus.serverRefreshHint")}</span></div>
        {!connected ? <div className={styles.emptyState}>{t("modbus.serverDisconnected")}</div> : <div className={styles.tableWrap}><table className={styles.table}><thead><tr><th>{t("modbus.columnProtocolAddress")}</th><th>{t("modbus.columnTraditionalReference")}</th><th>{t("modbus.columnValue")}</th></tr></thead><tbody>{entries.length === 0 ? <tr><td colSpan={3} className={styles.empty}>{t("modbus.serverNoPoints")}</td></tr> : entries.map(([entryAddress, entryValue]) => <tr key={entryAddress}><td>{entryAddress}</td><td>{reference(area, entryAddress)}</td><td className={styles.mono}>{String(entryValue)}</td></tr>)}</tbody></table></div>}
      </div>
    </section>
  </div>;
}

function reference(area: Area, address: number): string {
  const base = area === "coil" ? 1 : area === "discrete_input" ? 10001 : area === "input_register" ? 30001 : 40001;
  return String(base + address).padStart(5, "0");
}
