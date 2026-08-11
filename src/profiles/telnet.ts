import type { TabInfo } from "../context/SessionContext";
import type { ProfileResolver, SessionProfile } from "./types";
import type { IconName } from "../components/common/Icon";

/**
 * Telnet 连接的 Profile 解析器
 *
 * 身份信息：名称、类型、主机、状态
 * 协议参数：端口、发送栏
 */
export const telnetProfile: ProfileResolver = (tab: TabInfo): SessionProfile => {
  const p = tab.params ?? {};
  const port = p.port ?? 23;

  return {
    identity: [
      { label: "session.renameSession", value: tab.name, icon: "tag" },
      { label: "connectionType.label", value: "connectionType.telnet", icon: "plug" },
      { label: "telnet.host", value: tab.endpoint, icon: "pin" },
      {
        label: "session.status",
        value: statusValue(tab.state),
        icon: statusIconName(tab.state),
      },
    ],
    parameters: [
      { label: "telnet.port", value: String(port), monospace: true },
      {
        label: "telnet.enableSendBar",
        value: tab.sendBarEnabled !== false ? "on" : "off",
        monospace: true,
      },
    ],
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
