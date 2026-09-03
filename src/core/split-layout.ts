export type PaneId = string;
export type SplitId = string;
export type SplitDirection = "horizontal" | "vertical";
export type SplitEdge = "left" | "right" | "top" | "bottom";

export type LayoutNode =
  | { type: "pane"; id: PaneId }
  | {
      type: "split";
      id: SplitId;
      direction: SplitDirection;
      ratio: number;
      first: LayoutNode;
      second: LayoutNode;
    };

export interface SplitLayoutState {
  root: LayoutNode;
  assignments: Record<PaneId, string | null>;
  selectedPaneId: PaneId;
}

export interface PaneRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface DividerGeometry {
  splitId: SplitId;
  direction: SplitDirection;
  /** 该 split 节点在整个内容区内的归一化矩形。 */
  rect: PaneRect;
  /** 分割线在 rect 内的位置，0..1。 */
  ratio: number;
}

export interface ClosePaneResult {
  state: SplitLayoutState;
  selectedPaneId: PaneId;
  selectedSessionId: string | null;
}

const FULL_RECT: PaneRect = { left: 0, top: 0, width: 1, height: 1 };

export function createInitialSplitLayout(paneId = "pane-1"): SplitLayoutState {
  return {
    root: { type: "pane", id: paneId },
    assignments: { [paneId]: null },
    selectedPaneId: paneId,
  };
}

export function collectPaneIds(node: LayoutNode): PaneId[] {
  if (node.type === "pane") return [node.id];
  return [...collectPaneIds(node.first), ...collectPaneIds(node.second)];
}

export function countPanes(node: LayoutNode): number {
  if (node.type === "pane") return 1;
  return countPanes(node.first) + countPanes(node.second);
}

export function findPaneForSession(
  assignments: Record<PaneId, string | null>,
  sessionId: string,
): PaneId | null {
  for (const [paneId, assigned] of Object.entries(assignments)) {
    if (assigned === sessionId) return paneId;
  }
  return null;
}

export function activateSessionInLayout(
  state: SplitLayoutState,
  sessionId: string,
): SplitLayoutState {
  const existingPane = findPaneForSession(state.assignments, sessionId);
  if (existingPane) {
    if (existingPane === state.selectedPaneId) return state;
    return { ...state, selectedPaneId: existingPane };
  }

  if (state.assignments[state.selectedPaneId] === sessionId) return state;
  return {
    ...state,
    assignments: {
      ...state.assignments,
      [state.selectedPaneId]: sessionId,
    },
  };
}

export function selectPaneInLayout(
  state: SplitLayoutState,
  paneId: PaneId,
): SplitLayoutState {
  if (paneId === state.selectedPaneId) return state;
  if (!collectPaneIds(state.root).includes(paneId)) return state;
  return { ...state, selectedPaneId: paneId };
}

function replacePane(
  node: LayoutNode,
  paneId: PaneId,
  replacement: LayoutNode,
): LayoutNode {
  if (node.type === "pane") return node.id === paneId ? replacement : node;
  return {
    ...node,
    first: replacePane(node.first, paneId, replacement),
    second: replacePane(node.second, paneId, replacement),
  };
}

export function splitPaneInLayout(
  state: SplitLayoutState,
  paneId: PaneId,
  edge: SplitEdge,
  newPaneId: PaneId,
  newSplitId: SplitId,
  maxPanes = 4,
): SplitLayoutState {
  if (countPanes(state.root) >= maxPanes) return state;
  if (!collectPaneIds(state.root).includes(paneId)) return state;

  const oldPane: LayoutNode = { type: "pane", id: paneId };
  const newPane: LayoutNode = { type: "pane", id: newPaneId };
  const direction: SplitDirection = edge === "left" || edge === "right"
    ? "horizontal"
    : "vertical";
  const newFirst = edge === "left" || edge === "top" ? newPane : oldPane;
  const newSecond = edge === "left" || edge === "top" ? oldPane : newPane;

  const replacement: LayoutNode = {
    type: "split",
    id: newSplitId,
    direction,
    ratio: 0.5,
    first: newFirst,
    second: newSecond,
  };

  return {
    root: replacePane(state.root, paneId, replacement),
    assignments: {
      ...state.assignments,
      [newPaneId]: null,
    },
    selectedPaneId: newPaneId,
  };
}

interface RemovePaneNodeResult {
  node: LayoutNode | null;
  removed: boolean;
  fallbackPaneId: PaneId | null;
}

function firstPaneId(node: LayoutNode): PaneId {
  return node.type === "pane" ? node.id : firstPaneId(node.first);
}

function removePaneNode(node: LayoutNode, paneId: PaneId): RemovePaneNodeResult {
  if (node.type === "pane") {
    if (node.id === paneId) return { node: null, removed: true, fallbackPaneId: null };
    return { node, removed: false, fallbackPaneId: null };
  }

  const firstResult = removePaneNode(node.first, paneId);
  if (firstResult.removed) {
    if (!firstResult.node) {
      return {
        node: node.second,
        removed: true,
        fallbackPaneId: firstPaneId(node.second),
      };
    }
    return {
      node: { ...node, first: firstResult.node },
      removed: true,
      fallbackPaneId: firstResult.fallbackPaneId,
    };
  }

  const secondResult = removePaneNode(node.second, paneId);
  if (secondResult.removed) {
    if (!secondResult.node) {
      return {
        node: node.first,
        removed: true,
        fallbackPaneId: firstPaneId(node.first),
      };
    }
    return {
      node: { ...node, second: secondResult.node },
      removed: true,
      fallbackPaneId: secondResult.fallbackPaneId,
    };
  }

  return { node, removed: false, fallbackPaneId: null };
}

export function closePaneInLayout(
  state: SplitLayoutState,
  paneId: PaneId,
): ClosePaneResult | null {
  if (countPanes(state.root) <= 1) return null;

  const removed = removePaneNode(state.root, paneId);
  if (!removed.removed || !removed.node) return null;

  const assignments = { ...state.assignments };
  delete assignments[paneId];

  const selectedPaneId = state.selectedPaneId === paneId
    ? (removed.fallbackPaneId ?? firstPaneId(removed.node))
    : state.selectedPaneId;

  const next: SplitLayoutState = {
    root: removed.node,
    assignments,
    selectedPaneId,
  };

  return {
    state: next,
    selectedPaneId,
    selectedSessionId: assignments[selectedPaneId] ?? null,
  };
}

function updateSplitRatio(node: LayoutNode, splitId: SplitId, ratio: number): LayoutNode {
  if (node.type === "pane") return node;
  if (node.id === splitId) return { ...node, ratio };
  return {
    ...node,
    first: updateSplitRatio(node.first, splitId, ratio),
    second: updateSplitRatio(node.second, splitId, ratio),
  };
}

export function setSplitRatioInLayout(
  state: SplitLayoutState,
  splitId: SplitId,
  ratio: number,
): SplitLayoutState {
  const clamped = Math.max(0.05, Math.min(0.95, ratio));
  return { ...state, root: updateSplitRatio(state.root, splitId, clamped) };
}

export function pruneAssignments(
  state: SplitLayoutState,
  validSessionIds: ReadonlySet<string>,
): SplitLayoutState {
  let changed = false;
  const assignments: Record<PaneId, string | null> = { ...state.assignments };
  for (const paneId of collectPaneIds(state.root)) {
    const sessionId = assignments[paneId];
    if (sessionId && !validSessionIds.has(sessionId)) {
      assignments[paneId] = null;
      changed = true;
    }
  }
  return changed ? { ...state, assignments } : state;
}

export function computePaneRects(
  root: LayoutNode,
): Record<PaneId, PaneRect> {
  const result: Record<PaneId, PaneRect> = {};

  const walk = (node: LayoutNode, rect: PaneRect) => {
    if (node.type === "pane") {
      result[node.id] = rect;
      return;
    }

    if (node.direction === "horizontal") {
      const firstWidth = rect.width * node.ratio;
      walk(node.first, {
        left: rect.left,
        top: rect.top,
        width: firstWidth,
        height: rect.height,
      });
      walk(node.second, {
        left: rect.left + firstWidth,
        top: rect.top,
        width: rect.width - firstWidth,
        height: rect.height,
      });
    } else {
      const firstHeight = rect.height * node.ratio;
      walk(node.first, {
        left: rect.left,
        top: rect.top,
        width: rect.width,
        height: firstHeight,
      });
      walk(node.second, {
        left: rect.left,
        top: rect.top + firstHeight,
        width: rect.width,
        height: rect.height - firstHeight,
      });
    }
  };

  walk(root, FULL_RECT);
  return result;
}

export function computeDividerGeometries(root: LayoutNode): DividerGeometry[] {
  const result: DividerGeometry[] = [];

  const walk = (node: LayoutNode, rect: PaneRect) => {
    if (node.type === "pane") return;
    result.push({ splitId: node.id, direction: node.direction, rect, ratio: node.ratio });

    if (node.direction === "horizontal") {
      const firstWidth = rect.width * node.ratio;
      walk(node.first, {
        left: rect.left,
        top: rect.top,
        width: firstWidth,
        height: rect.height,
      });
      walk(node.second, {
        left: rect.left + firstWidth,
        top: rect.top,
        width: rect.width - firstWidth,
        height: rect.height,
      });
    } else {
      const firstHeight = rect.height * node.ratio;
      walk(node.first, {
        left: rect.left,
        top: rect.top,
        width: rect.width,
        height: firstHeight,
      });
      walk(node.second, {
        left: rect.left,
        top: rect.top + firstHeight,
        width: rect.width,
        height: rect.height - firstHeight,
      });
    }
  };

  walk(root, FULL_RECT);
  return result;
}

export function computeBlockedEdges(root: LayoutNode): Record<PaneId, Set<SplitEdge>> {
  const result: Record<PaneId, Set<SplitEdge>> = {};
  for (const paneId of collectPaneIds(root)) result[paneId] = new Set<SplitEdge>();

  const mark = (node: LayoutNode, edge: SplitEdge) => {
    for (const paneId of collectPaneIds(node)) result[paneId].add(edge);
  };

  const walk = (node: LayoutNode) => {
    if (node.type === "pane") return;
    if (node.direction === "horizontal") {
      mark(node.first, "right");
      mark(node.second, "left");
    } else {
      mark(node.first, "bottom");
      mark(node.second, "top");
    }
    walk(node.first);
    walk(node.second);
  };

  walk(root);
  return result;
}
