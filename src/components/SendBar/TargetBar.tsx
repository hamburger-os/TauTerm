import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { useSession } from "../../context/SessionContext";
import styles from "./TargetBar.module.css";

interface Props {
  containerId: string;
}

/** 群发伪目标值（TCP server 对端下拉里的「全部客户端」） */
const ALL_PEERS = "__all__";

/**
 * 判断会话参数是否需要显示目标栏（TargetBar）。
 * 仅 TCP/UDP server 角色存在可路由的多目标（对端下拉 / 手动 IP:port），其余返回 false。
 * 该判定与 App.tsx 的发送栏高度计算共用，需保持一致。
 */
export function isTargetBarVisible(params: Record<string, unknown> | undefined): boolean {
  const transport = params?.transport as string | undefined;
  const role = params?.role as string | undefined;
  return (transport === "tcp" || transport === "udp") && role === "server";
}

/**
 * 发送目标栏 — 横跨四个发送模式的共享目标选择器。
 *
 * 按会话 transport/role 渲染：
 * - TCP server：对端下拉（含「全部客户端」伪目标）；
 * - UDP server：手动 IP:port 输入 + 最近 RX 来源快捷回填；
 * - 其余（串口 / TCP client / UDP client）：返回 null（单一固定目标，无需选择）。
 *
 * 「当前目标」变化时同步到后端脚本引擎（set_network_send_target），
 * 使 auto-reply / script 的 send() 与界面选择保持一致。
 */
export default function TargetBar({ containerId }: Props) {
  const { t } = useTranslation();
  const { state, selectNetworkPeer, setNetworkBroadcast, setNetworkManualTarget } = useSession();

  const tab = state.tabs.find(t => t.id === containerId);
  const params = (tab?.params ?? {}) as Record<string, unknown>;
  const transport = params.transport as string | undefined;
  const targetBarVisible = isTargetBarVisible(params);

  const peers = (state.networkPeers[containerId] ?? []).filter(p => p.state === "connected");
  const selectedPeerId = state.selectedNetworkPeer[containerId] ?? null;
  const broadcast = state.networkBroadcast[containerId] === true;
  const manualTarget = state.networkManualTarget[containerId] ?? "";
  const recent = state.networkUdpSources[containerId] ?? [];

  // 同步「当前目标」到后端脚本引擎（仅 server 角色存在可路由目标）
  useEffect(() => {
    if (!targetBarVisible) return;
    const target = transport === "udp"
      ? (manualTarget.trim() || null)
      : (broadcast ? ALL_PEERS : selectedPeerId);
    invoke("set_network_send_target", { sessionId: containerId, target }).catch(() => { /* 后端尚未就绪时静默 */ });
  }, [containerId, targetBarVisible, transport, manualTarget, broadcast, selectedPeerId]);

  if (!targetBarVisible) return null;

  if (transport === "tcp") {
    const value = broadcast ? ALL_PEERS : (selectedPeerId ?? "");
    return (
      <div className={`${styles.bar} liquid-glass`}>
        <span className={styles.label}>{t("network.targetLabel")}</span>
        <select
          className={`${styles.select} liquid-glass-input liquid-glass-select`}
          value={value}
          onChange={(e) => {
            const v = e.target.value;
            if (v === ALL_PEERS) {
              setNetworkBroadcast(containerId, true);
            } else {
              setNetworkBroadcast(containerId, false);
              selectNetworkPeer(containerId, v || null);
            }
          }}
          title={t("network.selectTarget")}
        >
          <option value={ALL_PEERS}>{t("network.targetAllClients")}</option>
          {peers.map(p => (
            <option key={p.peerId} value={p.peerId}>{p.name} · {p.addr}</option>
          ))}
        </select>
      </div>
    );
  }

  if (transport === "udp") {
    return (
      <div className={`${styles.bar} liquid-glass`}>
        <span className={styles.label}>{t("network.targetLabel")}</span>
        <input
          className={`${styles.input} liquid-glass-input`}
          type="text"
          placeholder={t("network.manualTargetPlaceholder")}
          value={manualTarget}
          onChange={(e) => setNetworkManualTarget(containerId, e.target.value)}
          spellCheck={false}
        />
        <select
          className={`${styles.select} liquid-glass-input liquid-glass-select`}
          value=""
          onChange={(e) => { const v = e.target.value; if (v) setNetworkManualTarget(containerId, v); }}
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
