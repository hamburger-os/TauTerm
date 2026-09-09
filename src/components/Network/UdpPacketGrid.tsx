/**
 * UDP 报文网格 — 逐数据报展示（网络调试会话 UDP 模式）
 *
 * 每数据报一行：序号、时间戳、方向、来源/目标地址、长度、HEX、ASCII。
 * 数据来自后端逐报 flush 的 session-data 事件（报文边界保真）。
 */
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { PacketRow } from "./NetworkDebugSessionView";
import ScrollToBottomButton from "../Terminal/ScrollToBottomButton";
import ContextMenu, { type ContextMenuItem } from "../common/ContextMenu";
import type { ContextMenuState } from "../../hooks/useContextMenu";
import { useAutoScroll } from "../../hooks/useAutoScroll";
import styles from "./NetworkDebugSessionView.module.css";

interface Props {
  rows: PacketRow[];
  sessionId: string;
}

export default function UdpPacketGrid({ rows, sessionId }: Props) {
  const { t } = useTranslation();
  const { scrollRef, isAtBottom, handleScroll, scrollToBottom } = useAutoScroll<HTMLDivElement>(rows);
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
    <div className={styles.gridContainer}>
      <div className={styles.gridHeader}>
        <span className={styles.colSeq}>{t("network.seq")}</span>
        <span className={styles.colTime}>{t("network.time")}</span>
        <span className={styles.colDir}>{t("network.direction")}</span>
        <span className={styles.colPeer}>{t("network.peer")}</span>
        <span className={styles.colLen}>{t("network.length")}</span>
        <span className={styles.colHex}>HEX</span>
        <span className={styles.colAscii}>ASCII</span>
      </div>
      <div className={styles.gridBody} ref={scrollRef} onScroll={handleScroll}>
        {rows.length === 0 && (
          <div className={styles.gridEmpty}>{t("network.noData")}</div>
        )}
        {rows.map(row => (
          <div
            key={row.id}
            className={`${styles.gridRow} ${row.direction === "TX" ? styles.txRow : styles.rxRow}`}
            onContextMenu={(event) => openContextMenu(event, row.hex)}
          >
            <span className={styles.colSeq}>{row.id}</span>
            <span className={styles.colTime}>{row.time}</span>
            <span className={styles.colDir}>{row.direction}</span>
            <span className={styles.colPeer} title={row.peerLabel}>{row.peerLabel}</span>
            <span className={styles.colLen}>{row.length}</span>
            <span className={styles.colHex}>{row.hex}</span>
            <span className={styles.colAscii}>{row.text}</span>
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
    </div>
  );
}
