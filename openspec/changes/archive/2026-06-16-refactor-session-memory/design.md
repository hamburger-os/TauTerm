## Context

TauTerm v0.2 is a Tauri-based multi-session terminal emulator with a React/TypeScript frontend. The current architecture uses:

- **AppShell** → wraps the app with SessionProvider, ThemeProvider, TransferProvider
- **App.tsx** → root layout with Toolbar, SessionSidebar (animated), TabBar, TerminalView, FileTransferPanel, StatusBar, and overlay components (CommandPalette, ConnectDialog, Toast)
- **SessionContext** → reducer-based state management for tabs (sessions), active tab, endpoints, connection types; communicates with the Rust backend via Tauri `invoke` and event listeners
- **TabInfo** → `{ id, name, connection_type, endpoint, state, params }` where state is `"disconnected" | "connecting" | "connected"`
- **Session events**: `session-connected` dispatches ADD_TAB; `session-disconnected` dispatches REMOVE_TAB
- **Persist**: Sessions are loaded from the backend via `load_sessions`, restoring them as `"disconnected"` tabs

The frontend uses CSS modules with a glassmorphism design system (CSS custom properties), framer-motion for animations, and react-i18next for localization.

**Key constraints**: This is a Tauri v2 desktop app. The backend (Rust/src-tauri) handles serial port enumeration, session lifecycle, and data routing. The frontend communicates exclusively via `invoke` and Tauri events.

## Goals / Non-Goals

**Goals:**
- Remove the TabBar entirely — sidebar is the sole session switcher
- Redesign ConnectDialog to a two-step (mode → config) flow extensible for SSH, Telnet, TFTP
- Make the sidebar session-persistent: disconnecting keeps the session entry; user explicitly deletes from the sidebar
- Remove all session count limits (hard cap and UI display)
- Add right-click context menu to sidebar sessions
- Simplify toolbar to: New Session, Sidebar Toggle, Command Palette, Settings
- Replace the toggleable file-transfer panel with a static, always-visible bottom info/status panel
- Zero regressions on existing serial terminal functionality

**Non-Goals:**
- Implementing SSH/Telnet/TFTP connection types (only UI scaffolding)
- Redesigning the terminal rendering engine (xterm.js integration unchanged)
- Adding persistence to disk for sidebar layout preferences
- Changing the backend session management logic
- Adding drag-and-drop reordering to sidebar (TabBar reorder is already removed via TabBar removal)

## Decisions

### 1. Remove TabBar entirely instead of hiding its "+" button

**Decision**: Delete the `TabBar` component and all references to it.

**Rationale**: The user's requirement is to rely solely on the sidebar for session switching. Keeping TabBar without the "+" button would still show a row of tabs that competes with the sidebar's session list. Removing it entirely:
- Eliminates confusion between two session-switching mechanisms
- Frees vertical space in the terminal viewport
- Simplifies `App.tsx` layout (no TabBar import or rendering)

**Alternatives considered**:
- Hide "+" but keep tab list: Confusing UX — two places to see and switch sessions
- Make TabBar invisible and keep keyboard-only switching: Unnecessary complexity when sidebar handles it

### 2. Session lifecycle: disconnect → keep in sidebar (mark "disconnected")

**Decision**: Change the `session-disconnected` event handler to dispatch `SET_TAB_STATE` (state → "disconnected") instead of `REMOVE_TAB`. The existing `REMOVE_TAB` action is repurposed for explicit deletion from the sidebar context menu only.

**Rationale**: Sessions represent valuable connection history. Users often disconnect temporarily and want to reconnect to the same endpoint without re-entering all parameters. The existing code already has this concept — `load_sessions` restores `"disconnected"` tabs. This brings the runtime behavior in line with the persistence model.

**Data flow**:
```
User clicks × on terminal area (TabBar removed, so this path is gone)
User clicks "Disconnect" in context menu → dispatch explicit removal
Backend emits session-disconnected → dispatch SET_TAB_STATE(id, "disconnected")
```

Wait — the user wants the × removed entirely (TabBar gone), so there's no disconnect trigger from the terminal area. The only way to remove a session is via the sidebar context menu "Delete" action.

**Revised data flow**:
```
Backend session disconnected (port unplugged, etc.) → SET_TAB_STATE(id, "disconnected")
User right-clicks sidebar → "Delete" → explicit REMOVE_TAB
User right-clicks sidebar → "Reconnect" → connect with saved params
```

### 3. ConnectDialog: two-step mode-first design

**Decision**: Redesign ConnectDialog with two panels:
- **Step 1 — Mode Selection**: Grid of mode cards (Serial, SSH, Telnet, TFTP) with icons and descriptions. Only Serial is functional; others show "Coming Soon" badge but are still selectable (to preview their config layout).
- **Step 2 — Configuration**: Mode-specific parameter form. For Serial: port, baud rate, data bits, parity, stop bits, flow control, session name (same as current). For others: placeholder fields showing the parameters they would have.

**Rationale**: The current dialog shows connection type in a dropdown alongside serial config — it doesn't feel like a full-featured new-session flow. A two-step design:
- Makes it obvious this is a multi-protocol terminal
- Gives each mode room to show its unique configuration
- Makes adding new modes straightforward (add a card + config panel)
- Matches the UX pattern used by PuTTY, MobaXterm, and other terminal emulators

**Component structure**:
```
ConnectDialog
├── ModeSelector (grid of ModeCards)
│   ├── SerialCard (active)
│   ├── SshCard (coming soon)
│   ├── TelnetCard (coming soon)
│   └── TftpCard (coming soon)
└── ConfigPanel (dynamic based on selected mode)
    ├── SerialConfig (existing fields, extracted)
    ├── SshConfig (placeholder)
    ├── TelnetConfig (placeholder)
    └── TftpConfig (placeholder)
```

### 4. Sidebar context menu

**Decision**: Implement a custom `<ContextMenu>` component using a portal that appears at the click position. Actions depend on session state:
- **Disconnected**: Connect (reconnect), Configure (edit params), Rename, Delete
- **Connected**: Disconnect, Rename, Delete
- **Connecting**: (no actions, or Cancel)

**Rationale**: Native OS context menus are not easily customizable in a webview. A custom component:
- Matches the app's glassmorphism design
- Is fully controllable from React state
- Can show/hide actions based on session state

**Implementation**: Use a `useContextMenu` hook that manages position, visibility, and target session. Render via `ReactDOM.createPortal` to avoid overflow clipping issues in the sidebar container.

### 5. No session limit

**Decision**: Remove the `{state.tabs.length}/10` display and any logic that enforces a max. The sidebar list is naturally bounded by:
- Scroll (the list container has `overflow-y: auto`)
- Search/filter (existing functionality)
- User's own cleanup via context menu "Delete"

**Rationale**: An artificial cap of 10 is arbitrary for a desktop app with no resource constraints (sessions are lightweight objects, not active connections). Removing the cap is a one-line change in the sidebar display.

### 6. Toolbar simplification

**Decision**: Change `TOOLBAR_BUTTONS` to:
```ts
{ id: "newSession", icon: "➕", labelKey: "toolbar.newSession" },
{ id: "sidebar", icon: "☰", labelKey: "toolbar.sidebar" },
{ id: "commands", icon: "⌘", labelKey: "toolbar.commands" },
{ id: "settings", icon: "⚙", labelKey: "toolbar.settings" },
```

Remove the `handleToolbarAction` cases for `refresh` and `transfer`. Add a `settings` handler that opens a Settings dialog (initially a placeholder/toast saying "Settings coming soon" — the settings dialog itself is out of scope for this change).

**Rationale**: The refresh button (port enumeration) is already available inside the ConnectDialog as the refresh icon. The file transfer toggle is being removed entirely by requirement 7.

### 7. Replace file-transfer panel with static bottom panel

**Decision**: Remove `FileTransferPanel`, the `ResizeHandle` above it, and all transfer-related logic from `App.tsx`. Replace with a static `BottomInfoPanel` component at a fixed height (the previous `PANEL_DEFAULT` of 200px) that shows session info/status. The panel is always visible, not toggleable.

If `TransferContext` is no longer needed after removing the file-transfer panel, simplify or remove it. Keep only the dropzone overlay for future use (file drop to terminal).

**Rationale**: The toggle behavior (panel opens/closes via toolbar button) was tied to file transfer. Since file transfer is being removed, a static always-visible info panel is simpler and provides a consistent layout. The bottom panel can later host log output, session stats, or other persistent info.

## Risks / Trade-offs

- **Session list bloat**: Without a max limit, the sidebar could accumulate dozens of disconnected sessions. → Mitigation: The search/filter bar already exists; add a "Clear all disconnected" action in the sidebar header or context menu
- **State confusion**: Users may expect a disconnected session to stay "alive" somehow. → Mitigation: Clear visual distinction — the status dot is gray for disconnected, green for connected; right-click menu labels differ
- **Backend event behavior**: The Rust backend emits `session-disconnected` and may expect the frontend to handle cleanup. → Mitigation: Verify the backend does not rely on `REMOVE_TAB` for its own state; the existing `load_sessions` mechanism already handles persistence, suggesting the backend is agnostic to frontend tab state
- **i18n drift**: New UI text (context menu items, mode card labels, settings button) needs translation keys. → Mitigation: Add keys to the i18n resource files; use English fallbacks
