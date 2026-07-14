/** 发送栏模式 */
export type SendBarMode = "basic" | "command" | "auto-reply" | "script";

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

// ── 自动应答模式 ──────────────────────────────────────

/** 匹配模式 */
export type MatchMode =
  | "contains"
  | "equals"
  | "starts_with"
  | "regex"
  | "lua_pattern";

/** 匹配策略 */
export type MatchStrategy = "first" | "all";

/** 匹配表达式格式 */
export type MatchFormat = "text" | "hex";

/** 触发类型：数据驱动 | 定时器 */
export type TriggerType = "data" | "timer";

/** 条件间组合逻辑 */
export type ConditionLogic = "and" | "or";

/** 单个匹配条件（用于多条件组合逻辑） */
export interface MatchCondition {
  /** 匹配模式字符串 */
  pattern: string;
  /** 匹配方式 */
  mode: MatchMode;
  /** 是否区分大小写 */
  caseSensitive: boolean;
  /** 取反：true 表示"不匹配时才触发" */
  negate: boolean;
  /** 匹配格式（默认 text） */
  matchFormat?: MatchFormat;
}

/** 序列回复中的单个动作
 *
 * 支持以下动态宏（{{MACRO}} 模板）：
 *   {{CAPTURE(n)}}          — 正则捕获组 n
 *   {{RANDOM(min,max)}}      — 随机整数
 *   {{RANDOM_F(min,max,dec)}}— 随机浮点数
 *   {{TIMESTAMP}}            — Unix 毫秒时间戳
 *   {{DATETIME}}             — ISO 8601 日期时间
 *   {{DATETIME_F(format)}}   — 自定义 strftime 格式
 *   {{COUNTER}}              — 规则级自增计数器
 *   {{HEX(text)}}            — 文本转 HEX 大写字符串
 *   {{HEXVAL(num,width)}}    — 数字格式化为大写 HEX（如 {{HEXVAL(255,2)}} → "FF"）
 *   {{SIN(min,max,period)}}  — 正弦波传感器模拟值
 *   {{EXPR:expression}}      — 安全算术表达式（支持位运算 & | ~ << >>）
 *   {{CRC(data, width, poly)}} — 统一 CRC 计算（width: 8/16/32, poly: 多项式）
 *      示例: {{CRC(data, 16, 0x8005)}}=Modbus, {{CRC(data, 16, 0x1021)}}=CCITT
 *      {{CRC(data, 32, 0x04C11DB7)}}=CRC-32, {{CRC(data, 8, 0x07)}}=CRC-8/ITU
 *   {{XOR_SUM(data)}}        — XOR 校验和（NMEA *XX 格式）
 *   {{SUM8(data)}}           — 求和校验低 8 位（Intel HEX 格式）
 *
 * 宏支持嵌套展开，如 {{HEXVAL({{RANDOM(0,255)}},2)}}。 */
export interface ReplyAction {
  /** 本步延时 (ms) */
  delayMs: number;
  /** 回复数据，支持 {{MACRO}} 模板 */
  data: string;
  /** 回复格式 */
  format: "text" | "hex";
}

/** 单条自动应答规则
 *
 * 统一模型：匹配条件一律以 conditions 数组表示（data 规则至少 1 条），
 * timer 规则 conditions 为空。 */
export interface AutoReplyRule {
  id: string;
  /** 可选标签，用于标识规则用途 */
  label?: string;

  // ── 触发方式 ──
  /** 触发类型 */
  triggerType: TriggerType;
  /** 定时器间隔 (ms)（triggerType = "timer" 时使用） */
  timerIntervalMs: number;

  // ── 匹配条件（data 规则）──
  /** 匹配条件列表（data 规则至少 1 条） */
  conditions: MatchCondition[];
  /** 条件间逻辑 */
  conditionLogic: ConditionLogic;

  // ── 回复动作 ──
  /** 动作列表，支持拖拽排序 */
  actions: ReplyAction[];

  // ── 执行控制 ──
  /** 是否启用此规则 */
  enabled: boolean;
  /** 冷却时间 (ms)，0 = 无冷却 */
  cooldownMs: number;
}

/** 自动应答规则集配置 */
export interface AutoReplyConfig {
  name: string;
  /** 匹配策略：first = 首条命中即停 | all = 全部执行 */
  matchStrategy: MatchStrategy;
  rules: AutoReplyRule[];
}

// ── 脚本模式 ──────────────────────────────────────────

/** 脚本记录（持久化到 localStorage） */
export interface ScriptRecord {
  id: string;
  name: string;
  code: string;
  createdAt: number;
  updatedAt: number;
}
