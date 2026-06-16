## Context

TauTerm is a Tauri + React + TypeScript serial terminal application. It manages multiple serial port sessions via a backend `SessionManager` (Rust) that owns I/O threads for each active connection. The frontend `SessionContext` (React useReducer) mirrors session state.

Two issues exist:

1. **Serial reconnect failure**: After disconnecting a serial session and reconnecting via ConnectDialog, the serial port becomes unresponsive. The backend creates a new session (new UUID) successfully, but data written to the port receives no response. Deleting the session and creating a fresh one works. Root cause analysis points to Windows COM port handle release timing — when `CloseHandle` is called (via `serialport` crate's `Drop`), the OS serial driver may not immediately release the port, causing the subsequent `CreateFile` to either fail silently or open a port in a bad state.

2. **Context menu design gap**: The session context menu provides a "Rename" option using `window.prompt()`, which is a poor UX. Users need a full "Configure Session" dialog that reuses the ConnectDialog form to edit all parameters (port, baud rate, data bits, parity, stop bits, flow control, name).

## Goals / Non-Goals

**Goals:**
- Fix serial port reconnection so disconnect → reconnect works reliably without session deletion
- Add retry logic for Windows COM port handle release timing issues
- Clean up zombie session handles in the backend after unexpected disconnects
- Replace context menu "Rename" with "Configure Session" for all sessions
- Reuse ConnectDialog for session configuration with all parameters pre-filled
- Support reconfiguring connected sessions (disconnect + reconnect with new params)

**Non-Goals:**
- Changing the session persistence format
- Adding SSH/Telnet support (still "coming soon")
- Changing the ConnectDialog visual design
- Adding inline editing of session names in the sidebar

## Decisions

### Decision 1: Retry port opening instead of delay-before-open

**Chosen**: Add retry logic in `open_port()` (Rust) — attempt to open the port up to 3 times with a 100ms sleep between attempts.

**Alternatives considered**:
- `std::thread::sleep` in `close_session()` after joining I/O thread — rejected because it adds latency to ALL disconnects, even when no immediate reconnect follows
- Platform-specific `SetCommTimeouts`/`PurgeComm` calls — rejected for complexity; retry is simpler and universally handles the race
- Moving retry to the `connect_session` command level — rejected because the retry should be at the port-open layer where the failure occurs

### Decision 2: Reconnect flow in ConnectDialog via frontend orchestration

**Chosen**: Modify `ConnectDialog.handleConnect()` to detect when `editSessionId` refers to a connected session. In that case, first call `disconnect()` on the old session, wait for the disconnect to complete, then call `connect()` with new params.

**Alternatives considered**:
- New backend command `reconfigure_session` that handles disconnect+reconnect atomically — rejected as over-engineering; frontend orchestration is simpler and the Tauri event system already handles async state sync
- Passing `existing_session_id` to `connect_session` so backend reuses the same UUID — rejected because the existing `ADD_TAB` reducer already handles replacing disconnected tabs by endpoint match

**Note**: There is a potential race between the `session-disconnected` event (which sets tab state to "disconnected") and the `session-connected` event (which replaces disconnected tabs). The `ADD_TAB` reducer matches by endpoint and state, so if the disconnect event hasn't fired yet, we'd get a duplicate tab. Mitigation: the `handleConnect` function will explicitly dispatch `SET_TAB_STATE` to "disconnected" before calling `connect()`, ensuring the `ADD_TAB` reducer finds a matching disconnected tab.

### Decision 3: Zombie session cleanup approach

**Chosen**: Add `mark_disconnected()` calls in the backend disconnect callback (already defined but never called in the unexpected disconnect path). The call is added in the `close_session()` flow — after removing the handle, the I/O thread's disconnect callback should call `mark_disconnected()` on any zombie handle that might remain.

Actually, the cleaner fix: In `close_session()`, the handle is removed from the HashMap (via `remove()`), which is correct. The zombie issue is only relevant for **unexpected** disconnects (I/O error in thread, YModem completion). For those:
- The I/O thread's `on_disconnect` callback in `connect_session` emits a Tauri event but doesn't clean up the backend handle
- The `mark_disconnected()` method exists but is never called
- Fix: In the `on_disconnect` callback closure in `connect_session`, call `manager.mark_disconnected()` before emitting the event

**Alternatives considered**:
- Always removing the handle on unexpected disconnect — rejected because the session should remain in the tab list for reconnection
- Garbage collection sweep — rejected as unnecessary; just cleaning up on each disconnect event is sufficient

### Decision 4: Context menu item restructuring

**Chosen**: Remove "Rename" from both connected and disconnected session menus. Add "Configure" to the connected session menu (it already exists for disconnected). The "Connect" item remains for disconnected sessions.

New menu structure:
- **Connected**: Disconnect | Configure | Delete
- **Disconnected**: Connect | Configure | Delete

**Alternatives considered**:
- Keeping "Rename" alongside "Configure" — rejected per user request; "Configure" subsumes rename functionality via the session name field
- Adding "Configure" but keeping separate "Rename" for quick name edits — rejected per user request

### Decision 5: Session name field behavior in Configure dialog

**Chosen**: Always pre-fill the session name field with the current session name, regardless of whether it's auto-generated (e.g., "Serial @ COM3"). Remove the current filter that skips pre-filling auto-generated names.

**Rationale**: When configuring, the user should see the current name and have the option to change it. The auto-generated name filter was a heuristic for the "new session" flow that doesn't apply to configuration.

## Risks / Trade-offs

- **Retry loop blocks caller**: The 3×100ms retry in `open_port()` blocks the calling thread for up to 300ms. This is acceptable because `connect_session` is already an async operation from the user's perspective (UI shows "Connecting..." state).
- **Frontend disconnect-then-reconnect race**: The orchestrated disconnect+reconnect in `ConnectDialog` depends on Tauri events being processed in order. Mitigation: explicitly dispatch `SET_TAB_STATE` before calling backend `connect()`, ensuring the frontend state is correct regardless of event timing.
- **`rename_session` backend command remains**: The backend `rename_session` command and frontend `renameTab` function are kept (they may be used for future inline rename or API). They just won't be exposed in the context menu. If desired, they can be removed in a follow-up cleanup.
- **`session-renamed` event listener remains**: Kept for potential future use. No harm in keeping it registered.
