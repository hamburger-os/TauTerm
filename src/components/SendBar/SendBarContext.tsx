import { createContext, useContext, useEffect, useReducer, type ReactNode } from "react";
import type { SendBarMode, NewlineMode, SendMode, AutoReplyRule, AutoReplyConfig, MatchStrategy, ScriptRecord } from "./types";
import { BUILTIN_CONFIGS } from "./builtinRules";
import { BUILTIN_SCRIPTS } from "./builtinScripts";
import { ASSET_KEYS, loadAsset, persistAsset } from "./assetStore";

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
  return {
    inputText: "",
    newlineMode: "crlf",
    sendMode: "text",
    repeatEnabled: false,
    repeatInterval: 1000,
    sendHistory: [],
  };
};

function buildInitialState(): SendBarState {
  const autoReplyConfigs = [...BUILTIN_CONFIGS];
  const activeConfigName = autoReplyConfigs[0]?.name ?? "";
  const active = autoReplyConfigs.find(config => config.name === activeConfigName);
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
      rules: active?.rules ?? [],
      isRunning: false,
      matchStrategy: active?.matchStrategy ?? "all",
    },
    script: {
      scripts: [...BUILTIN_SCRIPTS],
      activeScriptId: null,
      code: "",
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

  useEffect(() => {
    let cancelled = false;
    void Promise.all([
      loadAsset<AutoReplyConfig[]>(ASSET_KEYS.autoReplyConfigs),
      loadAsset<string>(ASSET_KEYS.activeAutoReplyConfig),
      loadAsset<ScriptRecord[]>(ASSET_KEYS.scripts),
      loadAsset<string>(ASSET_KEYS.activeScriptId),
    ]).then(([storedConfigs, storedActiveConfig, storedScripts, storedActiveScript]) => {
      if (cancelled) return;

      const existingConfigs = Array.isArray(storedConfigs) ? storedConfigs : [];
      const existingConfigNames = new Set(existingConfigs.map(config => config.name));
      const autoReplyConfigs = [
        ...existingConfigs,
        ...BUILTIN_CONFIGS.filter(config => !existingConfigNames.has(config.name)),
      ];
      const activeConfigName = storedActiveConfig
        && autoReplyConfigs.some(config => config.name === storedActiveConfig)
        ? storedActiveConfig
        : autoReplyConfigs[0]?.name ?? "";
      const activeConfig = autoReplyConfigs.find(config => config.name === activeConfigName);

      const existingScripts = Array.isArray(storedScripts) ? storedScripts : [];
      const existingScriptIds = new Set(existingScripts.map(script => script.id));
      const scripts = [
        ...existingScripts,
        ...BUILTIN_SCRIPTS.filter(script => !existingScriptIds.has(script.id)),
      ];
      const activeScriptId = storedActiveScript
        && scripts.some(script => script.id === storedActiveScript)
        ? storedActiveScript
        : null;
      const activeCode = scripts.find(script => script.id === activeScriptId)?.code ?? "";

      dispatch({ type: "SET_AUTO_REPLY_CONFIGS", configs: autoReplyConfigs });
      dispatch({ type: "SET_ACTIVE_AUTO_REPLY_CONFIG", name: activeConfigName });
      dispatch({ type: "SET_AUTO_REPLY_RULES", rules: activeConfig?.rules ?? [] });
      dispatch({ type: "SET_MATCH_STRATEGY", strategy: activeConfig?.matchStrategy ?? "all" });
      dispatch({ type: "SET_SCRIPTS", scripts });
      dispatch({ type: "SET_ACTIVE_SCRIPT", id: activeScriptId });
      dispatch({ type: "SET_SCRIPT_CODE", code: activeCode });

      // Built-in examples are assets too. Persist the merged canonical view once so later
      // components can update it without browser-local fallback state.
      persistAsset(ASSET_KEYS.autoReplyConfigs, autoReplyConfigs);
      persistAsset(ASSET_KEYS.activeAutoReplyConfig, activeConfigName);
      persistAsset(ASSET_KEYS.scripts, scripts);
      if (activeScriptId) {
        persistAsset(ASSET_KEYS.activeScriptId, activeScriptId);
      }
    }).catch(() => {
      // Built-ins remain fully usable if the persistent store is temporarily unavailable.
    });

    return () => { cancelled = true; };
  }, []);

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
