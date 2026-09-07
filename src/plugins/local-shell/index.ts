import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/local-shell.json";
import LocalShellConnectForm from "./LocalShellConnectForm";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: LocalShellConnectForm,
  toolbarItems: [],
});
