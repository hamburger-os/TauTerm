import { normalizeModbusSessionParams } from "./model";

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

export function modbusEndpointLabel(params: Record<string, unknown>): string {
  const normalized = normalizeModbusSessionParams(params);
  if (normalized.mode === "tcp") {
    const fallback = normalized.role === "server" ? "0.0.0.0" : "127.0.0.1";
    return `${normalized.host.trim() || fallback}:${normalized.port}`;
  }
  return normalized.serial_port.trim() || "未选择串口";
}
