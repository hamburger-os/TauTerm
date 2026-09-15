/**
 * TFTP 插件前端注册
 */
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import i18n from "../../i18n";
import manifestJson from "../../plugin-manifests/tftp.json";
import TftpSessionView from "../../components/Tftp/TftpSessionView";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  reconnectGuard: ({ params }) => {
    const bindIp = String(params.listen_ip ?? "").trim().toLowerCase();
    const loopback = bindIp === "127.0.0.1" || bindIp === "::1" || bindIp === "localhost";
    if (
      !loopback
      && params.write_enabled === true
      && params.overwrite === true
      && params.exposure_confirmed !== true
    ) {
      return {
        ok: false,
        message: String(i18n.t("tftp.exposureWarning", {
          defaultValue:
            "This TFTP server will accept remote writes and allow overwriting files from a non-loopback interface. Continue only on a trusted network.",
        })),
      };
    }
    return { ok: true };
  },
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
