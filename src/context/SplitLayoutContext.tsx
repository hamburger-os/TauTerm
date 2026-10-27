import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSession } from "./SessionContext";
import i18n from "../i18n";
import {
  MAX_WORKSPACE_PANES,
  activateSessionInLayout,
  clearPaneInLayout,
  closePaneInLayout,
  collectPaneIds,
  computeBlockedEdges,
  computeDividerGeometries,
  computePaneRects,
  countPanes,
  createInitialSplitLayout,
  findPaneForSession,
  pruneAssignments,
  remapRemovedChildrenToDisconnectedRoots,
  selectPaneInLayout,
  setSplitRatioInLayout,
  splitPaneInLayout,
  type DividerGeometry,
  type LayoutNode,
  type PaneId,
  type PaneRect,
  type SplitEdge,
  type SplitLayoutState,
} from "../core/split-layout";
import {
  parsePersistedWorkspaceLayout,
  serializeWorkspaceLayout,
  canPersistWorkspaceLayout,
} from "../core/workspace-layout";

interface SplitLayoutContextValue {
  state: SplitLayoutState;
  paneRects: Record<PaneId, PaneRect>;
  dividers: DividerGeometry[];
  blockedEdges: Record<PaneId, Set<SplitEdge>>;
  paneCount: number;
  sessionToPane: Record<string, PaneId>;
  selectPane: (paneId: PaneId) => void;
  splitPane: (paneId: PaneId, edge: SplitEdge) => void;
  clearPane: (paneId: PaneId) => void;
  closePane: (paneId: PaneId) => void;
  resizeSplit: (splitId: string, ratio: number) => void;
  activateSession: (sessionId: string) => void;
}

const SplitLayoutContext = createContext<SplitLayoutContextValue | null>(null);

let nextPaneNumber = 2;
let nextSplitNumber = 1;

function hasSplitId(node: LayoutNode, splitId: string): boolean {
  if (node.type === "pane") return false;
  if (node.id === splitId) return true;
  return hasSplitId(node.first, splitId) || hasSplitId(node.second, splitId);
}

function makePaneId(root: LayoutNode): PaneId {
  const existing = new Set(collectPaneIds(root));
  let candidate: PaneId;
  do {
    candidate = `pane-${nextPaneNumber++}`;
  } while (existing.has(candidate));
  return candidate;
}

function makeSplitId(root: LayoutNode): string {
  let candidate: string;
  do {
    candidate = `split-${nextSplitNumber++}`;
  } while (hasSplitId(root, candidate));
  return candidate;
}

export function SplitLayoutProvider({ children }: { children: ReactNode }) {
  const { state: sessionState, switchTab, reportError } = useSession();
  const [state, setState] = useState<SplitLayoutState>(() => createInitialSplitLayout());
  const stateRef = useRef(state);
  stateRef.current = state;
  const restoringWorkspaceRef = useRef(false);
  const [layoutLoadStatus, setLayoutLoadStatus] = useState<"loading" | "ready" | "error">("loading");
  /** Runtime child Session ID -> stable root/config Session ID. Keep old mappings until app exit. */
  const stableSessionIdsRef = useRef<Map<string, string>>(new Map());
  for (const tab of sessionState.tabs) {
    stableSessionIdsRef.current.set(tab.id, tab.parentId ?? tab.id);
  }

  // Workspace Layout 与 Session Library 都由 Rust 持久层读取。初始化完成前禁止
  // 自动保存默认单 Pane，避免异步启动时覆盖昨日布局。
  useEffect(() => {
    let cancelled = false;

    void invoke<string | null>("get_config", { key: "workspace.layout" })
      .then(rawLayout => {
        if (cancelled) return;
        const restored = parsePersistedWorkspaceLayout(rawLayout);
        if (restored) {
          restoringWorkspaceRef.current = true;
          stateRef.current = restored;
          setState(restored);
        }
        setLayoutLoadStatus("ready");
      })
      .catch(error => {
        if (cancelled) return;
        setLayoutLoadStatus("error");
        console.warn("SplitLayoutContext: 读取 Workspace Layout 失败，已禁止自动保存:", error);
        reportError(i18n.t("session.workspaceLoadFailed", { error: String(error) }));
      });

    return () => { cancelled = true; };
  }, [reportError]);

  const syncActiveSession = useCallback((sessionId: string | null) => {
    void switchTab(sessionId);
  }, [switchTab]);

  const persistWorkspaceNow = useCallback(() => {
    if (!canPersistWorkspaceLayout(layoutLoadStatus, sessionState.catalogStatus, restoringWorkspaceRef.current)) return;
    try {
      const serialized = serializeWorkspaceLayout(stateRef.current, stableSessionIdsRef.current);
      void invoke("set_config", { key: "workspace.layout", value: serialized })
        .catch(error => {
          console.warn("SplitLayoutContext: 保存 Workspace Layout 失败:", error);
        });
    } catch (error) {
      console.warn("SplitLayoutContext: 序列化 Workspace Layout 失败:", error);
    }
  }, [layoutLoadStatus, sessionState.catalogStatus]);

  // Split Tree / assignment / ratio 变化后自动保存；拖动 divider 时短防抖。
  // Rust ConfigStore 是持久化权威源，浏览器本地存储不再承载工程 Workspace。
  useEffect(() => {
    if (!canPersistWorkspaceLayout(layoutLoadStatus, sessionState.catalogStatus, restoringWorkspaceRef.current)) return;
    const timer = window.setTimeout(persistWorkspaceNow, 160);
    return () => window.clearTimeout(timer);
  }, [state, sessionState.tabs, layoutLoadStatus, sessionState.catalogStatus, persistWorkspaceNow]);

  // 同步 SessionContext 的 activeTabId 与 Split Layout，同时优先清理已删除的 assignment。
  // 恢复 Workspace 时先等磁盘会话配置进入 SessionContext，再让持久化的 selected Pane 成为 active context；
  // 这样 loadSavedSessions() 默认选中的第一张卡片不会覆盖昨日保存的 Pane assignment。
  useEffect(() => {
    if (layoutLoadStatus !== "ready" || sessionState.catalogStatus !== "ready") return;
    const current = stateRef.current;
    const valid = new Set(sessionState.tabs.map(tab => tab.id));

    if (restoringWorkspaceRef.current) {
      // SessionContext atomically publishes the catalog and ready state, including an empty library.

      const next = pruneAssignments(current, valid);
      restoringWorkspaceRef.current = false;
      if (next !== current) {
        stateRef.current = next;
        setState(next);
      }

      const restoredSessionId = next.assignments[next.selectedPaneId] ?? null;
      syncActiveSession(restoredSessionId && valid.has(restoredSessionId) ? restoredSessionId : null);
      return;
    }

    const selectedSessionId = current.assignments[current.selectedPaneId];

    // 子终端关闭/父容器断开并不等于“用户删除了这个会话”。
    // 先把已消失的运行时 child ID 回退到稳定 root/config ID，再做真正的无效分配清理。
    const disconnectedRootIds = new Set(
      sessionState.tabs
        .filter(tab => !tab.parentId && tab.state === "disconnected")
        .map(tab => tab.id),
    );
    let next = remapRemovedChildrenToDisconnectedRoots(
      current,
      valid,
      disconnectedRootIds,
      stableSessionIdsRef.current,
    );
    next = pruneAssignments(next, valid);

    const selectedAssignmentLost = Boolean(
      selectedSessionId && !next.assignments[current.selectedPaneId]
    );
    if (selectedAssignmentLost && countPanes(current.root) > 1) {
      if (next !== current) {
        stateRef.current = next;
        setState(next);
      }
      syncActiveSession(null);
      return;
    }

    const activeId = sessionState.activeTabId;
    if (activeId && valid.has(activeId)) {
      next = activateSessionInLayout(next, activeId);
    }

    if (next !== current) {
      stateRef.current = next;
      setState(next);
    }
  }, [
    sessionState.activeTabId,
    sessionState.tabs,
    syncActiveSession,
    layoutLoadStatus,
    sessionState.catalogStatus,
  ]);

  const selectPane = useCallback((paneId: PaneId) => {
    const current = stateRef.current;
    if (!collectPaneIds(current.root).includes(paneId)) return;
    const next = selectPaneInLayout(current, paneId);
    if (next === current) return;
    stateRef.current = next;
    setState(next);
    syncActiveSession(next.assignments[paneId] ?? null);
  }, [syncActiveSession]);

  const splitPane = useCallback((paneId: PaneId, edge: SplitEdge) => {
    const current = stateRef.current;
    if (countPanes(current.root) >= MAX_WORKSPACE_PANES) return;
    const next = splitPaneInLayout(
      current,
      paneId,
      edge,
      makePaneId(current.root),
      makeSplitId(current.root),
    );
    if (next === current) return;
    stateRef.current = next;
    setState(next);
    // 新 Pane 按产品规则为空且自动 selected；附属 Session UI 同步为空。
    syncActiveSession(null);
  }, [syncActiveSession]);

  const clearPane = useCallback((paneId: PaneId) => {
    const current = stateRef.current;
    const next = clearPaneInLayout(current, paneId);
    if (next === current) return;
    stateRef.current = next;
    setState(next);
    if (current.selectedPaneId === paneId) {
      syncActiveSession(null);
    }
  }, [syncActiveSession]);

  const closePane = useCallback((paneId: PaneId) => {
    const current = stateRef.current;
    const result = closePaneInLayout(current, paneId);
    if (!result) return;
    stateRef.current = result.state;
    setState(result.state);
    if (current.selectedPaneId === paneId) {
      syncActiveSession(result.selectedSessionId);
    }
  }, [syncActiveSession]);

  const resizeSplit = useCallback((splitId: string, ratio: number) => {
    setState(prev => {
      const next = setSplitRatioInLayout(prev, splitId, ratio);
      stateRef.current = next;
      return next;
    });
  }, []);

  const activateSession = useCallback((sessionId: string) => {
    const current = stateRef.current;
    const next = activateSessionInLayout(current, sessionId);
    stateRef.current = next;
    setState(next);
    // 保持 SendBar / RightSidebar / StatusBar 与 selected Pane 一致。
    syncActiveSession(sessionId);
  }, [syncActiveSession]);

  const paneRects = useMemo(() => computePaneRects(state.root), [state.root]);
  const dividers = useMemo(() => computeDividerGeometries(state.root), [state.root]);
  const blockedEdges = useMemo(() => computeBlockedEdges(state.root), [state.root]);
  const paneCount = useMemo(() => countPanes(state.root), [state.root]);
  const sessionToPane = useMemo(() => {
    const result: Record<string, PaneId> = {};
    for (const [paneId, sessionId] of Object.entries(state.assignments)) {
      if (sessionId) result[sessionId] = paneId;
    }
    return result;
  }, [state.assignments]);

  const value = useMemo<SplitLayoutContextValue>(() => ({
    state,
    paneRects,
    dividers,
    blockedEdges,
    paneCount,
    sessionToPane,
    selectPane,
    splitPane,
    clearPane,
    closePane,
    resizeSplit,
    activateSession,
  }), [
    state,
    paneRects,
    dividers,
    blockedEdges,
    paneCount,
    sessionToPane,
    selectPane,
    splitPane,
    clearPane,
    closePane,
    resizeSplit,
    activateSession,
  ]);

  return <SplitLayoutContext.Provider value={value}>{children}</SplitLayoutContext.Provider>;
}

export function useSplitLayout(): SplitLayoutContextValue {
  const value = useContext(SplitLayoutContext);
  if (!value) throw new Error("useSplitLayout must be used within SplitLayoutProvider");
  return value;
}

export function getSessionPane(layout: SplitLayoutState, sessionId: string): PaneId | null {
  return findPaneForSession(layout.assignments, sessionId);
}

export type { LayoutNode, PaneId, PaneRect, SplitEdge, SplitLayoutState };
