/** Normalized journald record plus the complete raw field map. */
export interface JournalEntry {
  monotonicTimestamp?: string | null;
  realtimeTimestamp?: string | null;
  cursor?: string | null;
  identifier?: string | null;
  unit?: string | null;
  message?: string | null;
  priority?: string | null;
  hostname?: string | null;
  bootId?: string | null;
  fields: Record<string, unknown>;
}

export type LogLevel =
  | "emerg"
  | "alert"
  | "crit"
  | "err"
  | "warning"
  | "notice"
  | "info"
  | "debug";

export type JournaldSearchMode = "literal" | "regex";

export interface JournaldFilter {
  level?: LogLevel | null;
  keyword?: string;
  searchMode?: JournaldSearchMode;
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

export interface JournaldErrorPayload {
  code: string;
  message: string;
}

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

export type PriorityLevelClass =
  | "levelError"
  | "levelWarning"
  | "levelInfo"
  | "levelDebug";

export function priorityToLevelClass(priority: string | null | undefined): PriorityLevelClass {
  const parsed = Number.parseInt(priority ?? "6", 10);
  if (parsed <= 3) return "levelError";
  if (parsed === 4) return "levelWarning";
  if (parsed <= 6) return "levelInfo";
  return "levelDebug";
}

export function formatTimestamp(microTimestamp: string | null | undefined): string {
  if (!microTimestamp) return "";
  const micro = Number.parseInt(microTimestamp, 10);
  if (Number.isNaN(micro)) return microTimestamp;
  const date = new Date(micro / 1000);
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  const hour = String(date.getHours()).padStart(2, "0");
  const minute = String(date.getMinutes()).padStart(2, "0");
  const second = String(date.getSeconds()).padStart(2, "0");
  const millisecond = String(date.getMilliseconds()).padStart(3, "0");
  return `${year}-${month}-${day} ${hour}:${minute}:${second}.${millisecond}`;
}

export function formatTimestampTime(microTimestamp: string | null | undefined): string {
  if (!microTimestamp) return "";
  const micro = Number.parseInt(microTimestamp, 10);
  if (Number.isNaN(micro)) return microTimestamp;
  const date = new Date(micro / 1000);
  const hour = String(date.getHours()).padStart(2, "0");
  const minute = String(date.getMinutes()).padStart(2, "0");
  const second = String(date.getSeconds()).padStart(2, "0");
  const millisecond = String(date.getMilliseconds()).padStart(3, "0");
  return `${hour}:${minute}:${second}.${millisecond}`;
}

export function priorityLabel(priority: string | null | undefined): string {
  const parsed = Number.parseInt(priority ?? "6", 10);
  const labels = [
    "EMERG",
    "ALERT",
    "CRIT",
    "ERR",
    "WARNING",
    "NOTICE",
    "INFO",
    "DEBUG",
  ];
  return labels[parsed] ?? "INFO";
}

/** Realtime UI buffer. Rendering is windowed in compact mode. */
export const MAX_ENTRIES = 5000;
