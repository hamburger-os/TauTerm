/**
 * Serial 插件前端注册
 *
 * 串口专属配置、校验、会话展示和状态栏项均由插件拥有；通用 Session UI
 * 只负责承载这些声明式能力。
 */
import { createElement } from "react";
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/serial.json";
import SerialConnectForm, {
  DEFAULT_SERIAL_PARAMS,
  isSerialConnectionConfigValid,
  normalizeSerialParams,
} from "./SerialConnectForm";
import {
  SerialLinkStatus,
  SerialTypeStatus,
  SerialVirtualPortStatus,
} from "./SerialStatusItems";

function serialSubtitle(params: Record<string, unknown>, endpoint: string): string {
  const baudRate = typeof params.baud_rate === "number" && Number.isFinite(params.baud_rate)
    ? String(params.baud_rate)
    : "";
  const dataBits = typeof params.data_bits === "number" && Number.isInteger(params.data_bits)
    ? String(params.data_bits)
    : "";
  const parity = params.parity === "even" ? "E" : params.parity === "odd" ? "O" : params.parity === "none" ? "N" : "";
  const stopBits = params.stop_bits === "1" || params.stop_bits === "2" ? params.stop_bits : "";
  const frame = dataBits && parity && stopBits ? `${dataBits}${parity}${stopBits}` : "";
  const flowControl = params.flow_control === "rts_cts"
    ? "RTS/CTS"
    : params.flow_control === "xon_xoff"
      ? "XON/XOFF"
      : "";

  return [endpoint.trim(), baudRate, frame, flowControl].filter(Boolean).join(" · ");
}

const connected = ({ activeTab }: { activeTab: { state: string } | null }) =>
  activeTab?.state === "connected" || activeTab?.state === "transferring";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: SerialConnectForm,
  defaultConnectionParams: () => ({ ...DEFAULT_SERIAL_PARAMS }),
  normalizeConnectionParams: normalizeSerialParams,
  isConnectionConfigValid: isSerialConnectionConfigValid,
  sessionPresentation: {
    // 会话名只在创建时生成一次，表达稳定身份；Text/HEX/Dual 属于可变显示方式，不能进入名称。
    defaultName: (_params, endpoint) => `Serial @ ${endpoint}`,
    // 第二行始终由当前链路配置动态推导，配置变更后自然刷新。
    subtitle: serialSubtitle,
  },
  toolbarItems: [],
  statusBarItems: [
    {
      id: "serial-link",
      align: "left",
      priority: 880,
      when: connected,
      render: context => createElement(SerialLinkStatus, context),
    },
    {
      id: "serial-type",
      align: "left",
      priority: 860,
      when: connected,
      render: context => createElement(SerialTypeStatus, context),
    },
    {
      id: "serial-virtual-port",
      align: "left",
      priority: 300,
      render: context => createElement(SerialVirtualPortStatus, context),
    },
  ],
  locales: {
    "zh-CN": {
      "port": "端口",
      "baudRate": "波特率",
      "dataBits": "数据位",
      "parity": "校验位",
      "stopBits": "停止位",
      "flowControl": "流控",
      "connect": "连接",
      "disconnect": "断开",
      "noPorts": "未检测到串口",
      "refresh": "刷新端口列表",
    },
    "en-US": {
      "port": "Port",
      "baudRate": "Baud Rate",
      "dataBits": "Data Bits",
      "parity": "Parity",
      "stopBits": "Stop Bits",
      "flowControl": "Flow Control",
      "connect": "Connect",
      "disconnect": "Disconnect",
      "noPorts": "No serial ports detected",
      "refresh": "Refresh port list",
    },
  },
});

console.log("[Plugin] Serial plugin registered");
