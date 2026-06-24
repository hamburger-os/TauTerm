# session-manager

## Purpose

定义会话管理器要求，包括会话的创建、关闭、切换、重命名、排序和独立 I/O 线程管理。

## Requirements

### Requirement: Create Session
The system SHALL allow creating a new session by providing a `plugin_id` and connection configuration.
The system SHALL delegate session creation to the Plugin Host, which looks up the plugin's `ProtocolAdapter` and calls `connect()`.
The system SHALL assign each session a unique identifier.
The system SHALL return the session ID and initial state.
The system SHALL store a cancel-transfer channel in the session handle for future cancellation requests.
Session implementation SHALL use the `ProtocolAdapter` trait with dynamic dispatch, rather than a concrete `SessionImpl` enum.

#### Scenario: Create serial session via plugin
- **WHEN** user connects with `plugin_id = "serial"`, endpoint = "COM3", params = {baud_rate: 115200}
- **THEN** the Plugin Host looks up the "serial" plugin's `ProtocolAdapter`, calls `connect("COM3", params)`, which returns a `Box<dyn Channel>`. The kernel starts an I/O loop for the channel, returns session ID and "connected" state.

#### Scenario: Create SSH session via plugin
- **WHEN** user connects with `plugin_id = "ssh"`, endpoint = "192.168.1.1:22", params = {auth: "password", username: "root"}
- **THEN** the Plugin Host looks up the "ssh" plugin, calls `connect()`, the SSH plugin opens a TCP connection and performs authentication, returning an `SshChannel`. The kernel starts an async I/O loop, returns session ID.

#### Scenario: Create session with unknown plugin_id
- **WHEN** user attempts to connect with `plugin_id = "nonexistent"`
- **THEN** the Plugin Host returns a `SessionError::PluginNotFound` error and does not create a session

#### Scenario: Create session with invalid parameters
- **WHEN** user attempts to connect with nonexistent port "COM99"
- **THEN** the Serial plugin's `connect()` returns `Err(SessionError::ConnectionFailed { reason: "Port COM99 not found" })` and does not create a session

### Requirement: Close Session
The system SHALL allow closing any session by its ID.
The system SHALL stop the I/O loop, call the plugin's `disconnect()` method, and release all resources.
The system SHALL emit a disconnection event.

#### Scenario: Close active session
- **WHEN** user closes the currently active tab
- **THEN** system stops the session's I/O loop, calls `plugin.disconnect()`, releases the channel, and switches to the next available tab

#### Scenario: Close last session
- **WHEN** user closes the only remaining session
- **THEN** system shows the empty state (no active tab)

### Requirement: Cancel Transfer
The system SHALL allow cancelling an in-progress file transfer for a specific session.
The system SHALL use the stored cancel channel in `SessionHandle` to signal cancellation.

#### Scenario: Cancel active transfer
- **WHEN** user clicks "Cancel Transfer" during an active YModem transfer
- **THEN** system sends signal via `SessionHandle.cancel_transfer_tx`, transfer thread receives and aborts with CAN sequence. After cancellation, the channel sender is set to `None`.

#### Scenario: Cancel when no transfer active
- **WHEN** `cancel_transfer` is called for a session with no active transfer
- **THEN** system silently returns (no-op), no error thrown

### Requirement: Switch Active Session
The system SHALL allow switching the active session to any existing session by ID.
The system SHALL focus the terminal for the newly active session.

#### Scenario: Switch between tabs
- **WHEN** user clicks a different tab in the tab bar
- **THEN** system switches active session, the previous terminal is hidden, and the new terminal is shown and focused

### Requirement: Rename Session
The system SHALL allow renaming a session to a user-provided label.
The system SHALL persist the new name in the session store.

#### Scenario: Rename a session tab
- **WHEN** user double-clicks a tab and types "Router Debug"
- **THEN** the tab label updates to "Router Debug" and persists across restart

### Requirement: Tab Reordering
The system SHALL allow reordering tabs via drag and drop.
The system SHALL maintain tab order across session switches.

#### Scenario: Drag tab to reorder
- **WHEN** user drags tab B before tab A
- **THEN** the tab order updates to [B, A, ...] and rendering reflects the new order

### Requirement: Maximum Session Limit
The system SHALL enforce a maximum of 10 concurrent sessions.
The system SHALL notify the user when the limit is reached.

#### Scenario: Exceed session limit
- **WHEN** user attempts to create an 11th session
- **THEN** system rejects the request and shows a notification

### Requirement: Independent I/O Per Session
The system SHALL run each session's I/O on an independent thread or tokio task, depending on the plugin's declared `IoStrategy`.
The system SHALL NOT allow one session's I/O failure to affect other sessions.

#### Scenario: One session disconnects
- **WHEN** the serial device for session A is physically unplugged
- **THEN** session A shows "disconnected" state, but session B (SSH) and session C (FTP) continue operating normally

#### Scenario: Async I/O session coexists with sync I/O session
- **WHEN** an SSH session (async tokio task) and a Serial session (sync std::thread) are both connected
- **THEN** both SHALL operate independently, each using its respective I/O strategy

### Requirement: I/O Statistics Collection
The system SHALL collect per-session I/O byte counts during read and write operations. Each session's I/O thread SHALL increment TX and RX counters for every successfully transmitted or received byte.

#### Scenario: Serial session transmits data
- **WHEN** a serial session sends 256 bytes of data
- **THEN** the session's TX byte counter SHALL increment by 256

#### Scenario: Serial session receives data
- **WHEN** a serial session reads 1024 bytes from the port
- **THEN** the session's RX byte counter SHALL increment by 1024

#### Scenario: I/O error does not corrupt stats
- **WHEN** a serial read returns an error (e.g., timeout)
- **THEN** the session's RX counter SHALL remain unchanged
- **AND** the session SHALL NOT crash or panic

### Requirement: Stats Event Emission
The system SHALL emit I/O statistics to the frontend at 1-second intervals via the Tauri event `session-stats`. Each event payload SHALL contain the session's tab ID, TX byte count, RX byte count, and connection timestamp.

#### Scenario: Periodic stats emission
- **WHEN** a session is connected and I/O is active
- **THEN** the system SHALL emit a `session-stats` event every 1 second containing the current TX and RX byte counts

#### Scenario: No stats emission for disconnected session
- **WHEN** a session is in "disconnected" state
- **THEN** the system SHALL NOT emit `session-stats` events for that session

#### Scenario: Stats emission stops on session close
- **WHEN** a session is closed
- **THEN** the StatsCollector SHALL be dropped, and no further `session-stats` events SHALL be emitted for that session

### Requirement: Connection Timestamp Tracking
The system SHALL record the Unix timestamp (milliseconds) when a session successfully connects. This timestamp SHALL be included in the `session-connected` event payload and persisted in the session state.

#### Scenario: Session connects successfully
- **WHEN** a serial session successfully opens COM3 and starts its I/O thread
- **THEN** the `session-connected` event payload SHALL include a `connected_at` field with the current Unix timestamp in milliseconds

#### Scenario: Reconnection updates timestamp
- **WHEN** a previously disconnected session reconnects
- **THEN** the `connected_at` timestamp SHALL be updated to the new connection time
