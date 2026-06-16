import { createContext, useContext, useReducer, useCallback, useEffect, useRef, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ── Types ───────────────────────────────────────────

export type ConnectionStatus = "disconnected" | "connecting" | "connected";

export interface TabInfo {
  id: string;
  name: string;
  connection_type: string;
  endpoint: string;
  state: ConnectionStatus;
  /** 连接参数（恢复会话时用于回填配置） */
  params?: Record<string, unknown>;
}

export interface ConnectionTypeInfo {
  id: string;
  label: string;
  available: boolean;
}

export interface EndpointInfo {
  name: string;
  description: string;
  connection_type: string;
}

interface SessionState {
  tabs: TabInfo[];
  activeTabId: string | null;
  connectionTypes: ConnectionTypeInfo[];
  endpoints: EndpointInfo[];
  error: string | null;
}

type SessionAction =
  | { type: "SET_TABS"; tabs: TabInfo[] }
  | { type: "ADD_TAB"; tab: TabInfo }
  | { type: "REMOVE_TAB"; id: string }
  | { type: "RENAME_TAB"; id: string; name: string }
  | { type: "REORDER_TABS"; ids: string[] }
  | { type: "SET_ACTIVE"; id: string }
  | { type: "SET_CONNECTION_TYPES"; types: ConnectionTypeInfo[] }
  | { type: "SET_ENDPOINTS"; endpoints: EndpointInfo[] }
  | { type: "SET_ERROR"; error: string | null }
  | { type: "SET_TAB_STATE"; id: string; state: ConnectionStatus }
  | { type: "CLEAR_TABS" };

const initialState: SessionState = {
  tabs: [],
  activeTabId: null,
  connectionTypes: [],
  endpoints: [],
  error: null,
};

function sessionReducer(state: SessionState, action: SessionAction): SessionState {
  switch (action.type) {
    case "SET_TABS":
      return { ...state, tabs: action.tabs };
    case "ADD_TAB": {
      // 如果存在相同 endpoint 的已断开标签页，替换它（而非追加重复标签页）
      const existingIdx = state.tabs.findIndex(
        t => t.state === "disconnected" && t.endpoint === action.tab.endpoint
      );
      const newTabs = existingIdx >= 0
        ? state.tabs.map((t, i) => i === existingIdx ? action.tab : t)
        : [...state.tabs, action.tab];
      return { ...state, tabs: newTabs, activeTabId: action.tab.id };
    }
    case "REMOVE_TAB":
      return {
        ...state,
        tabs: state.tabs.filter(t => t.id !== action.id),
        activeTabId: state.activeTabId === action.id
          ? state.tabs.find(t => t.id !== action.id)?.id ?? null
          : state.activeTabId,
      };
    case "RENAME_TAB":
      return {
        ...state,
        tabs: state.tabs.map(t => t.id === action.id ? { ...t, name: action.name } : t),
      };
    case "REORDER_TABS":
      return {
        ...state,
        tabs: action.ids.map(id => state.tabs.find(t => t.id === id)!).filter(Boolean),
      };
    case "SET_ACTIVE":
      return { ...state, activeTabId: action.id };
    case "SET_CONNECTION_TYPES":
      return { ...state, connectionTypes: action.types };
    case "SET_ENDPOINTS":
      return { ...state, endpoints: action.endpoints };
    case "SET_ERROR":
      return { ...state, error: action.error };
    case "SET_TAB_STATE":
      return {
        ...state,
        tabs: state.tabs.map(t => t.id === action.id ? { ...t, state: action.state } : t),
      };
    case "CLEAR_TABS":
      return { ...state, tabs: [], activeTabId: null };
    default:
      return state;
  }
}

// ── Context ──────────────────────────────────────────

interface SessionContextValue {
  state: SessionState;
  fetchConnectionTypes: () => Promise<void>;
  refreshEndpoints: () => Promise<void>;
  connect: (endpoint: string, params: Record<string, unknown>, name?: string) => Promise<string | null>;
  disconnect: (sessionId: string) => Promise<void>;
  deleteSession: (sessionId: string) => Promise<void>;
  sendData: (sessionId: string, data: string | Uint8Array) => Promise<void>;
  switchTab: (sessionId: string) => Promise<void>;
  renameTab: (sessionId: string, name: string) => Promise<void>;
  getTabs: () => Promise<void>;
  onSessionData: (callback: (sessionId: string, data: Uint8Array) => void) => void;
  onSessionDisconnect: (callback: (sessionId: string, reason?: string) => void) => void;
  clearError: () => void;
}

const SessionContext = createContext<SessionContextValue | null>(null);

export function SessionProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(sessionReducer, initialState);
  const dataCallbackRef = useRef<((sessionId: string, data: Uint8Array) => void) | null>(null);
  const disconnectCallbackRef = useRef<((sessionId: string, reason?: string) => void) | null>(null);

  // ── Actions ─────────────────────────────────────

  const fetchConnectionTypes = useCallback(async () => {
    try {
      const types = await invoke<ConnectionTypeInfo[]>("get_connection_types");
      dispatch({ type: "SET_CONNECTION_TYPES", types });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `${e}` });
    }
  }, []);

  const refreshEndpoints = useCallback(async () => {
    try {
      const list = await invoke<EndpointInfo[]>("enumerate_endpoints");
      dispatch({ type: "SET_ENDPOINTS", endpoints: list });
      dispatch({ type: "SET_ERROR", error: null });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `${e}` });
    }
  }, []);

  const connect = useCallback(async (endpoint: string, params: Record<string, unknown>, name?: string) => {
    dispatch({ type: "SET_ERROR", error: null });
    try {
      const sessionId = await invoke<string>("connect_session", { endpoint, params, name });
      return sessionId;
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `连接失败: ${e}` });
      return null;
    }
  }, []);

  const disconnect = useCallback(async (sessionId: string) => {
    // 已断开的会话保留在侧栏中，不做任何操作
    const tab = state.tabs.find(t => t.id === sessionId);
    if (tab?.state === "disconnected") {
      return;
    }
    try {
      await invoke("disconnect_session", { sessionId });
      // 后端会发出 session-disconnected 事件，handler 会将状态设为 "disconnected"
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `断开失败: ${e}` });
    }
  }, [state.tabs]);

  const deleteSession = useCallback(async (sessionId: string) => {
    const tab = state.tabs.find(t => t.id === sessionId);
    // 如果会话已连接，先断开后端连接
    if (tab?.state === "connected" || tab?.state === "connecting") {
      try {
        await invoke("disconnect_session", { sessionId });
      } catch (_e) {
        // 后端断开可能失败，仍继续从前端移除
      }
    }
    dispatch({ type: "REMOVE_TAB", id: sessionId });
  }, [state.tabs]);

  const sendData = useCallback(async (sessionId: string, data: string | Uint8Array) => {
    try {
      const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
      await invoke("write_data", { sessionId, data: Array.from(bytes) });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `发送失败: ${e}` });
    }
  }, []);

  const switchTab = useCallback(async (sessionId: string) => {
    dispatch({ type: "SET_ACTIVE", id: sessionId });
    try {
      await invoke("switch_active_session", { sessionId });
    } catch (_e) {
      // 恢复的会话在后端不存在，静默忽略
    }
  }, []);

  const renameTab = useCallback(async (sessionId: string, name: string) => {
    dispatch({ type: "RENAME_TAB", id: sessionId, name });
    try {
      await invoke("rename_session", { sessionId, newName: name });
    } catch (_e) {
      // 恢复的标签页在后端不存在，静默忽略
    }
  }, []);

  const getTabs = useCallback(async () => {
    try {
      const tabs = await invoke<TabInfo[]>("get_tabs");
      dispatch({ type: "SET_TABS", tabs });
    } catch (e) {
      // tabs may not exist yet, ignore
    }
  }, []);

  const clearError = useCallback(() => dispatch({ type: "SET_ERROR", error: null }), []);

  const loadSavedSessions = useCallback(async () => {
    try {
      const saved = await invoke<Array<{
        id: string;
        name: string;
        connection_type: string;
        endpoint: string;
        params: Record<string, unknown>;
        timestamp: number;
      }>>("load_sessions");
      if (saved && saved.length > 0) {
        const tabs: TabInfo[] = saved.map((s) => ({
          id: s.id,
          name: s.name,
          connection_type: s.connection_type,
          endpoint: s.endpoint,
          state: "disconnected" as ConnectionStatus,
          params: s.params,
        }));
        dispatch({ type: "SET_TABS", tabs });
        if (tabs.length > 0) {
          dispatch({ type: "SET_ACTIVE", id: tabs[0].id });
        }
      }
    } catch (e) {
      // No saved sessions or file doesn't exist — normal for first launch
    }
  }, []);

  const onSessionData = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => {
    dataCallbackRef.current = callback;
  }, []);

  const onSessionDisconnect = useCallback((callback: (sessionId: string, reason?: string) => void) => {
    disconnectCallbackRef.current = callback;
  }, []);

  // ── Event Listeners ──────────────────────────────

  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    (async () => {
      const u1 = await listen<{ session_id: string; data: number[] }>("session-data", (event) => {
        const data = new Uint8Array(event.payload.data);
        dataCallbackRef.current?.(event.payload.session_id, data);
      });
      if (cancelled) { u1(); return; }
      unlisteners.push(u1);

      const u2 = await listen<{ session_id: string; endpoint: string; connection_type: string; name: string; params: Record<string, unknown> }>(
        "session-connected",
        (event) => {
          dispatch({
            type: "ADD_TAB",
            tab: {
              id: event.payload.session_id,
              name: event.payload.name || `Serial @ ${event.payload.endpoint}`,
              connection_type: event.payload.connection_type,
              endpoint: event.payload.endpoint,
              state: "connected",
              params: event.payload.params,
            },
          });
        }
      );
      if (cancelled) { u2(); return; }
      unlisteners.push(u2);

      const u3 = await listen<{ session_id: string; reason?: string }>("session-disconnected", (event) => {
        const reason = event.payload.reason;
        dispatch({ type: "SET_TAB_STATE", id: event.payload.session_id, state: "disconnected" });
        disconnectCallbackRef.current?.(event.payload.session_id, reason);
      });
      if (cancelled) { u3(); return; }
      unlisteners.push(u3);

      const u4 = await listen<{ session_id: string; success: boolean }>("session-transfer-complete", (event) => {
        // 传输完成后标记会话为 disconnected，但不删除标签页（方便用户重连）
        dispatch({ type: "SET_TAB_STATE", id: event.payload.session_id, state: "disconnected" });
      });
      if (cancelled) { u4(); return; }
      unlisteners.push(u4);

      const u5 = await listen<{ session_id: string }>("session-switched", (event) => {
        dispatch({ type: "SET_ACTIVE", id: event.payload.session_id });
      });
      if (cancelled) { u5(); return; }
      unlisteners.push(u5);

      const u6 = await listen<{ session_id: string; name: string }>("session-renamed", (event) => {
        dispatch({ type: "RENAME_TAB", id: event.payload.session_id, name: event.payload.name });
      });
      if (cancelled) { u6(); return; }
      unlisteners.push(u6);
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach(u => u());
    };
  }, []);

  // Init
  useEffect(() => {
    fetchConnectionTypes();
    refreshEndpoints();
    loadSavedSessions();
  }, [fetchConnectionTypes, refreshEndpoints, loadSavedSessions]);

  return (
    <SessionContext.Provider value={{
      state,
      fetchConnectionTypes,
      refreshEndpoints,
      connect,
      disconnect,
      deleteSession,
      sendData,
      switchTab,
      renameTab,
      getTabs,
      onSessionData,
      onSessionDisconnect,
      clearError,
    }}>
      {children}
    </SessionContext.Provider>
  );
}

export function useSession() {
  const ctx = useContext(SessionContext);
  if (!ctx) throw new Error("useSession must be used within SessionProvider");
  return ctx;
}
