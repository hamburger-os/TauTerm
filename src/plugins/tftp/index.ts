/**
 * TFTP 插件前端注册
 */
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/tftp.json";
import TftpSessionView from "../../components/Tftp/TftpSessionView";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  customView: TftpSessionView,
});

console.log("[Plugin] TFTP plugin registered");