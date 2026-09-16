/** TFTP frontend plugin definition. */
import { createElement } from "react";
import { StatusBarBadge } from "../../components/Layout/StatusBarPrimitives";
import TftpSessionView from "./TftpSessionView";
import { definePlugin, type PluginManifest } from "../../core/plugin-registry";
import i18n from "../../i18n";
import manifestJson from "../../plugin-manifests/tftp.json";
import TftpConnectForm, {
  DEFAULT_TFTP_PARAMS,
  hasTftpExposureRisk,
  isTftpConnectionConfigValid,
  normalizeTftpParams,
} from "./TftpConnectForm";

export const tftpPlugin = definePlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: TftpConnectForm,
  defaultConnectionParams: () => ({ ...DEFAULT_TFTP_PARAMS }),
  defaultSessionOptions: () => ({ transferEnabled: false, sendBarEnabled: false }),
  normalizeConnectionParams: normalizeTftpParams,
  isConnectionConfigValid: isTftpConnectionConfigValid,
  resolveEndpoint: params => `${String(params.listen_ip ?? "0.0.0.0").trim()}:${Number(params.listen_port ?? 69)}`,
  reconnectGuard: ({ params }) => {
    if (hasTftpExposureRisk(params) && params.exposure_confirmed !== true) {
      return {
        ok: false,
        message: i18n.t("tftp.exposureWarning", {
          defaultValue:
            "This TFTP server will accept remote writes and allow overwriting files from a non-loopback interface. Continue only on a trusted network.",
        }),
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
  workspace: { availability: "always" },
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
