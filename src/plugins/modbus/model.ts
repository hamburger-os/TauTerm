export type ModbusMode = "rtu" | "ascii" | "tcp";
export type ModbusRole = "client" | "server";

export interface ModbusSerialParams {
  baud_rate: number;
  data_bits: number;
  parity: "none" | "even" | "odd";
  stop_bits: string;
  flow_control: string;
  read_timeout_ms: number;
}

export interface ModbusTcpParams {
  connect_timeout_ms: number;
  read_timeout_ms: number;
  nodelay: boolean;
}

export interface ModbusSessionParams extends Record<string, unknown> {
  mode: ModbusMode;
  role: ModbusRole;
  serial_port: string;
  serial: ModbusSerialParams;
  host: string;
  port: number;
  tcp: ModbusTcpParams;
  /** Client 默认目标 Unit；实际事务和 Watch 行可以覆盖。Server 时为本机 Unit。 */
  unit_id: number;
  response_timeout_ms: number;
  read_retries: number;
  retry_writes: boolean;
  server_max_clients: number;
  server_fault: {
    no_response: boolean;
    delay_ms: number;
    exception_code: number | null;
  };
}

export function defaultModbusSessionParams(): ModbusSessionParams {
  return {
    mode: "rtu",
    role: "client",
    serial_port: "",
    serial: {
      baud_rate: 115200,
      data_bits: 8,
      parity: "none",
      stop_bits: "1",
      flow_control: "none",
      read_timeout_ms: 20,
    },
    host: "127.0.0.1",
    port: 502,
    tcp: {
      connect_timeout_ms: 5000,
      read_timeout_ms: 20,
      nodelay: true,
    },
    unit_id: 1,
    response_timeout_ms: 1000,
    read_retries: 1,
    retry_writes: false,
    server_max_clients: 16,
    server_fault: { no_response: false, delay_ms: 0, exception_code: null },
  };
}

const numberParam = (value: unknown, fallback: number): number =>
  typeof value === "number" && Number.isFinite(value) ? value : fallback;
const stringParam = (value: unknown, fallback = ""): string =>
  typeof value === "string" ? value : fallback;

export function normalizeModbusSessionParams(params: Record<string, unknown>): ModbusSessionParams {
  const defaults = defaultModbusSessionParams();
  const serial = typeof params.serial === "object" && params.serial
    ? params.serial as Record<string, unknown>
    : {};
  const tcp = typeof params.tcp === "object" && params.tcp
    ? params.tcp as Record<string, unknown>
    : {};
  const serverFault = typeof params.server_fault === "object" && params.server_fault
    ? params.server_fault as Record<string, unknown>
    : {};
  const mode: ModbusMode = params.mode === "rtu" || params.mode === "ascii" || params.mode === "tcp"
    ? params.mode
    : defaults.mode;
  const role: ModbusRole = params.role === "server" ? "server" : "client";
  return {
    ...defaults,
    ...params,
    mode,
    role,
    serial_port: stringParam(params.serial_port, defaults.serial_port),
    host: stringParam(params.host, role === "server" ? "0.0.0.0" : defaults.host),
    port: numberParam(params.port, defaults.port),
    unit_id: numberParam(params.unit_id, defaults.unit_id),
    response_timeout_ms: numberParam(params.response_timeout_ms, defaults.response_timeout_ms),
    read_retries: numberParam(params.read_retries, defaults.read_retries),
    retry_writes: params.retry_writes === true,
    server_max_clients: numberParam(params.server_max_clients, defaults.server_max_clients),
    serial: {
      ...defaults.serial,
      ...serial,
      baud_rate: numberParam(serial.baud_rate, defaults.serial.baud_rate),
      data_bits: numberParam(serial.data_bits, defaults.serial.data_bits),
      parity: serial.parity === "even" || serial.parity === "odd" ? serial.parity : "none",
      stop_bits: stringParam(serial.stop_bits, defaults.serial.stop_bits),
      flow_control: stringParam(serial.flow_control, defaults.serial.flow_control),
      read_timeout_ms: numberParam(serial.read_timeout_ms, defaults.serial.read_timeout_ms),
    },
    tcp: {
      ...defaults.tcp,
      ...tcp,
      connect_timeout_ms: numberParam(tcp.connect_timeout_ms, defaults.tcp.connect_timeout_ms),
      read_timeout_ms: numberParam(tcp.read_timeout_ms, defaults.tcp.read_timeout_ms),
      nodelay: tcp.nodelay !== false,
    },
    server_fault: {
      ...defaults.server_fault,
      ...serverFault,
      no_response: serverFault.no_response === true,
      delay_ms: numberParam(serverFault.delay_ms, defaults.server_fault.delay_ms),
      exception_code: typeof serverFault.exception_code === "number" ? serverFault.exception_code : null,
    },
  };
}

export type BitReadArea = "coils" | "discrete_inputs";
export type RegisterReadArea = "holding_registers" | "input_registers";

export type ModbusRequest =
  | { kind: "read_bits"; area: BitReadArea; address: number; quantity: number }
  | { kind: "read_registers"; area: RegisterReadArea; address: number; quantity: number }
  | { kind: "write_single_coil"; address: number; value: boolean }
  | { kind: "write_single_register"; address: number; value: number }
  | { kind: "read_exception_status" }
  | { kind: "diagnostics"; sub_function: number; data: number[] }
  | { kind: "get_comm_event_counter" }
  | { kind: "get_comm_event_log" }
  | { kind: "write_multiple_coils"; address: number; values: boolean[] }
  | { kind: "write_multiple_registers"; address: number; values: number[] }
  | { kind: "report_server_id" }
  | { kind: "read_file_record"; records: { file_number: number; record_number: number; record_length: number }[] }
  | { kind: "write_file_record"; records: { file_number: number; record_number: number; values: number[] }[] }
  | { kind: "mask_write_register"; address: number; and_mask: number; or_mask: number }
  | { kind: "read_write_multiple_registers"; read_address: number; read_quantity: number; write_address: number; values: number[] }
  | { kind: "read_fifo_queue"; address: number }
  | { kind: "mei"; mei_type: number; data: number[] }
  | { kind: "raw"; function: number; data: number[] };

export type ModbusOperation =
  | { kind: "request"; unit_id: number; request: ModbusRequest }
  | {
      kind: "raw_adu";
      data: number[];
      wait_response: boolean;
      quiet_period_ms: number;
    };

export function requestOperation(unitId: number, request: ModbusRequest): ModbusOperation {
  return { kind: "request", unit_id: unitId, request };
}

export type TransactionStatus =
  | "success"
  | "broadcast"
  | "modbus_exception"
  | "protocol_error"
  | "malformed_response"
  | "timeout"
  | "transport_error"
  | "cancelled"
  | "fault_injected";

export type SemanticResponse =
  | { kind: "bits"; values: boolean[] }
  | { kind: "registers"; values: number[] }
  | { kind: "acknowledged" }
  | { kind: "diagnostics"; sub_function: number; data: number[] }
  | { kind: "raw"; data: number[] };

export interface TransactionResult {
  timestamp_ms: number;
  status: TransactionStatus;
  function: number;
  transaction_id: number | null;
  unit_id: number;
  latency_ms: number;
  exception_code: number | null;
  raw_tx: number[];
  raw_rx: number[];
  response_pdu: number[];
  semantic_response: SemanticResponse | null;
  message: string | null;
  write_outcome_unknown: boolean;
  attempt: number;
}

export interface TransactionRecord {
  sequence: number;
  result: TransactionResult;
}

export interface TransactionHistoryBatch {
  records: TransactionRecord[];
  latest_sequence: number;
}

export interface ServerFaultConfig {
  no_response: boolean;
  delay_ms: number;
  exception_code: number | null;
}

export interface ModbusStatus {
  role: ModbusRole;
  mode: ModbusMode;
  running: boolean;
  default_unit_id: number;
  watch_rows: WatchRow[];
  watch_running: boolean;
  server_fault: ServerFaultConfig | null;
  transactions: TransactionHistoryBatch;
  watch_enabled: number;
  watch_total: number;
  last_status: TransactionStatus | null;
  last_unit_id: number | null;
  last_latency_ms: number | null;
}

export interface WatchRow {
  id: string;
  enabled: boolean;
  name: string;
  unit_id: number;
  request: ModbusRequest;
  period_ms: number;
  format?: ValueFormat;
}

export interface WatchValue {
  row_id: string;
  status: TransactionStatus;
  value: unknown;
  raw: number[];
  latency_ms: number;
  message: string | null;
  updated_at_ms: number;
}

export interface ValueFormat {
  value_type: "bool" | "uint16" | "int16" | "uint32" | "int32" | "float32" | "uint64" | "int64" | "float64" | "hex" | "binary" | "ascii" | "utf8";
  byte_order: "big" | "little";
  word_order: "normal" | "reverse";
  scale: number;
  offset: number;
  unit: string;
  bit?: number | null;
}

export interface ServerSnapshot {
  coils: [number, boolean][];
  discrete_inputs: [number, boolean][];
  holding_registers: [number, number][];
  input_registers: [number, number][];
}

// Presentation labels only. Protocol legality and quantity limits live in the Rust protocol core.
export const FUNCTION_LABELS: Record<number, string> = {
  0x01: "01 · Read Coils",
  0x02: "02 · Read Discrete Inputs",
  0x03: "03 · Read Holding Registers",
  0x04: "04 · Read Input Registers",
  0x05: "05 · Write Single Coil",
  0x06: "06 · Write Single Register",
  0x07: "07 · Read Exception Status",
  0x08: "08 · Diagnostics",
  0x0b: "0B · Get Comm Event Counter",
  0x0c: "0C · Get Comm Event Log",
  0x0f: "0F · Write Multiple Coils",
  0x10: "10 · Write Multiple Registers",
  0x11: "11 · Report Server ID",
  0x14: "14 · Read File Record",
  0x15: "15 · Write File Record",
  0x16: "16 · Mask Write Register",
  0x17: "17 · Read/Write Multiple Registers",
  0x18: "18 · Read FIFO Queue",
  0x2b: "2B · Encapsulated Interface / MEI",
};

export const STANDARD_EXCEPTIONS: Record<number, string> = {
  1: "Illegal Function",
  2: "Illegal Data Address",
  3: "Illegal Data Value",
  4: "Server Device Failure",
  5: "Acknowledge",
  6: "Server Device Busy",
  8: "Memory Parity Error",
  10: "Gateway Path Unavailable",
  11: "Gateway Target Failed to Respond",
};

export function hex(bytes: number[]): string {
  return bytes.map(byte => byte.toString(16).padStart(2, "0").toUpperCase()).join(" ");
}

export function parseHex(text: string): number[] {
  const normalized = text.replace(/0x/gi, " ").replace(/[,;:_-]/g, " ").trim();
  if (!normalized) return [];
  const tokens = normalized.split(/\s+/).flatMap(token => {
    if (!/^[0-9a-fA-F]+$/.test(token) || token.length % 2 !== 0) {
      throw new Error("十六进制输入必须由完整的两位字节组成");
    }
    return token.match(/.{2}/g) ?? [];
  });
  const bytes = tokens.map(token => Number.parseInt(token, 16));
  if (bytes.some(byte => !Number.isInteger(byte) || byte < 0 || byte > 255)) {
    throw new Error("无效十六进制字节");
  }
  return bytes;
}

export function parseU16List(text: string): number[] {
  const values = text.split(/[\s,;]+/).filter(Boolean).map(token => token.toLowerCase().startsWith("0x") ? Number.parseInt(token.slice(2), 16) : Number(token));
  if (values.some(value => !Number.isInteger(value) || value < 0 || value > 0xffff)) throw new Error("寄存器值必须是 0..65535");
  return values;
}

export function parseCoils(text: string): boolean[] {
  const tokens = text.split(/[\s,;]+/).filter(Boolean);
  if (tokens.length === 0) throw new Error("至少需要一个线圈值");
  return tokens.map(token => {
    const normalized = token.toLowerCase();
    if (normalized === "1" || normalized === "true") return true;
    if (normalized === "0" || normalized === "false") return false;
    throw new Error(`无效线圈值：${token}；仅支持 0/1/true/false`);
  });
}

export function traditionalAddress(functionCode: number, protocolAddress: number): string {
  const base = functionCode === 1 || functionCode === 5 || functionCode === 15 ? 1
    : functionCode === 2 ? 10001
    : functionCode === 4 ? 30001
    : 40001;
  return String(base + protocolAddress).padStart(5, "0");
}

export function unitIdMax(mode: ModbusMode): number {
  return mode === "tcp" ? 255 : 247;
}
