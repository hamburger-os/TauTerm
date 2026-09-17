/** Serial frontend plugin definition. */
import { createElement } from "react";
import { definePlugin, type PluginManifest } from "../../core/plugin-registry";
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
import { serialRuntimeStore } from "./runtime-store";

const SERIAL_DATA_MODE_LABELS = {
  text: "Text",
  hex: "HEX",
  dual: "Dual",
} as const;

function serialDataModeLabel(params: Record<string, unknown>): string {
  const mode = params.data_mode;
  if (mode === "hex" || mode === "dual") return SERIAL_DATA_MODE_LABELS[mode];
  return SERIAL_DATA_MODE_LABELS.text;
}

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

export const serialPlugin = definePlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: SerialConnectForm,
  defaultConnectionParams: () => ({ ...DEFAULT_SERIAL_PARAMS }),
  defaultSessionOptions: () => ({
    transferEnabled: true,
    transferProtocol: "ymodem",
    sendBarEnabled: true,
  }),
  normalizeConnectionParams: normalizeSerialParams,
  isConnectionConfigValid: (params, endpoint) =>
    isSerialConnectionConfigValid(params) && Boolean(endpoint?.trim()),
  sessionPresentation: {
    defaultName: params => `Serial @ ${serialDataModeLabel(params)}`,
    subtitle: serialSubtitle,
  },
  runtimeStore: serialRuntimeStore,
  toolbarItems: [],
  statusBarItems: [
    {
      id: "serial-link",
      priority: 880,
      when: connected,
      render: context => createElement(SerialLinkStatus, context),
    },
    {
      id: "serial-type",
      priority: 860,
      when: connected,
      render: context => createElement(SerialTypeStatus, context),
    },
    {
      id: "serial-virtual-port",
      priority: 300,
      overflow: "early",
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
      "virtualPortBridgeFailed": "虚拟串口桥接已停止，请查看日志详情",
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
      "virtualPortBridgeFailed": "Virtual-port bridge stopped; see logs for details",
    },
  },
});
