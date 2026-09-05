/**
 * TFTP 插件前端注册
 */
import { registerPlugin } from "../../core/plugin-registry";
import TftpSessionView from "../../components/Tftp/TftpSessionView";

registerPlugin({
  manifest: {
    id: "tftp",
    name: "TFTP",
    version: "1.0.0",
    category: "file_transfer",
    description: "TFTP 文件传输",
    icon: "package",
    content_type: "custom",
    send_bar: false,
    capabilities: ["connection", "transfer"],
    transfer_protocols: [],
  },
  customView: TftpSessionView,
});

console.log("[Plugin] TFTP plugin registered");