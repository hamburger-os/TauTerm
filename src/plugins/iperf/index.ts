/**
 * iperf 插件前端注册
 */
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/iperf.json";
import IperfSessionView from "../../components/Iperf/IperfSessionView";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  customView: IperfSessionView,
});

console.log("[Plugin] iperf plugin registered");
