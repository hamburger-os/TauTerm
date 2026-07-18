/**
 * 共享格式化工具函数
 *
 * 提取重复的格式化逻辑到一处，避免在多个组件中维护相同代码。
 */

/** 格式化字节数（自动选择 B/KB/MB/GB） */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
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

/** 格式化 Unix 时间戳为本地化时间字符串 */
export function formatTime(ts: number | null): string {
  if (!ts) return "-";
  return new Date(ts * 1000).toLocaleString();
}
