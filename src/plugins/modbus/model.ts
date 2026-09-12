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

export type ModbusRequest =
  | { kind: "read_bits"; function: 1 | 2; address: number; quantity: number }
  | { kind: "read_registers"; function: 3 | 4; address: number; quantity: number }
  | { kind: "write_single"; function: 5 | 6; address: number; value: number }
  | { kind: "read_exception_status" }
  | { kind: "diagnostics"; sub_function: number; data: number[] }
  | { kind: "get_comm_event_counter" }
  | { kind: "get_comm_event_log" }
  | { kind: "write_multiple_coils"; address: number; quantity: number; values: number[] }
  | { kind: "write_multiple_registers"; address: number; values: number[] }
  | { kind: "report_server_id" }
  | { kind: "read_file_record"; records: { file_number: number; record_number: number; record_length: number }[] }
  | { kind: "write_file_record"; records: { file_number: number; record_number: number; values: number[] }[] }
  | { kind: "mask_write_register"; address: number; and_mask: number; or_mask: number }
  | { kind: "read_write_multiple_registers"; read_address: number; read_quantity: number; write_address: number; values: number[] }
  | { kind: "read_fifo_queue"; address: number }
  | { kind: "mei"; mei_type: number; data: number[] }
  | { kind: "raw"; function: number; data: number[] };

export type ModbusOperation = ModbusRequest | {
  kind: "raw_adu";
  data: number[];
  wait_response: boolean;
  quiet_period_ms: number;
};

export interface TransactionResult {
  timestamp_ms: number;
  status: "success" | "broadcast" | "modbus_exception" | "protocol_error" | "malformed_response" | "timeout" | "transport_error" | "cancelled" | "fault_injected";
  function: number;
  transaction_id: number | null;
  unit_id: number;
  latency_ms: number;
  exception_code: number | null;
  raw_tx: number[];
  raw_rx: number[];
  response_pdu: number[];
  message: string | null;
  write_outcome_unknown: boolean;
  attempt: number;
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
  unit_id: number;
  transactions: TransactionResult[];
  server_fault: ServerFaultConfig | null;
  watch_rows: WatchRow[];
}

export interface WatchRow {
  id: string;
  enabled: boolean;
  name: string;
  request: ModbusRequest;
  period_ms: number;
  format?: ValueFormat;
}

export interface WatchValue {
  row_id: string;
  status: string;
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

export function packCoils(text: string): { quantity: number; values: number[] } {
  const tokens = text.split(/[\s,;]+/).filter(Boolean);
  if (tokens.length === 0 || tokens.length > 1968) throw new Error("线圈数量必须是 1..1968");
  const bits = tokens.map(token => {
    const normalized = token.toLowerCase();
    if (normalized === "1" || normalized === "true") return true;
    if (normalized === "0" || normalized === "false") return false;
    throw new Error(`无效线圈值：${token}；仅支持 0/1/true/false`);
  });
  const values = new Array(Math.ceil(bits.length / 8)).fill(0) as number[];
  bits.forEach((bit, index) => { if (bit) values[Math.floor(index / 8)] |= 1 << (index % 8); });
  return { quantity: bits.length, values };
}

export function traditionalAddress(functionCode: number, protocolAddress: number): string {
  const base = functionCode === 1 || functionCode === 5 || functionCode === 15 ? 1
    : functionCode === 2 ? 10001
    : functionCode === 4 ? 30001
    : 40001;
  return String(base + protocolAddress).padStart(5, "0");
}
