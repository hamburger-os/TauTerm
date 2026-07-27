/** 单条 journald 日志条目（从 `journalctl -o json` 解析） */
export interface JournalEntry {
  /** 单调时间戳（微秒） */
  __MONOTONIC_TIMESTAMP?: string;
  /** 墙上时钟时间戳（微秒） */
  __REALTIME_TIMESTAMP?: string;
  /** journald 游标（用于分页） */
  __CURSOR?: string;
  /** syslog 标识符（如 "sshd"） */
  SYSLOG_IDENTIFIER?: string;
  /** systemd 单元名（如 "sshd.service"） */
  _SYSTEMD_UNIT?: string;
  /** 日志消息正文 */
  MESSAGE?: string;
  /** 优先级 0-7 (0=emerg, 7=debug) */
  PRIORITY?: string;
  /** 来源主机名 */
  _HOSTNAME?: string;
  /** 启动 ID */
  _BOOT_ID?: string;
  /** 其他动态字段（journald JSON 输出可包含 number/object/array 等任意类型） */
  [key: string]: unknown;
}

export type LogLevel = "emerg" | "alert" | "crit" | "err" | "warning" | "notice" | "info" | "debug";

export interface JournaldFilter {
  level?: LogLevel | null;
  keyword?: string;
  unit?: string;
  kernelOnly?: boolean;
  since?: string | null;
  until?: string | null;
}

export type DisplayMode = "compact" | "full";

export type SubTab = "realtime" | "history";

export interface JournaldQueryResponse {
  entries: JournalEntry[];
  next_cursor: string | null;
  has_more: boolean;
}

/** 日志级别映射 */
export const LOG_LEVELS: { value: LogLevel; priority: number }[] = [
  { value: "emerg", priority: 0 },
  { value: "alert", priority: 1 },
  { value: "crit", priority: 2 },
  { value: "err", priority: 3 },
  { value: "warning", priority: 4 },
  { value: "notice", priority: 5 },
  { value: "info", priority: 6 },
  { value: "debug", priority: 7 },
];

/** 根据优先级数字获取 CSS 类名 */
export function priorityToLevelClass(priority: string | undefined): string {
  const p = parseInt(priority ?? "6", 10);
  if (p <= 3) return "levelError";
  if (p === 4) return "levelWarning";
  if (p <= 6) return "levelInfo";
  return "levelDebug";
}

/** 将微秒时间戳转换为可读本地时间（locale-independent 格式：YYYY-MM-DD HH:mm:ss） */
export function formatTimestamp(microTimestamp: string | undefined): string {
  if (!microTimestamp) return "";
  const micro = parseInt(microTimestamp, 10);
  if (isNaN(micro)) return microTimestamp;
  const d = new Date(micro / 1000);
  const Y = d.getFullYear();
  const M = String(d.getMonth() + 1).padStart(2, "0");
  const D = String(d.getDate()).padStart(2, "0");
  const h = String(d.getHours()).padStart(2, "0");
  const m = String(d.getMinutes()).padStart(2, "0");
  const s = String(d.getSeconds()).padStart(2, "0");
  return `${Y}-${M}-${D} ${h}:${m}:${s}`;
}

/** 从微秒时间戳中仅提取时间部分 (HH:mm:ss) */
export function formatTimestampTime(microTimestamp: string | undefined): string {
  if (!microTimestamp) return "";
  const micro = parseInt(microTimestamp, 10);
  if (isNaN(micro)) return microTimestamp;
  const d = new Date(micro / 1000);
  const h = String(d.getHours()).padStart(2, "0");
  const m = String(d.getMinutes()).padStart(2, "0");
  const s = String(d.getSeconds()).padStart(2, "0");
  return `${h}:${m}:${s}`;
}

/** 优先级数字转可读标签 */
export function priorityLabel(priority: string | undefined): string {
  const p = parseInt(priority ?? "6", 10);
  const labels = ["EMERG", "ALERT", "CRIT", "ERR", "WARNING", "NOTICE", "INFO", "DEBUG"];
  return labels[p] ?? "INFO";
}

/** 最大条目缓冲区（防止内存溢出） */
export const MAX_ENTRIES = 1000;
