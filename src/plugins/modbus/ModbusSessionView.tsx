import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { useSession } from "../../context/SessionContext";
import styles from "./Modbus.module.css";
import {
  normalizeModbusSessionParams,
  requestOperation,
  unitIdMax,
  type ModbusMode,
  type ModbusOperation,
  type ModbusRequest,
  type ServerFaultConfig,
  type TransactionResult,
} from "./model";
import {
  modbusEndpointLabel,
  modbusSessionTitle,
} from "./presentation";
import AdvancedPanel from "./components/AdvancedPanel";
import MonitorPanel from "./components/MonitorPanel";
import ReadWritePanel from "./components/ReadWritePanel";
import ServerPanel from "./components/ServerPanel";
import TransactionsPanel from "./components/TransactionsPanel";

type Page = "readwrite" | "monitor" | "transactions" | "advanced" | "server";
type ClientAction = ModbusRequest | Extract<ModbusOperation, { kind: "raw_adu" }>;

export default function ModbusSessionView({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const { state, reconfigureSession } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const params = useMemo(() => normalizeModbusSessionParams(tab?.params ?? {}), [tab?.params]);
  const role = params.role;
  const mode = params.mode as ModbusMode;
  const connected = tab?.state === "connected" || tab?.state === "transferring";
  const [page, setPage] = useState<Page>(role === "server" ? "server" : "readwrite");
  const [targetUnit, setTargetUnit] = useState(params.unit_id);
  const identityNormalizationKey = useRef<string | null>(null);

  useEffect(() => {
    setPage(role === "server" ? "server" : "readwrite");
  }, [role, sessionId]);

  useEffect(() => {
    setTargetUnit(params.unit_id);
  }, [params.unit_id, sessionId]);

  const generatedTitle = modbusSessionTitle(params);
  const generatedEndpoint = modbusEndpointLabel(params);
  useEffect(() => {
    if (!tab || tab.state !== "disconnected") return;

    // The generated title is only an initial name. Once a session has a real
    // name, reconfiguration may update its endpoint summary but must not rename it.
    const nextName = tab.name === "Modbus @ modbus" ? generatedTitle : tab.name;
    const needsNormalization = tab.endpoint !== generatedEndpoint || tab.name !== nextName;
    if (!needsNormalization) {
      identityNormalizationKey.current = null;
      return;
    }

    const normalizationKey = `${sessionId}\u0000${tab.name}\u0000${tab.endpoint}\u0000${nextName}\u0000${generatedEndpoint}`;
    if (identityNormalizationKey.current === normalizationKey) return;
    identityNormalizationKey.current = normalizationKey;
    void reconfigureSession(
      sessionId,
      generatedEndpoint,
      params,
      nextName,
      false,
      undefined,
      false,
    );
  }, [generatedEndpoint, generatedTitle, params, reconfigureSession, sessionId, tab]);

  const execute = async (action: ClientAction) => {
    if (!connected) throw new Error(t("modbus.connectToExecute"));
    const operation: ModbusOperation = action.kind === "raw_adu"
      ? action
      : requestOperation(targetUnit, action);
    return invoke<TransactionResult>("modbus_execute", { sessionId, operation });
  };

  const pages = useMemo<[Page, string][]>(() => role === "server"
    ? [["server", t("modbus.tabServer")], ["transactions", t("modbus.tabTransactions")], ["advanced", t("modbus.tabAdvanced")]]
    : [["readwrite", t("modbus.tabReadWrite")], ["monitor", t("modbus.tabMonitor")], ["transactions", t("modbus.tabTransactions")], ["advanced", t("modbus.tabAdvanced")]], [role, t]);
  const serverFault = params.server_fault as ServerFaultConfig;

  return <div className={styles.root} data-testid="tauterm-modbus-session-view">
    <nav className={styles.tabs} aria-label="Modbus workspace tabs">
      {pages.map(([id, label]) => <button key={id} type="button" className={`${styles.tabButton} liquid-glass-button ${page === id ? "liquid-theme-selected" : ""}`} aria-pressed={page === id} onClick={() => setPage(id)}>{label}</button>)}
    </nav>

    <div className={styles.body}>
      <main key={sessionId} className={styles.workspaceSurface}>
        {page === "readwrite" && <ReadWritePanel execute={execute} connected={connected} targetUnit={targetUnit} maxUnitId={unitIdMax(mode)} onTargetUnitChange={setTargetUnit} />}
        {page === "monitor" && <MonitorPanel sessionId={sessionId} connected={connected} mode={mode} defaultUnitId={params.unit_id} />}
        {page === "transactions" && <TransactionsPanel sessionId={sessionId} connected={connected} />}
        {page === "advanced" && <AdvancedPanel sessionId={sessionId} execute={execute} mode={mode} role={role} initialFault={serverFault} connected={connected} />}
        {page === "server" && <ServerPanel sessionId={sessionId} connected={connected} />}
      </main>
    </div>
  </div>;
}
