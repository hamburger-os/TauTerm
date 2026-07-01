/** 发送栏模式 */
export type SendBarMode = "basic" | "command";

/** 基础发送模式中的换行符类型 */
export type NewlineMode = "crlf" | "lf" | "cr" | "none";

/** 基础发送模式中的发送模式 */
export type SendMode = "text" | "hex";

/** 指令面板中的单条命令 */
export interface CommandItem {
  id: string;
  command: string;
  note: string;
  delay: number; // ms
}

/** 指令面板的命令配置 */
export interface CommandConfig {
  version: 1;
  name: string;
  defaultDelay: number;
  commands: CommandItem[];
}

/** 循环配置 — 由 BasicSend 和 CommandPanel 共用 */
export interface LoopConfig {
  /** 循环总次数。-1 = 无限循环，0 = 已停止/未开始 */
  count: number;
  /** 当前循环进度 (0-based) */
  current: number;
}

/** 执行状态 — 由 BasicSend 和 CommandPanel 共用 */
export interface ExecutionState {
  isRunning: boolean;
  loopConfig: LoopConfig | null;
  /** 仅 CommandPanel 使用，当前正在执行的命令索引 */
  currentCommandIndex: number | null;
}
