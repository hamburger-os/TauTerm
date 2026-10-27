import type { TabInfo } from "../context/SessionContext";
import type { ProfileResolver, SessionProfile } from "./types";
import { statusIconName, statusValue } from "./status";

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
      { label: "connectionType.label", value: "connectionType.ssh", icon: "connection" },
      { label: "ssh.host", value: tab.endpoint, icon: "endpoint" },
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
