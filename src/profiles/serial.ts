import type { TabInfo } from "../context/SessionContext";
import type { ProfileResolver, SessionProfile } from "./types";
import type { IconName } from "../components/common/Icon";

function displayParam(value: unknown): string {
  return value === undefined || value === null || value === "" ? "—" : String(value);
}

/**
 * Serial 连接的 Profile 解析器
 *
 * 身份信息：名称、类型、端口、状态
 * 协议参数：只展示当前 Session 已保存的真实链路参数，不在展示层复制默认配置。
 */
export const serialProfile: ProfileResolver = (tab: TabInfo): SessionProfile => {
  const p = tab.params ?? {};

  return {
    identity: [
      { label: "session.renameSession", value: tab.name, icon: "tag" },
      { label: "connectionType.label", value: "connectionType.serial", icon: "connection" },
      { label: "serial.port", value: tab.endpoint, icon: "endpoint" },
      {
        label: "session.status",
        value: statusValue(tab.state),
        icon: statusIconName(tab.state),
      },
    ],
    parameters: [
      { label: "serial.baudRate", value: displayParam(p.baud_rate), monospace: true },
      { label: "serial.dataBits", value: displayParam(p.data_bits), monospace: true },
      { label: "serial.parity", value: displayParam(p.parity), monospace: true },
      { label: "serial.stopBits", value: displayParam(p.stop_bits), monospace: true },
      { label: "serial.flowControl", value: displayParam(p.flow_control), monospace: true },
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
    case "connected": return "serial.connected";
    case "disconnected": return "serial.disconnected";
    case "connecting": return "serial.connecting";
    case "transferring": return "transfer.transferringStatus";
    default: return state;
  }
}
