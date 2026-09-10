import assert from "node:assert/strict";
import {
  MAX_WORKSPACE_PANES,
  activateSessionInLayout,
  clearPaneInLayout,
  closePaneInLayout,
  collectPaneIds,
  computePaneRects,
  countPanes,
  createInitialSplitLayout,
  findPaneForSession,
  pruneAssignments,
  remapRemovedChildrenToDisconnectedRoots,
  resetPaneSplitRatioInLayout,
  setSplitRatioInLayout,
  splitPaneInLayout,
} from "../src/core/split-layout.ts";
import {
  parsePersistedWorkspaceLayout,
  serializeWorkspaceLayout,
} from "../src/core/workspace-layout.ts";

let state = createInitialSplitLayout("p1");
assert.equal(countPanes(state.root), 1);
assert.deepEqual(state.assignments, { p1: null });
assert.equal(state.selectedPaneId, "p1");

// Sidebar click in single-pane mode assigns the clicked Session to the selected Pane.
state = activateSessionInLayout(state, "ssh-a");
assert.equal(state.assignments.p1, "ssh-a");

// Edge split preserves the old Pane, creates an empty selected Pane, and never duplicates a Session.
state = splitPaneInLayout(state, "p1", "right", "p2", "s1");
assert.equal(countPanes(state.root), 2);
assert.equal(state.selectedPaneId, "p2");
assert.equal(state.assignments.p1, "ssh-a");
assert.equal(state.assignments.p2, null);

state = activateSessionInLayout(state, "serial-com3");
assert.equal(state.assignments.p2, "serial-com3");
assert.equal(findPaneForSession(state.assignments, "serial-com3"), "p2");

// Clearing a Pane only detaches its Session. Pane geometry and selection stay intact,
// and the same Session can be assigned again immediately.
const clearRoot = state.root;
state = clearPaneInLayout(state, "p2");
assert.strictEqual(state.root, clearRoot);
assert.equal(state.selectedPaneId, "p2");
assert.equal(state.assignments.p1, "ssh-a");
assert.equal(state.assignments.p2, null);
const alreadyEmpty = clearPaneInLayout(state, "p2");
assert.strictEqual(alreadyEmpty, state);
state = activateSessionInLayout(state, "serial-com3");
assert.equal(state.assignments.p2, "serial-com3");

// Clicking a Session that is already visible focuses its existing Pane instead of cloning it.
state = activateSessionInLayout(state, "ssh-a");
assert.equal(state.selectedPaneId, "p1");
assert.deepEqual(
  Object.values(state.assignments).filter(id => id === "ssh-a"),
  ["ssh-a"],
);

// Split a leaf again: this is a recursive Split Tree, not a fixed 2/3/4 template.
state = splitPaneInLayout(state, "p1", "bottom", "p3", "s2");
assert.equal(countPanes(state.root), 3);
assert.equal(state.selectedPaneId, "p3");
assert.deepEqual(new Set(collectPaneIds(state.root)), new Set(["p1", "p2", "p3"]));

const rects = computePaneRects(state.root);
assert.equal(rects.p2.left, 0.5);
assert.equal(rects.p2.width, 0.5);
assert.equal(rects.p1.height, 0.5);
assert.equal(rects.p3.top, 0.5);

// Explicit layout quality matrix: vertical 2-pane and balanced 2x2 geometry must remain valid.
let verticalTwo = createInitialSplitLayout("v1");
verticalTwo = splitPaneInLayout(verticalTwo, "v1", "bottom", "v2", "vs1");
const verticalRects = computePaneRects(verticalTwo.root);
assert.equal(countPanes(verticalTwo.root), 2);
assert.deepEqual(verticalRects.v1, { left: 0, top: 0, width: 1, height: 0.5 });
assert.deepEqual(verticalRects.v2, { left: 0, top: 0.5, width: 1, height: 0.5 });

let grid = createInitialSplitLayout("g1");
grid = splitPaneInLayout(grid, "g1", "right", "g2", "gs1");
grid = splitPaneInLayout(grid, "g1", "bottom", "g3", "gs2");
grid = splitPaneInLayout(grid, "g2", "bottom", "g4", "gs3");
const gridRects = computePaneRects(grid.root);
assert.equal(countPanes(grid.root), MAX_WORKSPACE_PANES);
assert.deepEqual(gridRects.g1, { left: 0, top: 0, width: 0.5, height: 0.5 });
assert.deepEqual(gridRects.g3, { left: 0, top: 0.5, width: 0.5, height: 0.5 });
assert.deepEqual(gridRects.g2, { left: 0.5, top: 0, width: 0.5, height: 0.5 });
assert.deepEqual(gridRects.g4, { left: 0.5, top: 0.5, width: 0.5, height: 0.5 });

// Resetting a Pane ratio affects only its immediate parent Split, not the outer tree.
let ratioTree = createInitialSplitLayout("r1");
ratioTree = splitPaneInLayout(ratioTree, "r1", "right", "r2", "rs1");
ratioTree = setSplitRatioInLayout(ratioTree, "rs1", 0.7);
ratioTree = splitPaneInLayout(ratioTree, "r1", "bottom", "r3", "rs2");
ratioTree = setSplitRatioInLayout(ratioTree, "rs2", 0.3);
ratioTree = resetPaneSplitRatioInLayout(ratioTree, "r1");
assert.equal(ratioTree.root.type, "split");
assert.equal(ratioTree.root.ratio, 0.7);
assert.equal(ratioTree.root.first.type, "split");
assert.equal(ratioTree.root.first.ratio, 0.5);
const alreadyBalanced = resetPaneSplitRatioInLayout(ratioTree, "r1");
assert.strictEqual(alreadyBalanced, ratioTree);

// Closing a Pane removes only the view slot and collapses its now-redundant parent split.
const closed = closePaneInLayout(state, "p3");
assert.ok(closed);
state = closed.state;
assert.equal(countPanes(state.root), 2);
assert.equal(state.selectedPaneId, "p1");
assert.equal(state.assignments.p1, "ssh-a");
assert.equal(state.assignments.p2, "serial-com3");
assert.equal("p3" in state.assignments, false);

// Divider ratios are bounded defensively.
state = setSplitRatioInLayout(state, "s1", 0.001);
assert.equal(state.root.type, "split");
assert.equal(state.root.ratio, 0.05);
state = setSplitRatioInLayout(state, "s1", 2);
assert.equal(state.root.type, "split");
assert.equal(state.root.ratio, 0.95);

// A removed Session clears its Pane assignment but does not mutate/collapse the layout.
const beforePrunePanes = collectPaneIds(state.root);
state = pruneAssignments(state, new Set(["ssh-a"]));
assert.equal(state.assignments.p2, null);
assert.deepEqual(collectPaneIds(state.root), beforePrunePanes);

// Hard cap: no more than the shared Workspace Pane limit.
state = splitPaneInLayout(state, "p2", "bottom", "p4", "s3", MAX_WORKSPACE_PANES);
state = splitPaneInLayout(state, "p4", "right", "p5", "s4", MAX_WORKSPACE_PANES);
assert.equal(countPanes(state.root), MAX_WORKSPACE_PANES);
const capped = splitPaneInLayout(state, "p5", "bottom", "p6", "s5", MAX_WORKSPACE_PANES);
assert.strictEqual(capped, state);
assert.equal(countPanes(capped.root), MAX_WORKSPACE_PANES);

// Workspace persistence preserves geometry/selection while converting runtime child channels
// to stable saved Session IDs. Duplicate child channels of the same parent restore only once.
let workspace = createInitialSplitLayout("wp1");
workspace = activateSessionInLayout(workspace, "ssh-child-0");
workspace = splitPaneInLayout(workspace, "wp1", "right", "wp2", "ws1");
workspace = activateSessionInLayout(workspace, "ssh-child-1");
workspace = splitPaneInLayout(workspace, "wp2", "bottom", "wp3", "ws2");
workspace = activateSessionInLayout(workspace, "serial-root");
workspace = setSplitRatioInLayout(workspace, "ws1", 0.63);

const stableSessionIds = new Map([
  ["ssh-child-0", "ssh-root"],
  ["ssh-child-1", "ssh-root"],
  ["serial-root", "serial-root"],
]);
const serializedWorkspace = serializeWorkspaceLayout(workspace, stableSessionIds);
const restoredWorkspace = parsePersistedWorkspaceLayout(serializedWorkspace);
assert.ok(restoredWorkspace);
assert.equal(countPanes(restoredWorkspace.root), 3);
assert.equal(restoredWorkspace.selectedPaneId, "wp3");
assert.equal(restoredWorkspace.assignments.wp1, "ssh-root");
assert.equal(restoredWorkspace.assignments.wp2, null);
assert.equal(restoredWorkspace.assignments.wp3, "serial-root");
assert.equal(restoredWorkspace.root.type, "split");
assert.equal(restoredWorkspace.root.ratio, 0.63);

// If the selected Pane belongs to a duplicate child group, it owns the durable parent reference.
const selectedDuplicateWorkspace = activateSessionInLayout(workspace, "ssh-child-1");
const restoredSelectedDuplicate = parsePersistedWorkspaceLayout(
  serializeWorkspaceLayout(selectedDuplicateWorkspace, stableSessionIds),
);
assert.ok(restoredSelectedDuplicate);
assert.equal(restoredSelectedDuplicate.selectedPaneId, "wp2");
assert.equal(restoredSelectedDuplicate.assignments.wp1, null);
assert.equal(restoredSelectedDuplicate.assignments.wp2, "ssh-root");
assert.equal(restoredSelectedDuplicate.assignments.wp3, "serial-root");

// A deliberately empty Workspace is still meaningful: geometry and selected Pane survive.
let emptyWorkspace = createInitialSplitLayout("ep1");
emptyWorkspace = splitPaneInLayout(emptyWorkspace, "ep1", "right", "ep2", "es1");
emptyWorkspace = setSplitRatioInLayout(emptyWorkspace, "es1", 0.41);
const restoredEmptyWorkspace = parsePersistedWorkspaceLayout(
  serializeWorkspaceLayout(emptyWorkspace, new Map()),
);
assert.ok(restoredEmptyWorkspace);
assert.equal(restoredEmptyWorkspace.selectedPaneId, "ep2");
assert.deepEqual(restoredEmptyWorkspace.assignments, { ep1: null, ep2: null });
assert.equal(restoredEmptyWorkspace.root.type, "split");
assert.equal(restoredEmptyWorkspace.root.ratio, 0.41);

// Corrupt/future payloads fail closed and fall back to normal single-pane startup.
assert.equal(parsePersistedWorkspaceLayout("not json"), null);
assert.equal(parsePersistedWorkspaceLayout(JSON.stringify({ version: 2 })), null);
assert.equal(parsePersistedWorkspaceLayout(JSON.stringify({
  version: 1,
  root: { type: "pane", id: "p1" },
  assignments: { p1: null },
  selectedPaneId: "missing",
})), null);

// A hand-edited/corrupt payload may not place one Session in two Panes.
assert.equal(parsePersistedWorkspaceLayout(JSON.stringify({
  version: 1,
  root: {
    type: "split",
    id: "dup-split",
    direction: "horizontal",
    ratio: 0.5,
    first: { type: "pane", id: "dup-p1" },
    second: { type: "pane", id: "dup-p2" },
  },
  assignments: { "dup-p1": "same-session", "dup-p2": "same-session" },
  selectedPaneId: "dup-p1",
})), null);

// Runtime child disappears because its parent container disconnected:
// preserve the selected Pane by remapping child -> stable disconnected root.
let disconnectedWorkspace = createInitialSplitLayout("dp1");
disconnectedWorkspace = activateSessionInLayout(disconnectedWorkspace, "shell-child-1");
disconnectedWorkspace = remapRemovedChildrenToDisconnectedRoots(
  disconnectedWorkspace,
  new Set(["shell-root"]),
  new Set(["shell-root"]),
  new Map([["shell-child-1", "shell-root"]]),
);
assert.equal(disconnectedWorkspace.assignments.dp1, "shell-root");

// Multiple children of the same disconnected root may not duplicate the root across Panes.
// The selected Pane wins the durable root assignment; the other stale child is later pruned.
let duplicateRuntime = createInitialSplitLayout("dr1");
duplicateRuntime = activateSessionInLayout(duplicateRuntime, "shell-child-a");
duplicateRuntime = splitPaneInLayout(duplicateRuntime, "dr1", "right", "dr2", "drs1");
duplicateRuntime = activateSessionInLayout(duplicateRuntime, "shell-child-b");
assert.equal(duplicateRuntime.selectedPaneId, "dr2");
duplicateRuntime = remapRemovedChildrenToDisconnectedRoots(
  duplicateRuntime,
  new Set(["shell-root"]),
  new Set(["shell-root"]),
  new Map([
    ["shell-child-a", "shell-root"],
    ["shell-child-b", "shell-root"],
  ]),
);
duplicateRuntime = pruneAssignments(duplicateRuntime, new Set(["shell-root"]));
assert.equal(duplicateRuntime.assignments.dr1, null);
assert.equal(duplicateRuntime.assignments.dr2, "shell-root");

// Closing only one child while the parent is still connected must not remap to the root.
let connectedParentWorkspace = createInitialSplitLayout("cp1");
connectedParentWorkspace = activateSessionInLayout(connectedParentWorkspace, "ssh-child-1");
connectedParentWorkspace = remapRemovedChildrenToDisconnectedRoots(
  connectedParentWorkspace,
  new Set(["ssh-root"]),
  new Set(),
  new Map([["ssh-child-1", "ssh-root"]]),
);
connectedParentWorkspace = pruneAssignments(connectedParentWorkspace, new Set(["ssh-root"]));
assert.equal(connectedParentWorkspace.assignments.cp1, null);

console.log("split-layout: runtime and persisted workspace invariants preserved");