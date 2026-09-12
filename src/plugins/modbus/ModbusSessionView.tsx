import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import Icon, { type IconName } from "../../components/common/Icon";
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
  isGeneratedModbusSessionTitle,
  modbusConnectionSubtitle,
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

const statusPresentation: Record<string, { icon: IconName; labelKey: string }> = {
  disconnected: { icon: "status-disconnected", labelKey: "modbus.sessionDisconnected" },
  connecting: { icon: "status-connecting", labelKey: "modbus.sessionConnecting" },
  connected: { icon: "status-connected", labelKey: "modbus.sessionConnected" },
  transferring: { icon: "status-transferring", labelKey: "modbus.sessionTransferring" },
};

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

    const nextName = tab.name === "Modbus @ modbus" || isGeneratedModbusSessionTitle(tab.name)
      ? generatedTitle
      : tab.name;
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
  const summary = modbusConnectionSubtitle(params);
  const sessionTitle = tab?.name && tab.name !== "Modbus @ modbus" ? tab.name : generatedTitle;
  const status = statusPresentation[tab?.state ?? "disconnected"] ?? statusPresentation.disconnected;

  return <div className={styles.root} data-testid="tauterm-modbus-session-view">
    <header className={styles.workspaceHeader}>
      <div className={styles.identityGroup}>
        <Icon name="connection" size="md" />
        <div className={styles.identityText}>
          <div className={styles.title}>{sessionTitle}</div>
          <div className={styles.meta}>{summary}</div>
        </div>
      </div>
      <div className={styles.headerRuntime}>
        {role === "client" && <label className={styles.targetUnit}>
          <span>{t("modbus.targetUnit")}</span>
          <input
            className="liquid-glass-input"
            type="number"
            min={0}
            max={unitIdMax(mode)}
            value={targetUnit}
            onChange={event => setTargetUnit(Number(event.target.value))}
            aria-label="Modbus target Unit ID"
          />
        </label>}
        <span className={styles.statusLabel}><Icon name={status.icon} size="xs" />{t(status.labelKey)}</span>
      </div>
    </header>

    <nav className={`${styles.tabs} liquid-selector-strip`} aria-label="Modbus workspace tabs">
      {pages.map(([id, label]) => <button key={id} type="button" className={`liquid-selector-button ${page === id ? "liquid-theme-selected" : ""}`} aria-pressed={page === id} onClick={() => setPage(id)}>{label}</button>)}
    </nav>

    <div className={styles.body}>
      <main key={sessionId} className={styles.workspaceSurface}>
        {page === "readwrite" && <ReadWritePanel execute={execute} connected={connected} />}
        {page === "monitor" && <MonitorPanel sessionId={sessionId} connected={connected} mode={mode} defaultUnitId={params.unit_id} />}
        {page === "transactions" && <TransactionsPanel sessionId={sessionId} connected={connected} />}
        {page === "advanced" && <AdvancedPanel sessionId={sessionId} execute={execute} mode={mode} role={role} initialFault={serverFault} connected={connected} />}
        {page === "server" && <ServerPanel sessionId={sessionId} connected={connected} />}
      </main>
    </div>
  </div>;
}
