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

/** 前端只接收用户可访问的虚拟端点；内部 bridge path 不属于 UI 契约。 */
export interface VirtualPortEndpoint {
  external_path: string;
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
  /** 对外虚拟端点列表（连接成功时后端推送） */
  virtualVirtualEndpoints?: VirtualPortEndpoint[];
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
  /** Local Shell 子会话是否经系统提权 helper 启动。 */
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

function persistedSessionParams(
  pluginId: string,
  sessionId: string,
  params: Record<string, unknown>,
): Record<string, unknown> {
  if (pluginId !== "ssh") return params;

  const sanitized = { ...params };
  delete sanitized.password;
  delete sanitized.private_key;
  delete sanitized.passphrase;
  sanitized.credential_account = `ssh-session:${sessionId}`;
  return sanitized;
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
  | { type: "SET_ACTIVE"; id: string | null }
  | { type: "SET_CONNECTION_TYPES"; types: ConnectionTypeInfo[] }
  | { type: "SET_ENDPOINTS"; endpoints: EndpointInfo[] }
  | { type: "REPLACE_ENDPOINTS_FOR_PLUGIN"; pluginId: string; endpoints: EndpointInfo[] }
  | { type: "SET_ERROR"; error: string | null }
  | { type: "SET_TAB_STATE"; id: string; state: ConnectionStatus }
  | { type: "SET_TAB_DISCONNECTED"; id: string; info?: DisconnectInfo }
  | { type: "UPDATE_TAB_STATS"; id: string; stats: SessionStats; connectedAt?: number | null }
  | { type: "UPDATE_TAB_ECHO"; id: string; localEcho: boolean }
  | { type: "UPDATE_TAB_CONFIG"; id: string; endpoint: string; params: Record<string, unknown>; name: string; transferEnabled?: boolean; transferProtocol?: string; sendBarEnabled?: boolean; pluginId?: string; connectedAt?: number | null; journaldEnabled?: boolean; fileServiceEnabled?: boolean; fileServiceProtocol?: string }
  | { type: "UPDATE_TAB_VPORTS"; id: string; pairs: VirtualPortEndpoint[] }
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

function localizeSessionError(error: unknown): string {
  const message = String(error);
  return message.includes("User cancelled the UAC elevation prompt")
    ? i18n.t("localShell.elevationCancelled")
    : message;
}

const initialState: SessionState = {
  tabs: [],
  activeTabId: null,
  connectionTypes: [],
  endpoints: [],
  error: null,
  peerSessions: {},
  networkPeers: {},
  selectedNetworkPeer: {},
  networkManualTarget: {},
  networkUdpSources: {},
  networkLocalAddrs: {},
  networkBroadcast: {},
};

function sessionReducer(state: SessionState, action: SessionAction): SessionState {
  switch (action.type) {
    case "SET_TABS":
      return { ...state, tabs: action.tabs };
    case "ADD_TAB": {
      return {
        ...state,
        tabs: [...state.tabs, action.tab],
        activeTabId: action.tab.isContainer
          && state.tabs.some(tab => tab.parentId === action.tab.id)
          ? state.activeTabId
          : action.tab.id,
      };
    }
    case "REMOVE_TAB": {
      const childIds = state.tabs
        .filter(t => t.parentId === action.id)
        .map(t => t.id);
      const allRemoved = new Set([action.id, ...childIds]);
      const remaining = state.tabs.filter(t => !allRemoved.has(t.id));
      let nextActive = state.activeTabId;
      if (nextActive && allRemoved.has(nextActive)) {
        nextActive = remaining.find(t => !t.parentId)?.id ?? null;
      }
      return { ...state, tabs: remaining, activeTabId: nextActive };
    }
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
    case "REPLACE_ENDPOINTS_FOR_PLUGIN":
      return {
        ...state,
        endpoints: [
          ...state.endpoints.filter(endpoint => endpoint.connection_type !== action.pluginId),
          ...action.endpoints,
        ],
      };
    case "SET_ERROR":
      return { ...state, error: action.error };
    case "SET_TAB_STATE":
      return {
        ...state,
        tabs: state.tabs.map(t => t.id === action.id
          ? { ...t, state: action.state, disconnectInfo: undefined }
          : t),
      };
    case "SET_TAB_DISCONNECTED":
      return {
        ...state,
        tabs: state.tabs.map(t => t.id === action.id
          ? { ...t, state: "disconnected", disconnectInfo: action.info }
          : t),
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
    case "UPDATE_TAB_ECHO":
      return {
        ...state,
        tabs: state.tabs.map(t => t.id === action.id ? { ...t, localEcho: action.localEcho } : t),
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
                fileServiceEnabled: action.fileServiceEnabled ?? (action.params?.file_service_enabled as boolean) ?? t.fileServiceEnabled,
                fileServiceProtocol: action.fileServiceProtocol ?? (action.params?.file_service_protocol as string) ?? t.fileServiceProtocol,
                journaldEnabled: action.journaldEnabled ?? (action.params?.journald_enabled as boolean) ?? t.journaldEnabled,
              }
            : t
        ),
      };
    case "UPDATE_TAB_VPORTS":
      return {
        ...state,
        tabs: state.tabs.map(tab =>
          tab.id === action.id
            ? { ...tab, virtualVirtualEndpoints: action.pairs }
            : tab
        ),
      };
    case "SET_VPORT_ERROR":
      return {
        ...state,
        tabs: state.tabs.map(tab =>
          tab.id === action.id
            ? { ...tab, virtualPortError: action.error, virtualPortErrorKind: action.kind, virtualVirtualEndpoints: undefined }
            : tab
        ),
      };
    case "CLEAR_VPORT_ERROR":
      return {
        ...state,
        tabs: state.tabs.map(tab =>
          tab.id === action.id
            ? { ...tab, virtualPortError: undefined, virtualPortErrorKind: undefined }
            : tab
        ),
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
            tabs: remaining.map(t =>
              t.id === action.parentId ? { ...t, state: "disconnected" as ConnectionStatus } : t
            ),
            activeTabId: nextActive,
          };
        }
      }
      return { ...state, tabs: remaining, activeTabId: nextActive };
    }
    case "REMOVE_ALL_CHILDREN": {
      const childIds = new Set(
        state.tabs.filter(t => t.parentId === action.parentId).map(t => t.id)
      );
      return {
        ...state,
        tabs: state.tabs.filter(t => t.parentId !== action.parentId),
        activeTabId: state.activeTabId && childIds.has(state.activeTabId)
          ? action.parentId
          : state.activeTabId,
      };
    }
    case "CLEAR_TABS":
      return { ...state, tabs: [], activeTabId: null };
    case "SET_PEER_CONNECTED":
      return { ...state, peerSessions: { ...state.peerSessions, [action.id]: action.connected } };
    case "REMOVE_PEER": {
      const next = { ...state.peerSessions };
      delete next[action.id];
      return { ...state, peerSessions: next };
    }
    case "SET_NETWORK_PEER": {
      const list = state.networkPeers[action.containerId] ?? [];
      const ix = list.findIndex(p => p.peerId === action.peer.peerId);
      const nextList = ix >= 0
        ? list.map(p => p.peerId === action.peer.peerId ? { ...p, ...action.peer } : p)
        : [...list, action.peer];
      return { ...state, networkPeers: { ...state.networkPeers, [action.containerId]: nextList } };
    }
    case "SET_NETWORK_PEERS_BATCH": {
      const list = state.networkPeers[action.containerId] ?? [];
      const merged = [...list];
      for (const e of action.entries) {
        const ix = merged.findIndex(p => p.peerId === e.peerId);
        if (ix >= 0) merged[ix] = { ...merged[ix], ...e, txBytes: e.txBytes || merged[ix].txBytes, rxBytes: e.rxBytes || merged[ix].rxBytes };
        else merged.push(e);
      }
      return { ...state, networkPeers: { ...state.networkPeers, [action.containerId]: merged } };
    }
    case "SET_NETWORK_PEER_STATE": {
      const list = state.networkPeers[action.containerId] ?? [];
      return {
        ...state,
        networkPeers: {
          ...state.networkPeers,
          [action.containerId]: list.map(p =>
            p.peerId === action.peerId
              ? { ...p, state: action.state, txBytes: action.txBytes ?? p.txBytes, rxBytes: action.rxBytes ?? p.rxBytes }
              : p
          ),
        },
      };
    }
    case "SET_NETWORK_PEER_STATS": {
      const list = state.networkPeers[action.containerId] ?? [];
      return {
        ...state,
        networkPeers: {
          ...state.networkPeers,
          [action.containerId]: list.map(p =>
            p.peerId === action.peerId ? { ...p, txBytes: action.txBytes, rxBytes: action.rxBytes } : p
          ),
        },
      };
    }
    case "REMOVE_NETWORK_PEER": {
      const list = state.networkPeers[action.containerId] ?? [];
      return {
        ...state,
        networkPeers: {
          ...state.networkPeers,
          [action.containerId]: list.filter(p => p.peerId !== action.peerId),
        },
      };
    }
    case "CLEAR_NETWORK_PEERS": {
      const nextPeers = { ...state.networkPeers };
      delete nextPeers[action.containerId];
      const nextSel = { ...state.selectedNetworkPeer };
      delete nextSel[action.containerId];
      return { ...state, networkPeers: nextPeers, selectedNetworkPeer: nextSel };
    }
    case "SELECT_NETWORK_PEER":
      return { ...state, selectedNetworkPeer: { ...state.selectedNetworkPeer, [action.containerId]: action.peerId } };
    case "SET_NETWORK_MANUAL_TARGET":
      return { ...state, networkManualTarget: { ...state.networkManualTarget, [action.containerId]: action.target } };
    case "ADD_NETWORK_UDP_SOURCE": {
      const list = state.networkUdpSources[action.containerId] ?? [];
      if (list.includes(action.addr)) return state;
      const next = [...list, action.addr];
      if (next.length > 32) next.splice(0, next.length - 32);
      return { ...state, networkUdpSources: { ...state.networkUdpSources, [action.containerId]: next } };
    }
    case "SET_NETWORK_LOCAL_ADDR":
      return { ...state, networkLocalAddrs: { ...state.networkLocalAddrs, [action.containerId]: action.addr } };
    case "SET_NETWORK_BROADCAST":
      return { ...state, networkBroadcast: { ...state.networkBroadcast, [action.containerId]: action.on } };
    default:
      return state;
  }
}

// ── Context ──────────────────────────────────────────

interface SessionContextValue {
  state: SessionState;
  fetchConnectionTypes: () => Promise<void>;
  refreshEndpoints: (pluginId?: string, force?: boolean) => Promise<void>;
  connect: (opts: ConnectOptions) => Promise<string | null>;
  reconnectSession: (sessionId: string, initialElevated?: boolean) => Promise<string | null>;
  createOfflineSession: (endpoint: string, params: Record<string, unknown>, name?: string, pluginId?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean) => Promise<string | null>;
  disconnect: (sessionId: string, skipDisconnect?: boolean) => Promise<void>;
  deleteSession: (sessionId: string, skipDisconnect?: boolean) => Promise<void>;
  sendData: (sessionId: string, data: string | Uint8Array) => Promise<void>;
  switchTab: (sessionId: string | null) => Promise<void>;
  renameTab: (sessionId: string, name: string) => Promise<void>;
  reconfigureSession: (sessionId: string, endpoint: string, params: Record<string, unknown>, name?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean, pluginId?: string, journaldEnabled?: boolean) => Promise<void>;
  openChannel: (parentSessionId: string, elevated?: boolean) => Promise<string | null>;
  closeChannel: (channelId: string, parentId: string) => Promise<void>;
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
  const tabsRef = useRef(state.tabs);
  tabsRef.current = state.tabs;
  const stateRef = useRef(state);
  stateRef.current = state;
  const lastActiveChildRef = useRef<Map<string, string>>(new Map());
  const pendingEchoRef = useRef<Map<string, boolean>>(new Map());
  const endpointRefreshesRef = useRef<Map<string, Promise<EndpointInfo[]>>>(new Map());
  const endpointRefreshCompletedAtRef = useRef<Map<string, number>>(new Map());

  useEffect(() => {
    const activeId = state.activeTabId;
    if (!activeId) return;
    const activeTab = state.tabs.find(tab => tab.id === activeId);
    if (activeTab?.parentId) {
      lastActiveChildRef.current.set(activeTab.parentId, activeTab.id);
    }
  }, [state.activeTabId, state.tabs]);

  const [loggingSessions, setLoggingSessions] = useState<Set<string>>(new Set());
  const [logStatuses, setLogStatuses] = useState<Map<string, { fileName: string; bytesWritten: number }>>(new Map());

  const startSessionLog = useCallback(async (sessionId: string): Promise<string> => {
    try {
      await invoke<string>("start_session_log", { sessionId });
      setLoggingSessions(prev => new Set(prev).add(sessionId));
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

  const fetchConnectionTypes = useCallback(async () => {
    try {
      const types = await invoke<ConnectionTypeInfo[]>("get_connection_types");
      dispatch({ type: "SET_CONNECTION_TYPES", types });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `${e}` });
    }
  }, []);

  const refreshEndpoints = useCallback(async (pluginId?: string, force = false) => {
    const discoverableIds = pluginRegistry
      .getByCapability("endpoint_discovery")
      .map(plugin => plugin.manifest.id);
    const pluginIds = pluginId
      ? (discoverableIds.includes(pluginId) ? [pluginId] : [])
      : discoverableIds;

    await Promise.all(pluginIds.map(async (currentPluginId) => {
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
        dispatch({
          type: "REPLACE_ENDPOINTS_FOR_PLUGIN",
          pluginId: currentPluginId,
          endpoints,
        });
      } catch (e) {
        console.warn(`Endpoint discovery failed for ${currentPluginId}:`, e);
      } finally {
        if (endpointRefreshesRef.current.get(currentPluginId) === pending) {
          endpointRefreshesRef.current.delete(currentPluginId);
        }
      }
    }));
  }, []);

  const connect = useCallback(async (opts: ConnectOptions) => {
    const { endpoint, params, name, pluginId, transferEnabled, transferProtocol, sendBarEnabled, journaldEnabled, sessionId, initialElevated } = opts;
    const effectivePluginId = pluginId || "serial";
    const effectiveSendBarEnabled = pluginRegistry.resolveSendBarEnabled(effectivePluginId, sendBarEnabled);
    dispatch({ type: "SET_ERROR", error: null });
    if (sessionId) {
      dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "connecting" });
    }
    try {
      const sid = await invoke<string>("connect_session", {
        request: {
        endpoint, params, name,
        pluginId: effectivePluginId,
        transferEnabled: transferEnabled ?? true,
        transferProtocol: transferProtocol || "ymodem",
        sendBarEnabled: effectiveSendBarEnabled,
        journaldEnabled: journaldEnabled ?? false,
        sessionId: sessionId || null,
        initialElevated: initialElevated ?? false,

        },});
      return sid;
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `${i18n.t("localShell.connectFailed")}: ${localizeSessionError(e)}` });
      if (sessionId) {
        dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" });
      }
      return null;
    }
  }, []);

  const reconnectSession = useCallback(async (
    sessionId: string,
    initialElevated = false,
  ): Promise<string | null> => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    if (!tab || tab.state !== "disconnected" || !tab.params) return null;

    const params = tab.params as Record<string, unknown>;
    if (tab.pluginId === "tftp") {
      const bindIp = String(params.listen_ip ?? "").trim().toLowerCase();
      const loopback = bindIp === "127.0.0.1" || bindIp === "::1" || bindIp === "localhost";
      if (
        !loopback
        && params.write_enabled === true
        && params.overwrite === true
        && params.exposure_confirmed !== true
      ) {
        dispatch({
          type: "SET_ERROR",
          error: i18n.t("tftp.exposureWarning", {
            defaultValue:
              "This TFTP server will accept remote writes and allow overwriting files from a non-loopback interface. Continue only on a trusted network.",
          }),
        });
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
      journaldEnabled: tab.journaldEnabled,
      sessionId: tab.id,
      initialElevated,
    });
  }, [connect]);

  const createOfflineSession = useCallback(async (endpoint: string, params: Record<string, unknown>, name?: string, pluginId?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean) => {
    dispatch({ type: "SET_ERROR", error: null });
    try {
      const pid = pluginId || "serial";
      const plugin = pluginRegistry.get(pid);
      const effectiveSendBarEnabled = pluginRegistry.resolveSendBarEnabled(pid, sendBarEnabled);
      const pluginName = plugin?.manifest.name || pid.toUpperCase();
      const requestedName = name?.trim();
      const defaultName = plugin?.sessionPresentation?.defaultName?.(params, endpoint)?.trim();
      const effectiveName = requestedName || (pid === "local-shell"
        ? await invoke<string>("resolve_local_shell_session_name", { params })
        : defaultName || `${pluginName} @ ${endpoint}`);
      const sessionId = await invoke<string>("save_session_config", {
        request: {
        endpoint, params,
        name: effectiveName,
        pluginId: pid,
        transferEnabled: transferEnabled ?? true,
        transferProtocol: transferProtocol || "ymodem",
        sendBarEnabled: effectiveSendBarEnabled,

        },});
      const persistedParams = persistedSessionParams(pid, sessionId, params);
      dispatch({
        type: "ADD_TAB",
        tab: {
          id: sessionId,
          name: effectiveName,
          connection_type: pid,
          endpoint,
          state: "disconnected",
          pluginId: pid,
          params: persistedParams,
          stats: { txBytes: 0, rxBytes: 0 },
          connectedAt: null,
          transferEnabled: transferEnabled ?? true,
          transferProtocol,
          sendBarEnabled: effectiveSendBarEnabled,
          virtualPortEnabled: (params.virtual_port_enabled as boolean) ?? false,
          virtualPortCount: (params.virtual_port_count as number) ?? 0,
          fileServiceEnabled: (params.file_service_enabled as boolean) ?? false,
          fileServiceProtocol: params.file_service_protocol as string | undefined,
          journaldEnabled: (params.journald_enabled as boolean) ?? false,
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
    if (tab?.state === "disconnected") {
      return;
    }
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
    releaseSessionStore(sessionId);
  }, [state.tabs]);

  const sendData = useCallback(async (sessionId: string, data: string | Uint8Array) => {
    const tab = tabsRef.current.find(t => t.id === sessionId);
    const isPeer = peerSessionsRef.current[sessionId] === true;
    if ((!tab || tab.state === "disconnected") && !isPeer) return;
    try {
      const isText = typeof data === "string";
      const bytes = isText ? new TextEncoder().encode(data) : data;
      const written = await invoke<number[]>("write_data", { sessionId, data: Array.from(bytes), transcode: isText });
      sentDataCallbackRef.current?.(sessionId, new Uint8Array(written));
      sentDataSubscribersRef.current.forEach(cb => cb(sessionId, new Uint8Array(written)));
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `发送失败: ${e}` });
    }
  }, []);

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
      const supportsMultiple = pluginRegistry
        .get(targetTab.pluginId)?.manifest.capabilities.includes("multi_session") ?? false;
      if (supportsMultiple) {
        const children = tabs.filter(tab => tab.parentId === targetTab.id);
        if (children.length > 0) {
          const rememberedId = lastActiveChildRef.current.get(targetTab.id);
          const remembered = rememberedId
            ? children.find(child => child.id === rememberedId)
            : undefined;
          resolvedSessionId = remembered?.id ?? children[0].id;
          lastActiveChildRef.current.set(targetTab.id, resolvedSessionId);
        }
      }
    }

    dispatch({ type: "SET_ACTIVE", id: resolvedSessionId });
    try {
      await invoke("switch_active_session", { sessionId: resolvedSessionId });
    } catch (_e) {
    }
  }, []);

  const renameTab = useCallback(async (sessionId: string, name: string) => {
    const previousName = tabsRef.current.find(tab => tab.id === sessionId)?.name;
    dispatch({ type: "RENAME_TAB", id: sessionId, name });
    try {
      await invoke("rename_session", { sessionId, newName: name });
    } catch (e) {
      if (previousName !== undefined) {
        dispatch({ type: "RENAME_TAB", id: sessionId, name: previousName });
      }
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
    journaldEnabled?: boolean,
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
    const effectiveSendBarEnabled = pluginRegistry.resolveSendBarEnabled(effectivePluginId, sendBarEnabled);
    try {
      await invoke("save_session_config", {
        request: {
        endpoint,
        params,
        // 配置更新默认保留创建时名称；只有用户显式修改名称字段时才会变化。
        name: effectiveName,
        pluginId: effectivePluginId,
        transferEnabled: transferEnabled ?? true,
        transferProtocol: transferProtocol || "ymodem",
        sendBarEnabled: effectiveSendBarEnabled,
        sessionId,

        },});
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `保存配置失败: ${e}` });
      return;
    }

    const persistedParams = persistedSessionParams(effectivePluginId, sessionId, params);

    dispatch({
      type: "UPDATE_TAB_CONFIG",
      id: sessionId,
      endpoint,
      params: persistedParams,
      name: effectiveName,
      transferEnabled,
      transferProtocol,
      sendBarEnabled: effectiveSendBarEnabled,
      pluginId: effectivePluginId,
      journaldEnabled: journaldEnabled ?? (params?.journald_enabled as boolean) ?? tab?.journaldEnabled,
      fileServiceEnabled: (params?.file_service_enabled as boolean) ?? tab?.fileServiceEnabled,
      fileServiceProtocol: (params?.file_service_protocol as string) ?? tab?.fileServiceProtocol,
    });

    if (wasConnected) {
      try {
        const newSessionId = await invoke<string>("connect_session", {
        request: {
          endpoint,
          params: persistedParams,
          name: effectiveName,
          pluginId: effectivePluginId,
          transferEnabled: transferEnabled ?? true,
          transferProtocol: transferProtocol || "ymodem",
          sendBarEnabled: effectiveSendBarEnabled,
          journaldEnabled: (persistedParams?.journald_enabled as boolean) ?? tab?.journaldEnabled ?? false,
          sessionId,

        },});
        dispatch({ type: "SET_TAB_STATE", id: newSessionId, state: "connected" });
      } catch (e) {
        dispatch({ type: "SET_ERROR", error: `重连失败: ${e}` });
      }
    }
  }, [state.tabs]);

  const openChannel = useCallback(async (parentSessionId: string, elevated = false): Promise<string | null> => {
    try {
      const channelId = await invoke<string>("open_channel", {
        sessionId: parentSessionId,
        elevated,
      });
      return channelId;
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `${i18n.t("localShell.openFailed")}: ${localizeSessionError(e)}` });
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
        plugin_id?: string;
        transfer_enabled?: boolean;
        transfer_protocol?: string;
        send_bar_enabled?: boolean;
        virtual_port_enabled?: boolean;
        virtual_port_count?: number;
      }>>("load_sessions");
      if (saved && saved.length > 0) {
        const tabs: TabInfo[] = saved.map((s) => {
          const pluginId = s.plugin_id || "serial";
          return {
            id: s.id,
            name: s.name,
            connection_type: s.connection_type,
            endpoint: s.endpoint,
            state: "disconnected" as ConnectionStatus,
            pluginId,
            params: s.params,
            stats: { txBytes: 0, rxBytes: 0 },
            connectedAt: null,
            transferEnabled: s.transfer_enabled ?? true,
            transferProtocol: s.transfer_protocol,
            sendBarEnabled: pluginRegistry.resolveSendBarEnabled(pluginId, s.send_bar_enabled),
            virtualPortEnabled: s.virtual_port_enabled ?? false,
            virtualPortCount: s.virtual_port_count ?? 0,
            fileServiceEnabled: (s.params?.file_service_enabled as boolean) ?? false,
            fileServiceProtocol: s.params?.file_service_protocol as string | undefined,
            journaldEnabled: (s.params?.journald_enabled as boolean) ?? false,
          };
        });
        dispatch({ type: "SET_TABS", tabs });
        if (tabs.length > 0) {
          dispatch({ type: "SET_ACTIVE", id: tabs[0].id });
        }
      }
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

  const onSessionData = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => {
    dataCallbackRef.current = callback;
  }, []);

  const onDataSent = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => {
    sentDataCallbackRef.current = callback;
  }, []);

  const subscribeDataSent = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => {
    sentDataSubscribersRef.current.add(callback);
    return () => { sentDataSubscribersRef.current.delete(callback); };
  }, []);

  const isSessionConnected = useCallback((sessionId: string): boolean => {
    const tab = tabsRef.current.find(t => t.id === sessionId);
    if (tab) return tab.state === "connected" || tab.state === "transferring";
    return peerSessionsRef.current[sessionId] === true;
  }, []);

  const selectNetworkPeer = useCallback((containerId: string, peerId: string | null) => {
    dispatch({ type: "SELECT_NETWORK_PEER", containerId, peerId });
  }, []);

  const getNetworkPeers = useCallback((containerId: string): NetworkPeerEntry[] => {
    return stateRef.current.networkPeers[containerId] ?? [];
  }, []);

  const disconnectNetworkPeer = useCallback(async (containerId: string, peerId: string) => {
    try {
      await invoke("close_network_peer", { sessionId: peerId });
    } catch (e) {
      console.error("网络调试: 断开对端失败:", e);
    }
    dispatch({ type: "SET_NETWORK_PEER_STATE", containerId, peerId, state: "disconnected" });
    peerSessionsRef.current[peerId] = false;
    dispatch({ type: "SET_PEER_CONNECTED", id: peerId, connected: false });
  }, []);

  const clearNetworkPeer = useCallback(async (containerId: string, peerId: string) => {
    try {
      await invoke("close_network_peer", { sessionId: peerId }).catch(() => {
      });
    } catch (_e) { }
    delete peerSessionsRef.current[peerId];
    if (networkPeerContainerRef.current[peerId] === containerId) {
      delete networkPeerContainerRef.current[peerId];
    }
    dispatch({ type: "REMOVE_NETWORK_PEER", containerId, peerId });
    dispatch({ type: "REMOVE_PEER", id: peerId });
  }, []);

  const mergeNetworkPeers = useCallback((containerId: string, entries: NetworkPeerEntry[]) => {
    dispatch({ type: "SET_NETWORK_PEERS_BATCH", containerId, entries });
  }, []);

  const setNetworkManualTarget = useCallback((containerId: string, target: string) => {
    dispatch({ type: "SET_NETWORK_MANUAL_TARGET", containerId, target });
  }, []);

  const registerNetworkUdpSource = useCallback((containerId: string, addr: string) => {
    dispatch({ type: "ADD_NETWORK_UDP_SOURCE", containerId, addr });
  }, []);

  const subscribeNetworkManualSent = useCallback((
    callback: (containerId: string, target: string, bytes: Uint8Array) => void,
  ) => {
    networkManualSentSubscribersRef.current.add(callback);
    return () => { networkManualSentSubscribersRef.current.delete(callback); };
  }, []);

  const setNetworkBroadcast = useCallback((containerId: string, on: boolean) => {
    dispatch({ type: "SET_NETWORK_BROADCAST", containerId, on });
  }, []);

  const sendToTarget = useCallback(async (containerId: string, data: string | Uint8Array) => {
    const tab = tabsRef.current.find(t => t.id === containerId);
    const params = (tab?.params ?? {}) as Record<string, unknown>;
    const transport = params.transport as string | undefined;
    if (transport !== "tcp" && transport !== "udp") {
      await sendData(containerId, data);
      return;
    }

    const role = (params.role as string | undefined) ?? "client";
    const isText = typeof data === "string";
    const bytes = isText ? new TextEncoder().encode(data) : data;
    const byteArr = Array.from(bytes);
    const transcode = isText;

    if (transport === "udp") {
      if (role === "server") {
        const target = (stateRef.current.networkManualTarget[containerId] ?? "").trim();
        if (!target) throw new Error("无可用发送目标");
        const written = await invoke<number[]>("network_udp_send_to", {
          sessionId: containerId, targetAddr: target, data: byteArr, transcode,
        });
        networkManualSentSubscribersRef.current.forEach(cb => cb(containerId, target, new Uint8Array(written)));
      } else {
        const written = await invoke<number[]>("network_udp_send", {
          sessionId: containerId, data: byteArr, transcode,
        });
        const remote = `${params.remote_host ?? "127.0.0.1"}:${params.remote_port ?? 0}`;
        networkManualSentSubscribersRef.current.forEach(cb => cb(containerId, remote, new Uint8Array(written)));
      }
      return;
    }

    const peers = (stateRef.current.networkPeers[containerId] ?? [])
      .filter(p => p.state === "connected");
    if (stateRef.current.networkBroadcast[containerId] === true) {
      for (const p of peers) {
        await sendData(p.peerId, data);
      }
      return;
    }
    const selected = stateRef.current.selectedNetworkPeer[containerId];
    const peer = peers.find(p => p.peerId === selected);
    if (peer) { await sendData(peer.peerId, data); return; }
    if (peers.length === 1) { await sendData(peers[0].peerId, data); return; }
    throw new Error("无可用发送目标");
  }, [sendData]);

  const updateSessionStats = useCallback((sessionId: string, txBytes: number, rxBytes: number, rxPackets?: number, txPackets?: number) => {
    dispatch({
      type: "UPDATE_TAB_STATS",
      id: sessionId,
      stats: {
        txBytes,
        rxBytes,
        ...(rxPackets !== undefined ? { rxPackets } : {}),
        ...(txPackets !== undefined ? { txPackets } : {}),
      },
      connectedAt: undefined,
    });
  }, []);

  const onSessionDisconnect = useCallback((callback: (sessionId: string, reason?: string) => void) => {
    disconnectCallbackRef.current = callback;
  }, []);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    (async () => {
      const u1 = await listen<{ session_id: string; data_b64?: string; data?: number[] }>("session-data", (event) => {
        const data = event.payload.data_b64
          ? decodeBase64(event.payload.data_b64)
          : new Uint8Array(event.payload.data ?? []);
        dataCallbackRef.current?.(event.payload.session_id, data);
      });
      if (cancelled) { u1(); return; }
      unlisteners.push(u1);

      const u1b = await listen<{ session_id: string; local_echo: boolean }>(
        "telnet-echo-state",
        (event) => {
          const sid = event.payload.session_id;
          if (!tabsRef.current.some(t => t.id === sid)) {
            pendingEchoRef.current.set(sid, event.payload.local_echo);
            return;
          }
          dispatch({
            type: "UPDATE_TAB_ECHO",
            id: sid,
            localEcho: event.payload.local_echo,
          });
        }
      );
      if (cancelled) { u1b(); return; }
      unlisteners.push(u1b);

      const u2 = await listen<{ session_id: string; endpoint: string; connection_type: string; plugin_id?: string; name: string; params: Record<string, unknown>; connected_at?: number | null; transfer_enabled?: boolean; transfer_protocol?: string; send_bar_enabled?: boolean; virtual_endpoints?: VirtualPortEndpoint[]; file_service_enabled?: boolean; file_service_protocol?: string; journald_enabled?: boolean; parent_id?: string | null; channel_index?: number; elevated?: boolean; is_container?: boolean; local_addr?: string | null }>(
        "session-connected",
        (event) => {
          const sid = event.payload.session_id;
          const eventPluginId = event.payload.plugin_id || event.payload.connection_type || "serial";
          const eventSendBarEnabled = pluginRegistry.resolveSendBarEnabled(eventPluginId, event.payload.send_bar_enabled);
          const vPairs = event.payload.virtual_endpoints;
          const parentId = event.payload.parent_id ?? null;
          const isContainer = event.payload.is_container ?? false;
          if (typeof event.payload.local_addr === "string") {
            dispatch({ type: "SET_NETWORK_LOCAL_ADDR", containerId: sid, addr: event.payload.local_addr });
          }
          const existingTab = tabsRef.current.find(t => t.id === sid);
          if (existingTab) {
            dispatch({ type: "SET_TAB_STATE", id: sid, state: "connected" });
            dispatch({
              type: "UPDATE_TAB_CONFIG",
              id: sid,
              endpoint: event.payload.endpoint,
              params: event.payload.params,
              // 运行时连接事件只刷新配置/状态，绝不重新生成已保存会话的身份名称。
              name: existingTab.name,
              transferEnabled: event.payload.transfer_enabled,
              transferProtocol: event.payload.transfer_protocol,
              sendBarEnabled: eventSendBarEnabled,
              pluginId: eventPluginId,
              connectedAt: event.payload.connected_at ?? Date.now(),
              journaldEnabled: event.payload.journald_enabled ?? false,
              fileServiceEnabled: event.payload.file_service_enabled ?? (event.payload.params?.file_service_enabled as boolean),
              fileServiceProtocol: event.payload.file_service_protocol ?? (event.payload.params?.file_service_protocol as string),
            });
            const pendingEcho = pendingEchoRef.current.get(sid);
            if (pendingEcho !== undefined) {
              pendingEchoRef.current.delete(sid);
              dispatch({ type: "UPDATE_TAB_ECHO", id: sid, localEcho: pendingEcho });
            }
            if (vPairs && vPairs.length > 0) {
              dispatch({ type: "UPDATE_TAB_VPORTS", id: sid, pairs: vPairs });
            }
          } else if (parentId) {
            const chName = event.payload.name;
            dispatch({ type: "SET_TAB_STATE", id: parentId, state: "connected" });
            dispatch({
              type: "ADD_TAB",
              tab: {
                id: sid,
                name: chName,
                connection_type: event.payload.connection_type,
                endpoint: event.payload.endpoint,
                state: "connected",
                pluginId: eventPluginId,
                params: event.payload.params,
                stats: { txBytes: 0, rxBytes: 0 },
                connectedAt: event.payload.connected_at ?? Date.now(),
                transferEnabled: event.payload.transfer_enabled ?? false,
                transferProtocol: event.payload.transfer_protocol,
                sendBarEnabled: eventSendBarEnabled,
                parentId,
                channelIndex: event.payload.channel_index,
                elevated: event.payload.elevated ?? false,
                fileServiceEnabled: event.payload.file_service_enabled ?? (event.payload.params?.file_service_enabled as boolean) ?? false,
                fileServiceProtocol: event.payload.file_service_protocol ?? (event.payload.params?.file_service_protocol as string),
                journaldEnabled: event.payload.journald_enabled ?? (event.payload.params?.journald_enabled as boolean) ?? false,
              },
            });
          } else if (isContainer) {
            dispatch({
              type: "ADD_TAB",
              tab: {
                id: sid,
                name: event.payload.name,
                connection_type: event.payload.connection_type,
                endpoint: event.payload.endpoint,
                state: "connected",
                pluginId: eventPluginId,
                params: event.payload.params,
                stats: { txBytes: 0, rxBytes: 0 },
                connectedAt: event.payload.connected_at ?? Date.now(),
                transferEnabled: event.payload.transfer_enabled ?? false,
                transferProtocol: event.payload.transfer_protocol,
                sendBarEnabled: eventSendBarEnabled,
                isContainer: true,
                fileServiceEnabled: event.payload.file_service_enabled ?? false,
                fileServiceProtocol: event.payload.file_service_protocol,
                journaldEnabled: event.payload.journald_enabled ?? false,
              },
            });
          } else {
            const pendingEcho = pendingEchoRef.current.get(sid);
            if (pendingEcho !== undefined) {
              pendingEchoRef.current.delete(sid);
            }
            dispatch({
              type: "ADD_TAB",
              tab: {
                id: sid,
                name: event.payload.name || `${(event.payload.plugin_id && pluginRegistry.get(event.payload.plugin_id)?.manifest.name) || event.payload.plugin_id?.toUpperCase() || "Serial"} @ ${event.payload.endpoint}`,
                connection_type: event.payload.connection_type,
                endpoint: event.payload.endpoint,
                state: "connected",
                pluginId: eventPluginId,
                localEcho: pendingEcho,
                params: event.payload.params,
                stats: { txBytes: 0, rxBytes: 0 },
                connectedAt: event.payload.connected_at ?? Date.now(),
                transferEnabled: event.payload.transfer_enabled ?? true,
                transferProtocol: event.payload.transfer_protocol,
                sendBarEnabled: eventSendBarEnabled,
                virtualVirtualEndpoints: vPairs,
                virtualPortEnabled: (event.payload.params?.virtual_port_enabled as boolean) ?? false,
                virtualPortCount: (event.payload.params?.virtual_port_count as number) ?? 0,
                fileServiceEnabled: event.payload.file_service_enabled ?? (event.payload.params?.file_service_enabled as boolean) ?? false,
                fileServiceProtocol: event.payload.file_service_protocol ?? (event.payload.params?.file_service_protocol as string),
                journaldEnabled: event.payload.journald_enabled ?? (event.payload.params?.journald_enabled as boolean) ?? false,
              },
            });
          }
        }
      );
      if (cancelled) { u2(); return; }
      unlisteners.push(u2);

      const u2b = await listen<{ session_id: string; endpoints: VirtualPortEndpoint[] }>(
        "virtual-port-created",
        (event) => {
          dispatch({
            type: "UPDATE_TAB_VPORTS",
            id: event.payload.session_id,
            pairs: event.payload.endpoints,
          });
        }
      );
      if (cancelled) { u2b(); return; }
      unlisteners.push(u2b);

      const u2c = await listen<{ session_id: string; kind?: string; reason: string }>(
        "virtual-port-failed",
        (event) => {
          console.warn(`[VirtualPort] ${event.payload.session_id}: ${event.payload.reason}`);
          dispatch({
            type: "SET_VPORT_ERROR",
            id: event.payload.session_id,
            error: event.payload.reason,
            kind: event.payload.kind,
          });
        }
      );
      if (cancelled) { u2c(); return; }
      unlisteners.push(u2c);

      const u2d = await listen("virtual-port-driver-ready", () => {
        tabsRef.current.forEach((tab: { id: string; virtualPortError?: string }) => {
          if (tab.virtualPortError) {
            dispatch({ type: "CLEAR_VPORT_ERROR", id: tab.id });
          }
        });
      });
      if (cancelled) { u2d(); return; }
      unlisteners.push(u2d);

      const u2e = await listen<{ channel_id: string; parent_id: string; disconnect_info?: DisconnectInfo }>("channel-closed", (event) => {
        if (event.payload.disconnect_info?.retain_terminal) {
          dispatch({ type: "SET_TAB_DISCONNECTED", id: event.payload.channel_id, info: event.payload.disconnect_info });
        } else {
          dispatch({ type: "REMOVE_CHILD", id: event.payload.channel_id, parentId: event.payload.parent_id });
        }
      });
      if (cancelled) { u2e(); return; }
      unlisteners.push(u2e);

      const u2f = await listen<{ session_id: string; peer_id: string; peer_name: string; peer_addr: string; local_addr?: string }>(
        "netdbg-peer-joined",
        (event) => {
          const { session_id: cid, peer_id, peer_name, peer_addr, local_addr } = event.payload;
          peerSessionsRef.current[peer_id] = true;
          networkPeerContainerRef.current[peer_id] = cid;
          dispatch({ type: "SET_PEER_CONNECTED", id: peer_id, connected: true });
          dispatch({
            type: "SET_NETWORK_PEER",
            containerId: cid,
            peer: { peerId: peer_id, name: peer_name, addr: peer_addr, localAddr: local_addr, state: "connected", txBytes: 0, rxBytes: 0 },
          });
        }
      );
      if (cancelled) { u2f(); return; }
      unlisteners.push(u2f);

      const u2g = await listen<{ session_id: string; peer_id: string; tx_bytes?: number | null; rx_bytes?: number | null }>(
        "netdbg-peer-left",
        (event) => {
          const { session_id: cid, peer_id, tx_bytes, rx_bytes } = event.payload;
          const containerTab = tabsRef.current.find(t => t.id === cid);
          const isNetClient = containerTab?.pluginId === "network"
            && ((containerTab.params as Record<string, unknown> | undefined)?.role ?? "client") === "client";
          if (isNetClient) {
            invoke("disconnect_session", { sessionId: cid }).catch(() => { });
            dispatch({ type: "SET_TAB_STATE", id: cid, state: "disconnected" });
            return;
          }
          peerSessionsRef.current[peer_id] = false;
          dispatch({ type: "SET_PEER_CONNECTED", id: peer_id, connected: false });
          dispatch({
            type: "SET_NETWORK_PEER_STATE",
            containerId: cid,
            peerId: peer_id,
            state: "disconnected",
            txBytes: typeof tx_bytes === "number" ? tx_bytes : undefined,
            rxBytes: typeof rx_bytes === "number" ? rx_bytes : undefined,
          });
        }
      );
      if (cancelled) { u2g(); return; }
      unlisteners.push(u2g);

      const u3 = await listen<{ session_id: string; reason?: string; disconnect_info?: DisconnectInfo }>("session-disconnected", (event) => {
        const reason = event.payload.reason;
        const sid = event.payload.session_id;
        dispatch({ type: "SET_TAB_DISCONNECTED", id: sid, info: event.payload.disconnect_info });
        const peers = stateRef.current.networkPeers[sid];
        if (peers) {
          for (const p of peers) {
            delete peerSessionsRef.current[p.peerId];
            if (networkPeerContainerRef.current[p.peerId] === sid) {
              delete networkPeerContainerRef.current[p.peerId];
            }
            dispatch({ type: "REMOVE_PEER", id: p.peerId });
          }
          dispatch({ type: "CLEAR_NETWORK_PEERS", containerId: sid });
        }
        dispatch({ type: "UPDATE_TAB_ECHO", id: sid, localEcho: false });
        if (!event.payload.disconnect_info?.retain_terminal) {
          dispatch({ type: "REMOVE_ALL_CHILDREN", parentId: sid });
        }
        dispatch({ type: "UPDATE_TAB_VPORTS", id: sid, pairs: [] });
        disconnectCallbackRef.current?.(sid, reason);
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

      const u4 = await listen<{ session_id: string }>("file-transfer:started", (event) => {
        dispatch({ type: "SET_TAB_STATE", id: event.payload.session_id, state: "transferring" });
      });
      if (cancelled) { u4(); return; }
      unlisteners.push(u4);

      const u5 = await listen<{ session_id: string; success: boolean }>("file-transfer:finished", (event) => {
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
          const cid = networkPeerContainerRef.current[event.payload.tab_id];
          if (cid) {
            dispatch({
              type: "SET_NETWORK_PEER_STATS",
              containerId: cid,
              peerId: event.payload.tab_id,
              txBytes: event.payload.tx_bytes,
              rxBytes: event.payload.rx_bytes,
            });
            return;
          }
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

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    void listen<{ session_id: string; dropped_chunks: number }>(
      "session-display-overflow",
      event => {
        if (disposed) return;
        const tab = tabsRef.current.find(item => item.id === event.payload.session_id);
        const label = tab?.name ?? event.payload.session_id;
        dispatch({
          type: "SET_ERROR",
          error: i18n.t("session.displayOverflow", {
            defaultValue: "Display overflow in {{session}} — {{count}} chunks were dropped from the presentation path.",
            session: label,
            count: event.payload.dropped_chunks,
          }),
        });
      },
    ).then(fn => {
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
        const statuses: Array<{ session_id: string; file_name: string; bytes_written: number }> =
          await invoke("get_log_status");
        setLogStatuses(new Map(statuses.map(s => [s.session_id, { fileName: s.file_name, bytesWritten: s.bytes_written }])));
        setLoggingSessions(new Set(statuses.map(s => s.session_id)));
      } catch (_e) {
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
      selectNetworkPeer,
      getNetworkPeers,
      disconnectNetworkPeer,
      clearNetworkPeer,
      mergeNetworkPeers,
      setNetworkManualTarget,
      registerNetworkUdpSource,
      subscribeNetworkManualSent,
      setNetworkBroadcast,
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
