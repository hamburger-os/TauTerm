## 1. SessionContext refactor — disconnect behavior and tab removal

- [x] 1.1 Change `session-disconnected` event handler from `REMOVE_TAB` to `SET_TAB_STATE` with state `"disconnected"`
- [x] 1.2 Review `disconnect()` function — disconnected tabs no longer auto-removed; `REMOVE_TAB` is only for explicit user deletion
- [x] 1.3 Add `deleteSession(id)` action to SessionContext that dispatches `REMOVE_TAB` (explicit delete from sidebar context menu)
- [x] 1.4 Export `deleteSession` from the context value for use by sidebar components

## 2. Remove TabBar — sidebar becomes sole session switcher

- [x] 2.1 Remove `<TabBar>` import and rendering from `App.tsx`
- [x] 2.2 Remove `TabBar.tsx` and `TabBar.module.css` files
- [x] 2.3 Remove `onNewSession` prop from TabBar usage in `App.tsx`
- [x] 2.4 Verify terminal area renders correctly without TabBar (active session terminal fills the viewport)

## 3. Toolbar simplification

- [x] 3.1 Update `TOOLBAR_BUTTONS` array in `Toolbar.tsx` to four buttons: newSession, sidebar, commands, settings
- [x] 3.2 Add `handleSettingsOpen` callback in `App.tsx` for the new settings action (show toast: "Settings coming soon")
- [x] 3.3 Remove `handleToolbarAction` cases for `refresh` and `transfer`
- [x] 3.4 Remove old translation key references (`toolbar.refresh`, `toolbar.transfer`) from toolbar logic
- [x] 3.5 Add `toolbar.settings` translation key to locale JSON files (zh-CN: "设置", en-US: "Settings")

## 4. Replace file-transfer panel with static bottom info panel

- [x] 4.1 Create new `BottomInfoPanel` component (or `StatusPanel`) at `src/components/Layout/BottomInfoPanel.tsx` with CSS module
- [x] 4.2 BottomInfoPanel displays active session info: name, connection type, endpoint, status (or placeholder text when no session)
- [x] 4.3 Remove `<FileTransferPanel>`, its resize handle, and related motion.div from `App.tsx`
- [x] 4.4 Replace with `<BottomInfoPanel>` at fixed height (200px, always visible)
- [x] 4.5 Remove `panelHeight`, `isPanelOpen`, `togglePanel`, `handlePanelMouseDown`, `handleSendFiles`, `handleReceiveFiles`, and panel resize tracking from `App.tsx` state/handlers
- [x] 4.6 Remove `PANEL_MIN`, `PANEL_DEFAULT`, `PANEL_MAX_RATIO` constants no longer needed
- [x] 4.7 Remove `transfer.toggle` keyboard shortcut registration from `App.tsx`
- [x] 4.8 Remove `handlePaletteExecute` case for `transfer.toggle`
- [x] 4.9 Clean up `FileTransferPanel.tsx`, `FileTransferPanel.module.css` files
- [x] 4.10 Evaluate `TransferContext.tsx` — if no consumers remain, remove it; otherwise simplify to keep only dropzone overlay
- [x] 4.11 Update `AppShell.tsx` if it wraps `<TransferProvider>` — remove if TransferContext is deleted

## 5. Redesign ConnectDialog — two-step mode-first flow

- [x] 5.1 Create `ModeCard` component for the mode selection grid (icon, name, description, "Coming Soon" badge for unavailable modes)
- [x] 5.2 Create `SerialConfig` component by extracting current serial fields (port, baud rate, data bits, parity, stop bits, flow control, session name) from ConnectDialog
- [x] 5.3 Create placeholder config components for SSH, Telnet, TFTP modes (labeled fields but disabled, with "Coming Soon" indicator)
- [x] 5.4 Rewrite `ConnectDialog.tsx` as two-step: render `<ModeSelector>` grid initially, then render the selected mode's `<ConfigPanel>` with a "Back" button to return to mode selection
- [x] 5.5 Maintain pre-fill behavior for restored sessions: when entering ConfigPanel, check if a disconnected tab has saved params and populate them
- [x] 5.6 Ensure Connect button is only enabled when Serial is selected AND a valid port is chosen; disabled for other modes
- [x] 5.7 Update `ConnectDialog.module.css` with styles for mode cards grid, selected state, coming-soon badge, and back-navigation

## 6. Sidebar context menu

- [x] 6.1 Create `useContextMenu` hook: manages `{ x, y, visible, targetSession }` state; closes on outside click or Escape
- [x] 6.2 Create `<ContextMenu>` component rendered via `createPortal` to document.body; styled with glassmorphism
- [x] 6.3 Wire right-click (`onContextMenu`) on sidebar session items to the context menu hook
- [x] 6.4 Implement context menu actions based on session state:
  - Disconnected: "Connect" (opens ConnectDialog with pre-filled params), "Configure" (opens ConnectDialog for editing), "Rename" (inline or prompt), "Delete" (confirm then remove)
  - Connected: "Disconnect" (closes backend connection, session stays in sidebar as disconnected), "Rename", "Delete"
- [x] 6.5 Add `disconnectSession` action to keep session in sidebar (SET_TAB_STATE → disconnected) — ensure context menu "Disconnect" uses this
- [x] 6.6 Add confirmation dialog/prompt for "Delete" action before dispatching `deleteSession`
- [x] 6.7 Create `ContextMenu.module.css` matching glassmorphism design

## 7. Remove session count limit from sidebar

- [x] 7.1 Remove the `{state.tabs.length}/10` count display from `SessionSidebar.tsx` header
- [x] 7.2 Remove any hard-cap logic that limits sessions to 10 (verify no `if (tabs.length >= 10)` guards exist)
- [x] 7.3 Update or remove the `styles.count` CSS rule if no longer needed

## 8. i18n updates

- [x] 8.1 Add new translation keys to `zh-CN.json`
- [x] 8.2 Add corresponding keys to `en-US.json`
- [x] 8.3 Remove unused translation keys: `toolbar.refresh`, `toolbar.transfer`, `shortcuts.togglePanel`, `shortcuts.nextTab`, `shortcuts.prevTab`, `transfer.*`, `layout.quickConnect`
- [x] 8.4 Update `shortcuts.newSession` and `session.emptyHint` text to remove references to removed features ("quick connect bar", tab switching)

## 9. Cleanup and verification

- [x] 9.1 Remove unused imports across all modified files (`App.tsx`, `Toolbar.tsx`, etc.)
- [x] 9.2 Remove `reorderTabs` from SessionContext if no consumer remains (TabBar was the only one)
- [x] 9.3 Run `npm run build` (or `cargo tauri build`) to verify no compilation errors
- [ ] 9.4 Manually test: open app → create serial session → verify sidebar entry → disconnect → verify entry persists → right-click → delete → verify removal
- [ ] 9.5 Manually test: toolbar shows exactly 4 buttons, each triggers correct action
- [ ] 9.6 Manually test: bottom panel always visible at 200px, no toggle mechanism
- [ ] 9.7 Manually test: ConnectDialog shows mode grid first, serial config on selection, can connect successfully
- [ ] 9.8 Verify no visual regressions in glassmorphism styling, animations, or theme support
