# Split View & Workspace Persistence Design

## Scope

TauTerm Split View displays several Sessions at the same time and persists the **last local Workspace layout** across app restarts.

Persistence is deliberately limited to UI/workspace context: Pane geometry, Pane → saved Session references, and the selected Pane. It does **not** persist live sockets, PTYs, terminal process state, credentials, or automatically reconnect Sessions.

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
- Restore the last valid Pane tree, split ratios, Session placement and selected Pane on next startup.
- Keep restored Sessions disconnected until the user explicitly connects them.
- Let a disconnected Session shown in a Pane expose the same direct Connect / Configure / Delete workflow as its Sidebar card.

### Non-goals

This release does not implement:

- named/exportable Workspace files;
- automatic reconnect after app restart;
- saved layout templates;
- arbitrary unlimited recursive panes;
- multiple views of the exact same Session;
- cross-window workspace management.

On app restart TauTerm restores the most recent valid local layout. Saved Session configurations are loaded normally in the disconnected state; no transport connection is opened solely because the Session occupied a Pane yesterday.

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
10. The latest valid Split View state is persisted locally and restored on next app startup.
11. A `multi_session` parent is a navigation proxy for its most recently active child Channel; it does not always force navigation to the first child.
12. Creating a new child Channel from a parent context menu preserves the Pane that was selected before the context-menu navigation and uses that Pane as the new Channel's intended placement.
13. Runtime child Channels are never treated as durable connection objects; persistence resolves them to stable parent Session configuration IDs.

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

## Workspace persistence

The last Workspace layout is stored as a small, versioned local UI payload. The persistence boundary intentionally contains only:

- the recursive `LayoutNode` tree;
- normalized split ratios;
- Pane → stable saved Session ID assignments;
- `selectedPaneId`.

The payload does **not** contain Session parameters, passwords, credential material, terminal scrollback, process state, sockets, file handles or protocol runtime state. Session configuration and credential storage continue to use their existing dedicated stores.

Persistence follows these rules:

1. Split state is autosaved locally after layout/assignment/ratio changes with a short debounce.
2. Window shutdown performs a final synchronous save so the last divider movement is not lost.
3. The parser validates the version, bounded tree shape/depth, unique Pane/Split IDs, ratio bounds, selected Pane, one-Session-one-Pane assignment uniqueness and the four-Pane maximum. Invalid/corrupt/future payloads fall back safely to normal startup.
4. Session configuration loading is asynchronous. Workspace restoration waits for the saved-session catalog to resolve before treating an empty `SessionContext.tabs` as authoritative, including when every restored Pane is intentionally empty.
5. Once saved Sessions are available, missing/deleted Session IDs are pruned without collapsing the Pane tree.
6. A runtime SSH/Local Shell child Channel is persisted as its stable parent Session configuration ID. On next startup the Pane therefore shows the saved parent Session in the disconnected state.
7. If multiple visible child Channels belong to the same parent, only one durable parent assignment can be restored without violating the one-Session-one-Pane invariant. If the selected Pane belongs to that duplicate group it keeps the durable parent reference; otherwise the first Pane in Split Tree order keeps it. Additional Pane slots remain present but restore empty.
8. Restoring a Workspace never invokes `connect_session`, `open_channel`, or any equivalent automatic connection path.

The intended startup experience is therefore:

```text
Yesterday                           Next startup
---------                           ------------
Pane A -> SSH child channel   ->    Pane A -> saved SSH Session (disconnected)
Pane B -> Serial Session      ->    Pane B -> saved Serial Session (disconnected)
Pane C -> Local Shell child   ->    Pane C -> saved Local Shell Session (disconnected)
```

The geometry and engineering context survive; transport/runtime state does not.

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

The UI and persistence format use normalized ratios, not pixel sizes.

Terminal sizing remains the responsibility of each terminal's existing `ResizeObserver` + `FitAddon`; Split View only changes DOM geometry. Backend PTY resize notifications retain the existing debounce behavior.

## Pane chrome and disconnected Session actions

This section defines interaction semantics only. Visual/material rules for the Workspace surface, Pane Header, dividers, selection treatment and radius ownership are defined exclusively in [`.agents/skills/tauterm-theme/SKILL.md`](../.agents/skills/tauterm-theme/SKILL.md).

In multi-pane mode:

- each Pane gets a 24px header outside the Session content area;
- an occupied Pane shows its Session name in that header, while an empty Pane uses a subdued empty-state label;
- the header never overlays terminal or custom-renderer content;
- left-clicking a Pane Header selects that Pane;
- right-clicking the Pane Header opens the Pane-level context menu (`Close Pane`);
- a secondary-button press on a non-selected Pane Header does **not** activate the Pane first, so SendBar/RightSidebar changes cannot move the target before the context menu opens;
- the Pane-level `Close Pane` menu is owned **only** by the Pane Header; right-clicking Pane content never opens it;
- connected terminal content keeps its existing terminal right-click menu;
- right-clicking a disconnected terminal placeholder opens Session actions instead of the WebView/browser default context menu;
- the disconnected Session menu provides the same direct workflow as the Sidebar card: Connect, Configure, Delete, plus Run as administrator where the plugin supports elevation.

Right-clicking a disconnected Pane also selects that Pane first, so a subsequent Connect action continues in the same visible context.

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

SendBar presence follows Session capability/configuration (`sendBarEnabled`), not connection lifecycle. In particular, a disconnected Network Debug Session keeps its SendBar workspace visible when selected; connection state gates the actual send operation instead of creating/removing the bar and reshaping the center layout.

During Workspace restoration, the persisted selected Pane wins over the temporary first-tab selection produced while saved Session configurations are loading. This also applies to a deliberately all-empty Workspace: startup hydration must not populate its selected Pane merely because saved Session configurations exist.

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

Workspace persistence does not attempt to serialize any of this runtime terminal state.

## Non-terminal renderers

Non-terminal content is rendered inside its assigned Pane using the existing renderer/plugin contracts.

The same Session duplication rule applies: an exact Session is never assigned to two Panes simultaneously.

Long-lived plugin/runtime state should continue to live in Session/plugin stores rather than Pane state.

Pane surfaces allow contained scrolling for non-terminal/custom views so controls remain reachable when a Pane becomes short in three- or four-pane layouts. Plugins may still provide their own more specific internal scrolling behavior.

## Session deletion and child sessions

If a Session is deleted, or an SSH/Local Shell child channel is closed, any Pane assignment pointing to that runtime ID is cleared. In a multi-pane layout, a selected Pane whose Session was removed remains selected and empty; the Split Tree is not repopulated from SessionContext's single-pane fallback selection.

In single-pane mode, the existing Session navigation fallback may select a surviving sibling after the active child closes, preserving the pre-split workflow.

Split layout is not collapsed automatically in response to Session deletion. Only an explicit `Close Pane` action changes the number of panes. The next persistence write records the now-empty assignment.

Network peers remain internal state of their network container Session and do not become Pane assignments themselves.

## Testing and acceptance criteria

Pure Split Tree / Workspace tests cover:

- initial single Pane;
- assigning a Session;
- recursive edge splitting;
- selected new empty Pane;
- preventing duplicate Session placement;
- jumping to an already-visible Session;
- close-and-collapse behavior;
- ratio clamping;
- Session deletion/pruning without layout collapse;
- hard maximum of four Pane leaves;
- Workspace serialization/parsing;
- deliberately all-empty Workspace geometry/selection persistence;
- child Channel → stable parent Session canonicalization;
- duplicate child Channels from one parent restoring without duplicate Session placement;
- selected-Pane preference when duplicate child Channels canonicalize to one parent;
- corrupt/future Workspace payload rejection;
- duplicate persisted Session assignments rejection.

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
13. Arrange multiple saved Sessions in Panes, resize dividers, select a Pane, close TauTerm and reopen; verify the same Pane tree, ratios, stable Session assignments and selected Pane are restored while Sessions remain disconnected.
14. Activate a non-first SSH/Local Shell child, switch elsewhere, then click and right-click the parent; verify the recent child is restored rather than always selecting the first child.
15. Select an Empty Pane, right-click a connected SSH/Local Shell parent and choose New Terminal; verify the newly-created child is placed in the original Empty Pane.
16. In a multi-pane layout, disconnect the selected SSH/Local Shell child from terminal context menu or the close-current-session shortcut; verify only that child closes, its Pane remains selected and empty, the parent remains valid, and no root-session-not-found error is reported.
17. Put a saved but disconnected terminal Session in a Pane, right-click the disconnected placeholder, verify the WebView default menu never appears, then choose Connect and verify the Session connects in that Pane.
18. From the same disconnected Pane menu, verify Configure opens the existing Session editor and Delete follows the same confirmation/removal behavior as the Sidebar card.
19. Save a multi-Pane Workspace with every Pane intentionally empty, restart, and verify the Pane tree, ratios and selected Pane remain empty instead of being populated by the first saved Session.
20. Display two child Channels from the same SSH/Local Shell parent, select the second child's Pane, restart, and verify that selected Pane receives the single restored parent Session reference while the duplicate Pane restores empty.
21. Right-click connected Network Debug/custom content and an empty Pane body; verify the Pane-level `Close Pane` menu never appears there.
22. Right-click a non-selected Pane Header whose Session would change SendBar/RightSidebar layout if activated; verify `Close Pane` opens at the original pointer position and the active Session does not change before an explicit left-click.
23. Select a disconnected Network Debug Session from either the Sidebar card or its Pane; verify its SendBar is already visible and connecting/disconnecting does not create/remove the SendBar shell.
