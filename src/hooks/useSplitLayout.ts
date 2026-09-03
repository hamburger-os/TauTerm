import { useCallback, useMemo, useRef, useState } from "react";
import {
  activateSessionInLayout,
  closePaneInLayout,
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
  type PaneId,
  type SplitEdge,
  type SplitLayoutState,
} from "../core/split-layout";

export interface UseSplitLayoutResult {
  state: SplitLayoutState;
  paneCount: number;
  paneRects: ReturnType<typeof computePaneRects>;
  dividers: ReturnType<typeof computeDividerGeometries>;
  blockedEdges: ReturnType<typeof computeBlockedEdges>;
  activateSession: (sessionId: string) => PaneId;
  selectPane: (paneId: PaneId) => string | null;
  splitPane: (paneId: PaneId, edge: SplitEdge) => PaneId | null;
  closePane: (paneId: PaneId) => { paneId: PaneId; sessionId: string | null } | null;
  setSplitRatio: (splitId: string, ratio: number) => void;
  pruneSessions: (validSessionIds: readonly string[]) => void;
  paneForSession: (sessionId: string) => PaneId | null;
}

export function useSplitLayout(): UseSplitLayoutResult {
  const [state, setState] = useState<SplitLayoutState>(() => createInitialSplitLayout());
  const stateRef = useRef(state);
  const idCounterRef = useRef(1);
  stateRef.current = state;

  const commit = useCallback((next: SplitLayoutState) => {
    if (next === stateRef.current) return next;
    stateRef.current = next;
    setState(next);
    return next;
  }, []);

  const activateSession = useCallback((sessionId: string): PaneId => {
    const next = activateSessionInLayout(stateRef.current, sessionId);
    commit(next);
    return findPaneForSession(next.assignments, sessionId) ?? next.selectedPaneId;
  }, [commit]);

  const selectPane = useCallback((paneId: PaneId): string | null => {
    const next = selectPaneInLayout(stateRef.current, paneId);
    commit(next);
    return next.assignments[next.selectedPaneId] ?? null;
  }, [commit]);

  const splitPane = useCallback((paneId: PaneId, edge: SplitEdge): PaneId | null => {
    if (countPanes(stateRef.current.root) >= 4) return null;
    idCounterRef.current += 1;
    const newPaneId = `pane-${idCounterRef.current}`;
    const newSplitId = `split-${idCounterRef.current}`;
    const next = splitPaneInLayout(
      stateRef.current,
      paneId,
      edge,
      newPaneId,
      newSplitId,
      4,
    );
    if (next === stateRef.current) return null;
    commit(next);
    return newPaneId;
  }, [commit]);

  const closePane = useCallback((paneId: PaneId) => {
    const result = closePaneInLayout(stateRef.current, paneId);
    if (!result) return null;
    commit(result.state);
    return {
      paneId: result.selectedPaneId,
      sessionId: result.selectedSessionId,
    };
  }, [commit]);

  const setSplitRatio = useCallback((splitId: string, ratio: number) => {
    commit(setSplitRatioInLayout(stateRef.current, splitId, ratio));
  }, [commit]);

  const pruneSessions = useCallback((validSessionIds: readonly string[]) => {
    const valid = new Set(validSessionIds);
    commit(pruneAssignments(stateRef.current, valid));
  }, [commit]);

  const paneForSession = useCallback((sessionId: string): PaneId | null => {
    return findPaneForSession(stateRef.current.assignments, sessionId);
  }, []);

  const paneRects = useMemo(() => computePaneRects(state.root), [state.root]);
  const dividers = useMemo(() => computeDividerGeometries(state.root), [state.root]);
  const blockedEdges = useMemo(() => computeBlockedEdges(state.root), [state.root]);
  const paneCount = useMemo(() => countPanes(state.root), [state.root]);

  return {
    state,
    paneCount,
    paneRects,
    dividers,
    blockedEdges,
    activateSession,
    selectPane,
    splitPane,
    closePane,
    setSplitRatio,
    pruneSessions,
    paneForSession,
  };
}
