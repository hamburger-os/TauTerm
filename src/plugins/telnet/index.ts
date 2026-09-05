/**
 * Telnet 插件前端注册
 *
 * 向内核注册 Telnet 协议插件的 manifest 和翻译资源。
 * 无文件传输（transfer_protocols 为空）→ 右侧 Transmission 面板不显示。
 */
import { registerPlugin } from "../../core/plugin-registry";

registerPlugin({
  manifest: {
    id: "telnet",
    name: "Telnet",
    version: "1.0.0",
    category: "terminal",
    description: "Telnet 终端",
    icon: "globe",
    content_type: "terminal",
    send_bar: true,
    capabilities: ["connection"],
    transfer_protocols: [],
  },
  toolbarItems: [],
  locales: {
    "zh-CN": {
      "host": "主机地址",
      "hostPlaceholder": "192.168.1.1",
      "port": "端口",
      "enableSendBar": "启用发送栏",
    },
    "en-US": {
      "host": "Host",
      "hostPlaceholder": "192.168.1.1",
      "port": "Port",
      "enableSendBar": "Enable Send Bar",
    },
  },
});

console.log("[Plugin] Telnet plugin registered");
