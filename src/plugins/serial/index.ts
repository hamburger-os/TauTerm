/**
 * Serial 插件前端注册
 *
 * 向内核注册串口协议插件的 UI 组件、工具栏项、状态栏项和翻译资源。
 */
import { registerPlugin } from "../../core/plugin-registry";

registerPlugin({
  manifest: {
    id: "serial",
    name: "Serial Port",
    version: "1.0.0",
    category: "terminal",
    description: "RS-232/RS-485 串口终端",
    icon: "🔌",
    content_type: "terminal",
    capabilities: ["connection", "transfer", "endpoint_discovery"],
    transfer_protocols: ["ymodem", "xmodem", "zmodem"],
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
