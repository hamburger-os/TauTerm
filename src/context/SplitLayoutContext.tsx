import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSession } from "./SessionContext";
import {
  activateSessionInLayout,
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
  WORKSPACE_LAYOUT_STORAGE_KEY,
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

function loadInitialWorkspaceLayout(): SplitLayoutState | null {
  try {
    return parsePersistedWorkspaceLayout(localStorage.getItem(WORKSPACE_LAYOUT_STORAGE_KEY));
  } catch {
    return null;
  }
}

export function SplitLayoutProvider({ children }: { children: ReactNode }) {
  const { state: sessionState, switchTab } = useSession();
  const restoredLayoutRef = useRef<SplitLayoutState | null>(null);
  const [state, setState] = useState<SplitLayoutState>(() => {
    const restored = loadInitialWorkspaceLayout();
    restoredLayoutRef.current = restored;
    return restored ?? createInitialSplitLayout();
  });
  const stateRef = useRef(state);
  stateRef.current = state;
  const restoringWorkspaceRef = useRef(restoredLayoutRef.current !== null);
  const expectedSavedSessionIdsRef = useRef<Set<string>>(new Set());
  const [workspaceSessionCatalogReady, setWorkspaceSessionCatalogReady] = useState(
    restoredLayoutRef.current === null,
  );
  /** Runtime child Session ID -> stable root/config Session ID. Keep old mappings until app exit. */
  const stableSessionIdsRef = useRef<Map<string, string>>(new Map());
  for (const tab of sessionState.tabs) {
    stableSessionIdsRef.current.set(tab.id, tab.parentId ?? tab.id);
  }

  // SessionContext intentionally exposes live tabs rather than a startup-hydration flag. For a
  // restored Workspace we need one deterministic barrier so an all-empty layout is not mistaken
  // for "there are no assignments to restore" before loadSavedSessions() completes. The backend
  // command is read-only; this second read is used only to learn the expected stable Session IDs.
  useEffect(() => {
    if (!restoringWorkspaceRef.current) return;
    let cancelled = false;

    void invoke<Array<{ id: string }>>("load_sessions")
      .then(saved => {
        if (cancelled) return;
        expectedSavedSessionIdsRef.current = new Set((saved ?? []).map(session => session.id));
        setWorkspaceSessionCatalogReady(true);
      })
      .catch(() => {
        if (cancelled) return;
        // Missing/unreadable session storage is already treated as an empty catalog by
        // SessionContext. Mirror that startup fallback here rather than leaving restoration stuck.
        expectedSavedSessionIdsRef.current = new Set();
        setWorkspaceSessionCatalogReady(true);
      });

    return () => { cancelled = true; };
  }, []);

  const syncActiveSession = useCallback((sessionId: string | null) => {
    // 空字符串沿用现有 switchTab 的容错路径，前端等价于“当前无会话”：
    // App 的 activeTab 查找失败，因此 SendBar / RightSidebar / Sidebar active 状态都会隐藏。
    void switchTab(sessionId ?? "");
  }, [switchTab]);

  const persistWorkspaceNow = useCallback(() => {
    try {
      const serialized = serializeWorkspaceLayout(stateRef.current, stableSessionIdsRef.current);
      localStorage.setItem(WORKSPACE_LAYOUT_STORAGE_KEY, serialized);
    } catch (error) {
      console.warn("SplitLayoutContext: 保存 Workspace 布局失败:", error);
    }
  }, []);

  // Split Tree / assignment / ratio 变化后自动保存；拖动 divider 时用短防抖避免频繁写 localStorage。
  useEffect(() => {
    const timer = window.setTimeout(persistWorkspaceNow, 160);
    return () => window.clearTimeout(timer);
  }, [state, sessionState.tabs, persistWorkspaceNow]);

  // 若用户刚拖完 divider 就立即关闭窗口，确保最后状态仍同步落盘。
  useEffect(() => {
    window.addEventListener("beforeunload", persistWorkspaceNow);
    return () => window.removeEventListener("beforeunload", persistWorkspaceNow);
  }, [persistWorkspaceNow]);

  // 同步 SessionContext 的 activeTabId 与 Split Layout，同时优先清理已删除的 assignment。
  // 恢复 Workspace 时先等磁盘会话配置进入 SessionContext，再让持久化的 selected Pane 成为 active context；
  // 这样 loadSavedSessions() 默认选中的第一张卡片不会覆盖昨日保存的 Pane assignment。
  useEffect(() => {
    const current = stateRef.current;
    const valid = new Set(sessionState.tabs.map(tab => tab.id));

    if (restoringWorkspaceRef.current) {
      if (!workspaceSessionCatalogReady) return;

      // loadSavedSessions() 用一次 SET_TABS 写入完整磁盘目录。等目录中预期的稳定 ID 都出现后
      // 再结束恢复；空目录也能明确结束，因此纯空 Pane Workspace 不会被首会话意外填充。
      for (const sessionId of expectedSavedSessionIdsRef.current) {
        if (!valid.has(sessionId)) return;
      }

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
  }, [sessionState.activeTabId, sessionState.tabs, syncActiveSession, workspaceSessionCatalogReady]);

  const selectPane = useCallback((paneId: PaneId) => {
    const current = stateRef.current;
    if (!collectPaneIds(current.root).includes(paneId)) return;
    const next = selectPaneInLayout(current, paneId);
    stateRef.current = next;
    setState(next);
    syncActiveSession(next.assignments[paneId] ?? null);
  }, [syncActiveSession]);

  const splitPane = useCallback((paneId: PaneId, edge: SplitEdge) => {
    const current = stateRef.current;
    if (countPanes(current.root) >= 4) return;
    const next = splitPaneInLayout(
      current,
      paneId,
      edge,
      makePaneId(current.root),
      makeSplitId(current.root),
      4,
    );
    if (next === current) return;
    stateRef.current = next;
    setState(next);
    // 新 Pane 按产品规则为空且自动 selected；附属 Session UI 同步为空。
    syncActiveSession(null);
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
