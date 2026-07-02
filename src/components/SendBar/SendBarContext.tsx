import { createContext, useContext, useReducer, type ReactNode } from "react";
import type { SendBarMode, NewlineMode, SendMode } from "./types";

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

const initialState: SendBarState = {
  mode: "basic",
  basic: initialBasicState(),
  command: {
    selectedIds: new Set<string>(),
    loopCount: 1,
  },
};

// ── Actions ──────────────────────────────────────────

export type SendBarAction =
  | { type: "SET_MODE"; mode: SendBarMode }
  | { type: "SET_INPUT_TEXT"; text: string }
  | { type: "SET_NEWLINE_MODE"; mode: NewlineMode }
  | { type: "SET_SEND_MODE"; mode: SendMode }
  | { type: "SET_REPEAT_ENABLED"; enabled: boolean }
  | { type: "SET_REPEAT_INTERVAL"; ms: number }
  | { type: "ADD_SEND_HISTORY"; entry: string }
  | { type: "RESET_BASIC" }
  | { type: "TOGGLE_COMMAND_SELECT"; id: string }
  | { type: "CLEAR_COMMAND_SELECTION" }
  | { type: "SELECT_ALL_COMMANDS"; ids: string[] }
  | { type: "SET_LOOP_COUNT"; count: number };

function sendBarReducer(state: SendBarState, action: SendBarAction): SendBarState {
  switch (action.type) {
    case "SET_MODE":
      return { ...state, mode: action.mode };

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
  const [state, dispatch] = useReducer(sendBarReducer, initialState);

  // dispatch 由 useReducer 保证稳定引用；state 每次 dispatch 后更新。
  // 由于 state 引用每次都变，useMemo 无实际收益 — 直接传递对象即可。
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
