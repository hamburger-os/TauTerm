import { createContext, useContext, useEffect, useReducer, useRef, type ReactNode } from "react";
import type { SendBarMode, NewlineMode, SendMode, AutoReplyRule, AutoReplyConfig, MatchStrategy, ScriptRecord } from "./types";
import { BUILTIN_CONFIGS } from "./builtinRules";
import { BUILTIN_SCRIPTS } from "./builtinScripts";
import { ASSET_KEYS, loadAsset, persistAsset, subscribeAsset } from "./assetStore";

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
    /** Per-session selection. The global asset key is only a default for a new SendBar. */
    activeConfigName: string;
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
    /** Per-session editor draft. Shared asset updates must never overwrite it implicitly. */
    code: string;
    isRunning: boolean;
  };
  /** 共享脚本日志 — 始终由 Provider 层监听，不依赖面板模式焦点 */
  scriptLogs: string[];
}

const initialBasicState = (): SendBarState["basic"] => ({
  inputText: "",
  newlineMode: "crlf",
  sendMode: "text",
  repeatEnabled: false,
  repeatInterval: 1000,
  sendHistory: [],
});

function buildInitialState(): SendBarState {
  const autoReplyConfigs = [...BUILTIN_CONFIGS];
  const activeConfigName = autoReplyConfigs[0]?.name ?? "";
  const active = autoReplyConfigs.find(config => config.name === activeConfigName);
  return {
    mode: "basic",
    basic: initialBasicState(),
    command: {
      activeConfigName: "",
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
  | { type: "SET_ACTIVE_COMMAND_CONFIG"; name: string }
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
    case "SET_ACTIVE_COMMAND_CONFIG":
      return { ...state, command: { ...state.command, activeConfigName: action.name } };
    case "TOGGLE_COMMAND_SELECT": {
      const next = new Set(state.command.selectedIds);
      if (next.has(action.id)) next.delete(action.id);
      else next.add(action.id);
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
  const stateRef = useRef(state);
  stateRef.current = state;

  useEffect(() => {
    let cancelled = false;
    void Promise.all([
      loadAsset<AutoReplyConfig[]>(ASSET_KEYS.autoReplyConfigs),
      loadAsset<string>(ASSET_KEYS.activeAutoReplyConfig),
      loadAsset<ScriptRecord[]>(ASSET_KEYS.scripts),
      loadAsset<string>(ASSET_KEYS.activeScriptId),
    ]).then(([storedConfigs, storedActiveConfig, storedScripts, storedActiveScript]) => {
      if (cancelled) return;

      // Built-ins seed an uninitialized store once. An explicit empty array is a valid user state:
      // deleted examples stay deleted until the user chooses "load built-in examples" again.
      const hasStoredConfigs = Array.isArray(storedConfigs);
      const autoReplyConfigs = hasStoredConfigs ? storedConfigs : [...BUILTIN_CONFIGS];
      const activeConfigName = storedActiveConfig
        && autoReplyConfigs.some(config => config.name === storedActiveConfig)
        ? storedActiveConfig
        : autoReplyConfigs[0]?.name ?? "";
      const activeConfig = autoReplyConfigs.find(config => config.name === activeConfigName);

      const hasStoredScripts = Array.isArray(storedScripts);
      const scripts = hasStoredScripts ? storedScripts : [...BUILTIN_SCRIPTS];
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

      if (!hasStoredConfigs) void persistAsset(ASSET_KEYS.autoReplyConfigs, autoReplyConfigs);
      if (!hasStoredScripts) void persistAsset(ASSET_KEYS.scripts, scripts);
    }).catch(() => {
      // Built-ins remain usable if the persistent store is temporarily unavailable.
    });

    return () => { cancelled = true; };
  }, []);

  // Asset definitions are global; active selections and editor drafts are session-local.
  useEffect(() => {
    const unsubscribeConfigs = subscribeAsset<AutoReplyConfig[]>(
      ASSET_KEYS.autoReplyConfigs,
      value => {
        const configs = Array.isArray(value) ? value : [];
        const current = stateRef.current.autoReply;
        const activeName = configs.some(config => config.name === current.activeConfigName)
          ? current.activeConfigName
          : configs[0]?.name ?? "";
        const active = configs.find(config => config.name === activeName);

        dispatch({ type: "SET_AUTO_REPLY_CONFIGS", configs });
        if (activeName !== current.activeConfigName) {
          dispatch({ type: "SET_ACTIVE_AUTO_REPLY_CONFIG", name: activeName });
          dispatch({ type: "SET_AUTO_REPLY_RULES", rules: active?.rules ?? [] });
          dispatch({ type: "SET_MATCH_STRATEGY", strategy: active?.matchStrategy ?? "all" });
        }
      },
    );

    const unsubscribeScripts = subscribeAsset<ScriptRecord[]>(
      ASSET_KEYS.scripts,
      value => {
        const scripts = Array.isArray(value) ? value : [];
        const current = stateRef.current.script;
        const previousActive = current.activeScriptId
          ? current.scripts.find(script => script.id === current.activeScriptId)
          : undefined;
        const nextActive = current.activeScriptId
          ? scripts.find(script => script.id === current.activeScriptId)
          : undefined;
        const hasLocalDraft = previousActive != null && current.code !== previousActive.code;

        dispatch({ type: "SET_SCRIPTS", scripts });

        if (current.activeScriptId && !nextActive) {
          dispatch({ type: "SET_ACTIVE_SCRIPT", id: null });
          dispatch({ type: "SET_SCRIPT_CODE", code: "" });
          return;
        }

        // A clean editor follows shared asset changes; an unsaved local draft remains untouched.
        if (nextActive && !hasLocalDraft && nextActive.code !== current.code) {
          dispatch({ type: "SET_SCRIPT_CODE", code: nextActive.code });
        }
      },
    );

    return () => {
      unsubscribeConfigs();
      unsubscribeScripts();
    };
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
