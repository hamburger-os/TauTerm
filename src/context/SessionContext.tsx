import { createContext, useContext, useReducer, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { pluginRegistry } from "../core/plugin-registry";
import { releaseSessionStore } from "../hooks/usePluginSessionStore";

// ── Types ───────────────────────────────────────────

export type ConnectionStatus = "disconnected" | "connecting" | "connected" | "transferring";

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
  virtualPortPairs?: Array<{ port_a: string; port_b: string }>;
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
  | { type: "UPDATE_TAB_STATS"; id: string; stats: SessionStats; connectedAt?: number | null }
  | { type: "UPDATE_TAB_ECHO"; id: string; localEcho: boolean }
  | { type: "UPDATE_TAB_CONFIG"; id: string; endpoint: string; params: Record<string, unknown>; name: string; transferEnabled?: boolean; transferProtocol?: string; sendBarEnabled?: boolean; pluginId?: string; connectedAt?: number | null; journaldEnabled?: boolean; fileServiceEnabled?: boolean; fileServiceProtocol?: string }
  | { type: "UPDATE_TAB_VPORTS"; id: string; pairs: Array<{ port_a: string; port_b: string }> }
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
      // 始终追加新标签页（即使是同一端口），用户可通过右键菜单删除旧标签页
      return {
        ...state,
        tabs: [...state.tabs, action.tab],
        activeTabId: action.tab.id,
      };
    }
    case "REMOVE_TAB": {
      // 级联删除所有子 channel
      const childIds = state.tabs
        .filter(t => t.parentId === action.id)
        .map(t => t.id);
      const allRemoved = new Set([action.id, ...childIds]);
      const remaining = state.tabs.filter(t => !allRemoved.has(t.id));
      // 选择下一活跃 tab：优先相邻根节点
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
            ? { ...tab, virtualPortPairs: action.pairs }
            : tab
        ),
      };
    case "SET_VPORT_ERROR":
      return {
        ...state,
        tabs: state.tabs.map(tab =>
          tab.id === action.id
            ? { ...tab, virtualPortError: action.error, virtualPortErrorKind: action.kind, virtualPortPairs: undefined }
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
        // 优先切换到同一父会话的其他子 channel，否则切换到父会话
        const siblings = remaining.filter(t => t.parentId === action.parentId);
        nextActive = siblings[0]?.id ?? action.parentId ?? remaining.find(t => !t.parentId)?.id ?? null;
      }
      // 如果删除后父会话没有子 channel 了，断开父会话
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
    case "REMOVE_ALL_CHILDREN":
      return {
        ...state,
        tabs: state.tabs.filter(t => t.parentId !== action.parentId),
      };
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
  refreshEndpoints: () => Promise<void>;
  connect: (opts: ConnectOptions) => Promise<string | null>;
  createOfflineSession: (endpoint: string, params: Record<string, unknown>, name?: string, pluginId?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean) => Promise<string | null>;
  disconnect: (sessionId: string) => Promise<void>;
  deleteSession: (sessionId: string, skipDisconnect?: boolean) => Promise<void>;
  sendData: (sessionId: string, data: string | Uint8Array) => Promise<void>;
  switchTab: (sessionId: string) => Promise<void>;
  renameTab: (sessionId: string, name: string) => Promise<void>;
  reconfigureSession: (sessionId: string, endpoint: string, params: Record<string, unknown>, name?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean, pluginId?: string, journaldEnabled?: boolean) => Promise<void>;
  /** 在已有 SSH 会话上打开新 channel */
  openChannel: (parentSessionId: string) => Promise<string | null>;
  /** 关闭单个子 channel */
  closeChannel: (channelId: string, parentId: string) => Promise<void>;
  getTabs: () => Promise<void>;
  onSessionData: (callback: (sessionId: string, data: Uint8Array) => void) => void;
  onDataSent: (callback: (sessionId: string, data: Uint8Array) => void) => void;
  /** 订阅 TX 通知（多监听者；网络调试对端 TX 显示用），返回取消订阅函数 */
  subscribeDataSent: (callback: (sessionId: string, data: Uint8Array) => void) => () => void;
  /** 判定会话或对端是否处于连接态（SendBar 面板共用；对端经 peerSessions 注册） */
  isSessionConnected: (sessionId: string) => boolean;
  /** 网络调试：选中容器会话的对端（null = 取消选中；client 模式自动选中唯一对端） */
  selectNetworkPeer: (containerId: string, peerId: string | null) => void;
  /** 网络调试：读取容器会话的当前对端列表（读 stateRef，非活跃会话也始终新鲜；数据监听路由用） */
  getNetworkPeers: (containerId: string) => NetworkPeerEntry[];
  /** 网络调试：断开指定对端（后端 close_network_peer + 乐观置灰） */
  disconnectNetworkPeer: (containerId: string, peerId: string) => Promise<void>;
  /** 网络调试：移除已关闭对端墓碑（后端真实释放 + 前端清列表） */
  clearNetworkPeer: (containerId: string, peerId: string) => Promise<void>;
  /** 网络调试：按后端快照合并对端列表（getStatus 兜底，保留既有统计） */
  mergeNetworkPeers: (containerId: string, entries: NetworkPeerEntry[]) => void;
  /** 网络调试：设置 UDP 手动目标地址（目标栏手动目标输入） */
  setNetworkManualTarget: (containerId: string, target: string) => void;
  /** 网络调试：记录一个 UDP RX 来源地址（发送栏快捷回发用，去重 + 上限） */
  registerNetworkUdpSource: (containerId: string, addr: string) => void;
  /** 订阅 UDP 手动目标发送（报文网格 TX 行用），返回取消订阅函数 */
  subscribeNetworkManualSent: (callback: (containerId: string, target: string, bytes: Uint8Array) => void) => () => void;
  /** 网络调试：切换容器会话的「全部客户端」群发目标 */
  setNetworkBroadcast: (containerId: string, on: boolean) => void;
  /**
   * 统一发送路由：网络容器按「当前目标」（选中对端 / 全部 / 手动地址）路由，
   * 非网络会话走默认 sendData(sessionId)。基本发送与指令面板共用。
   */
  sendToTarget: (containerId: string, data: string | Uint8Array) => Promise<void>;
  /** 更新指定会话的 I/O 统计（网络调试容器汇总对端统计到状态栏用） */
  updateSessionStats: (sessionId: string, txBytes: number, rxBytes: number, rxPackets?: number, txPackets?: number) => void;
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
  /** 多监听者 TX 通知（网络调试对端视图订阅；单槽 sentDataCallbackRef 保留给终端） */
  const sentDataSubscribersRef = useRef<Set<(sessionId: string, data: Uint8Array) => void>>(new Set());
  /** 对端注册表 Ref 镜像（sendData 闭包读取；state.peerSessions 提供响应式渲染） */
  const peerSessionsRef = useRef<Record<string, boolean>>({});
  /** 对端 → 所属容器会话的反向映射（session-stats 路由用；netdbg 事件维护） */
  const networkPeerContainerRef = useRef<Record<string, string>>({});
  /** UDP 手动目标发送订阅者（网络调试报文网格 TX 行用） */
  const networkManualSentSubscribersRef = useRef<Set<(containerId: string, target: string, bytes: Uint8Array) => void>>(new Set());
  const disconnectCallbackRef = useRef<((sessionId: string, reason?: string) => void) | null>(null);
  // 保持最新的 tabs 引用，供事件监听器（闭包中 state 可能过期）使用
  const tabsRef = useRef(state.tabs);
  tabsRef.current = state.tabs;
  // 完整 state 镜像（网络对端清理等全局监听器需读取最新 networkPeers）
  const stateRef = useRef(state);
  stateRef.current = state;
  // Telnet 回显状态暂存：telnet-echo-state 早于 session-connected 到达
  // （tab 尚未创建）时暂存于此，session-connected 创建/更新 tab 时取出
  // 初始化 localEcho，避免事件被静默丢弃导致输入不可见
  const pendingEchoRef = useRef<Map<string, boolean>>(new Map());

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

  const connect = useCallback(async (opts: ConnectOptions) => {
    const { endpoint, params, name, pluginId, transferEnabled, transferProtocol, sendBarEnabled, journaldEnabled, sessionId } = opts;
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
        request: {
        endpoint, params, name,
        pluginId: pluginId || "serial",
        transferEnabled: transferEnabled ?? true,
        transferProtocol: transferProtocol || "ymodem",
        sendBarEnabled: sendBarEnabled ?? true,
        journaldEnabled: journaldEnabled ?? false,
        sessionId: sessionId || null,

        },});
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
        request: {
        endpoint, params,
        name: effectiveName,
        pluginId: pid,
        transferEnabled: transferEnabled ?? true,
        transferProtocol: transferProtocol || "ymodem",
        sendBarEnabled: sendBarEnabled ?? true,

        },});
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
    // 释放插件会话 store 的全部资源（keepAlive 会话的 Tauri 监听器与
    // Map 条目按设计常驻，会话删除后不再有存续意义——不清理则永久泄漏）
    releaseSessionStore(sessionId);
  }, [state.tabs]);

  /**
   * 统一发送：将数据写入指定会话（文本按会话编码转码，字节原样透传）。
   */
  const sendData = useCallback(async (sessionId: string, data: string | Uint8Array) => {
    // 保护：连接已断开时不发送，避免触发后端 "sending on a closed channel" 错误。
    // 网络调试对端不在 tabs 中，通过 peerSessions 注册表判定连接态并放行。
    const tab = tabsRef.current.find(t => t.id === sessionId);
    const isPeer = peerSessionsRef.current[sessionId] === true;
    if ((!tab || tab.state === "disconnected") && !isPeer) return;
    try {
      // 文本路径（键盘 / SendBar 文本 / 脚本字符串）：UTF-8 字节交给后端按会话编码转码；
      // 字节路径（HEX 发送 / 脚本原始字节）：原样透传，不做字符转码
      const isText = typeof data === "string";
      const bytes = isText ? new TextEncoder().encode(data) : data;
      // 返回值为实际写入设备的字节：文本路径按会话编码转码（如 GBK），
      // 用作 TX 通知/日志时保证面板显示与线上字节一致
      const written = await invoke<number[]>("write_data", { sessionId, data: Array.from(bytes), transcode: isText });
      // 通知 Dual 模式终端：数据已发送
      sentDataCallbackRef.current?.(sessionId, new Uint8Array(written));
      // 多监听者 TX 通知（网络调试对端视图）
      sentDataSubscribersRef.current.forEach(cb => cb(sessionId, new Uint8Array(written)));
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `发送失败: ${e}` });
    }
  }, []);

  const switchTab = useCallback(async (sessionId: string) => {
    // 如果选中的是父 session，自动路由到第一个子 channel
    const targetTab = state.tabs.find(t => t.id === sessionId);
    if (targetTab && !targetTab.parentId) {
      // 这是一个根节点（父 session）
      const firstChild = state.tabs.find(t => t.parentId === sessionId);
      if (firstChild) {
        dispatch({ type: "SET_ACTIVE", id: firstChild.id });
        try { await invoke("switch_active_session", { sessionId: firstChild.id }); } catch (_e) {}
        return;
      }
    }
    dispatch({ type: "SET_ACTIVE", id: sessionId });
    try {
      await invoke("switch_active_session", { sessionId });
    } catch (_e) {
      // 恢复的会话在后端不存在，静默忽略
    }
  }, [state.tabs]);

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
    journaldEnabled?: boolean,
  ) => {
    const tab = state.tabs.find(t => t.id === sessionId);
    const wasConnected = tab?.state === "connected" || tab?.state === "transferring";

    // 1. 如果已连接，先清理子通道再断连
    if (wasConnected) {
      // 先清除前端子通道 UI 状态
      dispatch({ type: "REMOVE_ALL_CHILDREN", parentId: sessionId });
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
        request: {
        endpoint,
        params,
        name: name || undefined,
        pluginId: effectivePluginId,
        transferEnabled: transferEnabled ?? true,
        transferProtocol: transferProtocol || "ymodem",
        sendBarEnabled: sendBarEnabled ?? true,
        sessionId, // 复用已有 UUID

        },});
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
      journaldEnabled: journaldEnabled ?? (params?.journald_enabled as boolean) ?? tab?.journaldEnabled,
      fileServiceEnabled: (params?.file_service_enabled as boolean) ?? tab?.fileServiceEnabled,
      fileServiceProtocol: (params?.file_service_protocol as string) ?? tab?.fileServiceProtocol,
    });

    // 4. 如果之前是连接状态，重新连接
    if (wasConnected) {
      try {
        const newSessionId = await invoke<string>("connect_session", {
        request: {
          endpoint,
          params,
          name: name || tab?.name || undefined,
          pluginId: effectivePluginId,
          transferEnabled: transferEnabled ?? true,
          transferProtocol: transferProtocol || "ymodem",
          sendBarEnabled: sendBarEnabled ?? true,
          journaldEnabled: (params?.journald_enabled as boolean) ?? tab?.journaldEnabled ?? false,
          sessionId, // 保持 UUID 连续性

        },});
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

  const openChannel = useCallback(async (parentSessionId: string): Promise<string | null> => {
    // 通道名称由后端 create_ssh_sub_channel 按 channel_index + 1 自动生成 "Channel N"
    try {
      const channelId = await invoke<string>("open_channel", {
        sessionId: parentSessionId,
      });
      return channelId;
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `创建通道失败: ${e}` });
      return null;
    }
  }, []);

  const closeChannel = useCallback(async (channelId: string, parentId: string) => {
    try {
      await invoke("close_channel", { sessionId: channelId });
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
          journaldEnabled: (s.params?.journald_enabled as boolean) ?? false,
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
    // 乐观置灰：后端 I/O loop 断开后仍会发 netdbg-peer-left（带最终统计），此处先行避免闪烁
    dispatch({ type: "SET_NETWORK_PEER_STATE", containerId, peerId, state: "disconnected" });
    peerSessionsRef.current[peerId] = false;
    dispatch({ type: "SET_PEER_CONNECTED", id: peerId, connected: false });
  }, []);

  const clearNetworkPeer = useCallback(async (containerId: string, peerId: string) => {
    try {
      await invoke("close_network_peer", { sessionId: peerId }).catch(() => {
        // 后端可能已清理（如容器断开级联），忽略
      });
    } catch (_e) { /* 忽略 */ }
    delete peerSessionsRef.current[peerId];
    if (networkPeerContainerRef.current[peerId] === containerId) {
      delete networkPeerContainerRef.current[peerId];
    }
    dispatch({ type: "REMOVE_NETWORK_PEER", containerId, peerId });
    dispatch({ type: "REMOVE_PEER", id: peerId });
  }, []);

  const mergeNetworkPeers = useCallback((containerId: string, entries: NetworkPeerEntry[]) => {
    // 与实时事件去重合并：后端快照可能早于/晚于 joined 事件，按 peerId 取并集，
    // 已存在条目保留既有统计（后端快照的 0 值不覆盖运行中的累计值）
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

  /**
   * 统一发送路由。
   *
   * 网络容器按「当前目标」路由：TCP server → 选中对端 / 全部扇出；
   * UDP server → 手动目标地址；UDP client → 固定远端。非网络会话回退
   * 到 sendData(sessionId)。基本发送与指令面板共用此入口。
   */
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

    // TCP：按当前目标路由（群发为容器级「全部客户端」）
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

      // Telnet 回显状态（服务器 ECHO 协商结果 → 本地回显开关）
      const u1b = await listen<{ session_id: string; local_echo: boolean }>(
        "telnet-echo-state",
        (event) => {
          const sid = event.payload.session_id;
          // tab 尚未创建（事件早于 session-connected 到达）时暂存，
          // 由 session-connected 处理路径取出初始化 localEcho
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

      const u2 = await listen<{ session_id: string; endpoint: string; connection_type: string; plugin_id?: string; name: string; params: Record<string, unknown>; connected_at?: number | null; transfer_enabled?: boolean; transfer_protocol?: string; send_bar_enabled?: boolean; virtual_port_pairs?: Array<{ port_a: string; port_b: string }>; file_service_enabled?: boolean; file_service_protocol?: string; journald_enabled?: boolean; parent_id?: string | null; channel_index?: number; is_container?: boolean; local_addr?: string | null }>(
        "session-connected",
        (event) => {
          const sid = event.payload.session_id;
          const vPairs = event.payload.virtual_port_pairs;
          const parentId = event.payload.parent_id ?? null;
          const isContainer = event.payload.is_container ?? false;
          // UDP client 本端地址（连接后本机 ip:port）→ 独立状态，供侧栏端点行展示
          if (typeof event.payload.local_addr === "string") {
            dispatch({ type: "SET_NETWORK_LOCAL_ADDR", containerId: sid, addr: event.payload.local_addr });
          }
          // 检查是否已存在同 ID 的 tab
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
              journaldEnabled: event.payload.journald_enabled ?? false,
              fileServiceEnabled: event.payload.file_service_enabled ?? (event.payload.params?.file_service_enabled as boolean),
              fileServiceProtocol: event.payload.file_service_protocol ?? (event.payload.params?.file_service_protocol as string),
            });
            // 若回显状态事件曾早于本事件暂存，补发（tab 已存在则正常路径已直达）
            const pendingEcho = pendingEchoRef.current.get(sid);
            if (pendingEcho !== undefined) {
              pendingEchoRef.current.delete(sid);
              dispatch({ type: "UPDATE_TAB_ECHO", id: sid, localEcho: pendingEcho });
            }
            if (vPairs && vPairs.length > 0) {
              dispatch({ type: "UPDATE_TAB_VPORTS", id: sid, pairs: vPairs });
            }
          } else if (parentId) {
            // 子 channel 连接成功（connect_session_ssh 的 channel-0 或 open_channel）
            // tab 不存在时直接 ADD_TAB，避免 SET_TAB_STATE 对不存在的 ID 无操作
            const chIdx = (event.payload.channel_index ?? 0) + 1;
            const chName = `Channel ${chIdx}`;
            dispatch({
              type: "ADD_TAB",
              tab: {
                id: sid,
                name: chName,
                connection_type: event.payload.connection_type,
                endpoint: event.payload.endpoint,
                state: "connected",
                pluginId: event.payload.plugin_id || "ssh",
                params: event.payload.params,
                stats: { txBytes: 0, rxBytes: 0 },
                connectedAt: event.payload.connected_at ?? Date.now(),
                transferEnabled: event.payload.transfer_enabled ?? false,
                transferProtocol: event.payload.transfer_protocol,
                sendBarEnabled: event.payload.send_bar_enabled ?? true,
                parentId,
                channelIndex: event.payload.channel_index,
                fileServiceEnabled: event.payload.file_service_enabled ?? (event.payload.params?.file_service_enabled as boolean) ?? false,
                fileServiceProtocol: event.payload.file_service_protocol ?? (event.payload.params?.file_service_protocol as string),
                journaldEnabled: event.payload.journald_enabled ?? (event.payload.params?.journald_enabled as boolean) ?? false,
              },
            });
          } else if (isContainer) {
            // SSH 容器会话 — 不创建独立的根 tab。实际的终端 tab
            // 由后续的 channel-0 session-connected 事件（带 parentId）创建。
            // 仅在重连场景（tab 已存在）时更新状态。
          } else {
            // 真正的新根会话：添加 tab
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
                pluginId: event.payload.plugin_id || "serial",
                // 早于本事件到达的回显状态（telnet-echo-state 暂存）；非 telnet 会话为 undefined
                localEcho: pendingEcho,
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
                journaldEnabled: event.payload.journald_enabled ?? (event.payload.params?.journald_enabled as boolean) ?? false,
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

      // 子通道关闭事件（后端 on_disconnect 或 close_channel 命令触发）
      const u2e = await listen<{ channel_id: string; parent_id: string }>("channel-closed", (event) => {
        dispatch({ type: "REMOVE_CHILD", id: event.payload.channel_id, parentId: event.payload.parent_id });
      });
      if (cancelled) { u2e(); return; }
      unlisteners.push(u2e);

      // 网络调试对端加入（后端 register_peer_channel 成功后广播）
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

      // 网络调试对端断开（后端 on_disconnect 广播，附带最终统计）
      const u2g = await listen<{ session_id: string; peer_id: string; tx_bytes?: number | null; rx_bytes?: number | null }>(
        "netdbg-peer-left",
        (event) => {
          const { session_id: cid, peer_id, tx_bytes, rx_bytes } = event.payload;
          // client 单连接语义：唯一对端断开 → 会话整体断开（无监听器可继续等待）。
          // 容器级清理由 session-disconnected 事件路径完成（CLEAR_NETWORK_PEERS）。
          const containerTab = tabsRef.current.find(t => t.id === cid);
          const isNetClient = containerTab?.pluginId === "network"
            && ((containerTab.params as Record<string, unknown> | undefined)?.role ?? "client") === "client";
          if (isNetClient) {
            invoke("disconnect_session", { sessionId: cid }).catch(() => { /* 后端可能已自行清理 */ });
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

      const u3 = await listen<{ session_id: string; reason?: string }>("session-disconnected", (event) => {
        const reason = event.payload.reason;
        const sid = event.payload.session_id;
        dispatch({ type: "SET_TAB_STATE", id: sid, state: "disconnected" });
        // 网络调试容器断开：级联清理对端注册（后端通道已随容器关闭）
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
        // 重置 Telnet 回显状态（重连时重新协商）
        dispatch({ type: "UPDATE_TAB_ECHO", id: sid, localEcho: false });
        // 父 session 断开时级联移除所有子 channel
        dispatch({ type: "REMOVE_ALL_CHILDREN", parentId: sid });
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
          const cid = networkPeerContainerRef.current[event.payload.tab_id];
          if (cid) {
            // 对端统计 → 网络调试容器对端条目
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

      openChannel,
      closeChannel,
      getTabs,
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
