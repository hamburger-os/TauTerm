/**
 * 共享格式化工具函数
 *
 * 提取重复的格式化逻辑到一处，避免在多个组件中维护相同代码。
 */

const BINARY_UNITS = ["B", "KiB", "MiB", "GiB", "TiB"] as const;

function formatBinary(value: number, suffix: string): string {
  if (!Number.isFinite(value) || value <= 0) return `0 B${suffix}`;
  const unitIndex = Math.min(
    Math.floor(Math.log(value) / Math.log(1024)),
    BINARY_UNITS.length - 1,
  );
  const scaled = value / Math.pow(1024, unitIndex);
  const text = unitIndex === 0 ? String(Math.round(scaled)) : scaled.toFixed(1);
  return `${text} ${BINARY_UNITS[unitIndex]}${suffix}`;
}

/** 格式化字节数（自动选择 B/KiB/MiB/GiB/TiB） */
export function formatBytes(bytes: number): string {
  return formatBinary(bytes, "");
}

/** 格式化速率（字节/秒，自适应单位 B/s → KiB/s → MiB/s） */
export function formatRate(bytesPerSecond: number): string {
  return formatBinary(bytesPerSecond, "/s");
}

/** 格式化秒数为 HH:MM:SS */
export function formatUptime(totalSeconds: number): string {
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** 格式化串口参数为可读字符串: "115200·8N1·None" */
export function formatPortParams(params: Record<string, unknown> | undefined): string {
  if (!params) return "";
  const baud = params.baud_rate ?? "";
  const data = params.data_bits ?? "8";
  const parityMap: Record<string, string> = { none: "N", even: "E", odd: "O" };
  const parity = parityMap[String(params.parity ?? "none")] ?? "N";
  const stop = params.stop_bits ?? "1";
  const flowMap: Record<string, string> = { none: "None", rts_cts: "RTS/CTS", xon_xoff: "XON/XOFF" };
  const flow = flowMap[String(params.flow_control ?? "none")] ?? "None";
  return `${baud}·${data}${parity}${stop}·${flow}`;
}

/** Unix 纪元（1970-01-01 00:00:00 UTC），用于过滤无效时间戳 */
const UNIX_EPOCH = 0;

/** 格式化 Unix 时间戳为本地化时间字符串 */
export function formatTime(ts: number | null): string {
  if (ts === null || ts === undefined) return "-";
  if (!isFinite(ts)) return "-";
  if (ts <= UNIX_EPOCH) return "-";
  return new Date(ts * 1000).toLocaleString();
}

/** 格式化日期为紧凑文件名时间戳（YYYYMMDD_HHmm），用于默认导出文件名 */
export function formatDateCompact(date: Date): string {
  const Y = date.getFullYear();
  const M = String(date.getMonth() + 1).padStart(2, "0");
  const D = String(date.getDate()).padStart(2, "0");
  const h = String(date.getHours()).padStart(2, "0");
  const m = String(date.getMinutes()).padStart(2, "0");
  return `${Y}${M}${D}_${h}${m}`;
}
