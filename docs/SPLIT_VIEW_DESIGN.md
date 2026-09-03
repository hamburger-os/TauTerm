# Runtime Split View Design

## Scope

TauTerm Split View is a **runtime-only layout capability** for displaying several already-open Sessions at the same time.

It is intentionally **not** a persistent Workspace system in this release.

### Goals

- Preserve the existing left Session Sidebar as the primary session navigator.
- Allow the center content area to contain up to four panes.
- Let the user split any eligible pane from its free left/right/top/bottom edge.
- Let the user resize split ratios by dragging dividers.
- Keep exactly one selected pane.
- Define the Session in the selected pane as the current/active Session context.
- Keep SendBar, RightSidebar, Status/Actions, search and keyboard commands bound to that active Session.
- Never create a second main view instance for the same Session.
- Closing a pane removes only the view slot; it never disconnects/deletes the Session.
- Show a small layout mini-map on Sidebar Session cards that are currently visible in a pane.

### Non-goals

This release does not implement:

- persistent Workspace files;
- restoring split layout after app restart;
- automatic reconnect after app restart;
- saved layout templates;
- arbitrary unlimited recursive panes;
- multiple views of the exact same Session;
- cross-window workspace management.

On app restart TauTerm starts with the normal single-pane behavior.

## Product model

The user-facing mental model is:

> A Pane is a display slot. Clicking a Session puts it into the selected Pane. If that Session is already visible, clicking it selects its existing Pane instead.

The core rules are:

1. There is always at least one Pane and at most four.
2. Exactly one Pane is selected.
3. A Session can be assigned to at most one Pane.
4. Clicking a Pane selects it.
5. Clicking a Sidebar Session that is not visible assigns it to the selected Pane.
6. Clicking a Sidebar Session that is already visible selects its existing Pane; it is not duplicated.
7. The selected Pane's Session is the active Session context.
8. An empty selected Pane has no active Session context until a Session is assigned.
9. Closing a Pane does not disconnect, delete, or dispose its Session runtime.
10. Split View state lives only for the current app process.
11. A `multi_session` parent is a navigation proxy for its most recently active child Channel; it does not always force navigation to the first child.
12. Creating a new child Channel from a parent context menu preserves the Pane that was selected before the context-menu navigation and uses that Pane as the new Channel's intended placement.

## Layout model

Split View uses a bounded recursive split tree rather than fixed two/three/four-pane templates.

```ts
type LayoutNode =
  | { type: "pane"; id: PaneId }
  | {
      type: "split";
      id: SplitId;
      direction: "horizontal" | "vertical";
      ratio: number;
      first: LayoutNode;
      second: LayoutNode;
    };
```

Content assignment is separate from geometry:

```ts
interface SplitLayoutState {
  root: LayoutNode;
  assignments: Record<PaneId, SessionId | null>;
  selectedPaneId: PaneId;
}
```

This separation is deliberate:

- `LayoutNode` owns geometry.
- `assignments` owns Pane → Session placement.
- SessionContext owns connection/runtime state.

## Split interaction

A free outer edge of a Pane is a split hit-zone.

- Hovering an eligible edge shows a subtle edge affordance and half-pane preview.
- Clicking the edge splits that Pane in the requested direction.
- The new Pane is empty and automatically selected.
- When four Pane leaves already exist, split triggers and previews are hidden completely.
- If splitting an edge would create panes below the minimum usable size, that edge does not offer a split trigger.
- An edge that is already an internal divider is reserved for resize and does not offer another split trigger.

The visible accent line is smaller than its hit-zone, but the hit-zones must not steal pointer interaction from the Pane Header. In particular, left/right hit-zones begin below the header and the top-edge hit-zone is deliberately compact.

## Divider resize

Each split node owns one ratio.

Dragging a divider updates only that split node. Nested split ratios remain independent.

The UI uses normalized ratios, not persisted pixel sizes.

Terminal sizing remains the responsibility of each terminal's existing `ResizeObserver` + `FitAddon`; Split View only changes DOM geometry. Backend PTY resize notifications retain the existing debounce behavior.

## Pane chrome

Pane chrome is intentionally lightweight.

Single-pane mode should remain visually close to existing TauTerm.

In multi-pane mode:

- each Pane gets a lightweight 24px header outside the Session content area;
- an occupied Pane shows its Session name in that header, while an empty Pane uses a subdued empty-state label;
- the selected Pane receives a subtle accent at the top edge and a lightly emphasized header;
- the header never overlays terminal or custom-renderer content;
- right-clicking the Pane Header opens the Pane-level context menu;
- terminal content keeps its existing right-click menu;
- Pane close is exposed from Pane chrome rather than stealing terminal content right-click.

## Closing panes

`Close Pane` removes a leaf from the split tree and collapses its redundant parent split automatically.

Example:

```text
A | (B / C)
```

Closing `C` collapses `(B / C)` to `B`:

```text
A | B
```

The Session formerly displayed in `C` continues to exist in the Session Sidebar and retains its runtime/connection state.

The final remaining Pane cannot be closed.

## Sidebar mini-map

When more than one Pane exists, a Session currently assigned to a Pane shows a compact mini-map of the current split tree on its Session card.

The Session's Pane is highlighted in the mini-map. The normal Session active highlight continues to indicate the current Session context.

This communicates two different facts without creating two "selected" card styles:

- active card highlight: current operation context;
- mini-map: where the Session is visible in the center layout.

## Session context and auxiliary UI

TauTerm retains a single active Session context.

```text
selectedPaneId
    ↓
assignments[selectedPaneId]
    ↓
active Session
    ↓
SendBar / RightSidebar / Status / Search / shortcuts
```

Existing `SessionContext.activeTabId` remains the compatibility bridge for current UI code.

Sidebar clicks and other existing session-switch entry points update `activeTabId`; Split Layout observes that change and either assigns the Session to the selected Pane or selects the Pane where it is already visible.

Selecting an occupied Pane updates `activeTabId` to that Pane's Session. Selecting a newly-created empty Pane clears the effective active Session context until a Session is assigned. When no active Session exists, Session-scoped auxiliary UI such as the RightSidebar shell must not reserve empty layout space.

## Multi-session parent navigation and child lifecycle

Plugins with the `multi_session` capability, currently SSH and Local Shell, have a root/container Session plus runtime child Channels. Split View treats the child Channel as the displayable terminal Session while preserving the parent card as a useful navigation and command surface.

The navigation contract is:

- left-clicking or right-clicking a multi-session parent remains navigational;
- the parent resolves to the most recently active valid child Channel in the current process;
- any child activation path, including Sidebar selection, Pane selection, shortcuts, or newly-created Channels, updates that remembered child;
- if the remembered child no longer exists, parent navigation falls back to the first remaining child;
- if no child exists, navigation falls back to the parent Session itself;
- this recent-child memory is runtime UI state and is not persisted across app restart.

A parent context menu has one additional placement rule. Opening the context menu may navigate to the parent's recent child, but TauTerm snapshots the Pane that was selected before that navigation. When the user chooses **New Terminal** or **Run as administrator**, that original Pane is re-selected immediately before creating the child Channel. The new Channel can therefore fill an intentionally-created Empty Pane without changing the normal right-click navigation behavior.

Root and child lifecycle commands are distinct:

- closing/disconnecting a child terminal uses `close_channel(childId, parentId)`;
- disconnecting a root Session uses the root Session disconnect path (`disconnect_session` through SessionContext);
- terminal context-menu **Disconnect** and the global **Close Current Session** shortcut must apply this same child-vs-root distinction;
- closing a child clears any Pane assignment to that child but does not collapse the Split Tree or implicitly disconnect the parent Session;
- in a multi-pane layout, if the removed child occupied the selected Pane, that Pane stays selected and empty instead of being automatically replaced by a sibling; single-pane mode may retain the pre-split sibling fallback behavior.

## Terminal lifecycle invariant

The most important implementation invariant is:

> One Session owns at most one main terminal/xterm instance.

Split View must not mount one `TerminalView` per Pane.

TauTerm already keeps connected terminal instances alive across normal tab changes. Split View extends that instance-pool model by giving each visible terminal a Pane placement.

A terminal can therefore be:

- visible + selected;
- visible + not selected;
- hidden + still alive.

Removing a terminal Session from a Pane does not dispose its xterm instance. This preserves scrollback, ANSI/TUI state, search context, streaming buffers and the backend Session runtime.

## Non-terminal renderers

Non-terminal content is rendered inside its assigned Pane using the existing renderer/plugin contracts.

The same Session duplication rule applies: an exact Session is never assigned to two Panes simultaneously.

Long-lived plugin/runtime state should continue to live in Session/plugin stores rather than Pane state.

Pane surfaces allow contained scrolling for non-terminal/custom views so controls remain reachable when a Pane becomes short in three- or four-pane layouts. Plugins may still provide their own more specific internal scrolling behavior.

## Session deletion and child sessions

If a Session is deleted, or an SSH/Local Shell child channel is closed, any Pane assignment pointing to that runtime ID is cleared. In a multi-pane layout, a selected Pane whose Session was removed remains selected and empty; the Split Tree is not repopulated from SessionContext's single-pane fallback selection.

In single-pane mode, the existing Session navigation fallback may select a surviving sibling after the active child closes, preserving the pre-split workflow.

Split layout is not collapsed automatically in response to Session deletion. Only an explicit `Close Pane` action changes the number of panes.

Network peers remain internal state of their network container Session and do not become Pane assignments themselves.

## Testing and acceptance criteria

Pure Split Tree tests cover:

- initial single Pane;
- assigning a Session;
- recursive edge splitting;
- selected new empty Pane;
- preventing duplicate Session placement;
- jumping to an already-visible Session;
- close-and-collapse behavior;
- ratio clamping;
- Session deletion/pruning without layout collapse;
- hard maximum of four Pane leaves.

CI must additionally pass:

- TypeScript type check;
- Split Layout invariant test;
- frontend production build with no Rollup/Vite warnings;
- repository diff hygiene;
- existing cross-platform Rust Clippy/tests.

Manual acceptance scenarios should include:

1. Single Pane behaves like pre-split TauTerm.
2. Hover right edge → preview → click → right empty Pane selected.
3. Sidebar click assigns a Session to the empty Pane.
4. Split the left Pane from its bottom edge to create a three-Pane layout.
5. Drag both nested dividers and verify independent resize.
6. Click different visible panes and verify SendBar/RightSidebar follow the selected Session.
7. Click a Session already visible elsewhere and verify TauTerm selects that Pane without duplicating it.
8. Right-click a Pane Header → Close Pane; verify layout collapses but Session remains connected/listed.
9. Reach four Panes and verify no further split triggers or previews are offered.
10. Shrink a Pane below the usable split threshold and verify ineligible edges no longer advertise another split.
11. Select an empty Pane and verify the RightSidebar does not leave an empty shell or resize handle.
12. Put iperf/TFTP or another custom renderer in a short Pane and verify controls remain reachable by scrolling.
13. Close TauTerm and reopen; verify no split layout is restored in this release.
14. Activate a non-first SSH/Local Shell child, switch elsewhere, then click and right-click the parent; verify the recent child is restored rather than always selecting the first child.
15. Select an Empty Pane, right-click a connected SSH/Local Shell parent and choose New Terminal; verify the newly-created child is placed in the original Empty Pane.
16. In a multi-pane layout, disconnect the selected SSH/Local Shell child from terminal context menu or the close-current-session shortcut; verify only that child closes, its Pane remains selected and empty, the parent remains valid, and no root-session-not-found error is reported.
