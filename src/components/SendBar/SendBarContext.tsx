import { createContext, useContext, useReducer, type ReactNode } from "react";
import type { SendBarMode, NewlineMode, SendMode, AutoReplyRule, AutoReplyConfig, MatchStrategy, ScriptRecord } from "./types";
import { BUILTIN_CONFIGS } from "./builtinRules";
import { BUILTIN_SCRIPTS } from "./builtinScripts";

// ── State ────────────────────────────────────────────

export interface SendBarState {
  mode: SendBarMode;
  basic: {
    inputText: string;
    newlineMode: NewlineMode;
    sendMode: SendMode;
    repeatEnabled: boolean;
    repeatInterval: number;
    sendHistory: string[];
  };
  command: {
    selectedIds: Set<string>;
    loopCount: number;
  };
  autoReply: {
    configs: AutoReplyConfig[];
    activeConfigName: string;
    rules: AutoReplyRule[];
    isRunning: boolean;
    matchStrategy: MatchStrategy;
  };
  script: {
    scripts: ScriptRecord[];
    activeScriptId: string | null;
    code: string;
    isRunning: boolean;
  };
  /** 共享脚本日志 — 始终由 Provider 层监听，不依赖面板模式焦点 */
  scriptLogs: string[];
}

const initialBasicState = (): SendBarState["basic"] => {
  const stored = localStorage.getItem("tauterm-default-data-mode");
  return {
    inputText: "",
    newlineMode: "crlf",
    sendMode: stored === "hex" ? "hex" : "text",
    repeatEnabled: false,
    repeatInterval: 1000,
    sendHistory: [],
  };
};

/**
 * 加载自动应答配置，首次使用时自动注入内置示例配置。
 *
 * 内置配置按 name 去重：若 localStorage 中已存在同名配置则保留用户版本，
 * 仅追加重名不存在的内置配置。这样用户修改过的内置示例不会被覆盖。
 */
const loadAutoReplyConfigs = (): AutoReplyConfig[] => {
  try {
    const raw = localStorage.getItem("tauterm-auto-reply-configs");
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) {
        const existing = parsed as AutoReplyConfig[];
        // 归并内置配置（按 name 去重，保留用户版本）
        const existingNames = new Set(existing.map(c => c.name));
        const newBuiltins = BUILTIN_CONFIGS.filter(c => !existingNames.has(c.name));
        if (newBuiltins.length > 0) {
          const merged = [...existing, ...newBuiltins];
          localStorage.setItem("tauterm-auto-reply-configs", JSON.stringify(merged));
          return merged;
        }
        return existing;
      }
    }
  } catch { /* ignore */ }
  // 无配置：首次使用 → 直接返回内置配置并持久化
  localStorage.setItem("tauterm-auto-reply-configs", JSON.stringify(BUILTIN_CONFIGS));
  return BUILTIN_CONFIGS;
};

/**
 * 读取上次选中的自动应答配置名。
 *
 * 镜像 loadActiveScriptId：初始化时回读持久化的选择，避免刷新/重启后
 * activeConfigName 丢失（此前硬编码为 ""，导致每次都回退到 configs[0]，
 * 所选配置及其 matchStrategy 被静默丢弃）。校验名称仍存在于配置列表中，
 * 否则回退到首个配置名。
 */
const loadActiveAutoReplyConfig = (configs: AutoReplyConfig[]): string => {
  const stored = localStorage.getItem("tauterm-active-auto-reply-config");
  if (stored && configs.some(c => c.name === stored)) return stored;
  return configs[0]?.name ?? "";
};

/**
 * 加载脚本列表，首次使用时自动注入内置示例脚本。
 *
 * 内置脚本按 id 去重：若 localStorage 中已存在同 id 脚本则保留用户版本，
 * 仅追加 id 不存在的内置脚本。用户重命名或修改内置脚本不会被覆盖。
 */
const loadScripts = (): ScriptRecord[] => {
  try {
    const raw = localStorage.getItem("tauterm-scripts");
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) {
        const existing = parsed as ScriptRecord[];
        // 归并内置脚本（按 id 去重，保留用户版本）
        const existingIds = new Set(existing.map(s => s.id));
        const newBuiltins = BUILTIN_SCRIPTS.filter(s => !existingIds.has(s.id));
        if (newBuiltins.length > 0) {
          const merged = [...existing, ...newBuiltins];
          localStorage.setItem("tauterm-scripts", JSON.stringify(merged));
          return merged;
        }
        return existing;
      }
    }
  } catch { /* ignore */ }
  // 无脚本：首次使用 → 直接返回内置脚本并持久化
  localStorage.setItem("tauterm-scripts", JSON.stringify(BUILTIN_SCRIPTS));
  return BUILTIN_SCRIPTS;
};

const loadActiveScriptId = (): string | null => {
  return localStorage.getItem("tauterm-active-script-id") || null;
};

/**
 * 构建初始状态（惰性求值：每次 SendBarProvider 挂载时调用，
 * 重新读取 localStorage，确保状态不是模块加载时冻结的旧值。）
 */
function buildInitialState(): SendBarState {
  const scripts = loadScripts();
  const activeScriptId = loadActiveScriptId();
  const activeCode = scripts.find(s => s.id === activeScriptId)?.code ?? "";

  const autoReplyConfigs = loadAutoReplyConfigs();
  const activeConfigName = loadActiveAutoReplyConfig(autoReplyConfigs);
  const active = autoReplyConfigs.find(c => c.name === activeConfigName);
  return {
    mode: "basic",
    basic: initialBasicState(),
    command: {
      selectedIds: new Set<string>(),
      loopCount: 1,
    },
    autoReply: {
      configs: autoReplyConfigs,
      activeConfigName,
      rules: [],
      isRunning: false,
      matchStrategy: active?.matchStrategy ?? "all",
    },
    script: {
      scripts,
      activeScriptId,
      code: activeCode,
      isRunning: false,
    },
    scriptLogs: [],
  };
}

// ── Actions ──────────────────────────────────────────

export type SendBarAction =
  | { type: "SET_MODE"; mode: SendBarMode }
  // Basic
  | { type: "SET_INPUT_TEXT"; text: string }
  | { type: "SET_NEWLINE_MODE"; mode: NewlineMode }
  | { type: "SET_SEND_MODE"; mode: SendMode }
  | { type: "SET_REPEAT_ENABLED"; enabled: boolean }
  | { type: "SET_REPEAT_INTERVAL"; ms: number }
  | { type: "ADD_SEND_HISTORY"; entry: string }
  | { type: "RESET_BASIC" }
  // Command
  | { type: "TOGGLE_COMMAND_SELECT"; id: string }
  | { type: "CLEAR_COMMAND_SELECTION" }
  | { type: "SELECT_ALL_COMMANDS"; ids: string[] }
  | { type: "SET_LOOP_COUNT"; count: number }
  // AutoReply
  | { type: "SET_AUTO_REPLY_CONFIGS"; configs: AutoReplyConfig[] }
  | { type: "SET_ACTIVE_AUTO_REPLY_CONFIG"; name: string }
  | { type: "SET_AUTO_REPLY_RULES"; rules: AutoReplyRule[] }
  | { type: "SET_AUTO_REPLY_RUNNING"; running: boolean }
  | { type: "SET_MATCH_STRATEGY"; strategy: MatchStrategy }
  // Script
  | { type: "SET_SCRIPTS"; scripts: ScriptRecord[] }
  | { type: "SET_ACTIVE_SCRIPT"; id: string | null }
  | { type: "SET_SCRIPT_CODE"; code: string }
  | { type: "SET_SCRIPT_RUNNING"; running: boolean }
  // Shared script logs (always-on listener in SendBarInner)
  | { type: "APPEND_SCRIPT_LOG"; message: string }
  | { type: "CLEAR_SCRIPT_LOGS" };

function sendBarReducer(state: SendBarState, action: SendBarAction): SendBarState {
  switch (action.type) {
    case "SET_MODE":
      return { ...state, mode: action.mode };

    // Basic
    case "SET_INPUT_TEXT":
      return { ...state, basic: { ...state.basic, inputText: action.text } };

    case "SET_NEWLINE_MODE":
      return { ...state, basic: { ...state.basic, newlineMode: action.mode } };

    case "SET_SEND_MODE":
      return { ...state, basic: { ...state.basic, sendMode: action.mode } };

    case "SET_REPEAT_ENABLED":
      return { ...state, basic: { ...state.basic, repeatEnabled: action.enabled } };

    case "SET_REPEAT_INTERVAL":
      return { ...state, basic: { ...state.basic, repeatInterval: action.ms } };

    case "ADD_SEND_HISTORY": {
      const entry = action.entry;
      const next = [entry, ...state.basic.sendHistory.filter(h => h !== entry)];
      return {
        ...state,
        basic: { ...state.basic, sendHistory: next.slice(0, 50) },
      };
    }

    case "RESET_BASIC":
      return { ...state, basic: initialBasicState() };

    // Command
    case "TOGGLE_COMMAND_SELECT": {
      const next = new Set(state.command.selectedIds);
      if (next.has(action.id)) {
        next.delete(action.id);
      } else {
        next.add(action.id);
      }
      return { ...state, command: { ...state.command, selectedIds: next } };
    }

    case "CLEAR_COMMAND_SELECTION":
      return { ...state, command: { ...state.command, selectedIds: new Set<string>() } };

    case "SELECT_ALL_COMMANDS":
      return { ...state, command: { ...state.command, selectedIds: new Set(action.ids) } };

    case "SET_LOOP_COUNT":
      return { ...state, command: { ...state.command, loopCount: action.count } };

    // AutoReply
    case "SET_AUTO_REPLY_CONFIGS":
      return { ...state, autoReply: { ...state.autoReply, configs: action.configs } };

    case "SET_ACTIVE_AUTO_REPLY_CONFIG":
      return { ...state, autoReply: { ...state.autoReply, activeConfigName: action.name } };

    case "SET_AUTO_REPLY_RULES":
      return { ...state, autoReply: { ...state.autoReply, rules: action.rules } };

    case "SET_AUTO_REPLY_RUNNING":
      return { ...state, autoReply: { ...state.autoReply, isRunning: action.running } };

    case "SET_MATCH_STRATEGY":
      return { ...state, autoReply: { ...state.autoReply, matchStrategy: action.strategy } };

    // Script
    case "SET_SCRIPTS":
      return { ...state, script: { ...state.script, scripts: action.scripts } };

    case "SET_ACTIVE_SCRIPT":
      return { ...state, script: { ...state.script, activeScriptId: action.id } };

    case "SET_SCRIPT_CODE":
      return { ...state, script: { ...state.script, code: action.code } };

    case "SET_SCRIPT_RUNNING":
      return { ...state, script: { ...state.script, isRunning: action.running } };

    // Shared script logs
    case "APPEND_SCRIPT_LOG":
      return {
        ...state,
        scriptLogs: [...state.scriptLogs.slice(-499), action.message],
      };

    case "CLEAR_SCRIPT_LOGS":
      return { ...state, scriptLogs: [] };

    default:
      return state;
  }
}

// ── Context ──────────────────────────────────────────

interface SendBarContextValue {
  state: SendBarState;
  dispatch: React.Dispatch<SendBarAction>;
}

const SendBarContext = createContext<SendBarContextValue | null>(null);

export function SendBarProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(sendBarReducer, buildInitialState());
  return (
    <SendBarContext.Provider value={{ state, dispatch }}>
      {children}
    </SendBarContext.Provider>
  );
}

export function useSendBar() {
  const ctx = useContext(SendBarContext);
  if (!ctx) throw new Error("useSendBar must be used within SendBarProvider");
  return ctx;
}
