export type ModbusMode = "rtu" | "ascii" | "tcp";
export type ModbusRole = "client" | "server";

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

export interface TransactionResult {
  status: "success" | "broadcast" | "modbus_exception" | "protocol_error" | "malformed_response" | "timeout" | "transport_error" | "cancelled";
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
  byte_order: "ABCD" | "BADC" | "CDAB" | "DCBA";
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

export function hex(bytes: number[]): string {
  return bytes.map(byte => byte.toString(16).padStart(2, "0").toUpperCase()).join(" ");
}

export function parseHex(text: string): number[] {
  const normalized = text.replace(/0x/gi, " ").replace(/[,;:_-]/g, " ").trim();
  if (!normalized) return [];
  const compact = normalized.includes(" ") ? normalized.split(/\s+/) : normalized.match(/.{1,2}/g) ?? [];
  const bytes = compact.map(token => Number.parseInt(token, 16));
  if (bytes.some(byte => !Number.isInteger(byte) || byte < 0 || byte > 255)) throw new Error("无效十六进制字节");
  return bytes;
}

export function traditionalAddress(functionCode: number, protocolAddress: number): string {
  const base = functionCode === 1 || functionCode === 5 || functionCode === 15 ? 1
    : functionCode === 2 ? 10001
    : functionCode === 4 ? 30001
    : 40001;
  return String(base + protocolAddress).padStart(5, "0");
}
