/**
 * iperf 共享类型与后端事件载荷（类型化：前端 ↔ 后端 snake_case JSON 契约）
 *
 * 所有 `iperf-*` 事件的 payload 在此声明；`summaryFromPayload` /
 * `paramsFromBackend` 为唯一的 snake_case → camelCase 转换点。
 */

// ═══════════════════════════════════════════════════════════════════
// 基础类型（前端 camelCase）
// ═══════════════════════════════════════════════════════════════════

export type IperfVersionStr = "iperf2" | "iperf3";
export type IperfRoleStr = "client" | "server";
export type IperfProtocolStr = "tcp" | "udp";
/** 测试方向（-d/-r 双向测试：fwd 客户端→服务端，rev 服务端→客户端） */
export type IperfDirection = "fwd" | "rev";

export interface IperfParams {
  protocol: IperfProtocolStr;
  durationSecs: number;
  port: number;
  parallelStreams: number;
  reportIntervalSecs: number;
  bandwidthBps: number | null; // -b
  // iperf2
  bidirectional: boolean; // -d dualtest（双连接双向同时）
  tradeoff: boolean; // -r tradeoff（同连接顺序反向）
  windowSize: number | null; // -w
  // iperf3
  reverse: boolean; // -R
  bidir: boolean; // --bidir
  omitSecs: number; // -O
}

export interface IperfInterval {
  startSecs: number;
  endSecs: number;
  transferredBytes: number;
  bandwidthBps: number;
  jitterMs: number | null;
  lostPackets: number | null;
  totalPackets: number | null;
  lostPercent: number | null;
}

export interface IperfSummary {
  version: IperfVersionStr;
  role: IperfRoleStr;
  protocol: IperfProtocolStr;
  durationSecs: number;
  totalBytes: number;
  avgBandwidthBps: number;
  intervals: IperfInterval[];
  jitterMs: number | null;
  lostPackets: number | null;
  totalPackets: number | null;
  lostPercent: number | null;
}

export interface IperfRecord {
  id: string;
  role: IperfRoleStr;
  version: IperfVersionStr;
  protocol: IperfProtocolStr;
  /** -d/-r 方向（普通测试为 fwd） */
  direction: IperfDirection;
  startTime: number; // epoch ms
  status: "running" | "completed" | "failed";
  error?: string;
  /** 非致命警告（如 UDP 未收到服务器统计回报） */
  warning?: string;
  summary: IperfSummary | null;
  intervals: IperfInterval[];
  logLines: string[];
  /** 并发配对键（UDP 服务端多路复用：started/done/interval 按 seq 归位） */
  seq?: number;
}

// ═══════════════════════════════════════════════════════════════════
// 后端事件载荷（snake_case）
// ═══════════════════════════════════════════════════════════════════

/** 后端 IperfSummary（done 事件 / get_status 的 last_summary） */
export interface IperfSummaryBackend {
  version: IperfVersionStr;
  role: IperfRoleStr;
  protocol: IperfProtocolStr;
  duration_secs: number;
  total_bytes: number;
  avg_bandwidth_bps: number;
  intervals: IperfIntervalBackend[];
  jitter_ms: number | null;
  lost_packets: number | null;
  total_packets: number | null;
  lost_percent: number | null;
}

export interface IperfIntervalBackend {
  start_secs: number;
  end_secs: number;
  transferred_bytes: number;
  bandwidth_bps: number;
  jitter_ms: number | null;
  lost_packets: number | null;
  total_packets: number | null;
  lost_percent: number | null;
}

/** 后端 IperfDynamicParams（iperf_get_status.dynamic_params） */
export interface IperfDynamicParamsBackend {
  version?: IperfVersionStr;
  protocol?: IperfProtocolStr;
  duration_secs?: number;
  port?: number;
  parallel_streams?: number;
  report_interval_secs?: number;
  bandwidth_bps?: number | null;
  bidirectional?: boolean;
  tradeoff?: boolean;
  window_size?: number | null;
  reverse?: boolean;
  bidir?: boolean;
  omit_secs?: number;
  listen_ip?: string;
  listen_port?: number;
}

export interface IperfServerStatusEvent {
  session_id: string;
  running: boolean;
  listen_addr?: string | null;
  error?: string | null;
}

export interface IperfTestStartedEvent {
  session_id: string;
  role: IperfRoleStr;
  direction?: IperfDirection;
  target?: string | null;
  protocol?: IperfProtocolStr | null;
  seq?: number;
  /** iperf3 服务端待命/接待中（先于任何客户端出现） */
  pending?: boolean;
  params?: Record<string, unknown>;
  /** 服务端测试时长提示（前端看门狗用；-d/-r 时总时长约为两倍） */
  duration_secs?: number;
  /** 服务端测试是否双向（-d/-r） */
  bidirectional?: boolean;
}

export interface IperfIntervalEvent {
  session_id: string;
  role: IperfRoleStr;
  direction?: IperfDirection;
  protocol?: IperfProtocolStr | null;
  seq?: number;
  start_secs: number;
  end_secs: number;
  transferred_bytes: number;
  bandwidth_bps: number;
  jitter_ms: number | null;
  lost_packets: number | null;
  total_packets: number | null;
  lost_percent: number | null;
}

export interface IperfDoneEvent {
  session_id: string;
  success: boolean;
  role: IperfRoleStr;
  direction?: IperfDirection;
  protocol?: IperfProtocolStr | null;
  seq?: number;
  error?: string | null;
  /** 非致命警告（如 UDP 未收到服务器统计回报） */
  warning?: string | null;
  summary: IperfSummaryBackend | null;
}

export interface SessionConnectedEvent {
  session_id: string;
  params: Record<string, unknown>;
}

export interface SessionDisconnectedEvent {
  session_id: string;
}

// ═══════════════════════════════════════════════════════════════════
// snake_case → camelCase 转换
// ═══════════════════════════════════════════════════════════════════

/** 后端区间 → 前端区间 */
export function intervalFromPayload(raw: IperfIntervalBackend): IperfInterval {
  return {
    startSecs: raw.start_secs,
    endSecs: raw.end_secs,
    transferredBytes: raw.transferred_bytes,
    bandwidthBps: raw.bandwidth_bps,
    jitterMs: raw.jitter_ms ?? null,
    lostPackets: raw.lost_packets ?? null,
    totalPackets: raw.total_packets ?? null,
    lostPercent: raw.lost_percent ?? null,
  };
}

/** done 事件 summary：后端 snake_case → 前端 camelCase */
export function summaryFromPayload(
  raw: IperfSummaryBackend | null
): IperfSummary | null {
  if (!raw) return null;
  return {
    version: raw.version,
    role: raw.role,
    protocol: raw.protocol,
    durationSecs: raw.duration_secs,
    totalBytes: raw.total_bytes,
    avgBandwidthBps: raw.avg_bandwidth_bps,
    intervals: raw.intervals.map(intervalFromPayload),
    jitterMs: raw.jitter_ms ?? null,
    lostPackets: raw.lost_packets ?? null,
    totalPackets: raw.total_packets ?? null,
    lostPercent: raw.lost_percent ?? null,
  };
}

/** 后端 IperfDynamicParams JSON → 前端参数（缺省回落默认值） */
export function paramsFromBackend(raw: IperfDynamicParamsBackend): IperfParams {
  return {
    protocol: raw.protocol === "udp" ? "udp" : "tcp",
    durationSecs: raw.duration_secs ?? 10,
    port: raw.port ?? 5001,
    parallelStreams: raw.parallel_streams ?? 1,
    reportIntervalSecs: raw.report_interval_secs ?? 1,
    bandwidthBps: raw.bandwidth_bps ?? null,
    bidirectional: !!raw.bidirectional,
    tradeoff: !!raw.tradeoff,
    windowSize: raw.window_size ?? null,
    reverse: !!raw.reverse,
    bidir: !!raw.bidir,
    omitSecs: raw.omit_secs ?? 0,
  };
}
