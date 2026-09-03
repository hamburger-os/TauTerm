import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
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
  hasWorkspaceAssignments,
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
  /** Runtime child Session ID -> stable root/config Session ID. Keep old mappings until app exit. */
  const stableSessionIdsRef = useRef<Map<string, string>>(new Map());
  for (const tab of sessionState.tabs) {
    stableSessionIdsRef.current.set(tab.id, tab.parentId ?? tab.id);
  }

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
      // SessionContext 异步 load_sessions。在持久化布局确实引用了 Session 时，空 tabs 只是“尚未加载”，
      // 不能把 assignment 当成已删除会话立即清空。
      if (valid.size === 0 && hasWorkspaceAssignments(current)) return;

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
    const selectedSessionRemoved = Boolean(selectedSessionId && !valid.has(selectedSessionId));

    let next = pruneAssignments(current, valid);

    if (selectedSessionRemoved && countPanes(current.root) > 1) {
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
  }, [sessionState.activeTabId, sessionState.tabs, syncActiveSession]);

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
