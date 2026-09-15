/**
 * 网络调试插件前端注册
 *
 * content_type: "custom" → CustomRenderer → NetworkDebugSessionView
 * 单标签 = 对端列表 + 选中对端详情；TCP/UDP 全角色。
 */
import { useTranslation } from "react-i18next";
import NetworkDebugSessionView from "../../components/Network/NetworkDebugSessionView";
import {
  StatusBarBadge,
  StatusBarGroup,
  StatusBarText,
} from "../../components/Layout/StatusBarPrimitives";
import { useSession } from "../../context/SessionContext";
import {
  registerPlugin,
  type PluginManifest,
  type StatusBarContext,
  type StatusBarItem,
} from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/network.json";
import { formatBytes } from "../../utils/format";

const PRI = {
  role: 850,
  peerCount: 450,
} as const;

function isConnectedState(state?: string): boolean {
  return state === "connected" || state === "transferring";
}

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
  const tab = state.tabs.find(item => item.id === sessionId);
  const transport = tab?.params?.transport === "udp" ? "UDP" : "TCP";
  const role = (tab?.params?.role as string | undefined) ?? "client";
  return (
    <StatusBarBadge>
      {transport} · {t(role === "server" ? "network.roleServerShort" : "network.roleClientShort")}
    </StatusBarBadge>
  );
}

function TcpPeerCount({ sessionId }: { sessionId: string }) {
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const peers = state.networkPeers[sessionId] ?? [];
  const connected = peers.filter(peer => peer.state === "connected").length;
  const maxClients = (tab?.params?.max_clients as number | undefined) ?? 0;
  const selectedId = state.selectedNetworkPeer[sessionId] ?? null;
  const selected = peers.find(peer => peer.peerId === selectedId) ?? null;
  const countText = maxClients > 0 ? `${connected}/${maxClients}` : `${connected}`;

  return (
    <StatusBarText>
      {countText}
      {selected ? ` · ↑${formatBytes(selected.txBytes)} ↓${formatBytes(selected.rxBytes)}` : ""}
    </StatusBarText>
  );
}

function UdpPacketCount({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const rx = tab?.stats.rxPackets ?? 0;
  const tx = tab?.stats.txPackets ?? 0;
  return (
    <StatusBarGroup>
      <StatusBarText>↑ {tx} {t("network.packets")}</StatusBarText>
      <StatusBarText>↓ {rx} {t("network.packets")}</StatusBarText>
    </StatusBarGroup>
  );
}

const statusBarItems: StatusBarItem[] = [
  {
    id: "network-role",
    priority: PRI.role,
    when: context => isNetwork(context),
    render: context => <RoleBadge sessionId={context.sessionId} />,
  },
  {
    id: "network-tcp-peer-count",
    priority: PRI.peerCount,
    overflow: "early",
    when: context => isNetwork(context, "tcp", "server"),
    render: context => <TcpPeerCount sessionId={context.sessionId} />,
  },
  {
    id: "network-udp-packet-count",
    priority: PRI.peerCount,
    overflow: "early",
    when: context => isNetwork(context, "udp"),
    render: context => <UdpPacketCount sessionId={context.sessionId} />,
  },
];

registerPlugin({
  manifest: manifestJson as PluginManifest,
  sessionPresentation: {
    defaultName: params => {
      const transport = params.transport === "udp" ? "UDP" : "TCP";
      const role = params.role === "server" ? "Server" : "Client";
      return `Network Debug @ ${transport} ${role}`;
    },
  },
  customView: NetworkDebugSessionView,
  statusBarItems,
});

console.log("[Plugin] Network debug plugin registered");
