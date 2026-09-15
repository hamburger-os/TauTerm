/** Network Debug frontend plugin registration. */
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
import { usePluginRuntime } from "../../core/usePluginRuntime";
import i18n from "../../i18n";
import manifestJson from "../../plugin-manifests/network.json";
import { formatBytes } from "../../utils/format";
import NetworkSendTarget, { isNetworkSendTargetVisible } from "./NetworkSendTarget";
import {
  clearNetworkPeer,
  disconnectNetworkPeer,
  getNetworkRuntime,
  networkRuntimeStore,
  selectNetworkPeer,
  sendNetworkData,
  type NetworkRuntimeSnapshot,
} from "./runtime-store";

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
  return <StatusBarBadge>{transport} · {t(role === "server" ? "network.roleServerShort" : "network.roleClientShort")}</StatusBarBadge>;
}

function TcpPeerCount({ sessionId }: { sessionId: string }) {
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const runtime = usePluginRuntime<NetworkRuntimeSnapshot>("network", sessionId);
  const connected = runtime.peers.filter(peer => peer.state === "connected").length;
  const maxClients = (tab?.params?.max_clients as number | undefined) ?? 0;
  const selected = runtime.peers.find(peer => peer.peerId === runtime.selectedPeerId) ?? null;
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

function networkSubtitle(params: Record<string, unknown>, endpoint: string): string {
  const transport = params.transport === "udp" ? "udp" : "tcp";
  const role = params.role === "server" ? "server" : "client";
  if (role === "client") {
    const host = typeof params.remote_host === "string" && params.remote_host.trim()
      ? params.remote_host.trim()
      : "";
    const port = typeof params.remote_port === "number" && Number.isFinite(params.remote_port)
      ? params.remote_port
      : undefined;
    if (host && port) return `${transport.toUpperCase()} · ${host}:${port}`;
  } else {
    const host = typeof params.listen_ip === "string" && params.listen_ip.trim()
      ? params.listen_ip.trim()
      : "0.0.0.0";
    const port = typeof params.listen_port === "number" && Number.isFinite(params.listen_port)
      ? params.listen_port
      : undefined;
    if (port) return `${transport.toUpperCase()} · ${host}:${port}`;
  }
  return endpoint;
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
    subtitle: networkSubtitle,
  },
  runtimeStore: networkRuntimeStore,
  sendData: sendNetworkData,
  sendTarget: NetworkSendTarget,
  sendTargetVisible: params => isNetworkSendTargetVisible(params),
  sessionTree: {
    groupKey: (params, fallback) => {
      const transport = params.transport === "udp" ? "udp" : "tcp";
      const role = params.role === "server" ? "server" : "client";
      return `${fallback}/${transport}/${role}`;
    },
    children: (sessionId, params, runtimeSnapshot) => {
      if (params.role !== "server") return [];
      const runtime = runtimeSnapshot as NetworkRuntimeSnapshot;
      return runtime.peers.map(peer => ({
        id: peer.peerId,
        name: peer.name,
        subtitle: peer.addr,
        state: peer.state,
        selected: runtime.selectedPeerId === peer.peerId,
        onSelect: () => selectNetworkPeer(sessionId, peer.peerId),
        menuItems: peer.state === "connected"
          ? [{
              id: "disconnect",
              label: i18n.t("network.disconnect", { defaultValue: "Disconnect Peer" }),
              icon: "stop" as const,
              run: () => disconnectNetworkPeer(sessionId, peer.peerId),
            }]
          : [{
              id: "remove",
              label: i18n.t("network.clearClosed", { defaultValue: "Remove Peer" }),
              icon: "trash" as const,
              danger: true,
              run: () => clearNetworkPeer(sessionId, peer.peerId),
            }],
      }));
    },
    onParentSelect: (sessionId, params) => {
      if (params.role !== "server") return;
      const firstConnected = getNetworkRuntime(sessionId).peers.find(peer => peer.state === "connected");
      selectNetworkPeer(sessionId, firstConnected?.peerId ?? null);
    },
  },
  customView: NetworkDebugSessionView,
  statusBarItems,
});

console.log("[Plugin] Network debug plugin registered");
