export type RttBackendKind = "probe_rs" | "jlink_existing";
export type RttWireProtocol = "swd" | "jtag";
export type RttLocatorMode = "auto" | "exact" | "ranges";
export type RttViewMode = "terminal" | "log" | "hex" | "trace" | "events" | "raw";

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

export function hasRttChannelIssues(channel: RttChannelInfo): boolean {
  return Boolean(channel.up?.issue || channel.down?.issue);
}

export function hasUsableRttDownChannel(snapshot: RttSnapshot | null | undefined): boolean {
  return snapshot?.phase === "running"
    && snapshot.channels.some(
      channel => isUsableRttDirection(channel.down)
        && !isRttDownChannelClaimed(snapshot, channel.index),
    );
}

export interface RttChannelClaim {
  channel_index: number;
  owner: string;
}

export interface RttObserverInfo {
  kind: string;
  channel_index: number;
  control_channel_index?: number | null;
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
  observers: RttObserverInfo[];
  channel_claims: RttChannelClaim[];
  rx_bytes: number;
  tx_bytes: number;
  evicted_history_bytes: number;
  evicted_history_chunks: number;
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

export type SystemViewPhase = "idle" | "recording" | "stopped";

export interface SystemViewTaskSnapshot {
  id: number;
  name?: string | null;
  priority?: number | null;
  runtime_cycles: number;
  switches: number;
}

export interface SystemViewEvent {
  sequence: number;
  event_id: number;
  kind: string;
  target_cycles: number;
  delta_cycles: number;
  context_id?: number | null;
  value?: number | null;
  text?: string | null;
}

export interface SystemViewSnapshot {
  generation: number;
  channel_index: number;
  phase: SystemViewPhase;
  control_available: boolean;
  event_count: number;
  task_count: number;
  target_overflow_packets: number;
  target_dropped_events: number;
  decoder_dropped_chunks: number;
  decoder_errors: number;
  presentation_dropped_events: number;
  cleared_through_sequence: number;
  sys_freq_hz?: number | null;
  cpu_freq_hz?: number | null;
  ram_base?: number | null;
  id_shift?: number | null;
  system_description: string[];
  window_start_cycles: number;
  last_target_cycles: number;
  tasks: SystemViewTaskSnapshot[];
}

export interface SystemViewHistoryResponse {
  generation: number;
  channel_index: number;
  events: SystemViewEvent[];
}

export type SystemViewRuntimeEvent =
  | {
      kind: "batch";
      session_id: string;
      generation: number;
      channel_index: number;
      events: SystemViewEvent[];
      snapshot: SystemViewSnapshot;
    }
  | {
      kind: "snapshot";
      session_id: string;
      generation: number;
      channel_index: number;
      snapshot: SystemViewSnapshot;
    };

export function isRttDownChannelClaimed(
  snapshot: RttSnapshot | null | undefined,
  channelIndex: number,
): boolean {
  return snapshot?.channel_claims?.some(claim => claim.channel_index === channelIndex) ?? false;
}

export function systemViewObserver(
  snapshot: RttSnapshot | null | undefined,
  channelIndex: number,
): RttObserverInfo | null {
  return snapshot?.observers.find(
    observer => observer.kind === "systemview" && observer.channel_index === channelIndex,
  ) ?? null;
}

export function resolveRttViewMode(
  snapshot: RttSnapshot | null | undefined,
  channelIndex: number | null,
  configuredMode: RttViewMode | undefined,
): RttViewMode {
  if (channelIndex == null) return "terminal";
  const observer = systemViewObserver(snapshot, channelIndex);
  if (observer) {
    return configuredMode === "trace" || configuredMode === "events" || configuredMode === "raw"
      ? configuredMode
      : "trace";
  }
  if (configuredMode === "terminal" || configuredMode === "log" || configuredMode === "hex") {
    return configuredMode;
  }
  return channelIndex === 0 ? "terminal" : "log";
}

export function isRttSemanticView(
  snapshot: RttSnapshot | null | undefined,
  channelIndex: number | null,
  configuredMode: RttViewMode | undefined,
): boolean {
  const mode = resolveRttViewMode(snapshot, channelIndex, configuredMode);
  return mode === "trace" || mode === "events" || mode === "raw";
}

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
