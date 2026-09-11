import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSession } from "../../context/SessionContext";
import styles from "./Modbus.module.css";
import type { ModbusMode, ModbusOperation, ServerFaultConfig, TransactionResult } from "./model";
import AdvancedPanel from "./components/AdvancedPanel";
import MonitorPanel from "./components/MonitorPanel";
import ReadWritePanel from "./components/ReadWritePanel";
import ServerPanel from "./components/ServerPanel";
import TransactionsPanel from "./components/TransactionsPanel";

type Page = "readwrite" | "monitor" | "transactions" | "advanced" | "server";

export default function ModbusSessionView({ sessionId }: { sessionId: string }) {
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const role = String(tab?.params?.role ?? "client") as "client" | "server";
  const mode = String(tab?.params?.mode ?? "tcp") as ModbusMode;
  const unit = Number(tab?.params?.unit_id ?? 1);
  const [page, setPage] = useState<Page>(role === "server" ? "server" : "readwrite");
  const [history, setHistory] = useState<TransactionResult[]>([]);

  const execute = async (request: ModbusOperation) => {
    const result = await invoke<TransactionResult>("modbus_execute", { sessionId, request });
    setHistory(current => [result, ...current].slice(0, 500));
    return result;
  };

  const pages = useMemo<[Page, string][]>(() => role === "server"
    ? [["server", "Server"], ["transactions", "事务"], ["advanced", "高级"]]
    : [["readwrite", "读写"], ["monitor", "监控"], ["transactions", "事务"], ["advanced", "高级"]], [role]);
  const endpoint = tab?.endpoint || (mode === "tcp"
    ? `${String(tab?.params?.host ?? "")}:${String(tab?.params?.port ?? 502)}`
    : String(tab?.params?.serial_port ?? ""));
  const serverFault = tab?.params?.server_fault && typeof tab.params.server_fault === "object"
    ? tab.params.server_fault as unknown as ServerFaultConfig
    : null;

  return <div className={styles.root}>
    <div className={styles.header}>
      <span className={styles.title}>Modbus</span>
      <span className={styles.badge}>{mode.toUpperCase()}</span>
      <span className={styles.badge}>{role === "server" ? "Server" : "Client"}</span>
      <span className={styles.meta}>{endpoint} · Unit {unit}</span>
    </div>
    <div className={styles.tabs}>{pages.map(([id, label]) => <button key={id} className={`${styles.tab} ${page === id ? styles.tabActive : ""}`} onClick={() => setPage(id)}>{label}</button>)}</div>
    <div className={styles.body}>
      {page === "readwrite" && <ReadWritePanel execute={execute} />}
      {page === "monitor" && <MonitorPanel sessionId={sessionId} />}
      {page === "transactions" && <TransactionsPanel sessionId={sessionId} fallback={history} />}
      {page === "advanced" && <AdvancedPanel sessionId={sessionId} execute={execute} mode={mode} role={role} initialFault={serverFault} />}
      {page === "server" && <ServerPanel sessionId={sessionId} />}
    </div>
  </div>;
}
