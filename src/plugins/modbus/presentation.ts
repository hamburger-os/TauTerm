import { normalizeModbusSessionParams } from "./model";

const GENERATED_SESSION_TITLES = new Set([
  "Modbus @ RTU Master",
  "Modbus @ RTU Slave",
  "Modbus @ ASCII Master",
  "Modbus @ ASCII Slave",
  "Modbus @ TCP Client",
  "Modbus @ TCP Server",
]);

export function modbusTypeLabel(params: Record<string, unknown>): string {
  const normalized = normalizeModbusSessionParams(params);
  const mode = normalized.mode.toUpperCase();
  const role = normalized.mode === "tcp"
    ? (normalized.role === "server" ? "Server" : "Client")
    : (normalized.role === "server" ? "Slave" : "Master");
  return `${mode} ${role}`;
}

export function modbusSessionTitle(params: Record<string, unknown>): string {
  return `Modbus @ ${modbusTypeLabel(params)}`;
}

export function isGeneratedModbusSessionTitle(name: string): boolean {
  return GENERATED_SESSION_TITLES.has(name);
}

export function modbusEndpointLabel(params: Record<string, unknown>): string {
  const normalized = normalizeModbusSessionParams(params);
  if (normalized.mode === "tcp") {
    const fallback = normalized.role === "server" ? "0.0.0.0" : "127.0.0.1";
    return `${normalized.host.trim() || fallback}:${normalized.port}`;
  }
  return normalized.serial_port.trim() || "未选择串口";
}

export function modbusConnectionSubtitle(params: Record<string, unknown>): string {
  const normalized = normalizeModbusSessionParams(params);
  const endpoint = modbusEndpointLabel(normalized);
  if (normalized.mode === "tcp") return `${endpoint} · Unit ${normalized.unit_id}`;

  const parity = normalized.serial.parity === "even" ? "E" : normalized.serial.parity === "odd" ? "O" : "N";
  const framing = `${normalized.serial.data_bits}${parity}${normalized.serial.stop_bits}`;
  return `${endpoint} · ${normalized.serial.baud_rate} ${framing} · Unit ${normalized.unit_id}`;
}
