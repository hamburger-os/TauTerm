## 1. Backend — Serial port retry and cleanup

- [x] 1.1 Add retry logic to `open_port()` in `src-tauri/src/session/serial.rs`: attempt to open the port up to 3 times with 100ms sleep between attempts, returning the first successful open or the last error
- [x] 1.2 Add zombie session cleanup in `connect_session` command (`src-tauri/src/commands.rs`): in the `on_disconnect` callback closure, lock the manager and call `mark_disconnected()` on the session before emitting the Tauri event, so unexpected disconnects (I/O error, device unplugged) properly mark the handle as disconnected

## 2. Frontend — Context menu restructuring

- [x] 2.1 In `src/components/Layout/SessionSidebar.tsx`, update `getMenuItems()`: remove the `{ id: "rename", ... }` item from both connected and disconnected session menus; add `{ id: "configure", ... }` to the connected session menu (it already exists for disconnected)
- [x] 2.2 In `src/components/Layout/SessionSidebar.tsx`, update `handleMenuSelect()`: remove the `case "rename"` block (lines 67-77) that uses `window.prompt()`; ensure `case "configure"` now handles both connected and disconnected sessions by calling `onEditSession?.(sessionId)`

## 3. Frontend — ConnectDialog configure and reconnect flow

- [x] 3.1 In `src/components/Layout/ConnectDialog.tsx`, update the `editSessionId` pre-fill logic (lines 87-89): always pre-fill `sessionName` with the current tab name regardless of auto-generated prefix, removing the `startsWith("Serial @")` / `startsWith("COM")` filter
- [x] 3.2 In `ConnectDialog.handleConnect()`, add logic for connected sessions: when `editSessionId` is set and the target tab state is `"connected"`, first call `disconnect(sessionId)` and explicitly dispatch `SET_TAB_STATE` to `"disconnected"` before calling `connect()` with new params — this ensures the `ADD_TAB` reducer finds and replaces the correct tab
- [x] 3.3 Update the connect button label: when `editSessionId` refers to a connected session, show "Reconnect" instead of "Connect"; when editing a disconnected session or creating new, show "Connect"

## 4. Internationalization

- [x] 4.1 In `src/i18n/locales/en-US.json`, add a `"configure"` key under `contextMenu` with value `"Configure"`; verify `"rename"` key can remain (used elsewhere or kept for future)
- [x] 4.2 In `src/i18n/locales/zh-CN.json`, add a `"configure"` key under `contextMenu` with value `"配置"`; verify `"rename"` key consistency
- [x] 4.3 Add a `"reconnect"` key under `contextMenu` (or reuse existing) with English `"Reconnect"` and Chinese `"重新连接"` for the dialog button label

## 5. Verification

- [x] 5.1 Build the project with `cargo tauri build` (or `cargo build` for backend + `npm run dev` for frontend) and verify no compilation errors
- [ ] 5.2 Manually test: create session → connect → disconnect → reconnect via "Connect" → verify serial port responds to input (e.g., press Enter, observe device response)
- [ ] 5.3 Manually test: right-click connected session → verify menu shows "Disconnect | Configure | Delete" (no "Rename")
- [ ] 5.4 Manually test: right-click disconnected session → verify menu shows "Connect | Configure | Delete" (no "Rename")
- [ ] 5.5 Manually test: "Configure" on connected session → verify dialog opens with params pre-filled → change baud rate → click "Reconnect" → verify new params take effect
