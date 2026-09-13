/**
 * TFTP 插件前端注册
 */
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/tftp.json";
import TftpSessionView from "../../components/Tftp/TftpSessionView";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  sessionPresentation: {
    defaultName: (params, endpoint) => {
      const root = typeof params.file_root === "string" && params.file_root.trim()
        ? params.file_root.trim()
        : endpoint;
      return `TFTP @ ${root}`;
    },
  },
  customView: TftpSessionView,
});

console.log("[Plugin] TFTP plugin registered");