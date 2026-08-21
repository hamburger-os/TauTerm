import type { TabInfo } from "../context/SessionContext";
import type { ProfileResolver, SessionProfile } from "./types";
import type { IconName } from "../components/common/Icon";

/**
 * 网络调试连接的 Profile 解析器
 *
 * 身份信息：名称、类型、端点、状态
 * 协议参数：传输层、角色、本地/远端端口
 */
export const networkProfile: ProfileResolver = (tab: TabInfo): SessionProfile => {
  const p = tab.params ?? {};
  const transport = (p.transport as string | undefined) ?? "tcp";
  const role = (p.role as string | undefined) ?? "client";
  const localPort = p.local_port ?? 0;
  const remotePort = p.remote_port ?? 0;

  const parameterRows: Array<{ label: string; value: string }> = [
    { label: "network.transport", value: transport.toUpperCase() },
    { label: "network.role", value: role },
  ];
  if (transport === "tcp" && role === "client") {
    parameterRows.push({ label: "network.remotePort", value: String(remotePort) });
  } else if (transport === "tcp" && role === "server") {
    parameterRows.push({ label: "network.localPort", value: String(localPort) });
    if (typeof p.max_clients === "number") {
      parameterRows.push({ label: "network.maxClients", value: String(p.max_clients) });
    }
  } else if (transport === "udp") {
    parameterRows.push({ label: "network.localPort", value: String(localPort) });
    if (typeof p.multicast_group === "string" && p.multicast_group) {
      parameterRows.push({ label: "network.multicastGroup", value: p.multicast_group });
    }
  }

  return {
    identity: [
      { label: "session.renameSession", value: tab.name, icon: "tag" },
      { label: "connectionType.label", value: "network.name", icon: "plug" },
      { label: "network.peer", value: tab.endpoint, icon: "pin" },
      {
        label: "session.status",
        value: statusValue(tab.state),
        icon: statusIconName(tab.state),
      },
    ],
    parameters: parameterRows.map(r => ({ ...r, monospace: true })),
  };
};

function statusIconName(state: string): IconName {
  switch (state) {
    case "connected": return "status-connected";
    case "disconnected": return "status-disconnected";
    case "connecting": return "status-connecting";
    case "transferring": return "status-connecting";
    default: return "status-idle";
  }
}

function statusValue(state: string): string {
  switch (state) {
    case "connected": return "statusBar.connected";
    case "disconnected": return "statusBar.disconnected";
    case "connecting": return "statusBar.connecting";
    case "transferring": return "transfer.transferringStatus";
    default: return state;
  }
}
