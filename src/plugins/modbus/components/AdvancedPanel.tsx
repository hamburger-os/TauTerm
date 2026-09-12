import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import styles from "../Modbus.module.css";
import {
  parseHex,
  parseU16List,
  STANDARD_EXCEPTIONS,
  type ModbusMode,
  type ModbusOperation,
  type ModbusRequest,
  type ServerFaultConfig,
  type TransactionResult,
} from "../model";
import ResultCard from "./ResultCard";

type ClientAction = ModbusRequest | Extract<ModbusOperation, { kind: "raw_adu" }>;

interface Props {
  sessionId: string;
  execute: (request: ClientAction) => Promise<TransactionResult>;
  mode: ModbusMode;
  role: "client" | "server";
  initialFault?: ServerFaultConfig | null;
  connected: boolean;
}

type Operation = "status" | "diagnostics" | "counter" | "event_log" | "server_id" | "read_file" | "write_file" | "fifo" | "device_id" | "canopen" | "raw_pdu" | "raw_adu";

const OPTIONS: { value: Operation; key: string }[] = [
  { value: "status", key: "advancedOpStatus" },
  { value: "diagnostics", key: "advancedOpDiagnostics" },
  { value: "counter", key: "advancedOpCounter" },
  { value: "event_log", key: "advancedOpEventLog" },
  { value: "server_id", key: "advancedOpServerId" },
  { value: "read_file", key: "advancedOpReadFile" },
  { value: "write_file", key: "advancedOpWriteFile" },
  { value: "fifo", key: "advancedOpFifo" },
  { value: "device_id", key: "advancedOpDeviceId" },
  { value: "canopen", key: "advancedOpCanopen" },
  { value: "raw_pdu", key: "advancedOpRawPdu" },
  { value: "raw_adu", key: "advancedOpRawAdu" },
];

export default function AdvancedPanel({ sessionId, execute, mode, role, initialFault, connected }: Props) {
  if (role === "server") return <ServerFaultPanel sessionId={sessionId} initial={initialFault} connected={connected} />;
  return <ClientAdvancedPanel execute={execute} mode={mode} connected={connected} />;
}

function ClientAdvancedPanel({ execute, mode, connected }: { execute: Props["execute"]; mode: ModbusMode; connected: boolean }) {
  const { t } = useTranslation();
  const [operation, setOperation] = useState<Operation>(OPTIONS[0].value);
  const [address, setAddress] = useState(0);
  const [fileNumber, setFileNumber] = useState(1);
  const [recordNumber, setRecordNumber] = useState(0);
  const [recordLength, setRecordLength] = useState(1);
  const [values, setValues] = useState("0");
  const [subFunction, setSubFunction] = useState(0);
  const [deviceReadCode, setDeviceReadCode] = useState(1);
  const [deviceObjectId, setDeviceObjectId] = useState(0);
  const [meiData, setMeiData] = useState("00");
  const [functionCode, setFunctionCode] = useState("2B");
  const [rawData, setRawData] = useState("");
  const [rawAdu, setRawAdu] = useState("");
  const [waitResponse, setWaitResponse] = useState(true);
  const [quietPeriod, setQuietPeriod] = useState(50);
  const [result, setResult] = useState<TransactionResult | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setResult(null);
    setError("");
  }, [mode]);

  const run = async () => {
    if (!connected) return;
    setBusy(true);
    setError("");
    try {
      let request: ClientAction;
      switch (operation) {
        case "status": request = { kind: "read_exception_status" }; break;
        case "diagnostics": request = { kind: "diagnostics", sub_function: subFunction, data: parseHex(rawData) }; break;
        case "counter": request = { kind: "get_comm_event_counter" }; break;
        case "event_log": request = { kind: "get_comm_event_log" }; break;
        case "server_id": request = { kind: "report_server_id" }; break;
        case "read_file": request = { kind: "read_file_record", records: [{ file_number: fileNumber, record_number: recordNumber, record_length: recordLength }] }; break;
        case "write_file": request = { kind: "write_file_record", records: [{ file_number: fileNumber, record_number: recordNumber, values: parseU16List(values) }] }; break;
        case "fifo": request = { kind: "read_fifo_queue", address }; break;
        case "device_id": request = { kind: "mei", mei_type: 0x0e, data: [deviceReadCode, deviceObjectId] }; break;
        case "canopen": request = { kind: "mei", mei_type: 0x0d, data: parseHex(meiData) }; break;
        case "raw_pdu": request = { kind: "raw", function: Number.parseInt(functionCode, 16), data: parseHex(rawData) }; break;
        case "raw_adu": request = { kind: "raw_adu", data: parseHex(rawAdu), wait_response: waitResponse, quiet_period_ms: quietPeriod }; break;
      }
      setResult(await execute(request));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };

  const envelope = mode === "tcp"
    ? t("modbus.advancedEnvelopeTcp")
    : mode === "rtu"
      ? t("modbus.advancedEnvelopeRtu")
      : t("modbus.advancedEnvelopeAscii");

  return <div className={styles.panelPage}>
    <section className={styles.workbenchSection}>
      <div className={styles.panelHeading}><div><strong>{t("modbus.advancedTitle")}</strong><span className={styles.hint}>{t("modbus.advancedHint")}</span></div></div>
      <label className={styles.field}><span className={styles.label}>{t("modbus.advancedOperation")}</span><select className="liquid-glass-input liquid-glass-select" value={operation} onChange={event => setOperation(event.target.value as Operation)}>{OPTIONS.map(option => <option key={option.value} value={option.value}>{t(`modbus.${option.key}`)}</option>)}</select></label>
      {(operation === "read_file" || operation === "write_file") && <div className={styles.grid}>
        <Field label={t("modbus.advancedFileNumber")}><NumberInput value={fileNumber} set={setFileNumber} /></Field>
        <Field label={t("modbus.advancedRecordNumber")}><NumberInput value={recordNumber} set={setRecordNumber} /></Field>
        {operation === "read_file" ? <Field label={t("modbus.advancedRecordLength")}><NumberInput value={recordLength} set={setRecordLength} min={1} /></Field> : <Field label={t("modbus.advancedRegisterValues")}><input className="liquid-glass-input" value={values} onChange={event => setValues(event.target.value)} /></Field>}
      </div>}
      {operation === "fifo" && <Field label={t("modbus.advancedFifoAddress")}><NumberInput value={address} set={setAddress} /></Field>}
      {operation === "diagnostics" && <div className={styles.twoColumns}><Field label={t("modbus.advancedSubFunction")}><NumberInput value={subFunction} set={setSubFunction} /></Field><Field label={t("modbus.advancedDiagnosticData")}><input className="liquid-glass-input" value={rawData} onChange={event => setRawData(event.target.value)} /></Field></div>}
      {operation === "device_id" && <div className={styles.twoColumns}>
        <Field label={t("modbus.advancedDeviceReadCode")}><select className="liquid-glass-input liquid-glass-select" value={deviceReadCode} onChange={event => setDeviceReadCode(Number(event.target.value))}><option value={1}>{t("modbus.advancedDeviceBasic")}</option><option value={2}>{t("modbus.advancedDeviceRegular")}</option><option value={3}>{t("modbus.advancedDeviceExtended")}</option><option value={4}>{t("modbus.advancedDeviceIndividual")}</option></select></Field>
        <Field label={t("modbus.advancedObjectId")}><NumberInput value={deviceObjectId} set={setDeviceObjectId} max={255} /></Field>
      </div>}
      {operation === "canopen" && <Field label={t("modbus.advancedCanopenData")}><input className="liquid-glass-input" value={meiData} onChange={event => setMeiData(event.target.value)} placeholder="00" /></Field>}
      {operation === "raw_pdu" && <><div className={styles.notice}>{t("modbus.advancedRawPduNotice", { envelope })}</div><div className={styles.twoColumns}><Field label={t("modbus.advancedFunctionCodeHex")}><input className="liquid-glass-input" value={functionCode} onChange={event => setFunctionCode(event.target.value)} /></Field><Field label={t("modbus.advancedPduDataHex")}><input className="liquid-glass-input" value={rawData} onChange={event => setRawData(event.target.value)} /></Field></div></>}
      {operation === "raw_adu" && <><div className={`${styles.notice} ${styles.warning}`}>{t("modbus.advancedRawAduNotice")}</div><Field label={t("modbus.advancedFullAdu")}><textarea className={`${styles.textarea} liquid-glass-input`} value={rawAdu} onChange={event => setRawAdu(event.target.value)} /></Field><div className={styles.twoColumns}><label className="liquid-glass-toggle"><input type="checkbox" checked={waitResponse} onChange={event => setWaitResponse(event.target.checked)} /><div/><span>{t("modbus.advancedWaitRawResponse")}</span></label><Field label={t("modbus.advancedQuietPeriod")}><NumberInput value={quietPeriod} set={setQuietPeriod} min={1} max={1000} /></Field></div></>}
      <div className={styles.actions}><button className="liquid-glass-button" disabled={busy || !connected} onClick={() => void run()}>{busy ? t("modbus.executing") : t("modbus.execute")}</button>{!connected && <span className={styles.hint}>{t("modbus.advancedConnectHint")}</span>}{error && <span className={styles.error}>{error}</span>}</div>
    </section>
    {result && <section className={styles.workbenchSection}><div className={styles.panelHeading}><div><strong>{t("modbus.advancedResult")}</strong></div></div><ResultCard result={result} /></section>}
  </div>;
}

function ServerFaultPanel({ sessionId, initial, connected }: { sessionId: string; initial?: ServerFaultConfig | null; connected: boolean }) {
  const { t } = useTranslation();
  const [fault, setFault] = useState<ServerFaultConfig>(initial ?? { no_response: false, delay_ms: 0, exception_code: null });
  const [saved, setSaved] = useState("");
  const [error, setError] = useState("");
  const apply = async () => {
    if (!connected) return;
    setSaved(""); setError("");
    try {
      await invoke("modbus_server_set_value", { sessionId, area: null, address: null, value: null, fault });
      setSaved(t("modbus.faultUpdated"));
    } catch (cause) { setError(String(cause)); }
  };
  return <div className={styles.panelPage}>
    <section className={styles.workbenchSection}>
      <div className={styles.panelHeading}><div><strong>{t("modbus.faultTitle")}</strong><span className={styles.hint}>{t("modbus.faultHint")}</span></div></div>
      <label className="liquid-glass-toggle"><input type="checkbox" checked={fault.no_response} onChange={event => setFault(current => ({ ...current, no_response: event.target.checked }))} /><div/><span>{t("modbus.faultNoResponse")}</span></label>
      <div className={styles.twoColumns}>
        <Field label={t("modbus.faultDelay")}><NumberInput value={fault.delay_ms} set={value => setFault(current => ({ ...current, delay_ms: value }))} min={0} max={60000} /></Field>
        <Field label={t("modbus.faultException")}><select className="liquid-glass-input liquid-glass-select" value={fault.exception_code ?? ""} onChange={event => setFault(current => ({ ...current, exception_code: event.target.value ? Number(event.target.value) : null }))}><option value="">{t("modbus.faultNormal")}</option>{Object.entries(STANDARD_EXCEPTIONS).map(([code, label]) => <option key={code} value={code}>{code} · {label}</option>)}</select></Field>
      </div>
      <div className={styles.actions}><button className="liquid-glass-button" disabled={!connected} onClick={() => void apply()}>{t("modbus.faultApply")}</button>{!connected && <span className={styles.hint}>{t("modbus.faultConnectHint")}</span>}{saved && <span className={styles.success}>{saved}</span>}{error && <span className={styles.error}>{error}</span>}</div>
    </section>
  </div>;
}

function Field({ label, children }: { label: string; children: React.ReactNode }) { return <label className={styles.field}><span className={styles.label}>{label}</span>{children}</label>; }
function NumberInput({ value, set, min = 0, max = 65535 }: { value: number; set: (value: number) => void; min?: number; max?: number }) { return <input className="liquid-glass-input" type="number" min={min} max={max} value={value} onChange={event => set(Number(event.target.value))} />; }
