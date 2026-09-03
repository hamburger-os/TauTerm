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
  /** 是否启用文件传输子系统（默认 true） */
  transferEnabled?: boolean;
  /** 文件传输协议（ymodem / xmodem / zmodem） */
  transferProtocol?: string;
  /** 是否启用发送栏（默认 true） */
  sendBarEnabled?: boolean;
  /** Telnet: 本地回显状态（服务器 WONT ECHO 时客户端回显输入，由后端协商推送） */
  localEcho?: boolean;
  /** 是否启用虚拟串口（默认 true） */
  virtualPortEnabled?: boolean;
  /** 虚拟端口对数量（默认 1） */
  virtualPortCount?: number;
  /** 虚拟端口对列表（连接成功时后端推送） */
  virtualVirtualEndpoints?: Array<{ bridge_path: string; external_path: string }>;
  /** 虚拟端口创建失败时的错误信息 */
  virtualPortError?: string;
  /** 虚拟端口失败原因分类（driver_missing | files_missing | permission | create_failed），供前端本地化 */
  virtualPortErrorKind?: string;
  /** SSH 文件服务是否启用（默认 true） */
  fileServiceEnabled?: boolean;
  /** SSH 文件服务协议（"sftp"） */
  fileServiceProtocol?: string;
  /** SSH: 是否启用 journald 日志查看器（默认 false） */
  journaldEnabled?: boolean;
  /**
   * 父会话 ID（多连接支持）。
   * - null/undefined = 根会话（Serial 或 SSH 父会话）
   * - 非空 = 隶属于某 SSH 父会话的子 channel
   */
  parentId?: string | null;
  /** 子 channel 在父会话中的自动编号（从 0 开始） */
  channelIndex?: number;
  /** Local Shell 子会话是否经 Windows UAC helper 启动。 */
  elevated?: boolean;
  /** 根会话是否是可创建多个子终端的容器。 */
  isContainer?: boolean;
}

/** connect() 参数对象 */
export interface ConnectOptions {
  endpoint: string;
  params: Record<string, unknown>;
  name?: string;
  pluginId?: string;
  transferEnabled?: boolean;
  transferProtocol?: string;
  sendBarEnabled?: boolean;
  initialElevated?: boolean;
  journaldEnabled?: boolean;
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

/** 网络调试会话的对端条目（左侧会话树 / 视图共用） */
export interface NetworkPeerEntry {
  peerId: string;
  /** 对端名称（后端按序号自动生成，如 "Peer 1"） */
  name: string;
  /** 对端地址（IP:Port） */
  addr: string;
  /** 本端地址（TCP client 连接后本机分配的 ip:port，与服务端对端条目对应） */
  localAddr?: string;
  state: "connected" | "disconnected";
  txBytes: number;
  rxBytes: number;
}

interface SessionState {
  tabs: TabInfo[];
  activeTabId: string | null;
  connectionTypes: ConnectionTypeInfo[];
  endpoints: EndpointInfo[];
  error: string | null;
  /**
   * 网络调试会话的对端注册表：peerId → 是否已连接。
   * 对端不占标签页（后端 sub_connection, tabbed=false），但 SendBar 各面板
   * 与 sendData 需要据此判定连接态并放行发送（按对端 UUID 路由）。
   */
  peerSessions: Record<string, boolean>;
  /** 网络调试容器会话 → 对端列表（netdbg-peer-joined/left 驱动，左侧树/视图共用） */
  networkPeers: Record<string, NetworkPeerEntry[]>;
  /** 网络调试容器会话 → 当前选中的对端 id（null = 未选中；client 模式自动选中） */
  selectedNetworkPeer: Record<string, string | null>;
  /** 网络调试容器会话 → UDP 手动目标地址（发送栏目标覆盖输入） */
  networkManualTarget: Record<string, string>;
  /** 网络调试容器会话 → 最近 RX 来源地址（UDP server 发送栏快捷回发，去重 + 上限） */
  networkUdpSources: Record<string, string[]>;
  /** 网络调试容器会话 → 本端地址（UDP client 连接后本机 ip:port，前端展示用） */
  networkLocalAddrs: Record<string, string>;
  /** 网络调试容器会话 → 是否群发到全部对端（目标栏「全部客户端」伪目标） */
  networkBroadcast: Record<string, boolean>;
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
  | { type: "SET_TAB_DISCONNECTED"; id: string; info?: DisconnectInfo }
  | { type: "UPDATE_TAB_STATS"; id: string; stats: SessionStats; connectedAt?: number | null }
  | { type: "UPDATE_TAB_ECHO"; id: string; localEcho: boolean }
  | { type: "UPDATE_TAB_CONFIG"; id: string; endpoint: string; params: Record<string, unknown>; name: string; transferEnabled?: boolean; transferProtocol?: string; sendBarEnabled?: boolean; pluginId?: string; connectedAt?: number | null; journaldEnabled?: boolean; fileServiceEnabled?: boolean; fileServiceProtocol?: string }
  | { type: "UPDATE_TAB_VPORTS"; id: string; pairs: Array<{ bridge_path: string; external_path: string }> }
  | { type: "SET_VPORT_ERROR"; id: string; error: string; kind?: string }
  | { type: "CLEAR_VPORT_ERROR"; id: string }
  | { type: "CLEAR_TABS" }
  | { type: "REMOVE_CHILD"; id: string; parentId: string }
  | { type: "REMOVE_ALL_CHILDREN"; parentId: string }
  | { type: "SET_PEER_CONNECTED"; id: string; connected: boolean }
  | { type: "REMOVE_PEER"; id: string }
  | { type: "SET_NETWORK_PEER"; containerId: string; peer: NetworkPeerEntry }
  | { type: "SET_NETWORK_PEERS_BATCH"; containerId: string; entries: NetworkPeerEntry[] }
  | { type: "SET_NETWORK_PEER_STATE"; containerId: string; peerId: string; state: NetworkPeerEntry["state"]; txBytes?: number; rxBytes?: number }
  | { type: "SET_NETWORK_PEER_STATS"; containerId: string; peerId: string; txBytes: number; rxBytes: number }
  | { type: "REMOVE_NETWORK_PEER"; containerId: string; peerId: string }
  | { type: "CLEAR_NETWORK_PEERS"; containerId: string }
  | { type: "SELECT_NETWORK_PEER"; containerId: string; peerId: string | null }
  | { type: "SET_NETWORK_MANUAL_TARGET"; containerId: string; target: string }
  | { type: "ADD_NETWORK_UDP_SOURCE"; containerId: string; addr: string }
  | { type: "SET_NETWORK_LOCAL_ADDR"; containerId: string; addr: string }
  | { type: "SET_NETWORK_BROADCAST"; containerId: string; on: boolean };

function decodeBase64(b64: string): Uint8Array {
  const binary = atob(b64);
  const len = binary.length;
  const bytes = new Uint8Array(len);
  for (let i = 0; i < len; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

function localizeSessionError(error: unknown): string {
  const message = String(error);
  return message.includes("User cancelled the UAC elevation prompt")
    ? i18n.t("localShell.elevationCancelled")
    : message;
}

const initialState: SessionState = {
  tabs: [], activeTabId: null, connectionTypes: [], endpoints: [], error: null,
  peerSessions: {}, networkPeers: {}, selectedNetworkPeer: {}, networkManualTarget: {},
  networkUdpSources: {}, networkLocalAddrs: {}, networkBroadcast: {},
};

function sessionReducer(state: SessionState, action: SessionAction): SessionState {
  switch (action.type) {
    case "SET_TABS": return { ...state, tabs: action.tabs };
    case "ADD_TAB": return { ...state, tabs: [...state.tabs, action.tab], activeTabId: action.tab.isContainer && state.tabs.some(tab => tab.parentId === action.tab.id) ? state.activeTabId : action.tab.id };
    case "REMOVE_TAB": {
      const childIds = state.tabs.filter(t => t.parentId === action.id).map(t => t.id);
      const allRemoved = new Set([action.id, ...childIds]);
      const remaining = state.tabs.filter(t => !allRemoved.has(t.id));
      let nextActive = state.activeTabId;
      if (nextActive && allRemoved.has(nextActive)) nextActive = remaining.find(t => !t.parentId)?.id ?? null;
      return { ...state, tabs: remaining, activeTabId: nextActive };
    }
    case "RENAME_TAB": return { ...state, tabs: state.tabs.map(t => t.id === action.id ? { ...t, name: action.name } : t) };
    case "REORDER_TABS": return { ...state, tabs: action.ids.map(id => state.tabs.find(t => t.id === id)).filter((t): t is TabInfo => t !== undefined) };
    case "SET_ACTIVE": return { ...state, activeTabId: action.id };
    case "SET_CONNECTION_TYPES": return { ...state, connectionTypes: action.types };
    case "SET_ENDPOINTS": return { ...state, endpoints: action.endpoints };
    case "SET_ERROR": return { ...state, error: action.error };
    case "SET_TAB_STATE": return { ...state, tabs: state.tabs.map(t => t.id === action.id ? { ...t, state: action.state, disconnectInfo: undefined } : t) };
    case "SET_TAB_DISCONNECTED": return { ...state, tabs: state.tabs.map(t => t.id === action.id ? { ...t, state: "disconnected", disconnectInfo: action.info } : t) };
    case "UPDATE_TAB_STATS": return { ...state, tabs: state.tabs.map(t => t.id === action.id ? { ...t, stats: action.stats, connectedAt: action.connectedAt ?? t.connectedAt } : t) };
    case "UPDATE_TAB_ECHO": return { ...state, tabs: state.tabs.map(t => t.id === action.id ? { ...t, localEcho: action.localEcho } : t) };
    case "UPDATE_TAB_CONFIG": return { ...state, tabs: state.tabs.map(t => t.id === action.id ? { ...t, name: action.name, endpoint: action.endpoint, params: action.params, transferEnabled: action.transferEnabled ?? t.transferEnabled, transferProtocol: action.transferProtocol ?? t.transferProtocol, sendBarEnabled: action.sendBarEnabled ?? t.sendBarEnabled, pluginId: action.pluginId ?? t.pluginId, connectedAt: action.connectedAt !== undefined ? action.connectedAt : t.connectedAt, virtualPortEnabled: (action.params?.virtual_port_enabled as boolean) ?? t.virtualPortEnabled, virtualPortCount: (action.params?.virtual_port_count as number) ?? t.virtualPortCount, fileServiceEnabled: action.fileServiceEnabled ?? (action.params?.file_service_enabled as boolean) ?? t.fileServiceEnabled, fileServiceProtocol: action.fileServiceProtocol ?? (action.params?.file_service_protocol as string) ?? t.fileServiceProtocol, journaldEnabled: action.journaldEnabled ?? (action.params?.journald_enabled as boolean) ?? t.journaldEnabled } : t) };
    case "UPDATE_TAB_VPORTS": return { ...state, tabs: state.tabs.map(tab => tab.id === action.id ? { ...tab, virtualVirtualEndpoints: action.pairs } : tab) };
    case "SET_VPORT_ERROR": return { ...state, tabs: state.tabs.map(tab => tab.id === action.id ? { ...tab, virtualPortError: action.error, virtualPortErrorKind: action.kind, virtualVirtualEndpoints: undefined } : tab) };
    case "CLEAR_VPORT_ERROR": return { ...state, tabs: state.tabs.map(tab => tab.id === action.id ? { ...tab, virtualPortError: undefined, virtualPortErrorKind: undefined } : tab) };
    case "REMOVE_CHILD": {
      const remaining = state.tabs.filter(t => t.id !== action.id);
      let nextActive = state.activeTabId;
      if (nextActive === action.id) {
        const siblings = remaining.filter(t => t.parentId === action.parentId);
        nextActive = siblings[0]?.id ?? action.parentId ?? remaining.find(t => !t.parentId)?.id ?? null;
      }
      if (!remaining.some(t => t.parentId === action.parentId)) {
        const parentTab = remaining.find(t => t.id === action.parentId);
        if (parentTab) return { ...state, tabs: remaining.map(t => t.id === action.parentId ? { ...t, state: "disconnected" as ConnectionStatus } : t), activeTabId: nextActive };
      }
      return { ...state, tabs: remaining, activeTabId: nextActive };
    }
    case "REMOVE_ALL_CHILDREN": return { ...state, tabs: state.tabs.filter(t => t.parentId !== action.parentId) };
    case "CLEAR_TABS": return { ...state, tabs: [], activeTabId: null };
    case "SET_PEER_CONNECTED": return { ...state, peerSessions: { ...state.peerSessions, [action.id]: action.connected } };
    case "REMOVE_PEER": { const next = { ...state.peerSessions }; delete next[action.id]; return { ...state, peerSessions: next }; }
    case "SET_NETWORK_PEER": { const list = state.networkPeers[action.containerId] ?? []; const ix = list.findIndex(p => p.peerId === action.peer.peerId); const nextList = ix >= 0 ? list.map(p => p.peerId === action.peer.peerId ? { ...p, ...action.peer } : p) : [...list, action.peer]; return { ...state, networkPeers: { ...state.networkPeers, [action.containerId]: nextList } }; }
    case "SET_NETWORK_PEERS_BATCH": { const list = state.networkPeers[action.containerId] ?? []; const merged = [...list]; for (const e of action.entries) { const ix = merged.findIndex(p => p.peerId === e.peerId); if (ix >= 0) merged[ix] = { ...merged[ix], ...e, txBytes: e.txBytes || merged[ix].txBytes, rxBytes: e.rxBytes || merged[ix].rxBytes }; else merged.push(e); } return { ...state, networkPeers: { ...state.networkPeers, [action.containerId]: merged } }; }
    case "SET_NETWORK_PEER_STATE": return { ...state, networkPeers: { ...state.networkPeers, [action.containerId]: (state.networkPeers[action.containerId] ?? []).map(p => p.peerId === action.peerId ? { ...p, state: action.state, txBytes: action.txBytes ?? p.txBytes, rxBytes: action.rxBytes ?? p.rxBytes } : p) } };
    case "SET_NETWORK_PEER_STATS": return { ...state, networkPeers: { ...state.networkPeers, [action.containerId]: (state.networkPeers[action.containerId] ?? []).map(p => p.peerId === action.peerId ? { ...p, txBytes: action.txBytes, rxBytes: action.rxBytes } : p) } };
    case "REMOVE_NETWORK_PEER": return { ...state, networkPeers: { ...state.networkPeers, [action.containerId]: (state.networkPeers[action.containerId] ?? []).filter(p => p.peerId !== action.peerId) } };
    case "CLEAR_NETWORK_PEERS": { const nextPeers = { ...state.networkPeers }; delete nextPeers[action.containerId]; const nextSel = { ...state.selectedNetworkPeer }; delete nextSel[action.containerId]; return { ...state, networkPeers: nextPeers, selectedNetworkPeer: nextSel }; }
    case "SELECT_NETWORK_PEER": return { ...state, selectedNetworkPeer: { ...state.selectedNetworkPeer, [action.containerId]: action.peerId } };
    case "SET_NETWORK_MANUAL_TARGET": return { ...state, networkManualTarget: { ...state.networkManualTarget, [action.containerId]: action.target } };
    case "ADD_NETWORK_UDP_SOURCE": { const list = state.networkUdpSources[action.containerId] ?? []; if (list.includes(action.addr)) return state; const next = [...list, action.addr]; if (next.length > 32) next.splice(0, next.length - 32); return { ...state, networkUdpSources: { ...state.networkUdpSources, [action.containerId]: next } }; }
    case "SET_NETWORK_LOCAL_ADDR": return { ...state, networkLocalAddrs: { ...state.networkLocalAddrs, [action.containerId]: action.addr } };
    case "SET_NETWORK_BROADCAST": return { ...state, networkBroadcast: { ...state.networkBroadcast, [action.containerId]: action.on } };
    default: return state;
  }
}

interface SessionContextValue {
  state: SessionState;
  fetchConnectionTypes: () => Promise<void>;
  refreshEndpoints: () => Promise<void>;
  connect: (opts: ConnectOptions) => Promise<string | null>;
  createOfflineSession: (endpoint: string, params: Record<string, unknown>, name?: string, pluginId?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean) => Promise<string | null>;
  disconnect: (sessionId: string) => Promise<void>;
  deleteSession: (sessionId: string, skipDisconnect?: boolean) => Promise<void>;
  sendData: (sessionId: string, data: string | Uint8Array) => Promise<void>;
  switchTab: (sessionId: string) => Promise<void>;
  renameTab: (sessionId: string, name: string) => Promise<void>;
  reconfigureSession: (sessionId: string, endpoint: string, params: Record<string, unknown>, name?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean, pluginId?: string, journaldEnabled?: boolean) => Promise<void>;
  openChannel: (parentSessionId: string, elevated?: boolean) => Promise<string | null>;
  closeChannel: (channelId: string, parentId: string) => Promise<void>;
  getTabs: () => Promise<void>;
  onSessionData: (callback: (sessionId: string, data: Uint8Array) => void) => void;
  onDataSent: (callback: (sessionId: string, data: Uint8Array) => void) => void;
  subscribeDataSent: (callback: (sessionId: string, data: Uint8Array) => void) => () => void;
  isSessionConnected: (sessionId: string) => boolean;
  selectNetworkPeer: (containerId: string, peerId: string | null) => void;
  getNetworkPeers: (containerId: string) => NetworkPeerEntry[];
  disconnectNetworkPeer: (containerId: string, peerId: string) => Promise<void>;
  clearNetworkPeer: (containerId: string, peerId: string) => Promise<void>;
  mergeNetworkPeers: (containerId: string, entries: NetworkPeerEntry[]) => void;
  setNetworkManualTarget: (containerId: string, target: string) => void;
  registerNetworkUdpSource: (containerId: string, addr: string) => void;
  subscribeNetworkManualSent: (callback: (containerId: string, target: string, bytes: Uint8Array) => void) => () => void;
  setNetworkBroadcast: (containerId: string, on: boolean) => void;
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
  const peerSessionsRef = useRef<Record<string, boolean>>({});
  const networkPeerContainerRef = useRef<Record<string, string>>({});
  const networkManualSentSubscribersRef = useRef<Set<(containerId: string, target: string, bytes: Uint8Array) => void>>(new Set());
  const disconnectCallbackRef = useRef<((sessionId: string, reason?: string) => void) | null>(null);
  const tabsRef = useRef(state.tabs); tabsRef.current = state.tabs;
  const stateRef = useRef(state); stateRef.current = state;
  const lastActiveChildRef = useRef<Map<string, string>>(new Map());
  const pendingEchoRef = useRef<Map<string, boolean>>(new Map());

  useEffect(() => {
    const activeId = state.activeTabId; if (!activeId) return;
    const activeTab = state.tabs.find(tab => tab.id === activeId);
    if (activeTab?.parentId) lastActiveChildRef.current.set(activeTab.parentId, activeTab.id);
  }, [state.activeTabId, state.tabs]);

  const [loggingSessions, setLoggingSessions] = useState<Set<string>>(new Set());
  const [logStatuses, setLogStatuses] = useState<Map<string, { fileName: string; bytesWritten: number }>>(new Map());

  const startSessionLog = useCallback(async (sessionId: string): Promise<string> => {
    await invoke<string>("start_session_log", { sessionId }); setLoggingSessions(prev => new Set(prev).add(sessionId));
    const statuses: Array<{ session_id: string; file_name: string; bytes_written: number }> = await invoke("get_log_status");
    setLogStatuses(new Map(statuses.map(s => [s.session_id, { fileName: s.file_name, bytesWritten: s.bytes_written }]))); return sessionId;
  }, []);
  const stopSessionLog = useCallback(async (sessionId: string) => { await invoke("stop_session_log", { sessionId }); setLoggingSessions(prev => { const next = new Set(prev); next.delete(sessionId); return next; }); setLogStatuses(prev => { const next = new Map(prev); next.delete(sessionId); return next; }); }, []);

  const fetchConnectionTypes = useCallback(async () => { try { dispatch({ type: "SET_CONNECTION_TYPES", types: await invoke<ConnectionTypeInfo[]>("get_connection_types") }); } catch (e) { dispatch({ type: "SET_ERROR", error: `${e}` }); } }, []);
  const refreshEndpoints = useCallback(async () => { const pluginIds = pluginRegistry.getByCapability("endpoint_discovery").map(plugin => plugin.manifest.id); const results = await Promise.allSettled(pluginIds.map(pluginId => invoke<EndpointInfo[]>("enumerate_endpoints", { pluginId }))); dispatch({ type: "SET_ENDPOINTS", endpoints: results.flatMap(result => result.status === "fulfilled" ? result.value : []) }); }, []);

  const connect = useCallback(async (opts: ConnectOptions) => {
    const { endpoint, params, name, pluginId, transferEnabled, transferProtocol, sendBarEnabled, journaldEnabled, sessionId, initialElevated } = opts;
    if (sessionId) dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "connecting" });
    try {
      const command = pluginId === "trdp" ? "connect_session_trdp" : "connect_session";
      return await invoke<string>(command, { request: { endpoint, params, name, pluginId: pluginId || "serial", transferEnabled: transferEnabled ?? true, transferProtocol: transferProtocol || "ymodem", sendBarEnabled: sendBarEnabled ?? true, journaldEnabled: journaldEnabled ?? false, sessionId: sessionId || null, initialElevated: initialElevated ?? false } });
    } catch (e) { dispatch({ type: "SET_ERROR", error: `${i18n.t("localShell.connectFailed")}: ${localizeSessionError(e)}` }); if (sessionId) dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" }); return null; }
  }, []);

  const createOfflineSession = useCallback(async (endpoint: string, params: Record<string, unknown>, name?: string, pluginId?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean) => {
    try { const pid = pluginId || "serial"; const pluginName = pluginRegistry.get(pid)?.manifest.name || pid.toUpperCase(); const effectiveName = name || (pid === "local-shell" ? await invoke<string>("resolve_local_shell_session_name", { params }) : `${pluginName} @ ${endpoint}`); const sessionId = await invoke<string>("save_session_config", { request: { endpoint, params, name: effectiveName, pluginId: pid, transferEnabled: transferEnabled ?? true, transferProtocol: transferProtocol || "ymodem", sendBarEnabled: sendBarEnabled ?? true } }); dispatch({ type: "ADD_TAB", tab: { id: sessionId, name: effectiveName, connection_type: pid, endpoint, state: "disconnected", pluginId: pid, params, stats: { txBytes: 0, rxBytes: 0 }, connectedAt: null, transferEnabled: transferEnabled ?? true, transferProtocol, sendBarEnabled: sendBarEnabled ?? true, virtualPortEnabled: (params.virtual_port_enabled as boolean) ?? false, virtualPortCount: (params.virtual_port_count as number) ?? 0, fileServiceEnabled: (params.file_service_enabled as boolean) ?? false, fileServiceProtocol: params.file_service_protocol as string | undefined, journaldEnabled: (params.journald_enabled as boolean) ?? false } }); return sessionId; } catch (e) { dispatch({ type: "SET_ERROR", error: `创建会话失败: ${e}` }); return null; }
  }, []);

  const disconnect = useCallback(async (sessionId: string) => { const tab = state.tabs.find(t => t.id === sessionId); if (tab?.state === "disconnected") return; dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" }); try { await invoke("disconnect_session", { sessionId }); } catch (e) { dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "connected" }); dispatch({ type: "SET_ERROR", error: `断开失败: ${e}` }); } }, [state.tabs]);
  const deleteSession = useCallback(async (sessionId: string, skipDisconnect = false) => { const tab = state.tabs.find(t => t.id === sessionId); if (!skipDisconnect && (tab?.state === "connected" || tab?.state === "connecting" || tab?.state === "transferring")) { dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" }); try { await invoke("disconnect_session", { sessionId }); } catch { dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "connected" }); return; } } await invoke("delete_session_config", { sessionId }).catch(() => {}); dispatch({ type: "REMOVE_TAB", id: sessionId }); releaseSessionStore(sessionId); }, [state.tabs]);
  const sendData = useCallback(async (sessionId: string, data: string | Uint8Array) => { const tab = tabsRef.current.find(t => t.id === sessionId); const isPeer = peerSessionsRef.current[sessionId] === true; if ((!tab || tab.state === "disconnected") && !isPeer) return; const isText = typeof data === "string"; const bytes = isText ? new TextEncoder().encode(data) : data; const written = await invoke<number[]>("write_data", { sessionId, data: Array.from(bytes), transcode: isText }); const actual = new Uint8Array(written); sentDataCallbackRef.current?.(sessionId, actual); sentDataSubscribersRef.current.forEach(cb => cb(sessionId, actual)); }, []);
  const switchTab = useCallback(async (sessionId: string) => { const tabs = tabsRef.current; const targetTab = tabs.find(tab => tab.id === sessionId); let resolved = sessionId; if (targetTab?.parentId) lastActiveChildRef.current.set(targetTab.parentId, targetTab.id); else if (targetTab && (pluginRegistry.get(targetTab.pluginId)?.manifest.capabilities.includes("multi_session") ?? false)) { const children = tabs.filter(tab => tab.parentId === targetTab.id); if (children.length) { const remembered = children.find(child => child.id === lastActiveChildRef.current.get(targetTab.id)); resolved = remembered?.id ?? children[0].id; } } dispatch({ type: "SET_ACTIVE", id: resolved }); await invoke("switch_active_session", { sessionId: resolved }).catch(() => {}); }, []);
  const renameTab = useCallback(async (sessionId: string, name: string) => { dispatch({ type: "RENAME_TAB", id: sessionId, name }); await invoke("rename_session", { sessionId, newName: name }).catch(() => {}); }, []);

  const reconfigureSession = useCallback(async (sessionId: string, endpoint: string, params: Record<string, unknown>, name?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean, pluginId?: string, journaldEnabled?: boolean) => {
    const tab = state.tabs.find(t => t.id === sessionId); const wasConnected = tab?.state === "connected" || tab?.state === "transferring"; if (wasConnected) { dispatch({ type: "REMOVE_ALL_CHILDREN", parentId: sessionId }); try { await invoke("disconnect_session", { sessionId }); dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" }); } catch (e) { dispatch({ type: "SET_ERROR", error: `断开失败: ${e}` }); return; } }
    const effectivePluginId = pluginId || tab?.pluginId; if (!effectivePluginId) return;
    await invoke("save_session_config", { request: { endpoint, params, name: name || undefined, pluginId: effectivePluginId, transferEnabled: transferEnabled ?? true, transferProtocol: transferProtocol || "ymodem", sendBarEnabled: sendBarEnabled ?? true, sessionId } });
    dispatch({ type: "UPDATE_TAB_CONFIG", id: sessionId, endpoint, params, name: name || tab?.name || `${pluginRegistry.get(effectivePluginId)?.manifest.name || effectivePluginId.toUpperCase()} @ ${endpoint}`, transferEnabled, transferProtocol, sendBarEnabled, pluginId: effectivePluginId, journaldEnabled: journaldEnabled ?? (params.journald_enabled as boolean) ?? tab?.journaldEnabled, fileServiceEnabled: (params.file_service_enabled as boolean) ?? tab?.fileServiceEnabled, fileServiceProtocol: (params.file_service_protocol as string) ?? tab?.fileServiceProtocol });
    if (wasConnected) { try { const command = effectivePluginId === "trdp" ? "connect_session_trdp" : "connect_session"; const newSessionId = await invoke<string>(command, { request: { endpoint, params, name: name || tab?.name || undefined, pluginId: effectivePluginId, transferEnabled: transferEnabled ?? true, transferProtocol: transferProtocol || "ymodem", sendBarEnabled: sendBarEnabled ?? true, journaldEnabled: (params.journald_enabled as boolean) ?? tab?.journaldEnabled ?? false, sessionId } }); dispatch({ type: "SET_TAB_STATE", id: newSessionId, state: "connected" }); } catch (e) { dispatch({ type: "SET_ERROR", error: `重连失败: ${e}` }); } }
  }, [state.tabs]);

  const getTabs = useCallback(async () => { try { dispatch({ type: "SET_TABS", tabs: await invoke<TabInfo[]>("get_tabs") }); } catch {} }, []);
  const openChannel = useCallback(async (parentSessionId: string, elevated = false) => { try { return await invoke<string>("open_channel", { sessionId: parentSessionId, elevated }); } catch (e) { dispatch({ type: "SET_ERROR", error: `${e}` }); return null; } }, []);
  const closeChannel = useCallback(async (channelId: string, parentId: string) => { const resetCounter = !tabsRef.current.some(tab => tab.parentId === parentId && tab.id !== channelId); dispatch({ type: "REMOVE_CHILD", id: channelId, parentId }); await invoke("close_channel", { sessionId: channelId, parentId, resetCounter }).catch(() => getTabs()); }, [getTabs]);
  const clearError = useCallback(() => dispatch({ type: "SET_ERROR", error: null }), []);

  const loadSavedSessions = useCallback(async () => { try { const saved = await invoke<Array<{ id: string; name: string; connection_type: string; endpoint: string; params: Record<string, unknown>; plugin_id?: string; transfer_enabled?: boolean; transfer_protocol?: string; send_bar_enabled?: boolean; virtual_port_enabled?: boolean; virtual_port_count?: number }>>("load_sessions"); const tabs: TabInfo[] = saved.map(s => ({ id: s.id, name: s.name, connection_type: s.connection_type, endpoint: s.endpoint, state: "disconnected", pluginId: s.plugin_id || "serial", params: s.params, stats: { txBytes: 0, rxBytes: 0 }, connectedAt: null, transferEnabled: s.transfer_enabled ?? true, transferProtocol: s.transfer_protocol, sendBarEnabled: s.send_bar_enabled ?? true, virtualPortEnabled: s.virtual_port_enabled ?? false, virtualPortCount: s.virtual_port_count ?? 0, fileServiceEnabled: (s.params.file_service_enabled as boolean) ?? false, fileServiceProtocol: s.params.file_service_protocol as string | undefined, journaldEnabled: (s.params.journald_enabled as boolean) ?? false })); if (tabs.length) { dispatch({ type: "SET_TABS", tabs }); dispatch({ type: "SET_ACTIVE", id: tabs[0].id }); } } catch {} }, []);

  const onSessionData = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => { dataCallbackRef.current = callback; }, []);
  const onDataSent = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => { sentDataCallbackRef.current = callback; }, []);
  const subscribeDataSent = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => { sentDataSubscribersRef.current.add(callback); return () => { sentDataSubscribersRef.current.delete(callback); }; }, []);
  const isSessionConnected = useCallback((sessionId: string) => { const tab = tabsRef.current.find(t => t.id === sessionId); return tab ? tab.state === "connected" || tab.state === "transferring" : peerSessionsRef.current[sessionId] === true; }, []);
  const selectNetworkPeer = useCallback((containerId: string, peerId: string | null) => dispatch({ type: "SELECT_NETWORK_PEER", containerId, peerId }), []);
  const getNetworkPeers = useCallback((containerId: string) => stateRef.current.networkPeers[containerId] ?? [], []);
  const disconnectNetworkPeer = useCallback(async (containerId: string, peerId: string) => { await invoke("close_network_peer", { sessionId: peerId }).catch(() => {}); peerSessionsRef.current[peerId] = false; dispatch({ type: "SET_PEER_CONNECTED", id: peerId, connected: false }); dispatch({ type: "SET_NETWORK_PEER_STATE", containerId, peerId, state: "disconnected" }); }, []);
  const clearNetworkPeer = useCallback(async (containerId: string, peerId: string) => { await invoke("close_network_peer", { sessionId: peerId }).catch(() => {}); delete peerSessionsRef.current[peerId]; delete networkPeerContainerRef.current[peerId]; dispatch({ type: "REMOVE_NETWORK_PEER", containerId, peerId }); dispatch({ type: "REMOVE_PEER", id: peerId }); }, []);
  const mergeNetworkPeers = useCallback((containerId: string, entries: NetworkPeerEntry[]) => dispatch({ type: "SET_NETWORK_PEERS_BATCH", containerId, entries }), []);
  const setNetworkManualTarget = useCallback((containerId: string, target: string) => dispatch({ type: "SET_NETWORK_MANUAL_TARGET", containerId, target }), []);
  const registerNetworkUdpSource = useCallback((containerId: string, addr: string) => dispatch({ type: "ADD_NETWORK_UDP_SOURCE", containerId, addr }), []);
  const subscribeNetworkManualSent = useCallback((callback: (containerId: string, target: string, bytes: Uint8Array) => void) => { networkManualSentSubscribersRef.current.add(callback); return () => { networkManualSentSubscribersRef.current.delete(callback); }; }, []);
  const setNetworkBroadcast = useCallback((containerId: string, on: boolean) => dispatch({ type: "SET_NETWORK_BROADCAST", containerId, on }), []);

  const sendToTarget = useCallback(async (containerId: string, data: string | Uint8Array) => { const tab = tabsRef.current.find(t => t.id === containerId); const params = (tab?.params ?? {}) as Record<string, unknown>; const transport = params.transport as string | undefined; if (transport !== "tcp" && transport !== "udp") return sendData(containerId, data); const role = (params.role as string | undefined) ?? "client"; const isText = typeof data === "string"; const bytes = isText ? new TextEncoder().encode(data) : data; if (transport === "udp") { if (role === "server") { const target = (stateRef.current.networkManualTarget[containerId] ?? "").trim(); if (!target) throw new Error("无可用发送目标"); const written = await invoke<number[]>("network_udp_send_to", { sessionId: containerId, targetAddr: target, data: Array.from(bytes), transcode: isText }); networkManualSentSubscribersRef.current.forEach(cb => cb(containerId, target, new Uint8Array(written))); } else { const written = await invoke<number[]>("network_udp_send", { sessionId: containerId, data: Array.from(bytes), transcode: isText }); const remote = `${params.remote_host ?? "127.0.0.1"}:${params.remote_port ?? 0}`; networkManualSentSubscribersRef.current.forEach(cb => cb(containerId, remote, new Uint8Array(written))); } return; } const peers = (stateRef.current.networkPeers[containerId] ?? []).filter(p => p.state === "connected"); if (stateRef.current.networkBroadcast[containerId]) { for (const p of peers) await sendData(p.peerId, data); return; } const selected = stateRef.current.selectedNetworkPeer[containerId]; const peer = peers.find(p => p.peerId === selected) ?? (peers.length === 1 ? peers[0] : undefined); if (!peer) throw new Error("无可用发送目标"); await sendData(peer.peerId, data); }, [sendData]);
  const updateSessionStats = useCallback((sessionId: string, txBytes: number, rxBytes: number, rxPackets?: number, txPackets?: number) => dispatch({ type: "UPDATE_TAB_STATS", id: sessionId, stats: { txBytes, rxBytes, ...(rxPackets !== undefined ? { rxPackets } : {}), ...(txPackets !== undefined ? { txPackets } : {}) } }), []);
  const onSessionDisconnect = useCallback((callback: (sessionId: string, reason?: string) => void) => { disconnectCallbackRef.current = callback; }, []);

  useEffect(() => {
    let cancelled = false; const unlisteners: UnlistenFn[] = [];
    (async () => {
      const push = async (promise: Promise<UnlistenFn>) => { const unlisten = await promise; if (cancelled) unlisten(); else unlisteners.push(unlisten); };
      await push(listen<{ session_id: string; data_b64?: string; data?: number[] }>("session-data", event => { const data = event.payload.data_b64 ? decodeBase64(event.payload.data_b64) : new Uint8Array(event.payload.data ?? []); dataCallbackRef.current?.(event.payload.session_id, data); }));
      await push(listen<{ session_id: string; local_echo: boolean }>("telnet-echo-state", event => { const sid = event.payload.session_id; if (!tabsRef.current.some(t => t.id === sid)) pendingEchoRef.current.set(sid, event.payload.local_echo); else dispatch({ type: "UPDATE_TAB_ECHO", id: sid, localEcho: event.payload.local_echo }); }));
      await push(listen<any>("session-connected", event => { const p = event.payload; const sid = p.session_id; const exists = tabsRef.current.some(t => t.id === sid); if (exists) { dispatch({ type: "SET_TAB_STATE", id: sid, state: "connected" }); dispatch({ type: "UPDATE_TAB_CONFIG", id: sid, endpoint: p.endpoint, params: p.params, name: p.name, transferEnabled: p.transfer_enabled, transferProtocol: p.transfer_protocol, sendBarEnabled: p.send_bar_enabled, pluginId: p.plugin_id, connectedAt: p.connected_at ?? Date.now(), journaldEnabled: p.journald_enabled, fileServiceEnabled: p.file_service_enabled, fileServiceProtocol: p.file_service_protocol }); return; } dispatch({ type: "ADD_TAB", tab: { id: sid, name: p.name, connection_type: p.connection_type, endpoint: p.endpoint, state: "connected", pluginId: p.plugin_id || p.connection_type, params: p.params, stats: { txBytes: 0, rxBytes: 0 }, connectedAt: p.connected_at ?? Date.now(), transferEnabled: p.transfer_enabled ?? false, transferProtocol: p.transfer_protocol, sendBarEnabled: p.send_bar_enabled ?? false, parentId: p.parent_id, channelIndex: p.channel_index, elevated: p.elevated, isContainer: p.is_container, fileServiceEnabled: p.file_service_enabled, fileServiceProtocol: p.file_service_protocol, journaldEnabled: p.journald_enabled } }); }));
      await push(listen<any>("virtual-port-created", event => dispatch({ type: "UPDATE_TAB_VPORTS", id: event.payload.session_id, pairs: event.payload.pairs })));
      await push(listen<any>("virtual-port-failed", event => dispatch({ type: "SET_VPORT_ERROR", id: event.payload.session_id, error: event.payload.reason, kind: event.payload.kind })));
      await push(listen<any>("channel-closed", event => event.payload.disconnect_info?.retain_terminal ? dispatch({ type: "SET_TAB_DISCONNECTED", id: event.payload.channel_id, info: event.payload.disconnect_info }) : dispatch({ type: "REMOVE_CHILD", id: event.payload.channel_id, parentId: event.payload.parent_id })));
      await push(listen<any>("netdbg-peer-joined", event => { const p = event.payload; peerSessionsRef.current[p.peer_id] = true; networkPeerContainerRef.current[p.peer_id] = p.session_id; dispatch({ type: "SET_PEER_CONNECTED", id: p.peer_id, connected: true }); dispatch({ type: "SET_NETWORK_PEER", containerId: p.session_id, peer: { peerId: p.peer_id, name: p.peer_name, addr: p.peer_addr, localAddr: p.local_addr, state: "connected", txBytes: 0, rxBytes: 0 } }); }));
      await push(listen<any>("netdbg-peer-left", event => { const p = event.payload; peerSessionsRef.current[p.peer_id] = false; dispatch({ type: "SET_PEER_CONNECTED", id: p.peer_id, connected: false }); dispatch({ type: "SET_NETWORK_PEER_STATE", containerId: p.session_id, peerId: p.peer_id, state: "disconnected", txBytes: p.tx_bytes, rxBytes: p.rx_bytes }); }));
      await push(listen<any>("session-disconnected", event => { const p = event.payload; dispatch({ type: "SET_TAB_DISCONNECTED", id: p.session_id, info: p.disconnect_info }); dispatch({ type: "REMOVE_ALL_CHILDREN", parentId: p.session_id }); disconnectCallbackRef.current?.(p.session_id, p.reason); }));
      await push(listen<any>("file-transfer:started", event => dispatch({ type: "SET_TAB_STATE", id: event.payload.session_id, state: "transferring" })));
      await push(listen<any>("file-transfer:finished", event => dispatch({ type: "SET_TAB_STATE", id: event.payload.session_id, state: "connected" })));
      await push(listen<any>("session-switched", event => dispatch({ type: "SET_ACTIVE", id: event.payload.session_id })));
      await push(listen<any>("session-renamed", event => dispatch({ type: "RENAME_TAB", id: event.payload.session_id, name: event.payload.name })));
      await push(listen<any>("session-stats", event => { const p = event.payload; const cid = networkPeerContainerRef.current[p.tab_id]; if (cid) dispatch({ type: "SET_NETWORK_PEER_STATS", containerId: cid, peerId: p.tab_id, txBytes: p.tx_bytes, rxBytes: p.rx_bytes }); else dispatch({ type: "UPDATE_TAB_STATS", id: p.tab_id, stats: { txBytes: p.tx_bytes, rxBytes: p.rx_bytes }, connectedAt: p.connected_at }); }));
    })().catch(console.error);
    return () => { cancelled = true; unlisteners.forEach(unlisten => unlisten()); };
  }, []);

  useEffect(() => { if (!loggingSessions.size) return; const timer = setInterval(async () => { const statuses: Array<{ session_id: string; file_name: string; bytes_written: number }> = await invoke("get_log_status").catch(() => []); setLogStatuses(new Map(statuses.map(s => [s.session_id, { fileName: s.file_name, bytesWritten: s.bytes_written }]))); }, 5000); return () => clearInterval(timer); }, [loggingSessions.size]);
  useEffect(() => { fetchConnectionTypes(); refreshEndpoints(); loadSavedSessions(); }, [fetchConnectionTypes, refreshEndpoints, loadSavedSessions]);

  return <SessionContext.Provider value={{ state, fetchConnectionTypes, refreshEndpoints, connect, createOfflineSession, disconnect, deleteSession, sendData, switchTab, renameTab, reconfigureSession, openChannel, closeChannel, getTabs, onSessionData, onDataSent, subscribeDataSent, isSessionConnected, selectNetworkPeer, getNetworkPeers, disconnectNetworkPeer, clearNetworkPeer, mergeNetworkPeers, setNetworkManualTarget, registerNetworkUdpSource, subscribeNetworkManualSent, setNetworkBroadcast, sendToTarget, updateSessionStats, onSessionDisconnect, clearError, startSessionLog, stopSessionLog, loggingSessions, logStatuses }}>{children}</SessionContext.Provider>;
}

export function useSession() {
  const ctx = useContext(SessionContext);
  if (!ctx) throw new Error("useSession must be used within SessionProvider");
  return ctx;
}
