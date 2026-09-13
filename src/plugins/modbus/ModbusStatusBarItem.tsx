import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import type { StatusBarContext } from "../../core/plugin-registry";
import styles from "./Modbus.module.css";
import { normalizeModbusSessionParams, type ModbusStatus, type TransactionStatus } from "./model";
import { modbusTypeLabel } from "./presentation";

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

function formatLatency(latencyMs: number): string {
  return latencyMs < 1 ? "<1 ms" : `${latencyMs} ms`;
}

export default function ModbusStatusBarItem({ context }: { context: StatusBarContext }) {
  const { t } = useTranslation();
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
        includeWatchRows: false,
      })
        .then(next => { if (mounted) setStatus(next); })
        .catch(() => { if (mounted) setStatus(null); });
    };
    refresh();
    const timer = window.setInterval(refresh, 1000);
    return () => { mounted = false; window.clearInterval(timer); };
  }, [context.sessionId]);

  const type = modbusTypeLabel(params);
  const role = status?.role ?? params.role;
  const isClient = role === "client";
  const unit = status?.last_unit_id ?? status?.default_unit_id ?? params.unit_id;
  const transactionCount = status?.transactions.latest_sequence ?? 0;
  const last = status?.last_status;
  const lastLabel = last && last !== "success"
    ? t(STATUS_KEY[last] ?? "modbus.statusProtocol")
    : null;
  const latency = status?.last_latency_ms != null ? formatLatency(status.last_latency_ms) : null;
  const latencyPrefix = last === "success" ? (isClient ? "RTT" : "Proc") : "Time";

  return <div className={styles.statusBarPlugin}>
    <span className={styles.statusBarType}>{type}</span>
    <span>Unit {unit}</span>
    {isClient && status?.watch_total ? <span className={status.watch_running ? styles.statusBarActive : ""}>{t("modbus.statusWatch", { enabled: status.watch_enabled, total: status.watch_total })}{status.watch_running ? " ▶" : ""}</span> : null}
    {transactionCount > 0 ? <span>Txn {transactionCount}</span> : null}
    {lastLabel ? <span className={last === "broadcast" ? styles.statusBarSuccess : styles.statusBarWarning}>{lastLabel}</span> : null}
    {last === "success" && latency ? <span className={styles.statusBarSuccess}>{latencyPrefix} {latency}</span> : null}
    {last && last !== "success" && last !== "broadcast" && latency ? <span>{latencyPrefix} {latency}</span> : null}
  </div>;
}
