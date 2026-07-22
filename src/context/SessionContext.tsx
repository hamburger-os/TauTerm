import { createContext, useContext, useReducer, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { pluginRegistry } from "../core/plugin-registry";

// ── Types ───────────────────────────────────────────

export type ConnectionStatus = "disconnected" | "connecting" | "connected" | "transferring";

/** I/O 运行时统计 */
export interface SessionStats {
  txBytes: number;
  rxBytes: number;
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
  /** 是否启用文件传输子系统（默认 true） */
  transferEnabled?: boolean;
  /** 文件传输协议（ymodem / xmodem / zmodem） */
  transferProtocol?: string;
  /** 是否启用发送栏（默认 true） */
  sendBarEnabled?: boolean;
  /** 是否启用虚拟串口（默认 true） */
  virtualPortEnabled?: boolean;
  /** 虚拟端口对数量（默认 1） */
  virtualPortCount?: number;
  /** 虚拟端口对列表（连接成功时后端推送） */
  virtualPortPairs?: Array<{ port_a: string; port_b: string }>;
  /** 虚拟端口创建失败时的错误信息 */
  virtualPortError?: string;
  /** SSH 文件服务是否启用（默认 true） */
  fileServiceEnabled?: boolean;
  /** SSH 文件服务协议（"sftp"） */
  fileServiceProtocol?: string;
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
  | { type: "UPDATE_TAB_STATS"; id: string; stats: SessionStats; connectedAt?: number | null }
  | { type: "UPDATE_TAB_CONFIG"; id: string; endpoint: string; params: Record<string, unknown>; name: string; transferEnabled?: boolean; transferProtocol?: string; sendBarEnabled?: boolean; pluginId?: string; connectedAt?: number | null }
  | { type: "UPDATE_TAB_VPORTS"; id: string; pairs: Array<{ port_a: string; port_b: string }> }
  | { type: "SET_VPORT_ERROR"; id: string; error: string }
  | { type: "CLEAR_VPORT_ERROR"; id: string }
  | { type: "CLEAR_TABS" };

// ── Base64 解码（与后端 data_batcher::base64_encode 配对） ───────────────────

/**
 * 解码 Base64 字符串为 Uint8Array。
 *
 * 使用浏览器原生 atob() + 手动字节填充，比 JSON.parse(number[]) 快 5-10 倍。
 * 后端批处理器（DataBatcher）将 16ms 窗口内的多包数据合并后用 Base64 编码 emit，
 * 前端在此解码后送入 xterm.write。
 */
function decodeBase64(b64: string): Uint8Array {
  const binary = atob(b64);
  const len = binary.length;
  const bytes = new Uint8Array(len);
  for (let i = 0; i < len; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
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
    case "SET_TABS":
      return { ...state, tabs: action.tabs };
    case "ADD_TAB": {
      // 始终追加新标签页（即使是同一端口），用户可通过右键菜单删除旧标签页
      return {
        ...state,
        tabs: [...state.tabs, action.tab],
        activeTabId: action.tab.id,
      };
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
        tabs: action.ids
          .map(id => state.tabs.find(t => t.id === id))
          .filter((t): t is TabInfo => t !== undefined),
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
    case "UPDATE_TAB_STATS":
      return {
        ...state,
        tabs: state.tabs.map(t =>
          t.id === action.id
            ? { ...t, stats: action.stats, connectedAt: action.connectedAt ?? t.connectedAt }
            : t
        ),
      };
    case "UPDATE_TAB_CONFIG":
      return {
        ...state,
        tabs: state.tabs.map(t =>
          t.id === action.id
            ? {
                ...t,
                name: action.name,
                endpoint: action.endpoint,
                params: action.params,
                transferEnabled: action.transferEnabled ?? t.transferEnabled,
                transferProtocol: action.transferProtocol ?? t.transferProtocol,
                sendBarEnabled: action.sendBarEnabled ?? t.sendBarEnabled,
                pluginId: action.pluginId ?? t.pluginId,
                connectedAt: action.connectedAt !== undefined ? action.connectedAt : t.connectedAt,
                virtualPortEnabled: (action.params?.virtual_port_enabled as boolean) ?? t.virtualPortEnabled,
                virtualPortCount: (action.params?.virtual_port_count as number) ?? t.virtualPortCount,
                fileServiceEnabled: (action.params?.file_service_enabled as boolean) ?? t.fileServiceEnabled,
                fileServiceProtocol: (action.params?.file_service_protocol as string) ?? t.fileServiceProtocol,
              }
            : t
        ),
      };
    case "UPDATE_TAB_VPORTS":
      return {
        ...state,
        tabs: state.tabs.map(tab =>
          tab.id === action.id
            ? { ...tab, virtualPortPairs: action.pairs }
            : tab
        ),
      };
    case "SET_VPORT_ERROR":
      return {
        ...state,
        tabs: state.tabs.map(tab =>
          tab.id === action.id
            ? { ...tab, virtualPortError: action.error, virtualPortPairs: undefined }
            : tab
        ),
      };
    case "CLEAR_VPORT_ERROR":
      return {
        ...state,
        tabs: state.tabs.map(tab =>
          tab.id === action.id
            ? { ...tab, virtualPortError: undefined }
            : tab
        ),
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
  connect: (endpoint: string, params: Record<string, unknown>, name?: string, pluginId?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean, sessionId?: string) => Promise<string | null>;
  createOfflineSession: (endpoint: string, params: Record<string, unknown>, name?: string, pluginId?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean) => Promise<string | null>;
  disconnect: (sessionId: string) => Promise<void>;
  deleteSession: (sessionId: string, skipDisconnect?: boolean) => Promise<void>;
  sendData: (sessionId: string, data: string | Uint8Array) => Promise<void>;
  switchTab: (sessionId: string) => Promise<void>;
  renameTab: (sessionId: string, name: string) => Promise<void>;
  reconfigureSession: (sessionId: string, endpoint: string, params: Record<string, unknown>, name?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean, pluginId?: string) => Promise<void>;
  getTabs: () => Promise<void>;
  onSessionData: (callback: (sessionId: string, data: Uint8Array) => void) => void;
  onDataSent: (callback: (sessionId: string, data: Uint8Array) => void) => void;
  onSessionDisconnect: (callback: (sessionId: string, reason?: string) => void) => void;
  clearError: () => void;
  /** 日志：启动会话数据日志记录 */
  startSessionLog: (sessionId: string) => Promise<string>;
  /** 日志：停止会话数据日志记录 */
  stopSessionLog: (sessionId: string) => Promise<void>;
  /** 日志：当前正在记录的会话 ID 集合 */
  loggingSessions: Set<string>;
  /** 日志：活跃日志状态 (sessionId → { fileName, bytesWritten }) */
  logStatuses: Map<string, { fileName: string; bytesWritten: number }>;
}

const SessionContext = createContext<SessionContextValue | null>(null);

export function SessionProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(sessionReducer, initialState);
  const dataCallbackRef = useRef<((sessionId: string, data: Uint8Array) => void) | null>(null);
  const sentDataCallbackRef = useRef<((sessionId: string, data: Uint8Array) => void) | null>(null);
  const disconnectCallbackRef = useRef<((sessionId: string, reason?: string) => void) | null>(null);
  // 保持最新的 tabs 引用，供事件监听器（闭包中 state 可能过期）使用
  const tabsRef = useRef(state.tabs);
  tabsRef.current = state.tabs;

  // ── Logging state ────────────────────────────────

  const [loggingSessions, setLoggingSessions] = useState<Set<string>>(new Set());
  const [logStatuses, setLogStatuses] = useState<Map<string, { fileName: string; bytesWritten: number }>>(new Map());

  const startSessionLog = useCallback(async (sessionId: string): Promise<string> => {
    try {
      await invoke<string>("start_session_log", { sessionId });
      setLoggingSessions(prev => new Set(prev).add(sessionId));
      // 立即查询状态获取文件名
      const statuses: Array<{ session_id: string; file_name: string; bytes_written: number }> =
        await invoke("get_log_status");
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

  const connect = useCallback(async (endpoint: string, params: Record<string, unknown>, name?: string, pluginId?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean, sessionId?: string) => {
    dispatch({ type: "SET_ERROR", error: null });
    // 如果已知 sessionId（已创建离线配置），立即将 tab 状态设为 connecting
    if (sessionId) {
      dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "connecting" });
    }
    try {
      // 不使用前端 Promise.race 超时 —— 后端已有 TCP connect_timeout(10s) +
      // SSH handshake timeout(10s) 等多层超时保护。前端超时会导致后端 invoke
      // 继续运行，连接成功后 emit session-connected 造成前后端状态不一致。
      const sid = await invoke<string>("connect_session", {
        endpoint, params, name,
        pluginId: pluginId || "serial",
        transferEnabled: transferEnabled ?? true,
        transferProtocol: transferProtocol || "ymodem",
        sendBarEnabled: sendBarEnabled ?? true,
        sessionId: sessionId || null,
      });
      return sid;
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `连接失败: ${e}` });
      // 连接失败时恢复为 disconnected 状态
      if (sessionId) {
        dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" });
      }
      return null;
    }
  }, []);

  const createOfflineSession = useCallback(async (endpoint: string, params: Record<string, unknown>, name?: string, pluginId?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean) => {
    dispatch({ type: "SET_ERROR", error: null });
    try {
      const pid = pluginId || "serial";
      // 协议无关的默认名：从 plugin-registry 查询 manifest.name，避免硬编码 "Serial @ ..."
      // 导致未来 telnet/tftp 等会话误显示为 "Serial"。回退为大写的 pluginId。
      const pluginName = (pluginRegistry.get(pid)?.manifest.name) || pid.toUpperCase();
      // Bug fix: 始终将计算后的 effectiveName 传给后端，避免前后端大小写不一致
      // 前端用 manifest.name ("SSH")，后端 fallback 用 pid ("ssh")，不传递会导致闪烁
      const effectiveName = name || `${pluginName} @ ${endpoint}`;
      const sessionId = await invoke<string>("save_session_config", {
        endpoint, params,
        name: effectiveName,
        pluginId: pid,
        transferEnabled: transferEnabled ?? true,
        transferProtocol: transferProtocol || "ymodem",
        sendBarEnabled: sendBarEnabled ?? true,
      });
      dispatch({
        type: "ADD_TAB",
        tab: {
          id: sessionId,
          name: effectiveName,
          connection_type: pid,
          endpoint,
          state: "disconnected",
          pluginId: pid,
          params,
          stats: { txBytes: 0, rxBytes: 0 },
          connectedAt: null,
          transferEnabled: transferEnabled ?? true,
          transferProtocol,
          sendBarEnabled: sendBarEnabled ?? true,
          virtualPortEnabled: (params.virtual_port_enabled as boolean) ?? false,
          virtualPortCount: (params.virtual_port_count as number) ?? 0,
          fileServiceEnabled: (params.file_service_enabled as boolean) ?? false,
          fileServiceProtocol: params.file_service_protocol as string | undefined,
        },
      });
      return sessionId;
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `创建会话失败: ${e}` });
      return null;
    }
  }, []);

  const disconnect = useCallback(async (sessionId: string) => {
    // 已断开的会话保留在侧栏中，不做任何操作
    const tab = state.tabs.find(t => t.id === sessionId);
    if (tab?.state === "disconnected") {
      return;
    }
    // 先更新前端状态为 disconnected，让 React 同步停止周期发送定时器，
    // 避免后端 close_session() 之后定时器还在触发 write_data 导致"会话不存在"错误
    dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" });
    try {
      await invoke("disconnect_session", { sessionId });
    } catch (e) {
      // 后端调用失败，恢复连接状态以便用户重试
      dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "connected" });
      dispatch({ type: "SET_ERROR", error: `断开失败: ${e}` });
    }
  }, [state.tabs]);

  const deleteSession = useCallback(async (sessionId: string, skipDisconnect = false) => {
    const tab = state.tabs.find(t => t.id === sessionId);
    // 如果会话已连接，先断开后端连接（除非调用方已提前断连）
    if (!skipDisconnect && (tab?.state === "connected" || tab?.state === "connecting" || tab?.state === "transferring")) {
      // 先更新前端状态，让 React 同步停止周期发送定时器
      dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" });
      try {
        await invoke("disconnect_session", { sessionId });
      } catch (_e) {
        // 断开失败，恢复连接状态并停止删除流程以避免后端资源泄漏
        dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "connected" });
        dispatch({ type: "SET_ERROR", error: "Cannot delete active session — disconnect failed" });
        return;
      }
    }
    // 从磁盘中删除会话配置（仅当会话已断开或从未连接时）
    try {
      await invoke("delete_session_config", { sessionId });
    } catch (_e) {
      // 删除失败不影响前端移除
    }
    dispatch({ type: "REMOVE_TAB", id: sessionId });
  }, [state.tabs]);

  const sendData = useCallback(async (sessionId: string, data: string | Uint8Array) => {
    try {
      const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
      await invoke("write_data", { sessionId, data: Array.from(bytes) });
      // 通知 Dual 模式终端：数据已发送
      sentDataCallbackRef.current?.(sessionId, bytes);
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

    // 1. 如果已连接，先断连
    if (wasConnected) {
      try {
        await invoke("disconnect_session", { sessionId });
        dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" });
      } catch (e) {
        dispatch({ type: "SET_ERROR", error: `断开失败: ${e}` });
        return;
      }
    }

    // 2. 更新磁盘配置（保持相同 UUID）
    // pluginId 优先生效：调用方传入 > tab 已记录的 > 报错（不应回退到默认值）
    const effectivePluginId = pluginId || tab?.pluginId;
    if (!effectivePluginId) {
      dispatch({ type: "SET_ERROR", error: "无法确定会话的协议类型 (pluginId)" });
      return;
    }
    try {
      await invoke("save_session_config", {
        endpoint,
        params,
        name: name || undefined,
        pluginId: effectivePluginId,
        transferEnabled: transferEnabled ?? true,
        transferProtocol: transferProtocol || "ymodem",
        sendBarEnabled: sendBarEnabled ?? true,
        sessionId, // 复用已有 UUID
      });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `保存配置失败: ${e}` });
      return;
    }

    // 3. 更新前端 tab 状态
    dispatch({
      type: "UPDATE_TAB_CONFIG",
      id: sessionId,
      endpoint,
      params,
      name: name || tab?.name || `${(tab?.pluginId && pluginRegistry.get(tab.pluginId)?.manifest.name) || tab?.pluginId?.toUpperCase() || "Serial"} @ ${endpoint}`,
      transferEnabled,
      transferProtocol,
      sendBarEnabled,
      pluginId: tab?.pluginId, // 保持原有 pluginId，为将来插件切换预留
    });

    // 4. 如果之前是连接状态，重新连接
    if (wasConnected) {
      try {
        const newSessionId = await invoke<string>("connect_session", {
          endpoint,
          params,
          name: name || tab?.name || undefined,
          pluginId: effectivePluginId,
          transferEnabled: transferEnabled ?? true,
          transferProtocol: transferProtocol || "ymodem",
          sendBarEnabled: sendBarEnabled ?? true,
          sessionId, // 保持 UUID 连续性
        });
        // connect_session 后端会 emit session-connected 事件，前端监听器会更新状态为 connected
        // 但我们也需要同步更新（事件可能异步到达）
        dispatch({ type: "SET_TAB_STATE", id: newSessionId, state: "connected" });
      } catch (e) {
        dispatch({ type: "SET_ERROR", error: `重连失败: ${e}` });
      }
    }
  }, [state.tabs]);

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
        plugin_id?: string;
        transfer_enabled?: boolean;
        transfer_protocol?: string;
        send_bar_enabled?: boolean;
        virtual_port_enabled?: boolean;
        virtual_port_count?: number;
      }>>("load_sessions");
      if (saved && saved.length > 0) {
        const tabs: TabInfo[] = saved.map((s) => ({
          id: s.id,
          name: s.name,
          connection_type: s.connection_type,
          endpoint: s.endpoint,
          state: "disconnected" as ConnectionStatus,
          pluginId: s.plugin_id || "serial",
          params: s.params,
          stats: { txBytes: 0, rxBytes: 0 },
          connectedAt: null,
          transferEnabled: s.transfer_enabled ?? true,
          transferProtocol: s.transfer_protocol,
          sendBarEnabled: s.send_bar_enabled ?? true,
          virtualPortEnabled: s.virtual_port_enabled ?? false,
          virtualPortCount: s.virtual_port_count ?? 0,
          fileServiceEnabled: (s.params?.file_service_enabled as boolean) ?? false,
          fileServiceProtocol: s.params?.file_service_protocol as string | undefined,
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

  const onDataSent = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => {
    sentDataCallbackRef.current = callback;
  }, []);

  const onSessionDisconnect = useCallback((callback: (sessionId: string, reason?: string) => void) => {
    disconnectCallbackRef.current = callback;
  }, []);

  // ── Event Listeners ──────────────────────────────

  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    (async () => {
      const u1 = await listen<{ session_id: string; data_b64?: string; data?: number[] }>("session-data", (event) => {
        // 支持两种数据格式：
        // - data_b64: Base64 字符串（新格式，后端批处理后）— 用 atob 解码，性能远优于 JSON 数字数组
        // - data: number[]（旧格式，向后兼容）
        const data = event.payload.data_b64
          ? decodeBase64(event.payload.data_b64)
          : new Uint8Array(event.payload.data ?? []);
        dataCallbackRef.current?.(event.payload.session_id, data);
      });
      if (cancelled) { u1(); return; }
      unlisteners.push(u1);

      const u2 = await listen<{ session_id: string; endpoint: string; connection_type: string; plugin_id?: string; name: string; params: Record<string, unknown>; connected_at?: number | null; transfer_enabled?: boolean; transfer_protocol?: string; send_bar_enabled?: boolean; virtual_port_pairs?: Array<{ port_a: string; port_b: string }>; file_service_enabled?: boolean; file_service_protocol?: string }>(
        "session-connected",
        (event) => {
          const sid = event.payload.session_id;
          const vPairs = event.payload.virtual_port_pairs;
          // 检查是否已存在同 ID 的 tab（如 reconfigureSession 重连场景），避免重复添加
          const exists = tabsRef.current.some(t => t.id === sid);
          if (exists) {
            // 已存在：更新状态和配置，不新增 tab
            dispatch({ type: "SET_TAB_STATE", id: sid, state: "connected" });
            dispatch({
              type: "UPDATE_TAB_CONFIG",
              id: sid,
              endpoint: event.payload.endpoint,
              params: event.payload.params,
              name: event.payload.name || `${(event.payload.plugin_id && pluginRegistry.get(event.payload.plugin_id)?.manifest.name) || event.payload.plugin_id?.toUpperCase() || "Serial"} @ ${event.payload.endpoint}`,
              transferEnabled: event.payload.transfer_enabled,
              transferProtocol: event.payload.transfer_protocol,
              sendBarEnabled: event.payload.send_bar_enabled,
              pluginId: event.payload.plugin_id || "serial",
              connectedAt: event.payload.connected_at ?? Date.now(),
            });
            // 同步更新虚拟端口对（reconnect 场景下 virtual-port-created
            // 可能先于 session-connected 到达，合并确保不丢失）
            if (vPairs && vPairs.length > 0) {
              dispatch({ type: "UPDATE_TAB_VPORTS", id: sid, pairs: vPairs });
            }
          } else {
            // 真正的新会话：添加 tab，
            // virtual_port_pairs 由 session-connected 直接携带，
            // 避免 virtual-port-created 事件先到达时 tab 尚未创建导致丢失
            dispatch({
              type: "ADD_TAB",
              tab: {
                id: sid,
                name: event.payload.name || `${(event.payload.plugin_id && pluginRegistry.get(event.payload.plugin_id)?.manifest.name) || event.payload.plugin_id?.toUpperCase() || "Serial"} @ ${event.payload.endpoint}`,
                connection_type: event.payload.connection_type,
                endpoint: event.payload.endpoint,
                state: "connected",
                pluginId: event.payload.plugin_id || "serial",
                params: event.payload.params,
                stats: { txBytes: 0, rxBytes: 0 },
                connectedAt: event.payload.connected_at ?? Date.now(),
                transferEnabled: event.payload.transfer_enabled ?? true,
                transferProtocol: event.payload.transfer_protocol,
                sendBarEnabled: event.payload.send_bar_enabled ?? true,
                virtualPortPairs: vPairs,
                virtualPortEnabled: (event.payload.params?.virtual_port_enabled as boolean) ?? false,
                virtualPortCount: (event.payload.params?.virtual_port_count as number) ?? 0,
                fileServiceEnabled: event.payload.file_service_enabled ?? (event.payload.params?.file_service_enabled as boolean) ?? false,
                fileServiceProtocol: event.payload.file_service_protocol ?? (event.payload.params?.file_service_protocol as string),
              },
            });
          }
        }
      );
      if (cancelled) { u2(); return; }
      unlisteners.push(u2);

      const u2b = await listen<{ session_id: string; pairs: Array<{ port_a: string; port_b: string }> }>(
        "virtual-port-created",
        (event) => {
          dispatch({
            type: "UPDATE_TAB_VPORTS",
            id: event.payload.session_id,
            pairs: event.payload.pairs,
          });
        }
      );
      if (cancelled) { u2b(); return; }
      unlisteners.push(u2b);

      const u2c = await listen<{ session_id: string; reason: string }>(
        "virtual-port-failed",
        (event) => {
          console.warn(`[VirtualPort] ${event.payload.session_id}: ${event.payload.reason}`);
          dispatch({
            type: "SET_VPORT_ERROR",
            id: event.payload.session_id,
            error: event.payload.reason,
          });
        }
      );
      if (cancelled) { u2c(); return; }
      unlisteners.push(u2c);

      // 驱动安装成功时清除所有标签页的 VPort 错误状态
      const u2d = await listen("virtual-port-driver-ready", () => {
        tabsRef.current.forEach((tab: { id: string; virtualPortError?: string }) => {
          if (tab.virtualPortError) {
            dispatch({ type: "CLEAR_VPORT_ERROR", id: tab.id });
          }
        });
      });
      if (cancelled) { u2d(); return; }
      unlisteners.push(u2d);

      const u3 = await listen<{ session_id: string; reason?: string }>("session-disconnected", (event) => {
        const reason = event.payload.reason;
        const sid = event.payload.session_id;
        dispatch({ type: "SET_TAB_STATE", id: sid, state: "disconnected" });
        // 清除虚拟端口对信息（端口已在后端销毁）
        dispatch({ type: "UPDATE_TAB_VPORTS", id: sid, pairs: [] });
        disconnectCallbackRef.current?.(sid, reason);
        // 自动停止该会话的日志记录
        setLoggingSessions(prev => {
          if (!prev.has(sid)) return prev;
          const next = new Set(prev);
          next.delete(sid);
          // 异步通知后端停止日志（不等待结果）
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

      const u4 = await listen<{ session_id: string }>("file-transfer:started", (event) => {
        // 传输开始，标记为 transferring（不断开！）
        dispatch({ type: "SET_TAB_STATE", id: event.payload.session_id, state: "transferring" });
      });
      if (cancelled) { u4(); return; }
      unlisteners.push(u4);

      const u5 = await listen<{ session_id: string; success: boolean }>("file-transfer:finished", (event) => {
        // 传输完成（含成功/失败/取消），恢复连接状态
        dispatch({ type: "SET_TAB_STATE", id: event.payload.session_id, state: "connected" });
      });
      if (cancelled) { u5(); return; }
      unlisteners.push(u5);

      const u7 = await listen<{ session_id: string }>("session-switched", (event) => {
        dispatch({ type: "SET_ACTIVE", id: event.payload.session_id });
      });
      if (cancelled) { u7(); return; }
      unlisteners.push(u7);

      const u8 = await listen<{ session_id: string; name: string }>("session-renamed", (event) => {
        dispatch({ type: "RENAME_TAB", id: event.payload.session_id, name: event.payload.name });
      });
      if (cancelled) { u8(); return; }
      unlisteners.push(u8);

      const u9 = await listen<{ tab_id: string; tx_bytes: number; rx_bytes: number; connected_at?: number | null }>(
        "session-stats",
        (event) => {
          dispatch({
            type: "UPDATE_TAB_STATS",
            id: event.payload.tab_id,
            stats: { txBytes: event.payload.tx_bytes, rxBytes: event.payload.rx_bytes },
            connectedAt: event.payload.connected_at,
          });
        }
      );
      if (cancelled) { u9(); return; }
      unlisteners.push(u9);
    })().catch((e) => {
      console.error("SessionContext: 事件监听器注册失败:", e);
    });

    return () => {
      cancelled = true;
      unlisteners.forEach(u => u());
    };
  }, []);

  // ── Periodic log status polling ──────────────────

  const hasActiveLogs = loggingSessions.size > 0;

  useEffect(() => {
    if (!hasActiveLogs) return; // 无活跃日志时清除定时器，节省资源
    const interval = setInterval(async () => {
      try {
        const statuses: Array<{ session_id: string; file_name: string; bytes_written: number }> =
          await invoke("get_log_status");
        setLogStatuses(new Map(statuses.map(s => [s.session_id, { fileName: s.file_name, bytesWritten: s.bytes_written }])));
      } catch (_e) {
        // 静默忽略
      }
    }, 5000); // 5s 轮询降低 IPC 开销，日志状态不需要秒级实时性
    return () => clearInterval(interval);
  }, [hasActiveLogs]);

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
      createOfflineSession,
      disconnect,
      deleteSession,
      sendData,
      switchTab,
      renameTab,
      reconfigureSession,
      getTabs,
      onSessionData,
      onDataSent,
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
