/**
 * iperf 插件前端注册
 */
import { registerPlugin } from "../../core/plugin-registry";
import IperfSessionView from "../../components/Iperf/IperfSessionView";

registerPlugin({
  manifest: {
    id: "iperf",
    name: "iperf",
    version: "1.0.0",
    category: "network_tool",
    description: "iperf 网络测速",
    icon: "stopwatch",
    content_type: "custom",
    capabilities: ["connection"],
    transfer_protocols: [],
  },
  customView: IperfSessionView,
});

console.log("[Plugin] iperf plugin registered");
