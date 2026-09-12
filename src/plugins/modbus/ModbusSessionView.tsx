import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Icon, { type IconName } from "../../components/common/Icon";
import { useSession } from "../../context/SessionContext";
import styles from "./Modbus.module.css";
import {
  modbusConnectionSummary,
  modbusDefaultSessionName,
  modbusModeRoleLabel,
  normalizeModbusSessionParams,
  type ModbusMode,
  type ModbusOperation,
  type ServerFaultConfig,
  type TransactionResult,
} from "./model";
import AdvancedPanel from "./components/AdvancedPanel";
import MonitorPanel from "./components/MonitorPanel";
import ReadWritePanel from "./components/ReadWritePanel";
import ServerPanel from "./components/ServerPanel";
import TransactionsPanel from "./components/TransactionsPanel";

type Page = "readwrite" | "monitor" | "transactions" | "advanced" | "server";

const statusPresentation: Record<string, { icon: IconName; label: string }> = {
  disconnected: { icon: "status-disconnected", label: "未连接" },
  connecting: { icon: "status-connecting", label: "连接中" },
  connected: { icon: "status-connected", label: "已连接" },
  transferring: { icon: "status-transferring", label: "传输中" },
};

export default function ModbusSessionView({ sessionId }: { sessionId: string }) {
  const { state, renameTab } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const params = useMemo(() => normalizeModbusSessionParams(tab?.params ?? {}), [tab?.params]);
  const role = params.role;
  const mode = params.mode as ModbusMode;
  const connected = tab?.state === "connected" || tab?.state === "transferring";
  const [page, setPage] = useState<Page>(role === "server" ? "server" : "readwrite");
  const [history, setHistory] = useState<TransactionResult[]>([]);

  useEffect(() => {
    setPage(role === "server" ? "server" : "readwrite");
    setHistory([]);
  }, [role, sessionId]);

  useEffect(() => {
    if (tab?.name !== "Modbus @ modbus") return;
    void renameTab(sessionId, modbusDefaultSessionName(params));
  }, [params, renameTab, sessionId, tab?.name]);

  const execute = async (request: ModbusOperation) => {
    if (!connected) throw new Error("Modbus 会话尚未连接");
    const result = await invoke<TransactionResult>("modbus_execute", { sessionId, request });
    setHistory(current => [result, ...current].slice(0, 500));
    return result;
  };

  const pages = useMemo<[Page, string][]>(() => role === "server"
    ? [["server", "数据模型"], ["transactions", "事务"], ["advanced", "高级"]]
    : [["readwrite", "读写"], ["monitor", "监控"], ["transactions", "事务"], ["advanced", "高级"]], [role]);
  const serverFault = params.server_fault as ServerFaultConfig;
  const identity = modbusModeRoleLabel(params);
  const summary = modbusConnectionSummary(params);
  const sessionTitle = tab?.name && tab.name !== "Modbus @ modbus" ? tab.name : identity;
  const status = statusPresentation[tab?.state ?? "disconnected"] ?? statusPresentation.disconnected;

  return <div className={styles.root} data-testid="tauterm-modbus-session-view">
    <section className={`${styles.workspaceHeader} liquid-glass-card`}>
      <div className={styles.identityGroup}>
        <Icon name="connection" size="lg" />
        <div className={styles.identityText}>
          <div className={styles.title}>{sessionTitle}</div>
          <div className={styles.meta}>{identity} · {summary}</div>
        </div>
      </div>
      <div className={styles.statusRow}>
        <span className={styles.badge}>{mode.toUpperCase()}</span>
        <span className={styles.badge}>{role === "server" ? (mode === "tcp" ? "Server" : "Slave") : (mode === "tcp" ? "Client" : "Master")}</span>
        <span className={styles.statusLabel}><Icon name={status.icon} size="xs" />{status.label}</span>
      </div>
    </section>

    <nav className={styles.tabs} aria-label="Modbus workspace tabs">
      {pages.map(([id, label]) => <button key={id} type="button" className={`${styles.tab} ${page === id ? styles.tabActive : ""}`} onClick={() => setPage(id)}>{label}</button>)}
    </nav>

    <div className={styles.body}>
      <div className={styles.workspaceContent}>
        {page === "readwrite" && <ReadWritePanel execute={execute} connected={connected} />}
        {page === "monitor" && <MonitorPanel sessionId={sessionId} connected={connected} />}
        {page === "transactions" && <TransactionsPanel sessionId={sessionId} fallback={history} connected={connected} />}
        {page === "advanced" && <AdvancedPanel sessionId={sessionId} execute={execute} mode={mode} role={role} initialFault={serverFault} connected={connected} />}
        {page === "server" && <ServerPanel sessionId={sessionId} connected={connected} />}
      </div>
    </div>
  </div>;
}
