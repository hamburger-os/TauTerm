import type { ConnectionStatus } from "./session-lifecycle.ts";
import { compareRuntimeIds } from "./session-lifecycle.ts";
export type { ConnectionStatus } from "./session-lifecycle.ts";

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
  runtimeId: string | null;
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

export interface SessionState {
  catalogStatus: "loading" | "ready" | "error";
  tabs: TabInfo[];
  activeTabId: string | null;
  connectionTypes: ConnectionTypeInfo[];
  endpoints: EndpointInfo[];
  error: string | null;
}

export interface SavedSessionConfig {
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
}

export type SessionAction =
  | { type: "SESSION_CONNECTED"; tab: TabInfo }
  | { type: "SESSION_DISCONNECTED"; id: string; runtimeId: string; info?: DisconnectInfo }
  | { type: "CATALOG_LOADED"; tabs: TabInfo[] }
  | { type: "CATALOG_FAILED"; error: string }
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
  | { type: "UPDATE_TAB_STATS"; id: string; runtimeId: string; stats: SessionStats; connectedAt?: number | null }
  | { type: "UPDATE_TAB_CONFIG"; id: string; endpoint: string; params: Record<string, unknown>; name: string; transferEnabled?: boolean; transferProtocol?: string; sendBarEnabled?: boolean; pluginId?: string; connectedAt?: number | null }
  | { type: "CLEAR_TABS" }
  | { type: "REMOVE_CHILD"; id: string; parentId: string }
  | { type: "REMOVE_ALL_CHILDREN"; parentId: string };


export const initialSessionState: SessionState = {
  catalogStatus: "loading",
  tabs: [],
  activeTabId: null,
  connectionTypes: [],
  endpoints: [],
  error: null,
};

export function sessionReducer(state: SessionState, action: SessionAction): SessionState {
  switch (action.type) {
    case "SESSION_CONNECTED": {
      const tab = action.tab;
      if (!tab.runtimeId) return state;
      const existing = state.tabs.find(current => current.id === tab.id);
      if (existing?.runtimeId) {
        const order = compareRuntimeIds(tab.runtimeId, existing.runtimeId);
        if (order <= 0) return state;
      }
      if (tab.parentId) {
        const parent = state.tabs.find(current => current.id === tab.parentId);
        if (parent?.runtimeId !== tab.runtimeId || parent.state !== "connected") return state;
      }
      const tabs = state.tabs.filter(current => current.id !== tab.id
        && !(current.parentId === tab.id && current.runtimeId !== tab.runtimeId));
      const updated = { ...tab, name: existing?.name ?? tab.name, disconnectInfo: undefined };
      const index = state.tabs.findIndex(current => current.id === tab.id);
      tabs.splice(index < 0 ? tabs.length : Math.min(index, tabs.length), 0, updated);
      return { ...state, tabs, activeTabId: tab.isContainer && tabs.some(current => current.parentId === tab.id)
        ? state.activeTabId : tab.id };
    }
    case "SESSION_DISCONNECTED": {
      const tab = state.tabs.find(current => current.id === action.id);
      if (!tab || tab.runtimeId !== action.runtimeId) return state;
      const tabs = state.tabs
        .filter(current => action.info?.retain_terminal || current.parentId !== action.id)
        .map(current => current.id === action.id
          || (current.parentId === action.id && current.state !== "disconnected")
          ? { ...current, state: "disconnected" as const, disconnectInfo: action.info } : current);
      const activeRemoved = state.activeTabId !== null
        && state.tabs.some(current => current.id === state.activeTabId && current.parentId === action.id)
        && !tabs.some(current => current.id === state.activeTabId);
      return { ...state, tabs, activeTabId: activeRemoved ? action.id : state.activeTabId };
    }
    case "CATALOG_LOADED": {
      // A connection created while startup I/O was pending already owns its runtime state.
      const existing = new Map(state.tabs.map(tab => [tab.id, tab]));
      const tabs = action.tabs.map(tab => existing.get(tab.id) ?? tab);
      const savedIds = new Set(action.tabs.map(tab => tab.id));
      tabs.push(...state.tabs.filter(tab => !savedIds.has(tab.id)));
      return { ...state, tabs, catalogStatus: "ready", activeTabId: state.activeTabId ?? tabs[0]?.id ?? null };
    }
    case "CATALOG_FAILED": return { ...state, catalogStatus: "error", error: action.error };
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
      return { ...state, tabs: state.tabs.map(t => t.id === action.id && t.runtimeId === action.runtimeId && t.state === "connected" ? { ...t, stats: action.stats, connectedAt: action.connectedAt ?? t.connectedAt } : t) };
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
