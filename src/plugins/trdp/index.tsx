import { StatusBarBadge } from "../../components/Layout/StatusBarPrimitives";
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/trdp.json";
import TrdpConnectForm from "./TrdpConnectForm";
import TrdpSessionRouter from "./TrdpSessionRouter";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: TrdpConnectForm,
  sessionPresentation: {
    defaultName: params => `TRDP @ ${params.mode === "monitor" ? "Monitor" : "Node"}`,
  },
  customView: TrdpSessionRouter,
  statusBarItems: [
    {
      id: "trdp-mode",
      priority: 850,
      when: ({ activeTab }) => activeTab?.state === "connected" || activeTab?.state === "transferring",
      render: ({ activeTab }) => (
        <StatusBarBadge>
          TRDP · {activeTab?.params?.mode === "monitor" ? "MONITOR" : "NODE"}
        </StatusBarBadge>
      ),
    },
  ],
});

console.log("[Plugin] TRDP plugin registered");
