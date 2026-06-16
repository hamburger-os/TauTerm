## Why

After disconnecting a serial session and reconnecting (via ConnectDialog), the serial port becomes unresponsive — characters sent to the port receive no response. The only workaround is to delete the session and create a new one. Additionally, the session context menu provides a "Rename" function using `window.prompt()`, but users need a full "Configure Session" dialog that reuses the existing ConnectDialog form to edit all session parameters.

## What Changes

- **Fix serial port reconnection**: Add a brief delay or retry mechanism between closing and reopening a COM port on Windows, so the OS has time to fully release the port handle before reconnecting.
- **Zombie session cleanup**: Ensure disconnected session handles are properly removed from the backend `SessionManager` HashMap to prevent stale entries from accumulating and interfering with reconnection.
- **Replace context menu "Rename" with "Configure Session"**: Remove the `window.prompt()`-based rename item from the session context menu. Add a "Configure Session" item that opens the ConnectDialog with the session's current parameters pre-filled (port, baud rate, data bits, parity, stop bits, flow control, name), reusing the existing `editSessionId` flow but extended to support connected sessions.
- **Remove `renameTab` from context menu flow**: The rename function via context menu is removed entirely. Session renaming remains available through the "Configure Session" dialog's name field.

## Capabilities

### New Capabilities
- `serial-reconnect`: Ensure serial ports can be reliably reconnected after disconnecting without requiring session deletion and recreation.
- `session-configure`: Replace the basic rename prompt with a full session configuration dialog that reuses the ConnectDialog form, supporting parameter editing for all sessions.

### Modified Capabilities
<!-- No existing specs to modify -->

## Impact

- **Backend (Rust)**: `src-tauri/src/session/manager.rs` — `close_session()` and `create_session()` methods need port release handling; zombie handle cleanup in disconnect paths.
- **Frontend (React)**: `src/components/Layout/SessionSidebar.tsx` — context menu items and `handleMenuSelect` logic.
- **Frontend (React)**: `src/components/Layout/ConnectDialog.tsx` — may need adjustments to support editing connected sessions (currently only supports disconnected/error sessions).
- **Frontend (React)**: `src/context/SessionContext.tsx` — may need a `configureSession` function to update session params for connected sessions.
- **Frontend (React)**: `src/hooks/useContextMenu.ts` — no changes expected.
- **i18n**: `src/i18n/locales/en-US.json` and `zh-CN.json` — rename-related keys may need removal or repurposing; new "Configure Session" labels needed.
