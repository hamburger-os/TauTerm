import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import { useToast } from "../../context/ToastContext";
import { ALL_NETWORK_PEERS, isTargetBarVisible } from "./networkSendTarget";

/**
 * Bridge the session-owned network target to the backend script engine.
 * TargetBar remains a pure selector; backend synchronization and error reporting live here.
 */
export function useNetworkSendTargetSync(containerId: string): void {
  const { t } = useTranslation();
  const { showToast } = useToast();
  const { state } = useSession();

  const tab = state.tabs.find(item => item.id === containerId);
  const params = (tab?.params ?? {}) as Record<string, unknown>;
  const transport = params.transport as string | undefined;
  const visible = isTargetBarVisible(params);
  const selectedPeerId = state.selectedNetworkPeer[containerId] ?? null;
  const broadcast = state.networkBroadcast[containerId] === true;
  const manualTarget = state.networkManualTarget[containerId] ?? "";

  useEffect(() => {
    if (!visible) return;

    const target = transport === "udp"
      ? (manualTarget.trim() || null)
      : (broadcast ? ALL_NETWORK_PEERS : selectedPeerId);

    // UDP targets are typed interactively; coalesce rapid edits before crossing the IPC boundary.
    const timer = setTimeout(() => {
      void invoke("set_network_send_target", { sessionId: containerId, target }).catch(error => {
        showToast(
          "error",
          `${t("network.targetSyncFailed", { defaultValue: "Failed to synchronize send target" })}: ${String(error)}`,
        );
      });
    }, transport === "udp" ? 120 : 0);

    return () => clearTimeout(timer);
  }, [
    containerId,
    visible,
    transport,
    manualTarget,
    broadcast,
    selectedPeerId,
    showToast,
    t,
  ]);
}
