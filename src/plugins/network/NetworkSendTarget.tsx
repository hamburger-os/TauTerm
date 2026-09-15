import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import { useToast } from "../../context/ToastContext";
import { usePluginRuntime } from "../../core/usePluginRuntime";
import {
  selectNetworkPeer,
  setNetworkBroadcast,
  setNetworkManualTarget,
  type NetworkRuntimeSnapshot,
} from "./runtime-store";
import styles from "../../components/SendBar/TargetBar.module.css";

const ALL_NETWORK_PEERS = "__all__";

export function isNetworkSendTargetVisible(params: Record<string, unknown> | undefined): boolean {
  const transport = params?.transport as string | undefined;
  return (transport === "tcp" || transport === "udp") && params?.role === "server";
}

export default function NetworkSendTarget({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const { showToast } = useToast();
  const { state } = useSession();
  const runtime = usePluginRuntime<NetworkRuntimeSnapshot>("network", sessionId);
  const tab = state.tabs.find(item => item.id === sessionId);
  const params = (tab?.params ?? {}) as Record<string, unknown>;
  const transport = params.transport as string | undefined;
  const visible = isNetworkSendTargetVisible(params);
  const syncReady = visible && tab?.state === "connected";

  useEffect(() => {
    if (!syncReady) return;
    let active = true;
    const target = transport === "udp"
      ? (runtime.manualTarget.trim() || null)
      : (runtime.broadcast ? ALL_NETWORK_PEERS : runtime.selectedPeerId);
    const timer = setTimeout(() => {
      void invoke("set_network_send_target", { sessionId, target }).catch(error => {
        if (!active) return;
        showToast(
          "error",
          `${t("network.targetSyncFailed", { defaultValue: "Failed to synchronize send target" })}: ${String(error)}`,
        );
      });
    }, transport === "udp" ? 120 : 0);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [sessionId, syncReady, transport, runtime.manualTarget, runtime.broadcast, runtime.selectedPeerId, showToast, t]);

  if (!visible) return null;

  const peers = runtime.peers.filter(peer => peer.state === "connected");
  if (transport === "tcp") {
    const value = runtime.broadcast ? ALL_NETWORK_PEERS : (runtime.selectedPeerId ?? "");
    return (
      <div className={`${styles.bar} liquid-glass-panel`}>
        <span className={styles.label}>{t("network.targetLabel")}</span>
        <select
          className={`${styles.select} liquid-glass-input liquid-glass-select`}
          value={value}
          onChange={(event) => {
            const next = event.target.value;
            if (next === ALL_NETWORK_PEERS) {
              setNetworkBroadcast(sessionId, true);
              return;
            }
            setNetworkBroadcast(sessionId, false);
            selectNetworkPeer(sessionId, next || null);
          }}
          title={t("network.selectTarget")}
        >
          <option value={ALL_NETWORK_PEERS}>{t("network.targetAllClients")}</option>
          {peers.map(peer => (
            <option key={peer.peerId} value={peer.peerId}>{peer.name} · {peer.addr}</option>
          ))}
        </select>
      </div>
    );
  }

  return (
    <div className={`${styles.bar} liquid-glass-panel`}>
      <span className={styles.label}>{t("network.targetLabel")}</span>
      <input
        className={`${styles.input} liquid-glass-input`}
        type="text"
        placeholder={t("network.manualTargetPlaceholder")}
        value={runtime.manualTarget}
        onChange={(event) => setNetworkManualTarget(sessionId, event.target.value)}
        spellCheck={false}
      />
      <select
        className={`${styles.select} liquid-glass-input liquid-glass-select`}
        value=""
        onChange={(event) => {
          if (event.target.value) setNetworkManualTarget(sessionId, event.target.value);
        }}
        title={t("network.selectTarget")}
      >
        <option value="">{t("network.selectTarget")}</option>
        {runtime.udpSources.map(addr => <option key={addr} value={addr}>{addr}</option>)}
      </select>
    </div>
  );
}
