## Why

The current multi-session UI has several ergonomic issues: the TabBar's "+" button duplicates the toolbar's new-session action, the ConnectDialog only exposes serial configuration directly (other modes are stubs), disconnecting a session from the terminal area removes it from the sidebar (losing session history), the sidebar artificially caps at 10 sessions, sessions lack right-click actions, and the toolbar is cluttered with low-value buttons (refresh, file transfer). This refactor consolidates session management around the sidebar as the single source of truth, makes the new-session flow extensible for future connection types, and streamlines the toolbar to essential actions only.

## What Changes

- **Remove** the TabBar component entirely — session switching moves exclusively to the left sidebar
- **Remove** the toolbar's "refresh" button and "transfer" (file transfer) button; add a "settings" button
- **Remove** the file-transfer panel toggle behavior — the bottom panel becomes a fixed-height info/status panel (always visible at its default height)
- **Redesign** the ConnectDialog into a two-step, mode-first flow: select connection type (Serial, SSH, Telnet, TFTP), then configure that mode's parameters. Only Serial is fully functional; other modes show "Coming Soon" with the mode's recognizable parameter layout
- **Change** session lifecycle: disconnecting (closing) a terminal session **no longer removes** the session from the sidebar. Disconnected sessions persist in the sidebar as historical entries that can be reconnected or deleted
- **Remove** the artificial 10-session cap from the sidebar (`/10` display and any hard limit)
- **Add** right-click context menu to sidebar session entries with actions: Connect / Reconnect, Configure (edit params), Rename, Delete
- Support **unlimited** historical sessions in the sidebar with a scrollable list and search/filter

## Capabilities

### New Capabilities

- `session-sidebar-persistence`: Sidebar retains disconnected sessions as historical entries; disconnecting a terminal does not remove the session from the list; no hard limit on session count
- `sidebar-context-menu`: Right-click context menu on sidebar session items with Connect/Reconnect, Configure, Rename, and Delete actions
- `new-session-dialog`: Full-featured new-session dialog with mode-first selection (Serial, SSH, Telnet, TFTP) followed by mode-specific configuration; extensible for adding new connection types
- `toolbar-simplification`: Toolbar reduced to four buttons: New Session, Sidebar Toggle, Command Palette, Settings
- `bottom-panel-static`: Bottom panel becomes a fixed-height info panel (always visible at default height), replacing the dismissible file-transfer panel

### Modified Capabilities

<!-- No existing specs to modify -->

## Impact

- **Removed components**: `TabBar.tsx`, `TabBar.module.css`, `FileTransferPanel.tsx`, `FileTransferPanel.module.css`
- **Modified components**: `App.tsx` (remove TabBar/transfer logic, restructure layout), `Toolbar.tsx` (reduce buttons, add settings), `ConnectDialog.tsx` (two-step mode-first redesign), `SessionSidebar.tsx` (add persistence, remove limit, add context menu)
- **Modified context**: `SessionContext.tsx` (change disconnect behavior to mark as disconnected instead of REMOVE_TAB, support session config editing, remove hard limits)
- **Modified state**: `TransferContext.tsx` — may be simplified or removed since the file transfer panel is being replaced with a static info panel
- **Breaking changes**: `session-disconnected` event no longer removes the tab from the list; `REMOVE_TAB` action semantics change; toolbar action IDs `refresh` and `transfer` are removed
