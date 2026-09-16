import { StatusBarBadge } from "../../components/Layout/StatusBarPrimitives";
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import i18n from "../../i18n";
import manifestJson from "../../plugin-manifests/trdp.json";
import TrdpConnectForm from "./TrdpConnectForm";
import TrdpSessionRouter from "./TrdpSessionRouter";

function trdpSubtitle(params: Record<string, unknown>): string {
  const unconfigured = String(i18n.t("trdpSidebar.unconfigured"));
  const disabled = String(i18n.t("trdpSidebar.disabled"));
  const capture = String(i18n.t("trdpSidebar.capture"));
  const mode = params.mode === "monitor" ? "monitor" : "node";

  if (mode === "monitor") {
    const interfaceA = typeof params.capture_interface === "string"
      ? params.capture_interface.trim()
      : "";
    const interfaceB = typeof params.capture_interface_b === "string"
      ? params.capture_interface_b.trim()
      : "";
    return params.capture_interface_b_enabled === true
      ? `A: ${interfaceA || unconfigured} · B: ${interfaceB || unconfigured}`
      : `${capture}: ${interfaceA || unconfigured}`;
  }

  const linkA = typeof params.link_a_ip === "string" && params.link_a_ip.trim()
    ? params.link_a_ip.trim()
    : "0.0.0.0";
  const linkB = typeof params.link_b_ip === "string" && params.link_b_ip.trim()
    ? params.link_b_ip.trim()
    : "0.0.0.0";
  return `A: ${linkA} · B: ${params.link_b_enabled === true ? linkB : disabled}`;
}

registerPlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: TrdpConnectForm,
  defaultSessionOptions: () => ({ transferEnabled: false, sendBarEnabled: false }),
  resolveEndpoint: () => "trdp",
  sessionPresentation: {
    defaultName: params => `TRDP @ ${params.mode === "monitor" ? "Monitor" : "Node"}`,
    subtitle: params => trdpSubtitle(params),
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
