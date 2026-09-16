export type RttBackendKind = "probe_rs" | "jlink_existing";
export type RttWireProtocol = "swd" | "jtag";
export type RttLocatorMode = "auto" | "exact" | "ranges";
export type RttViewMode = "terminal" | "text" | "hex";

export interface RttProbeInfo {
  selector: string;
  display_name: string;
  identifier: string;
  serial_number?: string | null;
}

export interface RttChannelDirectionInfo {
  buffer_size?: number | null;
}

export interface RttChannelInfo {
  index: number;
  name?: string | null;
  up?: RttChannelDirectionInfo | null;
  down?: RttChannelDirectionInfo | null;
  metadata_complete: boolean;
}

export interface RttBackendCapabilities {
  enumerate_channels: boolean;
  channel_metadata: boolean;
  locator: boolean;
  direct_target_control: boolean;
}

export interface RttBackendDescriptor {
  kind: string;
  display_name: string;
  target?: string | null;
  probe?: string | null;
  control_block_address?: number | null;
  capabilities: RttBackendCapabilities;
}

export interface RttErrorSnapshot {
  code: string;
  message: string;
}

export interface RttSnapshot {
  phase: "idle" | "opening_backend" | "running" | "faulted" | "stopping";
  backend?: RttBackendDescriptor | null;
  channels: RttChannelInfo[];
  rx_bytes: number;
  tx_bytes: number;
  dropped_history_bytes: number;
  dropped_history_chunks: number;
  last_error?: RttErrorSnapshot | null;
}

export interface RttChunk {
  sequence: number;
  timestamp_ms: number;
  channel_index: number;
  data_b64: string;
}

export interface RttHistoryResponse {
  chunks: RttChunk[];
  oldest_sequence?: number | null;
  newest_sequence?: number | null;
  dropped_chunks: number;
  dropped_bytes: number;
}

export type RttEvent =
  | ({ kind: "chunk"; session_id: string } & RttChunk)
  | { kind: "snapshot"; session_id: string; snapshot: RttSnapshot };

export function defaultRttParams(): Record<string, unknown> {
  return {
    backend: "probe_rs",
    probe_selector: "",
    target: "",
    wire_protocol: "swd",
    speed_khz: 0,
    core_index: 0,
    locator_mode: "auto",
    control_block_address: "",
    scan_ranges: "",
    attach_timeout_ms: 5000,
    poll_interval_ms: 5,
    write_timeout_ms: 500,
    jlink_port: 19021,
    jlink_channels: "0",
  };
}

export function normalizeRttParams(params: Record<string, unknown>): Record<string, unknown> {
  return { ...defaultRttParams(), ...params };
}

export function rttSubtitle(params: Record<string, unknown>): string {
  const normalized = normalizeRttParams(params);
  const backend = normalized.backend === "jlink_existing" ? "jlink_existing" : "probe_rs";
  if (backend === "jlink_existing") {
    return `J-Link Existing · :${Number(normalized.jlink_port) || 19021}`;
  }
  const selector = String(normalized.probe_selector || "Auto");
  const target = String(normalized.target || "Unconfigured");
  const wire = String(normalized.wire_protocol || "swd").toUpperCase();
  return `${selector || "Auto"} · ${target} · ${wire}`;
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const step = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += step) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + step));
  }
  return btoa(binary);
}

export function base64ToBytes(value: string): Uint8Array {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

export function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MiB`;
}
