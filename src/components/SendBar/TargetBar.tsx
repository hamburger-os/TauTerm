import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import { ALL_NETWORK_PEERS, isTargetBarVisible } from "./networkSendTarget";
import styles from "./TargetBar.module.css";

export { isTargetBarVisible } from "./networkSendTarget";

interface Props {
  containerId: string;
}

/**
 * 发送目标栏 — 横跨四个发送模式的共享目标选择器。
 *
 * 按会话 transport/role 渲染：
 * - TCP server：对端下拉（含「全部客户端」伪目标）；
 * - UDP server：手动 IP:port 输入 + 最近 RX 来源快捷回填；
 * - 其余（串口 / TCP client / UDP client）：返回 null（单一固定目标，无需选择）。
 *
 * 这里只负责呈现与更新 SessionContext 中的目标状态。后端脚本引擎同步由
 * useNetworkSendTargetSync 统一负责，避免展示组件同时维护第二套运行时状态。
 */
export default function TargetBar({ containerId }: Props) {
  const { t } = useTranslation();
  const { state, selectNetworkPeer, setNetworkBroadcast, setNetworkManualTarget } = useSession();

  const tab = state.tabs.find(item => item.id === containerId);
  const params = (tab?.params ?? {}) as Record<string, unknown>;
  const transport = params.transport as string | undefined;

  if (!isTargetBarVisible(params)) return null;

  const peers = (state.networkPeers[containerId] ?? []).filter(peer => peer.state === "connected");
  const selectedPeerId = state.selectedNetworkPeer[containerId] ?? null;
  const broadcast = state.networkBroadcast[containerId] === true;
  const manualTarget = state.networkManualTarget[containerId] ?? "";
  const recent = state.networkUdpSources[containerId] ?? [];

  if (transport === "tcp") {
    const value = broadcast ? ALL_NETWORK_PEERS : (selectedPeerId ?? "");
    return (
      <div className={`${styles.bar} liquid-glass-panel`}>
        <span className={styles.label}>{t("network.targetLabel")}</span>
        <select
          className={`${styles.select} liquid-glass-input liquid-glass-select`}
          value={value}
          onChange={(event) => {
            const value = event.target.value;
            if (value === ALL_NETWORK_PEERS) {
              setNetworkBroadcast(containerId, true);
            } else {
              setNetworkBroadcast(containerId, false);
              selectNetworkPeer(containerId, value || null);
            }
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

  if (transport === "udp") {
    return (
      <div className={`${styles.bar} liquid-glass-panel`}>
        <span className={styles.label}>{t("network.targetLabel")}</span>
        <input
          className={`${styles.input} liquid-glass-input`}
          type="text"
          placeholder={t("network.manualTargetPlaceholder")}
          value={manualTarget}
          onChange={(event) => setNetworkManualTarget(containerId, event.target.value)}
          spellCheck={false}
        />
        <select
          className={`${styles.select} liquid-glass-input liquid-glass-select`}
          value=""
          onChange={(event) => {
            const value = event.target.value;
            if (value) setNetworkManualTarget(containerId, value);
          }}
          title={t("network.selectTarget")}
        >
          <option value="">{t("network.selectTarget")}</option>
          {recent.map(addr => (
            <option key={addr} value={addr}>{addr}</option>
          ))}
        </select>
      </div>
    );
  }

  return null;
}
