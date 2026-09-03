import type {
  LayoutNode,
  PaneId,
  SplitLayoutState,
} from "./split-layout";

export const WORKSPACE_LAYOUT_STORAGE_KEY = "tauterm-workspace-layout-v1";
export const WORKSPACE_LAYOUT_VERSION = 1;

interface PersistedWorkspaceLayoutV1 {
  version: 1;
  root: LayoutNode;
  assignments: Record<PaneId, string | null>;
  selectedPaneId: PaneId;
}

function collectWorkspacePaneIds(node: LayoutNode): PaneId[] {
  if (node.type === "pane") return [node.id];
  return [...collectWorkspacePaneIds(node.first), ...collectWorkspacePaneIds(node.second)];
}

function countWorkspacePanes(node: LayoutNode): number {
  if (node.type === "pane") return 1;
  return countWorkspacePanes(node.first) + countWorkspacePanes(node.second);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseLayoutNode(
  value: unknown,
  paneIds: Set<string>,
  splitIds: Set<string>,
): LayoutNode | null {
  if (!isRecord(value) || typeof value.type !== "string" || typeof value.id !== "string" || !value.id) {
    return null;
  }

  if (value.type === "pane") {
    if (paneIds.has(value.id)) return null;
    paneIds.add(value.id);
    return { type: "pane", id: value.id };
  }

  if (value.type !== "split") return null;
  if (splitIds.has(value.id)) return null;
  if (value.direction !== "horizontal" && value.direction !== "vertical") return null;
  if (typeof value.ratio !== "number" || !Number.isFinite(value.ratio)) return null;
  if (value.ratio < 0.05 || value.ratio > 0.95) return null;

  splitIds.add(value.id);
  const first = parseLayoutNode(value.first, paneIds, splitIds);
  const second = parseLayoutNode(value.second, paneIds, splitIds);
  if (!first || !second) return null;

  return {
    type: "split",
    id: value.id,
    direction: value.direction,
    ratio: value.ratio,
    first,
    second,
  };
}

/**
 * Parse the last local Workspace layout defensively.
 *
 * The persisted payload is UI-only state. Invalid/corrupt/future payloads are ignored rather
 * than allowed to break app startup.
 */
export function parsePersistedWorkspaceLayout(raw: string | null): SplitLayoutState | null {
  if (!raw) return null;

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }

  if (!isRecord(parsed) || parsed.version !== WORKSPACE_LAYOUT_VERSION) return null;

  const paneIds = new Set<string>();
  const splitIds = new Set<string>();
  const root = parseLayoutNode(parsed.root, paneIds, splitIds);
  if (!root || paneIds.size < 1 || paneIds.size > 4 || countWorkspacePanes(root) !== paneIds.size) return null;

  if (typeof parsed.selectedPaneId !== "string" || !paneIds.has(parsed.selectedPaneId)) return null;
  if (!isRecord(parsed.assignments)) return null;

  const assignments: Record<PaneId, string | null> = {};
  for (const paneId of collectWorkspacePaneIds(root)) {
    const assigned = parsed.assignments[paneId];
    if (assigned !== null && assigned !== undefined && typeof assigned !== "string") return null;
    assignments[paneId] = typeof assigned === "string" && assigned.length > 0 ? assigned : null;
  }

  return {
    root,
    assignments,
    selectedPaneId: parsed.selectedPaneId,
  };
}

/**
 * Persist only stable Session configuration identities.
 *
 * Runtime child channels (SSH/Local Shell) are mapped to their parent configuration ID before
 * saving. If several panes currently show child channels from the same parent, only the first
 * pane keeps that stable reference; the remaining pane slots stay present but restore empty.
 */
export function serializeWorkspaceLayout(
  state: SplitLayoutState,
  stableSessionIds: ReadonlyMap<string, string>,
): string {
  const assignments: Record<PaneId, string | null> = {};
  const usedStableIds = new Set<string>();

  for (const paneId of collectWorkspacePaneIds(state.root)) {
    const runtimeId = state.assignments[paneId];
    if (!runtimeId) {
      assignments[paneId] = null;
      continue;
    }

    const stableId = stableSessionIds.get(runtimeId) ?? runtimeId;
    if (usedStableIds.has(stableId)) {
      assignments[paneId] = null;
      continue;
    }

    usedStableIds.add(stableId);
    assignments[paneId] = stableId;
  }

  const payload: PersistedWorkspaceLayoutV1 = {
    version: WORKSPACE_LAYOUT_VERSION,
    root: state.root,
    assignments,
    selectedPaneId: state.selectedPaneId,
  };

  return JSON.stringify(payload);
}

export function hasWorkspaceAssignments(state: SplitLayoutState): boolean {
  return Object.values(state.assignments).some(Boolean);
}
