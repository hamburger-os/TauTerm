import type { TabInfo } from "../context/SessionContext";
import type { ProfileResolver, SessionProfile } from "./types";
import type { IconName } from "../components/common/Icon";

/**
 * SSH 连接的 Profile 解析器
 *
 * 身份信息：名称、类型、主机地址、状态
 * 协议参数：端口、用户名、认证方式
 */
export const sshProfile: ProfileResolver = (tab: TabInfo): SessionProfile => {
  const p = tab.params ?? {};

  const port = p.port ?? "22";
  const username = p.username ?? "";
  const authMethod = p.auth_method ?? "password";

  return {
    identity: [
      { label: "session.renameSession", value: tab.name, icon: "tag" },
      { label: "connectionType.label", value: "connectionType.ssh", icon: "plug" },
      { label: "ssh.host", value: tab.endpoint, icon: "pin" },
      {
        label: "session.status",
        value: statusValue(tab.state),
        icon: statusIconName(tab.state),
      },
    ],
    parameters: [
      { label: "ssh.port", value: String(port), monospace: true },
      { label: "ssh.username", value: String(username), monospace: true },
      { label: "ssh.authMethod", value: String(authMethod), monospace: false },
    ],
  };
};

function statusIconName(state: string): IconName {
  switch (state) {
    case "connected": return "status-connected";
    case "disconnected": return "status-disconnected";
    case "connecting": return "status-connecting";
    // 与 serial 的 transferring 一致，统一使用旋转状态图标
    case "transferring": return "status-connecting";
    default: return "status-idle";
  }
}

function statusValue(state: string): string {
  switch (state) {
    // 使用协议无关的状态文案，避免 SSH 会话显示"串口已连接"
    case "connected": return "statusBar.connected";
    case "disconnected": return "statusBar.disconnected";
    case "connecting": return "statusBar.connecting";
    case "transferring": return "transfer.transferringStatus";
    default: return state;
  }
}
