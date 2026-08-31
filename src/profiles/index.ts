import type { TabInfo } from "../context/SessionContext";
import type { ProfileResolver, SessionProfile } from "./types";
import { serialProfile } from "./serial";
import { sshProfile } from "./ssh";
import { telnetProfile } from "./telnet";
import { tftpProfile } from "./tftp";
import { networkProfile } from "./network";

/** Profile 注册表：连接类型 -> ProfileResolver */
const registry: Record<string, ProfileResolver> = {
  serial: serialProfile,
  ssh: sshProfile,
  telnet: telnetProfile,
  tftp: tftpProfile,
  network: networkProfile,
};

/**
 * 根据连接类型解析会话 Profile
 *
 * 未注册的类型返回降级 Profile（仅显示身份信息，参数区为空）。
 */
export function resolveProfile(tab: TabInfo): SessionProfile {
  const resolver = registry[tab.connection_type];
  if (resolver) {
    return resolver(tab);
  }
  // 降级：未知协议类型
  return {
    identity: [
      { label: "session.renameSession", value: tab.name, icon: "tag" },
      { label: "connectionType.label", value: tab.connection_type, icon: "connection" },
      { label: "serial.port", value: tab.endpoint, icon: "endpoint" },
      { label: "session.status", value: tab.state, icon: "status-idle" },
    ],
    parameters: [
      { label: "session.status", value: tab.connection_type, monospace: true },
    ],
  };
}
