import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import styles from "../Modbus.module.css";
import {
  parseHex,
  parseU16List,
  STANDARD_EXCEPTIONS,
  type ModbusMode,
  type ModbusOperation,
  type ServerFaultConfig,
  type TransactionResult,
} from "../model";
import ResultCard from "./ResultCard";

interface Props {
  sessionId: string;
  execute: (request: ModbusOperation) => Promise<TransactionResult>;
  mode: ModbusMode;
  role: "client" | "server";
  initialFault?: ServerFaultConfig | null;
  connected: boolean;
}

type Operation = "status" | "diagnostics" | "counter" | "event_log" | "server_id" | "read_file" | "write_file" | "fifo" | "device_id" | "canopen" | "raw_pdu" | "raw_adu";

export default function AdvancedPanel({ sessionId, execute, mode, role, initialFault, connected }: Props) {
  if (role === "server") return <ServerFaultPanel sessionId={sessionId} initial={initialFault} connected={connected} />;
  return <ClientAdvancedPanel execute={execute} mode={mode} connected={connected} />;
}

function ClientAdvancedPanel({ execute, mode, connected }: { execute: Props["execute"]; mode: ModbusMode; connected: boolean }) {
  const serialOperations: { value: Operation; label: string }[] = useMemo(() => mode === "tcp" ? [] : [
    { value: "status", label: "07 · Read Exception Status" },
    { value: "diagnostics", label: "08 · Diagnostics" },
    { value: "counter", label: "0B · Get Comm Event Counter" },
    { value: "event_log", label: "0C · Get Comm Event Log" },
    { value: "server_id", label: "11 · Report Server ID" },
  ], [mode]);
  const options: { value: Operation; label: string }[] = [
    ...serialOperations,
    { value: "read_file", label: "14 · Read File Record" },
    { value: "write_file", label: "15 · Write File Record" },
    { value: "fifo", label: "18 · Read FIFO Queue" },
    { value: "device_id", label: "2B/0E · Read Device Identification" },
    { value: "canopen", label: "2B/0D · CANopen General Reference" },
    { value: "raw_pdu", label: "Raw PDU · 自动封装" },
    { value: "raw_adu", label: "Raw ADU · 原样发送" },
  ];
  const [operation, setOperation] = useState<Operation>(options[0]?.value ?? "read_file");
  const [address, setAddress] = useState(0);
  const [fileNumber, setFileNumber] = useState(1);
  const [recordNumber, setRecordNumber] = useState(0);
  const [recordLength, setRecordLength] = useState(1);
  const [values, setValues] = useState("0");
  const [subFunction, setSubFunction] = useState(0);
  const [meiData, setMeiData] = useState("01 00");
  const [functionCode, setFunctionCode] = useState("2B");
  const [rawData, setRawData] = useState("");
  const [rawAdu, setRawAdu] = useState("");
  const [waitResponse, setWaitResponse] = useState(true);
  const [quietPeriod, setQuietPeriod] = useState(50);
  const [result, setResult] = useState<TransactionResult | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const run = async () => {
    if (!connected) return;
    setBusy(true);
    setError("");
    try {
      let request: ModbusOperation;
      switch (operation) {
        case "status": request = { kind: "read_exception_status" }; break;
        case "diagnostics": request = { kind: "diagnostics", sub_function: subFunction, data: parseHex(rawData) }; break;
        case "counter": request = { kind: "get_comm_event_counter" }; break;
        case "event_log": request = { kind: "get_comm_event_log" }; break;
        case "server_id": request = { kind: "report_server_id" }; break;
        case "read_file": request = { kind: "read_file_record", records: [{ file_number: fileNumber, record_number: recordNumber, record_length: recordLength }] }; break;
        case "write_file": request = { kind: "write_file_record", records: [{ file_number: fileNumber, record_number: recordNumber, values: parseU16List(values) }] }; break;
        case "fifo": request = { kind: "read_fifo_queue", address }; break;
        case "device_id": request = { kind: "mei", mei_type: 0x0e, data: parseHex(meiData) }; break;
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

  return <div className={styles.panelPage}>
    <section className={styles.workbenchSection}>
      <div className={styles.panelHeading}><div><strong>高级事务</strong><span className={styles.hint}>诊断、文件记录、设备标识与原始帧集中在此；常规读写保持在“读写”页。</span></div></div>
      <label className={styles.field}><span className={styles.label}>高级操作</span><select className="liquid-glass-input liquid-glass-select" value={operation} onChange={event => setOperation(event.target.value as Operation)}>{options.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}</select></label>
      {(operation === "read_file" || operation === "write_file") && <div className={styles.grid}>
        <Field label="File Number"><NumberInput value={fileNumber} set={setFileNumber} /></Field>
        <Field label="Record Number"><NumberInput value={recordNumber} set={setRecordNumber} /></Field>
        {operation === "read_file" ? <Field label="Record Length"><NumberInput value={recordLength} set={setRecordLength} min={1} max={125} /></Field> : <Field label="寄存器值"><input className="liquid-glass-input" value={values} onChange={event => setValues(event.target.value)} /></Field>}
      </div>}
      {operation === "fifo" && <Field label="FIFO Pointer Address"><NumberInput value={address} set={setAddress} /></Field>}
      {operation === "diagnostics" && <div className={styles.twoColumns}><Field label="Sub-function"><NumberInput value={subFunction} set={setSubFunction} /></Field><Field label="Data (hex)"><input className="liquid-glass-input" value={rawData} onChange={event => setRawData(event.target.value)} /></Field></div>}
      {(operation === "device_id" || operation === "canopen") && <Field label={operation === "device_id" ? "Read Device ID Code + Object ID (hex)" : "MEI 0x0D Data (hex)"}><input className="liquid-glass-input" value={meiData} onChange={event => setMeiData(event.target.value)} placeholder={operation === "device_id" ? "01 00" : "00"} /></Field>}
      {operation === "raw_pdu" && <><div className={styles.notice}>Raw PDU 由 TauTerm 自动添加 {mode === "tcp" ? "MBAP + Unit ID" : mode === "rtu" ? "Unit ID + CRC" : "Unit ID + LRC + ASCII delimiters"}，输入内容本身不解释为完整 ADU。</div><div className={styles.twoColumns}><Field label="Function Code (hex)"><input className="liquid-glass-input" value={functionCode} onChange={event => setFunctionCode(event.target.value)} /></Field><Field label="PDU Data (hex)"><input className="liquid-glass-input" value={rawData} onChange={event => setRawData(event.target.value)} /></Field></div></>}
      {operation === "raw_adu" && <><div className={`${styles.notice} ${styles.warning}`}>Raw ADU 会逐字节原样发送，不自动修正 CRC/LRC、MBAP Length、Transaction ID、Byte Count 或任何协议字段，可用于故意发送畸形帧。</div><Field label="完整 ADU (hex)"><textarea className={`${styles.textarea} liquid-glass-input`} value={rawAdu} onChange={event => setRawAdu(event.target.value)} /></Field><div className={styles.twoColumns}><label className="liquid-glass-toggle"><input type="checkbox" checked={waitResponse} onChange={event => setWaitResponse(event.target.checked)} /><div/><span>等待原始响应</span></label><Field label="响应静默边界 (ms)"><NumberInput value={quietPeriod} set={setQuietPeriod} min={1} max={1000} /></Field></div></>}
      <div className={styles.actions}><button className="liquid-glass-button" disabled={busy || !connected} onClick={() => void run()}>{busy ? "执行中…" : "执行"}</button>{!connected && <span className={styles.hint}>连接会话后才能执行高级事务。</span>}{error && <span className={styles.error}>{error}</span>}</div>
    </section>
    {result && <section className={styles.workbenchSection}><div className={styles.panelHeading}><div><strong>结果</strong></div></div><ResultCard result={result} /></section>}
  </div>;
}

function ServerFaultPanel({ sessionId, initial, connected }: { sessionId: string; initial?: ServerFaultConfig | null; connected: boolean }) {
  const [fault, setFault] = useState<ServerFaultConfig>(initial ?? { no_response: false, delay_ms: 0, exception_code: null });
  const [saved, setSaved] = useState("");
  const [error, setError] = useState("");
  const apply = async () => {
    if (!connected) return;
    setSaved(""); setError("");
    try {
      await invoke("modbus_server_set_value", { sessionId, fault });
      setSaved("故障注入已更新");
    } catch (cause) { setError(String(cause)); }
  };
  return <div className={styles.panelPage}>
    <section className={styles.workbenchSection}>
      <div className={styles.panelHeading}><div><strong>Server 故障注入</strong><span className={styles.hint}>动态生效，不需要重建会话。无响应优先级最高，其次延迟，再返回指定标准异常。</span></div></div>
      <label className="liquid-glass-toggle"><input type="checkbox" checked={fault.no_response} onChange={event => setFault(current => ({ ...current, no_response: event.target.checked }))} /><div/><span>不响应请求</span></label>
      <div className={styles.twoColumns}>
        <Field label="响应延迟 (ms)"><NumberInput value={fault.delay_ms} set={value => setFault(current => ({ ...current, delay_ms: value }))} min={0} max={60000} /></Field>
        <Field label="强制异常"><select className="liquid-glass-input liquid-glass-select" value={fault.exception_code ?? ""} onChange={event => setFault(current => ({ ...current, exception_code: event.target.value ? Number(event.target.value) : null }))}><option value="">正常处理</option>{Object.entries(STANDARD_EXCEPTIONS).map(([code, label]) => <option key={code} value={code}>{code} · {label}</option>)}</select></Field>
      </div>
      <div className={styles.actions}><button className="liquid-glass-button" disabled={!connected} onClick={() => void apply()}>应用故障配置</button>{!connected && <span className={styles.hint}>连接 Server 会话后才能应用运行时故障配置。</span>}{saved && <span className={styles.success}>{saved}</span>}{error && <span className={styles.error}>{error}</span>}</div>
    </section>
  </div>;
}

function Field({ label, children }: { label: string; children: React.ReactNode }) { return <label className={styles.field}><span className={styles.label}>{label}</span>{children}</label>; }
function NumberInput({ value, set, min = 0, max = 65535 }: { value: number; set: (value: number) => void; min?: number; max?: number }) { return <input className="liquid-glass-input" type="number" min={min} max={max} value={value} onChange={event => set(Number(event.target.value))} />; }
