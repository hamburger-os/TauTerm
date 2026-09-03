import { useCallback, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSession, type TabInfo } from "../../context/SessionContext";
import { pluginRegistry } from "../../core/plugin-registry";
import type {
  DividerGeometry,
  PaneId,
  PaneRect,
  SplitEdge,
  SplitLayoutState,
} from "../../core/split-layout";
import TerminalView from "../Terminal/TerminalView";
import FileBrowserRenderer from "../../renderers/FileBrowserRenderer";
import StatsDashboardRenderer from "../../renderers/StatsDashboardRenderer";
import CustomRenderer from "../../renderers/CustomRenderer";
import styles from "./SplitView.module.css";

const MIN_PANE_PX = 160;
const EDGES: SplitEdge[] = ["left", "right", "top", "bottom"];

interface SplitViewProps {
  layout: SplitLayoutState;
  paneRects: Record<PaneId, PaneRect>;
  dividers: DividerGeometry[];
  blockedEdges: Record<PaneId, Set<SplitEdge>>;
  paneCount: number;
  onSelectPane: (paneId: PaneId) => void;
  onSplitPane: (paneId: PaneId, edge: SplitEdge) => void;
  onClosePane: (paneId: PaneId) => void;
  onResizeSplit: (splitId: string, ratio: number) => void;
}

interface PaneMenuState {
  paneId: PaneId;
  x: number;
  y: number;
}

function rectStyle(rect: PaneRect): React.CSSProperties {
  return {
    left: `${rect.left * 100}%`,
    top: `${rect.top * 100}%`,
    width: `${rect.width * 100}%`,
    height: `${rect.height * 100}%`,
  };
}

function renderNonTerminalContent(tab: TabInfo) {
  const plugin = pluginRegistry.get(tab.pluginId);
  const contentType = plugin?.manifest.content_type ?? "terminal";
  switch (contentType) {
    case "file_browser":
      return <FileBrowserRenderer tab={tab} />;
    case "stats_dashboard":
      return <StatsDashboardRenderer tab={tab} />;
    case "custom":
      return <CustomRenderer tab={tab} />;
    default:
      return null;
  }
}

function terminalHasRuntime(tab: TabInfo): boolean {
  return tab.state === "connected"
    || tab.state === "transferring"
    || Boolean(tab.disconnectInfo?.retain_terminal);
}

/**
 * Runtime-only Split View.
 *
 * Pane 只是显示槽；Session 生命周期仍由 SessionContext/插件 store 管理。
 * 最多四个 Pane，可从自由边缘继续分割；内部 Divider 只负责 resize。
 */
export default function SplitView({
  layout,
  paneRects,
  dividers,
  blockedEdges,
  paneCount,
  onSelectPane,
  onSplitPane,
  onClosePane,
  onResizeSplit,
}: SplitViewProps) {
  const { t } = useTranslation();
  const { state: sessionState } = useSession();
  const viewRef = useRef<HTMLDivElement>(null);
  const [hoveredSplit, setHoveredSplit] = useState<{ paneId: PaneId; edge: SplitEdge } | null>(null);
  const [paneMenu, setPaneMenu] = useState<PaneMenuState | null>(null);

  const tabsById = useMemo(() => {
    const map = new Map<string, TabInfo>();
    for (const tab of sessionState.tabs) map.set(tab.id, tab);
    return map;
  }, [sessionState.tabs]);

  const terminalPlacements = useMemo(() => {
    const result: Record<string, PaneRect> = {};
    for (const [paneId, sessionId] of Object.entries(layout.assignments)) {
      if (!sessionId) continue;
      const tab = tabsById.get(sessionId);
      if (!tab) continue;
      const plugin = pluginRegistry.get(tab.pluginId);
      if ((plugin?.manifest.content_type ?? "terminal") !== "terminal") continue;
      const rect = paneRects[paneId];
      if (rect) result[sessionId] = rect;
    }
    return result;
  }, [layout.assignments, paneRects, tabsById]);

  const terminalPaneIds = useMemo(() => {
    const result: Record<string, PaneId> = {};
    for (const [paneId, sessionId] of Object.entries(layout.assignments)) {
      if (!sessionId) continue;
      const tab = tabsById.get(sessionId);
      if (!tab) continue;
      const plugin = pluginRegistry.get(tab.pluginId);
      if ((plugin?.manifest.content_type ?? "terminal") === "terminal") {
        result[sessionId] = paneId;
      }
    }
    return result;
  }, [layout.assignments, tabsById]);

  const handleDividerMouseDown = useCallback((e: React.MouseEvent, divider: DividerGeometry) => {
    e.preventDefault();
    e.stopPropagation();
    const root = viewRef.current;
    if (!root) return;
    const bounds = root.getBoundingClientRect();
    const horizontal = divider.direction === "horizontal";
    document.body.style.userSelect = "none";
    document.body.style.cursor = horizontal ? "col-resize" : "row-resize";

    const handleMove = (event: MouseEvent) => {
      const regionStart = horizontal
        ? bounds.left + divider.rect.left * bounds.width
        : bounds.top + divider.rect.top * bounds.height;
      const regionSize = horizontal
        ? divider.rect.width * bounds.width
        : divider.rect.height * bounds.height;
      if (regionSize <= 0) return;
      const pointer = horizontal ? event.clientX : event.clientY;
      const minRatio = Math.min(0.45, MIN_PANE_PX / regionSize);
      const ratio = Math.max(minRatio, Math.min(1 - minRatio, (pointer - regionStart) / regionSize));
      onResizeSplit(divider.splitId, ratio);
    };

    const handleUp = () => {
      document.removeEventListener("mousemove", handleMove);
      document.removeEventListener("mouseup", handleUp);
      document.body.style.userSelect = "";
      document.body.style.cursor = "";
    };

    document.addEventListener("mousemove", handleMove);
    document.addEventListener("mouseup", handleUp);
  }, [onResizeSplit]);

  const openPaneMenu = useCallback((e: React.MouseEvent, paneId: PaneId) => {
    if (paneCount <= 1) return;
    e.preventDefault();
    e.stopPropagation();
    setPaneMenu({ paneId, x: e.clientX, y: e.clientY });
  }, [paneCount]);

  const previewRect = useMemo(() => {
    if (!hoveredSplit) return null;
    const base = paneRects[hoveredSplit.paneId];
    if (!base) return null;
    switch (hoveredSplit.edge) {
      case "left":
        return { ...base, width: base.width / 2 };
      case "right":
        return { ...base, left: base.left + base.width / 2, width: base.width / 2 };
      case "top":
        return { ...base, height: base.height / 2 };
      case "bottom":
        return { ...base, top: base.top + base.height / 2, height: base.height / 2 };
    }
  }, [hoveredSplit, paneRects]);

  return (
    <div
      ref={viewRef}
      className={styles.view}
      onMouseDown={() => setPaneMenu(null)}
    >
      {/* 非终端内容层与空 Pane。终端由下面唯一的 TerminalView 实例池覆盖投放。 */}
      {Object.entries(paneRects).map(([paneId, rect]) => {
        const sessionId = layout.assignments[paneId] ?? null;
        const tab = sessionId ? tabsById.get(sessionId) : undefined;
        const plugin = tab ? pluginRegistry.get(tab.pluginId) : null;
        const contentType = plugin?.manifest.content_type ?? "terminal";
        const isTerminal = Boolean(tab) && contentType === "terminal";
        const showTerminalPlaceholder = isTerminal && tab ? !terminalHasRuntime(tab) : false;
        return (
          <div
            key={`surface-${paneId}`}
            className={styles.paneSurface}
            style={rectStyle(rect)}
            onMouseDown={() => onSelectPane(paneId)}
            onContextMenu={(e) => {
              if (!isTerminal || !tab) openPaneMenu(e, paneId);
            }}
          >
            {!tab && (
              <div className={styles.emptyPane}>
                <span className={styles.emptyMark}>+</span>
                <span>{t("split.selectSession", "选择左侧会话")}</span>
              </div>
            )}
            {tab && !isTerminal && renderNonTerminalContent(tab)}
            {tab && showTerminalPlaceholder && (
              <div className={styles.emptyPane}>
                <span className={styles.disconnectedDot} />
                <span>{tab.name}</span>
                <small>{t("session.disconnected", "未连接")}</small>
              </div>
            )}
          </div>
        );
      })}

      <TerminalView
        dockedPlacements={terminalPlacements}
        onActivateSession={(sessionId) => {
          const paneId = terminalPaneIds[sessionId];
          if (paneId) onSelectPane(paneId);
        }}
      />

      {/* Pane Chrome：选中态、会话 badge、右键关闭入口与边缘分屏入口。 */}
      {Object.entries(paneRects).map(([paneId, rect]) => {
        const sessionId = layout.assignments[paneId] ?? null;
        const tab = sessionId ? tabsById.get(sessionId) : undefined;
        const selected = paneId === layout.selectedPaneId;
        const blocked = blockedEdges[paneId] ?? new Set<SplitEdge>();
        return (
          <div key={`chrome-${paneId}`} className={styles.paneChrome} style={rectStyle(rect)}>
            <div className={`${styles.paneFrame} ${selected ? styles.selectedFrame : ""}`} />
            {paneCount > 1 && tab && (
              <button
                type="button"
                className={`${styles.sessionBadge} ${selected ? styles.selectedBadge : ""}`}
                onMouseDown={(e) => {
                  e.stopPropagation();
                  onSelectPane(paneId);
                }}
                onContextMenu={(e) => openPaneMenu(e, paneId)}
                title={tab.name}
              >
                {tab.name}
              </button>
            )}
            {paneCount < 4 && EDGES.map(edge => {
              if (blocked.has(edge)) return null;
              return (
                <button
                  key={edge}
                  type="button"
                  className={`${styles.splitTrigger} ${styles[`splitTrigger${edge[0].toUpperCase()}${edge.slice(1)}`]}`}
                  onMouseEnter={() => setHoveredSplit({ paneId, edge })}
                  onMouseLeave={() => setHoveredSplit(current => current?.paneId === paneId && current.edge === edge ? null : current)}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                  }}
                  onClick={(e) => {
                    e.stopPropagation();
                    setHoveredSplit(null);
                    onSplitPane(paneId, edge);
                  }}
                  aria-label={`${t("split.split", "分屏")} ${edge}`}
                  title={t("split.edgeHint", "点击从此边缘创建分屏")}
                >
                  <span />
                </button>
              );
            })}
          </div>
        );
      })}

      {previewRect && (
        <div className={styles.splitPreview} style={rectStyle(previewRect)}>
          <span>{t("split.newPane", "新分屏")}</span>
        </div>
      )}

      {dividers.map(divider => {
        const horizontal = divider.direction === "horizontal";
        const boundary = horizontal
          ? divider.rect.left + divider.rect.width * divider.ratio
          : divider.rect.top + divider.rect.height * divider.ratio;
        const dividerStyle: React.CSSProperties = horizontal
          ? {
              left: `${boundary * 100}%`,
              top: `${divider.rect.top * 100}%`,
              height: `${divider.rect.height * 100}%`,
            }
          : {
              top: `${boundary * 100}%`,
              left: `${divider.rect.left * 100}%`,
              width: `${divider.rect.width * 100}%`,
            };
        return (
          <div
            key={divider.splitId}
            className={`${styles.divider} ${horizontal ? styles.verticalDivider : styles.horizontalDivider}`}
            style={dividerStyle}
            onMouseDown={(e) => handleDividerMouseDown(e, divider)}
          >
            <span />
          </div>
        );
      })}

      {paneMenu && (
        <div
          className={`${styles.paneMenu} liquid-glass-float`}
          style={{ left: paneMenu.x, top: paneMenu.y }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          <button
            type="button"
            onClick={() => {
              const paneId = paneMenu.paneId;
              setPaneMenu(null);
              onClosePane(paneId);
            }}
          >
            {t("split.closePane", "关闭分屏")}
          </button>
        </div>
      )}
    </div>
  );
}
