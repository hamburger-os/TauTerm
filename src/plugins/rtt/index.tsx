import { StatusBarBadge } from "../../components/Layout/StatusBarPrimitives";
import { definePlugin, type PluginManifest } from "../../core/plugin-registry";
import { usePluginRuntime } from "../../core/usePluginRuntime";
import manifestJson from "../../plugin-manifests/rtt.json";
import RttConnectForm from "./RttConnectForm";
import RttSendTarget from "./RttSendTarget";
import RttSessionView from "./RttSessionView";
import {
  defaultRttParams,
  normalizeRttParams,
  rttDefaultSessionName,
  rttSubtitle,
} from "./model";
import {
  rttRuntimeStore,
  sendRttData,
  type RttRuntimeSnapshot,
} from "./runtime-store";
import { rttLocales } from "./locales";

function validConfig(params: Record<string, unknown>): boolean {
  const value = normalizeRttParams(params);
  if (value.backend === "jlink_existing") {
    const port = Number(value.jlink_port);
    return Number.isInteger(port) && port >= 1 && port <= 65535;
  }
  const target = typeof value.target === "string" ? value.target.trim() : "";
  if (!target) return false;
  if (value.locator_mode === "exact") {
    return typeof value.control_block_address === "string" && value.control_block_address.trim().length > 0;
  }
  if (value.locator_mode === "ranges") {
    return typeof value.scan_ranges === "string" && value.scan_ranges.trim().length > 0;
  }
  return true;
}

function RttStatus({ sessionId }: { sessionId: string }) {
  const runtime = usePluginRuntime<RttRuntimeSnapshot>("rtt", sessionId);
  const backend = runtime.snapshot?.backend?.display_name ?? "RTT";
  const channel = runtime.snapshot?.send_channel ?? null;
  return (
    <StatusBarBadge>
      {backend}{channel != null ? ` · Ch ${channel}` : ""}
    </StatusBarBadge>
  );
}

export const rttPlugin = definePlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: RttConnectForm,
  defaultConnectionParams: defaultRttParams,
  normalizeConnectionParams: normalizeRttParams,
  isConnectionConfigValid: validConfig,
  defaultSessionOptions: () => ({ transferEnabled: false, sendBarEnabled: true }),
  resolveEndpoint: () => "rtt",
  sessionPresentation: {
    defaultName: params => rttDefaultSessionName(params),
    subtitle: params => rttSubtitle(params),
  },
  locales: rttLocales,
  runtimeStore: rttRuntimeStore,
  sendData: sendRttData,
  sendTarget: RttSendTarget,
  sendTargetVisible: () => true,
  customView: RttSessionView,
  formatSessionError: error => {
    if (error && typeof error === "object" && "message" in error) {
      return String((error as { message: unknown }).message);
    }
    const raw = String(error);
    const match = raw.match(/(?:^|:\s)([a-z_]+):\s(.+)$/);
    return match?.[2] ?? raw;
  },
  statusBarItems: [
    {
      id: "rtt-backend",
      priority: 850,
      when: ({ activeTab }) => activeTab?.state === "connected" || activeTab?.state === "transferring",
      render: ({ sessionId }) => <RttStatus sessionId={sessionId} />,
    },
  ],
});
