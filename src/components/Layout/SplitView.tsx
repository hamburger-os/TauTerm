import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSession, type TabInfo } from "../../context/SessionContext";
import { pluginRegistry } from "../../core/plugin-registry";
import {
  MAX_WORKSPACE_PANES,
  type DividerGeometry,
  type PaneId,
  type PaneRect,
  type SplitEdge,
  type SplitLayoutState,
} from "../../core/split-layout";
import { useContextMenu, type ContextMenuState } from "../../hooks/useContextMenu";
import { useKeyboard } from "../../hooks/useKeyboard";
import { ACTION_IDS } from "../../shortcuts/actionIds";
import TerminalView from "../Terminal/TerminalView";
import ContextMenu, { type ContextMenuItem } from "../common/ContextMenu";
import Icon from "../common/Icon";
import FileBrowserRenderer from "../../renderers/FileBrowserRenderer";
import StatsDashboardRenderer from "../../renderers/StatsDashboardRenderer";
import CustomRenderer from "../../renderers/CustomRenderer";
import DisconnectedSessionContextMenu from "./DisconnectedSessionContextMenu";
import { getPaneDisplayLabel } from "./sessionPresentation";
import styles from "./SplitView.module.css";

const MIN_PANE_PX = 160;
const PANE_HEADER_PX = 24;
/** Pointer distance (CSS px) at which a pane corner fades in. */
const CORNER_REVEAL_RADIUS_PX = 26;

type PaneCorner = "top-left" | "top-right" | "bottom-left" | "bottom-right";
const SPLIT_CORNERS: PaneCorner[] = ["top-left", "top-right", "bottom-left", "bottom-right"];

interface SplitViewProps {
  layout: SplitLayoutState;
  paneRects: Record<PaneId, PaneRect>;
  dividers: DividerGeometry[];
  paneCount: number;
  onSelectPane: (paneId: PaneId) => void;
  onSplitPane: (paneId: PaneId, edge: SplitEdge) => void;
  onClearPane: (paneId: PaneId) => void;
  onClosePane: (paneId: PaneId) => void;
  onResizeSplit: (splitId: string, ratio: number) => void;
}

interface PaneMenuState {
  paneId: PaneId;
  x: number;
  y: number;
}

interface NonTerminalPlacement {
  paneId: PaneId;
  rect: PaneRect;
}

function rectStyle(rect: PaneRect): React.CSSProperties {
  return {
    left: `${rect.left * 100}%`,
    top: `${rect.top * 100}%`,
    width: `${rect.width * 100}%`,
    height: `${rect.height * 100}%`,
  };
}

function insetPaneContent(rect: PaneRect, paneCount: number, viewportHeight: number): PaneRect {
  if (paneCount <= 1 || viewportHeight <= 0) return rect;
  const paneHeightPx = rect.height * viewportHeight;
  const headerPx = Math.min(PANE_HEADER_PX, Math.max(0, paneHeightPx));
  const headerRatio = headerPx / viewportHeight;
  return {
    ...rect,
    top: rect.top + headerRatio,
    height: Math.max(0, rect.height - headerRatio),
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
    || Boolean(tab.disconnectInfo?.retain_terminal);
}

function getConnectionStateFallback(state: TabInfo["state"]): string {
  switch (state) {
    case "connecting":
      return "连接中...";
    case "connected":
      return "已连接";
    case "disconnecting":
      return "断开中...";
    case "disconnected":
    default:
      return "未连接";
  }
}

function shouldShowDisconnectedPlaceholder(
  tab: TabInfo,
  contentType: string,
  workspaceAvailability: "connected" | "always" = "connected",
): boolean {
  if (contentType === "terminal") return !terminalHasRuntime(tab);
  return workspaceAvailability !== "always" && tab.state === "disconnected";
}

function PaneEmptyState({ message }: { message: string }) {
  return (
    <div className={styles.emptyPane}>
      <span className={styles.emptyMark}>
        <Icon name="connection" size="xl" className={styles.emptyPaneIcon} />
      </span>
      <span>{message}</span>
    </div>
  );
}

/**
 * Split View surface for the persisted local Workspace layout.
 *
 * Pane 只是显示槽；Session 生命周期仍由 SessionContext/插件 store 管理。
 * 最多 MAX_WORKSPACE_PANES 个 Pane，可从自由边缘继续分割；内部 Divider 只负责 resize。
 */
export default function SplitView({
  layout,
  paneRects,
  dividers,
  paneCount,
  onSelectPane,
  onSplitPane,
  onClearPane,
  onClosePane,
  onResizeSplit,
}: SplitViewProps) {
  const { t } = useTranslation();
  const { state: sessionState } = useSession();
  const { registerAction, unregisterAction } = useKeyboard();
  const viewRef = useRef<HTMLDivElement>(null);
  const [viewSize, setViewSize] = useState({ width: 0, height: 0 });
  const [paneMenu, setPaneMenu] = useState<PaneMenuState | null>(null);
  // Hovering a pane corner previews and commits a split toward the nearest of
  // that corner's two edges; the rest of the content surface stays untouched so
  // xterm selection and TUI pointer input always receive their events.
  const [splitIntent, setSplitIntent] = useState<{ paneId: PaneId; edge: SplitEdge } | null>(null);
  // A corner only exists while the pointer is next to it: no persistent dots on
  // the terminal surface, and no invisible hit-target stealing corner clicks from
  // a full-screen TUI.
  const [activeCorner, setActiveCorner] = useState<{ paneId: PaneId; corner: PaneCorner } | null>(null);
  const [retainedNonTerminalSessionIds, setRetainedNonTerminalSessionIds] = useState<Set<string>>(
    () => new Set(),
  );
  const {
    menu: disconnectedSessionMenu,
    openMenu: openDisconnectedSessionMenu,
    closeMenu: closeDisconnectedSessionMenu,
  } = useContextMenu();

  useLayoutEffect(() => {
    const root = viewRef.current;
    if (!root) return;
    const update = () => {
      const bounds = root.getBoundingClientRect();
      const width = Math.round(bounds.width);
      const height = Math.round(bounds.height);
      setViewSize(current => (
        current.width === width && current.height === height
          ? current
          : { width, height }
      ));
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(root);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (paneCount >= MAX_WORKSPACE_PANES) setSplitIntent(null);
  }, [paneCount]);

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
      if (rect) result[sessionId] = insetPaneContent(rect, paneCount, viewSize.height);
    }
    return result;
  }, [layout.assignments, paneRects, paneCount, tabsById, viewSize.height]);

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

  const nonTerminalPlacements = useMemo(() => {
    const result: Record<string, NonTerminalPlacement> = {};
    for (const [paneId, sessionId] of Object.entries(layout.assignments)) {
      if (!sessionId) continue;
      const tab = tabsById.get(sessionId);
      if (!tab) continue;
      const plugin = pluginRegistry.get(tab.pluginId);
      const contentType = plugin?.manifest.content_type ?? "terminal";
      if (contentType === "terminal") continue;
      if (shouldShowDisconnectedPlaceholder(tab, contentType, plugin?.workspace?.availability)) continue;
      const rect = paneRects[paneId];
      if (!rect) continue;
      result[sessionId] = {
        paneId,
        rect: insetPaneContent(rect, paneCount, viewSize.height),
      };
    }
    return result;
  }, [layout.assignments, paneRects, paneCount, tabsById, viewSize.height]);

  // Non-terminal Session views are Session-owned, not Pane-owned. Once a view has
  // been opened, switching/clearing Pane assignment only hides or moves it; the
  // React instance stays alive while the Session runtime is alive. This preserves
  // in-process UI state and prevents view unmount from accidentally stopping work.
  // Connected-only workspaces are released after disconnect; `always` workspaces
  // remain alive because their offline/client workbench is itself valid runtime UI.
  useEffect(() => {
    setRetainedNonTerminalSessionIds(previous => {
      const next = new Set<string>();
      for (const sessionId of previous) {
        const tab = tabsById.get(sessionId);
        if (!tab) continue;
        const plugin = pluginRegistry.get(tab.pluginId);
        const contentType = plugin?.manifest.content_type ?? "terminal";
        if (contentType === "terminal") continue;
        if (plugin?.workspace?.availability === "always" || tab.state !== "disconnected") {
          next.add(sessionId);
        }
      }
      for (const sessionId of Object.keys(nonTerminalPlacements)) next.add(sessionId);
      if (
        next.size === previous.size
        && [...next].every(sessionId => previous.has(sessionId))
      ) {
        return previous;
      }
      return next;
    });
  }, [nonTerminalPlacements, tabsById]);

  const nonTerminalSessionPoolIds = useMemo(() => {
    const ids = new Set(retainedNonTerminalSessionIds);
    for (const sessionId of Object.keys(nonTerminalPlacements)) ids.add(sessionId);
    return [...ids];
  }, [nonTerminalPlacements, retainedNonTerminalSessionIds]);

  const handleDividerMouseDown = useCallback((e: React.MouseEvent, divider: DividerGeometry) => {
    e.preventDefault();
    e.stopPropagation();
    const root = viewRef.current;
    if (!root) return;
    const bounds = root.getBoundingClientRect();
    const horizontal = divider.direction === "horizontal";
    document.body.style.userSelect = "none";
    document.body.style.cursor = horizontal ? "col-resize" : "row-resize";

    let animationFrame: number | null = null;
    let pendingRatio: number | null = null;

    const commitPendingRatio = () => {
      animationFrame = null;
      const ratio = pendingRatio;
      pendingRatio = null;
      if (ratio !== null) onResizeSplit(divider.splitId, ratio);
    };

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
      pendingRatio = Math.max(minRatio, Math.min(1 - minRatio, (pointer - regionStart) / regionSize));
      if (animationFrame === null) {
        animationFrame = requestAnimationFrame(commitPendingRatio);
      }
    };

    const handleUp = () => {
      document.removeEventListener("mousemove", handleMove);
      document.removeEventListener("mouseup", handleUp);
      window.removeEventListener("blur", handleUp);
      if (animationFrame !== null) {
        cancelAnimationFrame(animationFrame);
        animationFrame = null;
      }
      commitPendingRatio();
      document.body.style.userSelect = "";
      document.body.style.cursor = "";
    };

    document.addEventListener("mousemove", handleMove);
    document.addEventListener("mouseup", handleUp);
    window.addEventListener("blur", handleUp);
  }, [onResizeSplit]);

  const openPaneMenuAt = useCallback((paneId: PaneId, x: number, y: number) => {
    closeDisconnectedSessionMenu();
    setPaneMenu({ paneId, x, y });
  }, [closeDisconnectedSessionMenu]);

  const openPaneMenu = useCallback((e: React.MouseEvent, paneId: PaneId) => {
    e.preventDefault();
    e.stopPropagation();
    openPaneMenuAt(paneId, e.clientX, e.clientY);
  }, [openPaneMenuAt]);

  const closePaneMenu = useCallback(() => {
    setPaneMenu(null);
  }, []);

  const openDisconnectedMenu = useCallback((e: React.MouseEvent, paneId: PaneId, tab: TabInfo) => {
    e.preventDefault();
    e.stopPropagation();
    setPaneMenu(null);
    onSelectPane(paneId);
    openDisconnectedSessionMenu(e, tab);
  }, [onSelectPane, openDisconnectedSessionMenu]);

  const canOfferSplit = useCallback((rect: PaneRect, edge: SplitEdge): boolean => {
    if (paneCount >= MAX_WORKSPACE_PANES) return false;
    const axisSize = edge === "left" || edge === "right"
      ? rect.width * viewSize.width
      : rect.height * viewSize.height;
    if (axisSize <= 0) return true;
    return axisSize >= MIN_PANE_PX * 2;
  }, [paneCount, viewSize.height, viewSize.width]);

  /** Nearest of the two edges that meet at a corner decides the split direction. */
  const edgeForCorner = (
    corner: PaneCorner,
    event: React.MouseEvent<HTMLElement>,
  ): SplitEdge => {
    const bounds = event.currentTarget.getBoundingClientRect();
    const dx = corner.endsWith("left")
      ? event.clientX - bounds.left
      : bounds.right - event.clientX;
    const dy = corner.startsWith("top")
      ? event.clientY - bounds.top
      : bounds.bottom - event.clientY;
    if (dx <= dy) return corner.endsWith("left") ? "left" : "right";
    return corner.startsWith("top") ? "top" : "bottom";
  };

  const previewRect = useMemo(() => {
    if (!splitIntent || paneCount >= MAX_WORKSPACE_PANES) return null;
    const base = paneRects[splitIntent.paneId];
    if (!base || !canOfferSplit(base, splitIntent.edge)) return null;
    switch (splitIntent.edge) {
      case "left":
        return { ...base, width: base.width / 2 };
      case "right":
        return { ...base, left: base.left + base.width / 2, width: base.width / 2 };
      case "top":
        return { ...base, height: base.height / 2 };
      case "bottom":
        return { ...base, top: base.top + base.height / 2, height: base.height / 2 };
    }
  }, [splitIntent, paneCount, paneRects, canOfferSplit]);

  /** Split target: the pane that owns the active Session, else the selected pane. */
  const resolveSplitPaneId = useCallback((): PaneId | null => {
    const activeSessionId = sessionState.activeTabId;
    if (activeSessionId) {
      const entry = Object.entries(layout.assignments)
        .find(([, sessionId]) => sessionId === activeSessionId);
      if (entry) return entry[0] as PaneId;
    }
    return layout.selectedPaneId;
  }, [layout.assignments, layout.selectedPaneId, sessionState.activeTabId]);

  const requestSplit = useCallback((paneId: PaneId, edge: SplitEdge): boolean => {
    const rect = paneRects[paneId];
    if (!rect || !canOfferSplit(rect, edge)) return false;
    onSplitPane(paneId, edge);
    return true;
  }, [canOfferSplit, onSplitPane, paneRects]);

  // Split has one action path shared by the Pane menu, the Command Palette and
  // the configurable shortcut registry; edge hot-zones over terminal content are
  // deliberately gone so text selection and TUI pointer semantics always win.
  useEffect(() => {
    const splitActivePane = (edge: SplitEdge) => {
      const paneId = resolveSplitPaneId();
      if (paneId) requestSplit(paneId, edge);
    };
    registerAction(ACTION_IDS.WORKSPACE_SPLIT_RIGHT, () => splitActivePane("right"));
    registerAction(ACTION_IDS.WORKSPACE_SPLIT_DOWN, () => splitActivePane("bottom"));
    return () => {
      unregisterAction(ACTION_IDS.WORKSPACE_SPLIT_RIGHT);
      unregisterAction(ACTION_IDS.WORKSPACE_SPLIT_DOWN);
    };
  }, [registerAction, unregisterAction, requestSplit, resolveSplitPaneId]);

  const paneContextMenuState = useMemo<ContextMenuState>(() => ({
    x: paneMenu?.x ?? 0,
    y: paneMenu?.y ?? 0,
    visible: paneMenu !== null,
    session: null,
    extension: null,
  }), [paneMenu]);

  const paneMenuItems = useMemo<ContextMenuItem[]>(() => {
    if (!paneMenu) return [];
    const rect = paneRects[paneMenu.paneId];
    const occupied = Boolean(layout.assignments[paneMenu.paneId]);
    const canSplitRight = Boolean(rect && canOfferSplit(rect, "right"));
    const canSplitDown = Boolean(rect && canOfferSplit(rect, "bottom"));
    return [
      {
        id: "clear-pane",
        label: t("split.clearPane", { defaultValue: "清空分屏" }),
        disabled: !occupied,
      },
      { id: "pane-separator-1", label: "", type: "separator" },
      {
        id: "split-right",
        label: t("split.splitRight", { defaultValue: "向右分屏" }),
        disabled: !canSplitRight,
      },
      {
        id: "split-down",
        label: t("split.splitDown", { defaultValue: "向下分屏" }),
        disabled: !canSplitDown,
      },
      { id: "pane-separator-2", label: "", type: "separator" },
      {
        id: "close-pane",
        label: t("split.closePane", { defaultValue: "关闭分屏" }),
        disabled: paneCount <= 1,
      },
    ];
  }, [paneMenu, paneRects, layout.assignments, canOfferSplit, paneCount, t]);

  const handlePaneMenuSelect = useCallback((itemId: string) => {
    const paneId = paneMenu?.paneId;
    if (!paneId) return;
    setPaneMenu(null);
    switch (itemId) {
      case "clear-pane":
        onClearPane(paneId);
        break;
      case "split-right":
        onSplitPane(paneId, "right");
        break;
      case "split-down":
        onSplitPane(paneId, "bottom");
        break;
      case "close-pane":
        onClosePane(paneId);
        break;
    }
  }, [paneMenu, onClearPane, onClosePane, onSplitPane]);

  return (
    <div
      ref={viewRef}
      className={`${styles.view} liquid-glass-content`}
      onMouseMove={(event) => {
        const bounds = viewRef.current?.getBoundingClientRect();
        if (!bounds || bounds.width === 0 || bounds.height === 0) return;
        const x = event.clientX - bounds.left;
        const y = event.clientY - bounds.top;
        let nearest: { paneId: PaneId; corner: PaneCorner; distance: number } | null = null;
        for (const [paneId, rect] of Object.entries(paneRects)) {
          const left = rect.left * bounds.width;
          const top = rect.top * bounds.height;
          const right = (rect.left + rect.width) * bounds.width;
          const bottom = (rect.top + rect.height) * bounds.height;
          const corners: Array<[PaneCorner, number, number]> = [
            ["top-left", left, top],
            ["top-right", right, top],
            ["bottom-left", left, bottom],
            ["bottom-right", right, bottom],
          ];
          for (const [corner, cornerX, cornerY] of corners) {
            const distance = Math.hypot(x - cornerX, y - cornerY);
            if (!nearest || distance < nearest.distance) nearest = { paneId, corner, distance };
          }
        }
        const revealed = nearest && nearest.distance <= CORNER_REVEAL_RADIUS_PX ? nearest : null;
        setActiveCorner(current => {
          if (current?.paneId === revealed?.paneId && current?.corner === revealed?.corner) return current;
          return revealed ? { paneId: revealed.paneId, corner: revealed.corner } : null;
        });
        if (!revealed) setSplitIntent(null);
      }}
      onMouseLeave={() => {
        setActiveCorner(null);
        setSplitIntent(null);
      }}
    >
      {/* Pane 材质、空 Pane 与断连占位。实际非终端视图由下方 Session 实例池覆盖投放。 */}
      {Object.entries(paneRects).map(([paneId, rect]) => {
        const sessionId = layout.assignments[paneId] ?? null;
        const tab = sessionId ? tabsById.get(sessionId) : undefined;
        const plugin = tab ? pluginRegistry.get(tab.pluginId) : null;
        const contentType = plugin?.manifest.content_type ?? "terminal";
        const isTerminal = Boolean(tab) && contentType === "terminal";
        const showDisconnectedPlaceholder = tab
          ? shouldShowDisconnectedPlaceholder(tab, contentType, plugin?.workspace?.availability)
          : false;
        const selected = paneId === layout.selectedPaneId;
        const contentRect = insetPaneContent(rect, paneCount, viewSize.height);
        const ownsPaneBackground = !tab || !isTerminal || showDisconnectedPlaceholder;
        const paneMaterial = ownsPaneBackground && paneCount > 1
          ? (selected ? "liquid-glass-content-active" : "liquid-glass-content-inactive")
          : "";
        return (
          <div
            key={`surface-${paneId}`}
            className={`${styles.paneSurface} ${contentType === "custom" ? styles.customPaneSurface : ""} ${paneMaterial}`}
            style={rectStyle(contentRect)}
            onMouseDown={(e) => {
              if (e.button === 0) onSelectPane(paneId);
            }}
            onContextMenu={(e) => {
              if (showDisconnectedPlaceholder && tab) {
                openDisconnectedMenu(e, paneId, tab);
                return;
              }
              if (!isTerminal || !tab) {
                // Pane-level actions belong exclusively to Pane Header.
                // Suppress the WebView default menu on custom/empty content without stealing the gesture.
                e.preventDefault();
                e.stopPropagation();
              }
            }}
          >
            {!tab && (
              <PaneEmptyState message={t("split.selectSession", "选择左侧会话")} />
            )}
            {tab && showDisconnectedPlaceholder && (
              <PaneEmptyState message={t("session.connectToViewContent", "连接后显示会话内容")} />
            )}
          </div>
        );
      })}

      {/*
       * Session-owned non-terminal instance pool. A Pane assignment is only a
       * projection: changing the selected Session moves/hides an existing view
       * instead of unmounting it, so background work and ephemeral UI state are
       * not coupled to Pane visibility.
       */}
      {nonTerminalSessionPoolIds.map(sessionId => {
        const tab = tabsById.get(sessionId);
        if (!tab) return null;
        const plugin = pluginRegistry.get(tab.pluginId);
        const contentType = plugin?.manifest.content_type ?? "terminal";
        if (contentType === "terminal") return null;
        const placement = nonTerminalPlacements[sessionId];
        const placementStyle: React.CSSProperties = placement
          ? rectStyle(placement.rect)
          : { display: "none" };
        return (
          <div
            key={`session-surface-${tab.pluginId}:${sessionId}`}
            className={`${styles.paneSurface} ${contentType === "custom" ? styles.customPaneSurface : ""}`}
            style={placementStyle}
            onMouseDown={(e) => {
              if (e.button === 0 && placement) onSelectPane(placement.paneId);
            }}
            onContextMenu={(e) => {
              // Pane-level actions belong exclusively to Pane Header. Plugin
              // content may stop propagation first when it owns a context menu.
              e.preventDefault();
              e.stopPropagation();
            }}
          >
            {renderNonTerminalContent(tab)}
          </div>
        );
      })}

      <TerminalView
        dockedPlacements={terminalPlacements}
        paneCount={paneCount}
        onActivateSession={(sessionId) => {
          const paneId = terminalPaneIds[sessionId];
          if (paneId) onSelectPane(paneId);
        }}
      />

      {/* Pane Header Layer：标题栏拥有 Pane 级菜单；Workspace root 独占外框。 */}
      {Object.entries(paneRects).map(([paneId, rect]) => {
        const sessionId = layout.assignments[paneId] ?? null;
        const tab = sessionId ? tabsById.get(sessionId) : undefined;
        const selected = paneId === layout.selectedPaneId;
        const showSelection = paneCount > 1 && selected;
        const paneTitle = tab
          ? getPaneDisplayLabel(tab, tabsById)
          : t("split.emptyPane", "空分屏");
        const paneTitleTooltip = tab?.elevated
          ? `${paneTitle} · ${t("localShell.administrator", "管理员")}`
          : paneTitle;
        const stateLabel = tab
          ? t(`session.${tab.state}`, getConnectionStateFallback(tab.state))
          : "";
        const lifecycleClass = tab?.state === "connected"
          ? styles.lifecycleConnected
          : tab?.state === "connecting"
            ? styles.lifecycleConnecting
            : styles.lifecycleDisconnected;
        return (
          <div key={`header-layer-${paneId}`} className={styles.paneHeaderLayer} style={rectStyle(rect)}>
            {paneCount > 1 && (
              <div
                className={`${styles.paneHeader} ${showSelection ? styles.selectedHeader : ""}`}
                onMouseDown={(e) => {
                  e.stopPropagation();
                  // Secondary-button press must not activate the Pane before contextmenu.
                  // Otherwise SendBar/RightSidebar can reflow and move the Header out from under the pointer.
                  if (e.button === 0) onSelectPane(paneId);
                }}
                onContextMenu={(e) => openPaneMenu(e, paneId)}
              >
                <span className={`${styles.paneHeaderTitle} ${tab ? "" : styles.emptyPaneTitle}`} title={paneTitleTooltip}>
                  {paneTitle}
                </span>
                {tab?.elevated && (
                  <span className={styles.paneHeaderBadge} title={t("localShell.administrator", "管理员")}>
                    <Icon name="shield" size="xs" />
                  </span>
                )}
                {tab && (
                  <span className={styles.paneHeaderState} title={stateLabel}>
                    <span className={`${styles.lifecycleDot} ${lifecycleClass}`} />
                    <span className={styles.paneHeaderStateText}>{stateLabel}</span>
                  </span>
                )}
                <button
                  type="button"
                  className={styles.paneHeaderMenuButton}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                  }}
                  onClick={(e) => {
                    e.stopPropagation();
                    const bounds = e.currentTarget.getBoundingClientRect();
                    openPaneMenuAt(paneId, bounds.right, bounds.bottom + 2);
                  }}
                  aria-label={t("split.paneMenu", { defaultValue: "分屏菜单" })}
                  title={t("split.paneMenu", { defaultValue: "分屏菜单" })}
                >
                  <span aria-hidden="true">…</span>
                </button>
              </div>
            )}
            {paneCount < MAX_WORKSPACE_PANES && SPLIT_CORNERS.map(corner => {
              const horizontalEdge: SplitEdge = corner.endsWith("left") ? "left" : "right";
              const verticalEdge: SplitEdge = corner.startsWith("top") ? "top" : "bottom";
              if (!canOfferSplit(rect, horizontalEdge) && !canOfferSplit(rect, verticalEdge)) {
                return null;
              }
              const revealed = activeCorner?.paneId === paneId && activeCorner.corner === corner;
              const cornerStyle: React.CSSProperties = {
                ...(corner.startsWith("top")
                  ? { top: paneCount > 1 ? PANE_HEADER_PX : 1 }
                  : { bottom: 1 }),
                ...(corner.endsWith("left") ? { left: 1 } : { right: 1 }),
                opacity: revealed ? 0.9 : 0,
                pointerEvents: revealed ? "auto" : "none",
              };
              return (
                <button
                  key={`${paneId}-${corner}`}
                  type="button"
                  className={styles.splitCorner}
                  style={cornerStyle}
                  tabIndex={revealed ? 0 : -1}
                  aria-hidden={!revealed}
                  onMouseEnter={(e) => setSplitIntent({ paneId, edge: edgeForCorner(corner, e) })}
                  onMouseMove={(e) => {
                    const edge = edgeForCorner(corner, e);
                    setSplitIntent(current => (
                      current && current.paneId === paneId && current.edge === edge
                        ? current
                        : { paneId, edge }
                    ));
                  }}
                  onMouseLeave={() => setSplitIntent(current => (
                    current?.paneId === paneId ? null : current
                  ))}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                  }}
                  onClick={(e) => {
                    e.stopPropagation();
                    setSplitIntent(null);
                    onSplitPane(paneId, edgeForCorner(corner, e));
                  }}
                  aria-label={t("split.cornerHint", { defaultValue: "沿最近的边缘分屏" })}
                  title={t("split.cornerHint", { defaultValue: "沿最近的边缘分屏" })}
                >
                  <span aria-hidden="true" />
                </button>
              );
            })}
          </div>
        );
      })}

      {previewRect && (
        <div className={styles.splitPreview} style={rectStyle(previewRect)}>
          <span>{t("split.newPane", { defaultValue: "新分屏" })}</span>
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

      <DisconnectedSessionContextMenu
        state={disconnectedSessionMenu}
        onClose={closeDisconnectedSessionMenu}
      />
      <ContextMenu
        state={paneContextMenuState}
        items={paneMenuItems}
        onSelect={handlePaneMenuSelect}
        onClose={closePaneMenu}
      />
    </div>
  );
}
