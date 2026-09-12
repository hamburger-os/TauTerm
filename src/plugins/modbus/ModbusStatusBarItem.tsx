import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { StatusBarContext } from "../../core/plugin-registry";
import styles from "./Modbus.module.css";
import { normalizeModbusSessionParams, type ModbusStatus, type TransactionStatus } from "./model";
import { modbusTypeLabel } from "./presentation";

const STATUS_LABEL: Record<TransactionStatus, string> = {
  success: "OK",
  broadcast: "Broadcast",
  modbus_exception: "Exception",
  protocol_error: "Protocol",
  malformed_response: "Malformed",
  timeout: "Timeout",
  transport_error: "Transport",
  cancelled: "Cancelled",
  fault_injected: "Fault",
};

export default function ModbusStatusBarItem({ context }: { context: StatusBarContext }) {
  const params = useMemo(
    () => normalizeModbusSessionParams(context.activeTab?.params ?? {}),
    [context.activeTab?.params],
  );
  const [status, setStatus] = useState<ModbusStatus | null>(null);

  useEffect(() => {
    let mounted = true;
    const refresh = () => {
      void invoke<ModbusStatus>("modbus_status", {
        sessionId: context.sessionId,
        afterSequence: null,
        transactionLimit: null,
      })
        .then(next => { if (mounted) setStatus(next); })
        .catch(() => { if (mounted) setStatus(null); });
    };
    refresh();
    const timer = window.setInterval(refresh, 1000);
    return () => { mounted = false; window.clearInterval(timer); };
  }, [context.sessionId]);

  const type = modbusTypeLabel(params);
  const unit = status?.last_unit_id ?? status?.default_unit_id ?? params.unit_id;
  const last = status?.last_status;

  return <div className={styles.statusBarPlugin}>
    <span className={styles.statusBarType}>{type}</span>
    <span>Unit {unit}</span>
    {status?.watch_total ? <span className={status.watch_running ? styles.statusBarActive : ""}>Watch {status.watch_enabled}/{status.watch_total}{status.watch_running ? " ▶" : ""}</span> : null}
    {last ? <span className={last === "success" || last === "broadcast" ? styles.statusBarSuccess : styles.statusBarWarning}>{STATUS_LABEL[last]}{status?.last_latency_ms != null ? ` ${status.last_latency_ms} ms` : ""}</span> : null}
  </div>;
}
