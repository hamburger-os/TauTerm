import type { TabInfo } from "../context/SessionContext";
import type { ProfileResolver, SessionProfile } from "./types";
import { statusIconName, statusValue } from "./status";

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
      { label: "connectionType.label", value: "connectionType.telnet", icon: "connection" },
      { label: "telnet.host", value: tab.endpoint, icon: "endpoint" },
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
