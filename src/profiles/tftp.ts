import type { TabInfo } from "../context/SessionContext";
import type { ProfileResolver, SessionProfile } from "./types";
import type { IconName } from "../components/common/Icon";

/**
 * TFTP 连接的 Profile 解析器
 */
export const tftpProfile: ProfileResolver = (tab: TabInfo): SessionProfile => {
  const p = tab.params ?? {};

  const listenIp = p.listen_ip ?? "0.0.0.0";
  const port = p.listen_port ?? 69;
  const fileRoot = p.file_root ?? "";

  return {
    identity: [
      { label: "session.renameSession", value: tab.name, icon: "tag" },
      { label: "connectionType.label", value: "connectionType.tftp", icon: "connection" },
      { label: "tftp.listenAddr", value: `${listenIp}:${port}`, icon: "endpoint" },
      {
        label: "session.status",
        value: statusValue(tab.state),
        icon: statusIconName(tab.state),
      },
    ],
    parameters: [
      { label: "tftp.fileRoot", value: String(fileRoot), monospace: true },
    ],
  };
};

function statusIconName(state: string): IconName {
  switch (state) {
    case "connected": return "status-connected";
    case "disconnected": return "status-disconnected";
    case "connecting": return "status-connecting";
    case "transferring": return "status-transferring";
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
