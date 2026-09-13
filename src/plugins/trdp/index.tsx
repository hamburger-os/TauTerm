import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/trdp.json";
import TrdpConnectForm from "./TrdpConnectForm";
import TrdpSessionView from "./TrdpSessionView";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: TrdpConnectForm,
  sessionPresentation: {
    defaultName: params => `TRDP @ ${params.mode === "monitor" ? "Monitor" : "Node"}`,
  },
  customView: TrdpSessionView,
});

console.log("[Plugin] TRDP plugin registered");
