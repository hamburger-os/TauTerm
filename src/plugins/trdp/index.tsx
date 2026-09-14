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
});

console.log("[Plugin] TRDP plugin registered");
