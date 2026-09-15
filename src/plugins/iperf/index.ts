/**
 * iperf 插件前端注册
 */
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/iperf.json";
import IperfSessionView from "../../components/Iperf/IperfSessionView";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  sessionPresentation: {
    defaultName: params => `iperf @ ${params.version === "iperf3" ? "iperf3" : "iperf2"}`,
    subtitle: params => {
      const listenIp = typeof params.listen_ip === "string" && params.listen_ip.trim()
        ? params.listen_ip.trim()
        : "0.0.0.0";
      const listenPort = typeof params.listen_port === "number" && Number.isFinite(params.listen_port)
        ? params.listen_port
        : (params.version === "iperf3" ? 5201 : 5001);
      return `${listenIp}:${listenPort}`;
    },
  },
  customView: IperfSessionView,
});

console.log("[Plugin] iperf plugin registered");
