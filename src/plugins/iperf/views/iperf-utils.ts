/**
 * iperf 工具函数：带宽解析/格式化、命令行构建
 */

/** "100M"/"1G" → bps（1000 进制）；无法解析返回 null */
export function parseBits(s: string): number | null {
  const m = s.trim().match(/^(\d+(?:\.\d+)?)\s*([kmgtKMGT]?)$/);
  if (!m) return null;
  const v = parseFloat(m[1]);
  const suf = m[2].toLowerCase();
  const mult: Record<string, number> = { "": 1, k: 1e3, m: 1e6, g: 1e9, t: 1e12 };
  if (!(suf in mult)) return null;
  return Math.round(v * mult[suf]);
}

/** bps → 紧凑字符串（1000 进制后缀） */
export function formatBits(bps: number): string {
  if (bps >= 1e12) return (bps / 1e12).toFixed(1).replace(/\.0$/, "") + "T";
  if (bps >= 1e9) return (bps / 1e9).toFixed(1).replace(/\.0$/, "") + "G";
  if (bps >= 1e6) return (bps / 1e6).toFixed(1).replace(/\.0$/, "") + "M";
  if (bps >= 1e3) return (bps / 1e3).toFixed(1).replace(/\.0$/, "") + "K";
  return String(Math.round(bps));
}

/** bps → 人类可读速率（"98.12 Mbps"；单位走 i18n） */
export function formatMbps(
  bps: number,
  units: { gbps: string; mbps: string; kbps: string; bps: string }
): string {
  if (!isFinite(bps) || bps < 0) return "—";
  if (bps >= 1e9) return (bps / 1e9).toFixed(2) + " " + units.gbps;
  if (bps >= 1e6) return (bps / 1e6).toFixed(2) + " " + units.mbps;
  if (bps >= 1e3) return (bps / 1e3).toFixed(2) + " " + units.kbps;
  return bps.toFixed(0) + " " + units.bps;
}

/** 字节 → 人类可读 */
export function formatBytes(n: number): string {
  if (n >= 1 << 30) return (n / (1 << 30)).toFixed(2) + " GiB";
  if (n >= 1 << 20) return (n / (1 << 20)).toFixed(2) + " MiB";
  if (n >= 1 << 10) return (n / (1 << 10)).toFixed(1) + " KiB";
  return String(n) + " B";
}

/**
 * "-w" 窗口大小解析（**1024 进制**，对齐 iperf2 `byte_atoi`：
 * K/M/G 后缀为 1024 幂，无后缀 = 字节）。与 `parseBits` 的 1000 进制区分。
 */
export function parseWindowSize(s: string): number | null {
  const m = s.trim().match(/^(\d+(?:\.\d+)?)\s*([kmgKMGTt]?)$/);
  if (!m) return null;
  const suf = m[2].toLowerCase();
  const mult: Record<string, number> = {
    "": 1,
    k: 1024,
    m: 1024 ** 2,
    g: 1024 ** 3,
    t: 1024 ** 4,
  };
  if (!(suf in mult)) return null;
  return Math.round(parseFloat(m[1]) * mult[suf]);
}

/** 窗口大小字节 → 紧凑字符串（1024 进制；整除时用后缀，否则原字节数） */
export function formatWindowSize(n: number): string {
  if (n % 1024 ** 4 === 0) return `${n / 1024 ** 4}T`;
  if (n % 1024 ** 3 === 0) return `${n / 1024 ** 3}G`;
  if (n % 1024 ** 2 === 0) return `${n / 1024 ** 2}M`;
  if (n % 1024 === 0) return `${n / 1024}K`;
  return String(n);
}

// ── 标准 iperf2 输出格式（对齐 iperf 2.x 终端打印） ────────

/** 标准表头（TCP）：`[ ID] Interval       Transfer     Bandwidth` */
export const IPERF_HEADER_TCP = "[ ID] Interval       Transfer     Bandwidth";
/** 标准表头（UDP）：追加抖动/丢包列 */
export const IPERF_HEADER_UDP =
  "[ ID] Interval       Transfer     Bandwidth       Jitter    Lost/Total Datagrams";

/** iperf2 数值精度：值 < 10 → 2 位小数，≥ 10 → 1 位小数（对齐标准输出） */
function fmtIperfValue(v: number): string {
  return v < 10 ? v.toFixed(2) : v.toFixed(1);
}

/** 字节按 1024 进制（iperf2 惯例）：`3.01 GBytes` / `850.5 MBytes` */
function fmtIperfBytes(n: number): string {
  if (n >= 1024 ** 3) return `${fmtIperfValue(n / 1024 ** 3)} GBytes`;
  if (n >= 1024 ** 2) return `${fmtIperfValue(n / 1024 ** 2)} MBytes`;
  if (n >= 1024) return `${fmtIperfValue(n / 1024)} KBytes`;
  return `${n} Bytes`;
}

/** 比特按 1000 进制（iperf2 惯例）：`25.8 Gbits/sec` / `98.1 Mbits/sec` */
function fmtIperfBits(bps: number): string {
  if (bps >= 1e9) return `${fmtIperfValue(bps / 1e9)} Gbits/sec`;
  if (bps >= 1e6) return `${fmtIperfValue(bps / 1e6)} Mbits/sec`;
  if (bps >= 1e3) return `${fmtIperfValue(bps / 1e3)} Kbits/sec`;
  return `${bps} bits/sec`;
}

/**
 * 标准 iperf2 区间行：`[  1] 0.00-1.00 sec  3.01 GBytes  25.8 Gbits/sec`
 * UDP 行追加抖动/丢包列：`...  0.038 ms  0/822 (0%)`
 * `id` 为流标签（右对齐 3 位；-P>1 的聚合行传 "SUM"）
 */
export function formatIntervalLine(
  i: {
    startSecs: number;
    endSecs: number;
    transferredBytes: number;
    bandwidthBps: number;
    jitterMs?: number | null;
    lostPackets?: number | null;
    totalPackets?: number | null;
    lostPercent?: number | null;
  },
  id = "1"
): string {
  let line = `[${id.padStart(3)}] ${i.startSecs.toFixed(2)}-${i.endSecs.toFixed(2)} sec  ${fmtIperfBytes(i.transferredBytes)}  ${fmtIperfBits(i.bandwidthBps)}`;
  if (i.jitterMs != null) {
    const lost = i.lostPackets ?? 0;
    const total = i.totalPackets ?? 0;
    const pct = i.lostPercent ?? 0;
    line += `  ${i.jitterMs.toFixed(3)} ms  ${lost}/${total} (${pct.toFixed(0)}%)`;
  }
  return line;
}

export interface CommandParams {
  version: "iperf2" | "iperf3";
  role: "client" | "server";
  targetHost: string;
  listenPort: number;
  protocol: "tcp" | "udp";
  durationSecs: number;
  port: number;
  parallelStreams: number;
  reportIntervalSecs: number;
  bandwidthBps: number | null;
  bidirectional: boolean; // iperf2 -d
  tradeoff: boolean;      // iperf2 -r
  windowSize: number | null; // iperf2 -w
  reverse: boolean;       // iperf3 -R
  bidir: boolean;         // iperf3 --bidir
  omitSecs: number;       // iperf3 -O
}

/** 构建当前角色对应的命令行（提示性预览） */
export function buildIperfCommand(p: CommandParams): string {
  const bin = p.version === "iperf2" ? "iperf" : "iperf3";
  if (p.role === "server") {
    return `${bin} -s -p ${p.listenPort}`;
  }
  const parts = [`${bin} -c ${p.targetHost || "<host>"}`, `-p ${p.port}`];
  parts.push(`-t ${p.durationSecs}`, `-i ${p.reportIntervalSecs}`);
  if (p.protocol === "udp") parts.push("-u");
  // -b：iperf3 TCP/UDP 均生效；iperf2 仅 UDP 生效（其 TCP 引擎无配速，
  // 渲染 -b 会背书未实现的行为，测量结果可超上限几个数量级）
  if (
    p.bandwidthBps != null &&
    (p.protocol === "udp" || p.version === "iperf3")
  ) {
    parts.push(`-b ${formatBits(p.bandwidthBps)}`);
  }
  if (p.parallelStreams > 1) parts.push(`-P ${p.parallelStreams}`);
  if (p.version === "iperf2") {
    if (p.bidirectional) parts.push("-d");
    if (p.tradeoff) parts.push("-r");
    if (p.windowSize != null) parts.push(`-w ${formatWindowSize(p.windowSize)}`);
  } else {
    if (p.reverse) parts.push("-R");
    if (p.bidir) parts.push("--bidir");
    if (p.omitSecs > 0) parts.push(`-O ${p.omitSecs}`);
  }
  return parts.join(" ");
}
