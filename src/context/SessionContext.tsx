import { createContext, useContext, useReducer, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { pluginRegistry } from "../core/plugin-registry";
import { releaseSessionStore } from "../hooks/usePluginSessionStore";
import i18n from "../i18n";

// ── Types ───────────────────────────────────────────

export type ConnectionStatus = "disconnected" | "connecting" | "connected" | "transferring";

export interface DisconnectInfo {
  kind: "user_requested" | "remote_eof" | "io_error" | "device_removed" | "process_exited";
  reason: string;
  exit_code?: number | null;
  retain_terminal: boolean;
}

/** I/O 运行时统计 */
export interface SessionStats {
  txBytes: number;
  rxBytes: number;
  /** UDP 会话级累计 RX 报文数（无对端模型，报文计数语义明确） */
  rxPackets?: number;
  /** UDP 会话级累计 TX 报文数 */
  txPackets?: number;
}

export interface TabInfo {
  id: string;
  name: string;
  connection_type: string;
  endpoint: string;
  state: ConnectionStatus;
  /** 插件标识符 */
  pluginId: string;
  /** 连接参数（恢复会话时用于回填配置） */
  params?: Record<string, unknown>;
  /** I/O 实时统计 */
  stats: SessionStats;
  /** 连接建立时的时间戳 (Date.now()) */
  connectedAt: number | null;
  /** 异常断开时保留终端现场所需的结构化原因；仅驻留于当前进程内。 */
  disconnectInfo?: DisconnectInfo;
  /** 是否启用文件传输子系统；缺省值由插件 definition 决定。 */
  transferEnabled?: boolean;
  /** 文件传输协议 ID */
  transferProtocol?: string;
  /** 是否启用发送栏；缺省值由插件 manifest/definition 决定。 */
  sendBarEnabled?: boolean;
  /** 父会话 ID；非空表示通用子 channel。 */
  parentId?: string | null;
  /** 子 channel 在父会话中的自动编号（从 0 开始） */
  channelIndex?: number;
  /** 子 channel 是否以提升权限运行；仅对声明对应能力的插件有意义。 */
  elevated?: boolean;
  /** 根会话是否是可创建多个子终端的容器。 */
  isContainer?: boolean;
}

/** connect() 参数对象 */
export interface ConnectOptions {
  endpoint: string;
  params: Record<string, unknown>;
  name?: string;
  pluginId: string;
  transferEnabled?: boolean;
  transferProtocol?: string;
  sendBarEnabled?: boolean;
  initialElevated?: boolean;
  sessionId?: string;
}

export interface ConnectionTypeInfo {
  id: string;
  label: string;
  available: boolean;
  description: string;
  icon: string;
  content_type: string;
}

export interface EndpointInfo {
  name: string;
  description: string;
  connection_type: string;
  params?: Record<string, unknown>;
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
  | { type: "SET_ACTIVE"; id: string | null }
  | { type: "SET_CONNECTION_TYPES"; types: ConnectionTypeInfo[] }
  | { type: "SET_ENDPOINTS"; endpoints: EndpointInfo[] }
  | { type: "REPLACE_ENDPOINTS_FOR_PLUGIN"; pluginId: string; endpoints: EndpointInfo[] }
  | { type: "SET_ERROR"; error: string | null }
  | { type: "SET_TAB_STATE"; id: string; state: ConnectionStatus }
  | { type: "SET_TAB_DISCONNECTED"; id: string; info?: DisconnectInfo }
  | { type: "UPDATE_TAB_STATS"; id: string; stats: SessionStats; connectedAt?: number | null }
  | { type: "UPDATE_TAB_CONFIG"; id: string; endpoint: string; params: Record<string, unknown>; name: string; transferEnabled?: boolean; transferProtocol?: string; sendBarEnabled?: boolean; pluginId?: string; connectedAt?: number | null }
  | { type: "CLEAR_TABS" }
  | { type: "REMOVE_CHILD"; id: string; parentId: string }
  | { type: "REMOVE_ALL_CHILDREN"; parentId: string };

function decodeBase64(b64: string): Uint8Array {
  const binary = atob(b64);
  const len = binary.length;
  const bytes = new Uint8Array(len);
  for (let i = 0; i < len; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

const initialState: SessionState = {
  tabs: [],
  activeTabId: null,
  connectionTypes: [],
  endpoints: [],
  error: null,
};

function sessionReducer(state: SessionState, action: SessionAction): SessionState {
  switch (action.type) {
    case "SET_TABS": return { ...state, tabs: action.tabs };
    case "ADD_TAB":
      return {
        ...state,
        tabs: [...state.tabs, action.tab],
        activeTabId: action.tab.isContainer && state.tabs.some(tab => tab.parentId === action.tab.id)
          ? state.activeTabId
          : action.tab.id,
      };
    case "REMOVE_TAB": {
      const childIds = state.tabs.filter(t => t.parentId === action.id).map(t => t.id);
      const allRemoved = new Set([action.id, ...childIds]);
      const remaining = state.tabs.filter(t => !allRemoved.has(t.id));
      let nextActive = state.activeTabId;
      if (nextActive && allRemoved.has(nextActive)) nextActive = remaining.find(t => !t.parentId)?.id ?? null;
      return { ...state, tabs: remaining, activeTabId: nextActive };
    }
    case "RENAME_TAB": return { ...state, tabs: state.tabs.map(t => t.id === action.id ? { ...t, name: action.name } : t) };
    case "REORDER_TABS":
      return { ...state, tabs: action.ids.map(id => state.tabs.find(t => t.id === id)).filter((t): t is TabInfo => t !== undefined) };
    case "SET_ACTIVE": return { ...state, activeTabId: action.id };
    case "SET_CONNECTION_TYPES": return { ...state, connectionTypes: action.types };
    case "SET_ENDPOINTS": return { ...state, endpoints: action.endpoints };
    case "REPLACE_ENDPOINTS_FOR_PLUGIN":
      return { ...state, endpoints: [...state.endpoints.filter(endpoint => endpoint.connection_type !== action.pluginId), ...action.endpoints] };
    case "SET_ERROR": return { ...state, error: action.error };
    case "SET_TAB_STATE":
      return { ...state, tabs: state.tabs.map(t => t.id === action.id ? { ...t, state: action.state, disconnectInfo: undefined } : t) };
    case "SET_TAB_DISCONNECTED":
      return { ...state, tabs: state.tabs.map(t => t.id === action.id ? { ...t, state: "disconnected", disconnectInfo: action.info } : t) };
    case "UPDATE_TAB_STATS":
      return { ...state, tabs: state.tabs.map(t => t.id === action.id ? { ...t, stats: action.stats, connectedAt: action.connectedAt ?? t.connectedAt } : t) };
    case "UPDATE_TAB_CONFIG":
      return {
        ...state,
        tabs: state.tabs.map(t => t.id === action.id ? {
          ...t,
          name: action.name,
          endpoint: action.endpoint,
          params: action.params,
          transferEnabled: action.transferEnabled ?? t.transferEnabled,
          transferProtocol: action.transferProtocol ?? t.transferProtocol,
          sendBarEnabled: action.sendBarEnabled ?? t.sendBarEnabled,
          pluginId: action.pluginId ?? t.pluginId,
          connectedAt: action.connectedAt !== undefined ? action.connectedAt : t.connectedAt,
        } : t),
      };
    case "REMOVE_CHILD": {
      const remaining = state.tabs.filter(t => t.id !== action.id);
      let nextActive = state.activeTabId;
      if (nextActive === action.id) {
        const siblings = remaining.filter(t => t.parentId === action.parentId);
        nextActive = siblings[0]?.id ?? action.parentId ?? remaining.find(t => !t.parentId)?.id ?? null;
      }
      const hasOtherChildren = remaining.some(t => t.parentId === action.parentId);
      if (!hasOtherChildren) {
        const parentTab = remaining.find(t => t.id === action.parentId);
        if (parentTab) {
          return {
            ...state,
            tabs: remaining.map(t => t.id === action.parentId ? { ...t, state: "disconnected" as ConnectionStatus } : t),
            activeTabId: nextActive,
          };
        }
      }
      return { ...state, tabs: remaining, activeTabId: nextActive };
    }
    case "REMOVE_ALL_CHILDREN": {
      const childIds = new Set(state.tabs.filter(t => t.parentId === action.parentId).map(t => t.id));
      return {
        ...state,
        tabs: state.tabs.filter(t => t.parentId !== action.parentId),
        activeTabId: state.activeTabId && childIds.has(state.activeTabId) ? action.parentId : state.activeTabId,
      };
    }
    case "CLEAR_TABS": return { ...state, tabs: [], activeTabId: null };
    default: return state;
  }
}

interface SessionContextValue {
  state: SessionState;
  fetchConnectionTypes: () => Promise<void>;
  refreshEndpoints: (pluginId?: string, force?: boolean) => Promise<void>;
  connect: (opts: ConnectOptions) => Promise<string | null>;
  reconnectSession: (sessionId: string, initialElevated?: boolean) => Promise<string | null>;
  createOfflineSession: (endpoint: string, params: Record<string, unknown>, name: string | undefined, pluginId: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean) => Promise<string | null>;
  disconnect: (sessionId: string) => Promise<void>;
  deleteSession: (sessionId: string, skipDisconnect?: boolean) => Promise<void>;
  sendData: (sessionId: string, data: string | Uint8Array) => Promise<void>;
  switchTab: (sessionId: string | null) => Promise<void>;
  renameTab: (sessionId: string, name: string) => Promise<void>;
  reconfigureSession: (sessionId: string, endpoint: string, params: Record<string, unknown>, name?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean, pluginId?: string) => Promise<void>;
  openChannel: (parentSessionId: string, elevated?: boolean) => Promise<string | null>;
  closeChannel: (channelId: string, parentId: string) => Promise<void>;
  onSessionData: (callback: (sessionId: string, data: Uint8Array) => void) => void;
  onDataSent: (callback: (sessionId: string, data: Uint8Array) => void) => void;
  subscribeDataSent: (callback: (sessionId: string, data: Uint8Array) => void) => () => void;
  isSessionConnected: (sessionId: string) => boolean;
  sendToTarget: (containerId: string, data: string | Uint8Array) => Promise<void>;
  updateSessionStats: (sessionId: string, txBytes: number, rxBytes: number, rxPackets?: number, txPackets?: number) => void;
  onSessionDisconnect: (callback: (sessionId: string, reason?: string) => void) => void;
  clearError: () => void;
  startSessionLog: (sessionId: string) => Promise<string>;
  stopSessionLog: (sessionId: string) => Promise<void>;
  loggingSessions: Set<string>;
  logStatuses: Map<string, { fileName: string; bytesWritten: number }>;
}

const SessionContext = createContext<SessionContextValue | null>(null);

export function SessionProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(sessionReducer, initialState);
  const dataCallbackRef = useRef<((sessionId: string, data: Uint8Array) => void) | null>(null);
  const sentDataCallbackRef = useRef<((sessionId: string, data: Uint8Array) => void) | null>(null);
  const sentDataSubscribersRef = useRef<Set<(sessionId: string, data: Uint8Array) => void>>(new Set());
  const disconnectCallbackRef = useRef<((sessionId: string, reason?: string) => void) | null>(null);
  const tabsRef = useRef(state.tabs);
  tabsRef.current = state.tabs;
  const lastActiveChildRef = useRef<Map<string, string>>(new Map());
  const endpointRefreshesRef = useRef<Map<string, Promise<EndpointInfo[]>>>(new Map());
  const endpointRefreshCompletedAtRef = useRef<Map<string, number>>(new Map());

  useEffect(() => {
    const activeId = state.activeTabId;
    if (!activeId) return;
    const activeTab = state.tabs.find(tab => tab.id === activeId);
    if (activeTab?.parentId) lastActiveChildRef.current.set(activeTab.parentId, activeTab.id);
  }, [state.activeTabId, state.tabs]);

  const [loggingSessions, setLoggingSessions] = useState<Set<string>>(new Set());
  const [logStatuses, setLogStatuses] = useState<Map<string, { fileName: string; bytesWritten: number }>>(new Map());

  const startSessionLog = useCallback(async (sessionId: string): Promise<string> => {
    try {
      await invoke<string>("start_session_log", { sessionId });
      setLoggingSessions(prev => new Set(prev).add(sessionId));
      const statuses: Array<{ session_id: string; file_name: string; bytes_written: number }> = await invoke("get_log_status");
      setLogStatuses(new Map(statuses.map(s => [s.session_id, { fileName: s.file_name, bytesWritten: s.bytes_written }])));
      return sessionId;
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `启动日志失败: ${e}` });
      throw e;
    }
  }, []);

  const stopSessionLog = useCallback(async (sessionId: string) => {
    try {
      await invoke("stop_session_log", { sessionId });
      setLoggingSessions(prev => {
        const next = new Set(prev);
        next.delete(sessionId);
        return next;
      });
      setLogStatuses(prev => {
        const next = new Map(prev);
        next.delete(sessionId);
        return next;
      });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `停止日志失败: ${e}` });
    }
  }, []);

  const fetchConnectionTypes = useCallback(async () => {
    try {
      const types = await invoke<ConnectionTypeInfo[]>("get_connection_types");
      dispatch({ type: "SET_CONNECTION_TYPES", types });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `${e}` });
    }
  }, []);

  const refreshEndpoints = useCallback(async (pluginId?: string, force = false) => {
    const discoverableIds = pluginRegistry.getByCapability("endpoint_discovery").map(plugin => plugin.manifest.id);
    const pluginIds = pluginId ? (discoverableIds.includes(pluginId) ? [pluginId] : []) : discoverableIds;
    await Promise.all(pluginIds.map(async currentPluginId => {
      const lastCompleted = endpointRefreshCompletedAtRef.current.get(currentPluginId) ?? 0;
      if (!force && Date.now() - lastCompleted < 5000) return;
      let pending = endpointRefreshesRef.current.get(currentPluginId);
      if (!pending) {
        pending = invoke<EndpointInfo[]>("enumerate_endpoints", { pluginId: currentPluginId });
        endpointRefreshesRef.current.set(currentPluginId, pending);
      }
      try {
        const endpoints = await pending;
        endpointRefreshCompletedAtRef.current.set(currentPluginId, Date.now());
        dispatch({ type: "REPLACE_ENDPOINTS_FOR_PLUGIN", pluginId: currentPluginId, endpoints });
      } catch (e) {
        console.warn(`Endpoint discovery failed for ${currentPluginId}:`, e);
      } finally {
        if (endpointRefreshesRef.current.get(currentPluginId) === pending) endpointRefreshesRef.current.delete(currentPluginId);
      }
    }));
  }, []);

  const connect = useCallback(async (opts: ConnectOptions) => {
    const { endpoint, params, name, pluginId, transferEnabled, transferProtocol, sendBarEnabled, sessionId, initialElevated } = opts;
    const plugin = pluginRegistry.get(pluginId);
    const effectiveParams = plugin?.normalizeConnectionParams?.(params) ?? params;
    const sessionOptions = pluginRegistry.resolveSessionOptions(pluginId, {
      transferEnabled,
      transferProtocol,
      sendBarEnabled,
    });
    dispatch({ type: "SET_ERROR", error: null });
    if (sessionId) dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "connecting" });
    try {
      return await invoke<string>("connect_session", {
        request: {
          endpoint,
          params: effectiveParams,
          name,
          pluginId,
          transferEnabled: sessionOptions.transferEnabled,
          transferProtocol: sessionOptions.transferProtocol ?? null,
          sendBarEnabled: sessionOptions.sendBarEnabled,
          sessionId: sessionId || null,
          initialElevated: initialElevated ?? false,
        },
      });
    } catch (e) {
      const error = plugin?.formatSessionError?.(e, "connect") ?? String(e);
      dispatch({ type: "SET_ERROR", error });
      if (sessionId) dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" });
      return null;
    }
  }, []);

  const reconnectSession = useCallback(async (sessionId: string, initialElevated = false): Promise<string | null> => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    if (!tab || tab.state !== "disconnected" || !tab.params) return null;

    const params = tab.params as Record<string, unknown>;
    const guard = pluginRegistry.get(tab.pluginId)?.reconnectGuard;
    if (guard) {
      const result = await guard({ sessionId: tab.id, endpoint: tab.endpoint, params });
      if (!result.ok) {
        dispatch({ type: "SET_ERROR", error: result.message });
        return null;
      }
    }

    return connect({
      endpoint: tab.endpoint,
      params,
      name: tab.name,
      pluginId: tab.pluginId,
      transferEnabled: initialElevated ? false : tab.transferEnabled,
      transferProtocol: tab.transferProtocol,
      sendBarEnabled: tab.sendBarEnabled,
      sessionId: tab.id,
      initialElevated,
    });
  }, [connect]);

  const createOfflineSession = useCallback(async (
    endpoint: string,
    params: Record<string, unknown>,
    name: string | undefined,
    pluginId: string,
    transferEnabled?: boolean,
    transferProtocol?: string,
    sendBarEnabled?: boolean,
  ) => {
    dispatch({ type: "SET_ERROR", error: null });
    try {
      const plugin = pluginRegistry.get(pluginId);
      const normalizedParams = plugin?.normalizeConnectionParams?.(params) ?? params;
      const sessionOptions = pluginRegistry.resolveSessionOptions(pluginId, {
        transferEnabled,
        transferProtocol,
        sendBarEnabled,
      });
      const pluginName = plugin?.manifest.name || pluginId.toUpperCase();
      const requestedName = name?.trim();
      const presentationName = plugin?.sessionPresentation?.defaultName?.(normalizedParams, endpoint)?.trim();
      const resolvedName = plugin?.resolveDefaultSessionName
        ? (await plugin.resolveDefaultSessionName(normalizedParams, endpoint)).trim()
        : "";
      const effectiveName = requestedName || resolvedName || presentationName || `${pluginName} @ ${endpoint}`;
      const sessionId = await invoke<string>("save_session_config", {
        request: {
          endpoint,
          params: normalizedParams,
          name: effectiveName,
          pluginId,
          transferEnabled: sessionOptions.transferEnabled,
          transferProtocol: sessionOptions.transferProtocol ?? null,
          sendBarEnabled: sessionOptions.sendBarEnabled,
        },
      });
      const persistedParams = plugin?.persistedConnectionParams?.(normalizedParams, sessionId) ?? normalizedParams;
      dispatch({
        type: "ADD_TAB",
        tab: {
          id: sessionId,
          name: effectiveName,
          connection_type: pluginId,
          endpoint,
          state: "disconnected",
          pluginId,
          params: persistedParams,
          stats: { txBytes: 0, rxBytes: 0 },
          connectedAt: null,
          transferEnabled: sessionOptions.transferEnabled,
          transferProtocol: sessionOptions.transferProtocol,
          sendBarEnabled: sessionOptions.sendBarEnabled,
        },
      });
      return sessionId;
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `创建会话失败: ${e}` });
      return null;
    }
  }, []);

  const disconnect = useCallback(async (sessionId: string) => {
    const tab = state.tabs.find(t => t.id === sessionId);
    if (tab?.state === "disconnected") return;
    dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" });
    try {
      await invoke("disconnect_session", { sessionId });
    } catch (e) {
      dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "connected" });
      dispatch({ type: "SET_ERROR", error: `断开失败: ${e}` });
    }
  }, [state.tabs]);

  const deleteSession = useCallback(async (sessionId: string, skipDisconnect = false) => {
    const tab = state.tabs.find(t => t.id === sessionId);
    if (!skipDisconnect && (tab?.state === "connected" || tab?.state === "connecting" || tab?.state === "transferring")) {
      dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" });
      try {
        await invoke("disconnect_session", { sessionId });
      } catch (_e) {
        dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "connected" });
        dispatch({ type: "SET_ERROR", error: "Cannot delete active session — disconnect failed" });
        return;
      }
    }
    try {
      await invoke("delete_session_config", { sessionId });
    } catch (e) {
      dispatch({
        type: "SET_ERROR",
        error: i18n.t("session.deletePersistFailed", {
          defaultValue: "Failed to delete the saved session: {{error}}. The session remains in the list so you can retry.",
          error: String(e),
        }),
      });
      return;
    }
    dispatch({ type: "REMOVE_TAB", id: sessionId });
    tab && pluginRegistry.get(tab.pluginId)?.runtimeStore?.release?.(sessionId);
    releaseSessionStore(sessionId);
  }, [state.tabs]);

  const writeData = useCallback(async (sessionId: string, data: string | Uint8Array) => {
    const isText = typeof data === "string";
    const bytes = isText ? new TextEncoder().encode(data) : data;
    const written = await invoke<number[]>("write_data", { sessionId, data: Array.from(bytes), transcode: isText });
    const writtenBytes = new Uint8Array(written);
    sentDataCallbackRef.current?.(sessionId, writtenBytes);
    sentDataSubscribersRef.current.forEach(callback => callback(sessionId, writtenBytes));
  }, []);

  const sendData = useCallback(async (sessionId: string, data: string | Uint8Array) => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    if (!tab || tab.state === "disconnected") return;
    try {
      await writeData(sessionId, data);
    } catch (error) {
      dispatch({ type: "SET_ERROR", error: `发送失败: ${error}` });
      throw error;
    }
  }, [writeData]);

  const switchTab = useCallback(async (sessionId: string | null) => {
    if (sessionId === null) {
      dispatch({ type: "SET_ACTIVE", id: null });
      return;
    }
    const tabs = tabsRef.current;
    const targetTab = tabs.find(tab => tab.id === sessionId);
    let resolvedSessionId = sessionId;
    if (targetTab?.parentId) {
      lastActiveChildRef.current.set(targetTab.parentId, targetTab.id);
    } else if (targetTab) {
      const supportsMultiple = pluginRegistry.get(targetTab.pluginId)?.manifest.capabilities.includes("multi_session") ?? false;
      if (supportsMultiple) {
        const children = tabs.filter(tab => tab.parentId === targetTab.id);
        if (children.length > 0) {
          const rememberedId = lastActiveChildRef.current.get(targetTab.id);
          const remembered = rememberedId ? children.find(child => child.id === rememberedId) : undefined;
          resolvedSessionId = remembered?.id ?? children[0].id;
          lastActiveChildRef.current.set(targetTab.id, resolvedSessionId);
        }
      }
    }
    dispatch({ type: "SET_ACTIVE", id: resolvedSessionId });
    try {
      await invoke("switch_active_session", { sessionId: resolvedSessionId });
    } catch (_e) {
      // Restored/offline Session may not exist in the backend runtime yet.
    }
  }, []);

  const renameTab = useCallback(async (sessionId: string, name: string) => {
    const previousName = tabsRef.current.find(tab => tab.id === sessionId)?.name;
    dispatch({ type: "RENAME_TAB", id: sessionId, name });
    try {
      await invoke("rename_session", { sessionId, newName: name });
    } catch (e) {
      if (previousName !== undefined) dispatch({ type: "RENAME_TAB", id: sessionId, name: previousName });
      dispatch({
        type: "SET_ERROR",
        error: i18n.t("session.renamePersistFailed", {
          defaultValue: "Failed to rename the saved session: {{error}}. The previous name was restored.",
          error: String(e),
        }),
      });
    }
  }, []);

  const reconfigureSession = useCallback(async (
    sessionId: string,
    endpoint: string,
    params: Record<string, unknown>,
    name?: string,
    transferEnabled?: boolean,
    transferProtocol?: string,
    sendBarEnabled?: boolean,
    pluginId?: string,
  ) => {
    const tab = state.tabs.find(t => t.id === sessionId);
    const wasConnected = tab?.state === "connected" || tab?.state === "transferring";
    if (wasConnected) {
      dispatch({ type: "REMOVE_ALL_CHILDREN", parentId: sessionId });
      try {
        await invoke("disconnect_session", { sessionId });
        dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" });
      } catch (e) {
        dispatch({ type: "SET_ERROR", error: `断开失败: ${e}` });
        return;
      }
    }

    const effectivePluginId = pluginId || tab?.pluginId;
    if (!effectivePluginId) {
      dispatch({ type: "SET_ERROR", error: "无法确定会话的协议类型 (pluginId)" });
      return;
    }
    const effectiveName = name?.trim() || tab?.name;
    if (!effectiveName) {
      dispatch({ type: "SET_ERROR", error: "无法确定会话名称" });
      return;
    }
    const plugin = pluginRegistry.get(effectivePluginId);
    const normalizedParams = plugin?.normalizeConnectionParams?.(params) ?? params;
    const sessionOptions = pluginRegistry.resolveSessionOptions(effectivePluginId, {
      transferEnabled: transferEnabled ?? tab?.transferEnabled,
      transferProtocol: transferProtocol ?? tab?.transferProtocol,
      sendBarEnabled: sendBarEnabled ?? tab?.sendBarEnabled,
    });
    try {
      await invoke("save_session_config", {
        request: {
          endpoint,
          params: normalizedParams,
          name: effectiveName,
          pluginId: effectivePluginId,
          transferEnabled: sessionOptions.transferEnabled,
          transferProtocol: sessionOptions.transferProtocol ?? null,
          sendBarEnabled: sessionOptions.sendBarEnabled,
          sessionId,
        },
      });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `保存配置失败: ${e}` });
      return;
    }

    const persistedParams = plugin?.persistedConnectionParams?.(normalizedParams, sessionId) ?? normalizedParams;
    dispatch({
      type: "UPDATE_TAB_CONFIG",
      id: sessionId,
      endpoint,
      params: persistedParams,
      name: effectiveName,
      transferEnabled: sessionOptions.transferEnabled,
      transferProtocol: sessionOptions.transferProtocol,
      sendBarEnabled: sessionOptions.sendBarEnabled,
      pluginId: effectivePluginId,
    });

    if (wasConnected) {
      try {
        const newSessionId = await invoke<string>("connect_session", {
          request: {
            endpoint,
            params: persistedParams,
            name: effectiveName,
            pluginId: effectivePluginId,
            transferEnabled: sessionOptions.transferEnabled,
            transferProtocol: sessionOptions.transferProtocol ?? null,
            sendBarEnabled: sessionOptions.sendBarEnabled,
            sessionId,
          },
        });
        dispatch({ type: "SET_TAB_STATE", id: newSessionId, state: "connected" });
      } catch (e) {
        dispatch({ type: "SET_ERROR", error: `重连失败: ${e}` });
      }
    }
  }, [state.tabs]);

  const openChannel = useCallback(async (parentSessionId: string, elevated = false): Promise<string | null> => {
    try {
      return await invoke<string>("open_channel", { sessionId: parentSessionId, elevated });
    } catch (e) {
      const parent = tabsRef.current.find(tab => tab.id === parentSessionId);
      const plugin = parent ? pluginRegistry.get(parent.pluginId) : undefined;
      const error = plugin?.formatSessionError?.(e, "open_channel") ?? String(e);
      dispatch({ type: "SET_ERROR", error });
      return null;
    }
  }, []);

  const closeChannel = useCallback(async (channelId: string, parentId: string) => {
    const resetCounter = !tabsRef.current.some(tab => tab.parentId === parentId && tab.id !== channelId);
    try {
      await invoke("close_channel", { sessionId: channelId, parentId, resetCounter });
      dispatch({ type: "REMOVE_CHILD", id: channelId, parentId });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `关闭终端失败: ${e}` });
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
        plugin_id: string;
        transfer_enabled?: boolean;
        transfer_protocol?: string;
        send_bar_enabled?: boolean;
      }>>("load_sessions");
      if (!saved?.length) return;
      const tabs: TabInfo[] = saved.map(s => {
        const pluginId = s.plugin_id;
        const params = pluginRegistry.get(pluginId)?.normalizeConnectionParams?.(s.params) ?? s.params;
        const sessionOptions = pluginRegistry.resolveSessionOptions(pluginId, {
          transferEnabled: s.transfer_enabled,
          transferProtocol: s.transfer_protocol,
          sendBarEnabled: s.send_bar_enabled,
        });
        return {
          id: s.id,
          name: s.name,
          connection_type: s.connection_type,
          endpoint: s.endpoint,
          state: "disconnected" as ConnectionStatus,
          pluginId,
          params,
          stats: { txBytes: 0, rxBytes: 0 },
          connectedAt: null,
          transferEnabled: sessionOptions.transferEnabled,
          transferProtocol: sessionOptions.transferProtocol,
          sendBarEnabled: sessionOptions.sendBarEnabled,
        };
      });
      dispatch({ type: "SET_TABS", tabs });
      dispatch({ type: "SET_ACTIVE", id: tabs[0].id });
    } catch (e) {
      dispatch({
        type: "SET_ERROR",
        error: i18n.t("session.loadPersistFailed", {
          defaultValue: "Failed to load saved sessions: {{error}}. The on-disk library was left untouched.",
          error: String(e),
        }),
      });
    }
  }, []);

  const onSessionData = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => { dataCallbackRef.current = callback; }, []);
  const onDataSent = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => { sentDataCallbackRef.current = callback; }, []);
  const subscribeDataSent = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => {
    sentDataSubscribersRef.current.add(callback);
    return () => { sentDataSubscribersRef.current.delete(callback); };
  }, []);

  const isSessionConnected = useCallback((sessionId: string): boolean => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    return tab?.state === "connected" || tab?.state === "transferring";
  }, []);

  const sendToTarget = useCallback(async (sessionId: string, data: string | Uint8Array) => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    if (!tab || tab.state === "disconnected") return;
    const plugin = pluginRegistry.get(tab.pluginId);
    try {
      if (plugin?.sendData) {
        await plugin.sendData({
          sessionId,
          params: tab.params ?? {},
          data,
          sendDefault: writeData,
        });
      } else {
        await writeData(sessionId, data);
      }
    } catch (error) {
      dispatch({ type: "SET_ERROR", error: `发送失败: ${error}` });
      throw error;
    }
  }, [writeData]);

  const updateSessionStats = useCallback((sessionId: string, txBytes: number, rxBytes: number, rxPackets?: number, txPackets?: number) => {
    dispatch({
      type: "UPDATE_TAB_STATS",
      id: sessionId,
      stats: { txBytes, rxBytes, ...(rxPackets !== undefined ? { rxPackets } : {}), ...(txPackets !== undefined ? { txPackets } : {}) },
      connectedAt: undefined,
    });
  }, []);
  const onSessionDisconnect = useCallback((callback: (sessionId: string, reason?: string) => void) => { disconnectCallbackRef.current = callback; }, []);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];
    (async () => {
      const u1 = await listen<{ session_id: string; data_b64?: string; data?: number[] }>("session-data", event => {
        const data = event.payload.data_b64 ? decodeBase64(event.payload.data_b64) : new Uint8Array(event.payload.data ?? []);
        dataCallbackRef.current?.(event.payload.session_id, data);
      });
      if (cancelled) { u1(); return; }
      unlisteners.push(u1);

      const u2 = await listen<{
        session_id: string;
        endpoint: string;
        connection_type: string;
        plugin_id: string;
        name: string;
        params: Record<string, unknown>;
        connected_at?: number | null;
        transfer_enabled?: boolean;
        transfer_protocol?: string;
        send_bar_enabled?: boolean;
        parent_id?: string | null;
        channel_index?: number;
        elevated?: boolean;
        is_container?: boolean;
      }>("session-connected", event => {
        const sid = event.payload.session_id;
        const eventPluginId = event.payload.plugin_id;
        const eventParams = pluginRegistry.get(eventPluginId)?.normalizeConnectionParams?.(event.payload.params) ?? event.payload.params;
        const parentId = event.payload.parent_id ?? null;
        const existingTab = tabsRef.current.find(tab => tab.id === sid);
        const eventOptions = pluginRegistry.resolveSessionOptions(eventPluginId, {
          transferEnabled: event.payload.transfer_enabled ?? existingTab?.transferEnabled,
          transferProtocol: event.payload.transfer_protocol ?? existingTab?.transferProtocol,
          sendBarEnabled: event.payload.send_bar_enabled ?? existingTab?.sendBarEnabled,
        });
        if (existingTab) {
          dispatch({ type: "SET_TAB_STATE", id: sid, state: "connected" });
          dispatch({
            type: "UPDATE_TAB_CONFIG",
            id: sid,
            endpoint: event.payload.endpoint,
            params: eventParams,
            name: existingTab.name,
            transferEnabled: eventOptions.transferEnabled,
            transferProtocol: eventOptions.transferProtocol,
            sendBarEnabled: eventOptions.sendBarEnabled,
            pluginId: eventPluginId,
            connectedAt: event.payload.connected_at ?? Date.now(),
          });
        } else if (parentId) {
          dispatch({ type: "SET_TAB_STATE", id: parentId, state: "connected" });
          dispatch({
            type: "ADD_TAB",
            tab: {
              id: sid,
              name: event.payload.name,
              connection_type: event.payload.connection_type,
              endpoint: event.payload.endpoint,
              state: "connected",
              pluginId: eventPluginId,
              params: eventParams,
              stats: { txBytes: 0, rxBytes: 0 },
              connectedAt: event.payload.connected_at ?? Date.now(),
              transferEnabled: event.payload.transfer_enabled ?? false,
              transferProtocol: event.payload.transfer_protocol,
              sendBarEnabled: eventOptions.sendBarEnabled,
              parentId,
              channelIndex: event.payload.channel_index,
              elevated: event.payload.elevated ?? false,
            },
          });
        } else {
          dispatch({
            type: "ADD_TAB",
            tab: {
              id: sid,
              name: event.payload.name || `${pluginRegistry.get(eventPluginId)?.manifest.name || eventPluginId.toUpperCase()} @ ${event.payload.endpoint}`,
              connection_type: event.payload.connection_type,
              endpoint: event.payload.endpoint,
              state: "connected",
              pluginId: eventPluginId,
              params: eventParams,
              stats: { txBytes: 0, rxBytes: 0 },
              connectedAt: event.payload.connected_at ?? Date.now(),
              transferEnabled: eventOptions.transferEnabled,
              transferProtocol: eventOptions.transferProtocol,
              sendBarEnabled: eventOptions.sendBarEnabled,
              isContainer: event.payload.is_container ?? false,
            },
          });
        }
      });
      if (cancelled) { u2(); return; }
      unlisteners.push(u2);

      const u2e = await listen<{ channel_id: string; parent_id: string; disconnect_info?: DisconnectInfo }>("channel-closed", event => {
        if (event.payload.disconnect_info?.retain_terminal) dispatch({ type: "SET_TAB_DISCONNECTED", id: event.payload.channel_id, info: event.payload.disconnect_info });
        else dispatch({ type: "REMOVE_CHILD", id: event.payload.channel_id, parentId: event.payload.parent_id });
      });
      if (cancelled) { u2e(); return; }
      unlisteners.push(u2e);

      const u3 = await listen<{ session_id: string; reason?: string; disconnect_info?: DisconnectInfo }>("session-disconnected", event => {
        const sid = event.payload.session_id;
        dispatch({ type: "SET_TAB_DISCONNECTED", id: sid, info: event.payload.disconnect_info });
        if (!event.payload.disconnect_info?.retain_terminal) dispatch({ type: "REMOVE_ALL_CHILDREN", parentId: sid });
        disconnectCallbackRef.current?.(sid, event.payload.reason);
        setLoggingSessions(prev => {
          if (!prev.has(sid)) return prev;
          const next = new Set(prev);
          next.delete(sid);
          invoke("stop_session_log", { sessionId: sid }).catch(() => {});
          return next;
        });
        setLogStatuses(prev => {
          const next = new Map(prev);
          next.delete(sid);
          return next;
        });
      });
      if (cancelled) { u3(); return; }
      unlisteners.push(u3);

      const u4 = await listen<{ session_id: string }>("file-transfer:started", event => {
        dispatch({ type: "SET_TAB_STATE", id: event.payload.session_id, state: "transferring" });
      });
      if (cancelled) { u4(); return; }
      unlisteners.push(u4);

      const u5 = await listen<{ session_id: string; success: boolean }>("file-transfer:finished", event => {
        dispatch({ type: "SET_TAB_STATE", id: event.payload.session_id, state: "connected" });
      });
      if (cancelled) { u5(); return; }
      unlisteners.push(u5);

      const u7 = await listen<{ session_id: string }>("session-switched", event => {
        dispatch({ type: "SET_ACTIVE", id: event.payload.session_id });
      });
      if (cancelled) { u7(); return; }
      unlisteners.push(u7);

      const u8 = await listen<{ session_id: string; name: string }>("session-renamed", event => {
        dispatch({ type: "RENAME_TAB", id: event.payload.session_id, name: event.payload.name });
      });
      if (cancelled) { u8(); return; }
      unlisteners.push(u8);

      const u9 = await listen<{ tab_id: string; tx_bytes: number; rx_bytes: number; connected_at?: number | null }>("session-stats", event => {
        dispatch({ type: "UPDATE_TAB_STATS", id: event.payload.tab_id, stats: { txBytes: event.payload.tx_bytes, rxBytes: event.payload.rx_bytes }, connectedAt: event.payload.connected_at });
      });
      if (cancelled) { u9(); return; }
      unlisteners.push(u9);
    })().catch(e => console.error("SessionContext: 事件监听器注册失败:", e));

    return () => {
      cancelled = true;
      unlisteners.forEach(unlisten => unlisten());
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    void listen<{ session_id: string; dropped_chunks: number }>("session-display-overflow", event => {
      if (disposed) return;
      const tab = tabsRef.current.find(item => item.id === event.payload.session_id);
      dispatch({
        type: "SET_ERROR",
        error: i18n.t("session.displayOverflow", {
          defaultValue: "Display overflow in {{session}} — {{count}} chunks were dropped from the presentation path.",
          session: tab?.name ?? event.payload.session_id,
          count: event.payload.dropped_chunks,
        }),
      });
    }).then(fn => {
      if (disposed) fn();
      else unlisten = fn;
    }).catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const hasActiveLogs = loggingSessions.size > 0;
  useEffect(() => {
    if (!hasActiveLogs) return;
    const interval = setInterval(async () => {
      try {
        const statuses: Array<{ session_id: string; file_name: string; bytes_written: number }> = await invoke("get_log_status");
        setLogStatuses(new Map(statuses.map(s => [s.session_id, { fileName: s.file_name, bytesWritten: s.bytes_written }])));
        setLoggingSessions(new Set(statuses.map(s => s.session_id)));
      } catch (_e) {
        // best-effort status refresh
      }
    }, 5000);
    return () => clearInterval(interval);
  }, [hasActiveLogs]);

  useEffect(() => {
    fetchConnectionTypes();
    loadSavedSessions();
  }, [fetchConnectionTypes, loadSavedSessions]);

  return (
    <SessionContext.Provider value={{
      state,
      fetchConnectionTypes,
      refreshEndpoints,
      connect,
      reconnectSession,
      createOfflineSession,
      disconnect,
      deleteSession,
      sendData,
      switchTab,
      renameTab,
      reconfigureSession,
      openChannel,
      closeChannel,
      onSessionData,
      onDataSent,
      subscribeDataSent,
      isSessionConnected,
      sendToTarget,
      updateSessionStats,
      onSessionDisconnect,
      clearError,
      startSessionLog,
      stopSessionLog,
      loggingSessions,
      logStatuses,
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
