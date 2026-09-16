/**
 * TFTP 插件前端注册
 */
import { createElement } from "react";
import { StatusBarBadge } from "../../components/Layout/StatusBarPrimitives";
import TftpSessionView from "../../components/Tftp/TftpSessionView";
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/tftp.json";
import TftpConnectForm, {
  DEFAULT_TFTP_PARAMS,
  isTftpConnectionConfigValid,
  normalizeTftpParams,
} from "./TftpConnectForm";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: TftpConnectForm,
  defaultConnectionParams: () => ({ ...DEFAULT_TFTP_PARAMS }),
  defaultSessionOptions: () => ({ transferEnabled: false, sendBarEnabled: false }),
  normalizeConnectionParams: normalizeTftpParams,
  isConnectionConfigValid: isTftpConnectionConfigValid,
  resolveEndpoint: params => `${String(params.listen_ip ?? "0.0.0.0").trim()}:${Number(params.listen_port ?? 69)}`,
  sessionPresentation: {
    defaultName: (params, endpoint) => {
      const root = typeof params.file_root === "string" && params.file_root.trim()
        ? params.file_root.trim()
        : endpoint;
      return `TFTP @ ${root}`;
    },
  },
  customView: TftpSessionView,
  statusBarItems: [
    {
      id: "tftp-type",
      priority: 850,
      when: ({ activeTab }) => activeTab?.state === "connected" || activeTab?.state === "transferring",
      render: () => createElement(StatusBarBadge, null, "TFTP"),
    },
  ],
});

console.log("[Plugin] TFTP plugin registered");
