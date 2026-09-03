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
- When four Pane leaves already exist, split triggers are hidden.
- An edge that is already an internal divider is reserved for resize and does not offer another split trigger.

The hit-zone is wider than the visible accent line so it remains easy to target without permanently adding buttons around the terminal.

## Divider resize

Each split node owns one ratio.

Dragging a divider updates only that split node. Nested split ratios remain independent.

The UI uses normalized ratios, not persisted pixel sizes.

Terminal sizing remains the responsibility of each terminal's existing `ResizeObserver` + `FitAddon`; Split View only changes DOM geometry. Backend PTY resize notifications retain the existing debounce behavior.

## Pane chrome

Pane chrome is intentionally lightweight.

Single-pane mode should remain visually close to existing TauTerm.

In multi-pane mode:

- each occupied Pane shows a small Session badge;
- the selected Pane receives a subtle accent at the top edge;
- the badge is above edge split hit-zones and can be right-clicked;
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

Selecting an occupied Pane updates `activeTabId` to that Pane's Session. Selecting a newly-created empty Pane clears the effective active Session context until a Session is assigned.

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

## Session deletion and child sessions

If a Session is deleted, or an SSH/Local Shell child channel is closed, any Pane assignment pointing to that runtime ID is cleared. The Pane remains in the layout as an empty Pane.

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
8. Right-click Pane badge → Close Pane; verify layout collapses but Session remains connected/listed.
9. Reach four Panes and verify no further split triggers are offered.
10. Close TauTerm and reopen; verify no split layout is restored in this release.
