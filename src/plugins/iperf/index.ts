/** iperf frontend plugin definition. */
import { createElement } from "react";
import IperfSessionView from "./IperfSessionView";
import { StatusBarBadge } from "../../components/Layout/StatusBarPrimitives";
import { definePlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/iperf.json";
import IperfConnectForm, {
  DEFAULT_IPERF_PARAMS,
  isIperfConnectionConfigValid,
  normalizeIperfParams,
} from "./IperfConnectForm";

export const iperfPlugin = definePlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: IperfConnectForm,
  defaultConnectionParams: () => ({ ...DEFAULT_IPERF_PARAMS }),
  defaultSessionOptions: () => ({ transferEnabled: false, sendBarEnabled: false }),
  normalizeConnectionParams: normalizeIperfParams,
  isConnectionConfigValid: isIperfConnectionConfigValid,
  resolveEndpoint: () => "iperf",
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
  workspace: { availability: "always" },
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
