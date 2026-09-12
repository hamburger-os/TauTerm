import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import styles from "../Modbus.module.css";
import { unitIdMax, type ModbusMode, type ModbusRequest, type ModbusStatus, type TransactionStatus, type ValueFormat, type WatchRow, type WatchValue } from "../model";

const DEFAULT_FORMAT: ValueFormat = {
  value_type: "uint16",
  byte_order: "big",
  word_order: "normal",
  scale: 1,
  offset: 0,
  unit: "",
  bit: null,
};

const REGISTER_TYPES: ValueFormat["value_type"][] = [
  "bool", "uint16", "int16", "uint32", "int32", "float32", "uint64", "int64", "float64", "hex", "binary", "ascii", "utf8",
];

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

const fixedRegisterWidth = (format: ValueFormat): number | null => {
  if (format.bit != null) return 1;
  switch (format.value_type) {
    case "bool":
    case "uint16":
    case "int16": return 1;
    case "uint32":
    case "int32":
    case "float32": return 2;
    case "uint64":
    case "int64":
    case "float64": return 4;
    default: return null;
  }
};

const isExactInteger64 = (format: ValueFormat) => format.value_type === "uint64" || format.value_type === "int64";

function readRequest(functionCode: number, address: number, quantity: number): ModbusRequest {
  switch (functionCode) {
    case 1: return { kind: "read_bits", area: "coils", address, quantity };
    case 2: return { kind: "read_bits", area: "discrete_inputs", address, quantity };
    case 4: return { kind: "read_registers", area: "input_registers", address, quantity };
    default: return { kind: "read_registers", area: "holding_registers", address, quantity };
  }
}

function requestFields(request: ModbusRequest): { functionCode: number; address: number; quantity: number } {
  if (request.kind === "read_bits") {
    return { functionCode: request.area === "coils" ? 1 : 2, address: request.address, quantity: request.quantity };
  }
  if (request.kind === "read_registers") {
    return { functionCode: request.area === "holding_registers" ? 3 : 4, address: request.address, quantity: request.quantity };
  }
  return { functionCode: 3, address: 0, quantity: 1 };
}

function defaultWatchRow(defaultUnitId: number): WatchRow {
  return {
    id: crypto.randomUUID(),
    enabled: true,
    name: "Holding 0",
    unit_id: defaultUnitId,
    request: { kind: "read_registers", area: "holding_registers", address: 0, quantity: 1 },
    period_ms: 1000,
    format: DEFAULT_FORMAT,
  };
}

function displayValue(value: unknown): string {
  if (Array.isArray(value)) return value.map(item => item === true ? "1" : item === false ? "0" : String(item)).join(" ");
  if (value === true) return "1";
  if (value === false) return "0";
  return value == null ? "—" : String(value);
}

function areaLabel(functionCode: number): string {
  return functionCode === 1 ? "Coils" : functionCode === 2 ? "Discrete" : functionCode === 4 ? "Input Reg" : "Holding";
}

export default function MonitorPanel({
  sessionId,
  connected,
  mode,
  defaultUnitId,
}: {
  sessionId: string;
  connected: boolean;
  mode: ModbusMode;
  defaultUnitId: number;
}) {
  const { t } = useTranslation();
  const [rows, setRows] = useState<WatchRow[]>(() => [defaultWatchRow(defaultUnitId)]);
  const [values, setValues] = useState<Record<string, WatchValue>>({});
  const [running, setRunning] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    const initial = defaultWatchRow(defaultUnitId);
    setRows([initial]);
    setSelectedId(initial.id);
    setValues({});
    setRunning(false);
    setError("");
  }, [defaultUnitId, sessionId]);

  useEffect(() => {
    if (!connected) {
      setRunning(false);
      setValues({});
      setError("");
      return;
    }
    let mounted = true;
    void invoke<ModbusStatus>("modbus_status", {
      sessionId,
      afterSequence: null,
      transactionLimit: null,
    })
      .then(status => {
        if (!mounted) return;
        if (status.watch_rows.length > 0) {
          setRows(status.watch_rows);
          setSelectedId(current => status.watch_rows.some(row => row.id === current) ? current : status.watch_rows[0].id);
        }
        setRunning(status.watch_running);
      })
      .catch(cause => {
        if (mounted) setError(String(cause));
      });
    return () => { mounted = false; };
  }, [connected, sessionId]);

  useEffect(() => {
    if (!running || !connected) return;
    const refresh = () => {
      void invoke<WatchValue[]>("modbus_watch_values", { sessionId })
        .then(items => setValues(Object.fromEntries(items.map(item => [item.row_id, item]))))
        .catch(() => undefined);
    };
    refresh();
    const timer = window.setInterval(refresh, 300);
    return () => window.clearInterval(timer);
  }, [connected, running, sessionId]);

  const patchRow = (id: string, patch: Partial<WatchRow>) => setRows(current => current.map(row => row.id === id ? { ...row, ...patch } : row));
  const patchFormat = (row: WatchRow, patch: Partial<ValueFormat>) => {
    const next = { ...DEFAULT_FORMAT, ...row.format, ...patch };
    if (isExactInteger64(next)) {
      next.scale = 1;
      next.offset = 0;
    }
    const width = fixedRegisterWidth(next);
    const request = requestFields(row.request);
    patchRow(row.id, {
      format: next,
      ...(request.functionCode > 2 && width != null
        ? { request: readRequest(request.functionCode, request.address, width) }
        : {}),
    });
  };
  const setReadField = (row: WatchRow, patch: Partial<{ functionCode: number; address: number; quantity: number }>) => {
    const current = requestFields(row.request);
    const next = { ...current, ...patch };
    const switchingToBits = next.functionCode <= 2;
    const format = switchingToBits ? undefined : (row.format ?? DEFAULT_FORMAT);
    const width = format ? fixedRegisterWidth(format) : null;
    if (!switchingToBits && width != null) next.quantity = width;
    patchRow(row.id, {
      request: readRequest(next.functionCode, next.address, next.quantity),
      format,
    });
  };
  const add = () => {
    const next = defaultWatchRow(defaultUnitId);
    next.name = `Watch ${rows.length + 1}`;
    setRows(current => [...current, next]);
    setSelectedId(next.id);
  };
  const remove = (id: string) => {
    setRows(current => {
      const next = current.filter(item => item.id !== id);
      if (selectedId === id) setSelectedId(next[0]?.id ?? null);
      return next;
    });
  };
  const apply = async (start: boolean) => {
    if (!connected) return;
    setError("");
    try {
      await invoke("modbus_watch_set", { sessionId, rows });
      await invoke(start ? "modbus_watch_start" : "modbus_watch_stop", { sessionId });
      setRunning(start);
      if (!start) setValues({});
    } catch (cause) {
      setError(String(cause));
    }
  };

  const selected = rows.find(row => row.id === selectedId) ?? rows[0] ?? null;
  const selectedRequest = selected ? requestFields(selected.request) : null;
  const selectedIsBits = selectedRequest ? selectedRequest.functionCode <= 2 : false;
  const selectedFormat = selected ? { ...DEFAULT_FORMAT, ...selected.format } : DEFAULT_FORMAT;
  const selectedWidth = selected && !selectedIsBits ? fixedRegisterWidth(selectedFormat) : null;
  const selectedExact64 = isExactInteger64(selectedFormat);
  const enabledCount = useMemo(() => rows.filter(row => row.enabled).length, [rows]);
  const statusLabel = (status?: TransactionStatus) => status === "success" ? "OK" : status ? t(STATUS_KEY[status] ?? "modbus.statusProtocol") : "—";

  return (
    <div className={styles.panelPage}>
      <section className={styles.workbenchSection}>
        <div className={styles.panelHeading}>
          <div>
            <strong>{t("modbus.monitorTitle")}</strong>
            <span className={styles.hint}>{t("modbus.monitorHint")}</span>
          </div>
          <div className={styles.actions}>
            <span className={`${styles.runtimeState} ${running ? styles.runtimeStateActive : ""}`}>{running ? t("modbus.monitorRunning", { enabled: enabledCount, total: rows.length }) : t("modbus.monitorStopped")}</span>
            <button className="liquid-glass-button" onClick={add}>{t("modbus.monitorAdd")}</button>
            <button className="liquid-glass-button" disabled={!connected} onClick={() => void apply(!running)}>{running ? t("modbus.monitorStop") : t("modbus.monitorStart")}</button>
          </div>
        </div>
        {!connected && <span className={styles.hint}>{t("modbus.monitorConnectHint")}</span>}
        {error && <span className={styles.error}>{error}</span>}
      </section>

      <section className={`${styles.workbenchSection} ${styles.monitorLayout}`}>
        <div className={styles.monitorTablePane}>
          <div className={styles.tableWrap}>
            <table className={`${styles.table} ${styles.monitorTable}`}>
              <thead><tr><th>{t("modbus.columnEnabled")}</th><th>{t("modbus.columnName")}</th><th>{t("modbus.columnUnit")}</th><th>{t("modbus.columnArea")}</th><th>{t("modbus.columnAddress")}</th><th>{t("modbus.columnType")}</th><th>{t("modbus.columnCurrentValue")}</th><th>{t("modbus.columnStatus")}</th><th /></tr></thead>
              <tbody>{rows.length === 0 ? <tr><td colSpan={9} className={styles.empty}>{t("modbus.monitorNoRows")}</td></tr> : rows.map(row => {
                const request = requestFields(row.request);
                const isBits = request.functionCode <= 2;
                const format = { ...DEFAULT_FORMAT, ...row.format };
                const current = values[row.id];
                return <tr key={row.id} className={row.id === selected?.id ? styles.rowSelected : ""} onClick={() => setSelectedId(row.id)}>
                  <td><input type="checkbox" checked={row.enabled} onChange={event => { event.stopPropagation(); patchRow(row.id, { enabled: event.target.checked }); }} /></td>
                  <td><input className="liquid-glass-input" value={row.name} onClick={event => event.stopPropagation()} onChange={event => patchRow(row.id, { name: event.target.value })} /></td>
                  <td><input className="liquid-glass-input" type="number" min={0} max={unitIdMax(mode)} value={row.unit_id} onClick={event => event.stopPropagation()} onChange={event => patchRow(row.id, { unit_id: Number(event.target.value) })} /></td>
                  <td><select className="liquid-glass-input liquid-glass-select" value={request.functionCode} onClick={event => event.stopPropagation()} onChange={event => setReadField(row, { functionCode: Number(event.target.value), quantity: 1 })}><option value={1}>Coils</option><option value={2}>Discrete</option><option value={3}>Holding</option><option value={4}>Input Reg</option></select></td>
                  <td><input className="liquid-glass-input" type="number" min={0} max={65535} value={request.address} onClick={event => event.stopPropagation()} onChange={event => setReadField(row, { address: Number(event.target.value) })} /></td>
                  <td>{isBits ? "Bit" : <select className="liquid-glass-input liquid-glass-select" value={format.value_type} onClick={event => event.stopPropagation()} onChange={event => patchFormat(row, { value_type: event.target.value as ValueFormat["value_type"], bit: null })}>{REGISTER_TYPES.map(item => <option key={item}>{item}</option>)}</select>}</td>
                  <td className={styles.mono}>{displayValue(current?.value)}{!isBits && current?.value != null && format.unit ? ` ${format.unit}` : ""}</td>
                  <td title={current?.message ?? ""} className={current?.status === "success" ? styles.success : current ? styles.warning : ""}>{statusLabel(current?.status)}</td>
                  <td><button className="liquid-glass-button" onClick={event => { event.stopPropagation(); remove(row.id); }} aria-label={t("modbus.deleteNamed", { name: row.name })}>×</button></td>
                </tr>;
              })}</tbody>
            </table>
          </div>
        </div>

        <aside className={styles.monitorInspector}>
          <div className={styles.subHeading}><strong>{t("modbus.monitorInspector")}</strong><span className={styles.hint}>{selected ? `${selected.name} · Unit ${selected.unit_id} · ${areaLabel(selectedRequest?.functionCode ?? 3)}` : t("modbus.monitorSelectRow")}</span></div>
          {selected && selectedRequest ? <div className={styles.inspectorGrid}>
            <label className={styles.field}><span className={styles.label}>{t("modbus.rwQuantity")}</span><input className="liquid-glass-input" type="number" min={1} disabled={!selectedIsBits && selectedWidth != null} value={selectedRequest.quantity} onChange={event => setReadField(selected, { quantity: Number(event.target.value) })} /></label>
            <label className={styles.field}><span className={styles.label}>{t("modbus.monitorPeriod")}</span><input className="liquid-glass-input" type="number" min={20} max={86400000} value={selected.period_ms} onChange={event => patchRow(selected.id, { period_ms: Number(event.target.value) })} /></label>
            {!selectedIsBits && <>
              <label className={styles.field}><span className={styles.label}>{t("modbus.monitorByteOrder")}</span><select className="liquid-glass-input liquid-glass-select" value={selectedFormat.byte_order} onChange={event => patchFormat(selected, { byte_order: event.target.value as ValueFormat["byte_order"] })}><option value="big">Big</option><option value="little">Little</option></select></label>
              <label className={styles.field}><span className={styles.label}>{t("modbus.monitorWordOrder")}</span><select className="liquid-glass-input liquid-glass-select" value={selectedFormat.word_order} disabled={selectedRequest.quantity <= 1} onChange={event => patchFormat(selected, { word_order: event.target.value as ValueFormat["word_order"] })}><option value="normal">Normal</option><option value="reverse">Reverse</option></select></label>
              <label className={styles.field}><span className={styles.label}>Scale</span><input className="liquid-glass-input" type="number" step="any" value={selectedFormat.scale} disabled={selectedExact64} onChange={event => patchFormat(selected, { scale: Number(event.target.value) })} /></label>
              <label className={styles.field}><span className={styles.label}>Offset</span><input className="liquid-glass-input" type="number" step="any" value={selectedFormat.offset} disabled={selectedExact64} onChange={event => patchFormat(selected, { offset: Number(event.target.value) })} /></label>
              <label className={styles.field}><span className={styles.label}>Unit</span><input className="liquid-glass-input" value={selectedFormat.unit} onChange={event => patchFormat(selected, { unit: event.target.value })} /></label>
              <label className={styles.field}><span className={styles.label}>Bit</span><input className="liquid-glass-input" type="number" min={0} max={15} value={selectedFormat.bit ?? ""} onChange={event => patchFormat(selected, { bit: event.target.value === "" ? null : Number(event.target.value) })} /></label>
            </>}
          </div> : <div className={styles.emptyState}>{t("modbus.monitorEmptyInspector")}</div>}
        </aside>
      </section>
    </div>
  );
}
