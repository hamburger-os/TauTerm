/**
 * iperf 插件前端注册
 */
import { createElement } from "react";
import IperfSessionView from "../../components/Iperf/IperfSessionView";
import { StatusBarBadge } from "../../components/Layout/StatusBarPrimitives";
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/iperf.json";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  sessionPresentation: {
    defaultName: params => `iperf @ ${params.version === "iperf3" ? "iperf3" : "iperf2"}`,
  },
  customView: IperfSessionView,
  statusBarItems: [
    {
      id: "iperf-version",
      priority: 850,
      when: ({ activeTab }) => activeTab?.state === "connected" || activeTab?.state === "transferring",
      render: ({ activeTab }) => createElement(
        StatusBarBadge,
        null,
        activeTab?.params?.version === "iperf3" ? "IPERF3" : "IPERF2",
      ),
    },
  ],
});

console.log("[Plugin] iperf plugin registered");
