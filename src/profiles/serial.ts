import type { TabInfo } from "../context/SessionContext";
import type { ProfileResolver, SessionProfile } from "./types";
import type { IconName } from "../components/common/Icon";

/**
 * Serial 连接的 Profile 解析器
 *
 * 身份信息：名称、类型、端口、状态
 * 协议参数：波特率、数据位、校验位、停止位、流控
 */
export const serialProfile: ProfileResolver = (tab: TabInfo): SessionProfile => {
  const p = tab.params ?? {};

  const baudRate = p.baud_rate ?? "115200";
  const dataBits = p.data_bits ?? "8";
  const parity = p.parity ?? "none";
  const stopBits = p.stop_bits ?? "1";
  const flowControl = p.flow_control ?? "none";

  return {
    identity: [
      { label: "session.renameSession", value: tab.name, icon: "tag" },
      { label: "connectionType.label", value: "connectionType.serial", icon: "plug" },
      { label: "serial.port", value: tab.endpoint, icon: "pin" },
      {
        label: "session.status",
        value: statusValue(tab.state),
        icon: statusIconName(tab.state),
      },
    ],
    parameters: [
      { label: "serial.baudRate", value: String(baudRate), monospace: true },
      { label: "serial.dataBits", value: String(dataBits), monospace: true },
      { label: "serial.parity", value: String(parity), monospace: true },
      { label: "serial.stopBits", value: String(stopBits), monospace: true },
      { label: "serial.flowControl", value: String(flowControl), monospace: true },
    ],
  };
};

function statusIconName(state: string): IconName {
  switch (state) {
    case "connected": return "status-connected";
    case "disconnected": return "status-disconnected";
    case "connecting": return "status-connecting";
    case "transferring": return "upload";
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
