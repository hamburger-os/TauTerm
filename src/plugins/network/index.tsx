/**
 * 网络调试插件前端注册
 *
 * content_type: "custom" → CustomRenderer → NetworkDebugSessionView
 * 单标签 = 对端列表 + 选中对端详情；TCP/UDP 全角色。
 *
 * 状态栏专属段（声明式 statusBarItems，与核心 StatusBar 基础段统一按 priority 排序）：
 * - role 徽标：服务端/客户端（transport 已由 endpoint 前缀表达，仅补 role 维度）
 * - TCP server 对端计数：connected/max + 选中对端独立 TX/RX 内联
 * - UDP 报文计数：会话级累计 RX/TX 报文数（无对端模型，报文是传输层原生指标）
 */
import { registerPlugin, type StatusBarContext, type StatusBarItem } from "../../core/plugin-registry";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import { formatBytes } from "../../utils/format";
import NetworkDebugSessionView from "../../components/Network/NetworkDebugSessionView";
import statusStyles from "../../components/Layout/StatusBar.module.css";

/** 状态栏优先级（与 StatusBar 基础段协调，数值越大越靠左） */
const PRI = {
  /** 紧跟 endpoint，身份信息相邻 */
  role: 850,
  /** encoding 之后、TX/RX 之前 */
  peerCount: 450,
} as const;

function isConnectedState(state?: string): boolean {
  return state === "connected" || state === "transferring";
}

/** 网络调试会话 + 可选 transport/role 的可见性判定 */
function isNetwork(ctx: StatusBarContext, transport?: string, role?: string): boolean {
  const tab = ctx.activeTab;
  if (!tab || tab.pluginId !== "network" || !isConnectedState(tab.state)) return false;
  const params = tab.params ?? {};
  if (transport && params.transport !== transport) return false;
  if (role && params.role !== role) return false;
  return true;
}

function RoleBadge({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const { state } = useSession();
  const tab = state.tabs.find(t => t.id === sessionId);
  const role = (tab?.params?.role as string | undefined) ?? "client";
  return (
    <span className={statusStyles.modeBadge}>
      {t(role === "server" ? "network.roleServerShort" : "network.roleClientShort")}
    </span>
  );
}

function TcpPeerCount({ sessionId }: { sessionId: string }) {
  const { state } = useSession();
  const tab = state.tabs.find(t => t.id === sessionId);
  const peers = state.networkPeers[sessionId] ?? [];
  const connected = peers.filter(p => p.state === "connected").length;
  const maxClients = (tab?.params?.max_clients as number | undefined) ?? 0;
  const selectedId = state.selectedNetworkPeer[sessionId] ?? null;
  const selected = peers.find(p => p.peerId === selectedId) ?? null;
  const countText = maxClients > 0 ? `${connected}/${maxClients}` : `${connected}`;
  const peerText = selected
    ? ` · ↑${formatBytes(selected.txBytes)} ↓${formatBytes(selected.rxBytes)}`
    : "";
  return <span className={statusStyles.statItem}>{countText}{peerText}</span>;
}

function UdpPacketCount({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const { state } = useSession();
  const tab = state.tabs.find(t => t.id === sessionId);
  const rx = tab?.stats.rxPackets ?? 0;
  const tx = tab?.stats.txPackets ?? 0;
  return (
    <span className={statusStyles.stats}>
      <span className={statusStyles.statItem}>· ↑ {tx} {t("network.packets")}</span>
      <span className={statusStyles.statItem}>↓ {rx} {t("network.packets")}</span>
    </span>
  );
}

const statusBarItems: StatusBarItem[] = [
  {
    id: "network-role",
    align: "left",
    priority: PRI.role,
    when: ctx => isNetwork(ctx),
    render: ctx => <RoleBadge sessionId={ctx.sessionId} />,
  },
  {
    id: "network-tcp-peer-count",
    align: "left",
    priority: PRI.peerCount,
    when: ctx => isNetwork(ctx, "tcp", "server"),
    render: ctx => <TcpPeerCount sessionId={ctx.sessionId} />,
  },
  {
    id: "network-udp-packet-count",
    align: "left",
    priority: PRI.peerCount,
    when: ctx => isNetwork(ctx, "udp"),
    render: ctx => <UdpPacketCount sessionId={ctx.sessionId} />,
  },
];

registerPlugin({
  manifest: {
    id: "network",
    name: "Network Debug",
    version: "1.0.0",
    category: "network_tool",
    description: "TCP/UDP 网络调试助手",
    icon: "globe",
    content_type: "custom",
    send_bar: true,
    capabilities: ["connection", "network_outbound", "network_listen"],
    transfer_protocols: [],
  },
  customView: NetworkDebugSessionView,
  statusBarItems,
});

console.log("[Plugin] Network debug plugin registered");
