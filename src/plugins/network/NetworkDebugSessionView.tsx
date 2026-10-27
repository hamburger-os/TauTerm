/** Network data and presentation buffers belong to runtime-store; this view only projects snapshots. */
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import { usePluginRuntime } from "../../core/usePluginRuntime";
import { refreshNetworkPeers, type NetworkRuntimeSnapshot } from "./runtime-store";
import { useAutoScroll } from "../../hooks/useAutoScroll";
import DualPane, { type DualLine } from "../../components/Terminal/DualPane";
import ScrollToBottomButton from "../../components/Terminal/ScrollToBottomButton";
import ContextMenu, { type ContextMenuItem } from "../../components/common/ContextMenu";
import type { ContextMenuState } from "../../hooks/useContextMenu";
import Icon from "../../components/common/Icon";
import UdpPacketGrid from "./UdpPacketGrid";
import styles from "./NetworkDebugSessionView.module.css";
type TcpDisplayMode = "dual" | "text" | "hex";

// ── 主视图 ─────────────────────────────────────────

interface Props {
  sessionId: string;
}

export default function NetworkDebugSessionView({ sessionId }: Props) {
  const { t } = useTranslation();
  const { state: sessionState, updateSessionStats } = useSession();

  // 容器会话参数（连接时确定，稳定）：transport / role / encoding / data_mode
  const containerTab = sessionState.tabs.find(tab => tab.id === sessionId);
  const role = (containerTab?.params?.role as "client" | "server") ?? "client";
  /** 数据模式来自连接参数（与串口一致，会话内不可切换） */
  const displayMode = (containerTab?.params?.data_mode as TcpDisplayMode | undefined) ?? "dual";

  // 对端运行态完全由 Network 插件拥有；公共 SessionContext 不解释 peer 语义。
  const runtime = usePluginRuntime<NetworkRuntimeSnapshot>("network", sessionId);
  const peers = [...runtime.peers];
  const selectedPeerId = runtime.selectedPeerId;
  const snap = runtime;
  useEffect(() => { void refreshNetworkPeers(sessionId); }, [sessionId]);

  // ── 会话统计汇总 → 容器状态栏 ──
  // UDP 无对端：由视图内累计的 rx/tx 字节直接驱动；TCP 由对端统计聚合。
  useEffect(() => {
    if (snap.transport === "udp") {
      updateSessionStats(sessionId, snap.txBytes, snap.rxBytes, snap.rxPackets, snap.txPackets);
    } else {
      const tx = peers.reduce((s, p) => s + p.txBytes, 0);
      const rx = peers.reduce((s, p) => s + p.rxBytes, 0);
      updateSessionStats(sessionId, tx, rx);
    }
  }, [snap.transport, snap.txBytes, snap.rxBytes, snap.rxPackets, snap.txPackets, peers, sessionId, updateSessionStats]);

  // ── 渲染数据 ─────────────────────────────────────

  // TCP client 是单对端会话：渲染期直接以唯一对端为有效选中。
  // 避免 stored selection 为空/陈旧（切换瞬间、重连替换对端）时闪一帧"未选择对端"，
  // 自动选择 effect 随后会同步 stored selection，保持发送栏等消费方一致。
  const effectiveSelectedPeerId = role === "client" && peers.length === 1
    ? peers[0].peerId
    : selectedPeerId;
  const selectedPeer = peers.find(p => p.peerId === effectiveSelectedPeerId) ?? null;
  const containerConnected = containerTab?.state === "connected";
  const lifecycleHint = t("session.connectToViewContent", "连接后显示会话内容");

  return (
    <div className={styles.container} data-testid="network-workspace">
      {/* UDP：未连接时显示与终端会话一致的标准空状态；连接后展示全部来源时间线报文网格 */}
      {snap.transport === "udp" && (
        !containerConnected ? (
          <div className={styles.emptyState}>
            <Icon name="connection" size="xl" className={styles.emptyIcon} />
            <div>{lifecycleHint}</div>
          </div>
        ) : (
          <div className={styles.dataArea}>
            <UdpPacketGrid sessionId={sessionId} rows={snap.packets} />
          </div>
        )
      )}

      {/* TCP：未连接时显示与终端会话一致的标准空状态；连接后按对端状态渲染 */}
      {snap.transport === "tcp" && (
        !containerConnected ? (
          <div className={styles.emptyState}>
            <Icon name="connection" size="xl" className={styles.emptyIcon} />
            <div>{lifecycleHint}</div>
          </div>
        ) : peers.length === 0 ? (
          <div className={styles.emptyState}>
            <Icon name="globe" size="xl" className={styles.emptyIcon} />
            <div>{snap.role === "server" ? t("network.waitingClients") : t("network.noPeers")}</div>
          </div>
        ) : !selectedPeer ? (
          <div className={styles.emptyState}>
            <Icon name="globe" size="xl" className={styles.emptyIcon} />
            <div>{t("network.noSelectedPeer")}</div>
            <div className={styles.emptyHint}>{t("network.selectPeerHint")}</div>
          </div>
        ) : (
          <div className={styles.dataArea}>
            {displayMode === "dual" ? (
              <DualPane sessionId={sessionId} lines={snap.frames[selectedPeer.peerId] ?? []} />
            ) : displayMode === "text" ? (
              <TcpFrameList sessionId={sessionId} lines={snap.frames[selectedPeer.peerId] ?? []} mode="text" />
            ) : (
              <TcpFrameList sessionId={sessionId} lines={snap.frames[selectedPeer.peerId] ?? []} mode="hex" />
            )}
          </div>
        )
      )}
    </div>
  );
}

// ── TCP Text / Hex 单栏视图（从 DualLine 数据渲染） ──

function TcpFrameList({
  lines,
  sessionId,
  mode,
}: {
  lines: DualLine[];
  sessionId: string;
  mode: "text" | "hex";
}) {
  const { t } = useTranslation();
  const { scrollRef, isAtBottom, handleScroll, scrollToBottom } = useAutoScroll<HTMLDivElement>(lines);
  const [contextMenu, setContextMenu] = useState<ContextMenuState>({
    x: 0,
    y: 0,
    visible: false,
    session: null,
  });
  const [contextHex, setContextHex] = useState("");

  const contextMenuItems = useMemo<ContextMenuItem[]>(() => [
    {
      id: "inspectProtocol",
      label: t("terminal.inspectProtocolFrame"),
      icon: "search",
      disabled: !contextHex,
    },
  ], [contextHex, t]);

  const openContextMenu = useCallback((event: React.MouseEvent, hex: string) => {
    event.preventDefault();
    setContextHex(hex);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      visible: true,
      session: null,
    });
  }, []);

  const closeContextMenu = useCallback(() => {
    setContextMenu((previous) => ({ ...previous, visible: false }));
  }, []);

  const handleContextMenuSelect = useCallback((itemId: string) => {
    if (itemId !== "inspectProtocol" || !contextHex) return;
    window.dispatchEvent(
      new CustomEvent("tauterm:protocol-inspect", {
        detail: { sessionId, input: contextHex },
      }),
    );
  }, [contextHex, sessionId]);

  return (
    <>
      <div className={styles.singleList} ref={scrollRef} onScroll={handleScroll}>
        {lines.map((line) => (
          <div
            key={line.id}
            className={line.direction === "TX" ? styles.txLine : styles.rxLine}
            onContextMenu={(event) => openContextMenu(event, line.hex)}
          >
            <span className={styles.singleMeta}>[{line.direction}][{line.timestamp}]</span>
            {mode === "text" ? (
              <span className={styles.singleText}>{line.text}</span>
            ) : (
              <span className={styles.singleHex}>{line.hex}</span>
            )}
          </div>
        ))}
      </div>
      <ScrollToBottomButton visible={!isAtBottom} onClick={scrollToBottom} />
      <ContextMenu
        state={contextMenu}
        items={contextMenuItems}
        onSelect={handleContextMenuSelect}
        onClose={closeContextMenu}
      />
    </>
  );
}
