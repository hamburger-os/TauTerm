export type RttBackendKind = "probe_rs" | "jlink_existing";
export type RttWireProtocol = "swd" | "jtag";
export type RttLocatorMode = "auto" | "exact" | "ranges";
export type RttViewMode = "terminal" | "log" | "hex";

export interface RttProbeInfo {
  selector: string;
  display_name: string;
  identifier: string;
  serial_number?: string | null;
}

export interface RttChannelDirectionInfo {
  buffer_size?: number | null;
  usable: boolean;
  issue?: string | null;
}

export interface RttChannelInfo {
  index: number;
  name?: string | null;
  up?: RttChannelDirectionInfo | null;
  down?: RttChannelDirectionInfo | null;
  metadata_complete: boolean;
}

export function isUsableRttDirection(
  direction: RttChannelDirectionInfo | null | undefined,
): direction is RttChannelDirectionInfo {
  return direction?.usable === true;
}

export function rttChannelIssues(channel: RttChannelInfo): string[] {
  const issues: string[] = [];
  if (channel.up?.issue) issues.push(`Up: ${channel.up.issue}`);
  if (channel.down?.issue) issues.push(`Down: ${channel.down.issue}`);
  return issues;
}

export function hasUsableRttDownChannel(snapshot: RttSnapshot | null | undefined): boolean {
  return snapshot?.phase === "running"
    && snapshot.channels.some(channel => isUsableRttDirection(channel.down));
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
  control_block_address?: string | null;
  capabilities: RttBackendCapabilities;
}

export interface RttErrorSnapshot {
  code: string;
  message: string;
}

export interface RttSnapshot {
  generation: number;
  phase: "idle" | "opening_backend" | "running" | "faulted" | "stopping";
  backend?: RttBackendDescriptor | null;
  channels: RttChannelInfo[];
  automation_source_channel: number | null;
  send_channel: number | null;
  rx_bytes: number;
  tx_bytes: number;
  dropped_history_bytes: number;
  dropped_history_chunks: number;
  dropped_automation_bytes: number;
  dropped_automation_chunks: number;
  dropped_presentation_bytes: number;
  dropped_presentation_chunks: number;
  runtime_pressure_events: number;
  last_error?: RttErrorSnapshot | null;
}

export interface RttChunk {
  generation: number;
  sequence: number;
  timestamp_ms: number;
  channel_index: number;
  channel_offset: number;
  data_b64: string;
}

export interface RttHistoryResponse {
  generation: number;
  chunks: RttChunk[];
  oldest_sequence?: number | null;
  newest_sequence?: number | null;
  dropped_chunks: number;
  dropped_bytes: number;
}

export type RttEvent =
  | { kind: "batch"; session_id: string; generation: number; chunks: RttChunk[] }
  | { kind: "snapshot"; session_id: string; generation: number; snapshot: RttSnapshot };

export function defaultRttParams(): Record<string, unknown> {
  return {
    backend: "probe_rs",
    probe_selector: "",
    probe_name: "",
    target: "",
    wire_protocol: "swd",
    speed_khz: null,
    core_index: 0,
    firmware_path: "",
    locator_mode: "auto",
    control_block_address: "",
    scan_ranges: "",
    jlink_port: 19021,
    jlink_channels: "0",
  };
}

export function normalizeRttParams(params: Record<string, unknown>): Record<string, unknown> {
  const normalized = { ...defaultRttParams(), ...params };
  const speed = normalized.speed_khz;
  return {
    ...normalized,
    probe_name: typeof normalized.probe_name === "string" ? normalized.probe_name.trim() : "",
    firmware_path: typeof normalized.firmware_path === "string" ? normalized.firmware_path.trim() : "",
    speed_khz: typeof speed === "number" && Number.isFinite(speed) && speed > 0 ? speed : null,
  };
}

export function compactRttProbeName(probe: RttProbeInfo): string {
  const identifier = probe.identifier.trim();
  if (identifier) return identifier;

  const displayName = probe.display_name.trim();
  const separator = displayName.indexOf(" -- ");
  if (separator > 0) return displayName.slice(0, separator).trim();
  return displayName || "Debug Probe";
}

export function rttDefaultSessionName(params: Record<string, unknown>): string {
  const normalized = normalizeRttParams(params);
  return normalized.backend === "jlink_existing"
    ? "RTT @ J-Link Existing"
    : "RTT @ Debug Probe";
}

export function rttSubtitle(params: Record<string, unknown>): string {
  const normalized = normalizeRttParams(params);
  if (normalized.backend === "jlink_existing") {
    return `127.0.0.1:${Number(normalized.jlink_port) || 19021}`;
  }

  const probeName = String(normalized.probe_name || "").trim();
  if (probeName) return probeName;
  return String(normalized.probe_selector || "").trim() ? "Debug Probe" : "Auto";
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
