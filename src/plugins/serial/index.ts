/**
 * Serial 插件前端注册
 *
 * 向内核注册串口协议插件的 UI 组件、工具栏项、状态栏项和翻译资源。
 */
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/serial.json";

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

registerPlugin({
  manifest: manifestJson as PluginManifest,
  sessionPresentation: {
    // 会话名只在创建时生成一次，表达稳定身份；Text/HEX/Dual 属于可变显示方式，不能进入名称。
    defaultName: (_params, endpoint) => `Serial @ ${endpoint}`,
    // 第二行始终由当前链路配置动态推导，配置变更后自然刷新。
    subtitle: serialSubtitle,
  },
  toolbarItems: [],
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
