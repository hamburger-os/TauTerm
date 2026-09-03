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

function makePaneId(): PaneId {
  return `pane-${nextPaneNumber++}`;
}

function makeSplitId(): string {
  return `split-${nextSplitNumber++}`;
}

export function SplitLayoutProvider({ children }: { children: ReactNode }) {
  const { state: sessionState, switchTab } = useSession();
  const [state, setState] = useState<SplitLayoutState>(() => createInitialSplitLayout());
  const stateRef = useRef(state);
  stateRef.current = state;

  const syncActiveSession = useCallback((sessionId: string | null) => {
    // 空字符串沿用现有 switchTab 的容错路径，前端等价于“当前无会话”：
    // App 的 activeTab 查找失败，因此 SendBar / RightSidebar / Sidebar active 状态都会隐藏。
    void switchTab(sessionId ?? "");
  }, [switchTab]);

  // Sidebar、快捷键或其他既有入口仍会更新 SessionContext.activeTabId。
  // 把这种“外部会话切换”解释为：已显示则跳到所在 Pane；未显示则放入当前 Pane。
  useEffect(() => {
    const activeId = sessionState.activeTabId;
    if (!activeId) return;
    if (!sessionState.tabs.some(tab => tab.id === activeId)) return;
    setState(prev => activateSessionInLayout(prev, activeId));
  }, [sessionState.activeTabId, sessionState.tabs]);

  // 会话被删除/子 channel 被关闭后，只清掉 Pane assignment，不改变 Split Tree。
  useEffect(() => {
    const valid = new Set(sessionState.tabs.map(tab => tab.id));
    setState(prev => pruneAssignments(prev, valid));
  }, [sessionState.tabs]);

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
    const next = splitPaneInLayout(current, paneId, edge, makePaneId(), makeSplitId(), 4);
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
